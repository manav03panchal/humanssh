//! Test utilities for HumanSSH.
//!
//! This module provides common test fixtures, helpers, and mock structures
//! for testing terminal functionality. Only compiled when running tests.
//!
//! # Overview
//!
//! - `TestTerminal` - Simulates terminal interactions without a real PTY
//! - `MockPty` - Mock PTY for testing I/O operations
//! - `TestFixtures` - Common test data and configurations
//! - Async helpers for PTY operations
//! - Helper macros for common test patterns

#![cfg(test)]

use crate::terminal::types::{DisplayState, TermSize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;

// Re-export commonly used test dependencies
pub use std::time::Duration;

/// Default timeout for async test operations
pub const DEFAULT_TEST_TIMEOUT: Duration = Duration::from_secs(5);

/// Short timeout for operations expected to complete quickly
pub const SHORT_TIMEOUT: Duration = Duration::from_millis(500);

// ============================================================================
// Test Terminal
// ============================================================================

/// A simulated terminal for testing without spawning real PTY processes.
///
/// This struct allows testing terminal logic, escape sequence parsing,
/// and rendering without the complexity of actual PTY management.
///
/// # Example
///
/// ```ignore
/// let mut term = TestTerminal::new(80, 24);
/// term.feed_input("Hello, World!\r\n");
/// assert!(term.output_contains("Hello"));
/// ```
pub struct TestTerminal {
    /// Terminal dimensions
    pub size: TermSize,
    /// Display state for rendering tests
    pub display: DisplayState,
    /// Accumulated output buffer
    output_buffer: String,
    /// Input queue (simulated keyboard input)
    input_queue: VecDeque<String>,
    /// Whether the terminal has "exited"
    exited: bool,
    /// Simulated cursor position (col, row)
    cursor_pos: (usize, usize),
}

impl TestTerminal {
    /// Create a new test terminal with the given dimensions.
    pub fn new(cols: u16, rows: u16) -> Self {
        let size = TermSize { cols, rows };
        Self {
            size,
            display: DisplayState {
                size,
                cell_dims: (8.4, 17.0),
                bounds: None,
                font_size: 14.0,
            },
            output_buffer: String::new(),
            input_queue: VecDeque::new(),
            exited: false,
            cursor_pos: (0, 0),
        }
    }

    /// Create a test terminal with default dimensions (80x24).
    pub fn default_size() -> Self {
        Self::new(80, 24)
    }

    /// Feed input as if typed by a user.
    pub fn feed_input(&mut self, input: &str) {
        self.input_queue.push_back(input.to_string());
        // Simulate echo by adding to output
        self.output_buffer.push_str(input);
    }

    /// Feed raw bytes (for escape sequences).
    pub fn feed_bytes(&mut self, bytes: &[u8]) {
        if let Ok(s) = std::str::from_utf8(bytes) {
            self.output_buffer.push_str(s);
        }
    }

    /// Get all pending input.
    pub fn drain_input(&mut self) -> Vec<String> {
        self.input_queue.drain(..).collect()
    }

    /// Check if output contains a specific string.
    pub fn output_contains(&self, needle: &str) -> bool {
        self.output_buffer.contains(needle)
    }

    /// Get the full output buffer.
    pub fn get_output(&self) -> &str {
        &self.output_buffer
    }

    /// Clear the output buffer.
    pub fn clear_output(&mut self) {
        self.output_buffer.clear();
    }

    /// Simulate terminal resize.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.size = TermSize { cols, rows };
        self.display.size = self.size;
    }

    /// Mark the terminal as exited.
    pub fn set_exited(&mut self, exited: bool) {
        self.exited = exited;
    }

    /// Check if terminal has exited.
    pub fn has_exited(&self) -> bool {
        self.exited
    }

    /// Get current cursor position.
    pub fn cursor_position(&self) -> (usize, usize) {
        self.cursor_pos
    }

    /// Set cursor position (for testing cursor movement).
    pub fn set_cursor(&mut self, col: usize, row: usize) {
        self.cursor_pos = (
            col.min(self.size.cols as usize - 1),
            row.min(self.size.rows as usize - 1),
        );
    }

    /// Simulate writing output (as if from a shell).
    pub fn write_output(&mut self, output: &str) {
        self.output_buffer.push_str(output);
    }
}

impl Default for TestTerminal {
    fn default() -> Self {
        Self::default_size()
    }
}

// ============================================================================
// Mock PTY
// ============================================================================

/// A mock PTY for testing I/O operations without spawning real processes.
///
/// Useful for testing:
/// - Input/output handling
/// - Backpressure behavior
/// - Error conditions
/// - Exit detection
pub struct MockPty {
    /// Data written to the PTY (input from "user")
    pub written_data: Arc<TokioMutex<Vec<u8>>>,
    /// Data to be read from the PTY (output from "shell")
    pub read_queue: Arc<TokioMutex<VecDeque<Vec<u8>>>>,
    /// Whether the PTY has exited
    pub exited: Arc<AtomicBool>,
    /// Simulated process ID
    pub pid: u32,
}

impl MockPty {
    /// Create a new mock PTY.
    pub fn new() -> Self {
        Self {
            written_data: Arc::new(TokioMutex::new(Vec::new())),
            read_queue: Arc::new(TokioMutex::new(VecDeque::new())),
            exited: Arc::new(AtomicBool::new(false)),
            pid: 12345,
        }
    }

    /// Queue data to be read (simulates shell output).
    pub async fn queue_output(&self, data: &[u8]) {
        self.read_queue.lock().await.push_back(data.to_vec());
    }

    /// Queue string output (convenience method).
    pub async fn queue_str(&self, s: &str) {
        self.queue_output(s.as_bytes()).await;
    }

    /// Write data to the PTY (simulates user input).
    pub async fn write(&self, data: &[u8]) -> std::io::Result<()> {
        if self.exited.load(Ordering::SeqCst) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "PTY has exited",
            ));
        }
        self.written_data.lock().await.extend_from_slice(data);
        Ok(())
    }

    /// Read pending output (non-blocking).
    pub async fn read_output(&self) -> Option<Vec<u8>> {
        self.read_queue.lock().await.pop_front()
    }

    /// Get all written data (for verification).
    pub async fn get_written_data(&self) -> Vec<u8> {
        self.written_data.lock().await.clone()
    }

    /// Clear written data.
    pub async fn clear_written(&self) {
        self.written_data.lock().await.clear();
    }

    /// Mark the PTY as exited.
    pub fn set_exited(&self) {
        self.exited.store(true, Ordering::SeqCst);
    }

    /// Check if PTY has exited.
    pub fn has_exited(&self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }

    /// Get the simulated process ID.
    pub fn process_id(&self) -> u32 {
        self.pid
    }
}

impl Default for MockPty {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Test Fixtures
// ============================================================================

/// Common test fixtures and data.
pub struct TestFixtures;

impl TestFixtures {
    /// ANSI escape sequence for clearing the screen.
    pub const CLEAR_SCREEN: &'static str = "\x1b[2J";

    /// ANSI escape sequence for moving cursor to home.
    pub const CURSOR_HOME: &'static str = "\x1b[H";

    /// ANSI reset sequence.
    pub const RESET: &'static str = "\x1b[0m";

    /// Bold text sequence.
    pub const BOLD: &'static str = "\x1b[1m";

    /// Red foreground color.
    pub const RED_FG: &'static str = "\x1b[31m";

    /// Green foreground color.
    pub const GREEN_FG: &'static str = "\x1b[32m";

    /// Sample shell prompt.
    pub const SHELL_PROMPT: &'static str = "user@host:~$ ";

    /// Sample command output.
    pub const SAMPLE_LS_OUTPUT: &'static str = "file1.txt  file2.txt  directory/\r\n";

    /// Create a cursor position escape sequence.
    pub fn cursor_to(row: usize, col: usize) -> String {
        format!("\x1b[{};{}H", row + 1, col + 1)
    }

    /// Create a colored text sequence.
    pub fn colored_text(text: &str, color_code: u8) -> String {
        format!("\x1b[{}m{}\x1b[0m", color_code, text)
    }

    /// Create SGR mouse event sequence.
    pub fn sgr_mouse_click(button: u8, col: usize, row: usize, pressed: bool) -> String {
        let suffix = if pressed { 'M' } else { 'm' };
        format!("\x1b[<{};{};{}{}", button, col + 1, row + 1, suffix)
    }

    /// Create a terminal size for testing.
    pub fn term_size(cols: u16, rows: u16) -> TermSize {
        TermSize { cols, rows }
    }

    /// Standard 80x24 terminal size.
    pub fn standard_size() -> TermSize {
        TermSize { cols: 80, rows: 24 }
    }

    /// Wide terminal (for testing horizontal layouts).
    pub fn wide_size() -> TermSize {
        TermSize {
            cols: 200,
            rows: 24,
        }
    }

    /// Tall terminal (for testing vertical layouts).
    pub fn tall_size() -> TermSize {
        TermSize { cols: 80, rows: 60 }
    }

    /// Minimum viable terminal size.
    pub fn minimum_size() -> TermSize {
        TermSize { cols: 10, rows: 3 }
    }
}

// ============================================================================
// Async Test Helpers
// ============================================================================

/// Wait for a condition with timeout.
///
/// # Example
///
/// ```ignore
/// wait_for(|| mock_pty.has_exited(), SHORT_TIMEOUT).await?;
/// ```
pub async fn wait_for<F>(condition: F, timeout: Duration) -> Result<(), &'static str>
where
    F: Fn() -> bool,
{
    let start = std::time::Instant::now();
    while !condition() {
        if start.elapsed() > timeout {
            return Err("Timeout waiting for condition");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Ok(())
}

/// Wait for async condition with timeout.
pub async fn wait_for_async<F, Fut>(condition: F, timeout: Duration) -> Result<(), &'static str>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let start = std::time::Instant::now();
    loop {
        if condition().await {
            return Ok(());
        }
        if start.elapsed() > timeout {
            return Err("Timeout waiting for async condition");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Run an async test with a timeout.
///
/// Wraps tokio::time::timeout with better error handling.
pub async fn with_timeout<F, T>(timeout: Duration, future: F) -> Result<T, &'static str>
where
    F: std::future::Future<Output = T>,
{
    tokio::time::timeout(timeout, future)
        .await
        .map_err(|_| "Test timed out")
}

// ============================================================================
// Helper Macros
// ============================================================================

/// Assert that a result is Ok and return the value.
///
/// # Example
///
/// ```ignore
/// let value = assert_ok!(some_result);
/// ```
#[macro_export]
macro_rules! assert_ok {
    ($expr:expr) => {
        match $expr {
            Ok(v) => v,
            Err(e) => panic!("Expected Ok, got Err: {:?}", e),
        }
    };
    ($expr:expr, $msg:expr) => {
        match $expr {
            Ok(v) => v,
            Err(e) => panic!("{}: {:?}", $msg, e),
        }
    };
}

/// Assert that a result is Err.
///
/// # Example
///
/// ```ignore
/// assert_err!(fallible_operation());
/// ```
#[macro_export]
macro_rules! assert_err {
    ($expr:expr) => {
        match $expr {
            Ok(v) => panic!("Expected Err, got Ok: {:?}", v),
            Err(_) => {}
        }
    };
}

/// Assert that an Option is Some and return the value.
#[macro_export]
macro_rules! assert_some {
    ($expr:expr) => {
        match $expr {
            Some(v) => v,
            None => panic!("Expected Some, got None"),
        }
    };
}

/// Assert that an Option is None.
#[macro_export]
macro_rules! assert_none {
    ($expr:expr) => {
        if $expr.is_some() {
            panic!("Expected None, got Some");
        }
    };
}

/// Create a test terminal with the given dimensions.
///
/// # Example
///
/// ```ignore
/// let term = test_terminal!(80, 24);
/// // or with default size
/// let term = test_terminal!();
/// ```
#[macro_export]
macro_rules! test_terminal {
    () => {
        $crate::test_utils::TestTerminal::default_size()
    };
    ($cols:expr, $rows:expr) => {
        $crate::test_utils::TestTerminal::new($cols, $rows)
    };
}

// ============================================================================
// Tests for test utilities
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_terminal_creation() {
        let term = TestTerminal::new(100, 50);
        assert_eq!(term.size.cols, 100);
        assert_eq!(term.size.rows, 50);
        assert!(!term.has_exited());
    }

    #[test]
    fn test_terminal_default() {
        let term = TestTerminal::default_size();
        assert_eq!(term.size.cols, 80);
        assert_eq!(term.size.rows, 24);
    }

    #[test]
    fn test_terminal_input_output() {
        let mut term = TestTerminal::default();
        term.feed_input("hello");
        assert!(term.output_contains("hello"));

        let inputs = term.drain_input();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0], "hello");
    }

    #[test]
    fn test_terminal_resize() {
        let mut term = TestTerminal::default();
        term.resize(120, 40);
        assert_eq!(term.size.cols, 120);
        assert_eq!(term.size.rows, 40);
    }

    #[test]
    fn test_terminal_cursor() {
        let mut term = TestTerminal::new(80, 24);
        term.set_cursor(10, 5);
        assert_eq!(term.cursor_position(), (10, 5));

        // Test clamping
        term.set_cursor(100, 100);
        assert_eq!(term.cursor_position(), (79, 23));
    }

    #[tokio::test]
    async fn test_mock_pty() {
        let pty = MockPty::new();
        assert!(!pty.has_exited());

        // Test write
        pty.write(b"hello").await.unwrap();
        assert_eq!(pty.get_written_data().await, b"hello");

        // Test read queue
        pty.queue_str("output").await;
        let output = pty.read_output().await;
        assert_eq!(output, Some(b"output".to_vec()));

        // Test exit
        pty.set_exited();
        assert!(pty.has_exited());
        assert!(pty.write(b"fail").await.is_err());
    }

    #[test]
    fn test_fixtures() {
        assert_eq!(TestFixtures::cursor_to(0, 0), "\x1b[1;1H");
        assert_eq!(TestFixtures::cursor_to(5, 10), "\x1b[6;11H");

        let colored = TestFixtures::colored_text("test", 31);
        assert!(colored.starts_with("\x1b[31m"));
        assert!(colored.ends_with("\x1b[0m"));

        let mouse = TestFixtures::sgr_mouse_click(0, 10, 5, true);
        assert_eq!(mouse, "\x1b[<0;11;6M");
    }

    #[test]
    fn test_term_sizes() {
        let std = TestFixtures::standard_size();
        assert_eq!(std.cols, 80);
        assert_eq!(std.rows, 24);

        let min = TestFixtures::minimum_size();
        assert_eq!(min.cols, 10);
        assert_eq!(min.rows, 3);
    }

    #[tokio::test]
    async fn test_wait_for() {
        let flag = Arc::new(AtomicBool::new(false));
        let flag_clone = flag.clone();

        // Spawn a task to set the flag
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            flag_clone.store(true, Ordering::SeqCst);
        });

        let result = wait_for(|| flag.load(Ordering::SeqCst), SHORT_TIMEOUT).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wait_for_timeout() {
        let result = wait_for(|| false, Duration::from_millis(50)).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_with_timeout_success() {
        let result = with_timeout(SHORT_TIMEOUT, async { 42 }).await;
        assert_eq!(result, Ok(42));
    }

    #[tokio::test]
    async fn test_with_timeout_failure() {
        let result = with_timeout(Duration::from_millis(10), async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            42
        })
        .await;
        assert!(result.is_err());
    }
}
