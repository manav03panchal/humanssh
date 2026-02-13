# Ghostty vs iTerm2: Feature Comparison for HumanSSH Roadmap

**Date**: 2026-02-12

## 1. Crucial Features (Must-Have for Production-Grade Terminal)

### 1.1 Terminal Emulation

| Feature | Ghostty | iTerm2 | Notes |
|---------|---------|--------|-------|
| VT100/xterm compatibility | Yes | Yes | Both highly compatible |
| True color (24-bit) | Yes | Yes | Both support full 24-bit and 256-color |
| Unicode / emoji | Yes | Yes | Ghostty has grapheme clustering for multi-codepoint emoji |
| Wide character rendering | Yes | Yes | Both handle CJK double-width correctly |
| Right-to-left scripts | Yes (grapheme clustering) | Yes (opt-in RTL) | |

### 1.2 Performance & Rendering

| Feature | Ghostty | iTerm2 | Notes |
|---------|---------|--------|-------|
| GPU-accelerated rendering | Yes (Metal/OpenGL) | Partial (disabled with ligatures) | Ghostty's Metal renderer supports ligatures simultaneously |
| Input latency | Very low | Moderate | Ghostty ~480-500 FPS under stress |
| Memory efficiency | ~129MB under load | ~207MB under load | Benchmarked with identical workloads |
| Scrollback buffer | Configurable (byte-based, in-memory) | Configurable (line-based, disk persistence) | iTerm2 can persist scrollback to disk |

### 1.3 Tabs, Splits, and Panes

| Feature | Ghostty | iTerm2 | Notes |
|---------|---------|--------|-------|
| Tabs | Yes (native) | Yes | Both use native macOS tab bars |
| Split panes | Yes (native) | Yes (h/v) | Both support arbitrary splits |
| Multiple windows | Yes | Yes | |
| Drag-and-drop tab reordering | Yes | Yes | |
| Custom pane arrangements | Limited | Yes (saved arrangements, named layouts) | iTerm2 has rich layout saving |

### 1.4 Configuration

| Feature | Ghostty | iTerm2 | Notes |
|---------|---------|--------|-------|
| Config file format | Plain text key=value | GUI preferences + plist | Ghostty is text-config-first |
| Live config reload | Yes | Partial | Ghostty reloads without restart |
| Profiles | No | Yes (rich profile system) | iTerm2's profiles are significantly more mature |
| Automatic profile switching | No | Yes (hostname, directory, job) | iTerm2-exclusive |
| Themes | Yes (hundreds built-in, auto light/dark) | Yes (color presets, importable) | Ghostty ships more built-in |

### 1.5 Search

| Feature | Ghostty | iTerm2 | Notes |
|---------|---------|--------|-------|
| In-buffer text search | Yes | Yes | |
| Regex search | Limited | Yes (full regex + instant highlighting) | |
| Global search (all tabs) | No | Yes | iTerm2 searches across all tabs |

### 1.6 Selection and Copy

| Feature | Ghostty | iTerm2 | Notes |
|---------|---------|--------|-------|
| Mouse text selection | Yes | Yes | |
| Smart selection | Basic | Yes (configurable regex rules) | iTerm2 auto-detects URLs, emails, filenames |
| Rectangular selection | No | Yes (Cmd+Option drag) | |
| URL click-to-open | Yes | Yes | |
| Copy mode (keyboard selection) | No | Yes (vim-like) | |

### 1.7 Shell Integration

| Feature | Ghostty | iTerm2 | Notes |
|---------|---------|--------|-------|
| Automatic shell integration | Yes (Bash, Zsh, Fish, Elvish, Nushell) | Yes (Bash, Zsh, Fish, tcsh) | Ghostty supports more shells |
| OSC 133 prompt marking | Yes | Yes | |
| Current directory tracking | Yes (OSC 7) | Yes | |
| Command output navigation | Yes | Yes (+ select command output) | |

### 1.8 Keyboard & Input

| Feature | Ghostty | iTerm2 | Notes |
|---------|---------|--------|-------|
| Custom keybindings | Yes (sequences/chords) | Yes (GUI editor) | |
| Kitty keyboard protocol | Yes | No | Modern enhanced key reporting |
| IME support | Yes (some issues) | Yes (mature) | |

### 1.9 Fonts

| Feature | Ghostty | iTerm2 | Notes |
|---------|---------|--------|-------|
| Ligature support | Yes (GPU, no penalty) | Yes (disables GPU renderer) | Major Ghostty advantage |
| Font fallback chains | Yes (explicit) | Yes (system) | |
| Variable font support | Yes (configurable axes) | No | |
| Built-in Nerd Fonts | Yes (embedded) | No | |
| OpenType feature control | Yes | Limited | |

---

## 2. Nice-to-Have Features (Differentiators / Polish)

### 2.1 Platform Integration (macOS)

| Feature | Ghostty | iTerm2 | Notes |
|---------|---------|--------|-------|
| Secure Keyboard Entry | Yes (auto-detect) | Yes (manual toggle) | Ghostty's auto-detect is better UX |
| Quick Terminal (drop-down) | Yes | Yes (Hotkey Window) | |
| Proxy Icon | Yes | No | Native macOS drag-and-drop of CWD |
| Quick Look | Yes | No | Three-finger tap for definitions |
| Touch Bar | No | Yes | |
| Notification Center | No | Yes (activity, bell, idle) | |
| Command Palette | Yes | No | Ctrl+Shift+P, searchable actions |
| Progress Bar (OSC 9;4) | Yes | No | Native GUI progress bars |

### 2.2 Cross-Platform

| Feature | Ghostty | iTerm2 | Notes |
|---------|---------|--------|-------|
| Linux (Wayland/X11) | Yes | No (macOS only) | |
| Windows | Planned | No | |

### 2.3 Images & Graphics

| Feature | Ghostty | iTerm2 | Notes |
|---------|---------|--------|-------|
| Kitty graphics protocol | Yes | No | |
| iTerm2 inline images | No | Yes (imgcat, GIF, PDF) | Widely adopted protocol |
| Synchronized rendering | Yes | No | Eliminates tearing |

### 2.4 Multiplexer Support

| Feature | Ghostty | iTerm2 | Notes |
|---------|---------|--------|-------|
| tmux control mode | No | Yes (deep native integration) | Major iTerm2 differentiator |
| Native multiplexing | Yes | Partial | |

### 2.5 Scripting & Automation

| Feature | Ghostty | iTerm2 | Notes |
|---------|---------|--------|-------|
| Python scripting API | No | Yes (comprehensive) | |
| Triggers (regex actions) | No | Yes | Auto-respond, highlight, notify |
| Custom status bar | No | Yes (13 built-in + Python) | |

### 2.6 SSH & Remote

| Feature | Ghostty | iTerm2 | Notes |
|---------|---------|--------|-------|
| SSH terminfo auto-copy | Yes | No | |
| SCP file download (click) | No | Yes | |
| Drag-and-drop upload | No | Yes | |
| URL scheme handler (ssh://) | No | Yes | |

### 2.7 Accessibility

| Feature | Ghostty | iTerm2 | Notes |
|---------|---------|--------|-------|
| VoiceOver / screen reader | Minimal (opt-in) | Yes (reads screen, braille) | iTerm2 significantly better |
| Minimum contrast | No | Yes | Auto-adjusts for readability |
| Smart cursor color | No | Yes | |

### 2.8 History & Recovery

| Feature | Ghostty | iTerm2 | Notes |
|---------|---------|--------|-------|
| Instant Replay | No | Yes | Scrub through visual history |
| Paste history | No | Yes | Searchable clipboard history |
| Autocomplete | No | Yes (Cmd+;) | From terminal history |

### 2.9 Advanced Input

| Feature | Ghostty | iTerm2 | Notes |
|---------|---------|--------|-------|
| Advanced paste | No | Yes (edit, transform before paste) | |
| Password manager | No | Yes (Keychain-backed) | |
| Annotations | No | Yes | |
| Badges | No | Yes (overlay labels: hostname, git branch) | |
| Timestamps per line | No | Yes | |
| Captured Output | No | Yes (IDE-like error navigation) | |

---

## 3. HumanSSH Roadmap Priorities

### Tier 1 -- Ship Blockers (both competitors have these)
- [ ] True color, Unicode/emoji, wide chars
- [ ] Tabs, splits, multiple windows
- [ ] In-buffer search
- [ ] Mouse and keyboard selection
- [ ] Shell integration (OSC 7, OSC 133)
- [ ] Custom keybindings
- [ ] Font fallback and ligatures
- [ ] URL detection and click-to-open
- [ ] Configurable scrollback

### Tier 2 -- Competitive Differentiation
- [ ] **Decouple PTY processing from rendering** (critical perf fix)
  - Move PTY read + VT parsing to a dedicated OS thread (not GPUI's smol executor)
  - Render throttling: cap at 60fps, coalesce multiple reads into one render
  - Snapshot-based rendering: paint from a grid snapshot, don't hold term lock during paint
  - Damage tracking: only repaint rows that changed
  - Current issue: under high output (`yes`, big compiles), main thread starves → UI freezes
- [ ] GPU rendering with ligatures (follow Ghostty)
- [ ] Kitty keyboard/graphics protocol (follow Ghostty)
- [ ] tmux control mode integration (follow iTerm2)
- [ ] Regex search (follow iTerm2)
- [ ] Rectangular selection and Copy Mode (follow iTerm2)
- [ ] Profile system with auto-switching (follow iTerm2)
- [ ] Built-in SSH with native multiplexing (HumanSSH differentiator)

### Tier 3 -- Polish
- [ ] Command palette, progress bars, Quick Terminal
- [ ] Instant Replay, annotations, badges
- [ ] Password manager, advanced paste
- [ ] Accessibility (VoiceOver)
- [ ] Scripting API
