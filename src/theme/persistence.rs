//! Legacy persistence types.
//!
//! Contains platform-specific enums still referenced by actions.
//! Settings persistence has moved to `config::file` (TOML config).

use serde::{Deserialize, Serialize};

/// Available shell options for Windows.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum WindowsShell {
    #[default]
    PowerShell,
    PowerShellCore,
    Cmd,
}

/// Window decoration style for Linux.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum LinuxDecorations {
    #[default]
    Server,
    Client,
}
