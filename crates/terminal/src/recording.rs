//! Terminal session recording in asciinema v2 format.
//!
//! Records timestamped PTY output to `.cast` files that can be replayed with
//! `asciinema play` or any compatible player.

use anyhow::{Context, Result};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Records terminal session output to an asciinema v2 .cast file.
pub struct SessionRecorder {
    writer: BufWriter<std::fs::File>,
    start_time: Instant,
    active: bool,
    path: PathBuf,
}

impl SessionRecorder {
    /// Create a new recorder writing to the default recordings directory.
    ///
    /// Files are stored in `<data-dir>/humanssh/recordings/<timestamp>.cast`.
    pub fn new(width: u16, height: u16) -> Result<Self> {
        let directory = dirs::data_dir()
            .context("Could not determine data directory")?
            .join("humanssh")
            .join("recordings");
        std::fs::create_dir_all(&directory)
            .with_context(|| format!("Failed to create recordings directory: {:?}", directory))?;

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        let filename = format!("{}.cast", timestamp);
        let path = directory.join(&filename);

        Self::new_with_path(path, width, height)
    }

    /// Create a recorder writing to a specific file path.
    pub fn new_with_path(path: PathBuf, width: u16, height: u16) -> Result<Self> {
        let file = std::fs::File::create(&path)
            .with_context(|| format!("Failed to create recording file: {:?}", path))?;
        let mut writer = BufWriter::new(file);

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);

        let header = serde_json::json!({
            "version": 2,
            "width": width,
            "height": height,
            "timestamp": timestamp,
            "env": {
                "TERM": "xterm-256color",
                "SHELL": std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string())
            }
        });
        writeln!(writer, "{}", header).context("Failed to write recording header")?;

        tracing::info!("Started recording to {:?}", path);

        Ok(Self {
            writer,
            start_time: Instant::now(),
            active: true,
            path,
        })
    }

    /// Record an output event (data sent from PTY to terminal).
    pub fn record_output(&mut self, data: &[u8]) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        let elapsed = self.start_time.elapsed().as_secs_f64();
        let text = String::from_utf8_lossy(data);
        let event = serde_json::json!([elapsed, "o", text]);
        writeln!(self.writer, "{}", event).context("Failed to write recording event")?;
        Ok(())
    }

    /// Stop recording and flush the file.
    pub fn finish(&mut self) -> Result<()> {
        if !self.active {
            return Ok(());
        }
        self.active = false;
        self.writer.flush().context("Failed to flush recording")?;
        let elapsed = self.start_time.elapsed();
        tracing::info!(
            path = %self.path.display(),
            duration_secs = elapsed.as_secs_f64(),
            "Recording finished"
        );
        Ok(())
    }

    /// Whether the recorder is actively recording.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Get the recording file path.
    pub fn recording_path(&self) -> &Path {
        &self.path
    }
}

/// A single event parsed from an asciinema v2 .cast file.
#[derive(Clone, Debug)]
pub struct ReplayEvent {
    /// Elapsed time in seconds from start of recording.
    pub timestamp: f64,
    /// The output data for this event.
    pub data: Vec<u8>,
}

/// Header from an asciinema v2 .cast file.
#[derive(Clone, Debug)]
pub struct ReplayHeader {
    pub width: u16,
    pub height: u16,
}

/// Parse an asciinema v2 .cast file into header and events.
///
/// Only output events ("o") are included; input events ("i") are skipped.
pub fn parse_cast_file(path: &Path) -> Result<(ReplayHeader, Vec<ReplayEvent>)> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read cast file: {:?}", path))?;

    let mut lines = content.lines();
    let header_line = lines.next().context("Cast file is empty")?;
    let header: serde_json::Value =
        serde_json::from_str(header_line).context("Invalid cast file header")?;

    let width = header["width"]
        .as_u64()
        .context("Missing width in header")? as u16;
    let height = header["height"]
        .as_u64()
        .context("Missing height in header")? as u16;

    let mut events = Vec::new();
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let event: serde_json::Value =
            serde_json::from_str(line).with_context(|| format!("Invalid event line: {}", line))?;
        let arr = event.as_array().context("Event is not an array")?;
        if arr.len() < 3 {
            continue;
        }
        let timestamp = arr[0].as_f64().context("Invalid timestamp")?;
        let event_type = arr[1].as_str().unwrap_or("");
        if event_type != "o" {
            continue;
        }
        let data = arr[2].as_str().unwrap_or("").as_bytes().to_vec();
        events.push(ReplayEvent { timestamp, data });
    }

    Ok((ReplayHeader { width, height }, events))
}

/// Get the default recordings directory.
pub fn recordings_directory() -> Result<PathBuf> {
    let directory = dirs::data_dir()
        .context("Could not determine data directory")?
        .join("humanssh")
        .join("recordings");
    Ok(directory)
}

impl Drop for SessionRecorder {
    fn drop(&mut self) {
        if self.active {
            if let Err(error) = self.finish() {
                tracing::warn!("Failed to finish recording on drop: {}", error);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_recorder(
        width: u16,
        height: u16,
    ) -> Result<(SessionRecorder, PathBuf, tempfile::TempDir)> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("test.cast");
        let recorder = SessionRecorder::new_with_path(path.clone(), width, height)?;
        Ok((recorder, path, directory))
    }

    #[test]
    fn recorder_creates_file() {
        let (recorder, path, _dir) = temp_recorder(80, 24).expect("should create recorder");
        assert!(recorder.is_active());
        assert!(path.exists());
    }

    #[test]
    fn recorder_writes_valid_header() {
        let (recorder, path, _dir) = temp_recorder(80, 24).expect("should create recorder");
        drop(recorder);

        let content = std::fs::read_to_string(&path).expect("should read file");
        let lines: Vec<&str> = content.lines().collect();
        assert!(!lines.is_empty(), "file should have at least header line");

        let header: serde_json::Value =
            serde_json::from_str(lines[0]).expect("header should be valid JSON");
        assert_eq!(header["version"], 2);
        assert_eq!(header["width"], 80);
        assert_eq!(header["height"], 24);
    }

    #[test]
    fn recorder_records_output_event() {
        let (mut recorder, path, _dir) = temp_recorder(80, 24).expect("should create recorder");
        recorder
            .record_output(b"hello world")
            .expect("should record");
        recorder.finish().expect("should finish");

        let content = std::fs::read_to_string(&path).expect("should read file");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2, "should have header + 1 event");

        let event: serde_json::Value =
            serde_json::from_str(lines[1]).expect("event should be valid JSON");
        assert_eq!(event[1], "o");
        assert_eq!(event[2], "hello world");
        // Timestamp should be a non-negative number
        assert!(event[0].as_f64().is_some_and(|t| t >= 0.0));
    }

    #[test]
    fn finish_stops_recording() {
        let (mut recorder, _path, _dir) = temp_recorder(80, 24).expect("should create recorder");
        assert!(recorder.is_active());
        recorder.finish().expect("should finish");
        assert!(!recorder.is_active());
        // Further writes should be no-ops
        recorder
            .record_output(b"ignored")
            .expect("should not error");
    }

    #[test]
    fn double_finish_is_safe() {
        let (mut recorder, _path, _dir) = temp_recorder(80, 24).expect("should create recorder");
        recorder.finish().expect("first finish");
        recorder.finish().expect("second finish should be no-op");
    }

    #[test]
    fn recording_path_returns_correct_path() {
        let (recorder, path, _dir) = temp_recorder(80, 24).expect("should create recorder");
        assert_eq!(recorder.recording_path(), path.as_path());
    }

    #[test]
    fn multiple_events_have_increasing_timestamps() {
        let (mut recorder, path, _dir) = temp_recorder(80, 24).expect("should create recorder");
        recorder.record_output(b"first").expect("should record");
        std::thread::sleep(std::time::Duration::from_millis(10));
        recorder.record_output(b"second").expect("should record");
        recorder.finish().expect("should finish");

        let content = std::fs::read_to_string(&path).expect("should read file");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 3, "should have header + 2 events");

        let event1: serde_json::Value = serde_json::from_str(lines[1]).expect("valid JSON");
        let event2: serde_json::Value = serde_json::from_str(lines[2]).expect("valid JSON");
        let t1 = event1[0].as_f64().expect("timestamp");
        let t2 = event2[0].as_f64().expect("timestamp");
        assert!(t2 > t1, "second event should have later timestamp");
    }

    #[test]
    fn parse_roundtrip() {
        let (mut recorder, path, _dir) = temp_recorder(80, 24).expect("should create recorder");
        recorder.record_output(b"hello").expect("should record");
        recorder.record_output(b"world").expect("should record");
        recorder.finish().expect("should finish");

        let (header, events) = parse_cast_file(&path).expect("should parse");
        assert_eq!(header.width, 80);
        assert_eq!(header.height, 24);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].data, b"hello");
        assert_eq!(events[1].data, b"world");
        assert!(events[1].timestamp >= events[0].timestamp);
    }

    #[test]
    fn parse_skips_input_events() {
        let dir = tempfile::tempdir().expect("should create tempdir");
        let path = dir.path().join("test.cast");
        let content = r#"{"version":2,"width":80,"height":24,"timestamp":0}
[0.1, "o", "output"]
[0.2, "i", "input"]
[0.3, "o", "more output"]"#;
        std::fs::write(&path, content).expect("should write");

        let (_, events) = parse_cast_file(&path).expect("should parse");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].data, b"output");
        assert_eq!(events[1].data, b"more output");
    }

    #[test]
    fn parse_empty_events() {
        let dir = tempfile::tempdir().expect("should create tempdir");
        let path = dir.path().join("test.cast");
        let content = r#"{"version":2,"width":120,"height":40,"timestamp":0}"#;
        std::fs::write(&path, content).expect("should write");

        let (header, events) = parse_cast_file(&path).expect("should parse");
        assert_eq!(header.width, 120);
        assert_eq!(header.height, 40);
        assert!(events.is_empty());
    }
}
