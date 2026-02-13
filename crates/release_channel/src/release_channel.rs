//! Release channel detection for HumanSSH.
//!
//! Determines whether we're running in Dev, Nightly, Preview, or Stable mode.
//! Read from `HUMANSSH_RELEASE_CHANNEL` env var at runtime, or defaults to Dev
//! in debug builds and Stable in release builds.

use std::fmt;
use std::sync::LazyLock;

/// The active release channel for this build.
static RELEASE_CHANNEL: LazyLock<ReleaseChannel> = LazyLock::new(|| {
    if let Ok(channel) = std::env::var("HUMANSSH_RELEASE_CHANNEL") {
        match channel.to_lowercase().as_str() {
            "dev" => ReleaseChannel::Dev,
            "nightly" => ReleaseChannel::Nightly,
            "preview" => ReleaseChannel::Preview,
            "stable" => ReleaseChannel::Stable,
            other => {
                tracing::warn!("Unknown release channel '{}', defaulting", other);
                ReleaseChannel::default()
            }
        }
    } else {
        ReleaseChannel::default()
    }
});

/// Build release channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReleaseChannel {
    /// Local development builds.
    Dev,
    /// Nightly automated builds.
    Nightly,
    /// Pre-release builds for testing.
    Preview,
    /// Public stable releases.
    Stable,
}

impl Default for ReleaseChannel {
    fn default() -> Self {
        if cfg!(debug_assertions) {
            Self::Dev
        } else {
            Self::Stable
        }
    }
}

impl fmt::Display for ReleaseChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dev => write!(f, "dev"),
            Self::Nightly => write!(f, "nightly"),
            Self::Preview => write!(f, "preview"),
            Self::Stable => write!(f, "stable"),
        }
    }
}

impl ReleaseChannel {
    /// Get the active release channel.
    pub fn global() -> Self {
        *RELEASE_CHANNEL
    }

    /// True for local development builds.
    pub fn is_dev(self) -> bool {
        self == Self::Dev
    }

    /// True for public stable releases.
    pub fn is_stable(self) -> bool {
        self == Self::Stable
    }

    /// True for any non-stable channel (dev, nightly, preview).
    pub fn is_prerelease(self) -> bool {
        !self.is_stable()
    }

    /// App display name varies by channel.
    pub fn app_name(self) -> &'static str {
        match self {
            Self::Dev => "HumanSSH Dev",
            Self::Nightly => "HumanSSH Nightly",
            Self::Preview => "HumanSSH Preview",
            Self::Stable => "HumanSSH",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_dev_in_debug() {
        // In test builds (debug_assertions = true), default should be Dev.
        assert_eq!(ReleaseChannel::default(), ReleaseChannel::Dev);
    }

    #[test]
    fn display_formats_correctly() {
        assert_eq!(ReleaseChannel::Dev.to_string(), "dev");
        assert_eq!(ReleaseChannel::Stable.to_string(), "stable");
        assert_eq!(ReleaseChannel::Nightly.to_string(), "nightly");
        assert_eq!(ReleaseChannel::Preview.to_string(), "preview");
    }

    #[test]
    fn is_prerelease_matches() {
        assert!(ReleaseChannel::Dev.is_prerelease());
        assert!(ReleaseChannel::Nightly.is_prerelease());
        assert!(ReleaseChannel::Preview.is_prerelease());
        assert!(!ReleaseChannel::Stable.is_prerelease());
    }

    #[test]
    fn app_name_varies_by_channel() {
        assert_eq!(ReleaseChannel::Stable.app_name(), "HumanSSH");
        assert!(ReleaseChannel::Dev.app_name().contains("Dev"));
    }
}
