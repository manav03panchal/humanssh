//! GPUI rendering for pane groups.
//!
//! Separates rendering concerns from the PaneNode tree logic.
//! This allows the tree logic to be tested without GPUI context.

use crate::pane_group::{PaneNode, SplitDirection};
use crate::workspace_view::{TabDrag, Workspace};
use gpui::*;
use theme::terminal_colors;
use uuid::Uuid;

/// Render a pane tree as a GPUI element.
///
/// Recursively builds nested flex containers for splits and terminal views for leaves.
/// Unfocused panes are dimmed with a subtle overlay for visual distinction.
/// Pane leaves accept tab drops to create splits (drag a tab onto a pane to split it).
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
                // Accept tab drops to create splits
                .drag_over::<TabDrag>(|style, _, _, _| style.bg(hsla(0.6, 0.5, 0.3, 0.15)))
                .on_drop(cx.listener(move |this, drag: &TabDrag, window, cx| {
                    // Determine split direction from drop position relative to pane center.
                    // Left/right half → horizontal split, top/bottom half → vertical split.
                    let mouse = window.mouse_position();
                    let bounds = window.bounds();
                    let center_x = f32::from(bounds.origin.x) + f32::from(bounds.size.width) / 2.0;
                    let center_y = f32::from(bounds.origin.y) + f32::from(bounds.size.height) / 2.0;
                    let mx: f32 = mouse.x.into();
                    let my: f32 = mouse.y.into();

                    // Compare distance from center on each axis to decide direction
                    let dx = (mx - center_x).abs();
                    let dy = (my - center_y).abs();

                    let direction = if dx > dy {
                        SplitDirection::Horizontal
                    } else {
                        SplitDirection::Vertical
                    };

                    this.split_with_tab(drag.index, pane_id, direction, cx);
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
