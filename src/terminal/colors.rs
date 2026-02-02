//! Terminal color conversion utilities.
//!
//! Converts between alacritty_terminal colors and GPUI colors.
//! Handles:
//! - RGB to Hsla conversion
//! - Named ANSI colors (0-15)
//! - 256-color indexed palette (16-255)
//! - Theme fallbacks when terminal hasn't set custom colors

use crate::theme::TerminalColors;
use alacritty_terminal::term::color::Colors as TermColors;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};
use gpui::{hsla, Hsla, Rgba};

/// Convert RGB to Hsla.
pub fn rgb_to_hsla(rgb: Rgb) -> Hsla {
    Hsla::from(Rgba {
        r: rgb.r as f32 / 255.0,
        g: rgb.g as f32 / 255.0,
        b: rgb.b as f32 / 255.0,
        a: 1.0,
    })
}

/// Convert alacritty color to GPUI Hsla using terminal colors with theme fallbacks.
pub fn color_to_hsla(color: Color, term_colors: &TermColors, theme: &TerminalColors) -> Hsla {
    match color {
        Color::Named(named) => {
            // Check if terminal has custom color set, otherwise use theme
            if let Some(rgb) = term_colors[named] {
                rgb_to_hsla(rgb)
            } else {
                named_color_to_hsla(named, theme)
            }
        }
        Color::Spec(rgb) => rgb_to_hsla(rgb),
        Color::Indexed(idx) => {
            // Check if terminal has custom color set
            if let Some(rgb) = term_colors[idx as usize] {
                rgb_to_hsla(rgb)
            } else {
                indexed_color_to_hsla(idx, theme)
            }
        }
    }
}

/// Convert a named ANSI color to Hsla using theme colors.
pub fn named_color_to_hsla(color: NamedColor, colors: &TerminalColors) -> Hsla {
    match color {
        NamedColor::Black => colors.black,
        NamedColor::Red => colors.red,
        NamedColor::Green => colors.green,
        NamedColor::Yellow => colors.yellow,
        NamedColor::Blue => colors.blue,
        NamedColor::Magenta => colors.magenta,
        NamedColor::Cyan => colors.cyan,
        NamedColor::White => colors.white,
        NamedColor::BrightBlack => colors.bright_black,
        NamedColor::BrightRed => colors.bright_red,
        NamedColor::BrightGreen => colors.bright_green,
        NamedColor::BrightYellow => colors.bright_yellow,
        NamedColor::BrightBlue => colors.bright_blue,
        NamedColor::BrightMagenta => colors.bright_magenta,
        NamedColor::BrightCyan => colors.bright_cyan,
        NamedColor::BrightWhite => colors.bright_white,
        NamedColor::Foreground => colors.foreground,
        NamedColor::Background => colors.background,
        NamedColor::Cursor => colors.cursor,
        _ => colors.foreground,
    }
}

/// Convert an indexed color (0-255) to Hsla.
///
/// The 256-color palette is organized as:
/// - 0-15: Named ANSI colors
/// - 16-231: 6x6x6 color cube
/// - 232-255: 24-step grayscale
pub fn indexed_color_to_hsla(idx: u8, colors: &TerminalColors) -> Hsla {
    match idx {
        0..=15 => {
            let named = match idx {
                0 => NamedColor::Black,
                1 => NamedColor::Red,
                2 => NamedColor::Green,
                3 => NamedColor::Yellow,
                4 => NamedColor::Blue,
                5 => NamedColor::Magenta,
                6 => NamedColor::Cyan,
                7 => NamedColor::White,
                8 => NamedColor::BrightBlack,
                9 => NamedColor::BrightRed,
                10 => NamedColor::BrightGreen,
                11 => NamedColor::BrightYellow,
                12 => NamedColor::BrightBlue,
                13 => NamedColor::BrightMagenta,
                14 => NamedColor::BrightCyan,
                15 => NamedColor::BrightWhite,
                _ => NamedColor::Foreground,
            };
            named_color_to_hsla(named, colors)
        }
        16..=231 => {
            // 6x6x6 color cube
            let idx = idx - 16;
            let r = (idx / 36) as f32 / 5.0;
            let g = ((idx % 36) / 6) as f32 / 5.0;
            let b = (idx % 6) as f32 / 5.0;
            Hsla::from(Rgba { r, g, b, a: 1.0 })
        }
        232..=255 => {
            // Grayscale
            let gray = (idx - 232) as f32 / 23.0 * 0.9 + 0.08;
            hsla(0.0, 0.0, gray, 1.0)
        }
    }
}

/// Apply DIM flag - reduce brightness by 33%.
pub fn apply_dim(color: Hsla) -> Hsla {
    hsla(color.h, color.s, color.l * 0.66, color.a)
}

/// Get bright variant of a color.
///
/// Used when BOLD flag is set on a named color to get its bright variant.
pub fn get_bright_color(color: Color, term_colors: &TermColors, theme: &TerminalColors) -> Hsla {
    match color {
        Color::Named(NamedColor::Black) => term_colors[NamedColor::BrightBlack]
            .map(rgb_to_hsla)
            .unwrap_or(theme.bright_black),
        Color::Named(NamedColor::Red) => term_colors[NamedColor::BrightRed]
            .map(rgb_to_hsla)
            .unwrap_or(theme.bright_red),
        Color::Named(NamedColor::Green) => term_colors[NamedColor::BrightGreen]
            .map(rgb_to_hsla)
            .unwrap_or(theme.bright_green),
        Color::Named(NamedColor::Yellow) => term_colors[NamedColor::BrightYellow]
            .map(rgb_to_hsla)
            .unwrap_or(theme.bright_yellow),
        Color::Named(NamedColor::Blue) => term_colors[NamedColor::BrightBlue]
            .map(rgb_to_hsla)
            .unwrap_or(theme.bright_blue),
        Color::Named(NamedColor::Magenta) => term_colors[NamedColor::BrightMagenta]
            .map(rgb_to_hsla)
            .unwrap_or(theme.bright_magenta),
        Color::Named(NamedColor::Cyan) => term_colors[NamedColor::BrightCyan]
            .map(rgb_to_hsla)
            .unwrap_or(theme.bright_cyan),
        Color::Named(NamedColor::White) => term_colors[NamedColor::BrightWhite]
            .map(rgb_to_hsla)
            .unwrap_or(theme.bright_white),
        Color::Indexed(idx) if idx < 8 => {
            // Convert 0-7 to bright variants (8-15)
            let bright_idx = idx + 8;
            term_colors[bright_idx as usize]
                .map(rgb_to_hsla)
                .unwrap_or_else(|| indexed_color_to_hsla(bright_idx, theme))
        }
        other => color_to_hsla(other, term_colors, theme),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rgb_to_hsla() {
        let rgb = Rgb { r: 255, g: 0, b: 0 };
        let hsla = rgb_to_hsla(rgb);
        // Red should be approximately hue 0, saturation 1, lightness 0.5
        assert!((hsla.h - 0.0).abs() < 0.01 || (hsla.h - 1.0).abs() < 0.01);
        assert!((hsla.s - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_indexed_grayscale() {
        // Test grayscale ramp (232-255)
        let dark = indexed_color_to_hsla(232, &TerminalColors::default());
        let light = indexed_color_to_hsla(255, &TerminalColors::default());

        // Darker should have lower lightness than lighter
        assert!(dark.l < light.l);
    }

    #[test]
    fn test_apply_dim() {
        let original = hsla(0.5, 0.5, 0.5, 1.0);
        let dimmed = apply_dim(original);

        // Lightness should be reduced to 66%
        assert!((dimmed.l - original.l * 0.66).abs() < 0.01);
        // Other properties should be unchanged
        assert_eq!(dimmed.h, original.h);
        assert_eq!(dimmed.s, original.s);
        assert_eq!(dimmed.a, original.a);
    }
}
