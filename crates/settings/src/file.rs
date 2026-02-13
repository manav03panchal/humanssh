//! TOML config file support with live reload.
//!
//! Config location: `~/.config/humanssh/config.toml`

use serde::Deserialize;
use std::path::PathBuf;

/// Rule for automatically switching to a profile based on context.
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(rename_all = "kebab-case")]
pub struct AutoSwitchRule {
    #[serde(default)]
    pub hostname_pattern: Option<String>,
    #[serde(default)]
    pub directory_pattern: Option<String>,
}

/// Named profile that can override config defaults.
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(rename_all = "kebab-case")]
pub struct Profile {
    pub name: String,
    #[serde(default)]
    pub font_family: Option<String>,
    #[serde(default)]
    pub font_size: Option<f32>,
    #[serde(default)]
    pub font_fallbacks: Option<Vec<String>>,
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub working_directory: Option<String>,
    #[serde(default)]
    pub auto_switch: Option<AutoSwitchRule>,
}

/// Merged view of config defaults with profile overrides applied.
#[derive(Debug, Clone, PartialEq)]
pub struct MergedProfileConfig {
    pub font_family: String,
    pub font_size: f32,
    pub font_fallbacks: Vec<String>,
    pub shell: Option<String>,
    pub working_directory: Option<String>,
}

/// Custom keybinding: maps a key chord to an action name.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct KeybindingEntry {
    /// Key chord (e.g., "cmd-shift-t", "ctrl-l")
    pub keys: String,
    /// Action name (e.g., "new-tab", "close-tab", "quit")
    pub action: String,
    /// Optional context scope (e.g., "terminal")
    pub context: Option<String>,
}

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
    /// Maximum number of scrollback lines.
    pub scrollback_lines: usize,
    /// Window width (auto-managed unless user overrides).
    pub window_width: Option<f32>,
    /// Window height (auto-managed unless user overrides).
    pub window_height: Option<f32>,
    /// Linux: window decoration style ("server" or "client").
    pub linux_decorations: Option<String>,
    /// Windows: shell preference ("powershell", "pwsh", or "cmd").
    pub windows_shell: Option<String>,
    /// Reverse scroll direction ("natural" scrolling).
    pub scroll_reverse: bool,
    /// Fallback font families for glyphs not in the primary font.
    #[serde(default)]
    pub font_fallbacks: Vec<String>,
    /// Custom keybindings (override defaults).
    #[serde(default)]
    pub keybindings: Vec<KeybindingEntry>,
    /// Named profiles that override config defaults.
    #[serde(default)]
    pub profiles: Vec<Profile>,
    /// Name of the default profile to use when no auto-switch rule matches.
    #[serde(default)]
    pub default_profile: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: "Catppuccin Mocha".to_string(),
            font_family: crate::constants::terminal::FONT_FAMILY.to_string(),
            font_size: crate::constants::terminal::DEFAULT_FONT_SIZE,
            option_as_alt: true,
            secure_keyboard_entry: false,
            scrollback_lines: crate::constants::scrollback::DEFAULT_LINES,
            window_width: None,
            window_height: None,
            linux_decorations: None,
            windows_shell: None,
            scroll_reverse: false,
            font_fallbacks: Vec::new(),
            keybindings: Vec::new(),
            profiles: Vec::new(),
            default_profile: None,
        }
    }
}

impl Config {
    /// Find a profile matching the given context (hostname and/or directory).
    ///
    /// First checks auto_switch rules on all profiles. If no rule matches,
    /// falls back to the default_profile if one is set.
    pub fn resolve_profile(
        &self,
        hostname: Option<&str>,
        directory: Option<&str>,
    ) -> Option<&Profile> {
        for profile in &self.profiles {
            if let Some(rule) = &profile.auto_switch {
                if matches_auto_switch(rule, hostname, directory) {
                    return Some(profile);
                }
            }
        }

        if let Some(default_name) = &self.default_profile {
            return self.profiles.iter().find(|p| p.name == *default_name);
        }

        None
    }

    /// Produce a merged config view where profile values override config defaults.
    pub fn merged_config_for_profile(&self, profile: &Profile) -> MergedProfileConfig {
        MergedProfileConfig {
            font_family: profile
                .font_family
                .clone()
                .unwrap_or_else(|| self.font_family.clone()),
            font_size: profile.font_size.unwrap_or(self.font_size),
            font_fallbacks: profile
                .font_fallbacks
                .clone()
                .unwrap_or_else(|| self.font_fallbacks.clone()),
            shell: profile.shell.clone(),
            working_directory: profile.working_directory.clone(),
        }
    }
}

/// Check whether an auto-switch rule matches the given hostname and directory.
fn matches_auto_switch(
    rule: &AutoSwitchRule,
    hostname: Option<&str>,
    directory: Option<&str>,
) -> bool {
    let hostname_matches = match (&rule.hostname_pattern, hostname) {
        (Some(pattern), Some(host)) => glob_matches(pattern, host),
        (Some(_), None) => false,
        (None, _) => true,
    };

    let directory_matches = match (&rule.directory_pattern, directory) {
        (Some(pattern), Some(dir)) => glob_matches(pattern, dir),
        (Some(_), None) => false,
        (None, _) => true,
    };

    let has_any_pattern = rule.hostname_pattern.is_some() || rule.directory_pattern.is_some();

    has_any_pattern && hostname_matches && directory_matches
}

/// Simple glob matching supporting `*` (any sequence of characters) and `?`
/// (single character). This intentionally treats `*` as matching path separators
/// too, so patterns like `*.prod.*` and `*/projects/*` work intuitively.
fn glob_matches(pattern: &str, text: &str) -> bool {
    let pattern_bytes = pattern.as_bytes();
    let text_bytes = text.as_bytes();
    let mut pattern_index = 0;
    let mut text_index = 0;
    let mut star_pattern_index: Option<usize> = None;
    let mut star_text_index = 0;

    while text_index < text_bytes.len() {
        if pattern_index < pattern_bytes.len() && pattern_bytes[pattern_index] == b'*' {
            star_pattern_index = Some(pattern_index);
            star_text_index = text_index;
            pattern_index += 1;
        } else if pattern_index < pattern_bytes.len()
            && (pattern_bytes[pattern_index] == b'?'
                || pattern_bytes[pattern_index] == text_bytes[text_index])
        {
            pattern_index += 1;
            text_index += 1;
        } else if let Some(star_pos) = star_pattern_index {
            pattern_index = star_pos + 1;
            star_text_index += 1;
            text_index = star_text_index;
        } else {
            return false;
        }
    }

    while pattern_index < pattern_bytes.len() && pattern_bytes[pattern_index] == b'*' {
        pattern_index += 1;
    }

    pattern_index == pattern_bytes.len()
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

# Maximum scrollback buffer size (lines)
scrollback-lines = 10000

# Window dimensions (auto-managed; uncomment to override)
# window-width = 1200
# window-height = 800

# Linux: window decoration style — "server" (native) or "client" (app-drawn)
# linux-decorations = "server"

# Windows: shell — "powershell", "pwsh", or "cmd"
# windows-shell = "powershell"

# Reverse scroll direction ("natural" scrolling like macOS trackpad)
# scroll-reverse = false

# Fallback fonts for characters not in the primary font (e.g. CJK, emoji, icons)
# font-fallbacks = ["Symbols Nerd Font", "Apple Color Emoji"]

# Custom keybindings (override defaults)
# [[keybindings]]
# keys = "cmd-shift-t"
# action = "new-tab"
#
# [[keybindings]]
# keys = "ctrl-l"
# action = "clear"
# context = "terminal"

# Profiles: named sets of overrides that can be selected or auto-switched.
# default-profile = "default"
#
# [[profiles]]
# name = "default"
# font-family = "JetBrains Mono"
# font-size = 14.0
#
# [[profiles]]
# name = "server"
# font-family = "Fira Code"
# font-size = 12.0
# shell = "/bin/bash"
#
# [profiles.auto-switch]
# hostname-pattern = "*.prod.*"
#
# [[profiles]]
# name = "projects"
# working-directory = "~/projects"
#
# [profiles.auto-switch]
# directory-pattern = "*/projects/*"
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
    fn parses_scrollback_lines() {
        let toml_str = r#"scrollback-lines = 50000"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.scrollback_lines, 50_000);
    }

    #[test]
    fn scrollback_lines_defaults_to_10000() {
        let cfg: Config = toml::from_str("").unwrap();
        assert_eq!(cfg.scrollback_lines, 10_000);
    }

    #[test]
    fn parses_keybindings_array() {
        let toml_str = r#"
[[keybindings]]
keys = "cmd-shift-t"
action = "new-tab"

[[keybindings]]
keys = "ctrl-l"
action = "clear"
context = "terminal"
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.keybindings.len(), 2);
        assert_eq!(cfg.keybindings[0].keys, "cmd-shift-t");
        assert_eq!(cfg.keybindings[0].action, "new-tab");
        assert!(cfg.keybindings[0].context.is_none());
        assert_eq!(cfg.keybindings[1].keys, "ctrl-l");
        assert_eq!(cfg.keybindings[1].action, "clear");
        assert_eq!(cfg.keybindings[1].context.as_deref(), Some("terminal"));
    }

    #[test]
    fn empty_keybindings_default_to_empty_vec() {
        let toml_str = r#"theme = "Nord""#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert!(cfg.keybindings.is_empty());
    }

    #[test]
    fn scrollback_max_lines_constant_is_sane() {
        assert!(
            crate::constants::scrollback::MAX_LINES >= crate::constants::scrollback::DEFAULT_LINES
        );
        assert!(crate::constants::scrollback::MAX_LINES <= 100_000);
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

    #[test]
    fn parses_font_fallbacks() {
        let toml_str = r#"font-fallbacks = ["Nerd Font", "Apple Color Emoji"]"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.font_fallbacks, vec!["Nerd Font", "Apple Color Emoji"]);
    }

    #[test]
    fn empty_font_fallbacks_default() {
        let cfg: Config = toml::from_str("").unwrap();
        assert!(cfg.font_fallbacks.is_empty());
    }

    #[test]
    fn parses_scroll_reverse() {
        let toml_str = r#"scroll-reverse = true"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert!(cfg.scroll_reverse);
    }

    #[test]
    fn scroll_reverse_defaults_to_false() {
        let cfg: Config = toml::from_str("").unwrap();
        assert!(!cfg.scroll_reverse);
    }

    #[test]
    fn parses_profiles_from_toml() {
        let toml_str = r#"
default-profile = "default"

[[profiles]]
name = "default"
font-family = "JetBrains Mono"
font-size = 14.0

[[profiles]]
name = "server"
font-family = "Fira Code"
font-size = 12.0
shell = "/bin/bash"

[profiles.auto-switch]
hostname-pattern = "*.prod.*"
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.default_profile.as_deref(), Some("default"));
        assert_eq!(cfg.profiles.len(), 2);

        assert_eq!(cfg.profiles[0].name, "default");
        assert_eq!(
            cfg.profiles[0].font_family.as_deref(),
            Some("JetBrains Mono")
        );
        assert_eq!(cfg.profiles[0].font_size, Some(14.0));
        assert!(cfg.profiles[0].auto_switch.is_none());

        assert_eq!(cfg.profiles[1].name, "server");
        assert_eq!(cfg.profiles[1].font_family.as_deref(), Some("Fira Code"));
        assert_eq!(cfg.profiles[1].font_size, Some(12.0));
        assert_eq!(cfg.profiles[1].shell.as_deref(), Some("/bin/bash"));

        let rule = cfg.profiles[1].auto_switch.as_ref().unwrap();
        assert_eq!(rule.hostname_pattern.as_deref(), Some("*.prod.*"));
        assert!(rule.directory_pattern.is_none());
    }

    #[test]
    fn parses_profile_with_directory_auto_switch() {
        let toml_str = r#"
[[profiles]]
name = "projects"
working-directory = "~/projects"

[profiles.auto-switch]
directory-pattern = "*/projects/*"
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.profiles.len(), 1);
        assert_eq!(
            cfg.profiles[0].working_directory.as_deref(),
            Some("~/projects")
        );

        let rule = cfg.profiles[0].auto_switch.as_ref().unwrap();
        assert!(rule.hostname_pattern.is_none());
        assert_eq!(rule.directory_pattern.as_deref(), Some("*/projects/*"));
    }

    #[test]
    fn parses_profile_with_font_fallbacks() {
        let toml_str = r#"
[[profiles]]
name = "rich"
font-fallbacks = ["Nerd Font", "Apple Color Emoji"]
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(
            cfg.profiles[0].font_fallbacks.as_deref(),
            Some(vec!["Nerd Font".to_string(), "Apple Color Emoji".to_string()].as_slice())
        );
    }

    #[test]
    fn empty_profiles_default_to_empty_vec() {
        let cfg: Config = toml::from_str("").unwrap();
        assert!(cfg.profiles.is_empty());
        assert!(cfg.default_profile.is_none());
    }

    #[test]
    fn resolve_profile_by_hostname() {
        let cfg: Config = toml::from_str(
            r#"
[[profiles]]
name = "prod"
font-size = 12.0

[profiles.auto-switch]
hostname-pattern = "*.prod.*"
"#,
        )
        .unwrap();

        let matched = cfg.resolve_profile(Some("web01.prod.example.com"), None);
        assert!(matched.is_some());
        assert_eq!(matched.unwrap().name, "prod");

        let no_match = cfg.resolve_profile(Some("localhost"), None);
        assert!(no_match.is_none());
    }

    #[test]
    fn resolve_profile_by_directory() {
        let cfg: Config = toml::from_str(
            r#"
[[profiles]]
name = "projects"

[profiles.auto-switch]
directory-pattern = "*/projects/*"
"#,
        )
        .unwrap();

        let matched = cfg.resolve_profile(None, Some("/home/user/projects/myapp"));
        assert!(matched.is_some());
        assert_eq!(matched.unwrap().name, "projects");

        let no_match = cfg.resolve_profile(None, Some("/home/user/documents"));
        assert!(no_match.is_none());
    }

    #[test]
    fn resolve_profile_falls_back_to_default() {
        let cfg: Config = toml::from_str(
            r#"
default-profile = "fallback"

[[profiles]]
name = "fallback"
font-size = 16.0

[[profiles]]
name = "special"

[profiles.auto-switch]
hostname-pattern = "special-host"
"#,
        )
        .unwrap();

        let matched = cfg.resolve_profile(Some("unmatched-host"), None);
        assert!(matched.is_some());
        assert_eq!(matched.unwrap().name, "fallback");
    }

    #[test]
    fn resolve_profile_auto_switch_takes_priority_over_default() {
        let cfg: Config = toml::from_str(
            r#"
default-profile = "fallback"

[[profiles]]
name = "fallback"
font-size = 16.0

[[profiles]]
name = "special"
font-size = 10.0

[profiles.auto-switch]
hostname-pattern = "special-host"
"#,
        )
        .unwrap();

        let matched = cfg.resolve_profile(Some("special-host"), None);
        assert!(matched.is_some());
        assert_eq!(matched.unwrap().name, "special");
    }

    #[test]
    fn resolve_profile_returns_none_when_no_profiles() {
        let cfg = Config::default();
        assert!(cfg
            .resolve_profile(Some("anything"), Some("/any/path"))
            .is_none());
    }

    #[test]
    fn resolve_profile_returns_none_for_missing_default() {
        let cfg: Config = toml::from_str(
            r#"
default-profile = "nonexistent"

[[profiles]]
name = "other"
"#,
        )
        .unwrap();

        assert!(cfg.resolve_profile(None, None).is_none());
    }

    #[test]
    fn resolve_profile_hostname_and_directory_both_must_match() {
        let cfg: Config = toml::from_str(
            r#"
[[profiles]]
name = "strict"

[profiles.auto-switch]
hostname-pattern = "*.prod.*"
directory-pattern = "*/deploy/*"
"#,
        )
        .unwrap();

        let matched = cfg.resolve_profile(
            Some("web01.prod.example.com"),
            Some("/home/user/deploy/app"),
        );
        assert!(matched.is_some());

        let no_match = cfg.resolve_profile(Some("web01.prod.example.com"), Some("/home/user/docs"));
        assert!(no_match.is_none());

        let no_match = cfg.resolve_profile(Some("localhost"), Some("/home/user/deploy/app"));
        assert!(no_match.is_none());
    }

    #[test]
    fn merged_config_overrides_font_family() {
        let cfg = Config {
            font_family: "Menlo".to_string(),
            font_size: 14.0,
            ..Config::default()
        };
        let profile = Profile {
            name: "custom".to_string(),
            font_family: Some("Fira Code".to_string()),
            ..Profile::default()
        };

        let merged = cfg.merged_config_for_profile(&profile);
        assert_eq!(merged.font_family, "Fira Code");
        assert_eq!(merged.font_size, 14.0);
    }

    #[test]
    fn merged_config_overrides_font_size() {
        let cfg = Config {
            font_size: 14.0,
            ..Config::default()
        };
        let profile = Profile {
            name: "big".to_string(),
            font_size: Some(20.0),
            ..Profile::default()
        };

        let merged = cfg.merged_config_for_profile(&profile);
        assert_eq!(merged.font_size, 20.0);
        assert_eq!(merged.font_family, cfg.font_family);
    }

    #[test]
    fn merged_config_overrides_font_fallbacks() {
        let cfg = Config {
            font_fallbacks: vec!["Default Emoji".to_string()],
            ..Config::default()
        };
        let profile = Profile {
            name: "custom".to_string(),
            font_fallbacks: Some(vec!["Nerd Font".to_string()]),
            ..Profile::default()
        };

        let merged = cfg.merged_config_for_profile(&profile);
        assert_eq!(merged.font_fallbacks, vec!["Nerd Font"]);
    }

    #[test]
    fn merged_config_inherits_defaults_when_profile_is_empty() {
        let cfg = Config {
            font_family: "Menlo".to_string(),
            font_size: 14.0,
            font_fallbacks: vec!["Emoji".to_string()],
            ..Config::default()
        };
        let profile = Profile {
            name: "empty".to_string(),
            ..Profile::default()
        };

        let merged = cfg.merged_config_for_profile(&profile);
        assert_eq!(merged.font_family, "Menlo");
        assert_eq!(merged.font_size, 14.0);
        assert_eq!(merged.font_fallbacks, vec!["Emoji"]);
        assert!(merged.shell.is_none());
        assert!(merged.working_directory.is_none());
    }

    #[test]
    fn merged_config_includes_shell_and_working_directory() {
        let cfg = Config::default();
        let profile = Profile {
            name: "dev".to_string(),
            shell: Some("/bin/zsh".to_string()),
            working_directory: Some("~/projects".to_string()),
            ..Profile::default()
        };

        let merged = cfg.merged_config_for_profile(&profile);
        assert_eq!(merged.shell.as_deref(), Some("/bin/zsh"));
        assert_eq!(merged.working_directory.as_deref(), Some("~/projects"));
    }

    #[test]
    fn glob_matches_star_prefix() {
        assert!(glob_matches("*.prod.*", "web01.prod.example.com"));
        assert!(!glob_matches("*.prod.*", "web01.staging.example.com"));
    }

    #[test]
    fn glob_matches_star_suffix() {
        assert!(glob_matches("/home/user/*", "/home/user/projects"));
        assert!(!glob_matches("/home/user/*", "/home/other/projects"));
    }

    #[test]
    fn glob_matches_star_in_middle() {
        assert!(glob_matches("*/projects/*", "/home/user/projects/myapp"));
        assert!(!glob_matches("*/projects/*", "/home/user/documents/myapp"));
    }

    #[test]
    fn glob_matches_exact() {
        assert!(glob_matches("localhost", "localhost"));
        assert!(!glob_matches("localhost", "remotehost"));
    }

    #[test]
    fn glob_matches_question_mark() {
        assert!(glob_matches("host?", "host1"));
        assert!(!glob_matches("host?", "host12"));
    }

    #[test]
    fn glob_matches_empty_pattern_and_text() {
        assert!(glob_matches("", ""));
        assert!(!glob_matches("", "something"));
        assert!(glob_matches("*", ""));
        assert!(glob_matches("*", "anything"));
    }

    #[test]
    fn existing_config_without_profiles_still_parses() {
        let toml_str = r#"
theme = "Nord"
font-family = "Fira Code"
font-size = 16
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.theme, "Nord");
        assert!(cfg.profiles.is_empty());
        assert!(cfg.default_profile.is_none());
    }
}
