//! Settings persistence for HumanSSH.
//!
//! Handles loading and saving user settings to disk with validation.

use crate::config::settings::{self, window as window_limits};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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

impl WindowBoundsConfig {
    /// Validate window bounds, clamping to reasonable limits.
    pub fn validate(&mut self) {
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

/// Settings that persist across sessions
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Settings {
    pub theme: Option<String>,
    pub font_family: Option<String>,
    pub window_bounds: Option<WindowBoundsConfig>,
}

impl Settings {
    /// Validate and sanitize settings, replacing invalid values with defaults.
    pub fn validate(&mut self) {
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

/// Get the settings file path (cross-platform).
/// Uses platform-appropriate config directory:
/// - Linux: ~/.config/humanssh/settings.json
/// - macOS: ~/Library/Application Support/humanssh/settings.json
/// - Windows: C:\Users\<user>\AppData\Roaming\humanssh\settings.json
fn settings_path() -> Option<PathBuf> {
    dirs::config_dir().map(|config| config.join("humanssh").join("settings.json"))
}

/// Load settings from disk with validation
pub fn load_settings() -> Settings {
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

/// Save settings to disk
pub fn save_settings(settings: &Settings) {
    let Some(path) = settings_path() else {
        tracing::warn!("Could not determine settings path");
        return;
    };

    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            tracing::warn!("Failed to create settings directory: {}", e);
            return;
        }
    }

    // Serialize settings
    let json = match serde_json::to_string_pretty(settings) {
        Ok(j) => j,
        Err(e) => {
            tracing::warn!("Failed to serialize settings: {}", e);
            return;
        }
    };

    // Write to file
    if let Err(e) = std::fs::write(&path, json) {
        tracing::warn!("Failed to write settings file: {}", e);
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
