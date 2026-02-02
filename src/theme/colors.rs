//! Terminal color mapping from theme.
//!
//! Maps gpui-component theme colors to terminal ANSI colors.

use gpui::{rgb, App, Hsla, SharedString};
use gpui_component::theme::Theme;
use gpui_component::{ActiveTheme, Colorize};
use parking_lot::Mutex;

/// Cached terminal colors to avoid recomputation every frame
static TERMINAL_COLORS_CACHE: Mutex<Option<(SharedString, TerminalColors)>> = Mutex::new(None);

/// Terminal color palette mapped from theme
#[derive(Clone, Copy)]
pub struct TerminalColors {
    pub background: Hsla,
    pub foreground: Hsla,
    pub cursor: Hsla,
    pub selection: Hsla,
    // ANSI colors
    pub black: Hsla,
    pub red: Hsla,
    pub green: Hsla,
    pub yellow: Hsla,
    pub blue: Hsla,
    pub magenta: Hsla,
    pub cyan: Hsla,
    pub white: Hsla,
    // Bright ANSI colors
    pub bright_black: Hsla,
    pub bright_red: Hsla,
    pub bright_green: Hsla,
    pub bright_yellow: Hsla,
    pub bright_blue: Hsla,
    pub bright_magenta: Hsla,
    pub bright_cyan: Hsla,
    pub bright_white: Hsla,
    // UI colors
    pub title_bar: Hsla,
    pub tab_active: Hsla,
    pub tab_inactive: Hsla,
    pub border: Hsla,
    pub muted: Hsla,
    pub accent: Hsla,
}

impl Default for TerminalColors {
    fn default() -> Self {
        // Catppuccin Mocha fallback
        Self {
            background: rgb(0x1e1e2e).into(),
            foreground: rgb(0xcdd6f4).into(),
            cursor: rgb(0xf5e0dc).into(),
            selection: rgb(0x45475a).into(),
            black: rgb(0x45475a).into(),
            red: rgb(0xf38ba8).into(),
            green: rgb(0xa6e3a1).into(),
            yellow: rgb(0xf9e2af).into(),
            blue: rgb(0x89b4fa).into(),
            magenta: rgb(0xf5c2e7).into(),
            cyan: rgb(0x94e2d5).into(),
            white: rgb(0xbac2de).into(),
            bright_black: rgb(0x585b70).into(),
            bright_red: rgb(0xf38ba8).into(),
            bright_green: rgb(0xa6e3a1).into(),
            bright_yellow: rgb(0xf9e2af).into(),
            bright_blue: rgb(0x89b4fa).into(),
            bright_magenta: rgb(0xf5c2e7).into(),
            bright_cyan: rgb(0x94e2d5).into(),
            bright_white: rgb(0xa6adc8).into(),
            title_bar: rgb(0x181825).into(),
            tab_active: rgb(0x313244).into(),
            tab_inactive: rgb(0x1e1e2e).into(),
            border: rgb(0x313244).into(),
            muted: rgb(0x6c7086).into(),
            accent: rgb(0x89b4fa).into(),
        }
    }
}

/// Get terminal colors from the current theme (cached)
/// Maps gpui-component theme colors to terminal ANSI colors
pub fn terminal_colors(cx: &App) -> TerminalColors {
    let current_theme = cx.theme().theme_name().clone();

    // Fast path: return cached colors if theme hasn't changed
    {
        let cache = TERMINAL_COLORS_CACHE.lock();
        if let Some((cached_theme, cached_colors)) = cache.as_ref() {
            if cached_theme == &current_theme {
                return *cached_colors;
            }
        }
    }

    // Slow path: compute colors and cache them
    let theme = Theme::global(cx);
    let colors = &theme.colors;

    let terminal_colors = TerminalColors {
        background: colors.background,
        foreground: colors.foreground,
        cursor: colors.caret,
        selection: colors.selection,
        black: colors.background.darken(0.3),
        red: colors.red,
        green: colors.green,
        yellow: colors.yellow,
        blue: colors.blue,
        magenta: colors.magenta,
        cyan: colors.cyan,
        white: colors.foreground.lighten(0.1),
        bright_black: colors.muted_foreground,
        bright_red: colors.red_light,
        bright_green: colors.green_light,
        bright_yellow: colors.yellow_light,
        bright_blue: colors.blue_light,
        bright_magenta: colors.magenta_light,
        bright_cyan: colors.cyan_light,
        bright_white: colors.foreground.lighten(0.2),
        // UI colors
        title_bar: colors.title_bar,
        tab_active: colors.tab_active,
        tab_inactive: colors.tab,
        border: colors.border,
        muted: colors.muted_foreground,
        accent: colors.accent,
    };

    // Update cache
    *TERMINAL_COLORS_CACHE.lock() = Some((current_theme, terminal_colors));

    terminal_colors
}
