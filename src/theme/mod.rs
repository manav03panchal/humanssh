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
pub use actions::{SwitchFont, SwitchTheme, SwitchThemeMode};
pub use colors::{terminal_colors, TerminalColors};
pub use persistence::{
    load_settings, load_window_bounds, save_settings, save_window_bounds, Settings,
    WindowBoundsConfig,
};

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

    // Apply saved font family if present (consumes the Option)
    if let Some(font_family) = saved_font {
        Theme::global_mut(cx).font_family = font_family.into();
    }

    // Find and watch themes directory
    if let Some(themes_dir) = find_themes_dir() {
        tracing::info!("Loading themes from: {:?}", themes_dir);

        // Clone only once for the closure that outlives this scope
        let theme_for_closure = saved_theme.clone();
        if let Err(e) = ThemeRegistry::watch_dir(themes_dir, cx, move |cx| {
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
