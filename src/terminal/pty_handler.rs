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
