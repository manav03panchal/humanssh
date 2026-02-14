# Tier 3 Features Design — Badges, Instant Replay, Password Manager

**Date:** 2026-02-13
**Branch:** feat/tier3-polish

## Overview

Three features completing the Tier 3 polish layer:

1. **Annotations/Badges** — visual process state on tabs
2. **Instant Replay** — session recording to asciinema v2 files
3. **Password Manager** — password prompt detection + macOS Keychain autofill

## Feature 1: Annotations/Badges

### Purpose
Show process state visually on each tab: running indicator, exit code on completion (green for success, red for failure).

### Architecture

**Exit code capture:**
- `crates/terminal/src/pty_handler.rs`: `PtyHandler` already detects exit via `try_wait()`. Extend to capture `ExitStatus` and store as `Option<i32>`.
- `crates/terminal/src/backend.rs` (if TerminalBackend trait exists): add `fn exit_status(&self) -> Option<i32>` to trait.

**Badge state:**
```rust
pub enum TabBadge {
    Running,
    Success,        // exit code 0
    Failed(i32),    // non-zero exit code
}
```
- Lives in `crates/terminal_view/src/pane.rs` on `TerminalPane`.
- `TerminalPane::badge() -> TabBadge` checks PTY exit status.

**Tab rendering:**
- `crates/workspace/src/workspace_view.rs`: tab rendering adds colored indicator next to title.
- Running: small green dot. Success: checkmark or `[0]`. Failed: red `[!N]`.

### Files Modified
- `crates/terminal/src/pty_handler.rs` — capture exit code
- `crates/terminal_view/src/pane.rs` — `TabBadge` enum, `badge()` method
- `crates/workspace/src/workspace_view.rs` — badge rendering in tab bar

### Effort
~200-300 lines.

## Feature 2: Instant Replay (Session Recording)

### Purpose
Record terminal sessions to asciinema v2 `.cast` files. Replay within the terminal with a timeline scrubber.

### Asciinema v2 Format
```
{"version": 2, "width": 80, "height": 24, "timestamp": 1234567890}
[0.5, "o", "$ ls\r\n"]
[1.0, "o", "file1.txt\r\n"]
[2.0, "i", "cd src\r\n"]
```
Line 1: JSON header. Subsequent lines: `[elapsed_seconds, event_type, data]`.
- `"o"` = output (PTY → screen)
- `"i"` = input (keyboard → PTY)

### Architecture

**Recording layer** (`crates/terminal/src/recording.rs`):
```rust
pub struct SessionRecorder {
    writer: BufWriter<File>,
    start_time: Instant,
    active: bool,
}
```
- `new(width, height) -> Result<Self>` — creates `.cast` file in `<data-dir>/humanssh/recordings/`
- `record_output(data: &[u8])` — appends timestamped output event
- `record_input(data: &[u8])` — appends timestamped input event
- `finish()` — flushes and closes file

**Integration point:** PTY output tee in `TerminalPane` — when recording is active, each output chunk is also sent to `SessionRecorder`.

**Replay mode** (`crates/terminal_view/src/replay.rs`):
```rust
pub struct ReplayState {
    events: Vec<ReplayEvent>,
    current_index: usize,
    speed: f32,           // 1.0 = real-time
    playing: bool,
    elapsed: Duration,
}
```
- Opens a `.cast` file, parses events
- Feeds output events to a read-only terminal at recorded timing
- Timeline scrubber overlay with play/pause, speed controls, seek

**Actions** (`crates/actions`):
- `StartRecording`, `StopRecording` — toggle recording on active pane
- `OpenReplay` — file picker to open `.cast` file

**Command palette entries** for all three actions.

### Files Created/Modified
- NEW: `crates/terminal/src/recording.rs` — SessionRecorder
- NEW: `crates/terminal_view/src/replay.rs` — ReplayState, replay rendering
- `crates/terminal_view/src/pane.rs` — recording integration, replay pane mode
- `crates/actions/src/actions.rs` — recording/replay actions
- `crates/workspace/src/workspace_view.rs` — action handlers
- `crates/workspace/src/command_palette.rs` — command entries

### Effort
~500-600 lines.

## Feature 3: Password Manager

### Purpose
Detect password prompts in terminal output and offer autofill from macOS Keychain.

### Architecture

**Password detection** (`crates/terminal/src/password_detect.rs`):
```rust
pub struct PasswordDetector {
    patterns: Vec<Regex>,
}
```
- Scans recent terminal output for patterns: `Password:`, `password for`, `Enter passphrase`, `PIN:`, `sudo password`, etc.
- Checks terminal mode for no-echo flag (strong signal for password input).
- Returns `PasswordPrompt { service_hint: Option<String>, prompt_text: String }`.
- Detection runs on VT processor output, not raw bytes.

**Keychain integration** (`crates/platform/src/keychain.rs`):
macOS:
```rust
pub fn query_keychain(service: &str) -> Option<String>
pub fn store_keychain(service: &str, account: &str, password: &str) -> Result<()>
pub fn list_keychain_accounts(service: &str) -> Vec<String>
```
- Uses `security-framework` crate (Rust wrapper for macOS Security.framework)
- Queries generic passwords by service name
- Falls back gracefully on non-macOS (returns None / no-ops)

Other platforms: no-op stubs returning `None`.

**UI overlay** (`crates/terminal_view/src/pane.rs`):
- When password prompt detected: show subtle hint bar below terminal ("Password detected — press Ctrl+P to autofill")
- `AutofillPassword` action: queries Keychain, sends password to PTY as input
- `DismissPasswordHint` action: hides hint
- Password is written directly to PTY (never displayed or logged)

**Security considerations:**
- Keychain access requires macOS user authentication (Touch ID / system password)
- Password strings are zeroized after use (`zeroize` crate)
- No password logging even at trace level
- Hint bar does not show the password itself

### Files Created/Modified
- NEW: `crates/terminal/src/password_detect.rs` — PasswordDetector
- NEW: `crates/platform/src/keychain.rs` — Keychain integration
- `crates/terminal_view/src/pane.rs` — password hint UI, autofill handler
- `crates/actions/src/actions.rs` — AutofillPassword, DismissPasswordHint
- `crates/platform/Cargo.toml` — security-framework dependency
- `crates/workspace/src/command_palette.rs` — command entries

### Effort
~400-500 lines.

## Parallelization

All three features are independent — no shared state or code conflicts:
- **Agent A:** Badges (terminal → terminal_view → workspace tab rendering)
- **Agent B:** Instant Replay (terminal recording → terminal_view replay → workspace actions)
- **Agent C:** Password Manager (terminal detection → platform keychain → terminal_view UI)

File conflicts are minimal. Only `actions.rs` and `command_palette.rs` are touched by all three, but the changes are additive (appending new entries).

## Not in Scope
- Accessibility (blocked by GPUI lacking native a11y APIs)
- Scripting API (deferred)
- Recording input events (only output for MVP)
- Cross-platform keychain (macOS only for now, stubs for others)
