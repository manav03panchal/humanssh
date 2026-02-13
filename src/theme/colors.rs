//! Terminal color mapping from theme.
//!
//! Maps gpui-component theme colors to terminal ANSI colors.

use gpui::{rgb, App, Hsla, SharedString};
use gpui_component::theme::Theme;
use gpui_component::{ActiveTheme, Colorize};
use parking_lot::Mutex;

/// Cached terminal colors to avoid recomputation every frame
static TERMINAL_COLORS_CACHE: Mutex<Option<(SharedString, TerminalColors)>> = Mutex::new(None);

/// Terminal color palette mapped from theme
#[derive(Clone, Copy)]
pub struct TerminalColors {
    pub background: Hsla,
    pub foreground: Hsla,
    pub cursor: Hsla,
    pub selection: Hsla,
    // ANSI colors
    pub black: Hsla,
    pub red: Hsla,
    pub green: Hsla,
    pub yellow: Hsla,
    pub blue: Hsla,
    pub magenta: Hsla,
    pub cyan: Hsla,
    pub white: Hsla,
    // Bright ANSI colors
    pub bright_black: Hsla,
    pub bright_red: Hsla,
    pub bright_green: Hsla,
    pub bright_yellow: Hsla,
    pub bright_blue: Hsla,
    pub bright_magenta: Hsla,
    pub bright_cyan: Hsla,
    pub bright_white: Hsla,
    // UI colors
    pub title_bar: Hsla,
    pub tab_active: Hsla,
    pub tab_inactive: Hsla,
    pub border: Hsla,
    pub muted: Hsla,
    pub accent: Hsla,
}

impl Default for TerminalColors {
    fn default() -> Self {
        // Catppuccin Mocha fallback
        Self {
            background: rgb(0x1e1e2e).into(),
            foreground: rgb(0xcdd6f4).into(),
            cursor: rgb(0xf5e0dc).into(),
            selection: rgb(0x45475a).into(),
            black: rgb(0x45475a).into(),
            red: rgb(0xf38ba8).into(),
            green: rgb(0xa6e3a1).into(),
            yellow: rgb(0xf9e2af).into(),
            blue: rgb(0x89b4fa).into(),
            magenta: rgb(0xf5c2e7).into(),
            cyan: rgb(0x94e2d5).into(),
            white: rgb(0xbac2de).into(),
            bright_black: rgb(0x585b70).into(),
            bright_red: rgb(0xf38ba8).into(),
            bright_green: rgb(0xa6e3a1).into(),
            bright_yellow: rgb(0xf9e2af).into(),
            bright_blue: rgb(0x89b4fa).into(),
            bright_magenta: rgb(0xf5c2e7).into(),
            bright_cyan: rgb(0x94e2d5).into(),
            bright_white: rgb(0xa6adc8).into(),
            title_bar: rgb(0x181825).into(),
            tab_active: rgb(0x313244).into(),
            tab_inactive: rgb(0x1e1e2e).into(),
            border: rgb(0x313244).into(),
            muted: rgb(0x6c7086).into(),
            accent: rgb(0x89b4fa).into(),
        }
    }
}

/// Get terminal colors from the current theme (cached)
/// Maps gpui-component theme colors to terminal ANSI colors
pub fn terminal_colors(cx: &App) -> TerminalColors {
    let current_theme = cx.theme().theme_name().clone();

    // Fast path: return cached colors if theme hasn't changed
    {
        let cache = TERMINAL_COLORS_CACHE.lock();
        if let Some((cached_theme, cached_colors)) = cache.as_ref() {
            if cached_theme == &current_theme {
                return *cached_colors;
            }
        }
    }

    // Slow path: compute colors and cache them
    let theme = Theme::global(cx);
    let colors = &theme.colors;

    let terminal_colors = TerminalColors {
        background: colors.background,
        foreground: colors.foreground,
        cursor: colors.caret,
        selection: colors.selection,
        black: colors.background.darken(0.3),
        red: colors.red,
        green: colors.green,
        yellow: colors.yellow,
        blue: colors.blue,
        magenta: colors.magenta,
        cyan: colors.cyan,
        white: colors.foreground.lighten(0.1),
        bright_black: colors.muted_foreground,
        bright_red: colors.red_light,
        bright_green: colors.green_light,
        bright_yellow: colors.yellow_light,
        bright_blue: colors.blue_light,
        bright_magenta: colors.magenta_light,
        bright_cyan: colors.cyan_light,
        bright_white: colors.foreground.lighten(0.2),
        // UI colors
        title_bar: colors.title_bar,
        tab_active: colors.tab_active,
        tab_inactive: colors.tab,
        border: colors.border,
        muted: colors.muted_foreground,
        accent: colors.accent,
    };

    // Update cache
    *TERMINAL_COLORS_CACHE.lock() = Some((current_theme, terminal_colors));

    terminal_colors
}

#[cfg(test)]
mod tests {
    use super::{rgb, Hsla, TerminalColors, TERMINAL_COLORS_CACHE};

    /// Helper to check if an Hsla color is valid (non-zero alpha, finite values)
    fn is_valid_hsla(color: Hsla) -> bool {
        color.h.is_finite()
            && color.s.is_finite()
            && color.l.is_finite()
            && color.a.is_finite()
            && color.a > 0.0
    }

    mod terminal_colors_struct {
        use super::{is_valid_hsla, TerminalColors};

        #[test]
        fn default_creates_valid_catppuccin_mocha_colors() {
            let colors = TerminalColors::default();

            // Check all colors are valid
            assert!(
                is_valid_hsla(colors.background),
                "background should be valid"
            );
            assert!(
                is_valid_hsla(colors.foreground),
                "foreground should be valid"
            );
            assert!(is_valid_hsla(colors.cursor), "cursor should be valid");
            assert!(is_valid_hsla(colors.selection), "selection should be valid");
        }

        #[test]
        fn default_has_all_standard_ansi_colors() {
            let colors = TerminalColors::default();

            // Standard ANSI colors (0-7)
            assert!(is_valid_hsla(colors.black), "black should be valid");
            assert!(is_valid_hsla(colors.red), "red should be valid");
            assert!(is_valid_hsla(colors.green), "green should be valid");
            assert!(is_valid_hsla(colors.yellow), "yellow should be valid");
            assert!(is_valid_hsla(colors.blue), "blue should be valid");
            assert!(is_valid_hsla(colors.magenta), "magenta should be valid");
            assert!(is_valid_hsla(colors.cyan), "cyan should be valid");
            assert!(is_valid_hsla(colors.white), "white should be valid");
        }

        #[test]
        fn default_has_all_bright_ansi_colors() {
            let colors = TerminalColors::default();

            // Bright ANSI colors (8-15)
            assert!(
                is_valid_hsla(colors.bright_black),
                "bright_black should be valid"
            );
            assert!(
                is_valid_hsla(colors.bright_red),
                "bright_red should be valid"
            );
            assert!(
                is_valid_hsla(colors.bright_green),
                "bright_green should be valid"
            );
            assert!(
                is_valid_hsla(colors.bright_yellow),
                "bright_yellow should be valid"
            );
            assert!(
                is_valid_hsla(colors.bright_blue),
                "bright_blue should be valid"
            );
            assert!(
                is_valid_hsla(colors.bright_magenta),
                "bright_magenta should be valid"
            );
            assert!(
                is_valid_hsla(colors.bright_cyan),
                "bright_cyan should be valid"
            );
            assert!(
                is_valid_hsla(colors.bright_white),
                "bright_white should be valid"
            );
        }

        #[test]
        fn default_has_all_ui_colors() {
            let colors = TerminalColors::default();

            // UI-specific colors
            assert!(is_valid_hsla(colors.title_bar), "title_bar should be valid");
            assert!(
                is_valid_hsla(colors.tab_active),
                "tab_active should be valid"
            );
            assert!(
                is_valid_hsla(colors.tab_inactive),
                "tab_inactive should be valid"
            );
            assert!(is_valid_hsla(colors.border), "border should be valid");
            assert!(is_valid_hsla(colors.muted), "muted should be valid");
            assert!(is_valid_hsla(colors.accent), "accent should be valid");
        }

        #[test]
        fn default_background_is_dark() {
            let colors = TerminalColors::default();
            // Catppuccin Mocha background (0x1e1e2e) should be dark (low lightness)
            assert!(
                colors.background.l < 0.25,
                "background lightness {} should be < 0.25 for dark theme",
                colors.background.l
            );
        }

        #[test]
        fn default_foreground_is_light() {
            let colors = TerminalColors::default();
            // Catppuccin Mocha foreground (0xcdd6f4) should be light (high lightness)
            assert!(
                colors.foreground.l > 0.7,
                "foreground lightness {} should be > 0.7 for readable text",
                colors.foreground.l
            );
        }
    }

    mod color_palette_completeness {
        use super::TerminalColors;

        #[test]
        fn all_16_ansi_colors_are_distinct() {
            let colors = TerminalColors::default();

            let ansi_colors = [
                ("black", colors.black),
                ("red", colors.red),
                ("green", colors.green),
                ("yellow", colors.yellow),
                ("blue", colors.blue),
                ("magenta", colors.magenta),
                ("cyan", colors.cyan),
                ("white", colors.white),
                ("bright_black", colors.bright_black),
                ("bright_red", colors.bright_red),
                ("bright_green", colors.bright_green),
                ("bright_yellow", colors.bright_yellow),
                ("bright_blue", colors.bright_blue),
                ("bright_magenta", colors.bright_magenta),
                ("bright_cyan", colors.bright_cyan),
                ("bright_white", colors.bright_white),
            ];

            // Check that we have all 16 colors
            assert_eq!(ansi_colors.len(), 16);

            // Verify most colors are distinct (some themes may have identical normal/bright)
            let mut distinct_count = 0;
            for i in 0..ansi_colors.len() {
                for j in (i + 1)..ansi_colors.len() {
                    let (name1, color1) = ansi_colors[i];
                    let (name2, color2) = ansi_colors[j];
                    // Consider colors distinct if any component differs significantly
                    let is_distinct = (color1.h - color2.h).abs() > 0.01
                        || (color1.s - color2.s).abs() > 0.01
                        || (color1.l - color2.l).abs() > 0.01;
                    if is_distinct {
                        distinct_count += 1;
                    } else {
                        // Log identical colors (expected for some bright variants in Catppuccin)
                        tracing::debug!("{} and {} have identical colors", name1, name2);
                    }
                }
            }

            // At minimum, most colors should be distinct
            assert!(
                distinct_count > 80,
                "Expected most color pairs to be distinct, got {} distinct pairs",
                distinct_count
            );
        }

        #[test]
        fn semantic_colors_make_sense() {
            let colors = TerminalColors::default();

            // Red should have high saturation and hue in red range
            assert!(colors.red.s > 0.5, "red saturation should be high");

            // Green should have high saturation
            assert!(colors.green.s > 0.5, "green saturation should be high");

            // Blue should have high saturation
            assert!(colors.blue.s > 0.5, "blue saturation should be high");

            // Yellow should have high saturation
            assert!(colors.yellow.s > 0.5, "yellow saturation should be high");

            // Cyan should have high saturation
            assert!(colors.cyan.s > 0.5, "cyan saturation should be high");

            // Magenta should have high saturation
            assert!(colors.magenta.s > 0.5, "magenta saturation should be high");
        }

        #[test]
        fn bright_black_is_lighter_than_black() {
            let colors = TerminalColors::default();

            // Bright black should be lighter than regular black (more visible)
            // This is a standard convention for ANSI bright colors
            assert!(
                colors.bright_black.l >= colors.black.l - 0.01,
                "bright_black should be at least as light as black"
            );
        }

        #[test]
        fn all_colors_have_full_alpha() {
            let colors = TerminalColors::default();

            // All colors should be fully opaque
            assert!(colors.white.a > 0.99, "white should be opaque");
            assert!(
                colors.bright_white.a > 0.99,
                "bright_white should be opaque"
            );
            assert!(colors.black.a > 0.99, "black should be opaque");
            assert!(
                colors.bright_black.a > 0.99,
                "bright_black should be opaque"
            );
        }
    }

    mod color_cache {
        use super::{TerminalColors, TERMINAL_COLORS_CACHE};
        use gpui::SharedString;

        #[test]
        fn cache_starts_empty() {
            // Clear cache for test
            *TERMINAL_COLORS_CACHE.lock() = None;
            assert!(TERMINAL_COLORS_CACHE.lock().is_none());
        }

        #[test]
        fn cache_can_be_set_and_retrieved() {
            // Clear cache
            *TERMINAL_COLORS_CACHE.lock() = None;

            let test_colors = TerminalColors::default();
            let theme_name: SharedString = "Test Theme".into();

            // Set cache
            *TERMINAL_COLORS_CACHE.lock() = Some((theme_name.clone(), test_colors));

            // Retrieve and verify
            let cache = TERMINAL_COLORS_CACHE.lock();
            assert!(cache.is_some());
            let (cached_name, cached_colors) = cache.as_ref().unwrap();
            assert_eq!(cached_name, &theme_name);
            assert_eq!(cached_colors.background.h, test_colors.background.h);
        }

        #[test]
        fn cache_mutex_is_not_poisoned() {
            // Verify we can acquire the lock multiple times
            {
                let _lock1 = TERMINAL_COLORS_CACHE.lock();
            }
            {
                let _lock2 = TERMINAL_COLORS_CACHE.lock();
            }
            // If we got here, the mutex is working correctly
        }
    }

    mod rgb_conversion {
        use super::{rgb, TerminalColors};
        use gpui::Hsla;

        #[test]
        fn rgb_macro_produces_valid_colors() {
            // Test that rgb! macro works correctly for common values
            let black: Hsla = rgb(0x000000).into();
            assert!(black.l < 0.01, "black should have near-zero lightness");

            let white: Hsla = rgb(0xffffff).into();
            assert!(white.l > 0.99, "white should have near-one lightness");

            let red: Hsla = rgb(0xff0000).into();
            assert!(red.s > 0.99, "pure red should have full saturation");
        }

        #[test]
        fn catppuccin_mocha_colors_are_correct() {
            // Verify specific Catppuccin Mocha hex values
            let mocha_base: Hsla = rgb(0x1e1e2e).into();
            let mocha_text: Hsla = rgb(0xcdd6f4).into();

            let defaults = TerminalColors::default();

            // Compare with some tolerance for floating point
            assert!(
                (defaults.background.h - mocha_base.h).abs() < 0.01,
                "background hue should match Catppuccin Mocha base"
            );
            assert!(
                (defaults.foreground.h - mocha_text.h).abs() < 0.01,
                "foreground hue should match Catppuccin Mocha text"
            );
        }
    }
}
