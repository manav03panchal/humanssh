//! Terminal GPUI view layer.
//!
//! Rendering, input handling, and color conversion for the terminal.

mod colors;
mod pane;

pub use pane::{TerminalExitEvent, TerminalPane};
