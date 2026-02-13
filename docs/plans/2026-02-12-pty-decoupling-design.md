# PTY Decoupling: Dedicated VT Processing Thread

**Date**: 2026-02-12
**Status**: Approved

## Problem

Under high output (`yes`, large compiles), the terminal UI freezes. The root cause: VT parsing runs on GPUI's smol executor (in `start_pty_polling`), competing with GPUI's event processing. The polling task holds `term.lock()` for VT parsing at up to 125fps, while render prepaint also needs `term.lock()` to build RenderData. Under load, executor starvation + lock contention = frozen UI.

## Solution

Move VT processing to a **dedicated OS thread**. Add **frame coalescing** (batch PTY reads) and **render throttling** (60fps cap). Keep `Arc<Mutex<Term>>` shared — selection, scroll, resize still work directly from the main thread.

## Threading Model

```
PTY Reader Thread ──→ sync_channel ──→ [VT Thread: drain + batch + parse + throttle]
                                             holds term.lock() briefly
                                                    │
                                            async signal (≤60fps)
                                                    │
                                        [Smol Task: just cx.notify()]
                                                    │
                                        [Main: prepaint reads term.lock() → paint]
```

## Key Components

### TerminalProcessor (new, in crates/terminal)

Owns the VT processing thread. Spawned by TerminalPane. Contains:
- `JoinHandle` for the OS thread
- `mpsc::Sender<()>` for graceful shutdown

### VT Thread Loop

- Blocks on `output_rx.recv_timeout(100ms)` (PTY reader's existing channel)
- On data: drain all pending, batch into single buffer, lock term briefly for `advance()`
- Throttle render signals to 60fps (16ms minimum interval)
- Reuse batch buffer across iterations (avoid reallocation)
- On PTY exit or shutdown signal: send final render signal and exit

### GPUI Signal Receiver

Minimal async task replacing `start_pty_polling`. Does zero VT work:
- Awaits `smol::channel` signal from VT thread
- Coalesces multiple pending signals
- Calls `cx.notify()` or emits `TerminalExitEvent`

## Changes By File

1. `crates/terminal/src/pty_handler.rs` — Extract `output_rx` from PtyHandler, add `TerminalProcessor`
2. `crates/terminal_view/src/pane.rs` — Replace `start_pty_polling` with VT thread + signal receiver
3. `crates/terminal/src/types.rs` — No changes needed

## What Stays The Same

- PTY reader thread (background, reads fd into sync_channel)
- Render path (prepaint locks term → build RenderData → paint with no locks)
- Input path (pty.lock() → write bytes)
- Selection, scroll, resize (term.lock() from main thread)

## Future Work (not this PR)

- Damage tracking: hash rows, only rebuild changed rows
- Snapshot rendering: VT thread builds RenderData, render just reads
- Adaptive throttling: lower fps when idle
