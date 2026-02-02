//! Main workspace - container for tabs and split panes.

use super::pane::PaneKind;
use super::pane_group::{PaneNode, SplitDirection};
use crate::actions::{CloseTab, OpenSettings, Quit};
use crate::config::timing;
use crate::terminal::TerminalPane;
use crate::theme::{terminal_colors, SwitchTheme};
use gpui::prelude::FluentBuilder;
use gpui::{
    div, hsla, px, App, AppContext, ClickEvent, Context, ElementId, FontWeight, InteractiveElement,
    IntoElement, KeyDownEvent, ParentElement, Render, SharedString, StatefulInteractiveElement,
    Styled, Window,
};
use gpui_component::button::{Button, ButtonVariants};
use gpui_component::menu::DropdownMenu;
use gpui_component::theme::ThemeRegistry;
use gpui_component::{ActiveTheme, IconName, Root, Sizable};
use uuid::Uuid;

/// Pending action requiring confirmation
#[derive(Clone, Copy, Debug, PartialEq)]
enum PendingAction {
    ClosePane,
    CloseTab(usize),
    Quit,
}

/// A single tab in the workspace
struct Tab {
    id: Uuid,
    fallback_title: SharedString,
    panes: PaneNode,
    active_pane: Uuid,
}

impl Tab {
    /// Get the display title for this tab (dynamic from pane or fallback)
    fn display_title(&self, cx: &App) -> SharedString {
        // Try to get dynamic title from the active pane
        if let Some(pane) = self.panes.find_pane(self.active_pane) {
            if let Some(title) = pane.title(cx) {
                return title;
            }
        }
        self.fallback_title.clone()
    }
}

/// The main workspace view containing the tab bar and terminal panes.
pub struct Workspace {
    /// All open tabs, each containing a pane tree
    tabs: Vec<Tab>,
    /// Index of the currently active tab
    active_tab: usize,
    /// Pending action requiring confirmation (shows dialog)
    pending_action: Option<PendingAction>,
    /// Process name for confirmation dialog
    pending_process_name: Option<String>,
    /// Last time we checked for exited panes (debounce)
    last_cleanup: std::time::Instant,
    /// Cached tab titles to avoid recomputing every frame
    cached_titles: Vec<SharedString>,
    /// Last time we updated tab titles
    last_title_update: std::time::Instant,
    /// Last saved window bounds (for change detection)
    last_saved_bounds: Option<(f32, f32, f32, f32)>,
}

impl Workspace {
    /// Create a new workspace with a single tab containing one terminal pane.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let terminal = cx.new(TerminalPane::new);
        let panes = PaneNode::new_leaf(terminal.into());
        let active_pane = panes.first_leaf_id();

        let tab = Tab {
            id: Uuid::new_v4(),
            fallback_title: "Terminal 1".into(),
            panes,
            active_pane,
        };

        Self {
            tabs: vec![tab],
            active_tab: 0,
            pending_action: None,
            pending_process_name: None,
            last_cleanup: std::time::Instant::now(),
            cached_titles: vec!["Terminal 1".into()],
            last_title_update: std::time::Instant::now(),
            last_saved_bounds: None,
        }
    }

    /// Save window bounds if changed (called during render)
    fn maybe_save_window_bounds(&mut self, window: &Window) {
        let bounds = window.bounds();
        let current: (f32, f32, f32, f32) = (
            bounds.origin.x.into(),
            bounds.origin.y.into(),
            bounds.size.width.into(),
            bounds.size.height.into(),
        );

        // Only save if bounds changed
        if self.last_saved_bounds != Some(current) {
            self.last_saved_bounds = Some(current);
            crate::theme::save_window_bounds(crate::theme::WindowBoundsConfig {
                x: current.0,
                y: current.1,
                width: current.2,
                height: current.3,
            });
            tracing::trace!(
                x = current.0,
                y = current.1,
                w = current.2,
                h = current.3,
                "Saved window bounds"
            );
        }
    }

    /// Get tab titles, using cache if fresh enough
    fn get_tab_titles(&mut self, cx: &App) -> Vec<SharedString> {
        // Return cached if fresh and tab count matches
        if self.last_title_update.elapsed() < timing::TITLE_CACHE_TTL
            && self.cached_titles.len() == self.tabs.len()
        {
            return self.cached_titles.clone();
        }

        // Refresh cache
        self.cached_titles = self.tabs.iter().map(|t| t.display_title(cx)).collect();
        self.last_title_update = std::time::Instant::now();
        self.cached_titles.clone()
    }

    /// Check if a tab has any running child processes
    fn tab_has_running_processes(&self, index: usize, cx: &App) -> bool {
        if let Some(tab) = self.tabs.get(index) {
            for (_, pane) in tab.panes.all_panes() {
                if pane.has_running_processes(cx) {
                    return true;
                }
            }
        }
        false
    }

    /// Get the name of a running process in a tab (for display)
    fn get_tab_running_process_name(&self, index: usize, cx: &App) -> Option<String> {
        if let Some(tab) = self.tabs.get(index) {
            for (_, pane) in tab.panes.all_panes() {
                if let Some(name) = pane.get_running_process_name(cx) {
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
        let panes = PaneNode::new_leaf(terminal.into());
        let active_pane = panes.first_leaf_id();
        let tab_num = self.tabs.len() + 1;

        let tab = Tab {
            id: Uuid::new_v4(),
            fallback_title: format!("Terminal {}", tab_num).into(),
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
    fn split_pane(
        &mut self,
        direction: SplitDirection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            let new_terminal = cx.new(TerminalPane::new);
            let new_pane: PaneKind = new_terminal.clone().into();
            if let Some(new_pane_id) = tab.panes.split(tab.active_pane, direction, new_pane) {
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
            if let Some(pane) = tab.panes.find_pane(tab.active_pane) {
                if pane.has_running_processes(cx) {
                    // Show confirmation
                    self.pending_process_name = pane.get_running_process_name(cx);
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
            let pane_count = tab.panes.all_panes().len();

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
                    menu =
                        menu.menu_with_check(name.clone(), is_current, Box::new(SwitchTheme(name)));
                }

                menu
            })
    }
}

impl Workspace {
    /// Check for and clean up exited panes (debounced to avoid running every frame)
    fn cleanup_exited_panes(&mut self, cx: &mut Context<Self>) {
        // Debounce: only run cleanup every CLEANUP_INTERVAL
        if self.last_cleanup.elapsed() < timing::CLEANUP_INTERVAL {
            return;
        }
        self.last_cleanup = std::time::Instant::now();

        let mut tabs_to_remove: Vec<usize> = Vec::new();

        for (tab_idx, tab) in self.tabs.iter_mut().enumerate() {
            // Phase 1: Collect all exited pane IDs (avoid TOCTOU race)
            let panes = tab.panes.all_panes();
            let total_panes = panes.len();
            let exited_pane_ids: Vec<uuid::Uuid> = panes
                .iter()
                .filter(|(_, pane)| pane.has_exited(cx))
                .map(|(id, _)| *id)
                .collect();
            let exited_count = exited_pane_ids.len();

            // Phase 2: Remove all exited panes atomically
            for pane_id in exited_pane_ids {
                tab.panes.remove(pane_id);
            }

            // If all panes exited, mark tab for removal
            if exited_count >= total_panes {
                tabs_to_remove.push(tab_idx);
            } else if exited_count > 0 {
                // Some panes removed - make sure active pane is valid
                if tab.panes.find_pane(tab.active_pane).is_none() {
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

        // Save window bounds if changed
        self.maybe_save_window_bounds(window);

        // Focus the active pane
        if let Some(tab) = self.tabs.get(self.active_tab) {
            if let Some(pane) = tab.panes.find_pane(tab.active_pane) {
                let pane_focus = pane.focus_handle(cx);
                if !pane_focus.is_focused(window) {
                    window.focus(&pane_focus);
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
        let red = colors.red;

        let active_tab_idx = self.active_tab;
        let tab_count = self.tabs.len();

        // Pre-compute tab titles (dynamic from terminal or fallback)
        let tab_titles = self.get_tab_titles(cx);

        div()
            .size_full()
            .bg(background)
            .flex()
            .flex_col()
            .on_action(cx.listener(|_this, _: &OpenSettings, window, cx| {
                super::settings::toggle_settings_dialog(window, cx);
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
                        "d" | "D" => this.split_pane(SplitDirection::Vertical, window, cx),
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
                        "," => super::settings::toggle_settings_dialog(window, cx),
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
                    .children(self.tabs.iter().enumerate().zip(tab_titles).map(|((i, tab), title)| {
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
                                    .on_click(cx.listener(|_this, _, window, cx| {
                                        super::settings::toggle_settings_dialog(window, cx);
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
                        super::pane_group_view::render_pane_tree(&tab.panes, tab.active_pane, window, cx)
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    /// Initialize test context with required globals (theme, etc.)
    fn init_test_context(cx: &mut TestAppContext) {
        cx.update(|cx| {
            // Initialize gpui-component (sets up Theme global)
            gpui_component::init(cx);
        });
    }

    // ========================================================================
    // Workspace Creation Tests
    // ========================================================================

    #[gpui::test]
    fn test_workspace_creation(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        cx.read(|app| {
            let ws = workspace.read(app);
            // Should start with exactly one tab
            assert_eq!(ws.tabs.len(), 1, "Workspace should start with one tab");

            // Active tab should be 0
            assert_eq!(ws.active_tab, 0, "Active tab should be 0");

            // No pending action
            assert!(ws.pending_action.is_none(), "Should have no pending action");

            // First tab should have one pane
            let first_tab = &ws.tabs[0];
            assert_eq!(
                first_tab.panes.all_panes().len(),
                1,
                "First tab should have one pane"
            );

            // Tab title should be "Terminal 1"
            assert_eq!(
                first_tab.fallback_title.as_ref(),
                "Terminal 1",
                "First tab should be named 'Terminal 1'"
            );
        });
    }

    #[gpui::test]
    fn test_workspace_cached_titles_initialized(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        cx.read(|app| {
            let ws = workspace.read(app);
            // Cached titles should be initialized
            assert_eq!(
                ws.cached_titles.len(),
                1,
                "Cached titles should be initialized with one title"
            );
            assert_eq!(
                ws.cached_titles[0].as_ref(),
                "Terminal 1",
                "First cached title should be 'Terminal 1'"
            );
        });
    }

    // ========================================================================
    // Tab Management Tests
    // ========================================================================

    #[gpui::test]
    fn test_new_tab(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        workspace.update(cx, |ws, cx| {
            ws.new_tab(cx);
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            // Should now have 2 tabs
            assert_eq!(ws.tabs.len(), 2, "Should have 2 tabs after adding one");

            // Active tab should be the new one (index 1)
            assert_eq!(ws.active_tab, 1, "Active tab should be the new tab");

            // Second tab should have title "Terminal 2"
            assert_eq!(
                ws.tabs[1].fallback_title.as_ref(),
                "Terminal 2",
                "Second tab should be named 'Terminal 2'"
            );
        });
    }

    #[gpui::test]
    fn test_multiple_new_tabs(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        workspace.update(cx, |ws, cx| {
            ws.new_tab(cx);
            ws.new_tab(cx);
            ws.new_tab(cx);
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            // Should have 4 tabs total
            assert_eq!(ws.tabs.len(), 4, "Should have 4 tabs");

            // Active tab should be the last one (index 3)
            assert_eq!(ws.active_tab, 3, "Active tab should be index 3");

            // Check all tab titles
            assert_eq!(ws.tabs[0].fallback_title.as_ref(), "Terminal 1");
            assert_eq!(ws.tabs[1].fallback_title.as_ref(), "Terminal 2");
            assert_eq!(ws.tabs[2].fallback_title.as_ref(), "Terminal 3");
            assert_eq!(ws.tabs[3].fallback_title.as_ref(), "Terminal 4");
        });
    }

    #[gpui::test]
    fn test_switch_tab(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        // Create 3 tabs
        workspace.update(cx, |ws, cx| {
            ws.new_tab(cx);
            ws.new_tab(cx);
        });

        // Switch to first tab
        workspace.update(cx, |ws, cx| {
            ws.switch_tab(0, cx);
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert_eq!(ws.active_tab, 0, "Active tab should be 0 after switch");
        });

        // Switch to middle tab
        workspace.update(cx, |ws, cx| {
            ws.switch_tab(1, cx);
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert_eq!(ws.active_tab, 1, "Active tab should be 1 after switch");
        });
    }

    #[gpui::test]
    fn test_switch_tab_out_of_bounds(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        // Try to switch to an invalid tab index
        workspace.update(cx, |ws, cx| {
            ws.switch_tab(100, cx); // Out of bounds
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            // Should remain on tab 0
            assert_eq!(
                ws.active_tab, 0,
                "Active tab should remain 0 for invalid index"
            );
        });
    }

    #[gpui::test]
    fn test_next_tab(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        // Create 3 tabs, starting at tab 2 (index 2)
        workspace.update(cx, |ws, cx| {
            ws.new_tab(cx);
            ws.new_tab(cx);
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert_eq!(ws.active_tab, 2, "Should start at tab 2");
        });

        // Go to next tab (should wrap to 0)
        workspace.update(cx, |ws, cx| {
            ws.next_tab(cx);
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert_eq!(ws.active_tab, 0, "Should wrap around to tab 0");
        });

        // Go to next tab again
        workspace.update(cx, |ws, cx| {
            ws.next_tab(cx);
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert_eq!(ws.active_tab, 1, "Should be at tab 1");
        });
    }

    #[gpui::test]
    fn test_prev_tab(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        // Create 3 tabs
        workspace.update(cx, |ws, cx| {
            ws.new_tab(cx);
            ws.new_tab(cx);
            ws.switch_tab(0, cx); // Go to first tab
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert_eq!(ws.active_tab, 0, "Should start at tab 0");
        });

        // Go to prev tab (should wrap to last)
        workspace.update(cx, |ws, cx| {
            ws.prev_tab(cx);
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert_eq!(ws.active_tab, 2, "Should wrap around to last tab");
        });

        // Go to prev tab again
        workspace.update(cx, |ws, cx| {
            ws.prev_tab(cx);
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert_eq!(ws.active_tab, 1, "Should be at tab 1");
        });
    }

    #[gpui::test]
    fn test_close_tab_with_multiple_tabs(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        // Create 3 tabs
        workspace.update(cx, |ws, cx| {
            ws.new_tab(cx);
            ws.new_tab(cx);
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert_eq!(ws.tabs.len(), 3, "Should have 3 tabs");
        });

        // Close middle tab (index 1)
        workspace.update(cx, |ws, cx| {
            ws.close_tab(1, cx);
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert_eq!(ws.tabs.len(), 2, "Should have 2 tabs after closing one");
        });
    }

    #[gpui::test]
    fn test_close_active_tab_adjusts_index(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        // Create 3 tabs and stay on the last one
        workspace.update(cx, |ws, cx| {
            ws.new_tab(cx);
            ws.new_tab(cx);
            // Active tab is now 2
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert_eq!(ws.active_tab, 2, "Should be on tab 2");
        });

        // Close the last tab
        workspace.update(cx, |ws, cx| {
            ws.close_tab(2, cx);
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            // Active tab should adjust to the new last tab
            assert_eq!(ws.active_tab, 1, "Active tab should adjust to 1");
            assert_eq!(ws.tabs.len(), 2, "Should have 2 tabs");
        });
    }

    // ========================================================================
    // Focus Management Tests
    // ========================================================================

    #[gpui::test]
    fn test_set_active_pane(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        // Get the first pane's ID (workspace starts with one pane)
        let first_pane_id = cx.read(|app| {
            let ws = workspace.read(app);
            let tab = &ws.tabs[0];
            tab.panes.first_leaf_id()
        });

        // Set active pane (should work even with just one pane)
        workspace.update(cx, |ws, cx| {
            ws.set_active_pane(first_pane_id, cx);
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            let tab = &ws.tabs[0];
            assert_eq!(
                tab.active_pane, first_pane_id,
                "Active pane should be set to first pane"
            );
        });
    }

    // ========================================================================
    // Confirmation Dialog Tests
    // ========================================================================

    #[gpui::test]
    fn test_cancel_pending_action(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        // Set up a pending action manually
        workspace.update(cx, |ws, cx| {
            ws.pending_action = Some(PendingAction::Quit);
            ws.pending_process_name = Some("test_process".to_string());
            cx.notify();
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert!(ws.pending_action.is_some(), "Should have pending action");
            assert!(
                ws.pending_process_name.is_some(),
                "Should have process name"
            );
        });

        // Cancel the pending action
        workspace.update(cx, |ws, cx| {
            ws.cancel_pending_action(cx);
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert!(
                ws.pending_action.is_none(),
                "Pending action should be cleared"
            );
            assert!(
                ws.pending_process_name.is_none(),
                "Process name should be cleared"
            );
        });
    }

    #[gpui::test]
    fn test_pending_action_states(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        // Test ClosePane action
        workspace.update(cx, |ws, _| {
            ws.pending_action = Some(PendingAction::ClosePane);
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert_eq!(ws.pending_action, Some(PendingAction::ClosePane));
        });

        // Test CloseTab action
        workspace.update(cx, |ws, _| {
            ws.pending_action = Some(PendingAction::CloseTab(0));
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert_eq!(ws.pending_action, Some(PendingAction::CloseTab(0)));
        });

        // Test Quit action
        workspace.update(cx, |ws, _| {
            ws.pending_action = Some(PendingAction::Quit);
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert_eq!(ws.pending_action, Some(PendingAction::Quit));
        });
    }

    // ========================================================================
    // Tab Title Tests
    // ========================================================================

    #[gpui::test]
    fn test_get_tab_titles(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        // Add more tabs
        workspace.update(cx, |ws, cx| {
            ws.new_tab(cx);
            ws.new_tab(cx);
        });

        cx.update(|app| {
            workspace.update(app, |ws, cx| {
                let titles = ws.get_tab_titles(cx);
                assert_eq!(titles.len(), 3, "Should have 3 titles");
                assert_eq!(titles[0].as_ref(), "Terminal 1");
                assert_eq!(titles[1].as_ref(), "Terminal 2");
                assert_eq!(titles[2].as_ref(), "Terminal 3");
            });
        });
    }

    #[gpui::test]
    fn test_tab_titles_cached(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        // Get titles first time (populates cache)
        cx.update(|app| {
            workspace.update(app, |ws, cx| {
                let _ = ws.get_tab_titles(cx);
            });
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            // Cache should be populated
            assert_eq!(ws.cached_titles.len(), 1);
        });

        // Get titles again (should use cache)
        cx.update(|app| {
            workspace.update(app, |ws, cx| {
                let titles = ws.get_tab_titles(cx);
                assert_eq!(titles.len(), 1);
            });
        });
    }

    // ========================================================================
    // Edge Cases
    // ========================================================================

    #[gpui::test]
    fn test_single_tab_operations(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        // With single tab, next and prev should stay on same tab
        workspace.update(cx, |ws, cx| {
            ws.next_tab(cx);
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert_eq!(ws.active_tab, 0, "Should stay on tab 0 with single tab");
        });

        workspace.update(cx, |ws, cx| {
            ws.prev_tab(cx);
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert_eq!(ws.active_tab, 0, "Should stay on tab 0 with single tab");
        });
    }

    #[gpui::test]
    fn test_tab_ids_are_unique(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        workspace.update(cx, |ws, cx| {
            ws.new_tab(cx);
            ws.new_tab(cx);
            ws.new_tab(cx);
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            let ids: Vec<Uuid> = ws.tabs.iter().map(|t| t.id).collect();

            // Check all IDs are unique
            for i in 0..ids.len() {
                for j in (i + 1)..ids.len() {
                    assert_ne!(ids[i], ids[j], "Tab IDs should be unique");
                }
            }
        });
    }

    // ========================================================================
    // Pane Tree Structure Tests
    // ========================================================================

    #[gpui::test]
    fn test_pane_tree_first_leaf_id(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        // Verify first leaf ID is consistent
        let first_id = cx.read(|app| {
            let ws = workspace.read(app);
            ws.tabs[0].panes.first_leaf_id()
        });

        // Read again to verify it's the same
        cx.read(|app| {
            let ws = workspace.read(app);
            let second_read_id = ws.tabs[0].panes.first_leaf_id();
            assert_eq!(
                first_id, second_read_id,
                "First leaf ID should be consistent"
            );
        });
    }

    #[gpui::test]
    fn test_find_pane_by_id(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        // Get the first pane's ID
        let pane_id = cx.read(|app| {
            let ws = workspace.read(app);
            ws.tabs[0].panes.first_leaf_id()
        });

        // Should be able to find the pane
        cx.read(|app| {
            let ws = workspace.read(app);
            let found = ws.tabs[0].panes.find_pane(pane_id);
            assert!(found.is_some(), "Should find pane by ID");
        });

        // Should not find a random UUID
        cx.read(|app| {
            let ws = workspace.read(app);
            let random_id = Uuid::new_v4();
            let found = ws.tabs[0].panes.find_pane(random_id);
            assert!(found.is_none(), "Should not find random UUID");
        });
    }
}
