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
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use test_case::test_case;

    // ==================== Original Tests ====================

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

    // ==================== Property-Based Tests ====================

    // Small epsilon for floating point comparisons
    const FLOAT_EPSILON: f32 = 1e-5;

    proptest! {
        /// Property: RGB to HSLA conversion should produce valid HSLA values
        #[test]
        fn prop_rgb_to_hsla_produces_valid_hsla(r in 0u8..=255, g in 0u8..=255, b in 0u8..=255) {
            let rgb = Rgb { r, g, b };
            let result = rgb_to_hsla(rgb);

            // HSLA values should be in valid ranges (with small epsilon for float imprecision)
            prop_assert!(result.h >= -FLOAT_EPSILON && result.h <= 1.0 + FLOAT_EPSILON, "Hue out of range: {}", result.h);
            prop_assert!(result.s >= -FLOAT_EPSILON && result.s <= 1.0 + FLOAT_EPSILON, "Saturation out of range: {}", result.s);
            prop_assert!(result.l >= -FLOAT_EPSILON && result.l <= 1.0 + FLOAT_EPSILON, "Lightness out of range: {}", result.l);
            prop_assert_eq!(result.a, 1.0, "Alpha should always be 1.0");
        }

        /// Property: apply_dim should always reduce or maintain lightness (never increase)
        #[test]
        fn prop_apply_dim_reduces_lightness(h in 0.0f32..=1.0, s in 0.0f32..=1.0, l in 0.0f32..=1.0) {
            let original = hsla(h, s, l, 1.0);
            let dimmed = apply_dim(original);

            prop_assert!(dimmed.l <= original.l, "Dimmed lightness {} should be <= original {}", dimmed.l, original.l);
            prop_assert!((dimmed.l - original.l * 0.66).abs() < 0.001, "Lightness should be exactly 66% of original");
        }

        /// Property: apply_dim should preserve hue, saturation, and alpha
        #[test]
        fn prop_apply_dim_preserves_other_components(h in 0.0f32..=1.0, s in 0.0f32..=1.0, l in 0.0f32..=1.0, a in 0.0f32..=1.0) {
            let original = hsla(h, s, l, a);
            let dimmed = apply_dim(original);

            prop_assert_eq!(dimmed.h, original.h, "Hue should be preserved");
            prop_assert_eq!(dimmed.s, original.s, "Saturation should be preserved");
            prop_assert_eq!(dimmed.a, original.a, "Alpha should be preserved");
        }

        /// Property: Grayscale values (equal R, G, B) should have zero saturation
        #[test]
        fn prop_grayscale_rgb_has_zero_saturation(gray in 0u8..=255) {
            let rgb = Rgb { r: gray, g: gray, b: gray };
            let result = rgb_to_hsla(rgb);

            prop_assert!(result.s.abs() < 0.001, "Grayscale should have ~0 saturation, got {}", result.s);
        }

        /// Property: indexed_color_to_hsla should always produce valid HSLA for any index
        #[test]
        fn prop_indexed_color_always_valid(idx in 0u8..=255) {
            let colors = TerminalColors::default();
            let result = indexed_color_to_hsla(idx, &colors);

            // Allow small epsilon for floating point imprecision
            prop_assert!(result.h >= -FLOAT_EPSILON && result.h <= 1.0 + FLOAT_EPSILON, "Hue out of range for idx {}: {}", idx, result.h);
            prop_assert!(result.s >= -FLOAT_EPSILON && result.s <= 1.0 + FLOAT_EPSILON, "Saturation out of range for idx {}: {}", idx, result.s);
            prop_assert!(result.l >= -FLOAT_EPSILON && result.l <= 1.0 + FLOAT_EPSILON, "Lightness out of range for idx {}: {}", idx, result.l);
            prop_assert_eq!(result.a, 1.0, "Alpha should always be 1.0 for idx {}", idx);
        }

        /// Property: Color cube indices (16-231) should produce unique colors for different indices
        #[test]
        fn prop_color_cube_produces_distinct_colors(idx1 in 16u8..=231, idx2 in 16u8..=231) {
            prop_assume!(idx1 != idx2);
            let colors = TerminalColors::default();
            let color1 = indexed_color_to_hsla(idx1, &colors);
            let color2 = indexed_color_to_hsla(idx2, &colors);

            // At least one component should differ
            let h_diff = (color1.h - color2.h).abs();
            let s_diff = (color1.s - color2.s).abs();
            let l_diff = (color1.l - color2.l).abs();

            prop_assert!(
                h_diff > 0.001 || s_diff > 0.001 || l_diff > 0.001,
                "Indices {} and {} should produce different colors", idx1, idx2
            );
        }
    }

    // ==================== Parameterized Tests for All 256 Indexed Colors ====================

    // Test ANSI colors (0-15)
    #[test_case(0, "black" ; "index_0_black")]
    #[test_case(1, "red" ; "index_1_red")]
    #[test_case(2, "green" ; "index_2_green")]
    #[test_case(3, "yellow" ; "index_3_yellow")]
    #[test_case(4, "blue" ; "index_4_blue")]
    #[test_case(5, "magenta" ; "index_5_magenta")]
    #[test_case(6, "cyan" ; "index_6_cyan")]
    #[test_case(7, "white" ; "index_7_white")]
    #[test_case(8, "bright_black" ; "index_8_bright_black")]
    #[test_case(9, "bright_red" ; "index_9_bright_red")]
    #[test_case(10, "bright_green" ; "index_10_bright_green")]
    #[test_case(11, "bright_yellow" ; "index_11_bright_yellow")]
    #[test_case(12, "bright_blue" ; "index_12_bright_blue")]
    #[test_case(13, "bright_magenta" ; "index_13_bright_magenta")]
    #[test_case(14, "bright_cyan" ; "index_14_bright_cyan")]
    #[test_case(15, "bright_white" ; "index_15_bright_white")]
    fn test_ansi_colors_produce_valid_hsla(idx: u8, _name: &str) {
        let colors = TerminalColors::default();
        let result = indexed_color_to_hsla(idx, &colors);

        assert!(result.h >= 0.0 && result.h <= 1.0, "Hue out of range");
        assert!(
            result.s >= 0.0 && result.s <= 1.0,
            "Saturation out of range"
        );
        assert!(result.l >= 0.0 && result.l <= 1.0, "Lightness out of range");
        assert_eq!(result.a, 1.0, "Alpha should be 1.0");
    }

    // Test color cube corners (16-231)
    #[test_case(16, 0.0, 0.0, 0.0 ; "color_cube_origin_black")]
    #[test_case(21, 0.0, 0.0, 1.0 ; "color_cube_pure_blue")]
    #[test_case(46, 0.0, 1.0, 0.0 ; "color_cube_pure_green")]
    #[test_case(51, 0.0, 1.0, 1.0 ; "color_cube_cyan")]
    #[test_case(196, 1.0, 0.0, 0.0 ; "color_cube_pure_red")]
    #[test_case(201, 1.0, 0.0, 1.0 ; "color_cube_magenta")]
    #[test_case(226, 1.0, 1.0, 0.0 ; "color_cube_yellow")]
    #[test_case(231, 1.0, 1.0, 1.0 ; "color_cube_white")]
    fn test_color_cube_corners(idx: u8, expected_r: f32, expected_g: f32, expected_b: f32) {
        let colors = TerminalColors::default();
        let result = indexed_color_to_hsla(idx, &colors);

        // Convert back to check RGB components
        // For corner cases, we can verify specific properties
        let rgba = Rgba::from(result);

        assert!(
            (rgba.r - expected_r).abs() < 0.01,
            "Red component mismatch for idx {}: expected {}, got {}",
            idx,
            expected_r,
            rgba.r
        );
        assert!(
            (rgba.g - expected_g).abs() < 0.01,
            "Green component mismatch for idx {}: expected {}, got {}",
            idx,
            expected_g,
            rgba.g
        );
        assert!(
            (rgba.b - expected_b).abs() < 0.01,
            "Blue component mismatch for idx {}: expected {}, got {}",
            idx,
            expected_b,
            rgba.b
        );
    }

    // Test grayscale ramp (232-255)
    #[test_case(232 ; "grayscale_232_darkest")]
    #[test_case(237 ; "grayscale_237")]
    #[test_case(243 ; "grayscale_243_mid")]
    #[test_case(249 ; "grayscale_249")]
    #[test_case(255 ; "grayscale_255_lightest")]
    fn test_grayscale_has_zero_saturation(idx: u8) {
        let colors = TerminalColors::default();
        let result = indexed_color_to_hsla(idx, &colors);

        assert!(
            result.s.abs() < 0.001,
            "Grayscale index {} should have zero saturation, got {}",
            idx,
            result.s
        );
    }

    #[test]
    fn test_grayscale_monotonically_increasing() {
        let colors = TerminalColors::default();
        let mut prev_lightness = 0.0f32;

        for idx in 232..=255 {
            let result = indexed_color_to_hsla(idx, &colors);
            assert!(
                result.l >= prev_lightness,
                "Grayscale should be monotonically increasing: idx {} has lightness {} but previous was {}",
                idx,
                result.l,
                prev_lightness
            );
            prev_lightness = result.l;
        }
    }

    // ==================== Edge Case Tests ====================

    #[test]
    fn test_rgb_boundary_values() {
        // Minimum values (black)
        let black = rgb_to_hsla(Rgb { r: 0, g: 0, b: 0 });
        assert!(black.l < 0.01, "Black should have near-zero lightness");

        // Maximum values (white)
        let white = rgb_to_hsla(Rgb {
            r: 255,
            g: 255,
            b: 255,
        });
        assert!(white.l > 0.99, "White should have near-one lightness");

        // Pure colors at max intensity
        let pure_red = rgb_to_hsla(Rgb { r: 255, g: 0, b: 0 });
        let pure_green = rgb_to_hsla(Rgb { r: 0, g: 255, b: 0 });
        let pure_blue = rgb_to_hsla(Rgb { r: 0, g: 0, b: 255 });

        // All pure colors should have saturation of 1.0
        assert!((pure_red.s - 1.0).abs() < 0.01, "Pure red saturation");
        assert!((pure_green.s - 1.0).abs() < 0.01, "Pure green saturation");
        assert!((pure_blue.s - 1.0).abs() < 0.01, "Pure blue saturation");
    }

    #[test]
    fn test_indexed_color_boundary_indices() {
        let colors = TerminalColors::default();

        // First index
        let first = indexed_color_to_hsla(0, &colors);
        assert!(first.a == 1.0, "Alpha should be 1.0");

        // Last ANSI color
        let last_ansi = indexed_color_to_hsla(15, &colors);
        assert!(last_ansi.a == 1.0, "Alpha should be 1.0");

        // First color cube
        let first_cube = indexed_color_to_hsla(16, &colors);
        assert!(first_cube.a == 1.0, "Alpha should be 1.0");

        // Last color cube
        let last_cube = indexed_color_to_hsla(231, &colors);
        assert!(last_cube.a == 1.0, "Alpha should be 1.0");

        // First grayscale
        let first_gray = indexed_color_to_hsla(232, &colors);
        assert!(first_gray.a == 1.0, "Alpha should be 1.0");

        // Last grayscale (and last index)
        let last_gray = indexed_color_to_hsla(255, &colors);
        assert!(last_gray.a == 1.0, "Alpha should be 1.0");
    }

    #[test]
    fn test_apply_dim_boundary_values() {
        // Zero lightness should remain zero
        let black = hsla(0.0, 0.0, 0.0, 1.0);
        let dimmed_black = apply_dim(black);
        assert_eq!(dimmed_black.l, 0.0, "Dimming black should stay black");

        // Full lightness
        let white = hsla(0.0, 0.0, 1.0, 1.0);
        let dimmed_white = apply_dim(white);
        assert!(
            (dimmed_white.l - 0.66).abs() < 0.01,
            "Dimming white should give 0.66 lightness"
        );

        // Zero alpha should be preserved
        let transparent = hsla(0.5, 0.5, 0.5, 0.0);
        let dimmed_transparent = apply_dim(transparent);
        assert_eq!(dimmed_transparent.a, 0.0, "Alpha 0 should be preserved");

        // Full alpha should be preserved
        let opaque = hsla(0.5, 0.5, 0.5, 1.0);
        let dimmed_opaque = apply_dim(opaque);
        assert_eq!(dimmed_opaque.a, 1.0, "Alpha 1 should be preserved");
    }

    #[test]
    fn test_color_cube_6x6x6_structure() {
        let colors = TerminalColors::default();

        // Verify the 6x6x6 cube structure
        // Index = 16 + 36*r + 6*g + b where r,g,b are in 0..6
        for r in 0..6u8 {
            for g in 0..6u8 {
                for b in 0..6u8 {
                    let idx = 16 + 36 * r + 6 * g + b;
                    let result = indexed_color_to_hsla(idx, &colors);

                    // Verify the result is valid (with small epsilon for float imprecision)
                    assert!(
                        result.h >= -FLOAT_EPSILON && result.h <= 1.0 + FLOAT_EPSILON,
                        "Invalid hue for cube index ({}, {}, {})",
                        r,
                        g,
                        b
                    );
                    assert!(
                        result.s >= -FLOAT_EPSILON && result.s <= 1.0 + FLOAT_EPSILON,
                        "Invalid saturation for cube index ({}, {}, {})",
                        r,
                        g,
                        b
                    );
                    assert!(
                        result.l >= -FLOAT_EPSILON && result.l <= 1.0 + FLOAT_EPSILON,
                        "Invalid lightness for cube index ({}, {}, {})",
                        r,
                        g,
                        b
                    );
                }
            }
        }
    }

    // ==================== Named Color Tests ====================

    #[test]
    fn test_all_named_colors_produce_valid_hsla() {
        let colors = TerminalColors::default();

        let named_colors = [
            NamedColor::Black,
            NamedColor::Red,
            NamedColor::Green,
            NamedColor::Yellow,
            NamedColor::Blue,
            NamedColor::Magenta,
            NamedColor::Cyan,
            NamedColor::White,
            NamedColor::BrightBlack,
            NamedColor::BrightRed,
            NamedColor::BrightGreen,
            NamedColor::BrightYellow,
            NamedColor::BrightBlue,
            NamedColor::BrightMagenta,
            NamedColor::BrightCyan,
            NamedColor::BrightWhite,
            NamedColor::Foreground,
            NamedColor::Background,
            NamedColor::Cursor,
        ];

        for named in named_colors {
            let result = named_color_to_hsla(named, &colors);
            assert!(
                result.h >= 0.0 && result.h <= 1.0,
                "Invalid hue for {:?}",
                named
            );
            assert!(
                result.s >= 0.0 && result.s <= 1.0,
                "Invalid saturation for {:?}",
                named
            );
            assert!(
                result.l >= 0.0 && result.l <= 1.0,
                "Invalid lightness for {:?}",
                named
            );
            assert_eq!(result.a, 1.0, "Alpha should be 1.0 for {:?}", named);
        }
    }

    // ==================== get_bright_color Tests ====================

    #[test]
    fn test_get_bright_color_basic_colors() {
        let term_colors = TermColors::default();
        let theme = TerminalColors::default();

        // Test that basic colors get converted to their bright variants
        let basic_colors = [
            (NamedColor::Black, NamedColor::BrightBlack),
            (NamedColor::Red, NamedColor::BrightRed),
            (NamedColor::Green, NamedColor::BrightGreen),
            (NamedColor::Yellow, NamedColor::BrightYellow),
            (NamedColor::Blue, NamedColor::BrightBlue),
            (NamedColor::Magenta, NamedColor::BrightMagenta),
            (NamedColor::Cyan, NamedColor::BrightCyan),
            (NamedColor::White, NamedColor::BrightWhite),
        ];

        for (basic, bright) in basic_colors {
            let bright_result = get_bright_color(Color::Named(basic), &term_colors, &theme);
            let expected = named_color_to_hsla(bright, &theme);

            assert_eq!(
                bright_result, expected,
                "Bright variant of {:?} should match {:?}",
                basic, bright
            );
        }
    }

    #[test]
    fn test_get_bright_color_indexed_0_to_7() {
        let term_colors = TermColors::default();
        let theme = TerminalColors::default();

        // Indexed colors 0-7 should map to 8-15 (bright variants)
        for idx in 0..8u8 {
            let bright_result = get_bright_color(Color::Indexed(idx), &term_colors, &theme);
            let expected = indexed_color_to_hsla(idx + 8, &theme);

            assert_eq!(
                bright_result,
                expected,
                "Indexed {} should brighten to indexed {}",
                idx,
                idx + 8
            );
        }
    }

    #[test]
    fn test_get_bright_color_passes_through_other_colors() {
        let term_colors = TermColors::default();
        let theme = TerminalColors::default();

        // Already bright colors should pass through
        let bright_color = Color::Named(NamedColor::BrightRed);
        let result = get_bright_color(bright_color, &term_colors, &theme);
        let expected = color_to_hsla(bright_color, &term_colors, &theme);
        assert_eq!(
            result, expected,
            "Already bright colors should pass through"
        );

        // Spec colors should pass through
        let spec_color = Color::Spec(Rgb {
            r: 128,
            g: 64,
            b: 192,
        });
        let result = get_bright_color(spec_color, &term_colors, &theme);
        let expected = color_to_hsla(spec_color, &term_colors, &theme);
        assert_eq!(result, expected, "Spec colors should pass through");

        // Indexed colors 8+ should pass through
        for idx in 8..=255u8 {
            let indexed_color = Color::Indexed(idx);
            let result = get_bright_color(indexed_color, &term_colors, &theme);
            let expected = color_to_hsla(indexed_color, &term_colors, &theme);
            assert_eq!(
                result, expected,
                "Indexed {} should pass through unchanged",
                idx
            );
        }
    }

    // ==================== color_to_hsla Integration Tests ====================

    #[test]
    fn test_color_to_hsla_named_without_custom_colors() {
        let term_colors = TermColors::default();
        let theme = TerminalColors::default();

        // Without custom terminal colors, should use theme
        let result = color_to_hsla(Color::Named(NamedColor::Red), &term_colors, &theme);
        let expected = theme.red;
        assert_eq!(
            result, expected,
            "Should use theme color when no custom set"
        );
    }

    #[test]
    fn test_color_to_hsla_spec_color() {
        let term_colors = TermColors::default();
        let theme = TerminalColors::default();

        let rgb = Rgb {
            r: 100,
            g: 150,
            b: 200,
        };
        let result = color_to_hsla(Color::Spec(rgb), &term_colors, &theme);
        let expected = rgb_to_hsla(rgb);

        assert_eq!(result, expected, "Spec color should be converted directly");
    }

    #[test]
    fn test_color_to_hsla_indexed_without_custom_colors() {
        let term_colors = TermColors::default();
        let theme = TerminalColors::default();

        for idx in 0..=255u8 {
            let result = color_to_hsla(Color::Indexed(idx), &term_colors, &theme);
            let expected = indexed_color_to_hsla(idx, &theme);
            assert_eq!(
                result, expected,
                "Indexed {} should use theme fallback",
                idx
            );
        }
    }
}
