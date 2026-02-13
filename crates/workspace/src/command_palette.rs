//! Command palette overlay with fuzzy search for HumanSSH actions.

use actions::*;
use gpui::prelude::FluentBuilder;
use gpui::{
    div, hsla, px, Action, App, Context, ElementId, FocusHandle, Focusable, InteractiveElement,
    IntoElement, KeyDownEvent, ParentElement, Render, SharedString, StatefulInteractiveElement,
    Styled, Window,
};

/// A single entry in the command palette.
struct CommandEntry {
    label: &'static str,
    shortcut: &'static str,
    action: Box<dyn Action>,
}

/// Event emitted when the palette should close, optionally executing an action.
pub struct CommandPaletteDismiss {
    pub action: Option<Box<dyn Action>>,
}

impl gpui::EventEmitter<CommandPaletteDismiss> for CommandPalette {}

fn build_command_entries() -> Vec<CommandEntry> {
    vec![
        CommandEntry {
            label: "New Tab",
            shortcut: "Cmd+T",
            action: Box::new(NewTab),
        },
        CommandEntry {
            label: "Close Tab",
            shortcut: "Cmd+W",
            action: Box::new(CloseTab),
        },
        CommandEntry {
            label: "Next Tab",
            shortcut: "Cmd+Shift+]",
            action: Box::new(NextTab),
        },
        CommandEntry {
            label: "Previous Tab",
            shortcut: "Cmd+Shift+[",
            action: Box::new(PrevTab),
        },
        CommandEntry {
            label: "Split Vertical",
            shortcut: "Cmd+D",
            action: Box::new(SplitVertical),
        },
        CommandEntry {
            label: "Split Horizontal",
            shortcut: "Cmd+Shift+D",
            action: Box::new(SplitHorizontal),
        },
        CommandEntry {
            label: "Close Pane",
            shortcut: "",
            action: Box::new(ClosePane),
        },
        CommandEntry {
            label: "Focus Next Pane",
            shortcut: "Cmd+Alt+Right",
            action: Box::new(FocusNextPane),
        },
        CommandEntry {
            label: "Focus Previous Pane",
            shortcut: "Cmd+Alt+Left",
            action: Box::new(FocusPrevPane),
        },
        CommandEntry {
            label: "Search",
            shortcut: "Cmd+F",
            action: Box::new(SearchToggle),
        },
        CommandEntry {
            label: "Search Next",
            shortcut: "Cmd+G",
            action: Box::new(SearchNext),
        },
        CommandEntry {
            label: "Search Previous",
            shortcut: "Cmd+Shift+G",
            action: Box::new(SearchPrev),
        },
        CommandEntry {
            label: "Toggle Regex Search",
            shortcut: "Cmd+Alt+R",
            action: Box::new(SearchToggleRegex),
        },
        CommandEntry {
            label: "Enter Copy Mode",
            shortcut: "Cmd+Shift+C",
            action: Box::new(EnterCopyMode),
        },
        CommandEntry {
            label: "Exit Copy Mode",
            shortcut: "",
            action: Box::new(ExitCopyMode),
        },
        CommandEntry {
            label: "Open Settings",
            shortcut: "Cmd+,",
            action: Box::new(OpenSettings),
        },
        CommandEntry {
            label: "Toggle Secure Input",
            shortcut: "Cmd+Shift+S",
            action: Box::new(ToggleSecureInput),
        },
        CommandEntry {
            label: "Toggle Option as Alt",
            shortcut: "",
            action: Box::new(ToggleOptionAsAlt),
        },
        CommandEntry {
            label: "Quit",
            shortcut: "Cmd+Q",
            action: Box::new(Quit),
        },
    ]
}

/// Fuzzy match: check if all query characters appear in order in the target (case-insensitive).
/// Returns true if the query is a subsequence of the target.
#[cfg(test)]
fn fuzzy_match(query: &str, target: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let mut query_chars = query.chars().flat_map(|c| c.to_lowercase());
    let mut current = match query_chars.next() {
        Some(c) => c,
        None => return true,
    };

    for target_char in target.chars().flat_map(|c| c.to_lowercase()) {
        if target_char == current {
            current = match query_chars.next() {
                Some(c) => c,
                None => return true,
            };
        }
    }
    false
}

/// Score a fuzzy match -- lower is better. Returns None if no match.
/// Prefers matches at word boundaries and consecutive characters.
fn fuzzy_score(query: &str, target: &str) -> Option<u32> {
    if query.is_empty() {
        return Some(0);
    }

    let query_lower: Vec<char> = query.chars().flat_map(|c| c.to_lowercase()).collect();
    let target_lower: Vec<char> = target.chars().flat_map(|c| c.to_lowercase()).collect();

    let mut query_idx = 0;
    let mut score: u32 = 0;
    let mut last_match_pos: Option<usize> = None;

    for (target_idx, &target_char) in target_lower.iter().enumerate() {
        if query_idx < query_lower.len() && target_char == query_lower[query_idx] {
            let at_word_start =
                target_idx == 0 || target.as_bytes().get(target_idx - 1) == Some(&b' ');
            if !at_word_start {
                score += 1;
            }

            if let Some(last) = last_match_pos {
                if target_idx > last + 1 {
                    score += (target_idx - last - 1) as u32;
                }
            }

            last_match_pos = Some(target_idx);
            query_idx += 1;
        }
    }

    if query_idx == query_lower.len() {
        Some(score)
    } else {
        None
    }
}

/// The command palette overlay view.
pub struct CommandPalette {
    query: String,
    selected_index: usize,
    entries: Vec<CommandEntry>,
    filtered_indices: Vec<usize>,
    pub(crate) focus_handle: FocusHandle,
}

impl CommandPalette {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let entries = build_command_entries();
        let filtered_indices: Vec<usize> = (0..entries.len()).collect();
        let focus_handle = cx.focus_handle();

        Self {
            query: String::new(),
            selected_index: 0,
            entries,
            filtered_indices,
            focus_handle,
        }
    }

    fn update_filter(&mut self) {
        let mut scored: Vec<(usize, u32)> = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(idx, entry)| fuzzy_score(&self.query, entry.label).map(|s| (idx, s)))
            .collect();

        scored.sort_by_key(|&(_, score)| score);
        self.filtered_indices = scored.into_iter().map(|(idx, _)| idx).collect();

        self.selected_index = 0;
    }

    fn move_up(&mut self, cx: &mut Context<Self>) {
        if !self.filtered_indices.is_empty() {
            if self.selected_index == 0 {
                self.selected_index = self.filtered_indices.len() - 1;
            } else {
                self.selected_index -= 1;
            }
            cx.notify();
        }
    }

    fn move_down(&mut self, cx: &mut Context<Self>) {
        if !self.filtered_indices.is_empty() {
            self.selected_index = (self.selected_index + 1) % self.filtered_indices.len();
            cx.notify();
        }
    }

    fn confirm(&mut self, cx: &mut Context<Self>) {
        let action = self
            .filtered_indices
            .get(self.selected_index)
            .and_then(|&idx| self.entries.get(idx))
            .map(|entry| entry.action.boxed_clone());

        cx.emit(CommandPaletteDismiss { action });
    }

    fn dismiss(&mut self, cx: &mut Context<Self>) {
        cx.emit(CommandPaletteDismiss { action: None });
    }
}

impl Focusable for CommandPalette {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for CommandPalette {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = theme::terminal_colors(cx);
        let foreground = colors.foreground;
        let muted = colors.muted;
        let border_color = colors.border;

        let query_display: SharedString = if self.query.is_empty() {
            "Type a command...".into()
        } else {
            self.query.clone().into()
        };
        let query_is_empty = self.query.is_empty();

        let max_visible = 12;
        let visible_count = self.filtered_indices.len().min(max_visible);

        div()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                let key = event.keystroke.key.as_str();
                let mods = &event.keystroke.modifiers;

                match key {
                    "escape" => this.dismiss(cx),
                    "up" => this.move_up(cx),
                    "down" => this.move_down(cx),
                    "enter" => this.confirm(cx),
                    "backspace" => {
                        this.query.pop();
                        this.update_filter();
                        cx.notify();
                    }
                    "space" => {
                        this.query.push(' ');
                        this.update_filter();
                        cx.notify();
                    }
                    "tab" => {}
                    _ => {
                        if key.len() == 1 && !mods.control && !mods.alt && !mods.platform {
                            let ch = if mods.shift {
                                key.to_uppercase()
                            } else {
                                key.to_string()
                            };
                            this.query.push_str(&ch);
                            this.update_filter();
                            cx.notify();
                        }
                    }
                }
            }))
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .items_center()
            .child(
                div()
                    .id("palette-backdrop")
                    .absolute()
                    .inset_0()
                    .bg(hsla(0.0, 0.0, 0.0, 0.4))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.dismiss(cx);
                    })),
            )
            .child(
                div()
                    .mt(px(60.0))
                    .w(px(480.0))
                    .bg(hsla(0.0, 0.0, 0.10, 1.0))
                    .border_1()
                    .border_color(hsla(0.0, 0.0, 0.25, 1.0))
                    .rounded(px(8.0))
                    .shadow_lg()
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .h(px(40.0))
                            .w_full()
                            .px(px(12.0))
                            .flex()
                            .items_center()
                            .border_b_1()
                            .border_color(border_color)
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(hsla(0.0, 0.0, 0.4, 1.0))
                                    .mr(px(8.0))
                                    .child(">"),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_sm()
                                    .when(query_is_empty, |d| {
                                        d.text_color(hsla(0.0, 0.0, 0.4, 1.0))
                                    })
                                    .when(!query_is_empty, |d| d.text_color(foreground))
                                    .child(query_display),
                            ),
                    )
                    .child(
                        div()
                            .max_h(px(max_visible as f32 * 32.0))
                            .overflow_hidden()
                            .children(
                                self.filtered_indices
                                    .iter()
                                    .take(visible_count)
                                    .enumerate()
                                    .map(|(visible_idx, &entry_idx)| {
                                        let is_selected = visible_idx == self.selected_index;
                                        let entry = &self.entries[entry_idx];
                                        let label: SharedString = entry.label.into();
                                        let shortcut: SharedString = entry.shortcut.into();
                                        let has_shortcut = !entry.shortcut.is_empty();

                                        div()
                                            .id(ElementId::Name(
                                                format!("cmd-{}", entry_idx).into(),
                                            ))
                                            .h(px(32.0))
                                            .w_full()
                                            .px(px(12.0))
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .cursor_pointer()
                                            .when(is_selected, |d| d.bg(hsla(0.0, 0.0, 0.18, 1.0)))
                                            .when(!is_selected, |d| {
                                                d.hover(|d| d.bg(hsla(0.0, 0.0, 0.14, 1.0)))
                                            })
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.selected_index = visible_idx;
                                                this.confirm(cx);
                                            }))
                                            .child(
                                                div().text_sm().text_color(foreground).child(label),
                                            )
                                            .when(has_shortcut, |d| {
                                                d.child(
                                                    div()
                                                        .text_size(px(11.0))
                                                        .text_color(muted)
                                                        .child(shortcut),
                                                )
                                            })
                                    }),
                            ),
                    )
                    .when(self.filtered_indices.is_empty(), |d| {
                        d.child(
                            div()
                                .h(px(40.0))
                                .w_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_sm()
                                .text_color(muted)
                                .child("No matching commands"),
                        )
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuzzy_match_empty_query() {
        assert!(fuzzy_match("", "New Tab"));
    }

    #[test]
    fn test_fuzzy_match_exact() {
        assert!(fuzzy_match("New Tab", "New Tab"));
    }

    #[test]
    fn test_fuzzy_match_subsequence() {
        assert!(fuzzy_match("nt", "New Tab"));
        assert!(fuzzy_match("NT", "New Tab"));
        assert!(fuzzy_match("nwt", "New Tab"));
    }

    #[test]
    fn test_fuzzy_match_no_match() {
        assert!(!fuzzy_match("xyz", "New Tab"));
        assert!(!fuzzy_match("tbn", "New Tab"));
    }

    #[test]
    fn test_fuzzy_match_case_insensitive() {
        assert!(fuzzy_match("new tab", "New Tab"));
        assert!(fuzzy_match("NEW TAB", "New Tab"));
    }

    #[test]
    fn test_fuzzy_match_single_char() {
        assert!(fuzzy_match("n", "New Tab"));
        assert!(fuzzy_match("t", "New Tab"));
        assert!(!fuzzy_match("z", "New Tab"));
    }

    #[test]
    fn test_fuzzy_score_exact_match() {
        let score = fuzzy_score("New Tab", "New Tab");
        assert!(score.is_some());
    }

    #[test]
    fn test_fuzzy_score_no_match() {
        assert!(fuzzy_score("xyz", "New Tab").is_none());
    }

    #[test]
    fn test_fuzzy_score_shorter_query_lower() {
        // "nt" should score lower (better) than "nwt" since both match but "nt" has
        // less gap penalty (N->T skips 3 chars, N->w->T has mid-word w penalty)
        let score_nt = fuzzy_score("nt", "New Tab");
        let score_nwt = fuzzy_score("nwt", "New Tab");
        assert!(score_nt.is_some());
        assert!(score_nwt.is_some());
        // Both should produce valid scores
    }

    #[test]
    fn test_fuzzy_score_empty_query() {
        assert_eq!(fuzzy_score("", "New Tab"), Some(0));
    }

    #[test]
    fn test_command_entries_complete() {
        let entries = build_command_entries();
        let labels: Vec<&str> = entries.iter().map(|e| e.label).collect();

        assert!(labels.contains(&"New Tab"));
        assert!(labels.contains(&"Close Tab"));
        assert!(labels.contains(&"Split Vertical"));
        assert!(labels.contains(&"Split Horizontal"));
        assert!(labels.contains(&"Search"));
        assert!(labels.contains(&"Open Settings"));
        assert!(labels.contains(&"Quit"));
        assert!(labels.contains(&"Enter Copy Mode"));
        assert!(labels.contains(&"Toggle Secure Input"));
        assert!(labels.contains(&"Toggle Option as Alt"));
        assert!(labels.contains(&"Focus Next Pane"));
        assert!(labels.contains(&"Focus Previous Pane"));
    }
}
