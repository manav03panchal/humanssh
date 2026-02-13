//! Shared action definitions for HumanSSH.
//!
//! All gpui::actions! used across multiple crates are defined here
//! to avoid circular dependencies.

use std::sync::atomic::AtomicBool;

use gpui::actions;

// Application lifecycle
actions!(humanssh, [Quit, OpenSettings]);

// Tab management
actions!(humanssh, [NewTab, CloseTab, NextTab, PrevTab]);

// Split management
actions!(humanssh, [SplitVertical, SplitHorizontal, ClosePane]);

// Focus navigation
actions!(humanssh, [FocusNextPane, FocusPrevPane]);

// macOS native features
actions!(humanssh, [ToggleSecureInput, ToggleOptionAsAlt]);

// Search
actions!(
    humanssh,
    [SearchToggle, SearchNext, SearchPrev, SearchToggleRegex]
);

// Copy mode
actions!(humanssh, [EnterCopyMode, ExitCopyMode]);

// Terminal-specific actions to capture keys before GPUI's focus system
actions!(terminal, [SendTab, SendShiftTab]);

/// When true (default), macOS Option key is treated as Alt for terminal input.
/// When false, Option key is stripped from modifier set, allowing macOS to insert special characters.
pub static OPTION_AS_ALT: AtomicBool = AtomicBool::new(true);
