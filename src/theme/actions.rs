//! Theme switching actions.
//!
//! Provides GPUI actions for switching themes and fonts.

use gpui::{App, SharedString};
use gpui_component::theme::{Theme, ThemeMode, ThemeRegistry};

/// Action to switch theme by name
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = theme, no_json)]
pub struct SwitchTheme(pub SharedString);

/// Action to switch font family
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = theme, no_json)]
pub struct SwitchFont(pub SharedString);

/// Action to switch theme mode (light/dark)
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = theme, no_json)]
pub struct SwitchThemeMode(pub ThemeMode);

/// Action to switch Windows shell (Windows only)
#[cfg(target_os = "windows")]
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = theme, no_json)]
pub struct SwitchShell(pub super::persistence::WindowsShell);

/// Action to switch Linux window decorations (Linux only)
#[cfg(target_os = "linux")]
#[derive(Clone, PartialEq, Debug, gpui::Action)]
#[action(namespace = theme, no_json)]
pub struct SwitchDecorations(pub super::persistence::LinuxDecorations);

/// Register theme switching actions
pub fn register_actions(cx: &mut App) {
    cx.on_action(|action: &SwitchTheme, cx| {
        if let Some(theme_config) = ThemeRegistry::global(cx).themes().get(&action.0).cloned() {
            // Preserve current font before applying theme (apply_config resets it to .SystemUIFont)
            let current_font = Theme::global(cx).font_family.clone();
            Theme::global_mut(cx).apply_config(&theme_config);
            // Re-apply the font after theme switch
            Theme::global_mut(cx).font_family = current_font;
            tracing::info!("Switched to theme: {}", action.0);
        }
        cx.refresh_windows();
    });

    cx.on_action(|action: &SwitchFont, cx| {
        Theme::global_mut(cx).font_family = action.0.clone();
        tracing::info!("Switched to font: {}", action.0);
        cx.refresh_windows();
    });

    cx.on_action(|action: &SwitchThemeMode, cx| {
        Theme::change(action.0, None, cx);
        cx.refresh_windows();
    });

    // Register shell switching action (Windows only)
    #[cfg(target_os = "windows")]
    cx.on_action(|action: &SwitchShell, cx| {
        super::persistence::save_windows_shell(action.0.clone());
        tracing::info!("Switched to shell: {:?}", action.0);
        cx.refresh_windows();
    });

    // Register decoration switching action (Linux only)
    #[cfg(target_os = "linux")]
    cx.on_action(|action: &SwitchDecorations, cx| {
        super::persistence::save_linux_decorations(action.0.clone());
        tracing::info!("Switched to decorations: {:?} (restart required)", action.0);
        cx.refresh_windows();
    });
}

#[cfg(test)]
mod tests {
    use super::{SwitchFont, SwitchTheme, SwitchThemeMode};

    mod switch_theme_action {
        use super::SwitchTheme;
        use gpui::SharedString;
        use pretty_assertions::assert_eq;

        #[test]
        fn can_create_with_string_literal() {
            let action = SwitchTheme("Catppuccin Mocha".into());
            assert_eq!(action.0.as_ref(), "Catppuccin Mocha");
        }

        #[test]
        fn can_create_with_shared_string() {
            let name: SharedString = "Tokyo Night".into();
            let action = SwitchTheme(name);
            assert_eq!(action.0.as_ref(), "Tokyo Night");
        }

        #[test]
        fn clone_creates_identical_action() {
            let action = SwitchTheme("Dracula".into());
            let cloned = action.clone();
            assert_eq!(action.0, cloned.0);
        }

        #[test]
        fn equality_works_for_same_theme() {
            let action1 = SwitchTheme("Nord".into());
            let action2 = SwitchTheme("Nord".into());
            assert_eq!(action1, action2);
        }

        #[test]
        fn equality_works_for_different_themes() {
            let action1 = SwitchTheme("Nord".into());
            let action2 = SwitchTheme("Solarized".into());
            assert_ne!(action1, action2);
        }

        #[test]
        fn can_create_with_empty_string() {
            let action = SwitchTheme("".into());
            assert_eq!(action.0.as_ref(), "");
        }

        #[test]
        fn can_create_with_unicode() {
            let action = SwitchTheme("テーマ".into());
            assert_eq!(action.0.as_ref(), "テーマ");
        }

        #[test]
        fn can_create_with_special_characters() {
            let action = SwitchTheme("Theme (Dark) v2.0".into());
            assert_eq!(action.0.as_ref(), "Theme (Dark) v2.0");
        }
    }

    mod switch_font_action {
        use super::SwitchFont;
        use gpui::SharedString;
        use pretty_assertions::assert_eq;

        #[test]
        fn can_create_with_string_literal() {
            let action = SwitchFont("Iosevka Nerd Font".into());
            assert_eq!(action.0.as_ref(), "Iosevka Nerd Font");
        }

        #[test]
        fn can_create_with_shared_string() {
            let name: SharedString = "JetBrains Mono".into();
            let action = SwitchFont(name);
            assert_eq!(action.0.as_ref(), "JetBrains Mono");
        }

        #[test]
        fn clone_creates_identical_action() {
            let action = SwitchFont("Fira Code".into());
            let cloned = action.clone();
            assert_eq!(action.0, cloned.0);
        }

        #[test]
        fn equality_works_for_same_font() {
            let action1 = SwitchFont("Hack".into());
            let action2 = SwitchFont("Hack".into());
            assert_eq!(action1, action2);
        }

        #[test]
        fn equality_works_for_different_fonts() {
            let action1 = SwitchFont("Hack".into());
            let action2 = SwitchFont("Monaco".into());
            assert_ne!(action1, action2);
        }

        #[test]
        fn can_create_with_font_family_style() {
            let action = SwitchFont("SF Mono Regular".into());
            assert_eq!(action.0.as_ref(), "SF Mono Regular");
        }
    }

    mod switch_theme_mode_action {
        use super::SwitchThemeMode;
        use gpui_component::theme::ThemeMode;
        use pretty_assertions::assert_eq;

        #[test]
        fn can_create_with_dark_mode() {
            let action = SwitchThemeMode(ThemeMode::Dark);
            assert_eq!(action.0, ThemeMode::Dark);
        }

        #[test]
        fn can_create_with_light_mode() {
            let action = SwitchThemeMode(ThemeMode::Light);
            assert_eq!(action.0, ThemeMode::Light);
        }

        #[test]
        fn clone_creates_identical_action() {
            let action = SwitchThemeMode(ThemeMode::Dark);
            let cloned = action.clone();
            assert_eq!(action.0, cloned.0);
        }

        #[test]
        fn equality_works_for_same_mode() {
            let action1 = SwitchThemeMode(ThemeMode::Light);
            let action2 = SwitchThemeMode(ThemeMode::Light);
            assert_eq!(action1, action2);
        }

        #[test]
        fn equality_works_for_different_modes() {
            let action1 = SwitchThemeMode(ThemeMode::Dark);
            let action2 = SwitchThemeMode(ThemeMode::Light);
            assert_ne!(action1, action2);
        }
    }

    mod action_integration {
        use super::{SwitchFont, SwitchTheme, SwitchThemeMode};
        use gpui_component::theme::ThemeMode;

        #[test]
        fn all_actions_are_clone() {
            // Verify Clone is implemented for all actions
            let theme_action = SwitchTheme("Test".into());
            let font_action = SwitchFont("Mono".into());
            let mode_action = SwitchThemeMode(ThemeMode::Dark);

            let _theme_clone = theme_action.clone();
            let _font_clone = font_action.clone();
            let _mode_clone = mode_action.clone();
        }

        #[test]
        fn all_actions_are_partial_eq() {
            // Verify PartialEq is implemented for all actions
            let theme1 = SwitchTheme("Test".into());
            let theme2 = SwitchTheme("Test".into());
            assert!(theme1 == theme2);

            let font1 = SwitchFont("Mono".into());
            let font2 = SwitchFont("Mono".into());
            assert!(font1 == font2);

            let mode1 = SwitchThemeMode(ThemeMode::Dark);
            let mode2 = SwitchThemeMode(ThemeMode::Dark);
            assert!(mode1 == mode2);
        }
    }

    mod theme_name_variations {
        use super::{SwitchFont, SwitchTheme};

        #[test]
        fn common_theme_names() {
            // Test common theme names that users might use
            let themes = vec![
                "Catppuccin Mocha",
                "Catppuccin Latte",
                "Catppuccin Frappe",
                "Catppuccin Macchiato",
                "Tokyo Night",
                "Tokyo Night Storm",
                "Tokyo Night Light",
                "Dracula",
                "Nord",
                "One Dark Pro",
                "Solarized Dark",
                "Solarized Light",
                "Gruvbox Dark",
                "Gruvbox Light",
                "GitHub Dark",
                "GitHub Light",
            ];

            for theme in themes {
                let action = SwitchTheme(theme.into());
                assert_eq!(action.0.as_ref(), theme);
            }
        }

        #[test]
        fn common_font_names() {
            // Test common monospace font names
            let fonts = vec![
                "Iosevka Nerd Font",
                "JetBrains Mono",
                "Fira Code",
                "SF Mono",
                "Monaco",
                "Menlo",
                "Consolas",
                "Source Code Pro",
                "Hack",
                "Ubuntu Mono",
                "Cascadia Code",
            ];

            for font in fonts {
                let action = SwitchFont(font.into());
                assert_eq!(action.0.as_ref(), font);
            }
        }
    }
}
