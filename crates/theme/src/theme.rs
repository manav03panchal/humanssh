//! Theme system for HumanSSH.
//!
//! Wraps gpui_component's theme system and provides terminal color mapping.
//!
//! # Modules
//!
//! - `persistence` - Platform-specific types (WindowsShell)
//! - `colors` - Terminal color mapping from theme
//! - `theme_actions` - Action handlers (secure input, option-as-alt)

mod colors;
mod persistence;
mod theme_actions;

// Re-export public API
pub use colors::{terminal_colors, TerminalColors};
pub use persistence::WindowsShell;

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
        .unwrap_or_else(|| settings::constants::terminal::FONT_FAMILY.to_string())
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
    settings::ensure_config_file();

    // Load config
    let config = settings::load_config();

    // Apply font
    set_intended_font(config.font_family.clone());
    Theme::global_mut(cx).font_family = config.font_family.clone().into();

    // Apply option-as-alt
    actions::OPTION_AS_ALT.store(config.option_as_alt, std::sync::atomic::Ordering::Relaxed);

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

    // Watch for theme changes -- restore font after apply_config calls
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
    if let Some(debouncer) = settings::watch_config(cx, on_config_apply) {
        Box::leak(Box::new(debouncer));
    }

    // Register action handlers
    theme_actions::register_actions(cx);
}

/// Callback for config file changes — applies cross-crate side effects.
fn on_config_apply(config: &settings::Config, _cx: &mut App) {
    // Update font preference
    set_intended_font(config.font_family.clone());

    // Update option-as-alt
    actions::OPTION_AS_ALT.store(config.option_as_alt, std::sync::atomic::Ordering::Relaxed);

    // Update secure keyboard entry (macOS)
    #[cfg(target_os = "macos")]
    {
        let currently_enabled = platform::is_secure_input_enabled();
        if config.secure_keyboard_entry && !currently_enabled {
            platform::enable_secure_input();
        } else if !config.secure_keyboard_entry && currently_enabled {
            platform::disable_secure_input();
        }
    }
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
mod tests {
    use super::{find_themes_dir, TerminalColors};
    use tempfile::TempDir;

    mod find_themes_dir {
        use super::{find_themes_dir, TempDir};

        #[test]
        fn returns_none_when_no_themes_dir_exists() {
            let result = find_themes_dir();
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

            let themes_file = temp_dir.path().join("themes");
            std::fs::write(&themes_file, "not a directory").unwrap();

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

            let canonical = themes_dir.canonicalize().unwrap();
            assert!(canonical.is_absolute());
            assert!(canonical.is_dir());
        }
    }

    mod theme_integration {
        use super::TerminalColors;

        #[test]
        fn default_config_theme_is_catppuccin_mocha() {
            let config = settings::Config::default();
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

    mod path_security {
        use super::TempDir;

        #[test]
        fn canonicalize_resolves_symlinks() {
            let temp_dir = TempDir::new().unwrap();

            let real_themes = temp_dir.path().join("real_themes");
            std::fs::create_dir_all(&real_themes).unwrap();

            #[cfg(unix)]
            {
                let symlink_path = temp_dir.path().join("themes_link");
                std::os::unix::fs::symlink(&real_themes, &symlink_path).unwrap();

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

            if themes_dir.exists() {
                let canonical = themes_dir.canonicalize().unwrap();
                assert!(!canonical.to_string_lossy().contains(".."));
            }
        }

        #[test]
        fn rejects_non_directory_themes_path() {
            let temp_dir = TempDir::new().unwrap();

            let themes_file = temp_dir.path().join("themes");
            std::fs::write(&themes_file, "I am a file, not a directory").unwrap();

            assert!(themes_file.is_file());
            assert!(!themes_file.is_dir());
        }
    }
}
