//! Terminal emulation and PTY management.
//!
//! This module integrates alacritty_terminal for terminal emulation
//! and portable-pty for PTY spawning.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │  TerminalPane (GPUI View)                                   │
//! │  - Event handling (keyboard, mouse, scroll)                 │
//! │  - Focus management                                         │
//! │  - Render implementation                                    │
//! ├─────────────────────────────────────────────────────────────┤
//! │  PtyHandler           │  alacritty_terminal::Term           │
//! │  - PTY spawning       │  - VTE parsing                      │
//! │  - I/O channels       │  - Screen buffer                    │
//! │  - Process lifecycle  │  - Cursor state                     │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Module Structure
//!
//! - `types` - Core data structures (TermSize, DisplayState, RenderCell, etc.)
//! - `colors` - Color conversion utilities (RGB/ANSI to Hsla)
//! - `pty_handler` - Low-level PTY I/O with bounded channels
//! - `pane` - TerminalPane GPUI view
//!
//! # Design Decisions
//!
//! 1. **GPUI Integration**: TerminalPane is a GPUI view that handles events
//!    directly. Input handlers remain in pane.rs because GPUI's architecture
//!    requires views to implement `on_key_down`, `on_mouse_*`, etc.
//!
//! 2. **State Isolation**: PTY handler is in a separate mutex to allow
//!    background I/O without blocking the UI thread.
//!
//! 3. **Extracted Pure Functions**: Color conversion and data types are
//!    extracted to separate modules for testability and clarity.

mod colors;
mod pane;
mod pty_handler;
pub mod types;

pub use pane::{SendShiftTab, SendTab, TerminalExitEvent, TerminalPane};
pub use pty_handler::PtyHandler;

/// Register terminal-specific keybindings.
/// Call this during app initialization.
pub fn register_keybindings(cx: &mut gpui::App) {
    use gpui::KeyBinding;
    cx.bind_keys([
        // Tab key - bound to terminal context to bypass GPUI's focus navigation
        KeyBinding::new("tab", SendTab, Some("terminal")),
        KeyBinding::new("shift-tab", SendShiftTab, Some("terminal")),
    ]);
}
