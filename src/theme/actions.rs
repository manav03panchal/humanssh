//! Theme action handlers.
//!
//! Registers GPUI actions for macOS-native features.

use gpui::App;

/// Register theme-related actions
pub fn register_actions(cx: &mut App) {
    // macOS native feature actions
    cx.on_action(|_: &crate::actions::ToggleSecureInput, cx| {
        if crate::platform::is_secure_input_enabled() {
            crate::platform::disable_secure_input();
        } else {
            crate::platform::enable_secure_input();
        }
        cx.refresh_windows();
    });

    cx.on_action(|_: &crate::actions::ToggleOptionAsAlt, cx| {
        use std::sync::atomic::Ordering;
        let current = crate::terminal::OPTION_AS_ALT.load(Ordering::Relaxed);
        crate::terminal::OPTION_AS_ALT.store(!current, Ordering::Relaxed);
        tracing::info!("Option as Alt: {}", !current);
        cx.refresh_windows();
    });
}
