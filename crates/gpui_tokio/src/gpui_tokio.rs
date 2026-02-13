//! Tokio-GPUI bridge for HumanSSH.
//!
//! GPUI uses a smol-based executor. This crate runs a Tokio runtime alongside
//! it, allowing Tokio futures to be spawned from GPUI contexts. The returned
//! `gpui::Task` automatically cancels the Tokio future when dropped.

use gpui::{App, Global, Task};
use std::future::Future;
use tokio::runtime::Runtime;
use tokio::task::JoinError;

struct GlobalTokio {
    runtime: Runtime,
}

impl Global for GlobalTokio {}

/// Initialize the Tokio runtime as a GPUI global.
/// Call this once during app startup, before any `spawn()` calls.
pub fn init(cx: &mut App) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("Failed to initialize Tokio runtime");

    cx.set_global(GlobalTokio { runtime });
}

/// Spawn a future on the Tokio runtime, returning a GPUI `Task`.
///
/// The future runs on Tokio's thread pool. If the returned `Task` is dropped,
/// the Tokio future is cancelled automatically.
pub fn spawn<Fut, R>(cx: &App, f: Fut) -> Task<Result<R, JoinError>>
where
    Fut: Future<Output = R> + Send + 'static,
    R: Send + 'static,
{
    let runtime = cx.global::<GlobalTokio>();
    let join_handle = runtime.runtime.spawn(f);

    cx.background_executor().spawn(join_handle)
}

/// Run a blocking closure on the Tokio blocking thread pool.
pub fn spawn_blocking<F, R>(cx: &App, f: F) -> Task<Result<R, JoinError>>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let runtime = cx.global::<GlobalTokio>();
    let join_handle = runtime.runtime.spawn_blocking(f);

    cx.background_executor().spawn(join_handle)
}
