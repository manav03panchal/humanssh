//! HumanSSH - A fast, cross-platform SSH terminal
//!
//! Main entry point for the application.

use anyhow::{Context, Result};
use gpui::*;
use gpui_component_assets::Assets;
use humanssh::app::Workspace;
use humanssh::actions::Quit;
use humanssh::theme;
use once_cell::sync::Lazy;
use std::time::Instant;
use tracing::{debug, error, info};

/// Application startup time for performance monitoring
static STARTUP_TIME: Lazy<Instant> = Lazy::new(Instant::now);

/// Initialize required directories.
fn init_paths() -> Result<()> {
    let home = std::env::var_os("HOME").context("HOME environment variable not set")?;
    let config_dir = std::path::PathBuf::from(&home)
        .join(".config")
        .join("humanssh");
    let data_dir = std::path::PathBuf::from(&home)
        .join(".local")
        .join("share")
        .join("humanssh");

    std::fs::create_dir_all(&config_dir)
        .with_context(|| format!("Failed to create config directory: {:?}", config_dir))?;
    std::fs::create_dir_all(&data_dir)
        .with_context(|| format!("Failed to create data directory: {:?}", data_dir))?;

    debug!(
        "Initialized paths - config: {:?}, data: {:?}",
        config_dir, data_dir
    );
    Ok(())
}

/// Initialize the logging system.
fn init_logging() {
    use tracing_subscriber::{EnvFilter, fmt, prelude::*};

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("humanssh=info,warn"));

    tracing_subscriber::registry()
        .with(fmt::layer().with_target(true).with_line_number(true))
        .with(filter)
        .init();

    info!("HumanSSH v{} starting up", env!("CARGO_PKG_VERSION"));
}

/// Build window options.
fn build_window_options() -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: Point::new(px(100.0), px(100.0)),
            size: Size {
                width: px(1200.0),
                height: px(800.0),
            },
        })),
        titlebar: Some(TitlebarOptions {
            title: Some("HumanSSH".into()),
            appears_transparent: true,
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Open the main application window.
fn open_main_window(cx: &mut App) -> Result<()> {
    cx.open_window(build_window_options(), |window, cx| {
        let app_view = cx.new(Workspace::new);
        cx.new(|cx| gpui_component::Root::new(app_view, window, cx))
    })
    .context("Failed to open main window")?;

    info!("Main window opened in {:?}", STARTUP_TIME.elapsed());
    Ok(())
}

/// Register keybindings.
fn register_keybindings(cx: &mut App) {
    use humanssh::actions::{OpenSettings, CloseTab};

    // Note: Quit action is handled by Workspace.request_quit() for confirmation
    // The global handler is a fallback that shouldn't normally trigger
    cx.on_action(|_: &Quit, cx| {
        info!("Application quit requested (fallback)");
        cx.quit();
    });

    cx.bind_keys([
        // Quit (handled by workspace for confirmation)
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("ctrl-q", Quit, None),
        // Close tab
        KeyBinding::new("cmd-w", CloseTab, None),
        KeyBinding::new("ctrl-w", CloseTab, None),
        // Settings
        KeyBinding::new("cmd-,", OpenSettings, None),
        KeyBinding::new("ctrl-,", OpenSettings, None),
    ]);

    debug!("Keybindings registered");
}

/// Initialize subsystems.
fn initialize_subsystems(cx: &mut App) {
    gpui_component::init(cx);
    debug!("UI components initialized");

    theme::init(cx);
    debug!("Theme system initialized");

    register_keybindings(cx);
}

fn main() {
    let _ = *STARTUP_TIME;

    init_logging();

    if let Err(e) = init_paths() {
        error!("Failed to initialize paths: {}", e);
    }

    let app = Application::new().with_assets(Assets);

    app.on_reopen(|cx| {
        if cx.windows().is_empty() {
            if let Err(e) = open_main_window(cx) {
                error!("Failed to reopen window: {}", e);
            }
        }
    });

    app.run(|cx: &mut App| {
        cx.activate(true);
        initialize_subsystems(cx);

        if let Err(e) = open_main_window(cx) {
            error!("Failed to open main window: {}", e);
            cx.quit();
        }

        info!(
            "Application fully initialized in {:?}",
            STARTUP_TIME.elapsed()
        );
    });
}
