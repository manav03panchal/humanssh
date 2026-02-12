# Config File with Live Reload

Replace the settings dialog with a TOML config file (`~/.config/humanssh/config.toml`) that live-reloads on save, similar to Ghostty.

## Config Location

- All platforms: `~/.config/humanssh/config.toml`
- Replaces `settings.json`
- Created with defaults + comments on first launch if missing
- Existing `settings.json` migrated on first run, then ignored

## Config Schema

```toml
# Appearance
theme = "Catppuccin Mocha"
font-family = "Menlo"
font-size = 14

# Terminal behavior
option-as-alt = true

# macOS native
secure-keyboard-entry = false

# Window (auto-managed, user can override)
# window-width = 1200
# window-height = 800

# Platform-specific
# linux-decorations = "server"
# windows-shell = "powershell"
```

## Architecture

### `src/config/file.rs` (new)
- `Config` struct with `#[derive(Deserialize)]` via `toml` crate
- `notify` file watcher, debounced ~100ms
- On change: parse, validate, diff, apply to GPUI Theme global + terminal state
- Generates default config with comments when file is missing

### `src/app/settings.rs` (gutted)
- Replace `toggle_settings_dialog` with `open_config_in_editor`
- Uses `$EDITOR`, `open` (macOS), `xdg-open` (Linux), `start` (Windows)
- `Cmd+,` / `Ctrl+,` opens the config file

### Persistence
- `save_settings()` replaced with `Config::update_field()` using `toml_edit` for comment-preserving writes
- Window bounds still auto-saved

### Migration
- On first load: if `settings.json` exists but no `config.toml`, convert and write

## Dependencies
- `toml` — parsing
- `toml_edit` — comment-preserving writes
- `notify` — OS-native file watching (FSEvents/inotify)

## Removals
- Settings dialog UI (gpui-component Settings/SettingPage/SettingField)
- `discover_monospace_fonts`, `likely_latin_font`
- `config::dialog::SETTINGS_WIDTH`
- Font/theme dropdown builders

## Live Reload Flow

```
config.toml saved -> notify event -> debounce 100ms -> parse TOML
-> validate -> diff against current -> apply changes -> cx.refresh_windows()
```

## Open Config Flow

```
Cmd+, -> ensure config.toml exists (create default if needed)
-> open in $EDITOR / system default
```
