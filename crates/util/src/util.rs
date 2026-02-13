//! Shared utilities for HumanSSH.

/// Panic in debug builds, log error with backtrace in release.
///
/// Use for "this shouldn't happen" invariants that shouldn't crash
/// the terminal in production.
#[macro_export]
macro_rules! debug_panic {
    ( $($fmt_arg:tt)* ) => {
        if cfg!(debug_assertions) {
            panic!( $($fmt_arg)* );
        } else {
            let backtrace = std::backtrace::Backtrace::capture();
            tracing::error!("{}\n{:?}", format_args!($($fmt_arg)*), backtrace);
        }
    };
}
