//! Centralized configuration constants for HumanSSH.
//!
//! This module provides compile-time constants for UI dimensions and settings.
//! These are organized by component for maintainability.

/// Terminal pane configuration.
pub mod terminal {
    /// Default font size in pixels.
    pub const DEFAULT_FONT_SIZE: f32 = 14.0;
    /// Minimum allowed font size.
    pub const MIN_FONT_SIZE: f32 = 8.0;
    /// Maximum allowed font size.
    pub const MAX_FONT_SIZE: f32 = 32.0;
    /// Default monospace font family.
    pub const FONT_FAMILY: &str = "Iosevka Nerd Font";
    /// Padding around terminal content.
    pub const PADDING: f32 = 2.0;
    /// Cursor/border thickness in pixels.
    pub const CURSOR_THICKNESS: f32 = 2.0;
}

/// Tab bar configuration.
pub mod tab_bar {
    /// Tab bar height in pixels.
    pub const HEIGHT: f32 = 38.0;
    /// Left padding for traffic light buttons.
    pub const LEFT_PADDING: f32 = 78.0;
    /// Right padding.
    pub const RIGHT_PADDING: f32 = 8.0;
    /// Minimum tab width.
    pub const TAB_MIN_WIDTH: f32 = 120.0;
    /// Maximum tab width.
    pub const TAB_MAX_WIDTH: f32 = 200.0;
    /// Close button size.
    pub const CLOSE_BUTTON_SIZE: f32 = 18.0;
}

/// Dialog configuration.
pub mod dialog {
    /// Standard dialog width.
    pub const WIDTH: f32 = 420.0;
    /// Menu minimum width.
    pub const MENU_MIN_WIDTH: f32 = 180.0;
    /// Settings panel width.
    pub const SETTINGS_WIDTH: f32 = 500.0;
}

/// Split pane configuration.
pub mod split {
    /// Divider thickness in pixels.
    pub const DIVIDER_THICKNESS: f32 = 2.0;
}

/// Timing configuration.
pub mod timing {
    use std::time::Duration;

    /// Interval between cleanup checks.
    pub const CLEANUP_INTERVAL: Duration = Duration::from_millis(500);
    /// Tab title cache TTL.
    pub const TITLE_CACHE_TTL: Duration = Duration::from_millis(200);
    /// PTY polling interval.
    pub const PTY_POLL_INTERVAL: Duration = Duration::from_millis(16);
}
