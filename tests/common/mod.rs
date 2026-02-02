//! Shared test utilities for integration tests.
//!
//! This module provides common helpers, fixtures, and mock structures
//! that are shared across integration tests.

// Allow unused items - they are available for future tests
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

// Re-exports for convenience
pub use tempfile::{tempdir, TempDir};

/// Default timeout for async test operations
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// Short timeout for fast operations
pub const SHORT_TIMEOUT: Duration = Duration::from_millis(500);

// ============================================================================
// Test Environment Setup
// ============================================================================

/// Test environment that manages temporary directories and cleanup.
///
/// Automatically cleans up resources when dropped.
pub struct TestEnv {
    /// Temporary directory for test files
    pub temp_dir: TempDir,
    /// Path to a mock config directory
    pub config_dir: PathBuf,
    /// Path to mock settings file
    pub settings_path: PathBuf,
}

impl TestEnv {
    /// Create a new test environment with isolated paths.
    pub fn new() -> Self {
        let temp_dir = tempdir().expect("Failed to create temp directory");
        let config_dir = temp_dir.path().join("humanssh");
        std::fs::create_dir_all(&config_dir).expect("Failed to create mock config dir");
        let settings_path = config_dir.join("settings.json");

        Self {
            temp_dir,
            config_dir,
            settings_path,
        }
    }

    /// Get the base temp directory path.
    pub fn path(&self) -> &std::path::Path {
        self.temp_dir.path()
    }

    /// Write mock settings to the test environment.
    pub fn write_settings(&self, content: &str) {
        std::fs::write(&self.settings_path, content).expect("Failed to write settings");
    }

    /// Read settings from the test environment.
    pub fn read_settings(&self) -> Option<String> {
        std::fs::read_to_string(&self.settings_path).ok()
    }

    /// Create a mock theme file.
    pub fn create_mock_theme(&self, name: &str, content: &str) -> PathBuf {
        let themes_dir = self.temp_dir.path().join("themes");
        std::fs::create_dir_all(&themes_dir).expect("Failed to create themes dir");
        let theme_path = themes_dir.join(format!("{}.json", name));
        std::fs::write(&theme_path, content).expect("Failed to write theme");
        theme_path
    }
}

impl Default for TestEnv {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Mock Settings
// ============================================================================

/// Mock settings for testing configuration loading/saving.
#[derive(Debug, Clone)]
pub struct MockSettings {
    pub theme: Option<String>,
    pub font_family: Option<String>,
    pub window_x: Option<f32>,
    pub window_y: Option<f32>,
    pub window_width: Option<f32>,
    pub window_height: Option<f32>,
}

impl MockSettings {
    /// Create empty settings.
    pub fn empty() -> Self {
        Self {
            theme: None,
            font_family: None,
            window_x: None,
            window_y: None,
            window_width: None,
            window_height: None,
        }
    }

    /// Create settings with a theme.
    pub fn with_theme(theme: &str) -> Self {
        Self {
            theme: Some(theme.to_string()),
            ..Self::empty()
        }
    }

    /// Create settings with window bounds.
    pub fn with_window_bounds(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            window_x: Some(x),
            window_y: Some(y),
            window_width: Some(width),
            window_height: Some(height),
            ..Self::empty()
        }
    }

    /// Convert to JSON string.
    pub fn to_json(&self) -> String {
        let mut parts = Vec::new();

        if let Some(ref theme) = self.theme {
            parts.push(format!(r#""theme": "{}""#, theme));
        }
        if let Some(ref font) = self.font_family {
            parts.push(format!(r#""font_family": "{}""#, font));
        }

        if self.window_x.is_some() {
            let bounds = format!(
                r#""window_bounds": {{ "x": {}, "y": {}, "width": {}, "height": {} }}"#,
                self.window_x.unwrap_or(100.0),
                self.window_y.unwrap_or(100.0),
                self.window_width.unwrap_or(1200.0),
                self.window_height.unwrap_or(800.0)
            );
            parts.push(bounds);
        }

        format!("{{ {} }}", parts.join(", "))
    }
}

// ============================================================================
// Mock Terminal Session
// ============================================================================

/// A mock terminal session for testing without spawning real processes.
///
/// Simulates terminal behavior including:
/// - Input/output buffering
/// - Exit status tracking
/// - Resize handling
pub struct MockTerminalSession {
    /// Session ID for identification
    pub id: uuid::Uuid,
    /// Terminal dimensions (cols, rows)
    pub size: (u16, u16),
    /// Output buffer
    output: Arc<std::sync::Mutex<String>>,
    /// Input buffer (what was "typed")
    input: Arc<std::sync::Mutex<String>>,
    /// Whether the session has exited
    exited: Arc<AtomicBool>,
    /// Mock title
    title: Arc<std::sync::Mutex<Option<String>>>,
}

impl MockTerminalSession {
    /// Create a new mock terminal session.
    pub fn new() -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            size: (80, 24),
            output: Arc::new(std::sync::Mutex::new(String::new())),
            input: Arc::new(std::sync::Mutex::new(String::new())),
            exited: Arc::new(AtomicBool::new(false)),
            title: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Create a session with specific dimensions.
    pub fn with_size(cols: u16, rows: u16) -> Self {
        let mut session = Self::new();
        session.size = (cols, rows);
        session
    }

    /// Simulate sending input to the terminal.
    pub fn send_input(&self, input: &str) {
        let mut buf = self.input.lock().unwrap();
        buf.push_str(input);
    }

    /// Get all input that was sent.
    pub fn get_input(&self) -> String {
        self.input.lock().unwrap().clone()
    }

    /// Clear the input buffer.
    pub fn clear_input(&self) {
        self.input.lock().unwrap().clear();
    }

    /// Simulate output from the shell.
    pub fn write_output(&self, output: &str) {
        let mut buf = self.output.lock().unwrap();
        buf.push_str(output);
    }

    /// Get all accumulated output.
    pub fn get_output(&self) -> String {
        self.output.lock().unwrap().clone()
    }

    /// Clear the output buffer.
    pub fn clear_output(&self) {
        self.output.lock().unwrap().clear();
    }

    /// Check if output contains a string.
    pub fn output_contains(&self, needle: &str) -> bool {
        self.output.lock().unwrap().contains(needle)
    }

    /// Simulate resizing the terminal.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.size = (cols, rows);
    }

    /// Set the terminal title.
    pub fn set_title(&self, title: &str) {
        *self.title.lock().unwrap() = Some(title.to_string());
    }

    /// Get the terminal title.
    pub fn get_title(&self) -> Option<String> {
        self.title.lock().unwrap().clone()
    }

    /// Mark the session as exited.
    pub fn exit(&self) {
        self.exited.store(true, Ordering::SeqCst);
    }

    /// Check if the session has exited.
    pub fn has_exited(&self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }

    /// Check if the session is still running.
    pub fn is_running(&self) -> bool {
        !self.has_exited()
    }
}

impl Default for MockTerminalSession {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Test Fixtures
// ============================================================================

/// Common test data and constants.
pub struct Fixtures;

impl Fixtures {
    // ANSI escape sequences
    pub const CLEAR_SCREEN: &'static str = "\x1b[2J";
    pub const CURSOR_HOME: &'static str = "\x1b[H";
    pub const RESET: &'static str = "\x1b[0m";
    pub const BOLD: &'static str = "\x1b[1m";
    pub const DIM: &'static str = "\x1b[2m";
    pub const ITALIC: &'static str = "\x1b[3m";
    pub const UNDERLINE: &'static str = "\x1b[4m";
    pub const RED_FG: &'static str = "\x1b[31m";
    pub const GREEN_FG: &'static str = "\x1b[32m";
    pub const BLUE_FG: &'static str = "\x1b[34m";

    // Sample shell content
    pub const SHELL_PROMPT: &'static str = "user@host:~$ ";
    pub const SAMPLE_OUTPUT: &'static str = "Hello, World!\r\n";
    pub const SAMPLE_ERROR: &'static str = "\x1b[31mError: Something went wrong\x1b[0m\r\n";

    /// Valid settings JSON.
    pub fn valid_settings_json() -> &'static str {
        r#"{
            "theme": "Catppuccin Mocha",
            "font_family": "Iosevka Nerd Font",
            "window_bounds": {
                "x": 100.0,
                "y": 100.0,
                "width": 1200.0,
                "height": 800.0
            }
        }"#
    }

    /// Settings JSON with only theme.
    pub fn theme_only_settings() -> &'static str {
        r#"{ "theme": "Tokyo Night" }"#
    }

    /// Invalid/malformed JSON.
    pub fn invalid_json() -> &'static str {
        "{ this is not valid json }"
    }

    /// Settings with oversized string (for validation testing).
    pub fn oversized_theme_name() -> String {
        let long_name: String = (0..300).map(|_| 'a').collect();
        format!(r#"{{ "theme": "{}" }}"#, long_name)
    }

    /// Settings with invalid window bounds.
    pub fn invalid_window_bounds() -> &'static str {
        r#"{
            "window_bounds": {
                "x": "not a number",
                "y": 100.0,
                "width": -500.0,
                "height": 800.0
            }
        }"#
    }

    /// Create cursor position escape sequence.
    pub fn cursor_to(row: usize, col: usize) -> String {
        format!("\x1b[{};{}H", row + 1, col + 1)
    }

    /// Create colored text.
    pub fn colored_text(text: &str, color_code: u8) -> String {
        format!("\x1b[{}m{}\x1b[0m", color_code, text)
    }
}

// ============================================================================
// Async Helpers
// ============================================================================

/// Wait for a condition to become true with timeout.
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

/// Run an async operation with timeout.
pub async fn with_timeout<F, T>(timeout: Duration, future: F) -> Result<T, &'static str>
where
    F: std::future::Future<Output = T>,
{
    tokio::time::timeout(timeout, future)
        .await
        .map_err(|_| "Operation timed out")
}

// ============================================================================
// Assertion Helpers
// ============================================================================

/// Assert that a string contains a substring (with better error message).
#[macro_export]
macro_rules! assert_contains {
    ($haystack:expr, $needle:expr) => {
        if !$haystack.contains($needle) {
            panic!(
                "assertion failed: `haystack.contains(needle)`\n  haystack: {:?}\n  needle: {:?}",
                $haystack, $needle
            );
        }
    };
    ($haystack:expr, $needle:expr, $($arg:tt)+) => {
        if !$haystack.contains($needle) {
            panic!(
                "assertion failed: `haystack.contains(needle)`: {}\n  haystack: {:?}\n  needle: {:?}",
                format_args!($($arg)+), $haystack, $needle
            );
        }
    };
}

/// Assert that a string does not contain a substring.
#[macro_export]
macro_rules! assert_not_contains {
    ($haystack:expr, $needle:expr) => {
        if $haystack.contains($needle) {
            panic!(
                "assertion failed: `!haystack.contains(needle)`\n  haystack: {:?}\n  needle: {:?}",
                $haystack, $needle
            );
        }
    };
}

/// Assert that a value is within a range.
#[macro_export]
macro_rules! assert_in_range {
    ($value:expr, $min:expr, $max:expr) => {
        if $value < $min || $value > $max {
            panic!(
                "assertion failed: `{} <= {} <= {}`\n  value: {}",
                $min, $value, $max, $value
            );
        }
    };
}

#[cfg(test)]
mod tests {
    use super::{
        wait_for, with_timeout, Duration, Fixtures, MockSettings, MockTerminalSession, TestEnv,
        SHORT_TIMEOUT,
    };
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_env_creates_directories() {
        let env = TestEnv::new();
        assert!(env.config_dir.exists());
        assert!(env.path().exists());
    }

    #[test]
    fn test_env_write_read_settings() {
        let env = TestEnv::new();
        env.write_settings(r#"{"theme": "test"}"#);

        let content = env.read_settings();
        assert!(content.is_some());
        assert!(content.unwrap().contains("test"));
    }

    #[test]
    fn test_mock_settings_to_json() {
        let settings = MockSettings::with_theme("Test Theme");
        let json = settings.to_json();
        assert!(json.contains("Test Theme"));
    }

    #[test]
    fn test_mock_terminal_session() {
        let session = MockTerminalSession::new();
        assert!(!session.has_exited());
        assert!(session.is_running());

        session.send_input("hello");
        assert_eq!(session.get_input(), "hello");

        session.write_output("world");
        assert!(session.output_contains("world"));

        session.exit();
        assert!(session.has_exited());
        assert!(!session.is_running());
    }

    #[test]
    fn test_mock_terminal_resize() {
        let mut session = MockTerminalSession::new();
        assert_eq!(session.size, (80, 24));

        session.resize(120, 40);
        assert_eq!(session.size, (120, 40));
    }

    #[test]
    fn test_fixtures() {
        let cursor = Fixtures::cursor_to(5, 10);
        assert_eq!(cursor, "\x1b[6;11H");

        let colored = Fixtures::colored_text("test", 31);
        assert!(colored.starts_with("\x1b[31m"));
        assert!(colored.ends_with("\x1b[0m"));
    }

    #[tokio::test]
    async fn test_wait_for_success() {
        let flag = Arc::new(AtomicBool::new(false));
        let flag_clone = flag.clone();

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
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }
}
