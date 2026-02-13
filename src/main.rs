//! HumanSSH - A fast, cross-platform SSH terminal
//!
//! Main entry point for the application.

use anyhow::{Context, Result};
use gpui::*;
use gpui_component_assets::Assets;
use humanssh::actions::{Quit, ToggleSecureInput};
use humanssh::app::Workspace;
use humanssh::theme;
use once_cell::sync::Lazy;
use std::time::Instant;
use tracing::{debug, error, info};

/// Application startup time for performance monitoring
static STARTUP_TIME: Lazy<Instant> = Lazy::new(Instant::now);

/// Initialize required directories (cross-platform).
/// Uses platform-appropriate directories via the `dirs` crate.
fn init_paths() -> Result<()> {
    let config_dir = dirs::config_dir()
        .context("Could not determine config directory")?
        .join("humanssh");
    let data_dir = dirs::data_dir()
        .context("Could not determine data directory")?
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

/// Check if debug mode is enabled via environment variable.
fn is_debug_mode() -> bool {
    std::env::var("HUMANSSH_DEBUG").is_ok()
}

/// Initialize the logging system.
fn init_logging() {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};

    // In debug mode, enable trace logging for humanssh
    let default_filter = if is_debug_mode() {
        "humanssh=trace,gpui=debug,info"
    } else {
        "humanssh=info,warn"
    };

    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));

    tracing_subscriber::registry()
        .with(fmt::layer().with_target(true).with_line_number(true))
        .with(filter)
        .init();

    if is_debug_mode() {
        info!(
            "HumanSSH v{} starting up (DEBUG MODE ENABLED)",
            env!("CARGO_PKG_VERSION")
        );
        info!("Set RUST_LOG for custom log levels, e.g. RUST_LOG=humanssh=trace");
    } else {
        info!("HumanSSH v{} starting up", env!("CARGO_PKG_VERSION"));
    }
}

/// Compute a centered origin for the given window size on the primary display.
fn centered_origin(w: f32, h: f32, cx: &mut App) -> Point<Pixels> {
    if let Some(display) = cx.primary_display() {
        let screen = display.bounds();
        let x = (f32::from(screen.size.width) - w) / 2.0;
        let y = (f32::from(screen.size.height) - h) / 2.0;
        Point::new(px(x.max(0.0)), px(y.max(0.0)))
    } else {
        Point::default()
    }
}

/// Build window options using saved bounds (macOS/Windows).
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn build_window_options(cx: &mut App) -> WindowOptions {
    let config = humanssh::config::file::load_config();
    let w = config.window_width.unwrap_or(1200.0);
    let h = config.window_height.unwrap_or(800.0);
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: centered_origin(w, h, cx),
            size: Size {
                width: px(w),
                height: px(h),
            },
        })),
        titlebar: Some(build_titlebar_options()),
        ..Default::default()
    }
}

/// Build window options using saved bounds (Linux).
/// Includes window_decorations setting for Wayland/X11 compositors.
#[cfg(target_os = "linux")]
fn build_window_options(cx: &mut App) -> WindowOptions {
    let config = humanssh::config::file::load_config();
    let w = config.window_width.unwrap_or(1200.0);
    let h = config.window_height.unwrap_or(800.0);

    let window_decorations = match config.linux_decorations.as_deref() {
        Some("client") => Some(WindowDecorations::Client),
        _ => Some(WindowDecorations::Server),
    };

    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: centered_origin(w, h, cx),
            size: Size {
                width: px(w),
                height: px(h),
            },
        })),
        titlebar: Some(build_titlebar_options()),
        window_decorations,
        ..Default::default()
    }
}

/// Build window options using saved bounds (other Unix).
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn build_window_options(cx: &mut App) -> WindowOptions {
    let config = humanssh::config::file::load_config();
    let w = config.window_width.unwrap_or(1200.0);
    let h = config.window_height.unwrap_or(800.0);
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds {
            origin: centered_origin(w, h, cx),
            size: Size {
                width: px(w),
                height: px(h),
            },
        })),
        titlebar: Some(build_titlebar_options()),
        ..Default::default()
    }
}

/// Build platform-specific titlebar options.
/// macOS: transparent titlebar with traffic light buttons
/// Windows: native titlebar with min/max/close buttons
#[cfg(target_os = "macos")]
fn build_titlebar_options() -> TitlebarOptions {
    TitlebarOptions {
        title: Some("HumanSSH".into()),
        appears_transparent: true,
        ..Default::default()
    }
}

/// Build platform-specific titlebar options.
/// Windows: native titlebar with standard window chrome
#[cfg(target_os = "windows")]
fn build_titlebar_options() -> TitlebarOptions {
    TitlebarOptions {
        title: Some("HumanSSH".into()),
        // Use native Windows titlebar for proper min/max/close buttons
        appears_transparent: false,
        ..Default::default()
    }
}

/// Build platform-specific titlebar options.
/// Linux: configurable based on user preference (Server = native, Client = custom)
#[cfg(target_os = "linux")]
fn build_titlebar_options() -> TitlebarOptions {
    let config = humanssh::config::file::load_config();
    let appears_transparent = config.linux_decorations.as_deref() == Some("client");

    TitlebarOptions {
        title: Some("HumanSSH".into()),
        appears_transparent,
        ..Default::default()
    }
}

/// Build platform-specific titlebar options.
/// Other Unix (FreeBSD, etc.): native titlebar
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn build_titlebar_options() -> TitlebarOptions {
    TitlebarOptions {
        title: Some("HumanSSH".into()),
        appears_transparent: false,
        ..Default::default()
    }
}

/// Open the main application window.
fn open_main_window(cx: &mut App) -> Result<()> {
    let options = build_window_options(cx);
    cx.open_window(options, |window, cx| {
        let app_view = cx.new(Workspace::new);
        cx.new(|cx| gpui_component::Root::new(app_view, window, cx))
    })
    .context("Failed to open main window")?;

    info!("Main window opened in {:?}", STARTUP_TIME.elapsed());
    Ok(())
}

/// Register keybindings.
fn register_keybindings(cx: &mut App) {
    use humanssh::actions::{CloseTab, OpenSettings};

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
        // macOS: Toggle secure keyboard entry
        KeyBinding::new("cmd-shift-s", ToggleSecureInput, None),
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

    // Register terminal-specific keybindings (Tab, etc.)
    humanssh::terminal::register_keybindings(cx);
    debug!("Terminal keybindings registered");
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
