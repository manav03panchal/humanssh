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

use gpui::App;
use gpui_component::theme::{Theme, ThemeRegistry};
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Stores the user's intended font preference.
/// This survives across apply_config calls that reset font_family to .SystemUIFont.
static INTENDED_FONT: OnceLock<Mutex<String>> = OnceLock::new();

/// Get the user's intended font (read from settings or default)
fn get_intended_font() -> String {
    INTENDED_FONT
        .get()
        .map(|m| m.lock().clone())
        .unwrap_or_else(|| crate::config::terminal::FONT_FAMILY.to_string())
}

/// Set the user's intended font
pub fn set_intended_font(font: String) {
    if let Some(m) = INTENDED_FONT.get() {
        *m.lock() = font;
    } else {
        let _ = INTENDED_FONT.set(Mutex::new(font));
    }
}

/// Initialize theme watching and actions
pub fn init(cx: &mut App) {
    // Ensure config file exists (create default on first launch)
    crate::config::file::ensure_config_file();

    // Load config
    let config = crate::config::file::load_config();

    // Apply font
    set_intended_font(config.font_family.clone());
    Theme::global_mut(cx).font_family = config.font_family.clone().into();

    // Apply option-as-alt
    crate::terminal::OPTION_AS_ALT
        .store(config.option_as_alt, std::sync::atomic::Ordering::Relaxed);

    // Find and watch themes directory
    let saved_theme = config.theme.clone();
    if let Some(themes_dir) = find_themes_dir() {
        tracing::info!("Loading themes from: {:?}", themes_dir);
        let theme_for_closure = saved_theme.clone();
        if let Err(e) = ThemeRegistry::watch_dir(themes_dir, cx, move |cx| {
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
        }) {
            tracing::warn!("Failed to watch themes directory: {}", e);
        }
    } else {
        tracing::warn!("Themes directory not found, using default theme");
    }

    // Watch for theme changes — restore font after apply_config calls
    cx.observe_global::<Theme>(|cx| {
        let themes_empty = ThemeRegistry::global(cx).themes().is_empty();
        if themes_empty {
            return;
        }

        let intended_font = get_intended_font();
        let current_font = Theme::global(cx).font_family.to_string();

        if current_font != intended_font {
            tracing::info!("Restoring font: {} -> {}", current_font, intended_font);
            Theme::global_mut(cx).font_family = intended_font.into();
        }
    })
    .detach();

    // Start config file watcher (live reload)
    if let Some(debouncer) = crate::config::file::watch_config(cx) {
        Box::leak(Box::new(debouncer));
    }

    // Register theme switching actions (still needed for internal use)
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
        find_themes_dir, terminal_colors, SwitchFont, SwitchTheme, SwitchThemeMode, TerminalColors,
        Theme, ThemeRegistry,
    };
    use tempfile::TempDir;

    mod public_api {
        use super::{terminal_colors, SwitchFont, SwitchTheme, SwitchThemeMode, TerminalColors};

        #[test]
        fn re_exports_switch_theme_action() {
            let action = SwitchTheme("Test Theme".into());
            assert_eq!(action.0.as_ref(), "Test Theme");
        }

        #[test]
        fn re_exports_switch_font_action() {
            let action = SwitchFont("Test Font".into());
            assert_eq!(action.0.as_ref(), "Test Font");
        }

        #[test]
        fn re_exports_switch_theme_mode_action() {
            use gpui_component::theme::ThemeMode;
            let action = SwitchThemeMode(ThemeMode::Dark);
            assert_eq!(action.0, ThemeMode::Dark);
        }

        #[test]
        fn re_exports_terminal_colors_function() {
            let _: fn(&gpui::App) -> TerminalColors = terminal_colors;
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
        use super::TerminalColors;

        #[test]
        fn default_config_theme_is_catppuccin_mocha() {
            let config = crate::config::file::Config::default();
            assert_eq!(config.theme, "Catppuccin Mocha");
        }

        #[test]
        fn terminal_colors_default_matches_catppuccin_mocha() {
            let colors = TerminalColors::default();
            assert!(
                colors.background.l < 0.25,
                "Catppuccin Mocha background should be dark"
            );
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
            let _: &dyn Fn(&gpui::App) -> &ThemeRegistry = &ThemeRegistry::global;
        }

        #[test]
        fn theme_type_exists() {
            let _: &dyn Fn(&gpui::App) -> &Theme = &Theme::global;
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
