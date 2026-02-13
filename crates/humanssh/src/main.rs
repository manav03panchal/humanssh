//! HumanSSH - A fast, cross-platform SSH terminal
//!
//! Main entry point for the application.

use actions::{
    ClosePane, CloseTab, EnterCopyMode, ExitCopyMode, FocusNextPane, FocusPrevPane, NewTab,
    NextTab, OpenSettings, PrevTab, Quit, SearchNext, SearchPrev, SearchToggle, SearchToggleRegex,
    SendShiftTab, SendTab, SplitHorizontal, SplitVertical, ToggleCommandPalette, ToggleOptionAsAlt,
    ToggleScratchpad, ToggleSecureInput,
};
use anyhow::{Context, Result};
use gpui::*;
use gpui_component_assets::Assets;
use humanssh_workspace::Workspace;
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
    let config = settings::load_config();
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
#[cfg(target_os = "linux")]
fn build_window_options(cx: &mut App) -> WindowOptions {
    let config = settings::load_config();
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

/// Build window options (other Unix).
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn build_window_options(cx: &mut App) -> WindowOptions {
    let config = settings::load_config();
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

/// Build platform-specific titlebar options (macOS).
#[cfg(target_os = "macos")]
fn build_titlebar_options() -> TitlebarOptions {
    TitlebarOptions {
        title: Some("HumanSSH".into()),
        appears_transparent: false,
        ..Default::default()
    }
}

/// Build platform-specific titlebar options (Windows).
#[cfg(target_os = "windows")]
fn build_titlebar_options() -> TitlebarOptions {
    TitlebarOptions {
        title: Some("HumanSSH".into()),
        appears_transparent: false,
        ..Default::default()
    }
}

/// Build platform-specific titlebar options (Linux).
#[cfg(target_os = "linux")]
fn build_titlebar_options() -> TitlebarOptions {
    let config = settings::load_config();
    let appears_transparent = config.linux_decorations.as_deref() == Some("client");

    TitlebarOptions {
        title: Some("HumanSSH".into()),
        appears_transparent,
        ..Default::default()
    }
}

/// Build platform-specific titlebar options (other Unix).
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
    cx.on_action(|_: &Quit, cx| {
        info!("Application quit requested (fallback)");
        cx.quit();
    });

    cx.bind_keys([
        // Quit
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
        // Terminal-specific: Tab key
        KeyBinding::new("tab", SendTab, Some("terminal")),
        KeyBinding::new("shift-tab", SendShiftTab, Some("terminal")),
        // Tab navigation
        KeyBinding::new("cmd-t", NewTab, None),
        KeyBinding::new("ctrl-shift-t", NewTab, None),
        KeyBinding::new("cmd-shift-]", NextTab, None),
        KeyBinding::new("ctrl-tab", NextTab, None),
        KeyBinding::new("cmd-shift-[", PrevTab, None),
        KeyBinding::new("ctrl-shift-tab", PrevTab, None),
        // Splits
        KeyBinding::new("cmd-d", SplitVertical, None),
        KeyBinding::new("cmd-shift-d", SplitHorizontal, None),
        // Focus navigation
        KeyBinding::new("cmd-alt-right", FocusNextPane, None),
        KeyBinding::new("cmd-alt-left", FocusPrevPane, None),
        // Search
        KeyBinding::new("cmd-f", SearchToggle, Some("terminal")),
        KeyBinding::new("ctrl-f", SearchToggle, Some("terminal")),
        KeyBinding::new("cmd-alt-r", SearchToggleRegex, Some("terminal")),
        KeyBinding::new("alt-r", SearchToggleRegex, Some("terminal")),
        KeyBinding::new("cmd-g", SearchNext, Some("terminal")),
        KeyBinding::new("cmd-shift-g", SearchPrev, Some("terminal")),
        // Copy mode
        KeyBinding::new("cmd-shift-c", EnterCopyMode, Some("terminal")),
        // Command palette
        KeyBinding::new("cmd-shift-p", ToggleCommandPalette, None),
        KeyBinding::new("ctrl-shift-p", ToggleCommandPalette, None),
        // Scratchpad (persistent notes overlay)
        KeyBinding::new("ctrl-`", ToggleScratchpad, None),
    ]);

    // Apply user custom keybindings (these override defaults since GPUI uses last-wins)
    let config = settings::load_config();
    apply_custom_keybindings(&config, cx);

    debug!("Keybindings registered");
}

/// Apply custom keybindings from user config.
/// Maps action name strings to GPUI KeyBinding registrations.
fn apply_custom_keybindings(config: &settings::Config, cx: &mut App) {
    let mut bindings: Vec<KeyBinding> = Vec::new();

    for entry in &config.keybindings {
        let context = entry.context.as_deref();
        let keys = entry.keys.as_str();

        // Map action name to concrete action type
        match entry.action.as_str() {
            "quit" => bindings.push(KeyBinding::new(keys, Quit, context)),
            "new-tab" => bindings.push(KeyBinding::new(keys, NewTab, context)),
            "close-tab" => bindings.push(KeyBinding::new(keys, CloseTab, context)),
            "next-tab" => bindings.push(KeyBinding::new(keys, NextTab, context)),
            "prev-tab" => bindings.push(KeyBinding::new(keys, PrevTab, context)),
            "split-vertical" => bindings.push(KeyBinding::new(keys, SplitVertical, context)),
            "split-horizontal" => bindings.push(KeyBinding::new(keys, SplitHorizontal, context)),
            "close-pane" => bindings.push(KeyBinding::new(keys, ClosePane, context)),
            "focus-next-pane" => bindings.push(KeyBinding::new(keys, FocusNextPane, context)),
            "focus-prev-pane" => bindings.push(KeyBinding::new(keys, FocusPrevPane, context)),
            "open-settings" => bindings.push(KeyBinding::new(keys, OpenSettings, context)),
            "toggle-secure-input" => {
                bindings.push(KeyBinding::new(keys, ToggleSecureInput, context))
            }
            "toggle-option-as-alt" => {
                bindings.push(KeyBinding::new(keys, ToggleOptionAsAlt, context))
            }
            "search" => bindings.push(KeyBinding::new(keys, SearchToggle, context)),
            "search-next" => bindings.push(KeyBinding::new(keys, SearchNext, context)),
            "search-prev" => bindings.push(KeyBinding::new(keys, SearchPrev, context)),
            "search-toggle-regex" => {
                bindings.push(KeyBinding::new(keys, SearchToggleRegex, context))
            }
            "enter-copy-mode" => bindings.push(KeyBinding::new(keys, EnterCopyMode, context)),
            "exit-copy-mode" => bindings.push(KeyBinding::new(keys, ExitCopyMode, context)),
            "toggle-scratchpad" => bindings.push(KeyBinding::new(keys, ToggleScratchpad, context)),
            "toggle-command-palette" => {
                bindings.push(KeyBinding::new(keys, ToggleCommandPalette, context))
            }
            other => {
                tracing::warn!("Unknown keybinding action: '{}'", other);
            }
        }
    }

    if !bindings.is_empty() {
        tracing::info!("Applying {} custom keybinding(s)", bindings.len());
        cx.bind_keys(bindings);
    }
}

/// Callback for config file changes — re-applies custom keybindings.
fn on_keybinding_config_apply(config: &settings::Config, cx: &mut App) {
    apply_custom_keybindings(config, cx);
}

/// Initialize subsystems.
fn initialize_subsystems(cx: &mut App) {
    gpui_component::init(cx);
    debug!("UI components initialized");

    theme::init(cx);
    debug!("Theme system initialized");

    register_keybindings(cx);
    debug!("Keybindings registered");

    // Watch config for keybinding changes (re-applies custom bindings on reload)
    if let Some(debouncer) = settings::watch_config(cx, on_keybinding_config_apply) {
        Box::leak(Box::new(debouncer));
    }
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
