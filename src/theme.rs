//! Theme system for HumanSSH.
//!
//! Wraps gpui_component's theme system and provides terminal color mapping.
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

use crate::config::settings::{self, window as window_limits};
use gpui::{rgb, App, Hsla, SharedString};
use gpui_component::theme::{Theme, ThemeMode, ThemeRegistry};
use gpui_component::{ActiveTheme, Colorize};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Cached terminal colors to avoid recomputation every frame
static TERMINAL_COLORS_CACHE: Mutex<Option<(SharedString, TerminalColors)>> = Mutex::new(None);

/// Window bounds for persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowBoundsConfig {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Default for WindowBoundsConfig {
    fn default() -> Self {
        Self {
            x: 100.0,
            y: 100.0,
            width: 1200.0,
            height: 800.0,
        }
    }
}

/// Settings that persist across sessions
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Settings {
    theme: Option<String>,
    font_family: Option<String>,
    window_bounds: Option<WindowBoundsConfig>,
}

/// Get the settings file path
fn settings_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join(".config")
            .join("humanssh")
            .join("settings.json")
    })
}

/// Load settings from disk with validation
fn load_settings() -> Settings {
    let Some(path) = settings_path() else {
        return Settings::default();
    };

    // Check file size before reading (DoS protection)
    let metadata = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!("Failed to read settings metadata: {}", e);
            }
            return Settings::default();
        }
    };

    if metadata.len() > settings::MAX_FILE_SIZE {
        tracing::warn!(
            "Settings file too large ({} bytes, max {}), ignoring",
            metadata.len(),
            settings::MAX_FILE_SIZE
        );
        return Settings::default();
    }

    // Read and parse
    let json = match std::fs::read_to_string(&path) {
        Ok(j) => j,
        Err(e) => {
            tracing::warn!("Failed to read settings file: {}", e);
            return Settings::default();
        }
    };

    let mut parsed: Settings = match serde_json::from_str(&json) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Failed to parse settings: {}", e);
            return Settings::default();
        }
    };

    // Validate and sanitize
    parsed.validate();
    parsed
}

impl Settings {
    /// Validate and sanitize settings, replacing invalid values with defaults.
    fn validate(&mut self) {
        // Validate theme name length
        if let Some(ref theme) = self.theme {
            if theme.len() > settings::MAX_STRING_LENGTH {
                tracing::warn!(
                    "Theme name too long ({} chars, max {}), using default",
                    theme.len(),
                    settings::MAX_STRING_LENGTH
                );
                self.theme = None;
            }
        }

        // Validate font family length
        if let Some(ref font) = self.font_family {
            if font.len() > settings::MAX_STRING_LENGTH {
                tracing::warn!(
                    "Font family too long ({} chars, max {}), using default",
                    font.len(),
                    settings::MAX_STRING_LENGTH
                );
                self.font_family = None;
            }
        }

        // Validate window bounds
        if let Some(ref mut bounds) = self.window_bounds {
            bounds.validate();
        }
    }
}

impl WindowBoundsConfig {
    /// Validate window bounds, clamping to reasonable limits.
    fn validate(&mut self) {
        // Clamp position
        self.x = self
            .x
            .clamp(window_limits::MIN_POSITION, window_limits::MAX_POSITION);
        self.y = self
            .y
            .clamp(window_limits::MIN_POSITION, window_limits::MAX_POSITION);

        // Clamp dimensions
        self.width = self
            .width
            .clamp(window_limits::MIN_SIZE, window_limits::MAX_SIZE);
        self.height = self
            .height
            .clamp(window_limits::MIN_SIZE, window_limits::MAX_SIZE);

        // Handle NaN/Infinity by replacing with defaults
        let defaults = WindowBoundsConfig::default();
        if !self.x.is_finite() {
            self.x = defaults.x;
        }
        if !self.y.is_finite() {
            self.y = defaults.y;
        }
        if !self.width.is_finite() {
            self.width = defaults.width;
        }
        if !self.height.is_finite() {
            self.height = defaults.height;
        }
    }
}

/// Save settings to disk
fn save_settings(settings: &Settings) {
    if let Some(path) = settings_path() {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(settings) {
            let _ = std::fs::write(&path, json);
        }
    }
}

/// Load saved window bounds (public API for main.rs)
pub fn load_window_bounds() -> WindowBoundsConfig {
    load_settings().window_bounds.unwrap_or_default()
}

/// Save window bounds (public API for main.rs)
pub fn save_window_bounds(bounds: WindowBoundsConfig) {
    let mut settings = load_settings();
    settings.window_bounds = Some(bounds);
    save_settings(&settings);
}

/// Initialize theme watching and actions
pub fn init(cx: &mut App) {
    // Load saved settings
    let saved_settings = load_settings();
    let saved_theme = saved_settings
        .theme
        .clone()
        .unwrap_or_else(|| "Catppuccin Mocha".to_string());
    let saved_font = saved_settings.font_family.clone();

    // Apply saved font family if present
    if let Some(font_family) = saved_font.clone() {
        Theme::global_mut(cx).font_family = font_family.into();
    }

    // Find and watch themes directory
    if let Some(themes_dir) = find_themes_dir() {
        tracing::info!("Loading themes from: {:?}", themes_dir);

        let saved_theme_clone = saved_theme.clone();
        if let Err(e) = ThemeRegistry::watch_dir(themes_dir, cx, move |cx| {
            // Apply saved theme when themes are loaded
            if let Some(theme) = ThemeRegistry::global(cx)
                .themes()
                .get(&saved_theme_clone as &str)
                .cloned()
            {
                Theme::global_mut(cx).apply_config(&theme);
                tracing::info!("Applied saved theme: {}", saved_theme_clone);
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
        settings.theme = Some(theme_name.clone());
        settings.font_family = Some(font_family.clone());
        save_settings(&settings);
        tracing::debug!("Saved settings: theme={}, font={}", theme_name, font_family);
    })
    .detach();

    // Register theme switching actions
    cx.on_action(|action: &SwitchTheme, cx| {
        if let Some(theme_config) = ThemeRegistry::global(cx).themes().get(&action.0).cloned() {
            Theme::global_mut(cx).apply_config(&theme_config);
            tracing::info!("Switched to theme: {}", action.0);
        }
        cx.refresh_windows();
    });

    // Register font switching action
    cx.on_action(|action: &SwitchFont, cx| {
        Theme::global_mut(cx).font_family = action.0.clone();
        tracing::info!("Switched to font: {}", action.0);
        cx.refresh_windows();
    });

    cx.on_action(|action: &SwitchThemeMode, cx| {
        Theme::change(action.0, None, cx);
        cx.refresh_windows();
    });
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

/// Action to switch theme by name
#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = theme, no_json)]
pub struct SwitchTheme(pub SharedString);

/// Action to switch font family
#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = theme, no_json)]
pub struct SwitchFont(pub SharedString);

/// Action to switch theme mode (light/dark)
#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = theme, no_json)]
pub struct SwitchThemeMode(pub ThemeMode);

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
