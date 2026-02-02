//! PTY process management.

use anyhow::{Context, Result};
use portable_pty::{native_pty_system, Child, CommandBuilder, PtyPair, PtySize};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;

/// Handles PTY spawning and I/O for terminal sessions.
///
/// Spawns a pseudo-terminal with the user's default shell and provides
/// methods for reading output and writing input. Implements `Drop` to
/// properly clean up the child process when the handler is dropped.
pub struct PtyHandler {
    pair: PtyPair,
    writer: Box<dyn Write + Send>,
    output_rx: Receiver<Vec<u8>>,
    exited: Arc<AtomicBool>,
    child: Box<dyn Child + Send + Sync>,
    _reader_thread: thread::JoinHandle<()>,
}

impl PtyHandler {
    /// Spawn a new PTY with the user's default shell.
    ///
    /// # Arguments
    /// * `rows` - Initial terminal height in rows
    /// * `cols` - Initial terminal width in columns
    ///
    /// # Returns
    /// A new `PtyHandler` on success, or an error if spawning failed.
    ///
    /// # Shell Selection
    /// Uses the `SHELL` environment variable. Falls back to `/bin/zsh` if not set.
    pub fn spawn(rows: u16, cols: u16) -> Result<Self> {
        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("Failed to open PTY")?;

        // Get the user's default shell
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());

        let mut cmd = CommandBuilder::new(&shell);
        cmd.env("TERM", "xterm-256color");

        // Spawn the shell process
        let child = pair
            .slave
            .spawn_command(cmd)
            .context("Failed to spawn shell")?;

        // Get writer for sending input to PTY
        let writer = pair
            .master
            .take_writer()
            .context("Failed to get PTY writer")?;

        // Get reader for receiving output from PTY
        let mut reader = pair
            .master
            .try_clone_reader()
            .context("Failed to get PTY reader")?;

        // Channel for output bytes
        let (output_tx, output_rx): (Sender<Vec<u8>>, Receiver<Vec<u8>>) = mpsc::channel();

        // Flag to track if process exited
        let exited = Arc::new(AtomicBool::new(false));
        let exited_clone = exited.clone();

        // Spawn thread to read PTY output
        let reader_thread = thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        // EOF - process exited
                        exited_clone.store(true, Ordering::SeqCst);
                        break;
                    }
                    Ok(n) => {
                        if output_tx.send(buf[..n].to_vec()).is_err() {
                            break; // Channel closed
                        }
                    }
                    Err(_) => {
                        exited_clone.store(true, Ordering::SeqCst);
                        break;
                    }
                }
            }
        });

        Ok(Self {
            pair,
            writer,
            output_rx,
            exited,
            child,
            _reader_thread: reader_thread,
        })
    }

    /// Write input bytes to the PTY
    pub fn write(&mut self, data: &[u8]) -> Result<()> {
        self.writer.write_all(data)?;
        self.writer.flush()?;
        Ok(())
    }

    /// Read any pending output from the PTY (non-blocking)
    pub fn read_output(&self) -> Vec<Vec<u8>> {
        let mut output = Vec::new();
        while let Ok(data) = self.output_rx.try_recv() {
            output.push(data);
        }
        output
    }

    /// Check if the PTY process has exited
    pub fn has_exited(&self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }

    /// Resize the PTY
    pub fn resize(&self, rows: u16, cols: u16) -> Result<()> {
        self.pair
            .master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("Failed to resize PTY")?;
        Ok(())
    }

    /// Check if the shell has any running child processes.
    /// Uses /proc on Linux, pgrep fallback on macOS.
    pub fn has_running_processes(&self) -> bool {
        let Some(pid) = self.child.process_id() else {
            return false;
        };

        // Try /proc first (Linux)
        #[cfg(target_os = "linux")]
        {
            if let Ok(entries) = std::fs::read_dir("/proc") {
                for entry in entries.flatten() {
                    let Ok(name) = entry.file_name().into_string() else {
                        continue;
                    };
                    // Check if it's a numeric directory (PID)
                    if name.chars().all(|c| c.is_ascii_digit()) {
                        let stat_path = entry.path().join("stat");
                        if let Ok(stat) = std::fs::read_to_string(&stat_path) {
                            // stat format: pid (comm) state ppid ...
                            // Find ppid (4th field after the closing paren)
                            if let Some(idx) = stat.rfind(')') {
                                let rest = &stat[idx + 2..]; // skip ") "
                                let fields: Vec<&str> = rest.split_whitespace().collect();
                                if fields.len() >= 2 {
                                    if let Ok(ppid) = fields[1].parse::<u32>() {
                                        if ppid == pid {
                                            return true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            return false;
        }

        // macOS/BSD fallback using pgrep
        #[cfg(not(target_os = "linux"))]
        {
            if let Ok(output) = std::process::Command::new("pgrep")
                .args(["-P", &pid.to_string()])
                .output()
            {
                return !output.stdout.is_empty();
            }
            false
        }
    }

    /// Get the name of any running foreground process (for display in confirmation).
    /// Uses /proc on Linux, ps fallback on macOS.
    pub fn get_running_process_name(&self) -> Option<String> {
        let pid = self.child.process_id()?;

        // Try /proc first (Linux)
        #[cfg(target_os = "linux")]
        {
            if let Ok(entries) = std::fs::read_dir("/proc") {
                for entry in entries.flatten() {
                    let Ok(name) = entry.file_name().into_string() else {
                        continue;
                    };
                    if name.chars().all(|c| c.is_ascii_digit()) {
                        let stat_path = entry.path().join("stat");
                        if let Ok(stat) = std::fs::read_to_string(&stat_path) {
                            if let Some(idx) = stat.rfind(')') {
                                let rest = &stat[idx + 2..];
                                let fields: Vec<&str> = rest.split_whitespace().collect();
                                if fields.len() >= 2 {
                                    if let Ok(ppid) = fields[1].parse::<u32>() {
                                        if ppid == pid {
                                            // Extract process name from (comm) in stat
                                            if let (Some(start), Some(end)) =
                                                (stat.find('('), stat.rfind(')'))
                                            {
                                                return Some(stat[start + 1..end].to_string());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            return None;
        }

        // macOS/BSD fallback
        #[cfg(not(target_os = "linux"))]
        {
            let output = std::process::Command::new("pgrep")
                .args(["-P", &pid.to_string()])
                .output()
                .ok()?;

            if output.stdout.is_empty() {
                return None;
            }

            let child_pids = String::from_utf8_lossy(&output.stdout);
            let child_pid = child_pids.lines().next()?;

            let ps_output = std::process::Command::new("ps")
                .args(["-p", child_pid, "-o", "comm="])
                .output()
                .ok()?;

            let name = String::from_utf8_lossy(&ps_output.stdout)
                .trim()
                .to_string();

            if name.is_empty() {
                None
            } else {
                Some(name)
            }
        }
    }
}

impl Drop for PtyHandler {
    fn drop(&mut self) {
        // Signal reader thread to stop by marking as exited
        self.exited.store(true, Ordering::SeqCst);

        // Kill the child process if still running
        if let Err(e) = self.child.kill() {
            // ESRCH (no such process) is expected if already exited
            tracing::debug!("Kill child process: {}", e);
        }

        // Wait for child to reap it (avoid zombie)
        if let Err(e) = self.child.wait() {
            tracing::debug!("Wait for child process: {}", e);
        }

        tracing::debug!("PTY handler dropped, child process cleaned up");
    }
}
