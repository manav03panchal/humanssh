# HumanSSH MVP Design

## Overview

HumanSSH is a cross-platform, GPU-accelerated terminal application with SSH support. Think MobaXterm but fast, modern, and built on Rust.

## MVP Scope

### Included
- Tabbed interface (multiple terminals)
- Split panes (bento layout - horizontal/vertical splits)
- Local shell via PTY
- SSH connections via `russh`
- GPU-accelerated rendering via GPUI

### Deferred (Post-MVP)
- Saved server sidebar
- Encrypted credential vault
- SFTP file browser
- Server telemetry dashboard
- Session persistence

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                      HumanSSH                               │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐           │
│  │  Tab 1  │ │  Tab 2  │ │  Tab 3  │ │    +    │   Tabs    │
│  └─────────┴─┴─────────┴─┴─────────┴─┴─────────┘           │
├─────────────────────────────────────────────────────────────┤
│  ┌─────────────────────┬───────────────────────┐            │
│  │                     │                       │            │
│  │   Terminal Pane 1   │   Terminal Pane 2     │   Splits   │
│  │                     │                       │            │
│  └─────────────────────┴───────────────────────┘            │
├─────────────────────────────────────────────────────────────┤
│                      Status Bar                             │
└─────────────────────────────────────────────────────────────┘
```

## Technology Stack

| Component | Library | Purpose |
|-----------|---------|---------|
| UI Framework | gpui 0.2.2 | GPU-accelerated, Zed's framework |
| UI Components | gpui-component 0.6 | Pre-built widgets |
| Terminal Emulation | wezterm-term / termwiz | VT100/xterm escape codes |
| Local PTY | portable-pty | Cross-platform PTY spawning |
| SSH Client | russh | Pure Rust, async SSH |
| Async Runtime | tokio | Async I/O |

## Module Structure

```
src/
├── main.rs              # Entry point, window setup
├── lib.rs               # Re-exports
├── app/
│   ├── mod.rs
│   └── workspace.rs     # Main workspace container
├── terminal/
│   ├── mod.rs
│   ├── pane.rs          # Terminal pane (wezterm-term rendering)
│   ├── pty.rs           # Local PTY wrapper
│   └── input.rs         # Keyboard/mouse handling
├── ssh/
│   ├── mod.rs
│   └── session.rs       # russh session wrapper
├── tabs/
│   ├── mod.rs
│   ├── tab_bar.rs       # Tab strip UI
│   └── tab.rs           # Tab state
├── splits/
│   ├── mod.rs
│   └── container.rs     # Split pane container
└── theme.rs             # Colors, fonts
```

## Implementation Checkpoints

### Checkpoint 1: Project Setup + Window
- Create Cargo.toml with dependencies
- Basic main.rs that opens GPUI window
- **Verify:** `cargo run` opens empty window

### Checkpoint 2: Terminal Pane
- Integrate wezterm-term for terminal emulation
- Spawn local shell with portable-pty
- Render terminal output in pane
- Handle keyboard input
- **Verify:** `cargo run` shows working shell

### Checkpoint 3: Tab Bar
- Tab bar component with + button
- Tab switching
- New tab spawns new terminal
- Close tab with X or Cmd+W
- **Verify:** `cargo run` has working tabs

### Checkpoint 4: Split Panes
- Split container (horizontal/vertical)
- Keyboard shortcuts: Cmd+D (vertical), Cmd+Shift+D (horizontal)
- Drag to resize
- Focus management between panes
- **Verify:** `cargo run` can split and resize

### Checkpoint 5: SSH Support
- russh session management
- Detect `ssh` command or explicit SSH connection
- Channel multiplexing per session
- Disconnect handling
- **Verify:** `cargo run` can SSH to remote host

## Key Bindings (MVP)

| Binding | Action |
|---------|--------|
| Cmd+T | New tab |
| Cmd+W | Close tab/pane |
| Cmd+D | Split vertical |
| Cmd+Shift+D | Split horizontal |
| Cmd+[ / ] | Switch tabs |
| Cmd+Option+Arrow | Navigate panes |

## Design Decisions

1. **GPUI over Tauri**: Native performance, no webview overhead, matches Human* product family
2. **russh over system SSH**: Programmatic control needed for future SFTP, key management
3. **wezterm-term**: Battle-tested, handles edge cases, same ecosystem as portable-pty
4. **Single package**: Simpler than workspace for MVP, matches humanboard pattern
