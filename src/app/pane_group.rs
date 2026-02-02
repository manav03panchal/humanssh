//! Pane group for split pane layouts.

use crate::terminal::TerminalPane;
use crate::theme::terminal_colors;
use gpui::prelude::FluentBuilder;
use gpui::*;
use uuid::Uuid;

/// Direction of a split
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

/// A pane group node - either a leaf (terminal) or a split (two children)
#[derive(Clone)]
pub enum PaneNode {
    Leaf {
        id: Uuid,
        terminal: Entity<TerminalPane>,
    },
    Split {
        direction: SplitDirection,
        first: Box<PaneNode>,
        second: Box<PaneNode>,
        /// Ratio of first pane (0.0 to 1.0)
        ratio: f32,
    },
}

impl PaneNode {
    pub fn new_leaf(terminal: Entity<TerminalPane>) -> Self {
        Self::Leaf {
            id: Uuid::new_v4(),
            terminal,
        }
    }

    /// Find a pane by ID and split it, returns the new pane's ID if successful
    pub fn split(&mut self, target_id: Uuid, direction: SplitDirection, new_terminal: Entity<TerminalPane>) -> Option<Uuid> {
        match self {
            PaneNode::Leaf { id, terminal } => {
                if *id == target_id {
                    let old_terminal = terminal.clone();
                    let old_id = *id;
                    let new_id = Uuid::new_v4();

                    *self = PaneNode::Split {
                        direction,
                        first: Box::new(PaneNode::Leaf {
                            id: old_id,
                            terminal: old_terminal,
                        }),
                        second: Box::new(PaneNode::Leaf {
                            id: new_id,
                            terminal: new_terminal,
                        }),
                        ratio: 0.5,
                    };
                    Some(new_id)
                } else {
                    None
                }
            }
            PaneNode::Split { first, second, .. } => {
                first.split(target_id, direction, new_terminal.clone())
                    .or_else(|| second.split(target_id, direction, new_terminal))
            }
        }
    }

    /// Get the first leaf's ID (for focus)
    pub fn first_leaf_id(&self) -> Uuid {
        match self {
            PaneNode::Leaf { id, .. } => *id,
            PaneNode::Split { first, .. } => first.first_leaf_id(),
        }
    }

    /// Get all leaf terminal entities
    pub fn all_terminals(&self) -> Vec<(Uuid, Entity<TerminalPane>)> {
        match self {
            PaneNode::Leaf { id, terminal } => vec![(*id, terminal.clone())],
            PaneNode::Split { first, second, .. } => {
                let mut result = first.all_terminals();
                result.extend(second.all_terminals());
                result
            }
        }
    }

    /// Find a terminal by ID
    pub fn find_terminal(&self, target_id: Uuid) -> Option<Entity<TerminalPane>> {
        match self {
            PaneNode::Leaf { id, terminal } => {
                if *id == target_id {
                    Some(terminal.clone())
                } else {
                    None
                }
            }
            PaneNode::Split { first, second, .. } => {
                first.find_terminal(target_id).or_else(|| second.find_terminal(target_id))
            }
        }
    }

    /// Remove a pane by ID, returning true if removed
    pub fn remove(&mut self, target_id: Uuid) -> Option<PaneNode> {
        // First check what action to take without borrowing mutably
        let action = match self {
            PaneNode::Leaf { id, .. } => {
                if *id == target_id {
                    return None; // Can't remove self at this level
                }
                None
            }
            PaneNode::Split { first, second, .. } => {
                // Check if first child is the target leaf
                if let PaneNode::Leaf { id, .. } = first.as_ref() {
                    if *id == target_id {
                        Some(("promote_second", second.clone()))
                    } else {
                        None
                    }
                } else {
                    None
                }
                .or_else(|| {
                    // Check if second child is the target leaf
                    if let PaneNode::Leaf { id, .. } = second.as_ref() {
                        if *id == target_id {
                            Some(("promote_first", first.clone()))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
            }
        };

        if let Some((_, replacement)) = action {
            let old = std::mem::replace(self, *replacement);
            return Some(old);
        }

        // Recurse into children
        match self {
            PaneNode::Leaf { .. } => None,
            PaneNode::Split { first, second, .. } => {
                first.remove(target_id).or_else(|| second.remove(target_id))
            }
        }
    }

    /// Render the pane tree
    pub fn render(&self, active_pane: Uuid, _window: &mut Window, cx: &mut Context<'_, super::workspace::Workspace>) -> AnyElement {
        // Get theme colors
        let colors = terminal_colors(cx);
        let accent = colors.accent;
        let border = colors.border;

        match self {
            PaneNode::Leaf { id, terminal } => {
                let is_active = *id == active_pane;
                let pane_id = *id;

                div()
                    .id(ElementId::Name(format!("pane-{}", id).into()))
                    .size_full()
                    .border_1()
                    .bg(colors.background)
                    .when(is_active, |d| d.border_color(accent))
                    .when(!is_active, |d| d.border_color(border))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                        this.set_active_pane(pane_id, cx);
                    }))
                    .child(terminal.clone())
                    .into_any_element()
            }
            PaneNode::Split { direction, first, second, ratio } => {
                let ratio = *ratio;

                let first_elem = first.render(active_pane, _window, cx);
                let second_elem = second.render(active_pane, _window, cx);

                match direction {
                    SplitDirection::Horizontal => {
                        div()
                            .size_full()
                            .flex()
                            .flex_row()
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(ratio))
                                    .child(first_elem)
                            )
                            .child(
                                div()
                                    .h_full()
                                    .w(px(2.0))
                                    .bg(border)
                            )
                            .child(
                                div()
                                    .h_full()
                                    .w(relative(1.0 - ratio))
                                    .child(second_elem)
                            )
                            .into_any_element()
                    }
                    SplitDirection::Vertical => {
                        div()
                            .size_full()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .w_full()
                                    .h(relative(ratio))
                                    .child(first_elem)
                            )
                            .child(
                                div()
                                    .w_full()
                                    .h(px(2.0))
                                    .bg(border)
                            )
                            .child(
                                div()
                                    .w_full()
                                    .h(relative(1.0 - ratio))
                                    .child(second_elem)
                            )
                            .into_any_element()
                    }
                }
            }
        }
    }
}

