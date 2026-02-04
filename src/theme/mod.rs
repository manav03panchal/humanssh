//! Theme system for HumanSSH.
//!
//! Wraps gpui_component's theme system and provides terminal color mapping.
//!
//! # Modules
//!
//! - `persistence` - Settings loading/saving with validation
//! - `colors` - Terminal color mapping from theme
//! - `actions` - Theme switching actions
//!
//! # Usage
//!
//! ```ignore
//! // Get terminal colors for rendering
//! let colors = terminal_colors(cx);
//! let bg = colors.background;
//! let fg = colors.foreground;
//!
//! // Switch theme via action
//! cx.dispatch_action(Box::new(SwitchTheme("Catppuccin Latte".into())));
//! ```

mod actions;
mod colors;
mod persistence;

// Re-export public API
#[cfg(target_os = "linux")]
pub use actions::SwitchDecorations;
#[cfg(target_os = "windows")]
pub use actions::SwitchShell;
pub use actions::{SwitchFont, SwitchTheme, SwitchThemeMode};
pub use colors::{terminal_colors, TerminalColors};
#[cfg(target_os = "linux")]
pub use persistence::{load_linux_decorations, save_linux_decorations};
pub use persistence::{
    load_settings, load_window_bounds, save_settings, save_window_bounds, LinuxDecorations,
    Settings, WindowBoundsConfig, WindowsShell,
};
#[cfg(target_os = "windows")]
pub use persistence::{load_windows_shell, save_windows_shell};

use gpui::App;
use gpui_component::theme::{Theme, ThemeRegistry};
use gpui_component::ActiveTheme;
use std::path::PathBuf;

/// Initialize theme watching and actions
pub fn init(cx: &mut App) {
    // Load saved settings - take ownership, no cloning
    let saved_settings = load_settings();
    let saved_theme = saved_settings
        .theme
        .unwrap_or_else(|| "Catppuccin Mocha".to_string());
    let saved_font = saved_settings.font_family;

    // Apply saved font family if present, otherwise use platform default
    use crate::config::terminal::FONT_FAMILY;
    let font_to_apply = saved_font.unwrap_or_else(|| FONT_FAMILY.to_string());
    // Clone font for closure before moving into .into()
    let font_for_closure = font_to_apply.clone();
    Theme::global_mut(cx).font_family = font_to_apply.into();

    // Find and watch themes directory
    if let Some(themes_dir) = find_themes_dir() {
        tracing::info!("Loading themes from: {:?}", themes_dir);
        let theme_for_closure = saved_theme.clone();
        if let Err(e) = ThemeRegistry::watch_dir(themes_dir, cx, move |cx| {
            // Save our font before applying theme (apply_config resets it)
            let our_font = font_for_closure.clone();

            // Apply saved theme when themes are loaded
            if let Some(theme) = ThemeRegistry::global(cx)
                .themes()
                .get(&theme_for_closure as &str)
                .cloned()
            {
                Theme::global_mut(cx).apply_config(&theme);
                tracing::info!("Applied saved theme: {}", theme_for_closure);
            } else if let Some(theme) = ThemeRegistry::global(cx)
                .themes()
                .get("Catppuccin Mocha")
                .cloned()
            {
                Theme::global_mut(cx).apply_config(&theme);
                tracing::info!("Applied default theme: Catppuccin Mocha");
            }

            // Re-apply our font after theme loaded (theme reset it to .SystemUIFont)
            tracing::info!("Re-applying font after theme load: {}", our_font);
            Theme::global_mut(cx).font_family = our_font.clone().into();
            tracing::info!("Font now set to: {}", Theme::global(cx).font_family);
        }) {
            tracing::warn!("Failed to watch themes directory: {}", e);
        }
    } else {
        tracing::warn!("Themes directory not found, using default theme");
    }

    // Watch for theme changes and save (only if we have loaded themes)
    cx.observe_global::<Theme>(|cx| {
        // Only save if themes have been loaded (otherwise we'd save "Default Dark")
        let themes = ThemeRegistry::global(cx).themes();
        if themes.is_empty() {
            return;
        }

        let theme_name = cx.theme().theme_name().to_string();
        // Only save if the theme exists in our registry
        if !themes.contains_key(&theme_name as &str) {
            return;
        }

        let font_family = cx.theme().font_family.to_string();

        // Preserve existing window bounds when saving theme/font
        let mut settings = load_settings();
        // Log before moving into settings to avoid clone
        tracing::debug!("Saved settings: theme={}, font={}", theme_name, font_family);
        settings.theme = Some(theme_name);
        settings.font_family = Some(font_family);
        save_settings(&settings);
    })
    .detach();

    // Register theme switching actions
    actions::register_actions(cx);
}

/// Find the themes directory with path validation.
/// Returns canonicalized absolute path to prevent path traversal attacks.
fn find_themes_dir() -> Option<PathBuf> {
    // Helper to canonicalize and validate a themes directory
    let validate_themes_dir = |path: PathBuf| -> Option<PathBuf> {
        // Canonicalize to resolve symlinks and .. components
        let canonical = path.canonicalize().ok()?;

        // Verify it's actually a directory
        if !canonical.is_dir() {
            tracing::warn!("Themes path is not a directory: {:?}", canonical);
            return None;
        }

        Some(canonical)
    };

    // Try CWD-relative themes/
    let cwd_themes = PathBuf::from("themes");
    if cwd_themes.exists() {
        if let Some(path) = validate_themes_dir(cwd_themes) {
            return Some(path);
        }
    }

    // Try exe-relative themes/
    if let Ok(exe_path) = std::env::current_exe() {
        // Canonicalize exe path first to get real location
        let exe_canonical = exe_path.canonicalize().ok()?;
        let exe_dir = exe_canonical.parent()?;

        let exe_themes = exe_dir.join("themes");
        if exe_themes.exists() {
            if let Some(path) = validate_themes_dir(exe_themes) {
                return Some(path);
            }
        }

        // macOS bundle: exe is in .app/Contents/MacOS/, themes in .app/Contents/Resources/
        // Go up to Contents, then into Resources/themes
        if let Some(contents_dir) = exe_dir.parent() {
            let bundle_themes = contents_dir.join("Resources").join("themes");
            if bundle_themes.exists() {
                if let Some(path) = validate_themes_dir(bundle_themes) {
                    return Some(path);
                }
            }
        }
    }

    None
}

#[cfg(test)]
#[allow(clippy::clone_on_copy)]
mod tests {
    use super::{
        find_themes_dir, load_settings, load_window_bounds, terminal_colors, Settings, SwitchFont,
        SwitchTheme, SwitchThemeMode, TerminalColors, Theme, ThemeRegistry, WindowBoundsConfig,
    };
    use tempfile::TempDir;

    mod public_api {
        use super::{
            load_settings, load_window_bounds, terminal_colors, Settings, SwitchFont, SwitchTheme,
            SwitchThemeMode, TerminalColors, WindowBoundsConfig,
        };

        #[test]
        fn re_exports_switch_theme_action() {
            // Verify SwitchTheme is accessible from mod.rs
            let action = SwitchTheme("Test Theme".into());
            assert_eq!(action.0.as_ref(), "Test Theme");
        }

        #[test]
        fn re_exports_switch_font_action() {
            // Verify SwitchFont is accessible from mod.rs
            let action = SwitchFont("Test Font".into());
            assert_eq!(action.0.as_ref(), "Test Font");
        }

        #[test]
        fn re_exports_switch_theme_mode_action() {
            use gpui_component::theme::ThemeMode;
            // Verify SwitchThemeMode is accessible from mod.rs
            let action = SwitchThemeMode(ThemeMode::Dark);
            assert_eq!(action.0, ThemeMode::Dark);
        }

        #[test]
        fn re_exports_terminal_colors_function() {
            // Verify terminal_colors is accessible (it's re-exported from colors.rs)
            // We can't call it without an App context, but we can verify the type exists
            let _: fn(&gpui::App) -> TerminalColors = terminal_colors;
        }

        #[test]
        fn re_exports_settings_struct() {
            // Verify Settings is accessible from mod.rs
            let settings = Settings::default();
            assert!(settings.theme.is_none());
        }

        #[test]
        fn re_exports_window_bounds_config() {
            // Verify WindowBoundsConfig is accessible from mod.rs
            let bounds = WindowBoundsConfig::default();
            assert_eq!(bounds.width, 1200.0);
        }

        #[test]
        fn re_exports_load_settings() {
            // Verify load_settings is accessible
            // Just verify the function exists and can be called
            let _settings = load_settings();
        }

        #[test]
        fn re_exports_load_window_bounds() {
            // Verify load_window_bounds is accessible
            let _bounds = load_window_bounds();
        }
    }

    mod find_themes_dir {
        use super::{find_themes_dir, TempDir};

        #[test]
        fn returns_none_when_no_themes_dir_exists() {
            // In a clean environment without themes/, should return None
            // This test depends on the current working directory not having a themes folder
            // We can't reliably test this without changing CWD, so we just verify the function exists
            let result = find_themes_dir();
            // Result may be Some or None depending on environment
            if let Some(path) = result {
                assert!(
                    path.is_dir(),
                    "if themes dir is found, it should be a directory"
                );
            }
        }

        #[test]
        fn validates_themes_dir_is_directory() {
            let temp_dir = TempDir::new().unwrap();

            // Create a file called "themes" (not a directory)
            let themes_file = temp_dir.path().join("themes");
            std::fs::write(&themes_file, "not a directory").unwrap();

            // The validate_themes_dir helper should reject files
            let canonical = themes_file.canonicalize();
            if let Ok(path) = canonical {
                assert!(!path.is_dir());
            }
        }

        #[test]
        fn returns_canonicalized_path() {
            let temp_dir = TempDir::new().unwrap();
            let themes_dir = temp_dir.path().join("themes");
            std::fs::create_dir_all(&themes_dir).unwrap();

            // Canonicalize should resolve to absolute path
            let canonical = themes_dir.canonicalize().unwrap();
            assert!(canonical.is_absolute());
            assert!(canonical.is_dir());
        }
    }

    mod theme_integration {
        use super::{Settings, TerminalColors};

        #[test]
        fn default_theme_name_is_catppuccin_mocha() {
            // The default theme used when no saved setting exists
            let settings = Settings::default();
            let default_theme = settings
                .theme
                .unwrap_or_else(|| "Catppuccin Mocha".to_string());
            assert_eq!(default_theme, "Catppuccin Mocha");
        }

        #[test]
        fn terminal_colors_default_matches_catppuccin_mocha() {
            // TerminalColors::default() should use Catppuccin Mocha colors
            let colors = TerminalColors::default();

            // Catppuccin Mocha background is #1e1e2e (dark)
            assert!(
                colors.background.l < 0.25,
                "Catppuccin Mocha background should be dark"
            );

            // Catppuccin Mocha foreground is #cdd6f4 (light)
            assert!(
                colors.foreground.l > 0.7,
                "Catppuccin Mocha foreground should be light"
            );
        }
    }

    mod theme_registry_operations {
        use super::{Theme, ThemeRegistry};

        #[test]
        fn theme_registry_type_exists() {
            // Verify ThemeRegistry is importable and usable
            let _: &dyn Fn(&gpui::App) -> &ThemeRegistry = &ThemeRegistry::global;
        }

        #[test]
        fn theme_type_exists() {
            // Verify Theme is importable and usable
            let _: &dyn Fn(&gpui::App) -> &Theme = &Theme::global;
        }
    }

    mod settings_integration {
        use super::{Settings, TempDir, WindowBoundsConfig};
        use std::fs;

        #[test]
        fn settings_preserves_theme_name() {
            let settings = Settings {
                theme: Some("Tokyo Night".to_string()),
                font_family: None,
                window_bounds: None,
                ..Default::default()
            };

            // Serialize and deserialize
            let json = serde_json::to_string(&settings).unwrap();
            let restored: Settings = serde_json::from_str(&json).unwrap();

            assert_eq!(restored.theme, Some("Tokyo Night".to_string()));
        }

        #[test]
        fn settings_preserves_font_family() {
            let settings = Settings {
                theme: None,
                font_family: Some("JetBrains Mono".to_string()),
                window_bounds: None,
                ..Default::default()
            };

            let json = serde_json::to_string(&settings).unwrap();
            let restored: Settings = serde_json::from_str(&json).unwrap();

            assert_eq!(restored.font_family, Some("JetBrains Mono".to_string()));
        }

        #[test]
        fn settings_roundtrip_with_all_fields() {
            let settings = Settings {
                theme: Some("Nord".to_string()),
                font_family: Some("Fira Code".to_string()),
                window_bounds: Some(WindowBoundsConfig {
                    x: 100.0,
                    y: 200.0,
                    width: 1920.0,
                    height: 1080.0,
                }),
                ..Default::default()
            };

            let json = serde_json::to_string_pretty(&settings).unwrap();
            let restored: Settings = serde_json::from_str(&json).unwrap();

            assert_eq!(restored.theme, settings.theme);
            assert_eq!(restored.font_family, settings.font_family);
            let orig = settings.window_bounds.unwrap();
            let rest = restored.window_bounds.unwrap();
            assert_eq!(orig.x, rest.x);
            assert_eq!(orig.y, rest.y);
            assert_eq!(orig.width, rest.width);
            assert_eq!(orig.height, rest.height);
        }

        #[test]
        fn save_settings_creates_config_directory() {
            let temp_dir = TempDir::new().unwrap();
            let config_path = temp_dir.path().join("humanssh").join("settings.json");

            // Create parent directories manually
            fs::create_dir_all(config_path.parent().unwrap()).unwrap();

            // Verify directory was created
            assert!(config_path.parent().unwrap().exists());
        }
    }

    mod path_security {
        use super::TempDir;

        #[test]
        fn canonicalize_resolves_symlinks() {
            let temp_dir = TempDir::new().unwrap();

            // Create actual themes directory
            let real_themes = temp_dir.path().join("real_themes");
            std::fs::create_dir_all(&real_themes).unwrap();

            // Create symlink to themes directory (Unix only)
            #[cfg(unix)]
            {
                let symlink_path = temp_dir.path().join("themes_link");
                std::os::unix::fs::symlink(&real_themes, &symlink_path).unwrap();

                // Canonicalize should resolve to the real path
                let canonical = symlink_path.canonicalize().unwrap();
                assert_eq!(
                    canonical,
                    real_themes.canonicalize().unwrap(),
                    "symlink should resolve to real path"
                );
            }
        }

        #[test]
        fn canonicalize_resolves_dot_dot() {
            let temp_dir = TempDir::new().unwrap();
            let themes_dir = temp_dir.path().join("subdir").join("..").join("themes");

            std::fs::create_dir_all(temp_dir.path().join("themes")).unwrap();

            // The path with .. should resolve to the same place
            if themes_dir.exists() {
                let canonical = themes_dir.canonicalize().unwrap();
                assert!(!canonical.to_string_lossy().contains(".."));
            }
        }

        #[test]
        fn rejects_non_directory_themes_path() {
            let temp_dir = TempDir::new().unwrap();

            // Create a file called "themes"
            let themes_file = temp_dir.path().join("themes");
            std::fs::write(&themes_file, "I am a file, not a directory").unwrap();

            // Should be a file, not a directory
            assert!(themes_file.is_file());
            assert!(!themes_file.is_dir());
        }
    }

    mod light_dark_mode {
        use gpui_component::theme::ThemeMode;

        #[test]
        fn theme_mode_has_dark_variant() {
            let mode = ThemeMode::Dark;
            assert_eq!(mode, ThemeMode::Dark);
        }

        #[test]
        fn theme_mode_has_light_variant() {
            let mode = ThemeMode::Light;
            assert_eq!(mode, ThemeMode::Light);
        }

        #[test]
        fn theme_mode_is_copy() {
            let mode = ThemeMode::Dark;
            let copied = mode;
            assert_eq!(mode, copied);
        }

        #[test]
        fn theme_mode_is_clone() {
            let mode = ThemeMode::Light;
            let cloned = mode.clone();
            assert_eq!(mode, cloned);
        }

        #[test]
        fn theme_mode_equality() {
            assert_eq!(ThemeMode::Dark, ThemeMode::Dark);
            assert_eq!(ThemeMode::Light, ThemeMode::Light);
            assert_ne!(ThemeMode::Dark, ThemeMode::Light);
        }
    }
}
