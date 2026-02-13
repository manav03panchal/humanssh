//! TOML config file support with live reload.
//!
//! Config location: `~/.config/humanssh/config.toml`

use serde::Deserialize;
use std::path::PathBuf;

/// User-facing config parsed from TOML.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default, rename_all = "kebab-case")]
pub struct Config {
    /// Color theme name (must match a theme in the themes/ directory).
    pub theme: String,
    /// Terminal font family.
    pub font_family: String,
    /// Terminal font size in points.
    pub font_size: f32,
    /// macOS: treat Option key as Alt for terminal input.
    pub option_as_alt: bool,
    /// macOS: enable Secure Keyboard Entry.
    pub secure_keyboard_entry: bool,
    /// Window width (auto-managed unless user overrides).
    pub window_width: Option<f32>,
    /// Window height (auto-managed unless user overrides).
    pub window_height: Option<f32>,
    /// Linux: window decoration style ("server" or "client").
    pub linux_decorations: Option<String>,
    /// Windows: shell preference ("powershell", "pwsh", or "cmd").
    pub windows_shell: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: "Catppuccin Mocha".to_string(),
            font_family: crate::constants::terminal::FONT_FAMILY.to_string(),
            font_size: crate::constants::terminal::DEFAULT_FONT_SIZE,
            option_as_alt: true,
            secure_keyboard_entry: false,
            window_width: None,
            window_height: None,
            linux_decorations: None,
            windows_shell: None,
        }
    }
}

/// Default config file content with comments (generated on first launch).
const DEFAULT_CONFIG: &str = r#"# HumanSSH Configuration
# Changes are applied live — just save this file.

# Color theme (must match a theme name from the themes/ directory)
theme = "Catppuccin Mocha"

# Terminal font family (any monospace font installed on your system)
font-family = "FONT_PLACEHOLDER"

# Terminal font size in points
font-size = 14

# macOS: treat Option key as Alt for terminal input
# Set to false to type special characters with Option (e.g. Option+3 = #)
option-as-alt = true

# macOS: enable Secure Keyboard Entry (prevents other apps from intercepting keystrokes)
# secure-keyboard-entry = false

# Window dimensions (auto-managed; uncomment to override)
# window-width = 1200
# window-height = 800

# Linux: window decoration style — "server" (native) or "client" (app-drawn)
# linux-decorations = "server"

# Windows: shell — "powershell", "pwsh", or "cmd"
# windows-shell = "powershell"
"#;

/// Return the config file path.
pub fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("humanssh").join("config.toml"))
}

/// Migrate from legacy settings.json if it exists.
fn migrate_from_json(config_dir: &std::path::Path) -> Option<(String, String)> {
    let json_path = config_dir.join("settings.json");
    let content = std::fs::read_to_string(&json_path).ok()?;

    #[derive(Deserialize)]
    struct LegacySettings {
        theme: Option<String>,
        font_family: Option<String>,
    }

    let legacy: LegacySettings = serde_json::from_str(&content).ok()?;
    tracing::info!("Migrated settings from settings.json");

    Some((
        legacy
            .theme
            .unwrap_or_else(|| "Catppuccin Mocha".to_string()),
        legacy
            .font_family
            .unwrap_or_else(|| crate::constants::terminal::FONT_FAMILY.to_string()),
    ))
}

/// Ensure the config file exists, creating a default if missing.
/// Returns the path to the config file.
pub fn ensure_config_file() -> Option<PathBuf> {
    let path = config_path()?;
    if !path.exists() {
        let parent = path.parent()?;
        std::fs::create_dir_all(parent).ok()?;

        // Try migrating from legacy settings.json
        let (theme, font) = migrate_from_json(parent).unwrap_or_else(|| {
            (
                "Catppuccin Mocha".to_string(),
                crate::constants::terminal::FONT_FAMILY.to_string(),
            )
        });

        let content = DEFAULT_CONFIG.replace("FONT_PLACEHOLDER", &font).replace(
            "theme = \"Catppuccin Mocha\"",
            &format!("theme = \"{}\"", theme),
        );
        std::fs::write(&path, content).ok()?;
        tracing::info!("Created default config at {:?}", path);
    }
    Some(path)
}

/// Load and parse the config file. Returns default on any error.
pub fn load_config() -> Config {
    let Some(path) = config_path() else {
        return Config::default();
    };

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            if e.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!("Failed to read config: {}", e);
            }
            return Config::default();
        }
    };

    // Size guard
    if content.len() > crate::constants::settings::MAX_FILE_SIZE as usize {
        tracing::warn!(
            "Config file too large ({} bytes), using defaults",
            content.len()
        );
        return Config::default();
    }

    match toml::from_str(&content) {
        Ok(cfg) => cfg,
        Err(e) => {
            tracing::warn!("Failed to parse config.toml: {}", e);
            Config::default()
        }
    }
}

/// Update the window dimensions in the config file (preserving comments/formatting).
pub fn save_window_bounds(width: f32, height: f32) {
    let Some(path) = config_path() else {
        return;
    };

    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let mut doc = match content.parse::<toml_edit::DocumentMut>() {
        Ok(d) => d,
        Err(_) => return,
    };

    doc["window-width"] = toml_edit::value(width as f64);
    doc["window-height"] = toml_edit::value(height as f64);

    if let Err(e) = std::fs::write(&path, doc.to_string()) {
        tracing::warn!("Failed to save window bounds: {}", e);
    }
}

use gpui::App;
use gpui_component::theme::{Theme, ThemeRegistry};

/// Apply a parsed Config to the running application state.
/// Takes a callback `on_apply` for cross-crate side effects (theme font, option-as-alt, secure input).
pub fn apply_config(config: &Config, cx: &mut App, on_apply: impl FnOnce(&Config, &mut App)) {
    // Apply theme
    if let Some(theme_config) = ThemeRegistry::global(cx)
        .themes()
        .get(&config.theme as &str)
        .cloned()
    {
        let current_font = Theme::global(cx).font_family.clone();
        Theme::global_mut(cx).apply_config(&theme_config);
        // Restore font (apply_config resets it)
        Theme::global_mut(cx).font_family = current_font;
        tracing::info!("Applied theme: {}", config.theme);
    }

    // Apply font to GPUI theme
    Theme::global_mut(cx).font_family = config.font_family.clone().into();

    // Delegate cross-crate side effects to caller
    on_apply(config, cx);

    cx.refresh_windows();
}

/// Start watching the config file for changes.
/// Returns a guard that stops watching on drop.
/// `on_apply` is called when the config changes to apply cross-crate side effects.
pub fn watch_config(
    cx: &mut App,
    on_apply: fn(&Config, &mut App),
) -> Option<notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>> {
    use notify_debouncer_mini::new_debouncer;
    use std::time::Duration;

    let path = config_path()?;
    let watch_dir = path.parent()?.to_path_buf();

    let current = std::sync::Arc::new(parking_lot::Mutex::new(load_config()));
    let current_clone = current.clone();
    let path_clone = path.clone();

    let (tx, rx) = std::sync::mpsc::channel();

    let mut debouncer = new_debouncer(
        Duration::from_millis(100),
        move |res: Result<Vec<notify_debouncer_mini::DebouncedEvent>, _>| {
            if let Ok(events) = res {
                for event in &events {
                    if event.path == path_clone {
                        let _ = tx.send(());
                        break;
                    }
                }
            }
        },
    )
    .ok()?;

    debouncer
        .watcher()
        .watch(&watch_dir, notify::RecursiveMode::NonRecursive)
        .ok()?;

    // Poll channel on a timer to apply changes on the main thread
    cx.spawn(async move |cx: &mut gpui::AsyncApp| {
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(50))
                .await;
            if rx.try_recv().is_ok() {
                // Drain extra events
                while rx.try_recv().is_ok() {}

                let new_config = load_config();
                let mut prev = current_clone.lock();
                if new_config != *prev {
                    tracing::info!("Config file changed, reloading...");
                    *prev = new_config.clone();
                    let _ = cx.update(|cx| {
                        apply_config(&new_config, cx, on_apply);
                    });
                }
            }
        }
    })
    .detach();

    tracing::info!("Watching config file: {:?}", path);
    Some(debouncer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_sane_values() {
        let cfg = Config::default();
        assert_eq!(cfg.theme, "Catppuccin Mocha");
        assert_eq!(cfg.font_size, 14.0);
        assert!(cfg.option_as_alt);
        assert!(!cfg.secure_keyboard_entry);
        assert!(cfg.window_width.is_none());
    }

    #[test]
    fn parses_minimal_toml() {
        let toml_str = r#"theme = "Nord""#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.theme, "Nord");
        assert_eq!(cfg.font_size, 14.0);
    }

    #[test]
    fn parses_full_toml() {
        let toml_str = r#"
theme = "Dracula"
font-family = "JetBrains Mono"
font-size = 16
option-as-alt = false
secure-keyboard-entry = true
window-width = 1920
window-height = 1080
linux-decorations = "client"
windows-shell = "pwsh"
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.theme, "Dracula");
        assert_eq!(cfg.font_family, "JetBrains Mono");
        assert_eq!(cfg.font_size, 16.0);
        assert!(!cfg.option_as_alt);
        assert!(cfg.secure_keyboard_entry);
        assert_eq!(cfg.window_width, Some(1920.0));
        assert_eq!(cfg.window_height, Some(1080.0));
        assert_eq!(cfg.linux_decorations.as_deref(), Some("client"));
        assert_eq!(cfg.windows_shell.as_deref(), Some("pwsh"));
    }

    #[test]
    fn ignores_unknown_keys() {
        let toml_str = r#"
theme = "Nord"
unknown-key = "whatever"
"#;
        let result: Result<Config, _> = toml::from_str(toml_str);
        assert!(result.is_ok());
    }

    #[test]
    fn default_config_template_is_valid_toml() {
        let content = DEFAULT_CONFIG.replace("FONT_PLACEHOLDER", "Menlo");
        let cfg: Config = toml::from_str(&content).unwrap();
        assert_eq!(cfg.theme, "Catppuccin Mocha");
        assert_eq!(cfg.font_family, "Menlo");
    }

    #[test]
    fn empty_string_parses_to_defaults() {
        let cfg: Config = toml::from_str("").unwrap();
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn migration_parses_legacy_json() {
        let json = r#"{"theme": "Nord", "font_family": "Fira Code"}"#;

        #[derive(serde::Deserialize)]
        struct LegacySettings {
            theme: Option<String>,
            font_family: Option<String>,
        }

        let legacy: LegacySettings = serde_json::from_str(json).unwrap();
        assert_eq!(legacy.theme.as_deref(), Some("Nord"));
        assert_eq!(legacy.font_family.as_deref(), Some("Fira Code"));
    }
}
