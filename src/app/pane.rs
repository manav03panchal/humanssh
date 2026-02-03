//! Pane abstraction for extensible pane types.
//!
//! This module defines the `Pane` trait and `PaneKind` enum that allow
//! different pane types (terminal, SSH, file browser, etc.) to be used
//! interchangeably in the workspace.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │  PaneKind (enum dispatch - type-safe, extensible)           │
//! ├─────────────────────────────────────────────────────────────┤
//! │  Terminal(Entity<TerminalPane>)                             │
//! │  // Future: Ssh(Entity<SshPane>)                            │
//! │  // Future: FileBrowser(Entity<FileBrowserPane>)            │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Design Decisions
//!
//! We use enum dispatch instead of trait objects (`dyn Pane`) because:
//! 1. GPUI's `Entity<T>` doesn't support `Entity<dyn Trait>` directly
//! 2. Enum dispatch is more idiomatic Rust and avoids vtable overhead
//! 3. Adding new pane types is explicit and compile-time checked
//! 4. Pattern matching enables exhaustive handling of all pane types

use crate::terminal::TerminalPane;
use gpui::{AnyElement, App, Entity, FocusHandle, IntoElement, SharedString, Window};

/// Common behavior for all pane types.
///
/// This trait defines the contract that all panes must fulfill.
/// It's primarily used for documentation; actual dispatch happens
/// via the `PaneKind` enum for GPUI compatibility.
#[allow(dead_code)] // Trait serves as documentation/contract for future pane types
pub trait Pane {
    /// Check if the pane has any running child processes.
    ///
    /// Used to show confirmation dialogs before closing.
    fn has_running_processes(&self) -> bool;

    /// Get the name of the running foreground process, if any.
    ///
    /// Used in confirmation dialog messages.
    fn get_running_process_name(&self) -> Option<String>;

    /// Check if the pane's underlying process has exited.
    ///
    /// Used for automatic cleanup of dead panes.
    fn has_exited(&self) -> bool;

    /// Get the pane's display title.
    ///
    /// Returns `None` if no title has been set (fall back to default).
    fn title(&self) -> Option<SharedString>;

    /// Get the focus handle for this pane.
    ///
    /// Required for GPUI focus management.
    fn focus_handle(&self) -> FocusHandle;
}

/// Type-safe enum for different pane types.
///
/// This enum wraps GPUI entities and provides dispatch for common
/// pane operations. New pane types can be added by:
/// 1. Adding a new variant
/// 2. Implementing the pane behavior in each match arm
#[derive(Clone)]
pub enum PaneKind {
    /// A local terminal pane (PTY session)
    Terminal(Entity<TerminalPane>),
    // Future pane types:
    // Ssh(Entity<SshPane>),
    // FileBrowser(Entity<FileBrowserPane>),
    // Documentation(Entity<DocsPane>),
}

impl PaneKind {
    /// Check if the pane has any running child processes.
    pub fn has_running_processes(&self, cx: &App) -> bool {
        match self {
            PaneKind::Terminal(terminal) => terminal.read(cx).has_running_processes(),
        }
    }

    /// Get the name of the running foreground process, if any.
    pub fn get_running_process_name(&self, cx: &App) -> Option<String> {
        match self {
            PaneKind::Terminal(terminal) => terminal.read(cx).get_running_process_name(),
        }
    }

    /// Get the current working directory of the pane's foreground process.
    pub fn get_current_directory(&self, cx: &App) -> Option<std::path::PathBuf> {
        match self {
            PaneKind::Terminal(terminal) => terminal.read(cx).get_current_directory(),
        }
    }

    /// Check if the pane's underlying process has exited.
    #[cfg_attr(test, allow(dead_code))]
    pub fn has_exited(&self, cx: &App) -> bool {
        match self {
            PaneKind::Terminal(terminal) => terminal.read(cx).has_exited(),
        }
    }

    /// Get the pane's display title.
    pub fn title(&self, cx: &App) -> Option<SharedString> {
        match self {
            PaneKind::Terminal(terminal) => terminal.read(cx).title(),
        }
    }

    /// Get the focus handle for this pane.
    pub fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self {
            PaneKind::Terminal(terminal) => terminal.read(cx).focus_handle.clone(),
        }
    }

    /// Render this pane as an element.
    ///
    /// Returns an `AnyElement` that can be used in GPUI's element tree.
    pub fn render(&self, _window: &mut Window) -> AnyElement {
        match self {
            PaneKind::Terminal(terminal) => terminal.clone().into_any_element(),
        }
    }
}

impl From<Entity<TerminalPane>> for PaneKind {
    fn from(terminal: Entity<TerminalPane>) -> Self {
        PaneKind::Terminal(terminal)
    }
}
