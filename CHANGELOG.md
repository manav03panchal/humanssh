# Changelog

All notable changes to HumanSSH will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Disk usage display in status bar with color-coded percentage (green/yellow/red)
- Tab bar bottom border for visual separation
- Infrastructure for running commands in new tabs (`new_tab_with_command`)
- PWD inheritance: new tabs and splits inherit working directory from active pane
- Unfocused pane dimming (Ghostty-style 35% overlay) for visual focus indication
- Pane abstraction (`PaneKind` enum) for extensible pane types (terminal, SSH, etc.)
- Terminal types module (`src/terminal/types.rs`) for testable data structures
- Terminal colors module (`src/terminal/colors.rs`) for color conversion utilities
- Architecture documentation in terminal module
- Settings validation with file size limits (64KB max) and field validation
- Bracketed paste mode support to prevent clipboard injection attacks
- PTY spawn failure now displays error message in terminal with troubleshooting tips
- Drop implementation for PtyHandler to properly clean up child processes
- Cross-platform directory support using `dirs` crate
- High contrast theme for accessibility
- Debug mode via `HUMANSSH_DEBUG` environment variable
- Window state persistence (position and size saved across sessions)
- Drag-and-drop support for images (base64 encodes for AI assistants)
- Drag-and-drop support for files (pastes quoted path)
- Roadmap section in README documenting planned features

### Changed
- Drag-and-drop now pastes file paths instead of base64 encoding (reduces context window bloat for AI assistants)
- **Breaking**: `PaneNode` now uses `PaneKind` enum instead of `Entity<TerminalPane>` directly
- Extracted color conversion to `terminal/colors.rs` module
- Extracted terminal data types to `terminal/types.rs` module
- Reduced `terminal/pane.rs` from ~1815 lines to ~1560 lines
- Switched from `std::sync::Mutex` to `parking_lot::Mutex` (no poisoning panics)
- Extracted settings UI to separate module (`src/app/settings.rs`)
- Optimized `get_selected_text` to stream directly without intermediate grid allocation
- Optimized background region merging with single-pass on-the-fly algorithm
- Eliminated per-mouse-event string allocations using stack buffer
- Made process detection portable (Linux `/proc` + macOS `pgrep` fallback)
- Theme paths now canonicalized to prevent path traversal
- Split theme.rs into focused modules (persistence, colors, actions)
- Separated PaneNode tree logic from GPUI rendering (`pane_group_view.rs`)
- Consolidated display state into RwLock (size, cell_dims, bounds, font_size)
- Adaptive PTY polling (8ms active, 100ms idle) reduces CPU usage when terminal is idle
- Row-batched text shaping reduces allocations from O(cells) to O(rows)
- Bounded PTY output queue (1024 messages max) prevents memory exhaustion
- Increased PTY read buffer from 4KB to 32KB for better throughput
- Cached process detection (500ms TTL) avoids UI thread blocking
- Validated SHELL environment variable against allowlist (security)

### Fixed
- Race condition in process cleanup (TOCTOU fix)
- Silent settings save failures now logged
- Selection color now uses theme color instead of hardcoded value
- Tab title cache to reduce per-frame recomputation

### Removed
- Focused pane border styling (replaced with unfocused pane dimming)
- Commented SSH dependencies from Cargo.toml (will be re-added when implemented)

## [0.1.0] - 2026-01-30

### Added
- Initial release
- GPU-accelerated terminal emulator using GPUI framework
- Tabbed interface with multiple terminals
- Split panes (horizontal and vertical)
- Local shell via PTY
- Themeable interface (Catppuccin, Dracula, Gruvbox, Tokyo Night)
- Process-aware tab titles
- Mouse selection support
- Keyboard shortcuts for common operations

[Unreleased]: https://github.com/manav03panchal/humanssh/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/manav03panchal/humanssh/releases/tag/v0.1.0
