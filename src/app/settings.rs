//! Settings dialog UI.
//!
//! Extracted from workspace.rs to reduce module size and separate concerns.

#[cfg(target_os = "linux")]
use crate::theme::{load_linux_decorations, LinuxDecorations, SwitchDecorations};
#[cfg(target_os = "windows")]
use crate::theme::{load_windows_shell, SwitchShell, WindowsShell};
use crate::theme::{SwitchFont, SwitchTheme};
use gpui::{div, px, App, IntoElement, ParentElement, SharedString, Styled, Window};
use gpui_component::button::Button;
use gpui_component::menu::DropdownMenu;
use gpui_component::theme::{Theme, ThemeRegistry};
use gpui_component::{v_flex, ActiveTheme, StyledExt, WindowExt};

/// Common monospace fonts for terminals (macOS).
/// Built-in fonts first (guaranteed to exist), then popular installable fonts.
#[cfg(target_os = "macos")]
const TERMINAL_FONTS: &[&str] = &[
    "Menlo",       // Built-in since macOS 10.6 (DEFAULT)
    "Monaco",      // Built-in (legacy)
    "SF Mono",     // Built-in on newer macOS
    "Courier New", // Built-in
    "JetBrains Mono",
    "Fira Code",
    "Iosevka Nerd Font",
    "Source Code Pro",
    "Cascadia Code",
];

/// Common monospace fonts for terminals (Windows).
/// Built-in fonts first (guaranteed to exist), then popular installable fonts.
#[cfg(target_os = "windows")]
const TERMINAL_FONTS: &[&str] = &[
    "Consolas",       // Built-in since Vista (DEFAULT)
    "Courier New",    // Built-in
    "Lucida Console", // Built-in
    "Cascadia Code",  // Built-in with Windows Terminal
    "Cascadia Mono",
    "JetBrains Mono",
    "Fira Code",
    "Source Code Pro",
    "Iosevka Nerd Font",
];

/// Common monospace fonts for terminals (Linux and others).
/// Generic "monospace" first (always resolves), then common fonts.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const TERMINAL_FONTS: &[&str] = &[
    "monospace",        // Generic, always resolves (DEFAULT)
    "DejaVu Sans Mono", // Very common on Linux
    "Liberation Mono",  // Common on RHEL/Fedora
    "Ubuntu Mono",      // Ubuntu default
    "JetBrains Mono",
    "Fira Code",
    "Source Code Pro",
    "Iosevka Nerd Font",
];

/// Toggle the settings dialog (open if closed, close if open).
pub fn toggle_settings_dialog(window: &mut Window, cx: &mut App) {
    if window.has_active_dialog(cx) {
        window.close_dialog(cx);
        return;
    }

    window.open_dialog(cx, |dialog, window, cx| {
        dialog
            .title("Settings")
            .w(px(500.0))
            .child(render_settings_content(window, cx))
    });
}

/// Render the settings dialog content (macOS version).
#[cfg(target_os = "macos")]
pub fn render_settings_content(_window: &mut Window, cx: &mut App) -> impl IntoElement {
    let current_theme = cx.theme().theme_name().clone();
    // Use Theme::global directly to ensure we read from the same source that's modified
    let current_font = Theme::global(cx).font_family.to_string();

    v_flex()
        .gap_4()
        // Theme selection dropdown
        .child(
            v_flex()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .text_color(cx.theme().muted_foreground)
                        .child("Theme"),
                )
                .child(
                    Button::new("theme-dropdown")
                        .label(current_theme.clone())
                        .outline()
                        .w_full()
                        .dropdown_menu(move |menu, _, cx| {
                            let themes = ThemeRegistry::global(cx).sorted_themes();
                            let current = cx.theme().theme_name().clone();
                            let mut menu = menu.min_w(px(200.0));
                            for theme in themes {
                                let name = theme.name.clone();
                                let is_current = current == name;
                                menu = menu.menu_with_check(
                                    name.clone(),
                                    is_current,
                                    Box::new(SwitchTheme(name)),
                                );
                            }
                            menu
                        }),
                ),
        )
        // Font family selection dropdown
        .child(
            v_flex()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .text_color(cx.theme().muted_foreground)
                        .child("Terminal Font"),
                )
                .child(
                    Button::new("font-dropdown")
                        .label(current_font.clone())
                        .outline()
                        .w_full()
                        .dropdown_menu(move |menu, _, cx| {
                            let current = Theme::global(cx).font_family.to_string();
                            let mut menu = menu.min_w(px(200.0));
                            for font in TERMINAL_FONTS {
                                let is_current = current == *font;
                                let font_name: SharedString = (*font).into();
                                menu = menu.menu_with_check(
                                    *font,
                                    is_current,
                                    Box::new(SwitchFont(font_name)),
                                );
                            }
                            menu
                        }),
                ),
        )
        .child(
            div()
                .pt_2()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child("Press Cmd+, to close"),
        )
}

/// Render the settings dialog content (Windows version with shell selection).
#[cfg(target_os = "windows")]
pub fn render_settings_content(_window: &mut Window, cx: &mut App) -> impl IntoElement {
    let current_theme = cx.theme().theme_name().clone();
    let current_font = Theme::global(cx).font_family.to_string();
    let current_shell = load_windows_shell();

    v_flex()
        .gap_4()
        // Theme selection dropdown
        .child(
            v_flex()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .text_color(cx.theme().muted_foreground)
                        .child("Theme"),
                )
                .child(
                    Button::new("theme-dropdown")
                        .label(current_theme.clone())
                        .outline()
                        .w_full()
                        .dropdown_menu(move |menu, _, cx| {
                            let themes = ThemeRegistry::global(cx).sorted_themes();
                            let current = cx.theme().theme_name().clone();
                            let mut menu = menu.min_w(px(200.0));
                            for theme in themes {
                                let name = theme.name.clone();
                                let is_current = current == name;
                                menu = menu.menu_with_check(
                                    name.clone(),
                                    is_current,
                                    Box::new(SwitchTheme(name)),
                                );
                            }
                            menu
                        }),
                ),
        )
        // Font family selection dropdown
        .child(
            v_flex()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .text_color(cx.theme().muted_foreground)
                        .child("Terminal Font"),
                )
                .child(
                    Button::new("font-dropdown")
                        .label(current_font.clone())
                        .outline()
                        .w_full()
                        .dropdown_menu(move |menu, _, cx| {
                            let current = Theme::global(cx).font_family.to_string();
                            let mut menu = menu.min_w(px(200.0));
                            for font in TERMINAL_FONTS {
                                let is_current = current == *font;
                                let font_name: SharedString = (*font).into();
                                menu = menu.menu_with_check(
                                    *font,
                                    is_current,
                                    Box::new(SwitchFont(font_name)),
                                );
                            }
                            menu
                        }),
                ),
        )
        // Shell selection dropdown (Windows only)
        .child(
            v_flex()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .text_color(cx.theme().muted_foreground)
                        .child("Default Shell"),
                )
                .child(
                    Button::new("shell-dropdown")
                        .label(current_shell.display_name())
                        .outline()
                        .w_full()
                        .dropdown_menu(move |menu, _, _cx| {
                            let mut menu = menu.min_w(px(200.0));
                            for shell in WindowsShell::all() {
                                let is_current = current_shell == *shell;
                                menu = menu.menu_with_check(
                                    shell.display_name(),
                                    is_current,
                                    Box::new(SwitchShell(shell.clone())),
                                );
                            }
                            menu
                        }),
                ),
        )
        .child(
            div()
                .pt_2()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child("Press Ctrl+, to close (changes apply to new terminals)"),
        )
}

/// Render the settings dialog content (Linux version with decoration selection).
#[cfg(target_os = "linux")]
pub fn render_settings_content(_window: &mut Window, cx: &mut App) -> impl IntoElement {
    let current_theme = cx.theme().theme_name().clone();
    let current_font = Theme::global(cx).font_family.to_string();
    let current_decorations = load_linux_decorations();

    v_flex()
        .gap_4()
        // Theme selection dropdown
        .child(
            v_flex()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .text_color(cx.theme().muted_foreground)
                        .child("Theme"),
                )
                .child(
                    Button::new("theme-dropdown")
                        .label(current_theme.clone())
                        .outline()
                        .w_full()
                        .dropdown_menu(move |menu, _, cx| {
                            let themes = ThemeRegistry::global(cx).sorted_themes();
                            let current = cx.theme().theme_name().clone();
                            let mut menu = menu.min_w(px(200.0));
                            for theme in themes {
                                let name = theme.name.clone();
                                let is_current = current == name;
                                menu = menu.menu_with_check(
                                    name.clone(),
                                    is_current,
                                    Box::new(SwitchTheme(name)),
                                );
                            }
                            menu
                        }),
                ),
        )
        // Font family selection dropdown
        .child(
            v_flex()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .text_color(cx.theme().muted_foreground)
                        .child("Terminal Font"),
                )
                .child(
                    Button::new("font-dropdown")
                        .label(current_font.clone())
                        .outline()
                        .w_full()
                        .dropdown_menu(move |menu, _, cx| {
                            let current = Theme::global(cx).font_family.to_string();
                            let mut menu = menu.min_w(px(200.0));
                            for font in TERMINAL_FONTS {
                                let is_current = current == *font;
                                let font_name: SharedString = (*font).into();
                                menu = menu.menu_with_check(
                                    *font,
                                    is_current,
                                    Box::new(SwitchFont(font_name)),
                                );
                            }
                            menu
                        }),
                ),
        )
        // Window decorations dropdown (Linux only)
        .child(
            v_flex()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .text_color(cx.theme().muted_foreground)
                        .child("Window Decorations"),
                )
                .child(
                    Button::new("decorations-dropdown")
                        .label(current_decorations.display_name())
                        .outline()
                        .w_full()
                        .dropdown_menu(move |menu, _, _cx| {
                            let mut menu = menu.min_w(px(200.0));
                            for dec in LinuxDecorations::all() {
                                let is_current = current_decorations == *dec;
                                menu = menu.menu_with_check(
                                    dec.display_name(),
                                    is_current,
                                    Box::new(SwitchDecorations(dec.clone())),
                                );
                            }
                            menu
                        }),
                ),
        )
        .child(
            div()
                .pt_2()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child("Press Ctrl+, to close (decoration changes require restart)"),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== Terminal Fonts List Tests ====================

    mod terminal_fonts_tests {
        use super::TERMINAL_FONTS;

        #[test]
        fn test_terminal_fonts_has_reasonable_count() {
            // Should have at least a few font options
            assert!(
                TERMINAL_FONTS.len() >= 3,
                "TERMINAL_FONTS should have at least 3 options, got {}",
                TERMINAL_FONTS.len()
            );
            // But not too many to be overwhelming
            assert!(
                TERMINAL_FONTS.len() <= 50,
                "TERMINAL_FONTS should have at most 50 options, got {}",
                TERMINAL_FONTS.len()
            );
        }

        #[test]
        fn test_all_font_names_non_empty() {
            for (i, font) in TERMINAL_FONTS.iter().enumerate() {
                assert!(!font.is_empty(), "Font at index {} should not be empty", i);
            }
        }

        #[test]
        fn test_all_font_names_trimmed() {
            for (i, font) in TERMINAL_FONTS.iter().enumerate() {
                assert_eq!(
                    *font,
                    font.trim(),
                    "Font at index {} ('{}') should be trimmed",
                    i,
                    font
                );
            }
        }

        #[test]
        fn test_no_duplicate_fonts() {
            let mut seen = std::collections::HashSet::new();
            for font in TERMINAL_FONTS {
                assert!(seen.insert(*font), "Duplicate font found: '{}'", font);
            }
        }

        #[test]
        fn test_default_font_in_list() {
            // The default font from config should be in the list
            use crate::config::terminal::FONT_FAMILY;
            assert!(
                TERMINAL_FONTS.contains(&FONT_FAMILY),
                "Default font '{}' should be in TERMINAL_FONTS list",
                FONT_FAMILY
            );
        }

        #[test]
        fn test_fonts_are_likely_monospace() {
            // All fonts should contain indicators of being monospace
            let monospace_indicators = [
                "Mono", "Code", "Consolas", "Monaco", "Menlo", "Courier", "Iosevka", "Source",
                "Cascadia", "Ubuntu", "Fira",
            ];

            for font in TERMINAL_FONTS {
                let has_indicator = monospace_indicators
                    .iter()
                    .any(|indicator| font.contains(indicator));
                assert!(
                    has_indicator,
                    "Font '{}' doesn't appear to be a monospace font",
                    font
                );
            }
        }

        #[test]
        fn test_common_fonts_present() {
            // Some very common monospace fonts should be in the list
            let common_fonts = ["JetBrains Mono", "Fira Code", "Monaco", "Menlo"];

            for expected in common_fonts {
                assert!(
                    TERMINAL_FONTS.contains(&expected),
                    "Common font '{}' should be in TERMINAL_FONTS",
                    expected
                );
            }
        }

        #[test]
        fn test_font_name_lengths_reasonable() {
            for font in TERMINAL_FONTS {
                // Font names should be reasonable length
                assert!(
                    font.len() >= 4,
                    "Font name '{}' is too short (min 4 chars)",
                    font
                );
                assert!(
                    font.len() <= 50,
                    "Font name '{}' is too long (max 50 chars)",
                    font
                );
            }
        }

        #[test]
        fn test_fonts_use_valid_characters() {
            for font in TERMINAL_FONTS {
                // Font names should only contain alphanumeric, spaces, and some special chars
                for c in font.chars() {
                    assert!(
                        c.is_alphanumeric() || c == ' ' || c == '-' || c == '_',
                        "Font '{}' contains invalid character '{}'",
                        font,
                        c
                    );
                }
            }
        }
    }

    // ==================== Settings Dialog Width Tests ====================

    mod dialog_width_tests {
        #[test]
        fn test_settings_dialog_width_matches_config() {
            use crate::config::dialog::SETTINGS_WIDTH;
            // The hardcoded 500.0 in toggle_settings_dialog should match SETTINGS_WIDTH
            assert_eq!(
                SETTINGS_WIDTH, 500.0,
                "Settings dialog width should match config constant"
            );
        }
    }

    // ==================== Font List Ordering Tests ====================

    mod font_ordering_tests {
        use super::TERMINAL_FONTS;

        #[test]
        fn test_first_font_is_default() {
            // The first font in the list should be the default
            use crate::config::terminal::FONT_FAMILY;
            assert_eq!(
                TERMINAL_FONTS.first(),
                Some(&FONT_FAMILY),
                "First font in list should be the default font"
            );
        }

        #[test]
        fn test_builtin_fonts_near_top() {
            // Built-in platform fonts should be prioritized at the top
            // These are guaranteed to exist on the system
            #[cfg(target_os = "macos")]
            let builtin = ["Menlo", "Monaco", "SF Mono", "Courier New"];
            #[cfg(target_os = "windows")]
            let builtin = ["Consolas", "Courier New", "Lucida Console"];
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let builtin = ["monospace", "DejaVu Sans Mono"];

            let half = TERMINAL_FONTS.len() / 2;

            for font in builtin {
                if let Some(pos) = TERMINAL_FONTS.iter().position(|&f| f == font) {
                    assert!(
                        pos <= half,
                        "Built-in font '{}' should be in first half of list (position {}, half is {})",
                        font,
                        pos,
                        half
                    );
                }
            }
        }
    }

    // ==================== Font Name Validation Tests ====================

    mod font_validation_tests {
        use super::TERMINAL_FONTS;

        #[test]
        fn test_fonts_fit_in_settings_max_string_length() {
            use crate::config::settings::MAX_STRING_LENGTH;

            for font in TERMINAL_FONTS {
                assert!(
                    font.len() <= MAX_STRING_LENGTH,
                    "Font '{}' (len {}) exceeds MAX_STRING_LENGTH ({})",
                    font,
                    font.len(),
                    MAX_STRING_LENGTH
                );
            }
        }

        #[test]
        fn test_fonts_are_printable() {
            for font in TERMINAL_FONTS {
                for c in font.chars() {
                    assert!(
                        !c.is_control(),
                        "Font '{}' contains control character",
                        font
                    );
                }
            }
        }
    }
}
