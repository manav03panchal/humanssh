#![recursion_limit = "2048"]
//! Terminal GPUI view layer.
//!
//! Rendering, input handling, and color conversion for the terminal.

mod colors;
pub mod copy_mode;
pub mod kitty_keyboard;
mod pane;

pub use pane::{TabBadge, TerminalExitEvent, TerminalPane};
