//! Terminal emulation core.
//!
//! PTY management and terminal data structures.
//! This crate contains no GPUI behavioral dependencies — it's the pure logic layer.
//! (gpui types like Hsla and SharedString are used for data representation only.)

mod pty_handler;
pub mod types;
pub mod vt_processor;

pub use pty_handler::PtyHandler;
pub use types::*;
pub use vt_processor::TerminalProcessor;
