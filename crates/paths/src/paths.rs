//! Centralized path management for HumanSSH.
//!
//! All application directories are lazily initialized and cached.
//! Use `set_*` functions before `init()` to override for testing.

use std::path::PathBuf;
use std::sync::OnceLock;

static CONFIG_DIR: OnceLock<PathBuf> = OnceLock::new();
static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();
static LOGS_DIR: OnceLock<PathBuf> = OnceLock::new();
static CACHE_DIR: OnceLock<PathBuf> = OnceLock::new();

/// ~/.config/humanssh (or platform equivalent)
pub fn config_dir() -> &'static PathBuf {
    CONFIG_DIR.get_or_init(|| {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("humanssh")
    })
}

/// ~/Library/Application Support/humanssh (or platform equivalent)
pub fn data_dir() -> &'static PathBuf {
    DATA_DIR.get_or_init(|| {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("humanssh")
    })
}

/// ~/Library/Logs/humanssh (or platform equivalent)
pub fn logs_dir() -> &'static PathBuf {
    LOGS_DIR.get_or_init(|| {
        #[cfg(target_os = "macos")]
        {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("Library")
                .join("Logs")
                .join("humanssh")
        }
        #[cfg(not(target_os = "macos"))]
        {
            dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("humanssh")
                .join("logs")
        }
    })
}

/// ~/Library/Caches/humanssh (or platform equivalent)
pub fn cache_dir() -> &'static PathBuf {
    CACHE_DIR.get_or_init(|| {
        dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("humanssh")
    })
}

/// Override config dir (must be called before first access). For testing.
pub fn set_config_dir(path: PathBuf) {
    let _ = CONFIG_DIR.set(path);
}

/// Override data dir (must be called before first access). For testing.
pub fn set_data_dir(path: PathBuf) {
    let _ = DATA_DIR.set(path);
}

/// Override logs dir (must be called before first access). For testing.
pub fn set_logs_dir(path: PathBuf) {
    let _ = LOGS_DIR.set(path);
}

/// Override cache dir (must be called before first access). For testing.
pub fn set_cache_dir(path: PathBuf) {
    let _ = CACHE_DIR.set(path);
}

/// Config file path: config_dir()/config.toml
pub fn config_file() -> PathBuf {
    config_dir().join("config.toml")
}

/// Themes directory: look relative to executable first, then config dir.
pub fn themes_dir() -> PathBuf {
    config_dir().join("themes")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_dir_ends_with_humanssh() {
        let dir = config_dir();
        assert!(
            dir.ends_with("humanssh"),
            "config_dir should end with 'humanssh': {:?}",
            dir
        );
    }

    #[test]
    fn config_file_is_toml() {
        let path = config_file();
        assert_eq!(path.extension().and_then(|e| e.to_str()), Some("toml"));
    }

    #[test]
    fn data_dir_ends_with_humanssh() {
        let dir = data_dir();
        assert!(
            dir.ends_with("humanssh"),
            "data_dir should end with 'humanssh': {:?}",
            dir
        );
    }
}
