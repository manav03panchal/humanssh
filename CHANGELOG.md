# Changelog

All notable changes to HumanSSH will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Settings validation with file size limits (64KB max) and field validation
- Bracketed paste mode support to prevent clipboard injection attacks
- PTY spawn failure now displays error message in terminal with troubleshooting tips
- Drop implementation for PtyHandler to properly clean up child processes
- Cross-platform directory support using `dirs` crate
- High contrast theme for accessibility
- Debug mode via `HUMANSSH_DEBUG` environment variable
- Window state persistence (position and size saved across sessions)

### Changed
- Switched from `std::sync::Mutex` to `parking_lot::Mutex` (no poisoning panics)
- Extracted settings UI to separate module (`src/app/settings.rs`)
- Optimized `get_selected_text` to stream directly without intermediate grid allocation
- Optimized background region merging with single-pass on-the-fly algorithm
- Eliminated per-mouse-event string allocations using stack buffer
- Made process detection portable (Linux `/proc` + macOS `pgrep` fallback)
- Theme paths now canonicalized to prevent path traversal

### Fixed
- Race condition in process cleanup (TOCTOU fix)
- Silent settings save failures now logged
- Selection color now uses theme color instead of hardcoded value
- Tab title cache to reduce per-frame recomputation

### Removed
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
