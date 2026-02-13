//! Workspace UI for HumanSSH.
//!
//! Tabs, split panes, and status bar.

mod pane;
mod pane_group;
mod pane_group_view;
mod settings_opener;
mod status_bar;
mod workspace_view;

pub use settings_opener::open_config_file;
pub use status_bar::{render_status_bar, stats_collector, SystemStats};
pub use workspace_view::Workspace;
