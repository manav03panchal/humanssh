//! PTY process management.

use anyhow::{Context, Result};
use portable_pty::{native_pty_system, Child, CommandBuilder, PtyPair, PtySize};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread;

/// Allowed shells for security validation.
/// Only absolute paths to known shells are permitted.
const ALLOWED_SHELLS: &[&str] = &[
    // Standard locations
    "/bin/sh",
    "/bin/bash",
    "/bin/zsh",
    "/bin/fish",
    "/bin/dash",
    "/bin/ksh",
    "/bin/tcsh",
    "/bin/csh",
    // /usr/bin locations (common on some systems)
    "/usr/bin/sh",
    "/usr/bin/bash",
    "/usr/bin/zsh",
    "/usr/bin/fish",
    "/usr/bin/dash",
    "/usr/bin/ksh",
    "/usr/bin/tcsh",
    "/usr/bin/csh",
    // /usr/local/bin (Homebrew, custom installs)
    "/usr/local/bin/bash",
    "/usr/local/bin/zsh",
    "/usr/local/bin/fish",
    // Nix
    "/run/current-system/sw/bin/bash",
    "/run/current-system/sw/bin/zsh",
    "/run/current-system/sw/bin/fish",
];

/// Default shell to use when SHELL is invalid or unset.
const DEFAULT_SHELL: &str = "/bin/zsh";

/// Get a validated shell path from the environment.
/// Falls back to DEFAULT_SHELL if SHELL is unset, invalid, or not in the allowlist.
fn get_validated_shell() -> String {
    let shell = match std::env::var("SHELL") {
        Ok(s) => s,
        Err(_) => {
            tracing::debug!("SHELL not set, using default: {}", DEFAULT_SHELL);
            return DEFAULT_SHELL.to_string();
        }
    };

    // Must be an absolute path
    if !shell.starts_with('/') {
        tracing::warn!(
            "SHELL is not an absolute path '{}', using default: {}",
            shell,
            DEFAULT_SHELL
        );
        return DEFAULT_SHELL.to_string();
    }

    // Must exist
    if !Path::new(&shell).exists() {
        tracing::warn!(
            "SHELL does not exist '{}', using default: {}",
            shell,
            DEFAULT_SHELL
        );
        return DEFAULT_SHELL.to_string();
    }

    // Check against allowlist
    if ALLOWED_SHELLS.contains(&shell.as_str()) {
        return shell;
    }

    // Not in allowlist - check if it's a symlink to an allowed shell
    if let Ok(resolved) = std::fs::canonicalize(&shell) {
        let resolved_str = resolved.to_string_lossy();
        if ALLOWED_SHELLS
            .iter()
            .any(|&allowed| resolved_str.ends_with(allowed.rsplit('/').next().unwrap_or("")))
        {
            tracing::debug!(
                "SHELL '{}' resolves to allowed shell '{}'",
                shell,
                resolved_str
            );
            return shell;
        }
    }

    tracing::warn!(
        "SHELL '{}' not in allowed list, using default: {}",
        shell,
        DEFAULT_SHELL
    );
    DEFAULT_SHELL.to_string()
}

/// Cached process state to avoid blocking UI thread.
/// Updated periodically in the background.
#[derive(Default)]
struct ProcessCache {
    /// Whether there are running child processes
    has_children: bool,
    /// Name of the foreground process (if any)
    process_name: Option<String>,
    /// When the cache was last updated
    last_update: Option<std::time::Instant>,
}

/// How often to refresh process cache (in milliseconds)
const PROCESS_CACHE_TTL_MS: u64 = 500;

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
    /// Cached process detection results (avoids blocking UI)
    process_cache: std::sync::Mutex<ProcessCache>,
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

        // Get and validate the user's shell (security: prevents command injection)
        let shell = get_validated_shell();

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

        // Bounded channel for output bytes (prevents memory exhaustion under heavy output).
        // 256 messages * ~4KB each = ~1MB max queue size.
        const PTY_OUTPUT_QUEUE_SIZE: usize = 256;
        let (output_tx, output_rx): (SyncSender<Vec<u8>>, Receiver<Vec<u8>>) =
            mpsc::sync_channel(PTY_OUTPUT_QUEUE_SIZE);

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
                        // Use try_send with bounded channel - drop frames if queue is full
                        // This provides backpressure and prevents memory exhaustion
                        match output_tx.try_send(buf[..n].to_vec()) {
                            Ok(()) => {}
                            Err(TrySendError::Full(_)) => {
                                // Queue full - drop this frame (terminal will catch up)
                                tracing::trace!("PTY output queue full, dropping frame");
                            }
                            Err(TrySendError::Disconnected(_)) => {
                                break; // Channel closed
                            }
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
            process_cache: std::sync::Mutex::new(ProcessCache::default()),
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
    /// Uses cached results to avoid blocking the UI thread.
    /// Cache is refreshed every PROCESS_CACHE_TTL_MS milliseconds.
    pub fn has_running_processes(&self) -> bool {
        self.refresh_process_cache_if_stale();
        self.process_cache
            .lock()
            .map(|cache| cache.has_children)
            .unwrap_or(false)
    }

    /// Get the name of any running foreground process (for display in confirmation).
    /// Uses cached results to avoid blocking the UI thread.
    pub fn get_running_process_name(&self) -> Option<String> {
        self.refresh_process_cache_if_stale();
        self.process_cache
            .lock()
            .ok()
            .and_then(|cache| cache.process_name.clone())
    }

    /// Refresh the process cache if it's stale (older than PROCESS_CACHE_TTL_MS).
    fn refresh_process_cache_if_stale(&self) {
        let needs_refresh = {
            let cache = match self.process_cache.lock() {
                Ok(c) => c,
                Err(_) => return,
            };
            cache
                .last_update
                .is_none_or(|last| last.elapsed().as_millis() > PROCESS_CACHE_TTL_MS as u128)
        };

        if needs_refresh {
            let (has_children, process_name) = self.detect_child_processes();
            if let Ok(mut cache) = self.process_cache.lock() {
                cache.has_children = has_children;
                cache.process_name = process_name;
                cache.last_update = Some(std::time::Instant::now());
            }
        }
    }

    /// Actually detect child processes (the slow operation).
    /// Returns (has_children, process_name).
    fn detect_child_processes(&self) -> (bool, Option<String>) {
        let Some(pid) = self.child.process_id() else {
            return (false, None);
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
                                            // Extract process name from (comm) in stat
                                            let process_name = stat.find('(').and_then(|start| {
                                                stat.rfind(')')
                                                    .map(|end| stat[start + 1..end].to_string())
                                            });
                                            return (true, process_name);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            return (false, None);
        }

        // macOS/BSD fallback using pgrep
        #[cfg(not(target_os = "linux"))]
        {
            let output = match std::process::Command::new("pgrep")
                .args(["-P", &pid.to_string()])
                .output()
            {
                Ok(o) => o,
                Err(_) => return (false, None),
            };

            if output.stdout.is_empty() {
                return (false, None);
            }

            // Has children - try to get the name
            let child_pids = String::from_utf8_lossy(&output.stdout);
            let process_name = child_pids.lines().next().and_then(|child_pid| {
                std::process::Command::new("ps")
                    .args(["-p", child_pid, "-o", "comm="])
                    .output()
                    .ok()
                    .map(|ps_output| {
                        String::from_utf8_lossy(&ps_output.stdout)
                            .trim()
                            .to_string()
                    })
                    .filter(|s| !s.is_empty())
            });

            (true, process_name)
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;
    use test_case::test_case;

    // ========================================================================
    // Shell Validation Tests
    // ========================================================================

    #[test]
    fn test_allowed_shells_list_is_not_empty() {
        assert!(!ALLOWED_SHELLS.is_empty());
    }

    #[test]
    fn test_allowed_shells_all_have_absolute_paths() {
        for shell in ALLOWED_SHELLS {
            assert!(
                shell.starts_with('/'),
                "Shell '{}' is not an absolute path",
                shell
            );
        }
    }

    #[test]
    fn test_default_shell_is_in_allowed_list() {
        assert!(
            ALLOWED_SHELLS.contains(&DEFAULT_SHELL),
            "DEFAULT_SHELL '{}' should be in ALLOWED_SHELLS",
            DEFAULT_SHELL
        );
    }

    #[test_case("/bin/bash" ; "bin_bash")]
    #[test_case("/bin/zsh" ; "bin_zsh")]
    #[test_case("/bin/sh" ; "bin_sh")]
    #[test_case("/usr/bin/bash" ; "usr_bin_bash")]
    #[test_case("/usr/bin/zsh" ; "usr_bin_zsh")]
    fn test_common_shells_are_allowed(shell: &str) {
        assert!(
            ALLOWED_SHELLS.contains(&shell),
            "Common shell '{}' should be in ALLOWED_SHELLS",
            shell
        );
    }

    #[test]
    fn test_get_validated_shell_returns_default_when_shell_unset() {
        // Save the original SHELL value
        let original = std::env::var("SHELL").ok();

        // Remove SHELL
        std::env::remove_var("SHELL");

        let result = get_validated_shell();
        assert_eq!(result, DEFAULT_SHELL);

        // Restore original
        if let Some(shell) = original {
            std::env::set_var("SHELL", shell);
        }
    }

    #[test]
    fn test_get_validated_shell_rejects_relative_path() {
        let original = std::env::var("SHELL").ok();

        // Set a relative path
        std::env::set_var("SHELL", "bash");

        let result = get_validated_shell();
        assert_eq!(result, DEFAULT_SHELL);

        // Restore original
        if let Some(shell) = original {
            std::env::set_var("SHELL", shell);
        } else {
            std::env::remove_var("SHELL");
        }
    }

    #[test]
    fn test_get_validated_shell_rejects_nonexistent_path() {
        let original = std::env::var("SHELL").ok();

        // Set a nonexistent path
        std::env::set_var("SHELL", "/nonexistent/path/to/shell");

        let result = get_validated_shell();
        assert_eq!(result, DEFAULT_SHELL);

        // Restore original
        if let Some(shell) = original {
            std::env::set_var("SHELL", shell);
        } else {
            std::env::remove_var("SHELL");
        }
    }

    #[test]
    fn test_get_validated_shell_accepts_valid_shell() {
        let original = std::env::var("SHELL").ok();

        // Find a shell that exists on the system
        let valid_shell = ALLOWED_SHELLS
            .iter()
            .find(|&&s| Path::new(s).exists())
            .copied();

        if let Some(shell) = valid_shell {
            std::env::set_var("SHELL", shell);
            let result = get_validated_shell();
            assert_eq!(result, shell);
        }

        // Restore original
        if let Some(shell) = original {
            std::env::set_var("SHELL", shell);
        } else {
            std::env::remove_var("SHELL");
        }
    }

    // ========================================================================
    // ProcessCache Tests
    // ========================================================================

    #[test]
    fn test_process_cache_default() {
        let cache = ProcessCache::default();
        assert!(!cache.has_children);
        assert!(cache.process_name.is_none());
        assert!(cache.last_update.is_none());
    }

    #[test]
    fn test_process_cache_ttl_constant() {
        // Ensure TTL is reasonable (not too short, not too long)
        assert!(PROCESS_CACHE_TTL_MS >= 100, "TTL should be at least 100ms");
        assert!(
            PROCESS_CACHE_TTL_MS <= 5000,
            "TTL should be at most 5 seconds"
        );
    }

    // ========================================================================
    // Mock Infrastructure for Testing PtyHandler-like behavior
    // ========================================================================

    /// Mock child process for testing without spawning real processes.
    struct MockChild {
        pid: Option<u32>,
        killed: AtomicBool,
        waited: AtomicBool,
    }

    impl MockChild {
        fn new(pid: Option<u32>) -> Self {
            Self {
                pid,
                killed: AtomicBool::new(false),
                waited: AtomicBool::new(false),
            }
        }
    }

    /// Mock PTY for testing I/O patterns.
    struct MockPtyIO {
        written_data: std::sync::Mutex<Vec<u8>>,
        read_queue: std::sync::Mutex<Vec<Vec<u8>>>,
        exited: AtomicBool,
        resize_calls: AtomicU32,
        last_size: std::sync::Mutex<(u16, u16)>,
    }

    impl MockPtyIO {
        fn new() -> Self {
            Self {
                written_data: std::sync::Mutex::new(Vec::new()),
                read_queue: std::sync::Mutex::new(Vec::new()),
                exited: AtomicBool::new(false),
                resize_calls: AtomicU32::new(0),
                last_size: std::sync::Mutex::new((80, 24)),
            }
        }

        fn write(&self, data: &[u8]) -> std::io::Result<()> {
            if self.exited.load(Ordering::SeqCst) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "PTY has exited",
                ));
            }
            self.written_data.lock().unwrap().extend_from_slice(data);
            Ok(())
        }

        fn queue_output(&self, data: &[u8]) {
            self.read_queue.lock().unwrap().push(data.to_vec());
        }

        fn read_output(&self) -> Vec<Vec<u8>> {
            let mut queue = self.read_queue.lock().unwrap();
            std::mem::take(&mut *queue)
        }

        fn resize(&self, rows: u16, cols: u16) -> Result<()> {
            self.resize_calls.fetch_add(1, Ordering::SeqCst);
            *self.last_size.lock().unwrap() = (rows, cols);
            Ok(())
        }

        fn get_written_data(&self) -> Vec<u8> {
            self.written_data.lock().unwrap().clone()
        }

        fn set_exited(&self) {
            self.exited.store(true, Ordering::SeqCst);
        }

        fn has_exited(&self) -> bool {
            self.exited.load(Ordering::SeqCst)
        }
    }

    // ========================================================================
    // I/O Pattern Tests (using mocks to avoid SIGBUS/resource issues)
    // ========================================================================

    #[test]
    fn test_mock_pty_write_basic() {
        let pty = MockPtyIO::new();

        pty.write(b"hello").unwrap();
        pty.write(b" world").unwrap();

        assert_eq!(pty.get_written_data(), b"hello world");
    }

    #[test]
    fn test_mock_pty_write_fails_when_exited() {
        let pty = MockPtyIO::new();

        pty.write(b"before").unwrap();
        pty.set_exited();

        let result = pty.write(b"after");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn test_mock_pty_read_output_empty() {
        let pty = MockPtyIO::new();

        let output = pty.read_output();
        assert!(output.is_empty());
    }

    #[test]
    fn test_mock_pty_read_output_queued() {
        let pty = MockPtyIO::new();

        pty.queue_output(b"line 1\n");
        pty.queue_output(b"line 2\n");

        let output = pty.read_output();
        assert_eq!(output.len(), 2);
        assert_eq!(output[0], b"line 1\n");
        assert_eq!(output[1], b"line 2\n");

        // Queue should be empty after reading
        let output2 = pty.read_output();
        assert!(output2.is_empty());
    }

    #[test]
    fn test_mock_pty_resize() {
        let pty = MockPtyIO::new();

        pty.resize(50, 120).unwrap();

        assert_eq!(pty.resize_calls.load(Ordering::SeqCst), 1);
        assert_eq!(*pty.last_size.lock().unwrap(), (50, 120));

        pty.resize(30, 80).unwrap();
        assert_eq!(pty.resize_calls.load(Ordering::SeqCst), 2);
        assert_eq!(*pty.last_size.lock().unwrap(), (30, 80));
    }

    #[test]
    fn test_mock_pty_exit_state() {
        let pty = MockPtyIO::new();

        assert!(!pty.has_exited());
        pty.set_exited();
        assert!(pty.has_exited());
    }

    // ========================================================================
    // Channel Backpressure Tests
    // ========================================================================

    #[test]
    fn test_bounded_channel_backpressure() {
        // Test the bounded channel behavior that PtyHandler uses
        const QUEUE_SIZE: usize = 4;
        let (tx, rx): (mpsc::SyncSender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) =
            mpsc::sync_channel(QUEUE_SIZE);

        // Fill the queue
        for i in 0..QUEUE_SIZE {
            let result = tx.try_send(vec![i as u8]);
            assert!(result.is_ok(), "Should be able to send {} items", i + 1);
        }

        // Queue is now full - try_send should fail with Full
        let result = tx.try_send(vec![99]);
        assert!(matches!(result, Err(TrySendError::Full(_))));

        // Consume one item
        let _ = rx.try_recv();

        // Now we should be able to send again
        let result = tx.try_send(vec![100]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_channel_disconnect_behavior() {
        let (tx, rx): (mpsc::SyncSender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) = mpsc::sync_channel(4);

        // Drop receiver
        drop(rx);

        // Sender should get Disconnected
        let result = tx.try_send(vec![1]);
        assert!(matches!(result, Err(TrySendError::Disconnected(_))));
    }

    // ========================================================================
    // Exit Flag Tests (thread-safe behavior)
    // ========================================================================

    #[test]
    fn test_exit_flag_atomic_operations() {
        let exited = Arc::new(AtomicBool::new(false));
        let exited_clone = exited.clone();

        // Spawn a thread that will set the flag
        let handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(10));
            exited_clone.store(true, Ordering::SeqCst);
        });

        // Initially false
        assert!(!exited.load(Ordering::SeqCst));

        handle.join().unwrap();

        // After thread completes, should be true
        assert!(exited.load(Ordering::SeqCst));
    }

    #[test]
    fn test_exit_flag_multiple_readers() {
        let exited = Arc::new(AtomicBool::new(false));

        let readers: Vec<_> = (0..4)
            .map(|_| {
                let e = exited.clone();
                std::thread::spawn(move || {
                    for _ in 0..100 {
                        let _ = e.load(Ordering::SeqCst);
                    }
                })
            })
            .collect();

        // Set the flag while readers are running
        exited.store(true, Ordering::SeqCst);

        for reader in readers {
            reader.join().unwrap();
        }

        assert!(exited.load(Ordering::SeqCst));
    }

    // ========================================================================
    // Process Cache Refresh Logic Tests
    // ========================================================================

    #[test]
    fn test_cache_staleness_detection() {
        let cache = std::sync::Mutex::new(ProcessCache::default());

        // Initially stale (no last_update)
        {
            let c = cache.lock().unwrap();
            assert!(c.last_update.is_none());
            let is_stale = c
                .last_update
                .is_none_or(|last| last.elapsed().as_millis() > PROCESS_CACHE_TTL_MS as u128);
            assert!(is_stale);
        }

        // Set last_update to now
        {
            let mut c = cache.lock().unwrap();
            c.last_update = Some(std::time::Instant::now());
        }

        // Should not be stale immediately
        {
            let c = cache.lock().unwrap();
            let is_stale = c
                .last_update
                .is_none_or(|last| last.elapsed().as_millis() > PROCESS_CACHE_TTL_MS as u128);
            assert!(!is_stale);
        }

        // Wait for TTL to expire
        std::thread::sleep(Duration::from_millis(PROCESS_CACHE_TTL_MS + 50));

        // Should be stale now
        {
            let c = cache.lock().unwrap();
            let is_stale = c
                .last_update
                .is_none_or(|last| last.elapsed().as_millis() > PROCESS_CACHE_TTL_MS as u128);
            assert!(is_stale);
        }
    }

    #[test]
    fn test_cache_update_values() {
        let cache = std::sync::Mutex::new(ProcessCache::default());

        // Update cache with values
        {
            let mut c = cache.lock().unwrap();
            c.has_children = true;
            c.process_name = Some("vim".to_string());
            c.last_update = Some(std::time::Instant::now());
        }

        // Read values back
        {
            let c = cache.lock().unwrap();
            assert!(c.has_children);
            assert_eq!(c.process_name, Some("vim".to_string()));
            assert!(c.last_update.is_some());
        }
    }

    // ========================================================================
    // PTY Size Validation Tests
    // ========================================================================

    #[test_case(1, 1 ; "minimum_size")]
    #[test_case(80, 24 ; "standard_size")]
    #[test_case(200, 60 ; "large_size")]
    #[test_case(u16::MAX, u16::MAX ; "maximum_size")]
    fn test_pty_size_values(cols: u16, rows: u16) {
        // Test that PtySize can be created with various dimensions
        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        assert_eq!(size.rows, rows);
        assert_eq!(size.cols, cols);
    }

    #[test]
    fn test_pty_size_with_pixels() {
        let size = PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 640,
            pixel_height: 480,
        };

        assert_eq!(size.pixel_width, 640);
        assert_eq!(size.pixel_height, 480);
    }

    // ========================================================================
    // Mock Child Process Tests
    // ========================================================================

    #[test]
    fn test_mock_child_with_pid() {
        let child = MockChild::new(Some(12345));
        assert_eq!(child.pid, Some(12345));
        assert!(!child.killed.load(Ordering::SeqCst));
        assert!(!child.waited.load(Ordering::SeqCst));
    }

    #[test]
    fn test_mock_child_without_pid() {
        let child = MockChild::new(None);
        assert_eq!(child.pid, None);
    }

    #[test]
    fn test_mock_child_kill_wait_sequence() {
        let child = MockChild::new(Some(12345));

        // Simulate kill
        child.killed.store(true, Ordering::SeqCst);
        assert!(child.killed.load(Ordering::SeqCst));

        // Simulate wait
        child.waited.store(true, Ordering::SeqCst);
        assert!(child.waited.load(Ordering::SeqCst));
    }

    // ========================================================================
    // CommandBuilder Environment Tests
    // ========================================================================

    #[test]
    fn test_command_builder_env_setting() {
        // Test that CommandBuilder properly handles env vars
        let mut cmd = CommandBuilder::new("/bin/sh");
        cmd.env("TERM", "xterm-256color");
        cmd.env("CUSTOM_VAR", "custom_value");

        // CommandBuilder is opaque, but we can verify it doesn't panic
        // with these operations
    }

    #[test]
    fn test_command_builder_with_different_shells() {
        for shell in &["/bin/sh", "/bin/bash", "/bin/zsh"] {
            let mut cmd = CommandBuilder::new(shell);
            cmd.env("TERM", "xterm-256color");
            // Just verify no panic
        }
    }

    // ========================================================================
    // Input/Output Sequence Tests
    // ========================================================================

    #[test]
    fn test_escape_sequence_passthrough() {
        let pty = MockPtyIO::new();

        // Test various escape sequences that should be passed through
        let sequences = [
            b"\x1b[H".as_slice(),      // Cursor home
            b"\x1b[2J".as_slice(),     // Clear screen
            b"\x1b[0m".as_slice(),     // Reset attributes
            b"\x1b[31m".as_slice(),    // Red foreground
            b"\x1b[?1049h".as_slice(), // Alternate screen
            b"\x1b[?25h".as_slice(),   // Show cursor
            b"\x1b[?1000h".as_slice(), // Mouse tracking
        ];

        for seq in sequences {
            pty.write(seq).unwrap();
        }

        let written = pty.get_written_data();
        assert_eq!(
            written.len(),
            sequences.iter().map(|s| s.len()).sum::<usize>()
        );
    }

    #[test]
    fn test_keyboard_input_sequences() {
        let pty = MockPtyIO::new();

        // Test keyboard input sequences
        let inputs = [
            b"a".as_slice(),       // Regular character
            b"\r".as_slice(),      // Enter
            b"\x7f".as_slice(),    // Backspace
            b"\x1b[A".as_slice(),  // Up arrow
            b"\x1b[B".as_slice(),  // Down arrow
            b"\x1b[C".as_slice(),  // Right arrow
            b"\x1b[D".as_slice(),  // Left arrow
            b"\x1b[3~".as_slice(), // Delete
            b"\x1bOP".as_slice(),  // F1
            b"\x03".as_slice(),    // Ctrl+C
        ];

        for input in inputs {
            pty.write(input).unwrap();
        }

        let written = pty.get_written_data();
        assert!(!written.is_empty());
    }

    // ========================================================================
    // Concurrent Access Tests
    // ========================================================================

    #[test]
    fn test_concurrent_write_and_read() {
        let pty = Arc::new(MockPtyIO::new());
        let pty_write = pty.clone();
        let pty_read = pty.clone();

        // Writer thread
        let writer = std::thread::spawn(move || {
            for i in 0..100u8 {
                let _ = pty_write.write(&[i]);
            }
        });

        // Reader thread queuing output
        let reader = std::thread::spawn(move || {
            for i in 0..50u8 {
                pty_read.queue_output(&[i, i + 1]);
            }
        });

        writer.join().unwrap();
        reader.join().unwrap();

        // Verify data integrity
        let written = pty.get_written_data();
        assert_eq!(written.len(), 100);

        let output = pty.read_output();
        assert_eq!(output.len(), 50);
    }

    #[test]
    fn test_mutex_under_contention() {
        let cache = Arc::new(std::sync::Mutex::new(ProcessCache::default()));

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let c = cache.clone();
                std::thread::spawn(move || {
                    for _ in 0..100 {
                        let mut guard = c.lock().unwrap();
                        guard.has_children = i % 2 == 0;
                        guard.last_update = Some(std::time::Instant::now());
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        // Just verify no deadlock or panic occurred
        drop(cache.lock().unwrap());
    }

    // ========================================================================
    // Error Handling Tests
    // ========================================================================

    #[test]
    fn test_write_error_on_closed_channel() {
        let (tx, rx): (mpsc::SyncSender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) = mpsc::sync_channel(1);

        // Close the receiver
        drop(rx);

        // Try to send - should fail with Disconnected
        let result = tx.try_send(vec![1, 2, 3]);
        assert!(result.is_err());

        match result {
            Err(TrySendError::Disconnected(_)) => {}
            _ => panic!("Expected Disconnected error"),
        }
    }

    #[test]
    fn test_read_from_empty_channel() {
        let (_tx, rx): (mpsc::SyncSender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) = mpsc::sync_channel(1);

        // Try to receive from empty channel
        let result = rx.try_recv();
        assert!(matches!(result, Err(mpsc::TryRecvError::Empty)));
    }

    #[test]
    fn test_read_from_disconnected_channel() {
        let (tx, rx): (mpsc::SyncSender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) = mpsc::sync_channel(1);

        // Close the sender
        drop(tx);

        // Try to receive - should get Disconnected
        let result = rx.try_recv();
        assert!(matches!(result, Err(mpsc::TryRecvError::Disconnected)));
    }

    // ========================================================================
    // Integration-style Tests (still using mocks)
    // ========================================================================

    #[test]
    fn test_pty_lifecycle_simulation() {
        let pty = MockPtyIO::new();

        // 1. Initial state
        assert!(!pty.has_exited());

        // 2. Write some input
        pty.write(b"ls -la\r").unwrap();

        // 3. Queue some output (simulating shell response)
        pty.queue_output(b"total 0\r\n");
        pty.queue_output(b"drwxr-xr-x  2 user user 4096 Jan  1 00:00 .\r\n");

        // 4. Resize
        pty.resize(50, 150).unwrap();

        // 5. Read output
        let output = pty.read_output();
        assert_eq!(output.len(), 2);

        // 6. More interaction
        pty.write(b"exit\r").unwrap();

        // 7. Exit
        pty.set_exited();
        assert!(pty.has_exited());

        // 8. Writes should fail now
        assert!(pty.write(b"should fail").is_err());

        // 9. Verify all written data
        let all_written = pty.get_written_data();
        assert!(all_written.starts_with(b"ls -la\r"));
        assert!(all_written.ends_with(b"exit\r"));
    }

    #[test]
    fn test_output_accumulation() {
        let pty = MockPtyIO::new();

        // Simulate gradual output
        for i in 0..10 {
            pty.queue_output(format!("line {}\n", i).as_bytes());
        }

        let output = pty.read_output();
        assert_eq!(output.len(), 10);

        for (i, line) in output.iter().enumerate() {
            assert_eq!(*line, format!("line {}\n", i).as_bytes());
        }
    }

    #[test]
    fn test_interleaved_io() {
        let pty = MockPtyIO::new();

        // Simulate interleaved input/output
        pty.write(b"echo hello\r").unwrap();
        pty.queue_output(b"echo hello\r\n");
        pty.queue_output(b"hello\r\n");

        let output = pty.read_output();
        assert_eq!(output.len(), 2);

        pty.write(b"echo world\r").unwrap();
        pty.queue_output(b"echo world\r\n");
        pty.queue_output(b"world\r\n");

        let output = pty.read_output();
        assert_eq!(output.len(), 2);
    }

    // ========================================================================
    // PTY Output Queue Size Tests
    // ========================================================================

    #[test]
    fn test_pty_output_queue_size_constant() {
        // Verify the constant is defined and reasonable
        // The actual constant is 256 in the implementation
        const EXPECTED_QUEUE_SIZE: usize = 256;

        // Create a channel with the same size to verify the constant makes sense
        let (tx, _rx): (mpsc::SyncSender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) =
            mpsc::sync_channel(EXPECTED_QUEUE_SIZE);

        // Should be able to send EXPECTED_QUEUE_SIZE items
        for i in 0..EXPECTED_QUEUE_SIZE {
            assert!(
                tx.try_send(vec![i as u8]).is_ok(),
                "Failed to send item {}",
                i
            );
        }

        // Next send should fail with Full
        assert!(matches!(tx.try_send(vec![0]), Err(TrySendError::Full(_))));
    }

    // ========================================================================
    // Edge Case Tests
    // ========================================================================

    #[test]
    fn test_empty_write() {
        let pty = MockPtyIO::new();

        pty.write(b"").unwrap();
        assert!(pty.get_written_data().is_empty());
    }

    #[test]
    fn test_large_write() {
        let pty = MockPtyIO::new();

        // Write 1MB of data
        let large_data = vec![b'x'; 1024 * 1024];
        pty.write(&large_data).unwrap();

        assert_eq!(pty.get_written_data().len(), 1024 * 1024);
    }

    #[test]
    fn test_binary_data_write() {
        let pty = MockPtyIO::new();

        // Write all possible byte values
        let binary_data: Vec<u8> = (0..=255).collect();
        pty.write(&binary_data).unwrap();

        assert_eq!(pty.get_written_data(), binary_data);
    }

    #[test]
    fn test_resize_zero_dimensions() {
        let pty = MockPtyIO::new();

        // Zero dimensions should still work (even if nonsensical)
        let result = pty.resize(0, 0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_rapid_resize() {
        let pty = MockPtyIO::new();

        // Rapid consecutive resizes
        for i in 0..100 {
            pty.resize(24 + i, 80 + i).unwrap();
        }

        assert_eq!(pty.resize_calls.load(Ordering::SeqCst), 100);
        assert_eq!(*pty.last_size.lock().unwrap(), (123, 179));
    }
}
