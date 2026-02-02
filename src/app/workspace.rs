//! Main workspace - container for tabs and split panes.

use super::pane_group::{PaneNode, SplitDirection};
use crate::actions::{OpenSettings, CloseTab, Quit};
use crate::terminal::TerminalPane;
use crate::theme::{terminal_colors, SwitchTheme};
use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::menu::DropdownMenu;
use gpui_component::theme::ThemeRegistry;
use gpui_component::{v_flex, ActiveTheme, IconName, Root, Sizable, StyledExt, WindowExt};
use uuid::Uuid;

/// Pending action requiring confirmation
#[derive(Clone, Copy, PartialEq)]
enum PendingAction {
    ClosePane,
    CloseTab(usize),
    Quit,
}

/// A single tab in the workspace
struct Tab {
    id: Uuid,
    fallback_title: String,
    panes: PaneNode,
    active_pane: Uuid,
}

impl Tab {
    /// Get the display title for this tab (dynamic from terminal or fallback)
    fn display_title(&self, cx: &App) -> String {
        // Try to get dynamic title from the active pane's terminal
        if let Some(terminal) = self.panes.find_terminal(self.active_pane) {
            if let Some(title) = terminal.read(cx).title() {
                // Extract just the last component (e.g., "vim file.txt" or "zsh")
                return title;
            }
        }
        self.fallback_title.clone()
    }
}

/// The main workspace view containing the tab bar and terminal panes.
pub struct Workspace {
    tabs: Vec<Tab>,
    active_tab: usize,
    /// Pending action requiring confirmation (shows dialog)
    pending_action: Option<PendingAction>,
    /// Process name for confirmation dialog
    pending_process_name: Option<String>,
}

impl Workspace {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let terminal = cx.new(TerminalPane::new);
        let panes = PaneNode::new_leaf(terminal);
        let active_pane = panes.first_leaf_id();

        let tab = Tab {
            id: Uuid::new_v4(),
            fallback_title: "Terminal 1".to_string(),
            panes,
            active_pane,
        };

        Self {
            tabs: vec![tab],
            active_tab: 0,
            pending_action: None,
            pending_process_name: None,
        }
    }

    /// Check if a tab has any running child processes
    fn tab_has_running_processes(&self, index: usize, cx: &App) -> bool {
        if let Some(tab) = self.tabs.get(index) {
            for (_, terminal) in tab.panes.all_terminals() {
                if terminal.read(cx).has_running_processes() {
                    return true;
                }
            }
        }
        false
    }

    /// Get the name of a running process in a tab (for display)
    fn get_tab_running_process_name(&self, index: usize, cx: &App) -> Option<String> {
        if let Some(tab) = self.tabs.get(index) {
            for (_, terminal) in tab.panes.all_terminals() {
                if let Some(name) = terminal.read(cx).get_running_process_name() {
                    return Some(name);
                }
            }
        }
        None
    }

    /// Check if any tab has running processes
    fn any_tab_has_running_processes(&self, cx: &App) -> bool {
        for i in 0..self.tabs.len() {
            if self.tab_has_running_processes(i, cx) {
                return true;
            }
        }
        false
    }

    /// Get the name of any running process across all tabs
    fn get_any_running_process_name(&self, cx: &App) -> Option<String> {
        for i in 0..self.tabs.len() {
            if let Some(name) = self.get_tab_running_process_name(i, cx) {
                return Some(name);
            }
        }
        None
    }

    /// Request to close a tab (with confirmation if needed)
    fn request_close_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.tab_has_running_processes(index, cx) {
            self.pending_process_name = self.get_tab_running_process_name(index, cx);
            self.pending_action = Some(PendingAction::CloseTab(index));
            cx.notify();
        } else {
            self.close_tab(index, cx);
        }
    }

    /// Request to quit (with confirmation if needed)
    pub fn request_quit(&mut self, cx: &mut Context<Self>) {
        if self.any_tab_has_running_processes(cx) {
            self.pending_process_name = self.get_any_running_process_name(cx);
            self.pending_action = Some(PendingAction::Quit);
            cx.notify();
        } else {
            cx.quit();
        }
    }

    /// Confirm the pending action
    fn confirm_pending_action(&mut self, cx: &mut Context<Self>) {
        if let Some(action) = self.pending_action.take() {
            self.pending_process_name = None;
            match action {
                PendingAction::ClosePane => self.do_close_pane(cx),
                PendingAction::CloseTab(index) => self.close_tab(index, cx),
                PendingAction::Quit => cx.quit(),
            }
        }
    }

    /// Cancel the pending action
    fn cancel_pending_action(&mut self, cx: &mut Context<Self>) {
        self.pending_action = None;
        self.pending_process_name = None;
        cx.notify();
    }

    fn new_tab(&mut self, cx: &mut Context<Self>) {
        let terminal = cx.new(TerminalPane::new);
        let panes = PaneNode::new_leaf(terminal);
        let active_pane = panes.first_leaf_id();
        let tab_num = self.tabs.len() + 1;

        let tab = Tab {
            id: Uuid::new_v4(),
            fallback_title: format!("Terminal {}", tab_num),
            panes,
            active_pane,
        };
        self.tabs.push(tab);
        self.active_tab = self.tabs.len() - 1;
        cx.notify();
    }

    fn close_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.tabs.len() <= 1 {
            // Last tab - quit the app
            cx.quit();
            return;
        }

        self.tabs.remove(index);

        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        } else if self.active_tab > index {
            self.active_tab -= 1;
        }

        cx.notify();
    }

    fn switch_tab(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.tabs.len() {
            self.active_tab = index;
            cx.notify();
        }
    }

    fn next_tab(&mut self, cx: &mut Context<Self>) {
        self.active_tab = (self.active_tab + 1) % self.tabs.len();
        cx.notify();
    }

    fn prev_tab(&mut self, cx: &mut Context<Self>) {
        if self.active_tab == 0 {
            self.active_tab = self.tabs.len() - 1;
        } else {
            self.active_tab -= 1;
        }
        cx.notify();
    }

    /// Split the active pane
    fn split_pane(&mut self, direction: SplitDirection, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            let new_terminal = cx.new(TerminalPane::new);
            if let Some(new_pane_id) = tab.panes.split(tab.active_pane, direction, new_terminal.clone()) {
                // Set the new pane as active
                tab.active_pane = new_pane_id;
                // Focus the new terminal
                new_terminal.read(cx).focus_handle.focus(window);
            }
            cx.notify();
        }
    }

    /// Set the active pane within the current tab
    pub fn set_active_pane(&mut self, pane_id: Uuid, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            tab.active_pane = pane_id;
            cx.notify();
        }
    }

    /// Request to close the active pane (with confirmation if needed)
    fn request_close_pane(&mut self, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(self.active_tab) {
            if let Some(terminal) = tab.panes.find_terminal(tab.active_pane) {
                if terminal.read(cx).has_running_processes() {
                    // Show confirmation
                    self.pending_process_name = terminal.read(cx).get_running_process_name();
                    self.pending_action = Some(PendingAction::ClosePane);
                    cx.notify();
                    return;
                }
            }
        }
        // No confirmation needed
        self.do_close_pane(cx);
    }

    /// Actually close the active pane (or tab if only one pane, or quit if last tab)
    fn do_close_pane(&mut self, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            let pane_count = tab.panes.all_terminals().len();

            if pane_count <= 1 {
                // Last pane in tab - close the tab
                self.close_tab(self.active_tab, cx);
            } else {
                // Remove the pane
                if tab.panes.remove(tab.active_pane).is_some() {
                    tab.active_pane = tab.panes.first_leaf_id();
                    cx.notify();
                }
            }
        }
    }

    /// Render the theme selector dropdown
    fn render_theme_selector(&self, _cx: &mut Context<Self>) -> impl IntoElement {
        Button::new("theme-selector")
            .icon(IconName::Palette)
            .small()
            .ghost()
            .tooltip("Select Theme")
            .dropdown_menu(move |menu, _, cx| {
                let themes = ThemeRegistry::global(cx).sorted_themes();
                let current = cx.theme().theme_name().clone();

                let mut menu = menu.min_w(px(180.0));

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
            })
    }

    /// Toggle the settings modal (open if closed, close if open)
    fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // If dialog is already open, close it
        if window.has_active_dialog(cx) {
            window.close_dialog(cx);
            return;
        }

        window.open_dialog(cx, |dialog, window, cx| {
            dialog
                .title("Settings")
                .w(px(500.0))
                .child(Self::render_settings_content(window, cx))
        });
    }

    /// Render settings dialog content
    fn render_settings_content(_window: &mut Window, cx: &mut App) -> impl IntoElement {
        let current_theme = cx.theme().theme_name().clone();
        let current_font = cx.theme().font_family.to_string();

        // Common monospace fonts for terminals
        let fonts = [
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

        v_flex()
            .gap_4()
            // Theme selection
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
            // Font selection
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
                                for font in fonts {
                                    let is_current = current == font;
                                    let font_name: SharedString = font.into();
                                    menu = menu.menu_with_check(
                                        font,
                                        is_current,
                                        Box::new(crate::theme::SwitchFont(font_name)),
                                    );
                                }
                                menu
                            }),
                    ),
            )
    }

}

/// Toggle the settings dialog (can be called from anywhere)
pub fn open_settings_dialog(window: &mut Window, cx: &mut App) {
    use gpui_component::WindowExt;

    // Toggle: close if open, open if closed
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

/// Render settings dialog content (standalone version for open_settings_dialog)
fn render_settings_content(_window: &mut Window, cx: &mut App) -> impl IntoElement {
    use crate::theme::{SwitchFont, SwitchTheme};
    use gpui_component::theme::ThemeRegistry;
    use gpui_component::{v_flex, ActiveTheme, StyledExt};
    use gpui_component::button::Button;

    let current_theme = cx.theme().theme_name().clone();
    let current_font = cx.theme().font_family.to_string();

    // Common monospace fonts for terminals
    let fonts = [
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
                            for font in fonts {
                                let is_current = current == font;
                                let font_name: SharedString = font.into();
                                menu = menu.menu_with_check(
                                    font,
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

impl Workspace {
    /// Check for and clean up exited panes
    fn cleanup_exited_panes(&mut self, cx: &mut Context<Self>) {
        let mut tabs_to_remove: Vec<usize> = Vec::new();

        for (tab_idx, tab) in self.tabs.iter_mut().enumerate() {
            // Get all terminals and check for exited ones
            let terminals = tab.panes.all_terminals();
            let total_panes = terminals.len();
            let mut exited_count = 0;

            for (pane_id, terminal) in &terminals {
                if terminal.read(cx).has_exited() {
                    exited_count += 1;
                    // Try to remove (only works for non-root panes)
                    tab.panes.remove(*pane_id);
                }
            }

            // If all panes exited, mark tab for removal
            if exited_count >= total_panes {
                tabs_to_remove.push(tab_idx);
            } else if exited_count > 0 {
                // Some panes removed - make sure active pane is valid
                if tab.panes.find_terminal(tab.active_pane).is_none() {
                    tab.active_pane = tab.panes.first_leaf_id();
                }
            }
        }

        // Remove tabs with all panes exited (in reverse order)
        for tab_idx in tabs_to_remove.into_iter().rev() {
            if self.tabs.len() > 1 {
                self.tabs.remove(tab_idx);
                if self.active_tab >= self.tabs.len() {
                    self.active_tab = self.tabs.len().saturating_sub(1);
                }
            } else {
                // Last tab - quit the app
                cx.quit();
            }
        }
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Check for and close exited panes
        self.cleanup_exited_panes(cx);

        // Focus the active terminal in the active pane
        if let Some(tab) = self.tabs.get(self.active_tab) {
            if let Some(terminal) = tab.panes.find_terminal(tab.active_pane) {
                let terminal_focus = terminal.read(cx).focus_handle.clone();
                if !terminal_focus.is_focused(window) {
                    window.focus(&terminal_focus);
                }
            }
        }

        // Get theme colors
        let colors = terminal_colors(cx);
        let title_bar_bg = colors.title_bar;
        let border_color = colors.border;
        let background = colors.background;
        let foreground = colors.foreground;
        let muted = colors.muted;
        let tab_active_bg = colors.tab_active;
        let _tab_inactive_bg = colors.tab_inactive;
        let red = colors.red;
        let _green = colors.green;

        let active_tab_idx = self.active_tab;
        let tab_count = self.tabs.len();

        // Pre-compute tab titles (dynamic from terminal or fallback)
        let tab_titles: Vec<String> = self.tabs.iter().map(|t| t.display_title(cx)).collect();

        div()
            .size_full()
            .bg(background)
            .flex()
            .flex_col()
            .on_action(cx.listener(|this, _: &OpenSettings, window, cx| {
                this.open_settings(window, cx);
            }))
            .on_action(cx.listener(|this, _: &Quit, _window, cx| {
                this.request_quit(cx);
            }))
            .on_action(cx.listener(|this, _: &CloseTab, _window, cx| {
                this.request_close_pane(cx);
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                let cmd = event.keystroke.modifiers.platform;
                let shift = event.keystroke.modifiers.shift;

                // Handle Cmd+Shift combinations first (more specific)
                if cmd && shift {
                    match key {
                        "d" | "D" => {
                            this.split_pane(SplitDirection::Vertical, window, cx);
                            return; // Don't fall through to Cmd+D
                        }
                        "]" => this.next_tab(cx),
                        "[" => this.prev_tab(cx),
                        _ => {}
                    }
                } else if cmd {
                    // Cmd only (no shift)
                    match key {
                        "t" => this.new_tab(cx),
                        "w" => this.request_close_pane(cx),
                        "d" => this.split_pane(SplitDirection::Horizontal, window, cx),
                        "}" | "]" => this.next_tab(cx),
                        "{" | "[" => this.prev_tab(cx),
                        "," => this.open_settings(window, cx),
                        _ => {}
                    }
                }
            }))
            .child(
                // Tab bar - flat cells stuck together
                div()
                    .h(px(38.0))
                    .w_full()
                    .bg(title_bar_bg)
                    .flex()
                    .pl(px(78.0))
                    .pr(px(8.0))
                    // Tabs - stuck together, no gaps
                    .children(self.tabs.iter().enumerate().zip(tab_titles.into_iter()).map(|((i, tab), title)| {
                        let is_active = i == active_tab_idx;
                        let tab_id = tab.id;

                        div()
                            .id(ElementId::Name(format!("tab-{}", tab_id).into()))
                            .h(px(38.0))
                            .min_w(px(120.0))
                            .max_w(px(200.0))
                            .px_3()
                            .flex()
                            .items_center()
                            .justify_between()
                            .cursor_pointer()
                            .border_r_1()
                            .border_color(border_color)
                            .when(is_active, |d| d.bg(background).text_color(foreground))
                            .when(!is_active, |d| {
                                d.bg(title_bar_bg)
                                    .text_color(muted)
                                    .hover(|d| d.bg(tab_active_bg))
                            })
                            .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                                this.switch_tab(i, cx);
                            }))
                            .child(
                                div()
                                    .text_sm()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .child(title),
                            )
                            .child(
                                div()
                                    .id(ElementId::Name(format!("close-{}", tab_id).into()))
                                    .w(px(18.0))
                                    .h(px(18.0))
                                    .ml_2()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_sm()
                                    .text_color(muted)
                                    .hover(|d| d.text_color(red))
                                    .when(tab_count > 1, |d| {
                                        d.on_click(cx.listener(
                                            move |this, _: &ClickEvent, _window, cx| {
                                                this.request_close_tab(i, cx);
                                            },
                                        ))
                                    })
                                    .child("×"),
                            )
                    }))
                    // New tab cell
                    .child(
                        div()
                            .id("new-tab-btn")
                            .h(px(38.0))
                            .w(px(38.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .text_color(muted)
                            .hover(|d| d.bg(tab_active_bg).text_color(foreground))
                            .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                                this.new_tab(cx);
                            }))
                            .child("+"),
                    )
                    // Spacer
                    .child(div().flex_1())
                    // Theme selector
                    .child(
                        div()
                            .h(px(38.0))
                            .flex()
                            .items_center()
                            .child(self.render_theme_selector(cx)),
                    )
                    // Settings button
                    .child(
                        div()
                            .h(px(38.0))
                            .flex()
                            .items_center()
                            .child(
                                Button::new("settings-btn")
                                    .icon(IconName::Settings)
                                    .small()
                                    .ghost()
                                    .tooltip("Settings (Cmd+,)")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.open_settings(window, cx);
                                    })),
                            ),
                    )
            )
            .child(
                // Pane content
                div()
                    .flex_1()
                    .w_full()
                    .h_full()
                    .overflow_hidden()
                    .children(self.tabs.get(self.active_tab).map(|tab| {
                        tab.panes.render(tab.active_pane, window, cx)
                    }))
            )
            // Confirmation dialog overlay
            .when(self.pending_action.is_some(), |d| {
                let action_text = match self.pending_action {
                    Some(PendingAction::ClosePane) => "close this pane",
                    Some(PendingAction::CloseTab(_)) => "close this tab",
                    Some(PendingAction::Quit) => "quit",
                    None => "",
                };
                let process_name = self.pending_process_name.clone().unwrap_or_else(|| "a process".to_string());

                let confirm_label = if self.pending_action == Some(PendingAction::Quit) { "Quit" } else { "Close" };
                d.child(
                    // Backdrop
                    div()
                        .id("confirm-backdrop")
                        .absolute()
                        .inset_0()
                        .bg(hsla(0.0, 0.0, 0.0, 0.6))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            // Modal container (clickable to cancel)
                            div()
                                .id("confirm-modal-container")
                                .size_full()
                                .absolute()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.cancel_pending_action(cx);
                                }))
                        )
                        .child(
                            // Modal
                            div()
                                .bg(hsla(0.0, 0.0, 0.12, 1.0))
                                .border_1()
                                .border_color(hsla(0.0, 0.0, 0.25, 1.0))
                                .rounded_xl()
                                .shadow_lg()
                                .p_6()
                                .w(px(420.0))
                                .flex()
                                .flex_col()
                                .gap_5()
                                .child(
                                    // Title
                                    div()
                                        .text_base()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(foreground)
                                        .child(format!("\"{}\" is running", process_name))
                                )
                                .child(
                                    // Message
                                    div()
                                        .text_sm()
                                        .text_color(muted)
                                        .line_height(px(20.0))
                                        .child(format!("Are you sure you want to {}? The running process will be terminated.", action_text))
                                )
                                .child(
                                    // Buttons
                                    div()
                                        .flex()
                                        .gap_3()
                                        .justify_end()
                                        .mt_2()
                                        .child(
                                            Button::new("cancel-btn")
                                                .label("Cancel")
                                                .ghost()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.cancel_pending_action(cx);
                                                }))
                                        )
                                        .child(
                                            Button::new("confirm-btn")
                                                .label(confirm_label)
                                                .danger()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.confirm_pending_action(cx);
                                                }))
                                        )
                                )
                        )
                )
            })
            // Dialog layer - must be rendered for dialogs to appear
            .children(Root::render_dialog_layer(window, cx))
    }
}
