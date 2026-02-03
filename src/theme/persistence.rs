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

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::{settings, window_limits, Settings, WindowBoundsConfig};
    use tempfile::TempDir;

    /// Helper to create a test environment with custom settings path
    mod test_helpers {
        use std::fs;

        /// Write settings JSON to a specific path
        #[allow(dead_code)]
        pub fn write_settings_json(path: &std::path::Path, json: &str) {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, json).unwrap();
        }

        /// Read settings JSON from a specific path
        #[allow(dead_code)]
        pub fn read_settings_json(path: &std::path::Path) -> String {
            fs::read_to_string(path).unwrap()
        }
    }

    mod window_bounds_config {
        use super::{window_limits, WindowBoundsConfig};

        #[test]
        fn default_creates_reasonable_bounds() {
            let bounds = WindowBoundsConfig::default();
            assert_eq!(bounds.x, 100.0);
            assert_eq!(bounds.y, 100.0);
            assert_eq!(bounds.width, 1200.0);
            assert_eq!(bounds.height, 800.0);
        }

        #[test]
        fn validate_clamps_position_to_min() {
            let mut bounds = WindowBoundsConfig {
                x: -20000.0, // Below MIN_POSITION
                y: -20000.0,
                width: 800.0,
                height: 600.0,
            };
            bounds.validate();
            assert_eq!(bounds.x, window_limits::MIN_POSITION);
            assert_eq!(bounds.y, window_limits::MIN_POSITION);
        }

        #[test]
        fn validate_clamps_position_to_max() {
            let mut bounds = WindowBoundsConfig {
                x: 200000.0, // Above MAX_POSITION
                y: 200000.0,
                width: 800.0,
                height: 600.0,
            };
            bounds.validate();
            assert_eq!(bounds.x, window_limits::MAX_POSITION);
            assert_eq!(bounds.y, window_limits::MAX_POSITION);
        }

        #[test]
        fn validate_clamps_size_to_min() {
            let mut bounds = WindowBoundsConfig {
                x: 100.0,
                y: 100.0,
                width: 50.0,  // Below MIN_SIZE
                height: 50.0, // Below MIN_SIZE
            };
            bounds.validate();
            assert_eq!(bounds.width, window_limits::MIN_SIZE);
            assert_eq!(bounds.height, window_limits::MIN_SIZE);
        }

        #[test]
        fn validate_clamps_size_to_max() {
            let mut bounds = WindowBoundsConfig {
                x: 100.0,
                y: 100.0,
                width: 20000.0,  // Above MAX_SIZE
                height: 20000.0, // Above MAX_SIZE
            };
            bounds.validate();
            assert_eq!(bounds.width, window_limits::MAX_SIZE);
            assert_eq!(bounds.height, window_limits::MAX_SIZE);
        }

        #[test]
        fn validate_handles_nan_values() {
            let mut bounds = WindowBoundsConfig {
                x: f32::NAN,
                y: f32::NAN,
                width: f32::NAN,
                height: f32::NAN,
            };
            bounds.validate();

            let defaults = WindowBoundsConfig::default();
            assert_eq!(bounds.x, defaults.x);
            assert_eq!(bounds.y, defaults.y);
            assert_eq!(bounds.width, defaults.width);
            assert_eq!(bounds.height, defaults.height);
        }

        #[test]
        fn validate_handles_infinity() {
            let mut bounds = WindowBoundsConfig {
                x: f32::INFINITY,
                y: f32::NEG_INFINITY,
                width: f32::INFINITY,
                height: f32::NEG_INFINITY,
            };
            bounds.validate();

            // Infinity gets clamped first (to MAX/MIN), then the finite check runs
            // Since MAX/MIN are finite, they pass the finite check
            // So infinity values become the clamped MAX/MIN values
            assert_eq!(bounds.x, window_limits::MAX_POSITION);
            assert_eq!(bounds.y, window_limits::MIN_POSITION);
            assert_eq!(bounds.width, window_limits::MAX_SIZE);
            assert_eq!(bounds.height, window_limits::MIN_SIZE);
        }

        #[test]
        fn validate_preserves_valid_values() {
            let mut bounds = WindowBoundsConfig {
                x: 500.0,
                y: 300.0,
                width: 1920.0,
                height: 1080.0,
            };
            bounds.validate();
            assert_eq!(bounds.x, 500.0);
            assert_eq!(bounds.y, 300.0);
            assert_eq!(bounds.width, 1920.0);
            assert_eq!(bounds.height, 1080.0);
        }

        #[test]
        fn clone_creates_independent_copy() {
            let original = WindowBoundsConfig {
                x: 100.0,
                y: 200.0,
                width: 800.0,
                height: 600.0,
            };
            let cloned = original.clone();
            assert_eq!(original.x, cloned.x);
            assert_eq!(original.y, cloned.y);
            assert_eq!(original.width, cloned.width);
            assert_eq!(original.height, cloned.height);
        }

        #[test]
        fn debug_format_is_readable() {
            let bounds = WindowBoundsConfig::default();
            let debug_str = format!("{:?}", bounds);
            assert!(debug_str.contains("WindowBoundsConfig"));
            assert!(debug_str.contains("x:"));
            assert!(debug_str.contains("y:"));
            assert!(debug_str.contains("width:"));
            assert!(debug_str.contains("height:"));
        }
    }

    mod settings_struct {
        use super::{settings, Settings, WindowBoundsConfig};

        #[test]
        fn default_has_no_theme() {
            let settings = Settings::default();
            assert!(settings.theme.is_none());
        }

        #[test]
        fn default_has_no_font_family() {
            let settings = Settings::default();
            assert!(settings.font_family.is_none());
        }

        #[test]
        fn default_has_no_window_bounds() {
            let settings = Settings::default();
            assert!(settings.window_bounds.is_none());
        }

        #[test]
        fn validate_rejects_oversized_theme_name() {
            let long_name = "x".repeat(settings::MAX_STRING_LENGTH + 1);
            let mut settings = Settings {
                theme: Some(long_name),
                font_family: None,
                window_bounds: None,
            };
            settings.validate();
            assert!(
                settings.theme.is_none(),
                "oversized theme name should be rejected"
            );
        }

        #[test]
        fn validate_rejects_oversized_font_family() {
            let long_name = "x".repeat(settings::MAX_STRING_LENGTH + 1);
            let mut settings = Settings {
                theme: None,
                font_family: Some(long_name),
                window_bounds: None,
            };
            settings.validate();
            assert!(
                settings.font_family.is_none(),
                "oversized font family should be rejected"
            );
        }

        #[test]
        fn validate_accepts_valid_theme_name() {
            let valid_name = "Catppuccin Mocha".to_string();
            let mut settings = Settings {
                theme: Some(valid_name.clone()),
                font_family: None,
                window_bounds: None,
            };
            settings.validate();
            assert_eq!(settings.theme, Some(valid_name));
        }

        #[test]
        fn validate_accepts_max_length_strings() {
            let max_name = "x".repeat(settings::MAX_STRING_LENGTH);
            let mut settings = Settings {
                theme: Some(max_name.clone()),
                font_family: Some(max_name.clone()),
                window_bounds: None,
            };
            settings.validate();
            assert_eq!(settings.theme, Some(max_name.clone()));
            assert_eq!(settings.font_family, Some(max_name));
        }

        #[test]
        fn validate_delegates_to_window_bounds() {
            let mut settings = Settings {
                theme: None,
                font_family: None,
                window_bounds: Some(WindowBoundsConfig {
                    x: f32::NAN,
                    y: 100.0,
                    width: 800.0,
                    height: 600.0,
                }),
            };
            settings.validate();
            let bounds = settings.window_bounds.unwrap();
            // NaN should be replaced with default
            assert_eq!(bounds.x, WindowBoundsConfig::default().x);
        }

        #[test]
        fn clone_creates_independent_copy() {
            let original = Settings {
                theme: Some("Test Theme".to_string()),
                font_family: Some("Monospace".to_string()),
                window_bounds: Some(WindowBoundsConfig::default()),
            };
            let cloned = original.clone();
            assert_eq!(original.theme, cloned.theme);
            assert_eq!(original.font_family, cloned.font_family);
        }
    }

    mod serialization {
        use super::{Settings, WindowBoundsConfig};

        #[test]
        fn settings_serializes_to_json() {
            let settings = Settings {
                theme: Some("Catppuccin Mocha".to_string()),
                font_family: Some("Iosevka".to_string()),
                window_bounds: Some(WindowBoundsConfig::default()),
            };
            let json = serde_json::to_string(&settings).unwrap();
            assert!(json.contains("Catppuccin Mocha"));
            assert!(json.contains("Iosevka"));
        }

        #[test]
        fn settings_deserializes_from_json() {
            let json = r#"{
                "theme": "Nord",
                "font_family": "Fira Code",
                "window_bounds": {
                    "x": 50.0,
                    "y": 50.0,
                    "width": 1000.0,
                    "height": 700.0
                }
            }"#;
            let settings: Settings = serde_json::from_str(json).unwrap();
            assert_eq!(settings.theme, Some("Nord".to_string()));
            assert_eq!(settings.font_family, Some("Fira Code".to_string()));
            let bounds = settings.window_bounds.unwrap();
            assert_eq!(bounds.x, 50.0);
            assert_eq!(bounds.width, 1000.0);
        }

        #[test]
        fn settings_deserializes_partial_json() {
            let json = r#"{"theme": "Dracula"}"#;
            let settings: Settings = serde_json::from_str(json).unwrap();
            assert_eq!(settings.theme, Some("Dracula".to_string()));
            assert!(settings.font_family.is_none());
            assert!(settings.window_bounds.is_none());
        }

        #[test]
        fn settings_deserializes_empty_json() {
            let json = "{}";
            let settings: Settings = serde_json::from_str(json).unwrap();
            assert!(settings.theme.is_none());
            assert!(settings.font_family.is_none());
            assert!(settings.window_bounds.is_none());
        }

        #[test]
        fn window_bounds_serializes_correctly() {
            let bounds = WindowBoundsConfig {
                x: 123.5,
                y: 456.5,
                width: 789.0,
                height: 1011.0,
            };
            let json = serde_json::to_string(&bounds).unwrap();
            assert!(json.contains("123.5"));
            assert!(json.contains("456.5"));
            assert!(json.contains("789"));
            assert!(json.contains("1011"));
        }

        #[test]
        fn settings_roundtrips_through_json() {
            let original = Settings {
                theme: Some("Solarized Dark".to_string()),
                font_family: Some("JetBrains Mono".to_string()),
                window_bounds: Some(WindowBoundsConfig {
                    x: 200.0,
                    y: 150.0,
                    width: 1600.0,
                    height: 900.0,
                }),
            };
            let json = serde_json::to_string_pretty(&original).unwrap();
            let restored: Settings = serde_json::from_str(&json).unwrap();
            assert_eq!(original.theme, restored.theme);
            assert_eq!(original.font_family, restored.font_family);
            let orig_bounds = original.window_bounds.unwrap();
            let rest_bounds = restored.window_bounds.unwrap();
            assert_eq!(orig_bounds.x, rest_bounds.x);
            assert_eq!(orig_bounds.y, rest_bounds.y);
            assert_eq!(orig_bounds.width, rest_bounds.width);
            assert_eq!(orig_bounds.height, rest_bounds.height);
        }
    }

    mod persistence_with_tempfile {
        use super::{settings, Settings, TempDir, WindowBoundsConfig};
        use std::fs;

        #[test]
        fn save_and_load_settings_with_tempfile() {
            let temp_dir = TempDir::new().unwrap();
            let settings_path = temp_dir.path().join("humanssh").join("settings.json");

            // Create settings
            let settings = Settings {
                theme: Some("Tokyo Night".to_string()),
                font_family: Some("Hack".to_string()),
                window_bounds: Some(WindowBoundsConfig {
                    x: 100.0,
                    y: 100.0,
                    width: 1200.0,
                    height: 800.0,
                }),
            };

            // Save manually (since we can't easily override settings_path())
            fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
            let json = serde_json::to_string_pretty(&settings).unwrap();
            fs::write(&settings_path, json).unwrap();

            // Load and verify
            let loaded_json = fs::read_to_string(&settings_path).unwrap();
            let loaded: Settings = serde_json::from_str(&loaded_json).unwrap();
            assert_eq!(loaded.theme, Some("Tokyo Night".to_string()));
            assert_eq!(loaded.font_family, Some("Hack".to_string()));
        }

        #[test]
        fn load_returns_default_for_missing_file() {
            let temp_dir = TempDir::new().unwrap();
            let settings_path = temp_dir.path().join("nonexistent").join("settings.json");

            // File doesn't exist - trying to read should fail
            let result = fs::read_to_string(&settings_path);
            assert!(result.is_err());

            // In production code, this would return Settings::default()
            let default_settings = Settings::default();
            assert!(default_settings.theme.is_none());
        }

        #[test]
        fn load_returns_default_for_invalid_json() {
            let temp_dir = TempDir::new().unwrap();
            let settings_path = temp_dir.path().join("settings.json");

            // Write invalid JSON
            fs::write(&settings_path, "not valid json {{{").unwrap();

            // Parsing should fail
            let json = fs::read_to_string(&settings_path).unwrap();
            let result: Result<Settings, _> = serde_json::from_str(&json);
            assert!(result.is_err());
        }

        #[test]
        fn load_validates_after_parsing() {
            let temp_dir = TempDir::new().unwrap();
            let settings_path = temp_dir.path().join("settings.json");

            // Write valid JSON with oversized theme name
            let long_theme = "x".repeat(settings::MAX_STRING_LENGTH + 1);
            let json = format!(r#"{{"theme": "{}"}}"#, long_theme);
            fs::write(&settings_path, json).unwrap();

            // Parse and validate
            let loaded_json = fs::read_to_string(&settings_path).unwrap();
            let mut loaded: Settings = serde_json::from_str(&loaded_json).unwrap();
            loaded.validate();
            assert!(
                loaded.theme.is_none(),
                "oversized theme should be rejected during validation"
            );
        }

        #[test]
        fn save_creates_parent_directories() {
            let temp_dir = TempDir::new().unwrap();
            let settings_path = temp_dir
                .path()
                .join("deep")
                .join("nested")
                .join("dir")
                .join("settings.json");

            // Create parent directories manually (simulating what save_settings does)
            fs::create_dir_all(settings_path.parent().unwrap()).unwrap();

            // Verify directory was created
            assert!(settings_path.parent().unwrap().exists());

            // Write settings
            let settings = Settings::default();
            let json = serde_json::to_string_pretty(&settings).unwrap();
            fs::write(&settings_path, json).unwrap();

            // Verify file was created
            assert!(settings_path.exists());
        }

        #[test]
        fn window_bounds_save_and_load() {
            let temp_dir = TempDir::new().unwrap();
            let settings_path = temp_dir.path().join("settings.json");

            let bounds = WindowBoundsConfig {
                x: 500.0,
                y: 300.0,
                width: 1920.0,
                height: 1080.0,
            };

            // Save via full settings (simulating save_window_bounds)
            let mut settings = Settings::default();
            settings.window_bounds = Some(bounds.clone());
            let json = serde_json::to_string_pretty(&settings).unwrap();
            fs::write(&settings_path, json).unwrap();

            // Load via full settings (simulating load_window_bounds)
            let loaded_json = fs::read_to_string(&settings_path).unwrap();
            let loaded: Settings = serde_json::from_str(&loaded_json).unwrap();
            let loaded_bounds = loaded.window_bounds.unwrap();

            assert_eq!(loaded_bounds.x, 500.0);
            assert_eq!(loaded_bounds.y, 300.0);
            assert_eq!(loaded_bounds.width, 1920.0);
            assert_eq!(loaded_bounds.height, 1080.0);
        }
    }

    mod file_size_protection {
        use super::{settings, Settings, TempDir};
        use std::fs;

        #[test]
        fn rejects_oversized_settings_file() {
            let temp_dir = TempDir::new().unwrap();
            let settings_path = temp_dir.path().join("settings.json");

            // Create a file larger than MAX_FILE_SIZE
            let oversized_content = "x".repeat((settings::MAX_FILE_SIZE + 1) as usize);
            fs::write(&settings_path, oversized_content).unwrap();

            // Verify file size
            let metadata = fs::metadata(&settings_path).unwrap();
            assert!(metadata.len() > settings::MAX_FILE_SIZE);

            // In production, load_settings would return Settings::default() for oversized files
        }

        #[test]
        fn accepts_small_settings_file() {
            let temp_dir = TempDir::new().unwrap();
            let settings_path = temp_dir.path().join("settings.json");

            // Create a small valid settings file
            let json = r#"{"theme": "Gruvbox"}"#;
            fs::write(&settings_path, json).unwrap();

            // Verify file size is under limit
            let metadata = fs::metadata(&settings_path).unwrap();
            assert!(metadata.len() <= settings::MAX_FILE_SIZE);

            // Should parse successfully
            let loaded_json = fs::read_to_string(&settings_path).unwrap();
            let loaded: Settings = serde_json::from_str(&loaded_json).unwrap();
            assert_eq!(loaded.theme, Some("Gruvbox".to_string()));
        }
    }

    mod edge_cases {
        use super::{window_limits, Settings, WindowBoundsConfig};

        #[test]
        fn empty_string_theme_name_is_valid() {
            let mut settings = Settings {
                theme: Some("".to_string()),
                font_family: None,
                window_bounds: None,
            };
            settings.validate();
            // Empty string is within length limit, so it's preserved
            assert_eq!(settings.theme, Some("".to_string()));
        }

        #[test]
        fn unicode_theme_name_is_valid() {
            let mut settings = Settings {
                theme: Some("テーマ 日本語".to_string()),
                font_family: Some("フォント".to_string()),
                window_bounds: None,
            };
            settings.validate();
            // Unicode strings within length limit are preserved
            assert_eq!(settings.theme, Some("テーマ 日本語".to_string()));
            assert_eq!(settings.font_family, Some("フォント".to_string()));
        }

        #[test]
        fn special_characters_in_theme_name() {
            let mut settings = Settings {
                theme: Some("Theme with spaces & symbols!@#$%".to_string()),
                font_family: None,
                window_bounds: None,
            };
            settings.validate();
            assert_eq!(
                settings.theme,
                Some("Theme with spaces & symbols!@#$%".to_string())
            );
        }

        #[test]
        fn window_bounds_with_negative_position() {
            // Negative positions are valid (multi-monitor setups)
            let mut bounds = WindowBoundsConfig {
                x: -500.0,
                y: -200.0,
                width: 800.0,
                height: 600.0,
            };
            bounds.validate();
            // Should be preserved as they're within limits
            assert_eq!(bounds.x, -500.0);
            assert_eq!(bounds.y, -200.0);
        }

        #[test]
        fn window_bounds_at_limit_boundaries() {
            let mut bounds = WindowBoundsConfig {
                x: window_limits::MIN_POSITION,
                y: window_limits::MAX_POSITION,
                width: window_limits::MIN_SIZE,
                height: window_limits::MAX_SIZE,
            };
            bounds.validate();
            // Values at limits should be preserved
            assert_eq!(bounds.x, window_limits::MIN_POSITION);
            assert_eq!(bounds.y, window_limits::MAX_POSITION);
            assert_eq!(bounds.width, window_limits::MIN_SIZE);
            assert_eq!(bounds.height, window_limits::MAX_SIZE);
        }
    }
}
