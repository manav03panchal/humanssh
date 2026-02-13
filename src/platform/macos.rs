//! macOS-specific native integrations via Objective-C FFI.

use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{info, warn};

static SECURE_INPUT_ACTIVE: AtomicBool = AtomicBool::new(false);

// Carbon framework functions for secure keyboard entry
#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn EnableSecureEventInput() -> i32;
    fn DisableSecureEventInput() -> i32;
    fn IsSecureEventInputEnabled() -> u8; // Returns Boolean (UInt8)
}

/// Enable macOS Secure Keyboard Entry.
/// Prevents other apps from intercepting keystrokes (password entry, etc.)
pub fn enable_secure_input() {
    unsafe {
        let result = EnableSecureEventInput();
        if result == 0 {
            SECURE_INPUT_ACTIVE.store(true, Ordering::SeqCst);
            info!("Secure keyboard entry enabled");
        } else {
            warn!("Failed to enable secure keyboard entry: {}", result);
        }
    }
}

/// Disable macOS Secure Keyboard Entry.
pub fn disable_secure_input() {
    unsafe {
        let result = DisableSecureEventInput();
        if result == 0 {
            SECURE_INPUT_ACTIVE.store(false, Ordering::SeqCst);
            info!("Secure keyboard entry disabled");
        } else {
            warn!("Failed to disable secure keyboard entry: {}", result);
        }
    }
}

/// Check if Secure Keyboard Entry is currently enabled.
pub fn is_secure_input_enabled() -> bool {
    unsafe { IsSecureEventInputEnabled() != 0 }
}

/// Bounce the dock icon once (informational request).
pub fn bounce_dock_icon() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;

    // GPUI runs on the main thread, so this should always succeed
    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        // NSInformationalRequest = 10 (bounce once)
        unsafe {
            let _: isize = objc2::msg_send![&app, requestUserAttention: 10_isize];
        }
    } else {
        warn!("bounce_dock_icon called from non-main thread");
    }
}

/// Set the dock icon badge text.
pub fn set_dock_badge(text: &str) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;
    use objc2_foundation::NSString;

    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        let dock_tile = app.dockTile();
        let ns_text = NSString::from_str(text);
        dock_tile.setBadgeLabel(Some(&ns_text));
    } else {
        warn!("set_dock_badge called from non-main thread");
    }
}

/// Clear the dock icon badge.
pub fn clear_dock_badge() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;

    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        let dock_tile = app.dockTile();
        dock_tile.setBadgeLabel(None);
    } else {
        warn!("clear_dock_badge called from non-main thread");
    }
}

/// Send a macOS user notification via osascript.
/// Uses AppleScript as a simple, permission-free notification mechanism.
pub fn send_notification(title: &str, body: &str) {
    use std::process::Command;

    let script = format!(
        "display notification \"{}\" with title \"{}\"",
        body.replace('\\', "\\\\").replace('"', "\\\""),
        title.replace('\\', "\\\\").replace('"', "\\\""),
    );

    match Command::new("osascript").args(["-e", &script]).spawn() {
        Ok(_) => info!("Notification sent: {}", title),
        Err(e) => warn!("Failed to send notification: {}", e),
    }
}
