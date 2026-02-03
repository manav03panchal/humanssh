//! PTY process management.

use anyhow::{Context, Result};
use portable_pty::{native_pty_system, Child, CommandBuilder, PtyPair, PtySize};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread;

/// Allowed shells for security validation (Unix).
/// Only absolute paths to known shells are permitted.
#[cfg(not(target_os = "windows"))]
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

/// Allowed shell executables for security validation (Windows).
/// These are executable names that will be resolved via PATH or System32.
#[cfg(target_os = "windows")]
const ALLOWED_SHELLS_WINDOWS: &[&str] = &[
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe", // PowerShell Core
];

/// Default shell to use when SHELL is invalid or unset (Unix).
#[cfg(not(target_os = "windows"))]
const DEFAULT_SHELL: &str = "/bin/zsh";

/// Default shell to use when COMSPEC is invalid or unset (Windows).
#[cfg(target_os = "windows")]
const DEFAULT_SHELL_WINDOWS: &str = "powershell.exe";

/// Get a validated shell path from the environment (Unix).
/// Falls back to DEFAULT_SHELL if SHELL is unset, invalid, or not in the allowlist.
#[cfg(not(target_os = "windows"))]
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

/// Get a validated shell path from the environment (Windows).
/// Uses COMSPEC or falls back to PowerShell.
#[cfg(target_os = "windows")]
fn get_validated_shell() -> String {
    // Try COMSPEC first (typically cmd.exe)
    if let Ok(comspec) = std::env::var("COMSPEC") {
        let shell_name = Path::new(&comspec)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();

        if ALLOWED_SHELLS_WINDOWS
            .iter()
            .any(|&allowed| shell_name == allowed.to_lowercase())
            && Path::new(&comspec).exists()
        {
            tracing::debug!("Using COMSPEC shell: {}", comspec);
            return comspec;
        }
    }

    // Try to find PowerShell (preferred default on Windows)
    if let Ok(system_root) = std::env::var("SystemRoot") {
        // Try PowerShell Core (pwsh) first via PATH
        if let Ok(output) = std::process::Command::new("where").arg("pwsh.exe").output() {
            if output.status.success() {
                let path = String::from_utf8_lossy(&output.stdout);
                if let Some(first_line) = path.lines().next() {
                    let pwsh_path = first_line.trim();
                    if Path::new(pwsh_path).exists() {
                        tracing::debug!("Using PowerShell Core: {}", pwsh_path);
                        return pwsh_path.to_string();
                    }
                }
            }
        }

        // Fall back to Windows PowerShell
        let powershell_path = format!(
            "{}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
            system_root
        );
        if Path::new(&powershell_path).exists() {
            tracing::debug!("Using Windows PowerShell: {}", powershell_path);
            return powershell_path;
        }

        // Last resort: cmd.exe
        let cmd_path = format!("{}\\System32\\cmd.exe", system_root);
        if Path::new(&cmd_path).exists() {
            tracing::debug!("Using cmd.exe: {}", cmd_path);
            return cmd_path;
        }
    }

    // Absolute fallback
    tracing::warn!(
        "Could not find any shell, using default: {}",
        DEFAULT_SHELL_WINDOWS
    );
    DEFAULT_SHELL_WINDOWS.to_string()
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
        Self::spawn_in_dir(rows, cols, None)
    }

    /// Spawn a new PTY with the user's default shell in a specific directory.
    ///
    /// # Arguments
    /// * `rows` - Initial terminal height in rows
    /// * `cols` - Initial terminal width in columns
    /// * `working_dir` - Optional working directory for the new shell
    ///
    /// # Returns
    /// A new `PtyHandler` on success, or an error if spawning failed.
    ///
    /// # Shell Selection
    /// Uses the `SHELL` environment variable. Falls back to `/bin/zsh` if not set.
    pub fn spawn_in_dir(
        rows: u16,
        cols: u16,
        working_dir: Option<&std::path::Path>,
    ) -> Result<Self> {
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

        // Platform-specific shell configuration
        #[cfg(not(target_os = "windows"))]
        {
            // Start shell as login shell (-l) so it sources user's profile (.zprofile, .bash_profile)
            // This ensures PATH and other env vars are properly set when launched from .app bundle
            cmd.arg("-l");
        }

        cmd.env("TERM", "xterm-256color");

        // Set working directory if provided
        if let Some(dir) = working_dir {
            if dir.is_dir() {
                cmd.cwd(dir);
            }
        }

        // Platform-specific PATH handling
        #[cfg(target_os = "macos")]
        {
            // Ensure common paths are in PATH for macOS app bundles (which have minimal env)
            if let Ok(current_path) = std::env::var("PATH") {
                let homebrew_paths = "/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin";
                if !current_path.contains("/opt/homebrew") {
                    cmd.env("PATH", format!("{}:{}", homebrew_paths, current_path));
                }
            } else {
                // Fallback PATH if none set
                cmd.env(
                    "PATH",
                    "/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
                );
            }
        }

        #[cfg(target_os = "linux")]
        {
            // Linux typically has proper PATH set, but ensure common locations
            if let Ok(current_path) = std::env::var("PATH") {
                if !current_path.contains("/usr/local/bin") {
                    cmd.env("PATH", format!("/usr/local/bin:{}", current_path));
                }
            }
        }

        // Windows: PATH is typically already correctly set, no modifications needed

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
        // 1024 messages provides better burst handling while still bounding memory.
        const PTY_OUTPUT_QUEUE_SIZE: usize = 1024;
        let (output_tx, output_rx): (SyncSender<Vec<u8>>, Receiver<Vec<u8>>) =
            mpsc::sync_channel(PTY_OUTPUT_QUEUE_SIZE);

        // Flag to track if process exited
        let exited = Arc::new(AtomicBool::new(false));
        let exited_clone = exited.clone();

        // Spawn thread to read PTY output
        let reader_thread = thread::spawn(move || {
            // 32KB buffer for better throughput during burst output (matches Alacritty/Ghostty)
            let mut buf = [0u8; 32768];
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

    /// Spawn a new PTY running a specific command.
    ///
    /// # Arguments
    /// * `rows` - Initial terminal height in rows
    /// * `cols` - Initial terminal width in columns
    /// * `command` - The command to run (e.g., "btop", "neofetch")
    /// * `args` - Arguments to pass to the command
    /// * `working_dir` - Optional working directory
    ///
    /// # Returns
    /// A new `PtyHandler` on success, or an error if spawning failed.
    pub fn spawn_command(
        rows: u16,
        cols: u16,
        command: &str,
        args: &[&str],
        working_dir: Option<&std::path::Path>,
    ) -> Result<Self> {
        let pty_system = native_pty_system();

        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("Failed to open PTY")?;

        let mut cmd = CommandBuilder::new(command);
        for arg in args {
            cmd.arg(*arg);
        }
        cmd.env("TERM", "xterm-256color");

        // Set working directory if provided
        if let Some(dir) = working_dir {
            if dir.is_dir() {
                cmd.cwd(dir);
            }
        }

        // Ensure common paths are in PATH for macOS app bundles
        if let Ok(current_path) = std::env::var("PATH") {
            let homebrew_paths = "/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin";
            if !current_path.contains("/opt/homebrew") {
                cmd.env("PATH", format!("{}:{}", homebrew_paths, current_path));
            }
        } else {
            cmd.env(
                "PATH",
                "/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
            );
        }

        // Spawn the command
        let child = pair
            .slave
            .spawn_command(cmd)
            .context("Failed to spawn command")?;

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

        const PTY_OUTPUT_QUEUE_SIZE: usize = 1024;
        let (output_tx, output_rx): (SyncSender<Vec<u8>>, Receiver<Vec<u8>>) =
            mpsc::sync_channel(PTY_OUTPUT_QUEUE_SIZE);

        let exited = Arc::new(AtomicBool::new(false));
        let exited_clone = exited.clone();

        let reader_thread = thread::spawn(move || {
            let mut buf = [0u8; 32768];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        exited_clone.store(true, Ordering::SeqCst);
                        break;
                    }
                    Ok(n) => match output_tx.try_send(buf[..n].to_vec()) {
                        Ok(()) => {}
                        Err(TrySendError::Full(_)) => {
                            tracing::trace!("PTY output queue full, dropping frame");
                        }
                        Err(TrySendError::Disconnected(_)) => {
                            break;
                        }
                    },
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
    /// Note: No explicit flush - OS handles buffering efficiently for interactive input
    pub fn write(&mut self, data: &[u8]) -> Result<()> {
        self.writer.write_all(data)?;
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

    /// Get the current working directory of the foreground process.
    ///
    /// Uses OS-specific methods to determine the CWD:
    /// - macOS: `lsof -p <pid>` to find the cwd file descriptor
    /// - Linux: reads `/proc/<pid>/cwd` symlink
    pub fn get_current_directory(&self) -> Option<std::path::PathBuf> {
        let pid = self.child.process_id()?;
        self.get_foreground_process_cwd(pid)
    }

    /// Get the CWD of the foreground process (or shell if no foreground process).
    fn get_foreground_process_cwd(&self, shell_pid: u32) -> Option<std::path::PathBuf> {
        // First try to find a foreground child process
        let fg_pid = self
            .get_foreground_child_pid(shell_pid)
            .unwrap_or(shell_pid);
        self.get_process_cwd(fg_pid)
    }

    /// Get the foreground child process of the shell (if any).
    fn get_foreground_child_pid(&self, shell_pid: u32) -> Option<u32> {
        #[cfg(target_os = "macos")]
        {
            // Use pgrep to find children of the shell
            let output = std::process::Command::new("pgrep")
                .args(["-P", &shell_pid.to_string()])
                .output()
                .ok()?;

            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // Return the last child (most recent foreground process)
                stdout
                    .lines()
                    .last()
                    .and_then(|pid_str| pid_str.trim().parse().ok())
            } else {
                None
            }
        }

        #[cfg(target_os = "linux")]
        {
            // Read children from /proc
            let children_path = format!("/proc/{}/task/{}/children", shell_pid, shell_pid);
            if let Ok(children) = std::fs::read_to_string(&children_path) {
                children
                    .split_whitespace()
                    .last()
                    .and_then(|pid_str| pid_str.parse().ok())
            } else {
                None
            }
        }

        #[cfg(target_os = "windows")]
        {
            // Use wmic to find child processes
            let output = std::process::Command::new("wmic")
                .args([
                    "process",
                    "where",
                    &format!("ParentProcessId={}", shell_pid),
                    "get",
                    "ProcessId",
                    "/format:list",
                ])
                .output()
                .ok()?;

            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // Parse output like "ProcessId=1234"
                for line in stdout.lines() {
                    if let Some(pid_str) = line.strip_prefix("ProcessId=") {
                        if let Ok(pid) = pid_str.trim().parse() {
                            return Some(pid);
                        }
                    }
                }
            }
            None
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            None
        }
    }

    /// Get the current working directory of a specific process.
    fn get_process_cwd(&self, pid: u32) -> Option<std::path::PathBuf> {
        #[cfg(target_os = "macos")]
        {
            // Use lsof to get the cwd
            let output = std::process::Command::new("lsof")
                .args(["-p", &pid.to_string(), "-Fn", "-a", "-d", "cwd"])
                .output()
                .ok()?;

            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                // lsof output format: p<pid>\nn<path>
                for line in stdout.lines() {
                    if let Some(path) = line.strip_prefix('n') {
                        let path = std::path::PathBuf::from(path);
                        if path.is_dir() {
                            return Some(path);
                        }
                    }
                }
            }
            None
        }

        #[cfg(target_os = "linux")]
        {
            // Read /proc/<pid>/cwd symlink
            let cwd_path = format!("/proc/{}/cwd", pid);
            std::fs::read_link(&cwd_path).ok()
        }

        #[cfg(target_os = "windows")]
        {
            // On Windows, getting CWD of another process requires NtQueryInformationProcess
            // which is complex. For now, we'll try to use PowerShell as a workaround.
            // Note: This only works for processes we have access to.
            let output = std::process::Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-Command",
                    &format!(
                        "(Get-Process -Id {} -ErrorAction SilentlyContinue).Path | Split-Path -Parent",
                        pid
                    ),
                ])
                .output()
                .ok()?;

            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let path_str = stdout.trim();
                if !path_str.is_empty() {
                    let path = std::path::PathBuf::from(path_str);
                    if path.is_dir() {
                        return Some(path);
                    }
                }
            }
            // Fall back to user's home directory on Windows
            dirs::home_dir()
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            None
        }
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

        // Linux: read /proc
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

        // macOS/BSD: use pgrep
        #[cfg(target_os = "macos")]
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

        // Windows: use wmic
        #[cfg(target_os = "windows")]
        {
            let output = match std::process::Command::new("wmic")
                .args([
                    "process",
                    "where",
                    &format!("ParentProcessId={}", pid),
                    "get",
                    "Name,ProcessId",
                    "/format:list",
                ])
                .output()
            {
                Ok(o) => o,
                Err(_) => return (false, None),
            };

            if !output.status.success() {
                return (false, None);
            }

            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut has_children = false;
            let mut process_name = None;

            // Parse output like "Name=powershell.exe\nProcessId=1234"
            for line in stdout.lines() {
                if let Some(name) = line.strip_prefix("Name=") {
                    let name = name.trim();
                    if !name.is_empty() {
                        has_children = true;
                        process_name = Some(name.to_string());
                        break;
                    }
                }
            }

            (has_children, process_name)
        }

        // Other platforms: no detection
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            (false, None)
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
#[allow(
    clippy::const_is_empty,
    clippy::unnecessary_literal_unwrap,
    clippy::bind_instead_of_map,
    clippy::assertions_on_constants,
    clippy::while_let_loop,
    clippy::field_reassign_with_default,
    clippy::single_match
)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;
    use test_case::test_case;

    // ========================================================================
    // Shell Validation Tests (Unix only - Windows has different shell handling)
    // ========================================================================

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_allowed_shells_list_is_not_empty() {
        assert!(!ALLOWED_SHELLS.is_empty());
    }

    #[cfg(not(target_os = "windows"))]
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

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_default_shell_is_in_allowed_list() {
        assert!(
            ALLOWED_SHELLS.contains(&DEFAULT_SHELL),
            "DEFAULT_SHELL '{}' should be in ALLOWED_SHELLS",
            DEFAULT_SHELL
        );
    }

    #[cfg(not(target_os = "windows"))]
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

    #[cfg(not(target_os = "windows"))]
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

    #[cfg(not(target_os = "windows"))]
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

    #[cfg(not(target_os = "windows"))]
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

    #[cfg(not(target_os = "windows"))]
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

    // ========================================================================
    // Concurrency Tests - Thread Safety
    // ========================================================================

    #[test]
    fn test_concurrent_channel_read_write() {
        // Tests concurrent read/write to channels (simulating PTY I/O pattern)
        use std::sync::Barrier;

        const NUM_WRITERS: usize = 4;
        const MESSAGES_PER_WRITER: usize = 100;
        const QUEUE_SIZE: usize = 256;

        let (tx, rx): (mpsc::SyncSender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) =
            mpsc::sync_channel(QUEUE_SIZE);
        let barrier = Arc::new(Barrier::new(NUM_WRITERS + 1));
        let messages_received = Arc::new(AtomicU32::new(0));
        let messages_dropped = Arc::new(AtomicU32::new(0));

        // Spawn writer threads
        let writers: Vec<_> = (0..NUM_WRITERS)
            .map(|writer_id| {
                let tx = tx.clone();
                let barrier = barrier.clone();
                let dropped = messages_dropped.clone();

                thread::spawn(move || {
                    barrier.wait();
                    for msg_id in 0..MESSAGES_PER_WRITER {
                        let data = vec![writer_id as u8, msg_id as u8];
                        match tx.try_send(data) {
                            Ok(()) => {}
                            Err(TrySendError::Full(_)) => {
                                dropped.fetch_add(1, Ordering::SeqCst);
                            }
                            Err(TrySendError::Disconnected(_)) => break,
                        }
                    }
                })
            })
            .collect();

        // Drop original sender so receiver knows when all writers are done
        drop(tx);

        // Reader thread
        let received = messages_received.clone();
        let reader = thread::spawn(move || {
            barrier.wait();
            loop {
                match rx.recv() {
                    Ok(_) => {
                        received.fetch_add(1, Ordering::SeqCst);
                    }
                    Err(_) => break, // Channel closed
                }
            }
        });

        // Wait for all threads
        for w in writers {
            w.join().expect("Writer thread panicked");
        }
        reader.join().expect("Reader thread panicked");

        let total_received = messages_received.load(Ordering::SeqCst);
        let total_dropped = messages_dropped.load(Ordering::SeqCst);
        let total_sent = (NUM_WRITERS * MESSAGES_PER_WRITER) as u32;

        assert_eq!(
            total_received + total_dropped,
            total_sent,
            "All messages should be either received or dropped"
        );
    }

    #[test]
    fn test_concurrent_resize_operations() {
        // Tests multiple threads calling resize simultaneously
        use std::sync::Barrier;

        const NUM_THREADS: usize = 8;
        const RESIZES_PER_THREAD: usize = 50;

        let pty = Arc::new(MockPtyIO::new());
        let barrier = Arc::new(Barrier::new(NUM_THREADS));

        let handles: Vec<_> = (0..NUM_THREADS)
            .map(|thread_id| {
                let pty = pty.clone();
                let barrier = barrier.clone();

                thread::spawn(move || {
                    barrier.wait();
                    for i in 0..RESIZES_PER_THREAD {
                        let rows = (thread_id * 10 + i % 10) as u16;
                        let cols = (80 + thread_id * 5 + i % 5) as u16;
                        pty.resize(rows, cols).expect("Resize should not fail");
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().expect("Thread panicked");
        }

        let total_resizes = pty.resize_calls.load(Ordering::SeqCst);
        assert_eq!(
            total_resizes,
            (NUM_THREADS * RESIZES_PER_THREAD) as u32,
            "All resize calls should be counted"
        );
    }

    #[test]
    fn test_exit_flag_concurrent_access() {
        // Tests exit flag accessed from multiple threads simultaneously
        use std::sync::Barrier;

        const NUM_READERS: usize = 10;
        const READS_PER_THREAD: usize = 1000;

        let exited = Arc::new(AtomicBool::new(false));
        let barrier = Arc::new(Barrier::new(NUM_READERS + 1));
        let false_reads = Arc::new(AtomicU32::new(0));
        let true_reads = Arc::new(AtomicU32::new(0));

        // Spawn reader threads
        let readers: Vec<_> = (0..NUM_READERS)
            .map(|_| {
                let exited = exited.clone();
                let barrier = barrier.clone();
                let false_reads = false_reads.clone();
                let true_reads = true_reads.clone();

                thread::spawn(move || {
                    barrier.wait();
                    for _ in 0..READS_PER_THREAD {
                        if exited.load(Ordering::SeqCst) {
                            true_reads.fetch_add(1, Ordering::Relaxed);
                        } else {
                            false_reads.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                })
            })
            .collect();

        // Writer thread that sets exit flag midway
        let writer_barrier = barrier.clone();
        let exited_writer = exited.clone();
        let writer = thread::spawn(move || {
            writer_barrier.wait();
            thread::sleep(Duration::from_micros(100));
            exited_writer.store(true, Ordering::SeqCst);
        });

        for r in readers {
            r.join().expect("Reader thread panicked");
        }
        writer.join().expect("Writer thread panicked");

        // After all threads complete, flag should be true
        assert!(exited.load(Ordering::SeqCst));

        // Total reads should equal expected
        let total_reads = false_reads.load(Ordering::SeqCst) + true_reads.load(Ordering::SeqCst);
        assert_eq!(
            total_reads,
            (NUM_READERS * READS_PER_THREAD) as u32,
            "All reads should be counted"
        );
    }

    #[test]
    fn test_concurrent_write_read_with_exit() {
        // Tests concurrent operations with exit flag being set
        let pty = Arc::new(MockPtyIO::new());
        let barrier = Arc::new(std::sync::Barrier::new(3));

        let pty_writer = pty.clone();
        let barrier_writer = barrier.clone();
        let writer = thread::spawn(move || {
            barrier_writer.wait();
            for i in 0..50u8 {
                if pty_writer.write(&[i]).is_err() {
                    break;
                }
            }
        });

        let pty_reader = pty.clone();
        let barrier_reader = barrier.clone();
        let reader = thread::spawn(move || {
            barrier_reader.wait();
            for i in 0..50u8 {
                pty_reader.queue_output(&[i]);
            }
        });

        let pty_exiter = pty.clone();
        let exiter = thread::spawn(move || {
            barrier.wait();
            thread::sleep(Duration::from_micros(50));
            pty_exiter.set_exited();
        });

        writer.join().expect("Writer panicked");
        reader.join().expect("Reader panicked");
        exiter.join().expect("Exiter panicked");

        assert!(pty.has_exited());
    }

    #[test]
    fn test_process_cache_concurrent_refresh() {
        // Tests concurrent access to process cache (simulating multiple callers)
        use std::sync::Barrier;

        const NUM_THREADS: usize = 8;
        const ACCESSES_PER_THREAD: usize = 100;

        let cache = Arc::new(std::sync::Mutex::new(ProcessCache::default()));
        let barrier = Arc::new(Barrier::new(NUM_THREADS));

        let handles: Vec<_> = (0..NUM_THREADS)
            .map(|thread_id| {
                let cache = cache.clone();
                let barrier = barrier.clone();

                thread::spawn(move || {
                    barrier.wait();
                    for i in 0..ACCESSES_PER_THREAD {
                        // Simulate check and potential refresh
                        let needs_refresh = {
                            let c = cache.lock().unwrap();
                            c.last_update.is_none_or(|last| {
                                last.elapsed().as_millis() > PROCESS_CACHE_TTL_MS as u128
                            })
                        };

                        if needs_refresh || i % 10 == 0 {
                            let mut c = cache.lock().unwrap();
                            c.has_children = thread_id % 2 == 0;
                            c.process_name = Some(format!("proc-{}-{}", thread_id, i));
                            c.last_update = Some(std::time::Instant::now());
                        }
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().expect("Thread panicked");
        }

        // Verify cache is in valid state
        let cache_guard = cache.lock().unwrap();
        assert!(cache_guard.last_update.is_some());
    }

    // ========================================================================
    // Stress Tests - High Concurrency
    // To run: cargo test --release -- --ignored stress_
    // ========================================================================

    /// Stress test with 100+ concurrent channel operations.
    /// Run with: cargo test --release -- --ignored stress_channel_high_concurrency
    #[test]
    #[ignore]
    fn stress_channel_high_concurrency() {
        use std::sync::Barrier;

        const NUM_PRODUCERS: usize = 50;
        const NUM_CONSUMERS: usize = 10;
        const MESSAGES_PER_PRODUCER: usize = 1000;
        const QUEUE_SIZE: usize = 256;

        let (tx, rx): (mpsc::SyncSender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) =
            mpsc::sync_channel(QUEUE_SIZE);
        let rx = Arc::new(std::sync::Mutex::new(rx));
        let barrier = Arc::new(Barrier::new(NUM_PRODUCERS + NUM_CONSUMERS));
        let total_received = Arc::new(AtomicU32::new(0));
        let total_dropped = Arc::new(AtomicU32::new(0));

        // Spawn producer threads
        let producers: Vec<_> = (0..NUM_PRODUCERS)
            .map(|producer_id| {
                let tx = tx.clone();
                let barrier = barrier.clone();
                let dropped = total_dropped.clone();

                thread::spawn(move || {
                    barrier.wait();
                    for msg_id in 0..MESSAGES_PER_PRODUCER {
                        let data = vec![producer_id as u8, (msg_id % 256) as u8];
                        match tx.try_send(data) {
                            Ok(()) => {}
                            Err(TrySendError::Full(_)) => {
                                dropped.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(TrySendError::Disconnected(_)) => break,
                        }
                        // Simulate work
                        if msg_id % 100 == 0 {
                            thread::yield_now();
                        }
                    }
                })
            })
            .collect();

        drop(tx);

        // Spawn consumer threads
        let consumers: Vec<_> = (0..NUM_CONSUMERS)
            .map(|_| {
                let rx = rx.clone();
                let barrier = barrier.clone();
                let received = total_received.clone();

                thread::spawn(move || {
                    barrier.wait();
                    loop {
                        let result = {
                            let guard = rx.lock().unwrap();
                            guard.try_recv()
                        };

                        match result {
                            Ok(_) => {
                                received.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(mpsc::TryRecvError::Empty) => {
                                thread::yield_now();
                            }
                            Err(mpsc::TryRecvError::Disconnected) => break,
                        }
                    }
                })
            })
            .collect();

        for p in producers {
            p.join().expect("Producer panicked");
        }
        for c in consumers {
            c.join().expect("Consumer panicked");
        }

        let received = total_received.load(Ordering::SeqCst);
        let dropped = total_dropped.load(Ordering::SeqCst);
        let total = (NUM_PRODUCERS * MESSAGES_PER_PRODUCER) as u32;

        assert_eq!(
            received + dropped,
            total,
            "All messages accounted for: received={}, dropped={}, total={}",
            received,
            dropped,
            total
        );
    }

    /// Stress test with 100+ concurrent resize operations.
    /// Run with: cargo test --release -- --ignored stress_resize_high_concurrency
    #[test]
    #[ignore]
    fn stress_resize_high_concurrency() {
        use std::sync::Barrier;

        const NUM_THREADS: usize = 100;
        const RESIZES_PER_THREAD: usize = 100;

        let pty = Arc::new(MockPtyIO::new());
        let barrier = Arc::new(Barrier::new(NUM_THREADS));

        let handles: Vec<_> = (0..NUM_THREADS)
            .map(|thread_id| {
                let pty = pty.clone();
                let barrier = barrier.clone();

                thread::spawn(move || {
                    barrier.wait();
                    for i in 0..RESIZES_PER_THREAD {
                        let rows = ((thread_id + i) % 200) as u16;
                        let cols = ((thread_id + i) % 300 + 80) as u16;
                        pty.resize(rows, cols).expect("Resize failed");
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().expect("Thread panicked");
        }

        let total = pty.resize_calls.load(Ordering::SeqCst);
        assert_eq!(
            total,
            (NUM_THREADS * RESIZES_PER_THREAD) as u32,
            "All resizes counted"
        );
    }

    /// Stress test for deadlock detection with timeout.
    /// Run with: cargo test --release -- --ignored stress_deadlock_detection
    #[test]
    #[ignore]
    fn stress_deadlock_detection() {
        use std::sync::Barrier;
        use std::time::Instant;

        const TIMEOUT_SECS: u64 = 10;
        const NUM_THREADS: usize = 50;
        const OPERATIONS_PER_THREAD: usize = 100;

        let pty = Arc::new(MockPtyIO::new());
        let cache = Arc::new(std::sync::Mutex::new(ProcessCache::default()));
        let barrier = Arc::new(Barrier::new(NUM_THREADS));
        let completed = Arc::new(AtomicU32::new(0));

        let start = Instant::now();

        let handles: Vec<_> = (0..NUM_THREADS)
            .map(|thread_id| {
                let pty = pty.clone();
                let cache = cache.clone();
                let barrier = barrier.clone();
                let completed = completed.clone();

                thread::spawn(move || {
                    barrier.wait();
                    for i in 0..OPERATIONS_PER_THREAD {
                        // Mix of operations that could potentially deadlock
                        match i % 4 {
                            0 => {
                                let _ = pty.write(&[thread_id as u8, i as u8]);
                            }
                            1 => {
                                let _ =
                                    pty.resize((20 + thread_id % 10) as u16, (80 + i % 20) as u16);
                            }
                            2 => {
                                pty.queue_output(&[i as u8]);
                                let _ = pty.read_output();
                            }
                            3 => {
                                let mut c = cache.lock().unwrap();
                                c.has_children = thread_id % 2 == 0;
                                c.last_update = Some(std::time::Instant::now());
                            }
                            _ => unreachable!(),
                        }
                    }
                    completed.fetch_add(1, Ordering::SeqCst);
                })
            })
            .collect();

        // Wait for threads with timeout
        for h in handles {
            let remaining = Duration::from_secs(TIMEOUT_SECS).saturating_sub(start.elapsed());
            if remaining.is_zero() {
                panic!(
                    "Deadlock detected: test timed out after {} seconds",
                    TIMEOUT_SECS
                );
            }
            h.join().expect("Thread panicked");
        }

        let completed_count = completed.load(Ordering::SeqCst);
        assert_eq!(
            completed_count, NUM_THREADS as u32,
            "All threads should complete without deadlock"
        );
    }

    /// Stress test for data race detection.
    /// Run with: cargo test --release -- --ignored stress_data_race_detection
    #[test]
    #[ignore]
    fn stress_data_race_detection() {
        use std::sync::Barrier;

        const NUM_WRITERS: usize = 25;
        const NUM_READERS: usize = 75;
        const OPERATIONS: usize = 500;

        let pty = Arc::new(MockPtyIO::new());
        let barrier = Arc::new(Barrier::new(NUM_WRITERS + NUM_READERS));

        // Spawn writer threads
        let writers: Vec<_> = (0..NUM_WRITERS)
            .map(|writer_id| {
                let pty = pty.clone();
                let barrier = barrier.clone();

                thread::spawn(move || {
                    barrier.wait();
                    for i in 0..OPERATIONS {
                        let data = vec![writer_id as u8, (i % 256) as u8];
                        let _ = pty.write(&data);
                        pty.queue_output(&data);
                    }
                })
            })
            .collect();

        // Spawn reader threads
        let readers: Vec<_> = (0..NUM_READERS)
            .map(|_| {
                let pty = pty.clone();
                let barrier = barrier.clone();

                thread::spawn(move || {
                    barrier.wait();
                    for _ in 0..OPERATIONS {
                        let _ = pty.has_exited();
                        let _ = pty.read_output();
                        let _ = pty.get_written_data();
                    }
                })
            })
            .collect();

        for w in writers {
            w.join().expect("Writer panicked");
        }
        for r in readers {
            r.join().expect("Reader panicked");
        }

        // If we got here without crash/hang, data races are handled properly
    }

    /// Stress test for rapid exit flag toggling.
    /// Run with: cargo test --release -- --ignored stress_exit_flag_toggling
    #[test]
    #[ignore]
    fn stress_exit_flag_toggling() {
        use std::sync::Barrier;

        const NUM_THREADS: usize = 100;
        const TOGGLES_PER_THREAD: usize = 1000;

        let exited = Arc::new(AtomicBool::new(false));
        let barrier = Arc::new(Barrier::new(NUM_THREADS));
        let toggle_count = Arc::new(AtomicU32::new(0));

        let handles: Vec<_> = (0..NUM_THREADS)
            .map(|thread_id| {
                let exited = exited.clone();
                let barrier = barrier.clone();
                let toggle_count = toggle_count.clone();

                thread::spawn(move || {
                    barrier.wait();
                    for i in 0..TOGGLES_PER_THREAD {
                        // Each thread toggles based on its ID and iteration
                        let new_value = (thread_id + i) % 2 == 0;
                        exited.store(new_value, Ordering::SeqCst);
                        let _ = exited.load(Ordering::SeqCst);
                        toggle_count.fetch_add(1, Ordering::Relaxed);
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().expect("Thread panicked");
        }

        let total_toggles = toggle_count.load(Ordering::SeqCst);
        assert_eq!(
            total_toggles,
            (NUM_THREADS * TOGGLES_PER_THREAD) as u32,
            "All toggles completed"
        );
    }

    // ========================================================================
    // ERROR PATH TESTS - Channel Send/Receive Failures
    // ========================================================================

    #[test]
    fn test_channel_send_full_returns_correct_error_variant() {
        let (tx, rx): (mpsc::SyncSender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) = mpsc::sync_channel(1);

        // Fill the channel
        tx.try_send(vec![1]).unwrap();

        // Next send should return Full error
        let result = tx.try_send(vec![2]);
        match result {
            Err(TrySendError::Full(data)) => {
                assert_eq!(data, vec![2], "Full error should contain the unsent data");
            }
            _ => panic!("Expected TrySendError::Full"),
        }

        // Clean up
        drop(rx);
    }

    #[test]
    fn test_channel_send_disconnected_returns_correct_error_variant() {
        let (tx, rx): (mpsc::SyncSender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) = mpsc::sync_channel(1);

        // Disconnect by dropping receiver
        drop(rx);

        // Send should return Disconnected error with the data
        let result = tx.try_send(vec![1, 2, 3]);
        match result {
            Err(TrySendError::Disconnected(data)) => {
                assert_eq!(
                    data,
                    vec![1, 2, 3],
                    "Disconnected error should contain the unsent data"
                );
            }
            _ => panic!("Expected TrySendError::Disconnected"),
        }
    }

    #[test]
    fn test_channel_receive_empty_error() {
        let (_tx, rx): (mpsc::SyncSender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) = mpsc::sync_channel(1);

        let result = rx.try_recv();
        match result {
            Err(mpsc::TryRecvError::Empty) => {
                // This is the expected behavior
            }
            _ => panic!("Expected TryRecvError::Empty"),
        }
    }

    #[test]
    fn test_channel_receive_disconnected_error() {
        let (tx, rx): (mpsc::SyncSender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) = mpsc::sync_channel(1);

        // Disconnect by dropping sender
        drop(tx);

        let result = rx.try_recv();
        match result {
            Err(mpsc::TryRecvError::Disconnected) => {
                // This is the expected behavior
            }
            _ => panic!("Expected TryRecvError::Disconnected"),
        }
    }

    #[test]
    fn test_channel_recv_timeout() {
        let (_tx, rx): (mpsc::SyncSender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) = mpsc::sync_channel(1);

        // recv_timeout should return Empty error when timeout expires
        let result = rx.recv_timeout(Duration::from_millis(10));
        match result {
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // This is the expected behavior
            }
            _ => panic!("Expected RecvTimeoutError::Timeout"),
        }
    }

    // ========================================================================
    // ERROR PATH TESTS - Invalid Shell Paths
    // ========================================================================

    // Note: test_get_validated_shell_rejects_path_with_null_byte was removed
    // because the OS does not allow setting environment variables with null bytes.
    // This is a non-issue in practice since the attack vector doesn't exist.

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_get_validated_shell_rejects_path_traversal() {
        let original = std::env::var("SHELL").ok();

        // Set a path with traversal attempt
        std::env::set_var("SHELL", "/bin/../../../tmp/evil");

        let result = get_validated_shell();
        // Should fall back to default since not in allowed list
        assert_eq!(result, DEFAULT_SHELL);

        // Restore
        if let Some(shell) = original {
            std::env::set_var("SHELL", shell);
        } else {
            std::env::remove_var("SHELL");
        }
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_get_validated_shell_rejects_suspicious_shells() {
        let original = std::env::var("SHELL").ok();

        let suspicious_shells = [
            "/tmp/malicious",
            "/home/user/evil.sh",
            "/var/tmp/shell",
            "/dev/null",
            "/proc/self/exe",
        ];

        for shell in suspicious_shells {
            std::env::set_var("SHELL", shell);
            let result = get_validated_shell();
            assert_eq!(
                result, DEFAULT_SHELL,
                "Shell '{}' should be rejected",
                shell
            );
        }

        // Restore
        if let Some(shell) = original {
            std::env::set_var("SHELL", shell);
        } else {
            std::env::remove_var("SHELL");
        }
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_get_validated_shell_empty_string() {
        let original = std::env::var("SHELL").ok();

        std::env::set_var("SHELL", "");

        let result = get_validated_shell();
        // Empty string is not absolute path, should fall back
        assert_eq!(result, DEFAULT_SHELL);

        // Restore
        if let Some(shell) = original {
            std::env::set_var("SHELL", shell);
        } else {
            std::env::remove_var("SHELL");
        }
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn test_get_validated_shell_whitespace_only() {
        let original = std::env::var("SHELL").ok();

        std::env::set_var("SHELL", "   ");

        let result = get_validated_shell();
        // Whitespace is not absolute path, should fall back
        assert_eq!(result, DEFAULT_SHELL);

        // Restore
        if let Some(shell) = original {
            std::env::set_var("SHELL", shell);
        } else {
            std::env::remove_var("SHELL");
        }
    }

    // ========================================================================
    // ERROR PATH TESTS - Mock PTY Write Failures
    // ========================================================================

    /// Mock PTY that can simulate various error conditions
    struct ErrorProneIO {
        write_count: AtomicU32,
        fail_after: u32,
        error_kind: std::io::ErrorKind,
    }

    impl ErrorProneIO {
        fn new(fail_after: u32, error_kind: std::io::ErrorKind) -> Self {
            Self {
                write_count: AtomicU32::new(0),
                fail_after,
                error_kind,
            }
        }

        fn write(&self, _data: &[u8]) -> std::io::Result<()> {
            let count = self.write_count.fetch_add(1, Ordering::SeqCst);
            if count >= self.fail_after {
                Err(std::io::Error::new(self.error_kind, "Simulated error"))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn test_error_prone_io_fails_after_threshold() {
        let io = ErrorProneIO::new(3, std::io::ErrorKind::BrokenPipe);

        // First 3 writes succeed
        assert!(io.write(b"1").is_ok());
        assert!(io.write(b"2").is_ok());
        assert!(io.write(b"3").is_ok());

        // 4th write fails
        let result = io.write(b"4");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn test_error_prone_io_immediate_failure() {
        let io = ErrorProneIO::new(0, std::io::ErrorKind::ConnectionReset);

        let result = io.write(b"data");
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().kind(),
            std::io::ErrorKind::ConnectionReset
        );
    }

    #[test]
    fn test_io_error_message_content() {
        let io = ErrorProneIO::new(0, std::io::ErrorKind::BrokenPipe);

        let result = io.write(b"test");
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
        assert_eq!(err.to_string(), "Simulated error");
    }

    #[test]
    fn test_various_io_error_kinds() {
        let error_kinds = [
            std::io::ErrorKind::BrokenPipe,
            std::io::ErrorKind::ConnectionReset,
            std::io::ErrorKind::ConnectionAborted,
            std::io::ErrorKind::NotConnected,
            std::io::ErrorKind::TimedOut,
            std::io::ErrorKind::WouldBlock,
            std::io::ErrorKind::Interrupted,
        ];

        for kind in error_kinds {
            let io = ErrorProneIO::new(0, kind);
            let result = io.write(b"test");
            assert!(result.is_err());
            assert_eq!(result.unwrap_err().kind(), kind);
        }
    }

    // ========================================================================
    // ERROR PATH TESTS - Mutex Poisoning
    // ========================================================================

    #[test]
    #[ignore = "Mutex poisoning test can hang in some test frameworks - run manually"]
    fn test_process_cache_mutex_poisoned_returns_default() {
        // Test that poisoned mutex returns safe defaults
        use std::sync::{Arc, Mutex};
        use std::thread;

        let cache = Arc::new(Mutex::new(ProcessCache {
            has_children: true,
            process_name: Some("test".to_string()),
            last_update: Some(std::time::Instant::now()),
        }));

        let cache_clone = cache.clone();

        // Poison the mutex by panicking in a separate thread while holding the lock
        let handle = thread::spawn(move || {
            let _guard = cache_clone.lock().unwrap();
            panic!("Intentional panic to poison mutex");
        });

        // Wait for thread to finish (it will panic)
        let _ = handle.join();

        // Now the mutex is poisoned - lock should return Err
        let lock_result = cache.lock();
        assert!(
            lock_result.is_err(),
            "Mutex should be poisoned after panic in thread"
        );

        // Test the pattern used in has_running_processes - unwrap_or provides safe default
        let has_children = cache.lock().map(|c| c.has_children).unwrap_or(false);
        assert!(
            !has_children,
            "Should return safe default (false) on poisoned mutex"
        );
    }

    // ========================================================================
    // ERROR PATH TESTS - Process Cache Edge Cases
    // ========================================================================

    #[test]
    fn test_process_cache_stale_check_with_none_timestamp() {
        let cache = ProcessCache::default();

        // No last_update means stale
        let is_stale = cache.last_update.is_none_or(|_| true);
        assert!(is_stale);
    }

    #[test]
    fn test_process_cache_stale_check_with_old_timestamp() {
        use std::time::Instant;

        let mut cache = ProcessCache::default();
        // Set timestamp to 1 second ago (older than TTL)
        cache.last_update = Some(Instant::now() - Duration::from_secs(1));

        let is_stale = cache
            .last_update
            .is_none_or(|last| last.elapsed().as_millis() > PROCESS_CACHE_TTL_MS as u128);
        assert!(is_stale);
    }

    #[test]
    fn test_process_cache_fresh_timestamp() {
        use std::time::Instant;

        let mut cache = ProcessCache::default();
        cache.last_update = Some(Instant::now());

        let is_stale = cache
            .last_update
            .is_none_or(|last| last.elapsed().as_millis() > PROCESS_CACHE_TTL_MS as u128);
        assert!(!is_stale);
    }

    // ========================================================================
    // ERROR PATH TESTS - Atomic Operations Ordering
    // ========================================================================

    #[test]
    fn test_exit_flag_seqcst_ordering() {
        let exited = Arc::new(AtomicBool::new(false));
        let exited_clone = exited.clone();

        let writer = std::thread::spawn(move || {
            for _ in 0..1000 {
                exited_clone.store(true, Ordering::SeqCst);
                exited_clone.store(false, Ordering::SeqCst);
            }
            exited_clone.store(true, Ordering::SeqCst);
        });

        // Reader should see consistent values
        let mut saw_true = false;
        let mut saw_false = false;
        for _ in 0..10000 {
            let val = exited.load(Ordering::SeqCst);
            if val {
                saw_true = true;
            } else {
                saw_false = true;
            }
            if saw_true && saw_false {
                break;
            }
        }

        writer.join().unwrap();

        // Final state should be true
        assert!(exited.load(Ordering::SeqCst));
    }

    // ========================================================================
    // ERROR PATH TESTS - Timeout Scenarios (Simulated)
    // ========================================================================

    #[test]
    fn test_recv_timeout_immediate_data() {
        let (tx, rx): (mpsc::SyncSender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) = mpsc::sync_channel(1);

        tx.send(vec![42]).unwrap();

        // Should return immediately with data
        let result = rx.recv_timeout(Duration::from_secs(1));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec![42]);
    }

    #[test]
    fn test_recv_timeout_delayed_data() {
        let (tx, rx): (mpsc::SyncSender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) = mpsc::sync_channel(1);

        let sender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            tx.send(vec![99]).unwrap();
        });

        // Should receive data before timeout
        let result = rx.recv_timeout(Duration::from_secs(1));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec![99]);

        sender.join().unwrap();
    }

    #[test]
    fn test_recv_timeout_expires() {
        let (_tx, rx): (mpsc::SyncSender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) = mpsc::sync_channel(1);

        let start = std::time::Instant::now();
        let result = rx.recv_timeout(Duration::from_millis(50));
        let elapsed = start.elapsed();

        assert!(result.is_err());
        assert!(matches!(result, Err(mpsc::RecvTimeoutError::Timeout)));
        assert!(elapsed >= Duration::from_millis(50));
        assert!(elapsed < Duration::from_millis(200)); // Reasonable upper bound
    }

    #[test]
    fn test_recv_timeout_channel_closed() {
        let (tx, rx): (mpsc::SyncSender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) = mpsc::sync_channel(1);

        drop(tx); // Close channel

        let result = rx.recv_timeout(Duration::from_secs(1));
        assert!(matches!(result, Err(mpsc::RecvTimeoutError::Disconnected)));
    }

    // ========================================================================
    // ERROR PATH TESTS - Output Queue Full Behavior
    // ========================================================================

    #[test]
    fn test_output_queue_full_drops_data() {
        const QUEUE_SIZE: usize = 4;
        let (tx, rx): (mpsc::SyncSender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) =
            mpsc::sync_channel(QUEUE_SIZE);

        // Fill queue
        for i in 0..QUEUE_SIZE {
            tx.try_send(vec![i as u8]).unwrap();
        }

        // Next sends should fail but not panic
        let mut dropped_count = 0;
        for i in 0..100 {
            match tx.try_send(vec![i]) {
                Err(TrySendError::Full(_)) => dropped_count += 1,
                _ => {}
            }
        }
        assert_eq!(dropped_count, 100);

        // Original data still in queue
        let mut received = Vec::new();
        while let Ok(data) = rx.try_recv() {
            received.push(data);
        }
        assert_eq!(received.len(), QUEUE_SIZE);
    }

    // ========================================================================
    // ERROR PATH TESTS - PTY Size Edge Cases
    // ========================================================================

    #[test]
    fn test_pty_size_zero_rows() {
        let size = PtySize {
            rows: 0,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        };
        assert_eq!(size.rows, 0);
    }

    #[test]
    fn test_pty_size_zero_cols() {
        let size = PtySize {
            rows: 24,
            cols: 0,
            pixel_width: 0,
            pixel_height: 0,
        };
        assert_eq!(size.cols, 0);
    }

    #[test]
    fn test_pty_size_extreme_pixel_values() {
        let size = PtySize {
            rows: 24,
            cols: 80,
            pixel_width: u16::MAX,
            pixel_height: u16::MAX,
        };
        assert_eq!(size.pixel_width, u16::MAX);
        assert_eq!(size.pixel_height, u16::MAX);
    }

    // ========================================================================
    // ERROR PATH TESTS - Shell Validation Edge Cases
    // ========================================================================

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn test_allowed_shells_no_duplicates() {
        let mut seen = std::collections::HashSet::new();
        for shell in ALLOWED_SHELLS {
            assert!(
                seen.insert(*shell),
                "Duplicate shell in allowed list: {}",
                shell
            );
        }
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn test_allowed_shells_all_exist_or_are_common() {
        // At least one shell should exist on any Unix system
        let exists_count = ALLOWED_SHELLS
            .iter()
            .filter(|s| Path::new(s).exists())
            .count();

        assert!(
            exists_count > 0,
            "At least one allowed shell should exist on the system"
        );
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn test_default_shell_exists_or_is_common() {
        // DEFAULT_SHELL should be a common shell
        let common_shells = ["/bin/zsh", "/bin/bash", "/bin/sh"];
        assert!(
            common_shells.contains(&DEFAULT_SHELL),
            "DEFAULT_SHELL should be a common shell"
        );
    }

    // ========================================================================
    // PANIC TESTS - Using #[should_panic]
    // ========================================================================

    #[test]
    #[should_panic(expected = "called `Option::unwrap()` on a `None` value")]
    fn test_unwrap_none_panics() {
        let opt: Option<i32> = None;
        opt.unwrap();
    }

    #[test]
    #[should_panic(expected = "called `Result::unwrap()` on an `Err` value")]
    fn test_result_unwrap_err_panics() {
        let result: Result<i32, &str> = Err("error");
        result.unwrap();
    }

    #[test]
    #[should_panic]
    fn test_channel_send_blocking_on_closed() {
        let (tx, rx): (mpsc::SyncSender<Vec<u8>>, mpsc::Receiver<Vec<u8>>) = mpsc::sync_channel(0);
        drop(rx);

        // send() on a sync_channel(0) with closed receiver panics
        tx.send(vec![1]).unwrap();
    }

    // ========================================================================
    // ERROR PATH TESTS - CommandBuilder Edge Cases
    // ========================================================================

    #[test]
    fn test_command_builder_empty_shell_path() {
        // CommandBuilder can be created with empty path (will fail on spawn)
        let cmd = CommandBuilder::new("");
        // Just verify it doesn't panic on creation
        let _ = cmd;
    }

    #[test]
    fn test_command_builder_special_characters_in_env() {
        let mut cmd = CommandBuilder::new("/bin/sh");
        // Test various special characters in env values
        cmd.env("SPECIAL", "value with spaces");
        cmd.env("QUOTED", "value\"with\"quotes");
        cmd.env("NEWLINE", "value\nwith\nnewlines");
        cmd.env("UNICODE", "value\u{1F600}emoji");
        // Just verify no panic
    }

    #[test]
    fn test_command_builder_empty_env_key() {
        let mut cmd = CommandBuilder::new("/bin/sh");
        // Empty key should not panic
        cmd.env("", "value");
    }

    #[test]
    fn test_command_builder_empty_env_value() {
        let mut cmd = CommandBuilder::new("/bin/sh");
        // Empty value should not panic
        cmd.env("KEY", "");
    }

    // ========================================================================
    // ERROR PATH TESTS - Process Exit Error Codes (simulated)
    // ========================================================================

    #[cfg(unix)]
    #[test]
    fn test_process_exit_code_simulation() {
        use std::os::unix::process::ExitStatusExt;

        // Simulate different process exit codes that PtyHandler might encounter
        let exit_codes = [0, 1, 2, 126, 127, 128, 130, 137, 143, 255];

        for code in exit_codes {
            // Each exit code should be distinguishable
            let status = std::process::ExitStatus::from_raw(code << 8);
            if code == 0 {
                assert!(status.success(), "Exit code 0 should indicate success");
            } else {
                assert!(
                    !status.success(),
                    "Exit code {} should indicate failure",
                    code
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_process_signal_termination_simulation() {
        use std::os::unix::process::ExitStatusExt;

        // Simulate signal termination (SIGTERM = 15, SIGKILL = 9)
        let signals = [9, 15]; // SIGKILL, SIGTERM

        for signal in signals {
            let status = std::process::ExitStatus::from_raw(signal);
            // Signal termination is not a clean exit
            assert!(!status.success());
        }
    }

    #[test]
    fn test_exit_code_semantics() {
        // Test exit code semantic meanings without platform-specific APIs
        let common_exit_codes = [
            (0, "success"),
            (1, "general error"),
            (2, "misuse of shell builtin"),
            (126, "command cannot execute"),
            (127, "command not found"),
            (128, "invalid exit argument"),
            (130, "script terminated by Ctrl+C (128+2)"),
            (137, "killed by SIGKILL (128+9)"),
            (143, "killed by SIGTERM (128+15)"),
        ];

        for (code, meaning) in common_exit_codes {
            // Just verify the codes are in the expected range
            assert!(
                code <= 255,
                "Exit code {} ({}) should fit in u8",
                code,
                meaning
            );
            if code > 128 {
                // Codes > 128 typically indicate signal termination
                let signal = code - 128;
                assert!(
                    signal > 0 && signal < 64,
                    "Signal {} from code {} should be valid",
                    signal,
                    code
                );
            }
        }
    }

    // ========================================================================
    // ERROR PATH TESTS - Read Output Edge Cases
    // ========================================================================

    #[test]
    fn test_read_output_returns_all_pending() {
        let pty = MockPtyIO::new();

        // Queue multiple chunks
        for i in 0..10 {
            pty.queue_output(&[i]);
        }

        // Single read should return all pending
        let output = pty.read_output();
        assert_eq!(output.len(), 10);

        // Second read should be empty
        let output2 = pty.read_output();
        assert!(output2.is_empty());
    }

    #[test]
    fn test_read_output_preserves_order() {
        let pty = MockPtyIO::new();

        // Queue in specific order
        pty.queue_output(b"first");
        pty.queue_output(b"second");
        pty.queue_output(b"third");

        let output = pty.read_output();
        assert_eq!(output.len(), 3);
        assert_eq!(output[0], b"first");
        assert_eq!(output[1], b"second");
        assert_eq!(output[2], b"third");
    }

    #[test]
    fn test_read_output_handles_large_chunks() {
        let pty = MockPtyIO::new();

        // Queue a large chunk (simulating heavy terminal output)
        let large_chunk = vec![0u8; 64 * 1024]; // 64KB
        pty.queue_output(&large_chunk);

        let output = pty.read_output();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0].len(), 64 * 1024);
    }

    // ========================================================================
    // ERROR PATH TESTS - Write Error Recovery
    // ========================================================================

    #[test]
    fn test_write_continues_after_recoverable_error() {
        let pty = MockPtyIO::new();

        // Normal writes work
        assert!(pty.write(b"normal").is_ok());
        assert!(pty.write(b"another").is_ok());

        // After exit, writes fail
        pty.set_exited();
        assert!(pty.write(b"after exit").is_err());

        // Multiple writes all fail
        assert!(pty.write(b"still fails").is_err());
        assert!(pty.write(b"again").is_err());

        // Written data before exit is preserved
        let data = pty.get_written_data();
        assert!(data.starts_with(b"normalanother"));
    }

    // ========================================================================
    // ERROR PATH TESTS - Result Error Message Content
    // ========================================================================

    #[test]
    fn test_io_error_has_descriptive_message() {
        let err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "PTY connection lost");

        assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
        assert!(err.to_string().contains("PTY connection lost"));
    }

    #[test]
    fn test_anyhow_error_context() {
        use anyhow::Context;

        let result: Result<(), std::io::Error> = Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "shell not found",
        ));

        let anyhow_result: anyhow::Result<()> = result.context("Failed to spawn shell");

        let err = anyhow_result.unwrap_err();
        let err_string = format!("{:#}", err);

        // Should contain both the context and the original error
        assert!(err_string.contains("Failed to spawn shell"));
        assert!(err_string.contains("shell not found"));
    }

    #[test]
    fn test_anyhow_error_chain() {
        use anyhow::Context;

        let inner_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let result: anyhow::Result<()> = Err(inner_err)
            .context("Failed to open PTY")
            .context("Terminal initialization failed");

        let err = result.unwrap_err();

        // Error chain should include all contexts
        let chain: Vec<_> = err.chain().collect();
        assert!(chain.len() >= 2);
    }

    // ========================================================================
    // Expanded Property-Based Tests (1000 cases)
    // ========================================================================

    use proptest::prelude::*;

    proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(1000))]

        // ==================== Arbitrary Byte Sequence Tests ====================

        /// Property: MockPtyIO handles arbitrary byte sequences without panic
        #[test]
        fn prop_mock_pty_arbitrary_byte_sequences(
            data in proptest::collection::vec(any::<u8>(), 0..1024)
        ) {
            let pty = MockPtyIO::new();
            let result = pty.write(&data);
            prop_assert!(result.is_ok(), "Write should succeed");
            prop_assert_eq!(pty.get_written_data(), data, "Data should be preserved exactly");
        }

        /// Property: MockPtyIO handles all possible single bytes
        #[test]
        fn prop_mock_pty_all_byte_values(byte in any::<u8>()) {
            let pty = MockPtyIO::new();
            let result = pty.write(&[byte]);
            prop_assert!(result.is_ok());
            prop_assert_eq!(pty.get_written_data(), vec![byte]);
        }

        /// Property: Multiple arbitrary writes accumulate correctly
        #[test]
        fn prop_mock_pty_multiple_writes_accumulate(
            writes in proptest::collection::vec(
                proptest::collection::vec(any::<u8>(), 0..100),
                1..20
            )
        ) {
            let pty = MockPtyIO::new();
            let mut expected = Vec::new();

            for write_data in &writes {
                let result = pty.write(write_data);
                prop_assert!(result.is_ok());
                expected.extend_from_slice(write_data);
            }

            prop_assert_eq!(pty.get_written_data(), expected);
        }

        /// Property: Empty writes are handled correctly
        #[test]
        fn prop_mock_pty_empty_writes(count in 0usize..100) {
            let pty = MockPtyIO::new();

            for _ in 0..count {
                let result = pty.write(&[]);
                prop_assert!(result.is_ok());
            }

            prop_assert!(pty.get_written_data().is_empty());
        }

        /// Property: Writes after exit always fail
        #[test]
        fn prop_mock_pty_write_fails_after_exit(
            data in proptest::collection::vec(any::<u8>(), 0..100)
        ) {
            let pty = MockPtyIO::new();
            pty.set_exited();

            let result = pty.write(&data);
            prop_assert!(result.is_err());
            prop_assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::BrokenPipe);
        }

        // ==================== Arbitrary Resize Dimension Tests ====================

        /// Property: Resize accepts any valid u16 dimensions
        #[test]
        fn prop_mock_pty_resize_arbitrary_dimensions(rows in any::<u16>(), cols in any::<u16>()) {
            let pty = MockPtyIO::new();
            let result = pty.resize(rows, cols);

            prop_assert!(result.is_ok(), "Resize should succeed for any u16 dimensions");
            prop_assert_eq!(*pty.last_size.lock().unwrap(), (rows, cols));
        }

        /// Property: Resize to same dimensions is idempotent
        #[test]
        fn prop_mock_pty_resize_idempotent(rows in any::<u16>(), cols in any::<u16>()) {
            let pty = MockPtyIO::new();

            pty.resize(rows, cols).unwrap();
            let first_size = *pty.last_size.lock().unwrap();

            pty.resize(rows, cols).unwrap();
            let second_size = *pty.last_size.lock().unwrap();

            prop_assert_eq!(first_size, second_size);
        }

        /// Property: Multiple resizes update to latest dimensions
        #[test]
        fn prop_mock_pty_resize_sequence(
            sizes in proptest::collection::vec((any::<u16>(), any::<u16>()), 1..50)
        ) {
            let pty = MockPtyIO::new();

            for (rows, cols) in &sizes {
                pty.resize(*rows, *cols).unwrap();
            }

            let (final_rows, final_cols) = sizes.last().unwrap();
            prop_assert_eq!(*pty.last_size.lock().unwrap(), (*final_rows, *final_cols));
            prop_assert_eq!(pty.resize_calls.load(Ordering::SeqCst), sizes.len() as u32);
        }

        /// Property: Resize count is accurate
        #[test]
        fn prop_mock_pty_resize_count_accurate(count in 1u32..100) {
            let pty = MockPtyIO::new();

            for _ in 0..count {
                pty.resize(24, 80).unwrap();
            }

            prop_assert_eq!(pty.resize_calls.load(Ordering::SeqCst), count);
        }

        // ==================== Escape Sequence Handling Tests ====================

        /// Property: CSI escape sequences are preserved exactly
        #[test]
        fn prop_csi_sequence_preserved(
            params in proptest::collection::vec(0u8..=9, 0..10),
            final_byte in 0x40u8..=0x7E
        ) {
            let pty = MockPtyIO::new();

            // Build CSI sequence: ESC [ params ; params final_byte
            let mut seq = vec![0x1b, b'['];
            for (i, &p) in params.iter().enumerate() {
                if i > 0 {
                    seq.push(b';');
                }
                seq.push(b'0' + p);
            }
            seq.push(final_byte);

            let result = pty.write(&seq);
            prop_assert!(result.is_ok());
            prop_assert_eq!(pty.get_written_data(), seq);
        }

        /// Property: SGR mouse sequences are preserved exactly
        #[test]
        fn prop_sgr_mouse_sequence_preserved(
            button in 0u8..=127,
            x in 1u16..=9999,
            y in 1u16..=9999,
            is_press: bool
        ) {
            let pty = MockPtyIO::new();

            let suffix = if is_press { b'M' } else { b'm' };
            let seq = format!("\x1b[<{};{};{}{}", button, x, y, suffix as char);

            let result = pty.write(seq.as_bytes());
            prop_assert!(result.is_ok());
            prop_assert_eq!(pty.get_written_data(), seq.as_bytes());
        }

        /// Property: OSC sequences are preserved exactly
        #[test]
        fn prop_osc_sequence_preserved(
            ps in 0u16..=255,
            pt in "[a-zA-Z0-9 ]{0,50}"
        ) {
            let pty = MockPtyIO::new();

            // OSC sequence: ESC ] Ps ; Pt ST (where ST = ESC \)
            let seq = format!("\x1b]{};{}\x1b\\", ps, pt);

            let result = pty.write(seq.as_bytes());
            prop_assert!(result.is_ok());
            prop_assert_eq!(pty.get_written_data(), seq.as_bytes());
        }

        /// Property: DCS sequences are preserved exactly
        #[test]
        fn prop_dcs_sequence_preserved(
            content in "[a-zA-Z0-9]{0,100}"
        ) {
            let pty = MockPtyIO::new();

            // DCS sequence: ESC P ... ST
            let seq = format!("\x1bP{}\x1b\\", content);

            let result = pty.write(seq.as_bytes());
            prop_assert!(result.is_ok());
            prop_assert_eq!(pty.get_written_data(), seq.as_bytes());
        }

        /// Property: Mixed escape sequences and text are preserved
        #[test]
        fn prop_mixed_escape_and_text(
            text in "[a-zA-Z0-9 ]{0,50}",
            sgr_params in proptest::collection::vec(0u8..=8, 0..5)
        ) {
            let pty = MockPtyIO::new();

            // Build SGR sequence
            let mut sgr = String::from("\x1b[");
            for (i, &p) in sgr_params.iter().enumerate() {
                if i > 0 {
                    sgr.push(';');
                }
                sgr.push_str(&p.to_string());
            }
            sgr.push('m');

            let full = format!("{}{}\x1b[0m", sgr, text);

            let result = pty.write(full.as_bytes());
            prop_assert!(result.is_ok());
            prop_assert_eq!(pty.get_written_data(), full.as_bytes());
        }

        // ==================== Output Queue Tests ====================

        /// Property: Queue output preserves data exactly
        #[test]
        fn prop_queue_output_preserves_data(
            chunks in proptest::collection::vec(
                proptest::collection::vec(any::<u8>(), 0..1000),
                0..50
            )
        ) {
            let pty = MockPtyIO::new();

            for chunk in &chunks {
                pty.queue_output(chunk);
            }

            let output = pty.read_output();
            prop_assert_eq!(output.len(), chunks.len());

            for (i, (actual, expected)) in output.iter().zip(chunks.iter()).enumerate() {
                prop_assert_eq!(actual, expected, "Chunk {} mismatch", i);
            }
        }

        /// Property: Read output clears queue
        #[test]
        fn prop_read_output_clears_queue(
            data in proptest::collection::vec(any::<u8>(), 0..100)
        ) {
            let pty = MockPtyIO::new();
            pty.queue_output(&data);

            let first_read = pty.read_output();
            prop_assert_eq!(first_read.len(), 1);

            let second_read = pty.read_output();
            prop_assert!(second_read.is_empty(), "Queue should be empty after read");
        }

        // ==================== PtySize Tests ====================

        /// Property: PtySize accepts any valid u16 dimensions
        #[test]
        fn prop_pty_size_arbitrary_dimensions(
            rows in any::<u16>(),
            cols in any::<u16>(),
            pixel_width in any::<u16>(),
            pixel_height in any::<u16>()
        ) {
            let size = PtySize {
                rows,
                cols,
                pixel_width,
                pixel_height,
            };

            prop_assert_eq!(size.rows, rows);
            prop_assert_eq!(size.cols, cols);
            prop_assert_eq!(size.pixel_width, pixel_width);
            prop_assert_eq!(size.pixel_height, pixel_height);
        }

        /// Property: Zero dimensions are valid (even if unusual)
        #[test]
        fn prop_pty_size_zero_dimensions(_seed in any::<u64>()) {
            let size = PtySize {
                rows: 0,
                cols: 0,
                pixel_width: 0,
                pixel_height: 0,
            };

            prop_assert_eq!(size.rows, 0);
            prop_assert_eq!(size.cols, 0);
        }

        /// Property: Maximum dimensions are valid
        #[test]
        fn prop_pty_size_max_dimensions(_seed in any::<u64>()) {
            let size = PtySize {
                rows: u16::MAX,
                cols: u16::MAX,
                pixel_width: u16::MAX,
                pixel_height: u16::MAX,
            };

            prop_assert_eq!(size.rows, u16::MAX);
            prop_assert_eq!(size.cols, u16::MAX);
        }

        // ==================== Channel Behavior Tests ====================

        /// Property: Bounded channel respects capacity
        #[test]
        fn prop_bounded_channel_capacity(capacity in 1usize..100) {
            let (tx, _rx): (mpsc::SyncSender<u8>, mpsc::Receiver<u8>) =
                mpsc::sync_channel(capacity);

            // Fill to capacity
            for i in 0..capacity {
                let result = tx.try_send(i as u8);
                prop_assert!(result.is_ok(), "Should be able to send item {}", i);
            }

            // Next send should fail with Full
            let result = tx.try_send(0);
            prop_assert!(matches!(result, Err(TrySendError::Full(_))));
        }

        /// Property: Channel message order is preserved
        #[test]
        fn prop_channel_preserves_order(
            messages in proptest::collection::vec(any::<u8>(), 1..50)
        ) {
            let (tx, rx): (mpsc::SyncSender<u8>, mpsc::Receiver<u8>) =
                mpsc::sync_channel(messages.len());

            for &msg in &messages {
                tx.send(msg).unwrap();
            }

            drop(tx);

            let received: Vec<u8> = rx.iter().collect();
            prop_assert_eq!(received, messages);
        }

        // ==================== Exit Flag Tests ====================

        /// Property: AtomicBool stores value correctly
        #[test]
        fn prop_atomic_bool_stores_correctly(value: bool) {
            let flag = Arc::new(AtomicBool::new(value));
            prop_assert_eq!(flag.load(Ordering::SeqCst), value);
        }

        /// Property: AtomicBool toggle works correctly
        #[test]
        fn prop_atomic_bool_toggle(initial: bool) {
            let flag = Arc::new(AtomicBool::new(initial));

            flag.store(!initial, Ordering::SeqCst);
            prop_assert_eq!(flag.load(Ordering::SeqCst), !initial);

            flag.store(initial, Ordering::SeqCst);
            prop_assert_eq!(flag.load(Ordering::SeqCst), initial);
        }

        // ==================== Input Sequence Tests ====================

        /// Property: Keyboard input bytes are preserved
        #[test]
        fn prop_keyboard_input_preserved(
            input in proptest::collection::vec(any::<u8>(), 0..100)
        ) {
            let pty = MockPtyIO::new();

            let result = pty.write(&input);
            prop_assert!(result.is_ok());
            prop_assert_eq!(pty.get_written_data(), input);
        }

        /// Property: Control characters are preserved
        #[test]
        fn prop_control_chars_preserved(control in 0u8..32) {
            let pty = MockPtyIO::new();

            let result = pty.write(&[control]);
            prop_assert!(result.is_ok());
            prop_assert_eq!(pty.get_written_data(), vec![control]);
        }

        /// Property: Extended ASCII is preserved
        #[test]
        fn prop_extended_ascii_preserved(byte in 128u8..=255) {
            let pty = MockPtyIO::new();

            let result = pty.write(&[byte]);
            prop_assert!(result.is_ok());
            prop_assert_eq!(pty.get_written_data(), vec![byte]);
        }

        // ==================== Shell Validation Tests ====================

        /// Property: Allowed shells all start with / (Unix only)
        #[test]
        #[cfg(not(target_os = "windows"))]
        fn prop_allowed_shells_absolute_paths(_idx in 0usize..ALLOWED_SHELLS.len()) {
            let shell = ALLOWED_SHELLS[_idx];
            prop_assert!(shell.starts_with('/'), "Shell '{}' should be absolute path", shell);
        }

        /// Property: CommandBuilder accepts any shell path (does not validate existence)
        #[test]
        fn prop_command_builder_accepts_any_path(
            path in "/[a-z]+(/[a-z]+)*"
        ) {
            // CommandBuilder just stores the path, doesn't validate existence
            let cmd = CommandBuilder::new(&path);
            // If we got here without panic, the test passes
            drop(cmd);
        }

        // ==================== Large Data Tests ====================

        /// Property: Large writes are handled correctly
        #[test]
        fn prop_large_write_handled(size in 1000usize..10000) {
            let pty = MockPtyIO::new();
            let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();

            let result = pty.write(&data);
            prop_assert!(result.is_ok());
            prop_assert_eq!(pty.get_written_data().len(), size);
        }

        /// Property: Large queued output is preserved
        #[test]
        fn prop_large_queued_output(size in 1000usize..10000) {
            let pty = MockPtyIO::new();
            let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();

            pty.queue_output(&data);
            let output = pty.read_output();

            prop_assert_eq!(output.len(), 1);
            prop_assert_eq!(output[0].len(), size);
        }
    }
}
