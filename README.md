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
- macOS (Linux/Windows support planned)

## Building

```sh
# Development build
cargo build

# Release build
cargo build --release
```

## Running

```sh
cargo run
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

| Action | Shortcut |
|--------|----------|
| New tab | `Cmd+T` |
| Close tab | `Cmd+W` |
| Next tab | `Cmd+Shift+]` |
| Previous tab | `Cmd+Shift+[` |
| Split vertical | `Cmd+Shift+D` |
| Split horizontal | `Cmd+D` |
| Settings | `Cmd+,` |
| Quit | `Cmd+Q` |

## License

AGPL-3.0-or-later
