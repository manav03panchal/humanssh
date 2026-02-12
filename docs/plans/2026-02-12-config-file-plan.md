# Config File with Live Reload — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace the settings dialog with a TOML config file at `~/.config/humanssh/config.toml` that live-reloads on save (Ghostty-style).

**Architecture:** New `src/config/file.rs` owns the `Config` struct (TOML deserialization), file watcher (notify crate), and apply logic. `src/app/settings.rs` is gutted to a single `open_config_in_editor` function. The existing `src/theme/persistence.rs` is simplified — window bounds stay as JSON auto-save, everything else moves to the TOML config.

**Tech Stack:** `toml` (parsing), `toml_edit` (comment-preserving writes for window bounds), `notify` (OS-native file watching)

---

### Task 1: Add Dependencies

**Files:**
- Modify: `Cargo.toml`

**Step 1: Add toml, toml_edit, and notify to Cargo.toml**

In the `[dependencies]` section, after the `serde_json` line, add:

```toml
toml = "0.8"
toml_edit = "0.22"
notify = { version = "7", default-features = false, features = ["macos_fsevent"] }
notify-debouncer-mini = "0.5"
```

**Step 2: Run cargo check to verify deps resolve**

Run: `cargo check 2>&1 | head -20`
Expected: dependencies download and resolve without errors

**Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "feat: add toml, toml_edit, notify deps for config file system"
```

---

### Task 2: Create Config struct and TOML parsing

**Files:**
- Create: `src/config/file.rs`
- Modify: `src/config.rs` → rename to `src/config/mod.rs` (move existing content, add `pub mod file;`)

**Step 1: Convert `src/config.rs` to `src/config/mod.rs`**

Create `src/config/` directory. Move `src/config.rs` to `src/config/mod.rs`. Add at the top (after the module doc comment):

```rust
pub mod file;
```

**Step 2: Run cargo check to verify module restructure compiles**

Run: `cargo check 2>&1 | head -20`
Expected: compiles clean (all existing `crate::config::` paths still work)

**Step 3: Create `src/config/file.rs` with Config struct**

```rust
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
            font_family: crate::config::terminal::FONT_FAMILY.to_string(),
            font_size: crate::config::terminal::DEFAULT_FONT_SIZE,
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
pub const DEFAULT_CONFIG: &str = r#"# HumanSSH Configuration
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

/// Ensure the config file exists, creating a default if missing.
/// Returns the path to the config file.
pub fn ensure_config_file() -> Option<PathBuf> {
    let path = config_path()?;
    if !path.exists() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok()?;
        }
        let content = DEFAULT_CONFIG.replace(
            "FONT_PLACEHOLDER",
            crate::config::terminal::FONT_FAMILY,
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

    // Size guard (same as existing settings.json protection)
    if content.len() > crate::config::settings::MAX_FILE_SIZE as usize {
        tracing::warn!("Config file too large ({} bytes), using defaults", content.len());
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

/// Update the window dimensions in the config file (preserving comments).
pub fn save_window_bounds(width: f32, height: f32) {
    let Some(path) = config_path() else { return };

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
```

**Step 4: Run cargo check**

Run: `cargo check 2>&1 | head -20`
Expected: compiles clean

**Step 5: Commit**

```bash
git add src/config/
git commit -m "feat: add Config struct and TOML parsing (config/file.rs)"
```

---

### Task 3: Write tests for Config parsing

**Files:**
- Modify: `src/config/file.rs` (append test module)

**Step 1: Add tests to the bottom of `src/config/file.rs`**

```rust
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
        let toml = r#"theme = "Nord""#;
        let cfg: Config = toml::from_str(toml).unwrap();
        assert_eq!(cfg.theme, "Nord");
        // All other fields get defaults
        assert_eq!(cfg.font_size, 14.0);
    }

    #[test]
    fn parses_full_toml() {
        let toml = r#"
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
        let cfg: Config = toml::from_str(toml).unwrap();
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
        let toml = r#"
theme = "Nord"
unknown-key = "whatever"
"#;
        // serde with #[serde(default)] should ignore unknown keys
        let result: Result<Config, _> = toml::from_str(toml);
        assert!(result.is_ok());
    }

    #[test]
    fn default_config_template_is_valid_toml() {
        let content = DEFAULT_CONFIG.replace(
            "FONT_PLACEHOLDER",
            "Menlo",
        );
        let cfg: Config = toml::from_str(&content).unwrap();
        assert_eq!(cfg.theme, "Catppuccin Mocha");
        assert_eq!(cfg.font_family, "Menlo");
    }

    #[test]
    fn empty_string_parses_to_defaults() {
        let cfg: Config = toml::from_str("").unwrap();
        assert_eq!(cfg, Config::default());
    }
}
```

**Step 2: Run tests**

Run: `cargo test --lib config::file::tests -- --nocapture 2>&1 | tail -20`
Expected: all tests pass

**Step 3: Commit**

```bash
git add src/config/file.rs
git commit -m "test: add Config TOML parsing tests"
```

---

### Task 4: Add file watcher and apply logic

**Files:**
- Modify: `src/config/file.rs` (add `watch_config` and `apply_config` functions)

**Step 1: Add apply_config function**

Add above the `#[cfg(test)]` block in `src/config/file.rs`:

```rust
use gpui::App;
use gpui_component::theme::{Theme, ThemeRegistry};
use std::sync::atomic::Ordering;

/// Apply a parsed Config to the running application state.
pub fn apply_config(config: &Config, cx: &mut App) {
    // Apply theme
    if let Some(theme_config) = ThemeRegistry::global(cx)
        .themes()
        .get(&config.theme as &str)
        .cloned()
    {
        Theme::global_mut(cx).apply_config(&theme_config);
        tracing::info!("Applied theme: {}", config.theme);
    }

    // Apply font
    crate::theme::set_intended_font(config.font_family.clone());
    Theme::global_mut(cx).font_family = config.font_family.clone().into();

    // Apply option-as-alt
    crate::terminal::OPTION_AS_ALT.store(config.option_as_alt, Ordering::Relaxed);

    // Apply secure keyboard entry (macOS)
    #[cfg(target_os = "macos")]
    {
        let currently_enabled = crate::platform::is_secure_input_enabled();
        if config.secure_keyboard_entry && !currently_enabled {
            crate::platform::enable_secure_input();
        } else if !config.secure_keyboard_entry && currently_enabled {
            crate::platform::disable_secure_input();
        }
    }

    cx.refresh_windows();
}
```

**Step 2: Add watch_config function**

```rust
/// Start watching the config file for changes. Returns a guard that stops watching on drop.
pub fn watch_config(cx: &mut App) -> Option<notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>> {
    use notify::Watcher;
    use notify_debouncer_mini::new_debouncer;
    use std::time::Duration;

    let path = config_path()?;
    let watch_dir = path.parent()?.to_path_buf();

    // Load initial config
    let current = std::sync::Arc::new(parking_lot::Mutex::new(load_config()));

    let current_clone = current.clone();
    let path_clone = path.clone();

    // Use cx.background_spawn + channel to get back onto the main thread
    let (tx, rx) = std::sync::mpsc::channel();

    let mut debouncer = new_debouncer(Duration::from_millis(100), move |res: Result<Vec<notify_debouncer_mini::DebouncedEvent>, _>| {
        if let Ok(events) = res {
            for event in &events {
                if event.path == path_clone {
                    let _ = tx.send(());
                    break;
                }
            }
        }
    }).ok()?;

    debouncer.watcher().watch(&watch_dir, notify::RecursiveMode::NonRecursive).ok()?;

    // Poll the channel on a timer to apply changes on the main thread
    cx.spawn(|mut cx| async move {
        loop {
            tokio::time::sleep(Duration::from_millis(50)).await;
            if rx.try_recv().is_ok() {
                // Drain any extra events
                while rx.try_recv().is_ok() {}

                let new_config = load_config();
                let mut prev = current_clone.lock();
                if new_config != *prev {
                    tracing::info!("Config file changed, applying...");
                    *prev = new_config.clone();
                    let _ = cx.update(|_, cx| {
                        apply_config(&new_config, cx);
                    });
                }
            }
        }
    })
    .detach();

    tracing::info!("Watching config file: {:?}", path);
    Some(debouncer)
}
```

**Step 3: Run cargo check**

Run: `cargo check 2>&1 | head -30`
Expected: compiles clean

**Step 4: Commit**

```bash
git add src/config/file.rs
git commit -m "feat: add config file watcher and apply logic"
```

---

### Task 5: Wire config into theme::init (replace settings.json flow)

**Files:**
- Modify: `src/theme/mod.rs` — replace `load_settings()` usage with `config::file::load_config()`, start watcher

**Step 1: Update `theme::init` to use TOML config**

In `src/theme/mod.rs`, the `init` function currently calls `load_settings()` to get theme/font. Replace that with `crate::config::file::load_config()`. Also start the file watcher.

Replace the body of `pub fn init(cx: &mut App)` with:

```rust
pub fn init(cx: &mut App) {
    // Ensure config file exists (create default on first launch)
    crate::config::file::ensure_config_file();

    // Load config
    let config = crate::config::file::load_config();

    // Apply font
    set_intended_font(config.font_family.clone());
    Theme::global_mut(cx).font_family = config.font_family.clone().into();

    // Apply option-as-alt
    crate::terminal::OPTION_AS_ALT.store(config.option_as_alt, std::sync::atomic::Ordering::Relaxed);

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
    // The debouncer is stored in a leaked Box to keep it alive for the app lifetime.
    // This is intentional — the watcher must live as long as the app.
    if let Some(debouncer) = crate::config::file::watch_config(cx) {
        Box::leak(Box::new(debouncer));
    }

    // Register theme switching actions (still needed for internal use)
    actions::register_actions(cx);
}
```

**Step 2: Remove save_settings calls from the theme observer**

The old observer called `load_settings()` + `save_settings()` on every theme change. That's gone now — the config file is the source of truth, and changes flow config → app (not app → config).

**Step 3: Run cargo check**

Run: `cargo check 2>&1 | head -30`
Expected: compiles (may have unused import warnings which we'll clean up)

**Step 4: Commit**

```bash
git add src/theme/mod.rs
git commit -m "feat: wire TOML config into theme::init, start file watcher"
```

---

### Task 6: Replace settings dialog with open-in-editor

**Files:**
- Rewrite: `src/app/settings.rs`
- Modify: `src/app/mod.rs` (change re-export)
- Modify: `src/app/workspace.rs` (update calls)
- Modify: `src/terminal/pane.rs` (update call)

**Step 1: Rewrite `src/app/settings.rs`**

Replace the entire file with:

```rust
//! Config file opener.
//!
//! Opens the config file in the user's preferred editor.
//! Replaces the old settings dialog.

/// Open the config file in the user's editor.
/// Creates a default config file if it doesn't exist.
pub fn open_config_file() {
    let Some(path) = crate::config::file::ensure_config_file() else {
        tracing::warn!("Could not determine config file path");
        return;
    };

    let path_str = path.to_string_lossy().to_string();

    // Try $EDITOR first, then platform default
    if let Ok(editor) = std::env::var("EDITOR") {
        match std::process::Command::new(&editor).arg(&path_str).spawn() {
            Ok(_) => {
                tracing::info!("Opened config in $EDITOR ({})", editor);
                return;
            }
            Err(e) => tracing::warn!("Failed to open $EDITOR ({}): {}", editor, e),
        }
    }

    // Platform-specific fallback
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg("-t").arg(&path_str).spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(&path_str).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", &path_str])
            .spawn();
    }
}
```

**Step 2: Update `src/app/mod.rs`**

Change:
```rust
pub use settings::toggle_settings_dialog;
```
to:
```rust
pub use settings::open_config_file;
```

**Step 3: Update `src/app/workspace.rs`**

Replace all 4 occurrences of `super::settings::toggle_settings_dialog(window, cx)` with `super::settings::open_config_file()`. Also:
- The `.on_action` for `OpenSettings` no longer needs `window` or `cx` params
- The key handler `"," =>` call no longer needs `window, cx`
- The settings button `.on_click` no longer needs `window, cx`

Specifically:

Line ~586: `.on_action(cx.listener(|_this, _: &OpenSettings, _window, _cx| { super::settings::open_config_file(); }))`

Line ~616: `"," => super::settings::open_config_file(),`

Line ~727: `super::settings::open_config_file();`

**Step 4: Update `src/terminal/pane.rs`**

Line ~1655: Replace:
```rust
crate::app::toggle_settings_dialog(window, cx);
```
with:
```rust
crate::app::open_config_file();
```

**Step 5: Run cargo check**

Run: `cargo check 2>&1 | head -30`
Expected: compiles clean (may have unused import warnings for gpui-component settings types)

**Step 6: Commit**

```bash
git add src/app/settings.rs src/app/mod.rs src/app/workspace.rs src/terminal/pane.rs
git commit -m "feat: replace settings dialog with open-in-editor (Cmd+,)"
```

---

### Task 7: Update window bounds persistence

**Files:**
- Modify: `src/app/workspace.rs` — update `save_window_bounds` calls to use new config/file path
- Modify: `src/main.rs` — update `build_window_options` to read from Config

**Step 1: Update window bounds loading in main.rs**

In `build_window_options()` (all platform variants), replace:
```rust
let saved = theme::load_window_bounds();
```
with:
```rust
let config = crate::config::file::load_config();
let saved_width = config.window_width.unwrap_or(1200.0);
let saved_height = config.window_height.unwrap_or(800.0);
```

And use `saved_width`/`saved_height` instead of `saved.width`/`saved.height`. For position, use defaults (100.0, 100.0) since we're not persisting position in the new config (window managers handle this).

**Step 2: Update window bounds saving in workspace.rs**

Find where `save_window_bounds` is called and replace with `crate::config::file::save_window_bounds(width, height)`.

**Step 3: Run cargo check**

Run: `cargo check 2>&1 | head -30`

**Step 4: Commit**

```bash
git add src/main.rs src/app/workspace.rs
git commit -m "feat: update window bounds to use TOML config"
```

---

### Task 8: Clean up old settings.json code

**Files:**
- Modify: `src/theme/persistence.rs` — keep only `WindowsShell` and `LinuxDecorations` enums (used by main.rs), remove `Settings` struct, `load_settings`, `save_settings`, `settings_path`
- Modify: `src/theme/mod.rs` — remove re-exports of deleted items
- Modify: `src/theme/actions.rs` — remove `SwitchFont` action handler (config file is now the source of truth for font), keep `SwitchTheme` for internal use
- Modify: `src/config.rs` (now `src/config/mod.rs`) — remove `dialog::SETTINGS_WIDTH`

**Step 1: Slim down `src/theme/persistence.rs`**

Keep: `WindowBoundsConfig` (still used by some code paths), `WindowsShell`, `LinuxDecorations`, their impls and tests.

Remove: `Settings` struct, `load_settings`, `save_settings`, `settings_path`, `load_window_bounds`, `save_window_bounds`, and their tests.

**Step 2: Update `src/theme/mod.rs` re-exports**

Remove re-exports of deleted items: `load_settings`, `save_settings`, `Settings`, `load_window_bounds`, `save_window_bounds`. Keep `LinuxDecorations`, `WindowsShell`, `WindowBoundsConfig`.

**Step 3: Remove `SETTINGS_WIDTH` from `src/config/mod.rs`**

Delete the `SETTINGS_WIDTH` constant from `dialog` module and its test.

**Step 4: Clean up unused imports across all modified files**

Run: `cargo check 2>&1 | grep "unused"` and fix all warnings.

**Step 5: Run full test suite**

Run: `cargo test --workspace 2>&1 | tail -30`
Expected: all tests pass (some old settings dialog tests will be gone)

**Step 6: Commit**

```bash
git add src/theme/ src/config/ src/app/
git commit -m "refactor: remove settings.json code, clean up old dialog remnants"
```

---

### Task 9: Migration from settings.json

**Files:**
- Modify: `src/config/file.rs` — add migration in `ensure_config_file`

**Step 1: Add migration logic to `ensure_config_file`**

Before writing the default config, check if `settings.json` exists and extract theme/font from it:

```rust
/// Migrate from legacy settings.json if it exists.
fn migrate_from_json(config_dir: &std::path::Path) -> Option<(String, String)> {
    let json_path = config_dir.join("settings.json");
    let content = std::fs::read_to_string(&json_path).ok()?;

    #[derive(serde::Deserialize)]
    struct LegacySettings {
        theme: Option<String>,
        font_family: Option<String>,
    }

    let legacy: LegacySettings = serde_json::from_str(&content).ok()?;
    tracing::info!("Migrating from settings.json");

    Some((
        legacy.theme.unwrap_or_else(|| "Catppuccin Mocha".to_string()),
        legacy.font_family.unwrap_or_else(|| crate::config::terminal::FONT_FAMILY.to_string()),
    ))
}
```

Update `ensure_config_file` to call this and use the values when generating the default TOML.

**Step 2: Write migration test**

```rust
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
```

**Step 3: Run tests**

Run: `cargo test --lib config::file 2>&1 | tail -15`
Expected: all pass

**Step 4: Commit**

```bash
git add src/config/file.rs
git commit -m "feat: add settings.json → config.toml migration"
```

---

### Task 10: Remove unused dependencies and final cleanup

**Files:**
- Modify: `Cargo.toml` — check if `gpui-component`'s `setting` module imports can be reduced (likely no change needed since it's a feature of the overall crate)
- Modify: `src/main.rs` — remove `OpenSettings` import if no longer used, clean up
- All files: run `cargo clippy` and fix any warnings

**Step 1: Clean up main.rs imports**

Remove `use humanssh::actions::OpenSettings` if it's now only used in workspace.rs.

Actually, `OpenSettings` action is still dispatched via keybinding — keep it.

**Step 2: Run clippy**

Run: `cargo clippy --workspace 2>&1 | tail -30`
Fix any warnings.

**Step 3: Run full test suite**

Run: `cargo test --workspace 2>&1 | tail -20`
Expected: all tests pass

**Step 4: Build release to verify**

Run: `cargo build --release 2>&1 | tail -10`
Expected: builds clean

**Step 5: Commit**

```bash
git add -A
git commit -m "chore: final cleanup — remove unused imports, fix clippy warnings"
```

---

### Task 11: Manual smoke test

**Not a code task — manual verification checklist:**

1. Delete `~/.config/humanssh/config.toml` (if exists)
2. Run `cargo run`
3. Verify: default `config.toml` was created with comments
4. Press `Cmd+,` — verify editor opens with the config file
5. Change `theme = "Nord"` in the config file, save
6. Verify: theme changes live without restart
7. Change `font-family = "Monaco"`, save
8. Verify: font changes live
9. Change `option-as-alt = false`, save
10. Verify: Option key behavior changes
