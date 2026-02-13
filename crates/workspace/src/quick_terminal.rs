//! Quick Terminal (drop-down visor) — toggled overlay at the top of the workspace.

use gpui::{AppContext, Context, Entity, Subscription};
use terminal_view::TerminalPane;

/// State for the quick terminal overlay.
pub struct QuickTerminalState {
    /// The terminal pane entity rendered inside the overlay.
    pub terminal: Entity<TerminalPane>,
    /// Whether the overlay is currently visible.
    pub visible: bool,
    /// Height as a fraction of the workspace (0.0–1.0).
    pub height_fraction: f32,
    /// Subscriptions (e.g. terminal exit) kept alive by this state.
    pub(crate) _subscriptions: Vec<Subscription>,
}

impl QuickTerminalState {
    /// Create a new quick terminal with a fresh shell.
    pub fn new(
        height_fraction: f32,
        subscriptions: Vec<Subscription>,
        cx: &mut Context<crate::workspace_view::Workspace>,
    ) -> Self {
        let terminal = cx.new(TerminalPane::new);
        Self {
            terminal,
            visible: true,
            height_fraction,
            _subscriptions: subscriptions,
        }
    }

    /// Toggle visibility. Returns the new visible state.
    pub fn toggle(&mut self) -> bool {
        self.visible = !self.visible;
        self.visible
    }

    #[cfg(test)]
    pub fn is_visible(&self) -> bool {
        self.visible
    }
}

#[cfg(test)]
mod tests {
    use crate::workspace_view::Workspace;
    use gpui::TestAppContext;

    fn init_test_context(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
        });
    }

    #[gpui::test]
    fn test_quick_terminal_toggle(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        workspace.update(cx, |ws, cx| {
            ws.ensure_quick_terminal(cx);
            let qt = ws
                .quick_terminal
                .as_mut()
                .expect("quick terminal should exist");
            assert!(qt.is_visible(), "should start visible");

            qt.toggle();
            assert!(!qt.is_visible(), "should be hidden after toggle");

            qt.toggle();
            assert!(qt.is_visible(), "should be visible after second toggle");
        });
    }

    #[gpui::test]
    fn test_quick_terminal_height_fraction(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, _vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        workspace.update(cx, |ws, cx| {
            ws.ensure_quick_terminal(cx);
            let qt = ws
                .quick_terminal
                .as_ref()
                .expect("quick terminal should exist");
            assert!(
                (qt.height_fraction - 0.4).abs() < f32::EPSILON,
                "default height fraction should be 0.4"
            );
        });
    }
}
