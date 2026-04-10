use super::*;

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        self.cleanup_exited_panes(cx);

        self.maybe_save_window_bounds(window);

        let scratchpad_focused = self
            .scratchpad
            .as_ref()
            .is_some_and(|sp| sp.visible && sp.input.read(cx).focus_handle(cx).is_focused(window));
        let command_palette_focused = self.command_palette.is_some();

        if !scratchpad_focused && !command_palette_focused {
            if let Some(tab) = self.tabs.get(self.active_tab) {
                if let Some(pane) = tab.panes.find_pane(tab.active_pane) {
                    let pane_focus = pane.focus_handle(cx);
                    if !pane_focus.is_focused(window) {
                        window.focus(&pane_focus);
                    }
                }
            }
        }

        {
            let collector_arc = crate::status_bar::stats_collector();
            let mut collector = collector_arc.write();
            self.cached_stats = collector.refresh();

            if let Some(tab) = self.tabs.get(self.active_tab) {
                if let Some(pane) = tab.panes.find_pane(tab.active_pane) {
                    let (shell, cwd, process) = pane.get_terminal_info(cx);
                    collector.set_terminal_info(shell, cwd, process);
                    self.cached_stats = collector.current();
                }
            }
        }

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

        let tab_titles = self.get_tab_titles(cx);

        div()
            .size_full()
            .bg(background)
            .flex()
            .flex_col()
            .on_action(cx.listener(|_this, _: &OpenSettings, _window, _cx| {
                crate::settings_opener::open_config_file();
            }))
            .on_action(cx.listener(|this, _: &Quit, _window, cx| {
                this.request_quit(cx);
            }))
            .on_action(cx.listener(|this, _: &CloseTab, _window, cx| {
                if this.has_active_overlay() { return; }
                this.request_close_pane(cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleScratchpad, window, cx| {
                this.toggle_scratchpad(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleCommandPalette, window, cx| {
                this.toggle_command_palette(window, cx);
            }))
            .on_action(cx.listener(|this, _: &NewTab, _window, cx| {
                if this.has_active_overlay() { return; }
                this.new_tab(cx);
            }))
            .on_action(cx.listener(|this, _: &OpenReplay, _window, cx| {
                if this.has_active_overlay() { return; }
                this.open_replay(cx);
            }))
            .on_action(cx.listener(|this, _: &NextTab, _window, cx| {
                if this.has_active_overlay() { return; }
                this.next_tab(cx);
            }))
            .on_action(cx.listener(|this, _: &PrevTab, _window, cx| {
                if this.has_active_overlay() { return; }
                this.prev_tab(cx);
            }))
            .on_action(cx.listener(|this, _: &SplitVertical, window, cx| {
                if this.has_active_overlay() { return; }
                this.split_pane(SplitDirection::Horizontal, window, cx);
            }))
            .on_action(cx.listener(|this, _: &SplitHorizontal, window, cx| {
                if this.has_active_overlay() { return; }
                this.split_pane(SplitDirection::Vertical, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ClosePane, _window, cx| {
                if this.has_active_overlay() { return; }
                this.request_close_pane(cx);
            }))
            .on_action(cx.listener(|this, _: &FocusNextPane, window, cx| {
                if this.has_active_overlay() { return; }
                if let Some(tab) = this.tabs.get(this.active_tab) {
                    let panes = tab.panes.all_panes();
                    if let Some(pos) = panes.iter().position(|(id, _)| *id == tab.active_pane) {
                        let next = (pos + 1) % panes.len();
                        let (next_id, next_pane) = &panes[next];
                        let focus = next_pane.focus_handle(cx);
                        this.set_active_pane(*next_id, cx);
                        window.focus(&focus);
                    }
                }
            }))
            .on_action(cx.listener(|this, _: &FocusPrevPane, window, cx| {
                if this.has_active_overlay() { return; }
                if let Some(tab) = this.tabs.get(this.active_tab) {
                    let panes = tab.panes.all_panes();
                    if let Some(pos) = panes.iter().position(|(id, _)| *id == tab.active_pane) {
                        let prev = if pos == 0 { panes.len() - 1 } else { pos - 1 };
                        let (prev_id, prev_pane) = &panes[prev];
                        let focus = prev_pane.focus_handle(cx);
                        this.set_active_pane(*prev_id, cx);
                        window.focus(&focus);
                    }
                }
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                let key = event.keystroke.key.as_str();
                let cmd = event.keystroke.modifiers.platform;
                let shift = event.keystroke.modifiers.shift;

                if cmd && shift {
                    match key {
                        "d" | "D" => this.split_pane(SplitDirection::Vertical, window, cx),
                        "]" => this.next_tab(cx),
                        "[" => this.prev_tab(cx),
                        _ => {}
                    }
                } else if cmd {
                    match key {
                        "t" => this.new_tab(cx),
                        "w" => this.request_close_pane(cx),
                        "d" => this.split_pane(SplitDirection::Horizontal, window, cx),
                        "}" | "]" => this.next_tab(cx),
                        "{" | "[" => this.prev_tab(cx),
                        "," => crate::settings_opener::open_config_file(),
                        _ => {}
                    }
                }
            }))
            .child(
                div()
                    .id("tab-bar")
                    .h(px(38.0))
                    .w_full()
                    .bg(title_bar_bg)
                    .border_b_1()
                    .border_color(border_color)
                    .flex()
                    .pl(px(settings::constants::tab_bar::LEFT_PADDING))
                    .pr(px(settings::constants::tab_bar::RIGHT_PADDING))
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
                            .on_drag(TabDrag { index: i, title: title.clone() }, move |drag, _position, _window, cx| {
                                let ghost_title = drag.title.clone();
                                cx.new(move |_cx| TabDragGhost { title: ghost_title })
                            })
                            .drag_over::<TabDrag>(|style, _, _, _| {
                                style.bg(hsla(0.0, 0.0, 1.0, 0.08))
                            })
                            .on_drop(cx.listener(move |this, drag: &TabDrag, _window, cx| {
                                let from = drag.index;
                                let to = i;
                                if from != to && from < this.tabs.len() && to < this.tabs.len() {
                                    this.tabs.swap(from, to);
                                    if this.active_tab == from {
                                        this.active_tab = to;
                                    } else if this.active_tab == to {
                                        this.active_tab = from;
                                    }
                                    this.last_title_update = std::time::Instant::now()
                                        - settings::constants::timing::TITLE_CACHE_TTL;
                                    cx.notify();
                                }
                            }))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                                this.switch_tab(i, cx);
                            }))
                            .child({
                                let badge = tab.panes.find_pane(tab.active_pane)
                                    .map(|pane_kind| pane_kind.badge(cx))
                                    .unwrap_or(terminal_view::TabBadge::Running);
                                let (badge_color, badge_text) = match badge {
                                    terminal_view::TabBadge::Running => (hsla(0.33, 0.7, 0.5, 1.0), "\u{25CF}"),
                                    terminal_view::TabBadge::Success => (hsla(0.33, 0.7, 0.5, 0.7), "\u{2713}"),
                                    terminal_view::TabBadge::Failed(_) => (hsla(0.0, 0.7, 0.5, 1.0), "\u{2717}"),
                                };
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.0))
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .text_size(px(10.0))
                                            .text_color(badge_color)
                                            .child(badge_text),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .child(title),
                                    )
                            })
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
                    .child(div().flex_1())
            )
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .h_full()
                    .overflow_hidden()
                    .children(self.tabs.get(self.active_tab).map(|tab| {
                        crate::pane_group_view::render_pane_tree(&tab.panes, tab.active_pane, window, cx)
                    }))
            )
            .child(render_status_bar(&self.cached_stats, cx))
            .when(
                self.scratchpad.as_ref().is_some_and(|sp| sp.visible),
                |d| {
                    let sp = self.scratchpad.as_ref().expect("checked above");
                    let input_entity = sp.input.clone();
                    d.child(
                        div()
                            .id("scratchpad-backdrop")
                            .absolute()
                            .inset_0()
                            .bg(hsla(0.0, 0.0, 0.0, 0.4))
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.hide_scratchpad(window, cx);
                            }))
                    )
                    .child(
                        div()
                            .id("scratchpad-overlay")
                            .absolute()
                            .top(px(60.0))
                            .left(px(80.0))
                            .right(px(80.0))
                            .bottom(px(80.0))
                            .bg(hsla(0.0, 0.0, 0.10, 1.0))
                            .border_1()
                            .border_color(hsla(0.0, 0.0, 0.25, 1.0))
                            .rounded(px(8.0))
                            .shadow_lg()
                            .overflow_hidden()
                            .p(px(12.0))
                            .child(
                                gpui_component::input::Input::new(&input_entity)
                                    .appearance(false)
                                    .bordered(false)
                                    .h_full()
                            )
                    )
                },
            )
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
                    div()
                        .id("confirm-backdrop")
                        .absolute()
                        .inset_0()
                        .bg(hsla(0.0, 0.0, 0.0, 0.6))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            div()
                                .id("confirm-modal-container")
                                .size_full()
                                .absolute()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.cancel_pending_action(cx);
                                }))
                        )
                        .child(
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
                                    div()
                                        .text_base()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(foreground)
                                        .child(format!("\"{}\" is running", process_name))
                                )
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(muted)
                                        .line_height(px(20.0))
                                        .child(format!("Are you sure you want to {}? The running process will be terminated.", action_text))
                                )
                                .child(
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
            .when_some(self.command_palette.clone(), |d, palette| {
                d.child(palette)
            })
            .children(Root::render_dialog_layer(window, cx))
    }
}
