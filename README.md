# HumanSSH

A fast, cross-platform terminal emulator built with Rust.

## Features

- GPU-accelerated rendering via [GPUI](https://gpui.rs) (Zed's UI framework)
- Terminal emulation powered by [alacritty_terminal](https://github.com/alacritty/alacritty)
- Tabs and split panes
- Themeable (Catppuccin themes included)
- Process-aware tab titles
- Confirmation dialogs for closing terminals with running processes

## Requirements

- Rust 1.75+
- macOS, Windows, or Linux

## Building

```sh
# Development build
cargo build

# Release build
cargo build --release

# macOS: Create .app bundle and DMG
./scripts/build-dmg.sh

# Windows: Create release package (PowerShell)
.\scripts\build-windows.ps1
```

## Running

```sh
cargo run
```

## Debug Mode

Enable verbose logging with the `HUMANSSH_DEBUG` environment variable:

```sh
HUMANSSH_DEBUG=1 cargo run
```

For custom log levels, use `RUST_LOG`:

```sh
RUST_LOG=humanssh=trace cargo run
```

## Development

Set up git hooks for automated checks:

```sh
./scripts/setup-hooks.sh
```

This installs a pre-commit hook that runs:
- `cargo fmt --check`
- `cargo clippy`
- `cargo check`

## Keybindings

| Action | macOS | Windows/Linux |
|--------|-------|---------------|
| New tab | `Cmd+T` | `Ctrl+T` |
| Close tab | `Cmd+W` | `Ctrl+W` |
| Next tab | `Cmd+Shift+]` | `Ctrl+Shift+]` |
| Previous tab | `Cmd+Shift+[` | `Ctrl+Shift+[` |
| Split vertical | `Cmd+Shift+D` | `Ctrl+Shift+D` |
| Split horizontal | `Cmd+D` | `Ctrl+D` |
| Settings | `Cmd+,` | `Ctrl+,` |
| Quit | `Cmd+Q` | `Ctrl+Q` |

## Roadmap

Planned features for future releases:

- **SSH Support** - Remote terminal sessions via SSH
- **Profiles** - Save and switch between connection profiles
- **Serial/Telnet** - Additional connection protocols

## License

AGPL-3.0-or-later
