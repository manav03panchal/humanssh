//! Application module - main workspace container.

mod pane;
mod pane_group;
mod pane_group_view;
mod settings;
mod status_bar;
mod workspace;

pub use settings::toggle_settings_dialog;
pub use status_bar::{render_status_bar, stats_collector, SystemStats};
pub use workspace::Workspace;
