//! Platform-specific native integrations.
//!
//! Provides macOS-native features: Secure Keyboard Entry, dock badge, notifications.
//! All functions are no-ops on non-macOS platforms.

#[cfg(target_os = "macos")]
mod macos;

// Re-export macOS implementations
#[cfg(target_os = "macos")]
pub use macos::*;

// No-op stubs for non-macOS platforms
#[cfg(not(target_os = "macos"))]
pub fn enable_secure_input() {}
#[cfg(not(target_os = "macos"))]
pub fn disable_secure_input() {}
#[cfg(not(target_os = "macos"))]
pub fn is_secure_input_enabled() -> bool {
    false
}
#[cfg(not(target_os = "macos"))]
pub fn bounce_dock_icon() {}
#[cfg(not(target_os = "macos"))]
pub fn set_dock_badge(_text: &str) {}
#[cfg(not(target_os = "macos"))]
pub fn clear_dock_badge() {}
#[cfg(not(target_os = "macos"))]
pub fn send_notification(_title: &str, _body: &str) {}
