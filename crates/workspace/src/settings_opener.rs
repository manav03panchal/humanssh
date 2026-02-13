//! Config file opener.
//!
//! Opens the config file in the user's preferred editor.

/// Open the config file in the user's editor.
/// Creates a default config file if it doesn't exist.
#[allow(clippy::disallowed_methods)] // Fire-and-forget editor launch, blocking is fine
pub fn open_config_file() {
    let Some(path) = settings::ensure_config_file() else {
        tracing::warn!("Could not determine config file path");
        return;
    };

    let path_str = path.to_string_lossy().to_string();

    // Try $EDITOR first, then platform default
    if let Ok(editor) = std::env::var("EDITOR") {
        match std::process::Command::new(&editor).arg(&path_str).spawn() {
            Ok(_) => {
                tracing::info!("Opened config in $EDITOR ({})", editor);
                return;
            }
            Err(e) => tracing::warn!("Failed to open $EDITOR ({}): {}", editor, e),
        }
    }

    // Platform-specific fallback
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg("-t")
            .arg(&path_str)
            .spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open")
            .arg(&path_str)
            .spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", &path_str])
            .spawn();
    }
}
