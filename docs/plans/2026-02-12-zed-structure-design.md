# Zed-Style Project Structure Refactoring

**Date**: 2026-02-12
**Status**: Approved

## Goal

Refactor HumanSSH from a single binary crate into a Zed-style multi-crate workspace for production-grade maintainability and scalability.

## Crate Structure

```
crates/
├── humanssh/          # Binary crate — app bootstrap only
├── terminal/          # Terminal core — PTY, types, data structures
├── terminal_view/     # Terminal GPUI view — rendering, input, selection
├── workspace/         # Workspace UI — tabs, pane groups, splits, status bar
├── settings/          # Configuration — TOML config, constants, validation
├── theme/             # Theme system — loading, colors, file watching
├── platform/          # Platform-specific native code (macOS, Linux, Windows)
├── actions/           # Shared action definitions (gpui::actions!)
tooling/
└── xtask/             # Build automation — new-crate scaffolding
```

## Conventions (Matching Zed)

- Named lib roots: `src/<crate_name>.rs` not `src/lib.rs`
- `[lib] path = "src/<name>.rs"` + `doctest = false` in every Cargo.toml
- All dependencies centralized in root `[workspace.dependencies]`
- `[workspace.lints]` with clippy deny rules inherited by all crates
- `[workspace.package]` for shared metadata (edition, license, publish)
- `default-members = ["crates/humanssh"]` for fast `cargo build`
- `test-support` feature flags for test infrastructure
- Modern module style (file.rs not mod.rs/) where possible

## Dependency Flow (no cycles)

```
humanssh (bin)
  ├── workspace
  │   ├── terminal_view
  │   │   ├── terminal
  │   │   │   └── settings
  │   │   └── theme
  │   ├── settings
  │   └── actions
  ├── theme
  ├── settings
  ├── platform
  └── actions
```

## File Movement Plan

| Source | Destination Crate | Notes |
|--------|-------------------|-------|
| src/main.rs | crates/humanssh/src/main.rs | Slim down to bootstrap only |
| src/actions.rs | crates/actions/src/actions.rs | + terminal actions from terminal/mod.rs |
| src/config/mod.rs, file.rs | crates/settings/src/ | Rename config → settings |
| src/platform/mod.rs, macos.rs | crates/platform/src/ | |
| src/theme/*.rs | crates/theme/src/ | |
| src/terminal/pty_handler.rs, types.rs | crates/terminal/src/ | Core logic only |
| src/terminal/pane.rs, colors.rs | crates/terminal_view/src/ | View + color conversion |
| src/app/*.rs | crates/workspace/src/ | All workspace UI |
| (new) | tooling/xtask/src/main.rs | Build automation |

## Workspace Cargo.toml Patterns

- Centralized `[workspace.dependencies]` for all external + internal deps
- Workspace lints: `dbg_macro = "deny"`, `todo = "deny"`, style allow
- Profile tuning: dev incremental, release LTO + strip
- `default-members = ["crates/humanssh"]`
