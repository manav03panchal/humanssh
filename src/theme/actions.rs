//! Theme switching actions.
//!
//! Provides GPUI actions for switching themes and fonts.

use gpui::{App, SharedString};
use gpui_component::theme::{Theme, ThemeMode, ThemeRegistry};

/// Action to switch theme by name
#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = theme, no_json)]
pub struct SwitchTheme(pub SharedString);

/// Action to switch font family
#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = theme, no_json)]
pub struct SwitchFont(pub SharedString);

/// Action to switch theme mode (light/dark)
#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = theme, no_json)]
pub struct SwitchThemeMode(pub ThemeMode);

/// Register theme switching actions
pub fn register_actions(cx: &mut App) {
    cx.on_action(|action: &SwitchTheme, cx| {
        if let Some(theme_config) = ThemeRegistry::global(cx).themes().get(&action.0).cloned() {
            Theme::global_mut(cx).apply_config(&theme_config);
            tracing::info!("Switched to theme: {}", action.0);
        }
        cx.refresh_windows();
    });

    cx.on_action(|action: &SwitchFont, cx| {
        Theme::global_mut(cx).font_family = action.0.clone();
        tracing::info!("Switched to font: {}", action.0);
        cx.refresh_windows();
    });

    cx.on_action(|action: &SwitchThemeMode, cx| {
        Theme::change(action.0, None, cx);
        cx.refresh_windows();
    });
}
