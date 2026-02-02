//! Settings dialog UI.
//!
//! Extracted from workspace.rs to reduce module size and separate concerns.

use crate::theme::{SwitchFont, SwitchTheme};
use gpui::{div, px, App, IntoElement, ParentElement, SharedString, Styled, Window};
use gpui_component::button::Button;
use gpui_component::menu::DropdownMenu;
use gpui_component::theme::ThemeRegistry;
use gpui_component::{v_flex, ActiveTheme, StyledExt, WindowExt};

/// Common monospace fonts for terminals.
const TERMINAL_FONTS: &[&str] = &[
    "Iosevka Nerd Font",
    "JetBrains Mono",
    "Fira Code",
    "SF Mono",
    "Monaco",
    "Menlo",
    "Source Code Pro",
    "Cascadia Code",
    "Consolas",
    "Ubuntu Mono",
];

/// Toggle the settings dialog (open if closed, close if open).
pub fn toggle_settings_dialog(window: &mut Window, cx: &mut App) {
    if window.has_active_dialog(cx) {
        window.close_dialog(cx);
        return;
    }

    window.open_dialog(cx, |dialog, window, cx| {
        dialog
            .title("Settings")
            .w(px(500.0))
            .child(render_settings_content(window, cx))
    });
}

/// Render the settings dialog content.
pub fn render_settings_content(_window: &mut Window, cx: &mut App) -> impl IntoElement {
    let current_theme = cx.theme().theme_name().clone();
    let current_font = cx.theme().font_family.to_string();

    v_flex()
        .gap_4()
        // Theme selection dropdown
        .child(
            v_flex()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .text_color(cx.theme().muted_foreground)
                        .child("Theme"),
                )
                .child(
                    Button::new("theme-dropdown")
                        .label(current_theme.clone())
                        .outline()
                        .w_full()
                        .dropdown_menu(move |menu, _, cx| {
                            let themes = ThemeRegistry::global(cx).sorted_themes();
                            let current = cx.theme().theme_name().clone();
                            let mut menu = menu.min_w(px(200.0));
                            for theme in themes {
                                let name = theme.name.clone();
                                let is_current = current == name;
                                menu = menu.menu_with_check(
                                    name.clone(),
                                    is_current,
                                    Box::new(SwitchTheme(name)),
                                );
                            }
                            menu
                        }),
                ),
        )
        // Font family selection dropdown
        .child(
            v_flex()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_semibold()
                        .text_color(cx.theme().muted_foreground)
                        .child("Terminal Font"),
                )
                .child(
                    Button::new("font-dropdown")
                        .label(current_font.clone())
                        .outline()
                        .w_full()
                        .dropdown_menu(move |menu, _, cx| {
                            let current = cx.theme().font_family.to_string();
                            let mut menu = menu.min_w(px(200.0));
                            for font in TERMINAL_FONTS {
                                let is_current = current == *font;
                                let font_name: SharedString = (*font).into();
                                menu = menu.menu_with_check(
                                    *font,
                                    is_current,
                                    Box::new(SwitchFont(font_name)),
                                );
                            }
                            menu
                        }),
                ),
        )
        .child(
            div()
                .pt_2()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child("Press Cmd+, to close"),
        )
}
