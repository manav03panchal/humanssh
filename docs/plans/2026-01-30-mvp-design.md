# HumanSSH MVP Design

## Overview

HumanSSH is a cross-platform, GPU-accelerated terminal application with SSH support. Think MobaXterm but fast, modern, and built on Rust.

## MVP Scope

### Included
- Tabbed interface (multiple terminals)
- Split panes (bento layout - horizontal/vertical splits)
- Local shell via PTY
- GPU-accelerated rendering via GPUI
- Themeable (Catppuccin, Dracula, Gruvbox, Tokyo Night, High Contrast)
- Process-aware tab titles
- Window state persistence

### Deferred (Post-MVP)
- SSH connections via `russh`
- Saved server sidebar
- Encrypted credential vault
- SFTP file browser
- Server telemetry dashboard
- Session persistence

## Architecture

```
+-------------------------------------------------------------+
|                      HumanSSH                               |
+-------------------------------------------------------------+
|  +---------+ +---------+ +---------+ +---------+            |
|  |  Tab 1  | |  Tab 2  | |  Tab 3  | |    +    |   Tabs     |
|  +---------+-+---------+-+---------+-+---------+            |
+-------------------------------------------------------------+
|  +---------------------+-----------------------+             |
|  |                     |                       |             |
|  |   Terminal Pane 1   |   Terminal Pane 2     |   Splits    |
|  |                     |                       |             |
|  +---------------------+-----------------------+             |
+-------------------------------------------------------------+
```

## Technology Stack

| Component | Library | Purpose |
|-----------|---------|---------|
| UI Framework | gpui 0.2.2 | GPU-accelerated, Zed's framework |
| UI Components | gpui-component 0.5 | Pre-built widgets |
| Terminal Emulation | alacritty_terminal 0.25 | VT100/xterm escape codes |
| Local PTY | portable-pty 0.8 | Cross-platform PTY spawning |
| Async Runtime | tokio 1.x | Async I/O |

## Module Structure

```
src/
+-- main.rs              # Entry point, window setup, keybindings
+-- lib.rs               # Module re-exports
+-- config.rs            # Centralized configuration constants
+-- theme.rs             # Theme system, terminal colors
+-- actions.rs           # GPUI actions (Quit, CloseTab, etc.)
+-- app/
|   +-- mod.rs           # App module exports
|   +-- workspace.rs     # Main workspace (tabs, dialogs, settings)
|   +-- pane_group.rs    # Split pane tree structure
+-- terminal/
    +-- mod.rs           # Terminal module exports
    +-- pane.rs          # Terminal pane (rendering, input handling)
    +-- pty.rs           # PTY process wrapper
```

## Implementation Status

### Checkpoint 1: Project Setup + Window [COMPLETE]
- Created Cargo.toml with dependencies
- Basic main.rs that opens GPUI window
- Theme system with multiple themes

### Checkpoint 2: Terminal Pane [COMPLETE]
- Integrated alacritty_terminal for terminal emulation
- Spawn local shell with portable-pty
- Render terminal output in pane (GPU canvas)
- Handle keyboard input
- Mouse selection support

### Checkpoint 3: Tab Bar [COMPLETE]
- Tab bar component with + button
- Tab switching (Cmd+Shift+[ and Cmd+Shift+])
- New tab spawns new terminal (Cmd+T)
- Close tab with X or Cmd+W
- Process-aware tab titles

### Checkpoint 4: Split Panes [COMPLETE]
- Split container (horizontal/vertical)
- Keyboard shortcuts: Cmd+D (horizontal), Cmd+Shift+D (vertical)
- Focus management between panes
- Confirmation dialog for closing panes with running processes

### Checkpoint 5: SSH Support [DEFERRED]
- Commented out russh dependencies
- Will be implemented post-MVP

## Key Bindings (Current)

| Binding | Action |
|---------|--------|
| Cmd+T | New tab |
| Cmd+W | Close tab/pane |
| Cmd+D | Split horizontal |
| Cmd+Shift+D | Split vertical |
| Cmd+Shift+[ | Previous tab |
| Cmd+Shift+] | Next tab |
| Cmd+, | Settings |
| Cmd+Q | Quit |
| Cmd++/- | Zoom in/out |

## Design Decisions

1. **GPUI over Tauri**: Native performance, no webview overhead
2. **alacritty_terminal over wezterm-term**: Better maintained, simpler API, battle-tested
3. **portable-pty**: Cross-platform PTY spawning, works with alacritty_terminal
4. **Single package**: Simpler than workspace for MVP
