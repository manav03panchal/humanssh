# GPUI API Audit: Modernization Opportunities

Audit of places where manual code could be replaced by GPUI / gpui-component built-in APIs.

## Actionable

### 1. Search Bar: Manual keyboard handling -> TextInput component

**Location:** `crates/terminal_view/src/pane.rs:1784-1830`

Currently handling backspace, char insertion, escape, enter manually in `on_key_down`. gpui-component has `TextInput` / `TextField` that handles cursor, selection, clipboard, IME natively. Would delete ~50 lines of fragile key routing.

**Priority:** High
**Effort:** Medium

### 2. Confirmation Dialog: Raw divs -> Modal/Dialog component

**Location:** `crates/workspace/src/workspace_view.rs:799-886`

85 lines of manual backdrop + modal container + button layout. We already use `Root::render_dialog_layer()`, and gpui-component has `Modal` / `Dialog`. Most obvious win.

**Priority:** Medium
**Effort:** Low

### 3. Hardcoded colors -> Theme tokens

**Locations:**
- `workspace_view.rs` — dialog backdrop, modal bg, tab drag ghost, drag-over highlight
- `pane.rs` — search bar bg/border, match highlight colors
- `pane_group_view.rs` — unfocused pane overlay

Scattered `hsla(...)` inline values. We already have `terminal_colors(cx)` from the theme crate. These should pull from the theme system so they adapt to light/dark themes.

**Priority:** Medium
**Effort:** Low

### 4. Hardcoded pixel sizes -> Existing constants

**Location:** `crates/workspace/src/workspace_view.rs`

Inline `px(38.0)`, `px(120.0)`, `px(200.0)`, `px(18.0)` — but we already define `constants::tab_bar::HEIGHT`, `TAB_MIN_WIDTH`, `TAB_MAX_WIDTH`, `CLOSE_BUTTON_SIZE`. Defined but not always used.

**Priority:** Low
**Effort:** Trivial

### 5. Config file watcher: Polling channel -> Async channel

**Location:** `crates/settings/src/file.rs:287-305`

A 50ms timer polling a `mpsc::channel`. GPUI's `cx.spawn` could await an async channel (e.g. `smol::channel`) directly instead of poll-and-sleep.

**Priority:** Low
**Effort:** Low

## Correct As-Is (Not Actionable)

These patterns look manual but are the right approach for a terminal emulator:

- **Canvas rendering / pixel math** — inherent to terminal grid rendering. Every terminal emulator does per-cell coordinate math. No framework abstraction exists for this.
- **PTY adaptive polling (8ms/100ms)** — standard PTY reading pattern. No reactive PTY API exists. Zed does the same.
- **UUID-based pane tracking** — GPUI focus tracks keyboard focus, not logical "active pane" for dimming overlays. Both are needed. Zed uses the same pattern.
- **Manual scroll offset math** — `display_offset` conversions are alacritty_terminal's API. No GPUI scroll view replaces a terminal grid.
- **Manual viewport clipping** — skipping off-screen cells in paint is a performance optimization, not a hack. GPUI clips at element boundary but we'd still waste time shaping invisible text.
- **Cell dimension measurement** — `text_system().advance()` and `ascent()`/`descent()` are GPUI's own APIs used correctly.

## Summary

| What | Effort | Impact |
|------|--------|--------|
| Search input -> TextInput | Medium | High |
| Dialog -> Modal component | Low | Medium |
| Inline colors -> theme tokens | Low | Medium |
| Use existing size constants | Trivial | Low |
| Config poll -> async channel | Low | Low |
