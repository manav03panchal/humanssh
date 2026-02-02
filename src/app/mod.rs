//! Application module - main workspace container.

mod pane;
mod pane_group;
mod pane_group_view;
mod settings;
mod workspace;

pub use settings::toggle_settings_dialog;
pub use workspace::Workspace;
