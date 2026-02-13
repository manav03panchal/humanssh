//! Configuration system for HumanSSH.
//!
//! Provides compile-time constants and TOML config file support.

pub mod constants;
pub mod file;

pub use file::{
    apply_config, config_path, ensure_config_file, load_config, watch_config, Config,
    KeybindingEntry,
};
