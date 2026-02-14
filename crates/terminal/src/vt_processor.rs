//! Dedicated VT processing thread.
//!
//! Moves escape sequence parsing off GPUI's smol executor onto a real OS thread.
//! This prevents UI freezes under high terminal output (e.g. `yes`, large compiles)
//! by ensuring VT parsing never starves GPUI's event loop.
//!
//! Signaling to the UI uses a simple `AtomicBool` render-needed flag that GPUI
//! polls via a lightweight timer, avoiding async channel dependencies.

use crate::recording::SessionRecorder;
use crate::types::ProgressState;
use alacritty_terminal::event::EventListener;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi::Processor;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Minimum interval between render signals.
/// Keep this low — the GPUI poll timer already throttles actual repaints.
const MIN_FRAME_INTERVAL: Duration = Duration::from_millis(4);

/// Timeout for blocking recv when no PTY output is available.
/// Keeps the thread responsive to shutdown signals during idle.
const IDLE_RECV_TIMEOUT: Duration = Duration::from_millis(100);

/// Initial capacity for the batch buffer (64KB covers most burst scenarios).
const BATCH_BUFFER_CAPACITY: usize = 65536;

/// Manages a dedicated OS thread for VT escape sequence processing.
///
/// On drop, signals the thread to shut down (it exits within ~100ms).
/// We intentionally don't join the thread to avoid deadlocks — the thread
/// holds `Arc<Mutex<Term>>` which may be locked by the caller during drop.
pub struct TerminalProcessor {
    shutdown: Arc<AtomicBool>,
    render_needed: Arc<AtomicBool>,
    exited_flag: Arc<AtomicBool>,
    progress: Arc<Mutex<ProgressState>>,
    recorder: Arc<Mutex<Option<SessionRecorder>>>,
}

impl TerminalProcessor {
    /// Start the VT processing thread.
    ///
    /// Generic over `L: EventListener` so the terminal crate doesn't depend on
    /// the concrete `Listener` type defined in `terminal_view`.
    ///
    /// Returns `(Self, render_needed, exited_flag)` — the caller should poll
    /// `render_needed` to know when to repaint, and `exited_flag` for process exit.
    pub fn start<L>(
        output_rx: Receiver<Vec<u8>>,
        term: Arc<Mutex<Term<L>>>,
        processor: Arc<Mutex<Processor>>,
        exited: Arc<AtomicBool>,
    ) -> Self
    where
        L: EventListener + Send + 'static,
    {
        let shutdown = Arc::new(AtomicBool::new(false));
        let render_needed = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Mutex::new(ProgressState::default()));
        let recorder = Arc::new(Mutex::new(None));

        let shutdown_clone = shutdown.clone();
        let render_needed_clone = render_needed.clone();
        let exited_clone = exited.clone();
        let progress_clone = progress.clone();
        let recorder_clone = recorder.clone();

        thread::Builder::new()
            .name("humanssh-vt-processor".into())
            .spawn(move || {
                vt_thread_loop(
                    output_rx,
                    term,
                    processor,
                    exited_clone,
                    render_needed_clone,
                    shutdown_clone,
                    progress_clone,
                    recorder_clone,
                );
            })
            .expect("failed to spawn VT processing thread");

        Self {
            shutdown,
            render_needed,
            exited_flag: exited,
            progress,
            recorder,
        }
    }

    /// Check and clear the render-needed flag (returns true if a repaint is needed).
    pub fn take_render_needed(&self) -> bool {
        self.render_needed.swap(false, Ordering::AcqRel)
    }

    /// Check if the PTY process has exited.
    pub fn has_exited(&self) -> bool {
        self.exited_flag.load(Ordering::Acquire)
    }

    /// Get the current progress bar state (set by OSC 9;4 sequences).
    pub fn progress(&self) -> ProgressState {
        *self.progress.lock()
    }

    /// Get a shared reference to the recorder slot.
    ///
    /// The caller can set or clear the recorder; the VT thread will tee
    /// output to it when present.
    pub fn recorder(&self) -> &Arc<Mutex<Option<SessionRecorder>>> {
        &self.recorder
    }
}

impl Drop for TerminalProcessor {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
    }
}

/// Main loop for the VT processing thread.
///
/// Blocks on the PTY output channel, batches all available data, parses VT sequences
/// under a brief term lock, then sets a render-needed flag (throttled to 60fps).
///
/// Also intercepts OSC 9;4 (progress bar) sequences before alacritty processes them,
/// since alacritty doesn't handle this ConEmu extension natively.
fn vt_thread_loop<L: EventListener>(
    output_rx: Receiver<Vec<u8>>,
    term: Arc<Mutex<Term<L>>>,
    processor: Arc<Mutex<Processor>>,
    exited: Arc<AtomicBool>,
    render_needed: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
    progress: Arc<Mutex<ProgressState>>,
    recorder: Arc<Mutex<Option<SessionRecorder>>>,
) {
    // Start in the past so the first batch of data always triggers a signal
    let mut last_signal = Instant::now() - MIN_FRAME_INTERVAL;
    let mut batch_buffer = Vec::with_capacity(BATCH_BUFFER_CAPACITY);

    loop {
        if shutdown.load(Ordering::Acquire) {
            break;
        }

        // Block until data arrives or timeout (keeps thread responsive to shutdown)
        match output_rx.recv_timeout(IDLE_RECV_TIMEOUT) {
            Ok(data) => {
                // Batch: drain all pending data into a single buffer
                batch_buffer.clear();
                batch_buffer.extend_from_slice(&data);
                while let Ok(more) = output_rx.try_recv() {
                    batch_buffer.extend_from_slice(&more);
                }

                // Tee output to session recorder if active
                {
                    let mut recorder_guard = recorder.lock();
                    if let Some(ref mut rec) = *recorder_guard {
                        if let Err(error) = rec.record_output(&batch_buffer) {
                            tracing::warn!("Recording error, stopping: {}", error);
                            *recorder_guard = None;
                        }
                    }
                }

                // Intercept OSC 9;4 sequences before alacritty processes them
                extract_osc9_4(&batch_buffer, &progress);

                // Parse VT sequences under brief lock
                {
                    let mut term_guard = term.lock();
                    let mut proc_guard = processor.lock();
                    proc_guard.advance(&mut *term_guard, &batch_buffer);
                }

                // Throttled render signal (60fps cap)
                let now = Instant::now();
                if now.duration_since(last_signal) >= MIN_FRAME_INTERVAL {
                    render_needed.store(true, Ordering::Release);
                    last_signal = now;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                // No data — check if process exited below
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                // PTY reader thread dropped the sender — process exited
                render_needed.store(true, Ordering::Release);
                break;
            }
        }

        // Check if PTY process has exited
        if exited.load(Ordering::Acquire) {
            render_needed.store(true, Ordering::Release);
            break;
        }
    }
}

/// Scan a byte buffer for OSC 9;4 progress bar sequences and update the shared state.
///
/// OSC 9;4 format: `ESC ] 9 ; 4 ; STATE ; PROGRESS BEL` or `ESC ] 9 ; 4 ; STATE ; PROGRESS ESC \`
/// where ESC = 0x1B, BEL = 0x07, and ST = ESC \.
///
/// We scan for the prefix `\x1b]9;4;` and extract the payload up to the terminator.
/// This is called on every batch before alacritty processes it, so it handles split
/// sequences across batches by only matching complete sequences.
fn extract_osc9_4(buffer: &[u8], progress: &Arc<Mutex<ProgressState>>) {
    const PREFIX: &[u8] = b"\x1b]9;4;";

    let mut pos = 0;
    while pos + PREFIX.len() < buffer.len() {
        if let Some(offset) = memchr_prefix(&buffer[pos..], PREFIX) {
            let start = pos + offset + PREFIX.len();
            // Find the terminator: BEL (0x07) or ST (ESC \)
            if let Some((end, payload)) = find_osc_terminator(&buffer[start..]) {
                if let Ok(payload_str) = std::str::from_utf8(payload) {
                    if let Some(new_state) = ProgressState::parse_osc9_4(payload_str) {
                        *progress.lock() = new_state;
                    }
                }
                pos = start + end;
            } else {
                // No terminator found — incomplete sequence, stop scanning
                break;
            }
        } else {
            break;
        }
    }
}

/// Find the prefix in a byte slice (simple linear scan).
fn memchr_prefix(haystack: &[u8], prefix: &[u8]) -> Option<usize> {
    haystack.windows(prefix.len()).position(|w| w == prefix)
}

/// Find the OSC string terminator (BEL or ST) and return (offset past terminator, payload slice).
fn find_osc_terminator(data: &[u8]) -> Option<(usize, &[u8])> {
    for (i, &byte) in data.iter().enumerate() {
        if byte == 0x07 {
            // BEL terminator
            return Some((i + 1, &data[..i]));
        }
        if byte == 0x1b && data.get(i + 1) == Some(&b'\\') {
            // ST (ESC \) terminator
            return Some((i + 2, &data[..i]));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TermSize;
    use alacritty_terminal::event::Event;
    use alacritty_terminal::term::Config;

    #[derive(Clone)]
    struct TestListener;
    impl EventListener for TestListener {
        fn send_event(&self, _event: Event) {}
    }

    #[test]
    fn vt_processor_processes_data_and_signals() {
        let (output_tx, output_rx) = std::sync::mpsc::sync_channel(64);
        let size = TermSize::default();
        let config = Config::default();
        let term = Arc::new(Mutex::new(Term::new(config, &size, TestListener)));
        let processor = Arc::new(Mutex::new(Processor::new()));
        let exited = Arc::new(AtomicBool::new(false));

        let vt = TerminalProcessor::start(output_rx, term.clone(), processor, exited);

        // Send some data
        output_tx.send(b"hello world".to_vec()).unwrap();

        // Wait for render signal (poll with timeout)
        let deadline = Instant::now() + Duration::from_secs(2);
        while !vt.take_render_needed() {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for render signal"
            );
            std::thread::sleep(Duration::from_millis(1));
        }

        // Verify data was parsed into the terminal
        let term_guard = term.lock();
        let grid = term_guard.grid();
        let first_char =
            grid[alacritty_terminal::index::Line(0)][alacritty_terminal::index::Column(0)].c;
        assert_eq!(first_char, 'h');

        drop(vt);
    }

    #[test]
    fn vt_processor_stops_on_pty_exit() {
        let (output_tx, output_rx) = std::sync::mpsc::sync_channel(64);
        let size = TermSize::default();
        let config = Config::default();
        let term = Arc::new(Mutex::new(Term::new(config, &size, TestListener)));
        let processor = Arc::new(Mutex::new(Processor::new()));
        let exited = Arc::new(AtomicBool::new(false));

        let vt = TerminalProcessor::start(output_rx, term, processor, exited.clone());

        // Signal exit
        exited.store(true, Ordering::Release);
        drop(output_tx);

        drop(vt);
    }

    #[test]
    fn vt_processor_stops_on_shutdown() {
        let (_output_tx, output_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(64);
        let size = TermSize::default();
        let config = Config::default();
        let term = Arc::new(Mutex::new(Term::new(config, &size, TestListener)));
        let processor = Arc::new(Mutex::new(Processor::new()));
        let exited = Arc::new(AtomicBool::new(false));

        let vt = TerminalProcessor::start(output_rx, term, processor, exited);

        drop(vt);
    }

    #[test]
    fn vt_processor_detects_osc9_4_progress() {
        let (output_tx, output_rx) = std::sync::mpsc::sync_channel(64);
        let size = TermSize::default();
        let config = Config::default();
        let term = Arc::new(Mutex::new(Term::new(config, &size, TestListener)));
        let processor = Arc::new(Mutex::new(Processor::new()));
        let exited = Arc::new(AtomicBool::new(false));

        let vt = TerminalProcessor::start(output_rx, term, processor, exited);

        // Send OSC 9;4 with BEL terminator: 50% normal progress
        output_tx.send(b"\x1b]9;4;1;50\x07".to_vec()).unwrap();

        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if vt.progress() == ProgressState::Normal(50) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "timed out waiting for progress update"
            );
            std::thread::sleep(Duration::from_millis(1));
        }

        drop(vt);
    }

    // ==================== OSC 9;4 Parsing Tests ====================

    #[test]
    fn extract_osc9_4_normal_bel() {
        let progress = Arc::new(Mutex::new(ProgressState::default()));
        extract_osc9_4(b"\x1b]9;4;1;75\x07", &progress);
        assert_eq!(*progress.lock(), ProgressState::Normal(75));
    }

    #[test]
    fn extract_osc9_4_normal_st() {
        let progress = Arc::new(Mutex::new(ProgressState::default()));
        extract_osc9_4(b"\x1b]9;4;1;42\x1b\\", &progress);
        assert_eq!(*progress.lock(), ProgressState::Normal(42));
    }

    #[test]
    fn extract_osc9_4_hidden() {
        let progress = Arc::new(Mutex::new(ProgressState::Normal(50)));
        extract_osc9_4(b"\x1b]9;4;0;0\x07", &progress);
        assert_eq!(*progress.lock(), ProgressState::Hidden);
    }

    #[test]
    fn extract_osc9_4_error() {
        let progress = Arc::new(Mutex::new(ProgressState::default()));
        extract_osc9_4(b"\x1b]9;4;2;80\x07", &progress);
        assert_eq!(*progress.lock(), ProgressState::Error(80));
    }

    #[test]
    fn extract_osc9_4_indeterminate() {
        let progress = Arc::new(Mutex::new(ProgressState::default()));
        extract_osc9_4(b"\x1b]9;4;3;0\x07", &progress);
        assert_eq!(*progress.lock(), ProgressState::Indeterminate);
    }

    #[test]
    fn extract_osc9_4_paused() {
        let progress = Arc::new(Mutex::new(ProgressState::default()));
        extract_osc9_4(b"\x1b]9;4;4;60\x07", &progress);
        assert_eq!(*progress.lock(), ProgressState::Paused(60));
    }

    #[test]
    fn extract_osc9_4_embedded_in_output() {
        let progress = Arc::new(Mutex::new(ProgressState::default()));
        let buffer = b"some text before\x1b]9;4;1;33\x07more text after";
        extract_osc9_4(buffer, &progress);
        assert_eq!(*progress.lock(), ProgressState::Normal(33));
    }

    #[test]
    fn extract_osc9_4_multiple_sequences() {
        let progress = Arc::new(Mutex::new(ProgressState::default()));
        let buffer = b"\x1b]9;4;1;25\x07\x1b]9;4;1;75\x07";
        extract_osc9_4(buffer, &progress);
        // Last one wins
        assert_eq!(*progress.lock(), ProgressState::Normal(75));
    }

    #[test]
    fn extract_osc9_4_clamps_to_100() {
        let progress = Arc::new(Mutex::new(ProgressState::default()));
        extract_osc9_4(b"\x1b]9;4;1;200\x07", &progress);
        assert_eq!(*progress.lock(), ProgressState::Normal(100));
    }

    #[test]
    fn extract_osc9_4_incomplete_ignored() {
        let progress = Arc::new(Mutex::new(ProgressState::default()));
        // No terminator — should not update
        extract_osc9_4(b"\x1b]9;4;1;50", &progress);
        assert_eq!(*progress.lock(), ProgressState::Hidden);
    }

    #[test]
    fn extract_osc9_4_invalid_state_ignored() {
        let progress = Arc::new(Mutex::new(ProgressState::default()));
        extract_osc9_4(b"\x1b]9;4;9;50\x07", &progress);
        assert_eq!(*progress.lock(), ProgressState::Hidden);
    }
}
