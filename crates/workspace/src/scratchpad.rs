//! Persistent scratchpad — a drop-down notes overlay toggled with Ctrl+`.
//!
//! Notes are auto-saved to `<data-dir>/humanssh/scratchpad.md` and loaded on startup.

use gpui::{App, Entity, Subscription};
use gpui_component::input::InputState;
use std::path::PathBuf;

fn scratchpad_path() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("humanssh").join("scratchpad.md"))
}

pub(crate) fn load_scratchpad_content() -> String {
    scratchpad_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default()
}

fn save_scratchpad_content(content: &str) {
    if let Some(path) = scratchpad_path() {
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                tracing::warn!("Failed to create scratchpad directory: {}", e);
                return;
            }
        }
        if let Err(e) = std::fs::write(&path, content) {
            tracing::warn!("Failed to save scratchpad: {}", e);
        }
    }
}

/// State for the scratchpad overlay.
pub struct ScratchpadState {
    /// The multi-line text input entity.
    pub input: Entity<InputState>,
    /// Whether the overlay is currently visible.
    pub visible: bool,
    /// Subscriptions kept alive by this state.
    pub(crate) _subscriptions: Vec<Subscription>,
}

impl ScratchpadState {
    /// Toggle visibility. Returns the new visible state.
    pub fn toggle(&mut self) -> bool {
        self.visible = !self.visible;
        self.visible
    }

    /// Save scratchpad contents to disk.
    pub fn save(&self, cx: &App) {
        let content = self.input.read(cx).value();
        save_scratchpad_content(&content);
    }

    #[cfg(test)]
    pub fn is_visible(&self) -> bool {
        self.visible
    }
}

#[cfg(test)]
mod tests {
    use crate::workspace_view::Workspace;
    use gpui::{TestAppContext, VisualContext};

    fn init_test_context(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_component::init(cx);
        });
    }

    #[gpui::test]
    fn test_scratchpad_toggle(cx: &mut TestAppContext) {
        init_test_context(cx);
        let (workspace, vcx) = cx.add_window_view(|_window, cx| Workspace::new(cx));

        vcx.update_window_entity(&workspace, |ws, window, cx| {
            ws.ensure_scratchpad(window, cx);
            let sp = ws.scratchpad.as_mut().expect("scratchpad should exist");
            assert!(sp.is_visible(), "should start visible");

            sp.toggle();
            assert!(!sp.is_visible(), "should be hidden after toggle");

            sp.toggle();
            assert!(sp.is_visible(), "should be visible after second toggle");
        });
    }
}
