//! Dedicated VT processing thread.
//!
//! Moves escape sequence parsing off GPUI's smol executor onto a real OS thread.
//! This prevents UI freezes under high terminal output (e.g. `yes`, large compiles)
//! by ensuring VT parsing never starves GPUI's event loop.
//!
//! Signaling to the UI uses a simple `AtomicBool` render-needed flag that GPUI
//! polls via a lightweight timer, avoiding async channel dependencies.

use alacritty_terminal::event::EventListener;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi::Processor;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

/// Minimum interval between render signals (16ms = 60fps cap).
const MIN_FRAME_INTERVAL: Duration = Duration::from_millis(16);

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

        let shutdown_clone = shutdown.clone();
        let render_needed_clone = render_needed.clone();
        let exited_clone = exited.clone();

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
                );
            })
            .expect("failed to spawn VT processing thread");

        Self {
            shutdown,
            render_needed,
            exited_flag: exited,
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
fn vt_thread_loop<L: EventListener>(
    output_rx: Receiver<Vec<u8>>,
    term: Arc<Mutex<Term<L>>>,
    processor: Arc<Mutex<Processor>>,
    exited: Arc<AtomicBool>,
    render_needed: Arc<AtomicBool>,
    shutdown: Arc<AtomicBool>,
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
}
