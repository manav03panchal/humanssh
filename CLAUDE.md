# Rust coding guidelines

* Prioritize code correctness and clarity. Speed and efficiency are secondary priorities unless otherwise specified.
* Do not write organizational or comments that summarize the code. Comments should only be written to explain "why" something is done a particular way when the reason is tricky or non-obvious.
* Prefer implementing functionality in existing files unless it is a new logical component.
* Avoid using functions that panic like `unwrap()`, instead use `?` to propagate errors.
* Be careful with operations like indexing which may panic if indexes are out of bounds.
* Never silently discard errors with `let _ =` on fallible operations. Always handle errors appropriately:
  - Propagate errors with `?` when the calling function should handle them
  - Use `tracing::warn!` or `tracing::error!` when you need to ignore errors but want visibility
  - Use explicit error handling with `match` or `if let Err(...)` when you need custom logic
* When implementing async operations that may fail, ensure errors propagate to the UI layer so users get meaningful feedback.
* Never create files with `mod.rs` paths — prefer `src/some_module.rs` instead of `src/some_module/mod.rs`.
* When creating new crates, specify the library root path in `Cargo.toml` using `[lib] path = "src/<crate_name>.rs"` and `doctest = false`.
* Use full words for variable names (no abbreviations like "q" for "queue").
* Use `parking_lot::Mutex` and `RwLock`, not `std::sync`.
* Use variable shadowing to scope clones in async contexts for clarity:
  ```rust
  cx.spawn({
      let handler = handler.clone();
      async move |cx| {
          // use handler
      }
  });
  ```

# Project structure

HumanSSH uses a Zed-style multi-crate workspace. All crates live under `crates/`, tooling under `tooling/`.

* `crates/humanssh` — binary entry point (bootstrap only)
* `crates/actions` — shared `gpui::actions!` definitions used across crates
* `crates/settings` — TOML config file, compile-time constants, validation
* `crates/platform` — platform-specific native code (macOS secure input, dock, notifications)
* `crates/theme` — theme loading, terminal color mapping, font persistence
* `crates/terminal` — terminal core: PTY handler, types, data structures (minimal GPUI dependency)
* `crates/terminal_view` — terminal GPUI view: rendering, input handling, color conversion
* `crates/workspace` — workspace UI: tabs, split panes, status bar
* `tooling/xtask` — build automation (`cargo xtask`)

Dependency flow (no cycles): `humanssh` → `workspace` → `terminal_view` → `terminal` → `settings`.

# Concurrency

* **No Tokio in GPUI spawn**: `App::spawn` / `Context::spawn` run on GPUI's smol executor, NOT Tokio. Use `cx.background_executor().timer(duration).await` instead of `tokio::time::sleep`.
* `App::spawn` signature: `AsyncFnOnce(&mut AsyncApp) -> R` (single arg).
* `window.text_system()` borrows `window` immutably; scope it in a block before `.paint()` which needs `&mut window`.

# GPUI patterns

* `cx.notify()` after any state change that affects rendering.
* Actions are defined in the `actions` crate, not inline. Use `actions!(namespace, [Action1, Action2])`.
* Event handlers: use `cx.listener(|this, event, window, cx| ...)` for entity-bound handlers.
* Entity events: `cx.emit(event)` to emit, `cx.subscribe(entity, handler)` to listen. Store subscriptions in `Vec<Subscription>`.

# Terminal-specific patterns

* `MouseEscBuf`: 32-byte fixed stack buffer for zero-alloc mouse escape sequence encoding.
* `InlineProcessName`: 64-byte inline string for process name cache (avoids heap alloc in hot path).
* `Arc<Mutex<Option<PtyHandler>>>` for PTY lifecycle (`None` = process exited).
* Adaptive polling: 8ms active (125fps), 100ms idle for PTY output.
* Wide chars: `CellFlags::WIDE_CHAR` = double-width, `WIDE_CHAR_SPACER` = placeholder. Spacers are skipped in render data.

# Testing

* Run all tests: `cargo test --workspace`
* Run clippy: `cargo clippy --workspace`
* `doctest = false` on all library crates.
* Platform `#[cfg]` inactive-code warnings are expected on macOS.

# Build

* `cargo build` only builds the main binary (via `default-members`).
* `cargo build --workspace` builds everything.
* `cargo xtask new-crate <name>` scaffolds a new crate.
* Release: `cargo build --release` (LTO, single codegen unit, stripped, panic=abort).
