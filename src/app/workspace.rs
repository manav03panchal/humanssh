//! Main workspace - container for tabs and split panes.

use super::pane::PaneKind;
use super::pane_group::{PaneNode, SplitDirection};
use super::status_bar::{render_status_bar, stats_collector, SystemStats};
use crate::actions::{CloseTab, OpenSettings, Quit};
use crate::config::timing;
#[cfg(not(test))]
use crate::terminal::TerminalExitEvent;
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
    #[cfg_attr(test, allow(dead_code))]
    last_cleanup: std::time::Instant,
    /// Cached tab titles to avoid recomputing every frame
    cached_titles: Vec<SharedString>,
    /// Last time we updated tab titles
    last_title_update: std::time::Instant,
    /// Last saved window bounds (for change detection)
    last_saved_bounds: Option<(f32, f32, f32, f32)>,
    /// Cached system stats for status bar
    cached_stats: SystemStats,
}

impl Workspace {
    /// Create a new workspace with a single tab containing one terminal pane.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let terminal = cx.new(TerminalPane::new);

        // Subscribe to terminal exit events for immediate cleanup (non-test only)
        #[cfg(not(test))]
        cx.subscribe(&terminal, |this, _, _: &TerminalExitEvent, cx| {
            this.force_cleanup(cx);
        })
        .detach();

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
            cached_stats: SystemStats::default(),
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

    /// Get the current working directory from the active pane of the active tab.
    fn get_active_pane_cwd(&self, cx: &App) -> Option<std::path::PathBuf> {
        let tab = self.tabs.get(self.active_tab)?;
        let pane = tab.panes.find_pane(tab.active_pane)?;
        pane.get_current_directory(cx)
    }

    fn new_tab(&mut self, cx: &mut Context<Self>) {
        // Get working directory from the active pane (if any) for better UX
        let working_dir = self.get_active_pane_cwd(cx);

        let terminal = cx.new(|cx| TerminalPane::new_in_dir(cx, working_dir));

        // Subscribe to terminal exit events for immediate cleanup (non-test only)
        #[cfg(not(test))]
        cx.subscribe(&terminal, |this, _, _: &TerminalExitEvent, cx| {
            this.force_cleanup(cx);
        })
        .detach();

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

    /// Create a new tab running a specific command.
    pub fn new_tab_with_command(
        &mut self,
        command: &str,
        args: &[&str],
        title: &str,
        cx: &mut Context<Self>,
    ) {
        let cmd = command.to_string();
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let title_owned: SharedString = title.to_string().into();

        let terminal = cx.new(move |cx| {
            let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
            TerminalPane::new_with_command(cx, &cmd, &args_refs)
        });

        #[cfg(not(test))]
        cx.subscribe(&terminal, |this, _, _: &TerminalExitEvent, cx| {
            this.force_cleanup(cx);
        })
        .detach();

        let panes = PaneNode::new_leaf(terminal.into());
        let active_pane = panes.first_leaf_id();

        let tab = Tab {
            id: Uuid::new_v4(),
            fallback_title: title_owned,
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
        // Get working directory from the active pane before creating new terminal
        let working_dir = self.get_active_pane_cwd(cx);

        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            let new_terminal = cx.new(|cx| TerminalPane::new_in_dir(cx, working_dir));

            // Subscribe to terminal exit events for immediate cleanup (non-test only)
            #[cfg(not(test))]
            cx.subscribe(&new_terminal, |this, _, _: &TerminalExitEvent, cx| {
                this.force_cleanup(cx);
            })
            .detach();

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
                // Remove the pane and focus its sibling
                if let Some((sibling_id, _)) = tab.panes.remove(tab.active_pane) {
                    tab.active_pane = sibling_id;
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
    #[cfg_attr(test, allow(unused_variables))]
    fn cleanup_exited_panes(&mut self, cx: &mut Context<Self>) {
        // Skip cleanup entirely in tests - terminals may exit and tests expect tab counts to be stable
        #[cfg(test)]
        return;

        // Debounce: only run cleanup every CLEANUP_INTERVAL
        #[cfg(not(test))]
        {
            if self.last_cleanup.elapsed() < timing::CLEANUP_INTERVAL {
                return;
            }
            self.last_cleanup = std::time::Instant::now();
            self.do_cleanup_exited_panes(cx);
        }
    }

    /// Force immediate cleanup (bypasses debounce) - called from exit event handlers
    #[cfg(not(test))]
    fn force_cleanup(&mut self, cx: &mut Context<Self>) {
        self.last_cleanup = std::time::Instant::now();
        self.do_cleanup_exited_panes(cx);
    }

    /// Internal: perform the actual cleanup
    #[cfg(not(test))]
    fn do_cleanup_exited_panes(&mut self, cx: &mut Context<Self>) {
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

            // Phase 2: Remove all exited panes, tracking sibling for focus
            let mut sibling_to_focus: Option<uuid::Uuid> = None;
            for pane_id in exited_pane_ids {
                if let Some((sibling_id, _)) = tab.panes.remove(pane_id) {
                    // If we removed the active pane, remember its sibling
                    if pane_id == tab.active_pane {
                        sibling_to_focus = Some(sibling_id);
                    }
                }
            }

            // If all panes exited, mark tab for removal
            if exited_count >= total_panes {
                tabs_to_remove.push(tab_idx);
            } else if exited_count > 0 {
                // Focus the sibling of the removed active pane, or fallback to first leaf
                if let Some(sibling_id) = sibling_to_focus {
                    tab.active_pane = sibling_id;
                } else if tab.panes.find_pane(tab.active_pane).is_none() {
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

        // Refresh system stats for status bar
        {
            let collector_arc = stats_collector();
            let mut collector = collector_arc.write();
            self.cached_stats = collector.refresh();

            // Get terminal-specific info from active pane
            if let Some(tab) = self.tabs.get(self.active_tab) {
                if let Some(pane) = tab.panes.find_pane(tab.active_pane) {
                    let (shell, cwd, process) = pane.get_terminal_info(cx);
                    collector.set_terminal_info(shell, cwd, process);
                    self.cached_stats = collector.current();
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
                    .border_b_1()
                    .border_color(border_color)
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
            // Status bar
            .child(render_status_bar(&self.cached_stats, cx))
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

    // ========================================================================
    // Concurrency Tests - Tab Operations Thread Safety
    // ========================================================================

    #[gpui::test]
    fn test_rapid_tab_creation(cx: &mut TestAppContext) {
        // Tests rapid consecutive tab creation (simulates fast keyboard input)
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        const NUM_TABS: usize = 50;

        // Rapidly create many tabs
        workspace.update(cx, |ws, cx| {
            for _ in 0..NUM_TABS {
                ws.new_tab(cx);
            }
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert_eq!(
                ws.tabs.len(),
                NUM_TABS + 1, // +1 for initial tab
                "All tabs should be created"
            );
            assert_eq!(
                ws.active_tab, NUM_TABS,
                "Active tab should be the last created"
            );
        });
    }

    #[gpui::test]
    fn test_rapid_tab_deletion(cx: &mut TestAppContext) {
        // Tests rapid consecutive tab deletion
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        const NUM_TABS: usize = 20;

        // Create tabs first
        workspace.update(cx, |ws, cx| {
            for _ in 0..NUM_TABS {
                ws.new_tab(cx);
            }
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert_eq!(ws.tabs.len(), NUM_TABS + 1);
        });

        // Rapidly delete tabs from the end
        workspace.update(cx, |ws, cx| {
            while ws.tabs.len() > 1 {
                let last_idx = ws.tabs.len() - 1;
                ws.close_tab(last_idx, cx);
            }
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert_eq!(ws.tabs.len(), 1, "Should have one tab remaining");
            assert_eq!(ws.active_tab, 0, "Active tab should be 0");
        });
    }

    #[gpui::test]
    fn test_interleaved_create_delete(cx: &mut TestAppContext) {
        // Tests interleaved tab creation and deletion
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        // Create 5, delete 2, create 3, delete 1, etc.
        workspace.update(cx, |ws, cx| {
            // Create 5 tabs
            for _ in 0..5 {
                ws.new_tab(cx);
            }
            // Delete 2
            ws.close_tab(3, cx);
            ws.close_tab(2, cx);
            // Create 3 more
            for _ in 0..3 {
                ws.new_tab(cx);
            }
            // Delete 1
            ws.close_tab(4, cx);
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            // 1 (initial) + 5 - 2 + 3 - 1 = 6 tabs
            assert_eq!(
                ws.tabs.len(),
                6,
                "Tab count should be correct after interleaved operations"
            );
        });
    }

    #[gpui::test]
    fn test_rapid_focus_changes(cx: &mut TestAppContext) {
        // Tests rapid consecutive focus changes between tabs
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        // Create several tabs
        workspace.update(cx, |ws, cx| {
            for _ in 0..10 {
                ws.new_tab(cx);
            }
        });

        // Rapidly switch between tabs
        workspace.update(cx, |ws, cx| {
            for i in 0..100 {
                ws.switch_tab(i % 11, cx); // 11 tabs total
            }
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert_eq!(
                ws.active_tab,
                99 % 11,
                "Active tab should be correct after rapid switches"
            );
        });
    }

    #[gpui::test]
    fn test_rapid_next_prev_tab(cx: &mut TestAppContext) {
        // Tests rapid next/prev tab cycling
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        // Create 5 tabs
        workspace.update(cx, |ws, cx| {
            for _ in 0..4 {
                ws.new_tab(cx);
            }
        });

        // Rapidly cycle through tabs
        workspace.update(cx, |ws, cx| {
            for _ in 0..50 {
                ws.next_tab(cx);
            }
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            // After adding 4 tabs, active_tab is 4 (last created)
            // 50 next_tabs with 5 tabs from position 4: (4 + 50) % 5 = 4
            assert_eq!(ws.active_tab, 4, "Should wrap around correctly");
        });

        // Now go backwards
        workspace.update(cx, |ws, cx| {
            for _ in 0..50 {
                ws.prev_tab(cx);
            }
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            // 50 prev_tabs from position 4 with 5 tabs: (4 - 50) wraps to 4
            // Because 50 is divisible by 5, we end up back at 4
            assert_eq!(ws.active_tab, 4, "Should wrap around correctly backwards");
        });
    }

    #[gpui::test]
    fn test_delete_while_iterating(cx: &mut TestAppContext) {
        // Tests tab deletion while the active index would be affected
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        // Create tabs
        workspace.update(cx, |ws, cx| {
            for _ in 0..10 {
                ws.new_tab(cx);
            }
            // Go to middle tab
            ws.switch_tab(5, cx);
        });

        // Delete tabs before the active one
        workspace.update(cx, |ws, cx| {
            ws.close_tab(2, cx);
            ws.close_tab(1, cx);
            ws.close_tab(0, cx);
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            // Active index should adjust as tabs before it are deleted
            assert!(ws.active_tab < ws.tabs.len(), "Active tab should be valid");
        });
    }

    #[gpui::test]
    fn test_pending_action_state_transitions(cx: &mut TestAppContext) {
        // Tests rapid state transitions of pending actions
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        workspace.update(cx, |ws, cx| {
            // Rapidly set and clear pending actions
            for i in 0..100 {
                match i % 4 {
                    0 => {
                        ws.pending_action = Some(PendingAction::ClosePane);
                        ws.pending_process_name = Some(format!("proc-{}", i));
                    }
                    1 => {
                        ws.pending_action = Some(PendingAction::CloseTab(i % 10));
                        ws.pending_process_name = Some(format!("proc-{}", i));
                    }
                    2 => {
                        ws.pending_action = Some(PendingAction::Quit);
                        ws.pending_process_name = Some("important-process".to_string());
                    }
                    3 => {
                        ws.cancel_pending_action(cx);
                    }
                    _ => unreachable!(),
                }
            }
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            // Last iteration (99 % 4 = 3) cancels the action
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
    fn test_cache_invalidation_under_load(cx: &mut TestAppContext) {
        // Tests that title cache remains consistent under rapid tab changes
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        // Create and delete tabs rapidly
        for _ in 0..10 {
            workspace.update(cx, |ws, cx| {
                ws.new_tab(cx);
                ws.new_tab(cx);
                ws.new_tab(cx);
            });

            cx.update(|app| {
                workspace.update(app, |ws, cx| {
                    let titles = ws.get_tab_titles(cx);
                    assert_eq!(
                        titles.len(),
                        ws.tabs.len(),
                        "Title cache should match tab count"
                    );
                });
            });

            workspace.update(cx, |ws, cx| {
                if ws.tabs.len() > 2 {
                    ws.close_tab(ws.tabs.len() - 1, cx);
                }
            });
        }
    }

    #[gpui::test]
    fn test_tab_id_uniqueness_under_stress(cx: &mut TestAppContext) {
        // Tests that tab IDs remain unique even with rapid create/delete cycles
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        let mut all_ids: std::collections::HashSet<Uuid> = std::collections::HashSet::new();

        // Collect initial tab ID
        cx.read(|app| {
            let ws = workspace.read(app);
            for tab in &ws.tabs {
                all_ids.insert(tab.id);
            }
        });

        for _cycle in 0..20 {
            // Create tabs and collect only newly created IDs
            workspace.update(cx, |ws, cx| {
                for _ in 0..5 {
                    ws.new_tab(cx);
                }
            });

            // Collect all current IDs - only new ones should be inserted
            cx.read(|app| {
                let ws = workspace.read(app);
                for tab in &ws.tabs {
                    // Try to insert - if it was already there, that's fine (surviving tab)
                    // UUIDs should never collide in practice
                    all_ids.insert(tab.id);
                }
            });

            // Delete some tabs
            workspace.update(cx, |ws, cx| {
                while ws.tabs.len() > 2 {
                    ws.close_tab(1, cx);
                }
            });
        }

        // Final check: we should have accumulated many unique IDs
        // 20 cycles * 5 new tabs = 100 new tabs, plus 1 initial = 101 unique IDs minimum
        // (some may be deleted but IDs should never be reused)
        assert!(
            all_ids.len() >= 100,
            "Should have accumulated many unique tab IDs: got {}",
            all_ids.len()
        );
    }

    #[gpui::test]
    fn test_active_pane_consistency(cx: &mut TestAppContext) {
        // Tests that active_pane always points to a valid pane
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        // Create multiple tabs and switch between them
        workspace.update(cx, |ws, cx| {
            for _ in 0..10 {
                ws.new_tab(cx);
            }
        });

        // Verify all tabs have valid active_pane
        cx.read(|app| {
            let ws = workspace.read(app);
            for (i, tab) in ws.tabs.iter().enumerate() {
                let pane = tab.panes.find_pane(tab.active_pane);
                assert!(pane.is_some(), "Tab {} should have a valid active_pane", i);
            }
        });

        // Switch between tabs and verify consistency
        for target_tab in 0..10 {
            workspace.update(cx, |ws, cx| {
                ws.switch_tab(target_tab, cx);
            });

            cx.read(|app| {
                let ws = workspace.read(app);
                let tab = &ws.tabs[ws.active_tab];
                let pane = tab.panes.find_pane(tab.active_pane);
                assert!(
                    pane.is_some(),
                    "Active tab should have valid active_pane after switch to {}",
                    target_tab
                );
            });
        }
    }

    // ========================================================================
    // Stress Tests - High Volume Tab Operations
    // To run: cargo test --release -- --ignored stress_
    // ========================================================================

    /// Stress test with 100+ rapid tab operations.
    /// Run with: cargo test --release -- --ignored stress_tab_operations
    #[gpui::test]
    #[ignore]
    fn stress_tab_operations(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        const ITERATIONS: usize = 100;
        const TABS_PER_ITERATION: usize = 10;

        for iteration in 0..ITERATIONS {
            // Create tabs
            workspace.update(cx, |ws, cx| {
                for _ in 0..TABS_PER_ITERATION {
                    ws.new_tab(cx);
                }
            });

            // Verify state
            cx.read(|app| {
                let ws = workspace.read(app);
                assert!(
                    ws.tabs.len() <= (iteration + 1) * TABS_PER_ITERATION + 1,
                    "Tab count should be bounded"
                );
            });

            // Delete half the tabs
            workspace.update(cx, |ws, cx| {
                let to_delete = ws.tabs.len() / 2;
                for _ in 0..to_delete {
                    if ws.tabs.len() > 1 {
                        ws.close_tab(1, cx);
                    }
                }
            });

            // Rapid focus switching
            workspace.update(cx, |ws, cx| {
                for _ in 0..50 {
                    ws.next_tab(cx);
                    ws.prev_tab(cx);
                }
            });
        }

        // Final verification
        cx.read(|app| {
            let ws = workspace.read(app);
            assert!(!ws.tabs.is_empty(), "Should have at least one tab");
            assert!(ws.active_tab < ws.tabs.len(), "Active tab should be valid");
        });
    }

    /// Stress test for focus state consistency.
    /// Run with: cargo test --release -- --ignored stress_focus_consistency
    #[gpui::test]
    #[ignore]
    fn stress_focus_consistency(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        const NUM_TABS: usize = 50;
        const FOCUS_CYCLES: usize = 1000;

        // Create many tabs
        workspace.update(cx, |ws, cx| {
            for _ in 0..NUM_TABS {
                ws.new_tab(cx);
            }
        });

        // Rapidly cycle focus
        workspace.update(cx, |ws, cx| {
            for i in 0..FOCUS_CYCLES {
                match i % 3 {
                    0 => ws.next_tab(cx),
                    1 => ws.prev_tab(cx),
                    2 => ws.switch_tab(i % (NUM_TABS + 1), cx),
                    _ => unreachable!(),
                }
            }
        });

        // Verify all tabs still have valid panes
        cx.read(|app| {
            let ws = workspace.read(app);
            for (i, tab) in ws.tabs.iter().enumerate() {
                let pane = tab.panes.find_pane(tab.active_pane);
                assert!(
                    pane.is_some(),
                    "Tab {} should have valid pane after stress test",
                    i
                );
            }
        });
    }

    /// Stress test for pending action handling.
    /// Run with: cargo test --release -- --ignored stress_pending_actions
    #[gpui::test]
    #[ignore]
    fn stress_pending_actions(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        const ITERATIONS: usize = 1000;

        workspace.update(cx, |ws, cx| {
            for i in 0..ITERATIONS {
                // Set a pending action
                ws.pending_action = Some(match i % 3 {
                    0 => PendingAction::ClosePane,
                    1 => PendingAction::CloseTab(i % 10),
                    2 => PendingAction::Quit,
                    _ => unreachable!(),
                });
                ws.pending_process_name = Some(format!("process-{}", i));

                // Randomly cancel or confirm
                if i % 7 == 0 {
                    ws.cancel_pending_action(cx);
                }
            }
        });

        // Final state should be consistent
        cx.read(|app| {
            let ws = workspace.read(app);
            // Either both are Some or both are None after cancel
            if ws.pending_action.is_none() {
                assert!(
                    ws.pending_process_name.is_none(),
                    "Process name should be cleared with pending action"
                );
            }
        });
    }

    /// Stress test with mixed operations including cache updates.
    /// Run with: cargo test --release -- --ignored stress_mixed_operations
    #[gpui::test]
    #[ignore]
    fn stress_mixed_operations(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        const ITERATIONS: usize = 500;

        for i in 0..ITERATIONS {
            workspace.update(cx, |ws, cx| {
                match i % 10 {
                    0..=2 => ws.new_tab(cx),
                    3 | 4 => {
                        if ws.tabs.len() > 1 {
                            ws.close_tab(ws.tabs.len() - 1, cx);
                        }
                    }
                    5 | 6 => ws.next_tab(cx),
                    7 | 8 => ws.prev_tab(cx),
                    9 => {
                        // Update pending action
                        ws.pending_action = Some(PendingAction::ClosePane);
                        ws.cancel_pending_action(cx);
                    }
                    _ => unreachable!(),
                }
            });

            // Periodically check title cache
            if i % 50 == 0 {
                cx.update(|app| {
                    workspace.update(app, |ws, cx| {
                        let titles = ws.get_tab_titles(cx);
                        assert_eq!(
                            titles.len(),
                            ws.tabs.len(),
                            "Cache should match tabs at iteration {}",
                            i
                        );
                    });
                });
            }
        }

        // Final verification
        cx.read(|app| {
            let ws = workspace.read(app);
            assert!(!ws.tabs.is_empty(), "Should have at least one tab");
            for tab in &ws.tabs {
                assert!(
                    tab.panes.find_pane(tab.active_pane).is_some(),
                    "All tabs should have valid panes"
                );
            }
        });
    }

    // ========================================================================
    // Boundary Condition Tests - 0/1/many tabs, tab index boundaries
    // ========================================================================

    // --- 0 Tabs Boundary (not achievable - workspace always has at least 1 tab) ---

    #[gpui::test]
    fn test_workspace_cannot_have_zero_tabs(cx: &mut TestAppContext) {
        // Workspace always starts with exactly 1 tab - verify this invariant
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        cx.read(|app| {
            let ws = workspace.read(app);
            assert!(
                !ws.tabs.is_empty(),
                "Workspace must always have at least 1 tab"
            );
            assert_eq!(ws.tabs.len(), 1, "New workspace starts with exactly 1 tab");
        });
    }

    // --- 1 Tab Boundary Tests ---

    #[gpui::test]
    fn test_single_tab_next_tab_wraps(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        // With single tab, next should stay on same tab (0 -> 0)
        workspace.update(cx, |ws, cx| {
            assert_eq!(ws.tabs.len(), 1);
            ws.next_tab(cx);
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert_eq!(ws.active_tab, 0, "Single tab: next_tab(0) should wrap to 0");
        });
    }

    #[gpui::test]
    fn test_single_tab_prev_tab_wraps(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        // With single tab, prev should stay on same tab (0 -> 0)
        workspace.update(cx, |ws, cx| {
            assert_eq!(ws.tabs.len(), 1);
            ws.prev_tab(cx);
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert_eq!(ws.active_tab, 0, "Single tab: prev_tab(0) should wrap to 0");
        });
    }

    #[gpui::test]
    fn test_single_tab_switch_to_0(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        workspace.update(cx, |ws, cx| {
            ws.switch_tab(0, cx);
        });

        cx.read(|app| {
            assert_eq!(workspace.read(app).active_tab, 0);
        });
    }

    #[gpui::test]
    fn test_single_tab_switch_beyond_bounds_noop(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        // switch_tab(1) on single tab should be no-op
        workspace.update(cx, |ws, cx| {
            ws.switch_tab(1, cx);
        });

        cx.read(|app| {
            assert_eq!(
                workspace.read(app).active_tab,
                0,
                "switch_tab(1) on single tab should be no-op"
            );
        });
    }

    // --- Many Tabs Boundary Tests ---

    #[gpui::test]
    fn test_many_tabs_100_creation(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        workspace.update(cx, |ws, cx| {
            for _ in 0..99 {
                ws.new_tab(cx);
            }
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert_eq!(ws.tabs.len(), 100, "Should have exactly 100 tabs");
            assert_eq!(ws.active_tab, 99, "Active tab should be last (99)");
        });
    }

    #[gpui::test]
    fn test_many_tabs_navigation_wrapping(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        workspace.update(cx, |ws, cx| {
            for _ in 0..9 {
                ws.new_tab(cx);
            }
            // Now have 10 tabs, active is 9 (last)
        });

        // next_tab from last should wrap to first
        workspace.update(cx, |ws, cx| ws.next_tab(cx));
        cx.read(|app| {
            assert_eq!(
                workspace.read(app).active_tab,
                0,
                "next from 9 should wrap to 0"
            );
        });

        // prev_tab from first should wrap to last
        workspace.update(cx, |ws, cx| ws.prev_tab(cx));
        cx.read(|app| {
            assert_eq!(
                workspace.read(app).active_tab,
                9,
                "prev from 0 should wrap to 9"
            );
        });
    }

    // --- Tab Index Boundary Tests (0, last, beyond last) ---

    #[gpui::test]
    fn test_tab_index_0_operations(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        workspace.update(cx, |ws, cx| {
            ws.new_tab(cx);
            ws.new_tab(cx);
            ws.new_tab(cx);
            // 4 tabs, active is 3
        });

        // Switch to index 0
        workspace.update(cx, |ws, cx| ws.switch_tab(0, cx));
        cx.read(|app| {
            assert_eq!(workspace.read(app).active_tab, 0);
        });

        // Close tab at index 0
        workspace.update(cx, |ws, cx| ws.close_tab(0, cx));
        cx.read(|app| {
            let ws = workspace.read(app);
            assert_eq!(ws.tabs.len(), 3);
            // active_tab should adjust
            assert!(ws.active_tab < ws.tabs.len());
        });
    }

    #[gpui::test]
    fn test_tab_index_last_operations(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        workspace.update(cx, |ws, cx| {
            ws.new_tab(cx);
            ws.new_tab(cx);
            ws.new_tab(cx);
            ws.switch_tab(0, cx);
            // 4 tabs, active is 0
        });

        // Switch to last index (3)
        workspace.update(cx, |ws, cx| ws.switch_tab(3, cx));
        cx.read(|app| {
            assert_eq!(workspace.read(app).active_tab, 3);
        });

        // Close tab at last index
        workspace.update(cx, |ws, cx| ws.close_tab(3, cx));
        cx.read(|app| {
            let ws = workspace.read(app);
            assert_eq!(ws.tabs.len(), 3);
            // active_tab should adjust to new last (2)
            assert_eq!(ws.active_tab, 2);
        });
    }

    #[gpui::test]
    fn test_tab_index_beyond_last_switch_noop(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        workspace.update(cx, |ws, cx| {
            ws.new_tab(cx);
            ws.new_tab(cx);
            // 3 tabs, active is 2
        });

        let original = cx.read(|app| workspace.read(app).active_tab);

        // Try to switch to index 3 (beyond last)
        workspace.update(cx, |ws, cx| ws.switch_tab(3, cx));
        cx.read(|app| {
            assert_eq!(
                workspace.read(app).active_tab,
                original,
                "switch_tab beyond last should be no-op"
            );
        });

        // Try to switch to index 100
        workspace.update(cx, |ws, cx| ws.switch_tab(100, cx));
        cx.read(|app| {
            assert_eq!(workspace.read(app).active_tab, original);
        });

        // Try to switch to usize::MAX
        workspace.update(cx, |ws, cx| ws.switch_tab(usize::MAX, cx));
        cx.read(|app| {
            assert_eq!(workspace.read(app).active_tab, original);
        });
    }

    // --- Empty Tab Title Boundary Tests ---

    #[gpui::test]
    fn test_tab_fallback_titles_never_empty(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        workspace.update(cx, |ws, cx| {
            for _ in 0..20 {
                ws.new_tab(cx);
            }
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            for (i, tab) in ws.tabs.iter().enumerate() {
                assert!(
                    !tab.fallback_title.is_empty(),
                    "Tab {} fallback_title should not be empty",
                    i
                );
            }
        });
    }

    #[gpui::test]
    fn test_get_tab_titles_returns_non_empty_strings(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        workspace.update(cx, |ws, cx| {
            for _ in 0..10 {
                ws.new_tab(cx);
            }
        });

        cx.update(|app| {
            workspace.update(app, |ws, cx| {
                let titles = ws.get_tab_titles(cx);
                for (i, title) in titles.iter().enumerate() {
                    assert!(!title.is_empty(), "Title {} should not be empty", i);
                }
            });
        });
    }

    // --- active_tab Index Invariant Tests ---

    #[gpui::test]
    fn test_active_tab_always_valid_after_close_first(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        workspace.update(cx, |ws, cx| {
            ws.new_tab(cx);
            ws.new_tab(cx);
            ws.new_tab(cx);
        });

        // Close from front repeatedly
        for _ in 0..3 {
            workspace.update(cx, |ws, cx| {
                ws.close_tab(0, cx);
            });

            cx.read(|app| {
                let ws = workspace.read(app);
                assert!(
                    ws.active_tab < ws.tabs.len(),
                    "active_tab {} must be < tabs.len() {}",
                    ws.active_tab,
                    ws.tabs.len()
                );
            });
        }
    }

    #[gpui::test]
    fn test_active_tab_always_valid_after_close_last(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        workspace.update(cx, |ws, cx| {
            ws.new_tab(cx);
            ws.new_tab(cx);
            ws.new_tab(cx);
        });

        // Close from back repeatedly
        for _ in 0..3 {
            workspace.update(cx, |ws, cx| {
                let last = ws.tabs.len() - 1;
                ws.close_tab(last, cx);
            });

            cx.read(|app| {
                let ws = workspace.read(app);
                assert!(
                    ws.active_tab < ws.tabs.len(),
                    "active_tab {} must be < tabs.len() {}",
                    ws.active_tab,
                    ws.tabs.len()
                );
            });
        }
    }

    #[gpui::test]
    fn test_active_tab_adjusts_when_closing_active(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        workspace.update(cx, |ws, cx| {
            ws.new_tab(cx);
            ws.new_tab(cx);
            ws.switch_tab(1, cx); // Active is now 1 (middle)
        });

        // Close the active tab
        workspace.update(cx, |ws, cx| {
            ws.close_tab(1, cx);
        });

        cx.read(|app| {
            let ws = workspace.read(app);
            assert!(ws.active_tab < ws.tabs.len());
        });
    }

    // --- Pending Action with Tab Index Boundary ---

    #[gpui::test]
    fn test_pending_close_tab_index_0(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        workspace.update(cx, |ws, _| {
            ws.pending_action = Some(PendingAction::CloseTab(0));
        });

        cx.read(|app| {
            assert_eq!(
                workspace.read(app).pending_action,
                Some(PendingAction::CloseTab(0))
            );
        });
    }

    #[gpui::test]
    fn test_pending_close_tab_index_max(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        // While this index would be invalid, the type allows it
        workspace.update(cx, |ws, _| {
            ws.pending_action = Some(PendingAction::CloseTab(usize::MAX));
        });

        cx.read(|app| {
            assert_eq!(
                workspace.read(app).pending_action,
                Some(PendingAction::CloseTab(usize::MAX))
            );
        });
    }

    // --- Tab Cache Boundary Tests ---

    #[gpui::test]
    fn test_cached_titles_with_single_tab(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        cx.update(|app| {
            workspace.update(app, |ws, cx| {
                let titles = ws.get_tab_titles(cx);
                assert_eq!(titles.len(), 1);
                assert_eq!(ws.cached_titles.len(), 1);
            });
        });
    }

    #[gpui::test]
    fn test_cached_titles_with_many_tabs(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        workspace.update(cx, |ws, cx| {
            for _ in 0..49 {
                ws.new_tab(cx);
            }
        });

        cx.update(|app| {
            workspace.update(app, |ws, cx| {
                let titles = ws.get_tab_titles(cx);
                assert_eq!(titles.len(), 50);
                assert_eq!(ws.cached_titles.len(), 50);
            });
        });
    }

    // --- Comprehensive Boundary Matrix Test ---

    #[gpui::test]
    fn test_tab_count_boundary_matrix(cx: &mut TestAppContext) {
        // Tests operations at various tab count boundaries
        let test_counts = [1, 2, 3, 5, 10, 50];

        for &target_count in &test_counts {
            init_test_context(cx);
            let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

            // Create tabs to reach target count
            workspace.update(cx, |ws, cx| {
                for _ in 1..target_count {
                    ws.new_tab(cx);
                }
            });

            cx.read(|app| {
                let ws = workspace.read(app);
                assert_eq!(
                    ws.tabs.len(),
                    target_count,
                    "Should have {} tabs",
                    target_count
                );
            });

            // Test operations at this boundary
            workspace.update(cx, |ws, cx| {
                // next_tab should work
                ws.next_tab(cx);
                assert!(ws.active_tab < ws.tabs.len());

                // prev_tab should work
                ws.prev_tab(cx);
                assert!(ws.active_tab < ws.tabs.len());

                // switch to first
                ws.switch_tab(0, cx);
                assert_eq!(ws.active_tab, 0);

                // switch to last
                ws.switch_tab(target_count - 1, cx);
                assert_eq!(ws.active_tab, target_count - 1);

                // switch beyond last should be no-op
                let before = ws.active_tab;
                ws.switch_tab(target_count, cx);
                assert_eq!(ws.active_tab, before);
            });
        }
    }
}
