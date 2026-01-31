//! Terminal emulation and PTY management.
//!
//! This module integrates termwiz for terminal emulation
//! and portable-pty for PTY spawning.

mod pane;
mod pty_handler;

pub use pane::TerminalPane;
pub use pty_handler::PtyHandler;
