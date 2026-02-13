//! GPUI rendering for pane groups.
//!
//! Separates rendering concerns from the PaneNode tree logic.
//! This allows the tree logic to be tested without GPUI context.

use crate::pane_group::{PaneNode, SplitDirection};
use crate::workspace_view::Workspace;
use gpui::*;
use theme::terminal_colors;
use uuid::Uuid;

/// Render a pane tree as a GPUI element.
///
/// Recursively builds nested flex containers for splits and terminal views for leaves.
/// Unfocused panes are dimmed with a subtle overlay for visual distinction.
pub fn render_pane_tree(
    node: &PaneNode,
    active_pane: Uuid,
    _window: &mut Window,
    cx: &mut Context<'_, Workspace>,
) -> AnyElement {
    // Get theme colors
    let colors = terminal_colors(cx);
    let border = colors.border;

    match node {
        PaneNode::Leaf { id, pane } => {
            let pane_id = *id;
            let is_active = *id == active_pane;

            let mut container = div()
                .id(ElementId::Name(format!("pane-{}", id).into()))
                .size_full()
                .relative()
                .bg(colors.background)
                .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                    this.set_active_pane(pane_id, cx);
                }))
                .child(pane.render(_window));

            // Dim overlay for unfocused panes (like Ghostty)
            if !is_active {
                container = container.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .bg(hsla(0.0, 0.0, 0.0, 0.35)),
                );
            }

            container.into_any_element()
        }
        PaneNode::Split {
            direction,
            first,
            second,
            ratio,
        } => {
            let ratio = *ratio;

            let first_elem = render_pane_tree(first, active_pane, _window, cx);
            let second_elem = render_pane_tree(second, active_pane, _window, cx);

            match direction {
                SplitDirection::Horizontal => div()
                    .size_full()
                    .flex()
                    .flex_row()
                    .child(div().h_full().w(relative(ratio)).child(first_elem))
                    .child(div().h_full().w(px(2.0)).bg(border))
                    .child(div().h_full().w(relative(1.0 - ratio)).child(second_elem))
                    .into_any_element(),
                SplitDirection::Vertical => div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .child(div().w_full().h(relative(ratio)).child(first_elem))
                    .child(div().w_full().h(px(2.0)).bg(border))
                    .child(div().w_full().h(relative(1.0 - ratio)).child(second_elem))
                    .into_any_element(),
            }
        }
    }
}
