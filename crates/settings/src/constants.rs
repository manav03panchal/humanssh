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

    /// Default monospace font family (macOS).
    /// Menlo is built-in on all macOS versions since 10.6.
    #[cfg(target_os = "macos")]
    pub const FONT_FAMILY: &str = "Menlo";

    /// Default monospace font family (Windows).
    /// Consolas is built-in on all Windows versions since Vista.
    #[cfg(target_os = "windows")]
    pub const FONT_FAMILY: &str = "Consolas";

    /// Default monospace font family (Linux and others).
    /// "monospace" is the generic font family that always resolves to something.
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    pub const FONT_FAMILY: &str = "monospace";

    /// Padding around terminal content.
    pub const PADDING: f32 = 2.0;
    /// Cursor/border thickness in pixels.
    pub const CURSOR_THICKNESS: f32 = 2.0;
}

/// Tab bar configuration.
pub mod tab_bar {
    /// Tab bar height in pixels.
    pub const HEIGHT: f32 = 38.0;

    /// Left padding (traffic lights are in native titlebar, not in tab bar).
    pub const LEFT_PADDING: f32 = 8.0;

    /// Right padding.
    pub const RIGHT_PADDING: f32 = 8.0;
    /// Minimum tab width.
    pub const TAB_MIN_WIDTH: f32 = 120.0;
    /// Maximum tab width.
    pub const TAB_MAX_WIDTH: f32 = 200.0;
    /// Close button size.
    pub const CLOSE_BUTTON_SIZE: f32 = 18.0;
}

/// Split pane configuration.
pub mod split {
    /// Divider thickness in pixels.
    pub const DIVIDER_THICKNESS: f32 = 2.0;
}

/// Status bar configuration.
pub mod status_bar {
    use std::time::Duration;

    /// Status bar height in pixels.
    pub const HEIGHT: f32 = 24.0;
    /// Horizontal padding.
    pub const HORIZONTAL_PADDING: f32 = 12.0;
    /// Gap between items.
    pub const ITEM_GAP: f32 = 12.0;
    /// Refresh interval for system stats.
    pub const REFRESH_INTERVAL: Duration = Duration::from_secs(1);
}

/// Timing configuration.
pub mod timing {
    use std::time::Duration;

    /// Interval between cleanup checks.
    pub const CLEANUP_INTERVAL: Duration = Duration::from_millis(500);
    /// Tab title cache TTL.
    pub const TITLE_CACHE_TTL: Duration = Duration::from_millis(200);
}

/// Scrollback buffer configuration.
pub mod scrollback {
    /// Default scrollback buffer size in lines.
    pub const DEFAULT_LINES: usize = 10_000;
    /// Maximum allowed scrollback buffer size in lines.
    pub const MAX_LINES: usize = 100_000;
}

/// Settings file validation limits.
pub mod settings {
    /// Maximum settings file size in bytes (64 KB).
    /// Settings files should be tiny; anything larger is suspicious.
    pub const MAX_FILE_SIZE: u64 = 64 * 1024;

    /// Maximum length for string fields (theme name, font family).
    pub const MAX_STRING_LENGTH: usize = 256;
}

#[cfg(test)]
#[allow(clippy::assertions_on_constants, clippy::const_is_empty)]
mod tests {
    use super::*;

    #[test]
    fn test_font_size_range_allows_zoom() {
        // Should allow at least 2x zoom from default
        let zoom_range = terminal::MAX_FONT_SIZE / terminal::MIN_FONT_SIZE;
        assert!(
            zoom_range >= 2.0,
            "Font size range ({:.1}x) should allow at least 2x zoom",
            zoom_range
        );
    }

    #[test]
    fn test_font_family_is_monospace() {
        let known_monospace = ["Iosevka", "Mono", "Consolas", "Monaco", "Menlo", "Courier"];
        assert!(
            known_monospace
                .iter()
                .any(|m| terminal::FONT_FAMILY.contains(m)),
            "FONT_FAMILY '{}' should be a monospace font",
            terminal::FONT_FAMILY
        );
    }

    #[test]
    fn test_close_button_fits_in_tab_height() {
        assert!(
            tab_bar::CLOSE_BUTTON_SIZE < tab_bar::HEIGHT,
            "CLOSE_BUTTON_SIZE ({}) should be less than tab bar HEIGHT ({})",
            tab_bar::CLOSE_BUTTON_SIZE,
            tab_bar::HEIGHT
        );
    }

    #[test]
    fn test_max_string_length_allows_font_names() {
        let long_font_name = "Iosevka Term Slab Extended Extra Light Italic Nerd Font";
        assert!(
            settings::MAX_STRING_LENGTH >= long_font_name.len(),
            "MAX_STRING_LENGTH ({}) should allow font names like '{}'",
            settings::MAX_STRING_LENGTH,
            long_font_name
        );
    }

    #[test]
    fn test_font_defaults_are_consistent() {
        let known_fonts = [
            "Iosevka Nerd Font",
            "JetBrains Mono",
            "Fira Code",
            "SF Mono",
            "Monaco",
            "Menlo",
            "Source Code Pro",
            "Cascadia Code",
            "Consolas",
            "Ubuntu Mono",
        ];
        assert!(
            known_fonts.contains(&terminal::FONT_FAMILY),
            "Default FONT_FAMILY '{}' should be a known terminal font",
            terminal::FONT_FAMILY
        );
    }
}
