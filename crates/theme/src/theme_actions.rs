//! Theme action handlers.
//!
//! Registers GPUI actions for macOS-native features.

use gpui::App;

/// Register theme-related actions
pub fn register_actions(cx: &mut App) {
    // macOS native feature actions
    cx.on_action(|_: &actions::ToggleSecureInput, cx| {
        if platform::is_secure_input_enabled() {
            platform::disable_secure_input();
        } else {
            platform::enable_secure_input();
        }
        cx.refresh_windows();
    });

    cx.on_action(|_: &actions::ToggleOptionAsAlt, cx| {
        use std::sync::atomic::Ordering;
        let current = actions::OPTION_AS_ALT.load(Ordering::Relaxed);
        actions::OPTION_AS_ALT.store(!current, Ordering::Relaxed);
        tracing::info!("Option as Alt: {}", !current);
        cx.refresh_windows();
    });
}
