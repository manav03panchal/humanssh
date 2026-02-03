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
    /// PTY polling interval.
    pub const PTY_POLL_INTERVAL: Duration = Duration::from_millis(16);
}

/// Settings file validation limits.
pub mod settings {
    /// Maximum settings file size in bytes (64 KB).
    /// Settings files should be tiny; anything larger is suspicious.
    pub const MAX_FILE_SIZE: u64 = 64 * 1024;

    /// Maximum length for string fields (theme name, font family).
    pub const MAX_STRING_LENGTH: usize = 256;

    /// Window bounds validation limits.
    pub mod window {
        /// Minimum window position (allows for off-screen windows on multi-monitor).
        pub const MIN_POSITION: f32 = -10000.0;
        /// Maximum window position.
        pub const MAX_POSITION: f32 = 100000.0;
        /// Minimum window dimension.
        pub const MIN_SIZE: f32 = 100.0;
        /// Maximum window dimension.
        pub const MAX_SIZE: f32 = 10000.0;
    }
}

#[cfg(test)]
// Allow assertions on constants - these tests verify const relationships
// serve as documentation that config values are sensible
#[allow(clippy::assertions_on_constants, clippy::const_is_empty)]
mod tests {
    use super::*;

    // ==================== Terminal Configuration Tests ====================

    mod terminal_tests {
        use super::terminal;

        #[test]
        fn test_font_size_limits_are_sensible() {
            // MIN < DEFAULT < MAX
            assert!(
                terminal::MIN_FONT_SIZE < terminal::DEFAULT_FONT_SIZE,
                "MIN_FONT_SIZE ({}) should be less than DEFAULT_FONT_SIZE ({})",
                terminal::MIN_FONT_SIZE,
                terminal::DEFAULT_FONT_SIZE
            );
            assert!(
                terminal::DEFAULT_FONT_SIZE < terminal::MAX_FONT_SIZE,
                "DEFAULT_FONT_SIZE ({}) should be less than MAX_FONT_SIZE ({})",
                terminal::DEFAULT_FONT_SIZE,
                terminal::MAX_FONT_SIZE
            );
        }

        #[test]
        fn test_font_size_values() {
            assert_eq!(terminal::DEFAULT_FONT_SIZE, 14.0);
            assert_eq!(terminal::MIN_FONT_SIZE, 8.0);
            assert_eq!(terminal::MAX_FONT_SIZE, 32.0);
        }

        #[test]
        fn test_font_size_min_is_readable() {
            // Minimum font size should still be readable (at least 6px)
            assert!(
                terminal::MIN_FONT_SIZE >= 6.0,
                "MIN_FONT_SIZE ({}) should be at least 6.0 for readability",
                terminal::MIN_FONT_SIZE
            );
        }

        #[test]
        fn test_font_size_max_is_reasonable() {
            // Maximum font size shouldn't be absurdly large (< 100px)
            assert!(
                terminal::MAX_FONT_SIZE <= 100.0,
                "MAX_FONT_SIZE ({}) should be at most 100.0",
                terminal::MAX_FONT_SIZE
            );
        }

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
            // Font family should contain "Mono" or be a known monospace font
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
        fn test_font_family_not_empty() {
            assert!(
                !terminal::FONT_FAMILY.is_empty(),
                "FONT_FAMILY should not be empty"
            );
        }

        #[test]
        fn test_padding_is_non_negative() {
            assert!(
                terminal::PADDING >= 0.0,
                "PADDING ({}) should be non-negative",
                terminal::PADDING
            );
        }

        #[test]
        fn test_padding_is_reasonable() {
            // Padding shouldn't be larger than a typical character width
            assert!(
                terminal::PADDING <= 20.0,
                "PADDING ({}) should be at most 20.0",
                terminal::PADDING
            );
        }

        #[test]
        fn test_cursor_thickness_is_positive() {
            assert!(
                terminal::CURSOR_THICKNESS > 0.0,
                "CURSOR_THICKNESS ({}) should be positive",
                terminal::CURSOR_THICKNESS
            );
        }

        #[test]
        fn test_cursor_thickness_is_visible() {
            // Cursor should be at least 1px thick to be visible
            assert!(
                terminal::CURSOR_THICKNESS >= 1.0,
                "CURSOR_THICKNESS ({}) should be at least 1.0 for visibility",
                terminal::CURSOR_THICKNESS
            );
        }

        #[test]
        fn test_cursor_thickness_is_reasonable() {
            // Cursor shouldn't be too thick
            assert!(
                terminal::CURSOR_THICKNESS <= 10.0,
                "CURSOR_THICKNESS ({}) should be at most 10.0",
                terminal::CURSOR_THICKNESS
            );
        }
    }

    // ==================== Tab Bar Configuration Tests ====================

    mod tab_bar_tests {
        use super::tab_bar;

        #[test]
        fn test_tab_width_limits_are_sensible() {
            // MIN < MAX
            assert!(
                tab_bar::TAB_MIN_WIDTH < tab_bar::TAB_MAX_WIDTH,
                "TAB_MIN_WIDTH ({}) should be less than TAB_MAX_WIDTH ({})",
                tab_bar::TAB_MIN_WIDTH,
                tab_bar::TAB_MAX_WIDTH
            );
        }

        #[test]
        fn test_tab_bar_height_is_positive() {
            assert!(
                tab_bar::HEIGHT > 0.0,
                "Tab bar HEIGHT ({}) should be positive",
                tab_bar::HEIGHT
            );
        }

        #[test]
        fn test_tab_bar_height_is_reasonable() {
            // Tab bar should be at least 20px but not more than 100px
            assert!(
                tab_bar::HEIGHT >= 20.0 && tab_bar::HEIGHT <= 100.0,
                "Tab bar HEIGHT ({}) should be between 20.0 and 100.0",
                tab_bar::HEIGHT
            );
        }

        #[test]
        fn test_tab_min_width_is_usable() {
            // Minimum tab width should fit at least "Tab" text (>= 40px)
            assert!(
                tab_bar::TAB_MIN_WIDTH >= 40.0,
                "TAB_MIN_WIDTH ({}) should be at least 40.0",
                tab_bar::TAB_MIN_WIDTH
            );
        }

        #[test]
        fn test_tab_max_width_is_reasonable() {
            // Maximum tab width shouldn't take up entire screen
            assert!(
                tab_bar::TAB_MAX_WIDTH <= 500.0,
                "TAB_MAX_WIDTH ({}) should be at most 500.0",
                tab_bar::TAB_MAX_WIDTH
            );
        }

        #[test]
        fn test_close_button_size_is_positive() {
            assert!(
                tab_bar::CLOSE_BUTTON_SIZE > 0.0,
                "CLOSE_BUTTON_SIZE ({}) should be positive",
                tab_bar::CLOSE_BUTTON_SIZE
            );
        }

        #[test]
        fn test_close_button_fits_in_tab_height() {
            // Close button should fit within the tab bar height
            assert!(
                tab_bar::CLOSE_BUTTON_SIZE < tab_bar::HEIGHT,
                "CLOSE_BUTTON_SIZE ({}) should be less than tab bar HEIGHT ({})",
                tab_bar::CLOSE_BUTTON_SIZE,
                tab_bar::HEIGHT
            );
        }

        #[test]
        fn test_left_padding_is_non_negative() {
            assert!(
                tab_bar::LEFT_PADDING >= 0.0,
                "LEFT_PADDING ({}) should be non-negative",
                tab_bar::LEFT_PADDING
            );
        }

        #[test]
        fn test_right_padding_is_non_negative() {
            assert!(
                tab_bar::RIGHT_PADDING >= 0.0,
                "RIGHT_PADDING ({}) should be non-negative",
                tab_bar::RIGHT_PADDING
            );
        }
    }

    // ==================== Dialog Configuration Tests ====================

    mod dialog_tests {
        use super::dialog;

        #[test]
        fn test_dialog_width_is_positive() {
            assert!(
                dialog::WIDTH > 0.0,
                "Dialog WIDTH ({}) should be positive",
                dialog::WIDTH
            );
        }

        #[test]
        fn test_dialog_width_is_reasonable() {
            // Dialog should be at least 200px but not more than 1000px
            assert!(
                dialog::WIDTH >= 200.0 && dialog::WIDTH <= 1000.0,
                "Dialog WIDTH ({}) should be between 200.0 and 1000.0",
                dialog::WIDTH
            );
        }

        #[test]
        fn test_menu_min_width_is_positive() {
            assert!(
                dialog::MENU_MIN_WIDTH > 0.0,
                "MENU_MIN_WIDTH ({}) should be positive",
                dialog::MENU_MIN_WIDTH
            );
        }

        #[test]
        fn test_menu_min_width_is_usable() {
            // Menu should be wide enough to display text
            assert!(
                dialog::MENU_MIN_WIDTH >= 100.0,
                "MENU_MIN_WIDTH ({}) should be at least 100.0",
                dialog::MENU_MIN_WIDTH
            );
        }

        #[test]
        fn test_settings_width_is_positive() {
            assert!(
                dialog::SETTINGS_WIDTH > 0.0,
                "SETTINGS_WIDTH ({}) should be positive",
                dialog::SETTINGS_WIDTH
            );
        }

        #[test]
        fn test_settings_width_is_larger_than_dialog() {
            // Settings panel is typically larger than standard dialog
            assert!(
                dialog::SETTINGS_WIDTH >= dialog::WIDTH,
                "SETTINGS_WIDTH ({}) should be at least as large as dialog WIDTH ({})",
                dialog::SETTINGS_WIDTH,
                dialog::WIDTH
            );
        }
    }

    // ==================== Split Configuration Tests ====================

    mod split_tests {
        use super::split;

        #[test]
        fn test_divider_thickness_is_positive() {
            assert!(
                split::DIVIDER_THICKNESS > 0.0,
                "DIVIDER_THICKNESS ({}) should be positive",
                split::DIVIDER_THICKNESS
            );
        }

        #[test]
        fn test_divider_thickness_is_visible() {
            // Divider should be at least 1px to be visible
            assert!(
                split::DIVIDER_THICKNESS >= 1.0,
                "DIVIDER_THICKNESS ({}) should be at least 1.0 for visibility",
                split::DIVIDER_THICKNESS
            );
        }

        #[test]
        fn test_divider_thickness_is_not_obtrusive() {
            // Divider shouldn't be too thick
            assert!(
                split::DIVIDER_THICKNESS <= 10.0,
                "DIVIDER_THICKNESS ({}) should be at most 10.0",
                split::DIVIDER_THICKNESS
            );
        }
    }

    // ==================== Timing Configuration Tests ====================

    mod timing_tests {
        use super::timing;
        use std::time::Duration;

        #[test]
        fn test_cleanup_interval_is_positive() {
            assert!(
                timing::CLEANUP_INTERVAL > Duration::ZERO,
                "CLEANUP_INTERVAL should be positive"
            );
        }

        #[test]
        fn test_cleanup_interval_is_reasonable() {
            // Cleanup should happen at least every 10 seconds
            assert!(
                timing::CLEANUP_INTERVAL <= Duration::from_secs(10),
                "CLEANUP_INTERVAL should be at most 10 seconds"
            );
        }

        #[test]
        fn test_title_cache_ttl_is_positive() {
            assert!(
                timing::TITLE_CACHE_TTL > Duration::ZERO,
                "TITLE_CACHE_TTL should be positive"
            );
        }

        #[test]
        fn test_title_cache_ttl_is_reasonable() {
            // Title cache shouldn't be stale for more than a few seconds
            assert!(
                timing::TITLE_CACHE_TTL <= Duration::from_secs(5),
                "TITLE_CACHE_TTL should be at most 5 seconds"
            );
        }

        #[test]
        fn test_pty_poll_interval_is_positive() {
            assert!(
                timing::PTY_POLL_INTERVAL > Duration::ZERO,
                "PTY_POLL_INTERVAL should be positive"
            );
        }

        #[test]
        fn test_pty_poll_interval_is_responsive() {
            // PTY polling should be frequent enough for responsive UX (~60fps or better)
            assert!(
                timing::PTY_POLL_INTERVAL <= Duration::from_millis(50),
                "PTY_POLL_INTERVAL should be at most 50ms for responsiveness"
            );
        }

        #[test]
        fn test_pty_poll_interval_not_too_aggressive() {
            // PTY polling shouldn't be too aggressive to avoid CPU waste
            assert!(
                timing::PTY_POLL_INTERVAL >= Duration::from_millis(1),
                "PTY_POLL_INTERVAL should be at least 1ms to avoid busy-waiting"
            );
        }
    }

    // ==================== Settings Validation Configuration Tests ====================

    mod settings_validation_tests {
        use super::settings;

        #[test]
        fn test_max_file_size_is_positive() {
            assert!(
                settings::MAX_FILE_SIZE > 0,
                "MAX_FILE_SIZE should be positive"
            );
        }

        #[test]
        fn test_max_file_size_value() {
            // Should be 64 KB
            assert_eq!(
                settings::MAX_FILE_SIZE,
                64 * 1024,
                "MAX_FILE_SIZE should be 64 KB"
            );
        }

        #[test]
        fn test_max_file_size_is_reasonable() {
            // Settings file shouldn't need to be more than 1MB
            assert!(
                settings::MAX_FILE_SIZE <= 1024 * 1024,
                "MAX_FILE_SIZE should be at most 1 MB"
            );
        }

        #[test]
        fn test_max_string_length_is_positive() {
            assert!(
                settings::MAX_STRING_LENGTH > 0,
                "MAX_STRING_LENGTH should be positive"
            );
        }

        #[test]
        fn test_max_string_length_allows_font_names() {
            // Should allow reasonable font family names
            let long_font_name = "Iosevka Term Slab Extended Extra Light Italic Nerd Font";
            assert!(
                settings::MAX_STRING_LENGTH >= long_font_name.len(),
                "MAX_STRING_LENGTH ({}) should allow font names like '{}'",
                settings::MAX_STRING_LENGTH,
                long_font_name
            );
        }

        #[test]
        fn test_max_string_length_is_bounded() {
            // Shouldn't allow absurdly long strings
            assert!(
                settings::MAX_STRING_LENGTH <= 4096,
                "MAX_STRING_LENGTH should be at most 4096"
            );
        }

        // Window bounds tests
        mod window_bounds_tests {
            use super::settings::window;

            #[test]
            fn test_position_range_is_sensible() {
                // MIN < MAX
                assert!(
                    window::MIN_POSITION < window::MAX_POSITION,
                    "MIN_POSITION ({}) should be less than MAX_POSITION ({})",
                    window::MIN_POSITION,
                    window::MAX_POSITION
                );
            }

            #[test]
            fn test_position_allows_negative() {
                // Should allow negative positions for multi-monitor setups
                assert!(
                    window::MIN_POSITION < 0.0,
                    "MIN_POSITION ({}) should allow negative values for multi-monitor",
                    window::MIN_POSITION
                );
            }

            #[test]
            fn test_position_allows_origin() {
                // Origin (0,0) should be within valid range
                assert!(
                    window::MIN_POSITION <= 0.0 && 0.0 <= window::MAX_POSITION,
                    "Position range should include origin (0,0)"
                );
            }

            #[test]
            fn test_size_range_is_sensible() {
                // MIN < MAX
                assert!(
                    window::MIN_SIZE < window::MAX_SIZE,
                    "MIN_SIZE ({}) should be less than MAX_SIZE ({})",
                    window::MIN_SIZE,
                    window::MAX_SIZE
                );
            }

            #[test]
            fn test_min_size_is_usable() {
                // Window should be at least 50px to be usable
                assert!(
                    window::MIN_SIZE >= 50.0,
                    "MIN_SIZE ({}) should be at least 50.0",
                    window::MIN_SIZE
                );
            }

            #[test]
            fn test_max_size_allows_large_monitors() {
                // Should support at least 8K displays (7680 pixels)
                assert!(
                    window::MAX_SIZE >= 7680.0,
                    "MAX_SIZE ({}) should support 8K displays (7680px)",
                    window::MAX_SIZE
                );
            }

            #[test]
            fn test_typical_window_size_is_valid() {
                // A typical 1920x1080 window should be valid
                let typical_width = 1920.0;
                let typical_height = 1080.0;
                assert!(
                    (window::MIN_SIZE..=window::MAX_SIZE).contains(&typical_width),
                    "Typical width 1920 should be valid"
                );
                assert!(
                    (window::MIN_SIZE..=window::MAX_SIZE).contains(&typical_height),
                    "Typical height 1080 should be valid"
                );
            }
        }
    }

    // ==================== Cross-Module Consistency Tests ====================

    mod consistency_tests {
        use super::*;

        #[test]
        fn test_settings_width_matches_dialog_config() {
            // The settings dialog width in settings.rs should match config
            // Note: The actual value in settings.rs is 500.0, same as SETTINGS_WIDTH
            assert_eq!(
                dialog::SETTINGS_WIDTH,
                500.0,
                "SETTINGS_WIDTH should match the value used in settings.rs"
            );
        }

        #[test]
        fn test_font_defaults_are_consistent() {
            // Terminal font family should be in the list of known terminal fonts
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
}
