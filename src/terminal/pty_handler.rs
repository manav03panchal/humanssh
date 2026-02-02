//! PTY process management.

use anyhow::{Context, Result};
use portable_pty::{native_pty_system, Child, CommandBuilder, PtyPair, PtySize};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;

/// Handles PTY spawning and I/O
pub struct PtyHandler {
    pair: PtyPair,
    writer: Box<dyn Write + Send>,
    output_rx: Receiver<Vec<u8>>,
    exited: Arc<AtomicBool>,
    child: Box<dyn Child + Send + Sync>,
    _reader_thread: thread::JoinHandle<()>,
}

impl PtyHandler {
    /// Spawn a new PTY with the user's default shell
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

    /// Check if the shell has any running child processes
    pub fn has_running_processes(&self) -> bool {
        // Get the shell's process ID
        if let Some(pid) = self.child.process_id() {
            // Use pgrep to check for child processes
            if let Ok(output) = std::process::Command::new("pgrep")
                .args(["-P", &pid.to_string()])
                .output()
            {
                // If pgrep returns any output, there are child processes
                return !output.stdout.is_empty();
            }
        }
        false
    }

    /// Get the name of any running foreground process (for display in confirmation)
    pub fn get_running_process_name(&self) -> Option<String> {
        if let Some(pid) = self.child.process_id() {
            // Get child PIDs
            if let Ok(output) = std::process::Command::new("pgrep")
                .args(["-P", &pid.to_string()])
                .output()
            {
                if !output.stdout.is_empty() {
                    // Get the first child PID
                    let child_pids = String::from_utf8_lossy(&output.stdout);
                    if let Some(child_pid) = child_pids.lines().next() {
                        // Get the process name using ps
                        if let Ok(ps_output) = std::process::Command::new("ps")
                            .args(["-p", child_pid, "-o", "comm="])
                            .output()
                        {
                            let name = String::from_utf8_lossy(&ps_output.stdout)
                                .trim()
                                .to_string();
                            if !name.is_empty() {
                                return Some(name);
                            }
                        }
                    }
                }
            }
        }
        None
    }
}
