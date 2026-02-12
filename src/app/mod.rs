//! Application module - main workspace container.

mod pane;
mod pane_group;
mod pane_group_view;
mod settings;
mod status_bar;
mod workspace;

pub use settings::open_config_file;
pub use status_bar::{render_status_bar, stats_collector, SystemStats};
pub use workspace::Workspace;
