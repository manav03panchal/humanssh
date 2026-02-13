//! Terminal color conversion utilities.
//!
//! Converts between alacritty_terminal colors and GPUI colors.
//! Handles:
//! - RGB to Hsla conversion
//! - Named ANSI colors (0-15)
//! - 256-color indexed palette (16-255)
//! - Theme fallbacks when terminal hasn't set custom colors

use alacritty_terminal::term::color::Colors as TermColors;
use alacritty_terminal::vte::ansi::{Color, NamedColor, Rgb};
use gpui::{hsla, Hsla, Rgba};
use theme::TerminalColors;

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

    // ==================== Boundary Condition Tests ====================

    // --- RGB Boundary Tests (0, 127, 255) ---

    #[test_case(0, 0, 0 ; "rgb_black_minimum")]
    #[test_case(255, 255, 255 ; "rgb_white_maximum")]
    #[test_case(127, 127, 127 ; "rgb_mid_gray")]
    #[test_case(0, 0, 255 ; "rgb_pure_blue_max")]
    #[test_case(0, 255, 0 ; "rgb_pure_green_max")]
    #[test_case(255, 0, 0 ; "rgb_pure_red_max")]
    #[test_case(0, 0, 1 ; "rgb_near_black_blue")]
    #[test_case(0, 1, 0 ; "rgb_near_black_green")]
    #[test_case(1, 0, 0 ; "rgb_near_black_red")]
    #[test_case(254, 255, 255 ; "rgb_near_white_cyan")]
    #[test_case(255, 254, 255 ; "rgb_near_white_magenta")]
    #[test_case(255, 255, 254 ; "rgb_near_white_yellow")]
    fn test_rgb_to_hsla_boundary_values(r: u8, g: u8, b: u8) {
        let rgb = Rgb { r, g, b };
        let result = rgb_to_hsla(rgb);

        // Allow small floating point tolerance for edge cases
        // GPUI's color conversion can produce values slightly outside [0,1] range
        let epsilon = 1e-4;

        // All results should be valid HSLA (with tolerance for floating point precision)
        assert!(
            result.h >= -epsilon && result.h <= 1.0 + epsilon,
            "Hue out of range for ({}, {}, {}): got {}",
            r,
            g,
            b,
            result.h
        );
        assert!(
            result.s >= -epsilon && result.s <= 1.0 + epsilon,
            "Saturation out of range for ({}, {}, {}): got {}",
            r,
            g,
            b,
            result.s
        );
        assert!(
            result.l >= -epsilon && result.l <= 1.0 + epsilon,
            "Lightness out of range for ({}, {}, {}): got {}",
            r,
            g,
            b,
            result.l
        );
        assert!(
            (result.a - 1.0).abs() < epsilon,
            "Alpha should be ~1.0, got {}",
            result.a
        );
    }

    #[test]
    fn test_rgb_0_produces_black() {
        let rgb = Rgb { r: 0, g: 0, b: 0 };
        let result = rgb_to_hsla(rgb);
        assert!(
            result.l < 0.001,
            "Black should have lightness ~0, got {}",
            result.l
        );
    }

    #[test]
    fn test_rgb_255_produces_white() {
        let rgb = Rgb {
            r: 255,
            g: 255,
            b: 255,
        };
        let result = rgb_to_hsla(rgb);
        assert!(
            result.l > 0.999,
            "White should have lightness ~1, got {}",
            result.l
        );
    }

    #[test]
    fn test_rgb_127_produces_mid_lightness() {
        let rgb = Rgb {
            r: 127,
            g: 127,
            b: 127,
        };
        let result = rgb_to_hsla(rgb);
        // Mid gray should have lightness around 0.5
        assert!(
            (result.l - 0.498).abs() < 0.01,
            "Mid gray should have lightness ~0.5, got {}",
            result.l
        );
        // And zero saturation
        assert!(
            result.s < 0.001,
            "Gray should have saturation ~0, got {}",
            result.s
        );
    }

    #[test]
    fn test_rgb_boundary_pure_colors_saturation() {
        // Pure red, green, blue at max intensity should have saturation 1.0
        let pure_red = rgb_to_hsla(Rgb { r: 255, g: 0, b: 0 });
        let pure_green = rgb_to_hsla(Rgb { r: 0, g: 255, b: 0 });
        let pure_blue = rgb_to_hsla(Rgb { r: 0, g: 0, b: 255 });

        assert!(
            (pure_red.s - 1.0).abs() < 0.001,
            "Pure red saturation should be 1.0"
        );
        assert!(
            (pure_green.s - 1.0).abs() < 0.001,
            "Pure green saturation should be 1.0"
        );
        assert!(
            (pure_blue.s - 1.0).abs() < 0.001,
            "Pure blue saturation should be 1.0"
        );
    }

    #[test]
    fn test_rgb_boundary_secondary_colors() {
        // Cyan, magenta, yellow at max intensity
        let cyan = rgb_to_hsla(Rgb {
            r: 0,
            g: 255,
            b: 255,
        });
        let magenta = rgb_to_hsla(Rgb {
            r: 255,
            g: 0,
            b: 255,
        });
        let yellow = rgb_to_hsla(Rgb {
            r: 255,
            g: 255,
            b: 0,
        });

        assert!(
            (cyan.s - 1.0).abs() < 0.001,
            "Cyan saturation should be 1.0"
        );
        assert!(
            (magenta.s - 1.0).abs() < 0.001,
            "Magenta saturation should be 1.0"
        );
        assert!(
            (yellow.s - 1.0).abs() < 0.001,
            "Yellow saturation should be 1.0"
        );
    }

    // --- Indexed Color Boundary Tests (0, 15, 16, 231, 232, 255) ---

    #[test]
    fn test_indexed_color_0_black() {
        let colors = TerminalColors::default();
        let result = indexed_color_to_hsla(0, &colors);
        assert_eq!(result, colors.black, "Index 0 should be black");
    }

    #[test]
    fn test_indexed_color_15_bright_white() {
        let colors = TerminalColors::default();
        let result = indexed_color_to_hsla(15, &colors);
        assert_eq!(
            result, colors.bright_white,
            "Index 15 should be bright white"
        );
    }

    #[test]
    fn test_indexed_color_16_first_color_cube() {
        let colors = TerminalColors::default();
        let result = indexed_color_to_hsla(16, &colors);
        // Index 16 is the origin of the 6x6x6 cube (r=0, g=0, b=0 in cube)
        let rgba = Rgba::from(result);
        assert!(rgba.r < 0.01, "First cube color R should be ~0");
        assert!(rgba.g < 0.01, "First cube color G should be ~0");
        assert!(rgba.b < 0.01, "First cube color B should be ~0");
    }

    #[test]
    fn test_indexed_color_231_last_color_cube() {
        let colors = TerminalColors::default();
        let result = indexed_color_to_hsla(231, &colors);
        // Index 231 is r=5, g=5, b=5 (white in the cube)
        let rgba = Rgba::from(result);
        assert!(rgba.r > 0.99, "Last cube color R should be ~1");
        assert!(rgba.g > 0.99, "Last cube color G should be ~1");
        assert!(rgba.b > 0.99, "Last cube color B should be ~1");
    }

    #[test]
    fn test_indexed_color_232_first_grayscale() {
        let colors = TerminalColors::default();
        let result = indexed_color_to_hsla(232, &colors);
        // Should be dark gray with zero saturation
        assert!(
            result.s < 0.001,
            "First grayscale should have zero saturation"
        );
        // Should be the darkest grayscale
        assert!(result.l < 0.15, "First grayscale should be dark");
    }

    #[test]
    fn test_indexed_color_255_last_grayscale() {
        let colors = TerminalColors::default();
        let result = indexed_color_to_hsla(255, &colors);
        // Should be light gray with zero saturation
        assert!(
            result.s < 0.001,
            "Last grayscale should have zero saturation"
        );
        // Should be the lightest grayscale
        assert!(
            result.l > 0.85,
            "Last grayscale should be light, got {}",
            result.l
        );
    }

    #[test_case(0, "black" ; "idx_0_ansi_black")]
    #[test_case(7, "white" ; "idx_7_ansi_white")]
    #[test_case(8, "bright_black" ; "idx_8_bright_black")]
    #[test_case(15, "bright_white" ; "idx_15_bright_white")]
    #[test_case(16, "cube_origin" ; "idx_16_cube_start")]
    #[test_case(231, "cube_max" ; "idx_231_cube_end")]
    #[test_case(232, "gray_start" ; "idx_232_gray_start")]
    #[test_case(255, "gray_end" ; "idx_255_gray_end")]
    fn test_indexed_color_boundary_indices_parameterized(idx: u8, _name: &str) {
        let colors = TerminalColors::default();
        let result = indexed_color_to_hsla(idx, &colors);

        // All should produce valid HSLA
        assert!(
            result.h >= 0.0 && result.h <= 1.0,
            "Hue out of range for idx {}",
            idx
        );
        assert!(
            result.s >= 0.0 && result.s <= 1.0,
            "Saturation out of range for idx {}",
            idx
        );
        assert!(
            result.l >= 0.0 && result.l <= 1.0,
            "Lightness out of range for idx {}",
            idx
        );
        assert_eq!(result.a, 1.0, "Alpha should be 1.0 for idx {}", idx);
    }

    // --- Grayscale Edge Indices Tests ---

    #[test]
    fn test_grayscale_range_all_indices() {
        let colors = TerminalColors::default();
        let mut prev_lightness = -1.0f32;

        for idx in 232..=255 {
            let result = indexed_color_to_hsla(idx, &colors);

            // All grayscale should have zero saturation
            assert!(
                result.s < 0.001,
                "Grayscale {} should have zero saturation",
                idx
            );

            // Lightness should be monotonically increasing
            assert!(
                result.l >= prev_lightness,
                "Grayscale {} lightness {} should be >= previous {}",
                idx,
                result.l,
                prev_lightness
            );
            prev_lightness = result.l;
        }
    }

    #[test]
    fn test_grayscale_first_vs_last() {
        let colors = TerminalColors::default();
        let first = indexed_color_to_hsla(232, &colors);
        let last = indexed_color_to_hsla(255, &colors);

        // Last should be significantly lighter than first
        assert!(
            last.l - first.l > 0.7,
            "Grayscale range should span most of lightness range: {} to {}",
            first.l,
            last.l
        );
    }

    #[test]
    fn test_grayscale_midpoint() {
        let colors = TerminalColors::default();
        let mid = indexed_color_to_hsla(243, &colors); // Approximately middle of 232-255

        // Should be somewhere around middle lightness
        assert!(
            mid.l > 0.3 && mid.l < 0.7,
            "Middle grayscale should have mid-range lightness: {}",
            mid.l
        );
    }

    // --- Alpha Boundary Tests (0.0, 0.5, 1.0) ---

    #[test]
    fn test_apply_dim_alpha_0() {
        let original = hsla(0.5, 0.5, 0.5, 0.0);
        let dimmed = apply_dim(original);
        assert_eq!(dimmed.a, 0.0, "Alpha 0.0 should be preserved");
    }

    #[test]
    fn test_apply_dim_alpha_half() {
        let original = hsla(0.5, 0.5, 0.5, 0.5);
        let dimmed = apply_dim(original);
        assert_eq!(dimmed.a, 0.5, "Alpha 0.5 should be preserved");
    }

    #[test]
    fn test_apply_dim_alpha_1() {
        let original = hsla(0.5, 0.5, 0.5, 1.0);
        let dimmed = apply_dim(original);
        assert_eq!(dimmed.a, 1.0, "Alpha 1.0 should be preserved");
    }

    #[test_case(0.0 ; "alpha_zero")]
    #[test_case(0.001 ; "alpha_tiny")]
    #[test_case(0.25 ; "alpha_quarter")]
    #[test_case(0.5 ; "alpha_half")]
    #[test_case(0.75 ; "alpha_three_quarters")]
    #[test_case(0.999 ; "alpha_near_one")]
    #[test_case(1.0 ; "alpha_one")]
    fn test_apply_dim_preserves_alpha(a: f32) {
        let original = hsla(0.5, 0.5, 0.5, a);
        let dimmed = apply_dim(original);
        assert!(
            (dimmed.a - a).abs() < 0.0001,
            "Alpha {} should be preserved, got {}",
            a,
            dimmed.a
        );
    }

    // --- apply_dim Lightness Boundary Tests ---

    #[test]
    fn test_apply_dim_lightness_0() {
        let original = hsla(0.5, 0.5, 0.0, 1.0);
        let dimmed = apply_dim(original);
        assert_eq!(dimmed.l, 0.0, "Lightness 0 dimmed should stay 0");
    }

    #[test]
    fn test_apply_dim_lightness_1() {
        let original = hsla(0.5, 0.5, 1.0, 1.0);
        let dimmed = apply_dim(original);
        assert!(
            (dimmed.l - 0.66).abs() < 0.01,
            "Lightness 1 dimmed should be ~0.66, got {}",
            dimmed.l
        );
    }

    #[test]
    fn test_apply_dim_lightness_half() {
        let original = hsla(0.5, 0.5, 0.5, 1.0);
        let dimmed = apply_dim(original);
        assert!(
            (dimmed.l - 0.33).abs() < 0.01,
            "Lightness 0.5 dimmed should be ~0.33, got {}",
            dimmed.l
        );
    }

    #[test_case(0.0, 0.0 ; "lightness_zero")]
    #[test_case(0.1, 0.066 ; "lightness_tenth")]
    #[test_case(0.5, 0.33 ; "lightness_half")]
    #[test_case(1.0, 0.66 ; "lightness_one")]
    fn test_apply_dim_lightness_calculation(l: f32, expected_dimmed: f32) {
        let original = hsla(0.5, 0.5, l, 1.0);
        let dimmed = apply_dim(original);
        assert!(
            (dimmed.l - expected_dimmed).abs() < 0.01,
            "Lightness {} dimmed should be ~{}, got {}",
            l,
            expected_dimmed,
            dimmed.l
        );
    }

    // --- Color Cube Structure Tests ---

    #[test]
    fn test_color_cube_6x6x6_boundary_r() {
        let colors = TerminalColors::default();

        // Test R axis (indices 16, 52, 88, 124, 160, 196) with g=0, b=0
        let indices = [16, 52, 88, 124, 160, 196];
        let mut prev_r = -0.1f32;

        for &idx in &indices {
            let result = indexed_color_to_hsla(idx, &colors);
            let rgba = Rgba::from(result);

            // R should increase monotonically
            assert!(
                rgba.r > prev_r,
                "R should increase: idx {} has R {}",
                idx,
                rgba.r
            );
            prev_r = rgba.r;

            // G and B should be 0
            assert!(rgba.g < 0.01, "G should be 0 for idx {}", idx);
            assert!(rgba.b < 0.01, "B should be 0 for idx {}", idx);
        }
    }

    #[test]
    fn test_color_cube_6x6x6_boundary_g() {
        let colors = TerminalColors::default();

        // Test G axis (indices 16, 22, 28, 34, 40, 46) with r=0, b=0
        let indices = [16, 22, 28, 34, 40, 46];
        let mut prev_g = -0.1f32;

        for &idx in &indices {
            let result = indexed_color_to_hsla(idx, &colors);
            let rgba = Rgba::from(result);

            // G should increase monotonically
            assert!(
                rgba.g > prev_g,
                "G should increase: idx {} has G {}",
                idx,
                rgba.g
            );
            prev_g = rgba.g;
        }
    }

    #[test]
    fn test_color_cube_6x6x6_boundary_b() {
        let colors = TerminalColors::default();

        // Test B axis (indices 16, 17, 18, 19, 20, 21) with r=0, g=0
        let indices = [16, 17, 18, 19, 20, 21];
        let mut prev_b = -0.1f32;

        for &idx in &indices {
            let result = indexed_color_to_hsla(idx, &colors);
            let rgba = Rgba::from(result);

            // B should increase monotonically
            assert!(
                rgba.b > prev_b,
                "B should increase: idx {} has B {}",
                idx,
                rgba.b
            );
            prev_b = rgba.b;
        }
    }

    // --- Additional Property Tests ---

    proptest! {
        #[test]
        fn prop_rgb_boundary_always_valid(r in 0u8..=255, g in 0u8..=255, b in 0u8..=255) {
            let rgb = Rgb { r, g, b };
            let result = rgb_to_hsla(rgb);

            prop_assert!(result.h >= -FLOAT_EPSILON && result.h <= 1.0 + FLOAT_EPSILON);
            prop_assert!(result.s >= -FLOAT_EPSILON && result.s <= 1.0 + FLOAT_EPSILON);
            prop_assert!(result.l >= -FLOAT_EPSILON && result.l <= 1.0 + FLOAT_EPSILON);
            prop_assert_eq!(result.a, 1.0);
        }

        #[test]
        fn prop_apply_dim_never_increases_lightness(l in 0.0f32..=1.0) {
            let original = hsla(0.5, 0.5, l, 1.0);
            let dimmed = apply_dim(original);
            prop_assert!(dimmed.l <= l + FLOAT_EPSILON, "Dimmed lightness should not exceed original");
        }

        #[test]
        fn prop_apply_dim_exact_calculation(l in 0.0f32..=1.0) {
            let original = hsla(0.5, 0.5, l, 1.0);
            let dimmed = apply_dim(original);
            let expected = l * 0.66;
            prop_assert!((dimmed.l - expected).abs() < 0.001,
                "Dimmed lightness should be exactly 66% of original");
        }
    }

    // --- Transition Tests (boundary between ranges) ---

    #[test]
    fn test_indexed_color_ansi_to_cube_transition() {
        let colors = TerminalColors::default();

        // Index 15 is last ANSI color (bright white)
        let last_ansi = indexed_color_to_hsla(15, &colors);
        // Index 16 is first color cube (black in cube)
        let first_cube = indexed_color_to_hsla(16, &colors);

        // Both should be valid
        assert_eq!(last_ansi.a, 1.0);
        assert_eq!(first_cube.a, 1.0);

        // They should be different colors
        let h_diff = (last_ansi.h - first_cube.h).abs();
        let s_diff = (last_ansi.s - first_cube.s).abs();
        let l_diff = (last_ansi.l - first_cube.l).abs();
        assert!(
            h_diff > 0.001 || s_diff > 0.001 || l_diff > 0.001,
            "ANSI 15 and cube 16 should be different colors"
        );
    }

    #[test]
    fn test_indexed_color_cube_to_grayscale_transition() {
        let colors = TerminalColors::default();

        // Index 231 is last color cube (white in cube)
        let last_cube = indexed_color_to_hsla(231, &colors);
        // Index 232 is first grayscale (dark gray)
        let first_gray = indexed_color_to_hsla(232, &colors);

        // Both should be valid
        assert_eq!(last_cube.a, 1.0);
        assert_eq!(first_gray.a, 1.0);

        // First grayscale should have zero saturation
        assert!(
            first_gray.s < 0.001,
            "First grayscale should have zero saturation"
        );

        // They should have significantly different lightness
        assert!(
            (last_cube.l - first_gray.l).abs() > 0.5,
            "Cube 231 (white) and gray 232 (dark) should have different lightness"
        );
    }

    // ==================== Expanded Property-Based Tests (1000 cases) ====================

    /// Small epsilon for floating point comparisons in roundtrip tests
    const ROUNDTRIP_EPSILON: f32 = 0.02;

    /// Strategy for generating arbitrary RGB values
    fn arb_rgb() -> impl Strategy<Value = Rgb> {
        (0u8..=255, 0u8..=255, 0u8..=255).prop_map(|(r, g, b)| Rgb { r, g, b })
    }

    proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(1000))]

        // ==================== RGB Conversion Tests ====================

        /// Property: RGB to HSLA conversion produces valid HSLA values
        #[test]
        fn prop_rgb_to_hsla_valid_output_1000(rgb in arb_rgb()) {
            let result = rgb_to_hsla(rgb);

            // HSLA values should be in valid ranges (with epsilon for float precision)
            prop_assert!(result.h >= -FLOAT_EPSILON && result.h <= 1.0 + FLOAT_EPSILON,
                "Hue {} out of range for RGB({}, {}, {})", result.h, rgb.r, rgb.g, rgb.b);
            prop_assert!(result.s >= -FLOAT_EPSILON && result.s <= 1.0 + FLOAT_EPSILON,
                "Saturation {} out of range for RGB({}, {}, {})", result.s, rgb.r, rgb.g, rgb.b);
            prop_assert!(result.l >= -FLOAT_EPSILON && result.l <= 1.0 + FLOAT_EPSILON,
                "Lightness {} out of range for RGB({}, {}, {})", result.l, rgb.r, rgb.g, rgb.b);
            prop_assert_eq!(result.a, 1.0, "Alpha should always be 1.0");
        }

        /// Property: RGB to HSLA roundtrip maintains accuracy within epsilon
        #[test]
        fn prop_rgb_hsla_roundtrip_accuracy_1000(rgb in arb_rgb()) {
            let hsla_result = rgb_to_hsla(rgb);
            let rgba_back = Rgba::from(hsla_result);

            // Convert back to 0-255 range for comparison
            let r_back = (rgba_back.r * 255.0).round() as u8;
            let g_back = (rgba_back.g * 255.0).round() as u8;
            let b_back = (rgba_back.b * 255.0).round() as u8;

            // Should be within 1 step due to rounding
            prop_assert!((r_back as i16 - rgb.r as i16).abs() <= 1,
                "Red roundtrip failed: {} -> {} -> {}", rgb.r, hsla_result.l, r_back);
            prop_assert!((g_back as i16 - rgb.g as i16).abs() <= 1,
                "Green roundtrip failed: {} -> {} -> {}", rgb.g, hsla_result.l, g_back);
            prop_assert!((b_back as i16 - rgb.b as i16).abs() <= 1,
                "Blue roundtrip failed: {} -> {} -> {}", rgb.b, hsla_result.l, b_back);
        }

        /// Property: Grayscale RGB values should have zero saturation
        #[test]
        fn prop_grayscale_rgb_zero_saturation_1000(gray in 0u8..=255) {
            let rgb = Rgb { r: gray, g: gray, b: gray };
            let result = rgb_to_hsla(rgb);

            prop_assert!(result.s.abs() < 0.001,
                "Grayscale RGB({}, {}, {}) should have ~0 saturation, got {}", gray, gray, gray, result.s);
        }

        /// Property: Pure colors (max one channel) should have saturation ~1
        #[test]
        fn prop_pure_colors_full_saturation_1000(channel in 0u8..3) {
            let rgb = match channel {
                0 => Rgb { r: 255, g: 0, b: 0 },   // Pure red
                1 => Rgb { r: 0, g: 255, b: 0 },   // Pure green
                _ => Rgb { r: 0, g: 0, b: 255 },   // Pure blue
            };
            let result = rgb_to_hsla(rgb);

            prop_assert!((result.s - 1.0).abs() < 0.01,
                "Pure color should have saturation ~1, got {}", result.s);
        }

        // ==================== Indexed Color Tests ====================

        /// Property: Indexed color always produces valid HSLA for any index
        #[test]
        fn prop_indexed_color_always_valid_1000(idx in 0u8..=255) {
            let colors = TerminalColors::default();
            let result = indexed_color_to_hsla(idx, &colors);

            prop_assert!(result.h >= -FLOAT_EPSILON && result.h <= 1.0 + FLOAT_EPSILON,
                "Hue {} out of range for idx {}", result.h, idx);
            prop_assert!(result.s >= -FLOAT_EPSILON && result.s <= 1.0 + FLOAT_EPSILON,
                "Saturation {} out of range for idx {}", result.s, idx);
            prop_assert!(result.l >= -FLOAT_EPSILON && result.l <= 1.0 + FLOAT_EPSILON,
                "Lightness {} out of range for idx {}", result.l, idx);
            prop_assert_eq!(result.a, 1.0, "Alpha should be 1.0 for idx {}", idx);
        }

        /// Property: Grayscale indices (232-255) have zero saturation
        #[test]
        fn prop_grayscale_indices_zero_saturation_1000(offset in 0u8..24) {
            let idx = 232 + offset;
            let colors = TerminalColors::default();
            let result = indexed_color_to_hsla(idx, &colors);

            prop_assert!(result.s.abs() < 0.001,
                "Grayscale idx {} should have ~0 saturation, got {}", idx, result.s);
        }

        /// Property: Color cube indices (16-231) produce distinct colors
        #[test]
        fn prop_color_cube_distinct_1000(idx1 in 16u8..=231, idx2 in 16u8..=231) {
            prop_assume!(idx1 != idx2);
            let colors = TerminalColors::default();
            let color1 = indexed_color_to_hsla(idx1, &colors);
            let color2 = indexed_color_to_hsla(idx2, &colors);

            // At least one component should differ
            let h_diff = (color1.h - color2.h).abs();
            let s_diff = (color1.s - color2.s).abs();
            let l_diff = (color1.l - color2.l).abs();

            prop_assert!(h_diff > 0.001 || s_diff > 0.001 || l_diff > 0.001,
                "Indices {} and {} should produce different colors", idx1, idx2);
        }

        /// Property: ANSI colors (0-15) produce valid HSLA
        #[test]
        fn prop_ansi_colors_valid_1000(idx in 0u8..16) {
            let colors = TerminalColors::default();
            let result = indexed_color_to_hsla(idx, &colors);

            prop_assert!(result.h >= 0.0 && result.h <= 1.0, "Hue out of range for ANSI idx {}", idx);
            prop_assert!(result.s >= 0.0 && result.s <= 1.0, "Saturation out of range for ANSI idx {}", idx);
            prop_assert!(result.l >= 0.0 && result.l <= 1.0, "Lightness out of range for ANSI idx {}", idx);
            prop_assert_eq!(result.a, 1.0, "Alpha should be 1.0");
        }

        // ==================== apply_dim Tests ====================

        /// Property: apply_dim never increases lightness
        #[test]
        fn prop_apply_dim_never_increases_lightness_1000(
            h in 0.0f32..=1.0,
            s in 0.0f32..=1.0,
            l in 0.0f32..=1.0,
            a in 0.0f32..=1.0
        ) {
            let original = hsla(h, s, l, a);
            let dimmed = apply_dim(original);

            prop_assert!(dimmed.l <= original.l + FLOAT_EPSILON,
                "Dimmed lightness {} should be <= original {}", dimmed.l, original.l);
        }

        /// Property: apply_dim reduces lightness to exactly 66%
        #[test]
        fn prop_apply_dim_exact_reduction_1000(
            h in 0.0f32..=1.0,
            s in 0.0f32..=1.0,
            l in 0.0f32..=1.0,
            a in 0.0f32..=1.0
        ) {
            let original = hsla(h, s, l, a);
            let dimmed = apply_dim(original);

            let expected_l = original.l * 0.66;
            prop_assert!((dimmed.l - expected_l).abs() < 0.001,
                "Dimmed lightness {} should be {} (66% of {})", dimmed.l, expected_l, original.l);
        }

        /// Property: apply_dim preserves hue
        #[test]
        fn prop_apply_dim_preserves_hue_1000(
            h in 0.0f32..=1.0,
            s in 0.0f32..=1.0,
            l in 0.0f32..=1.0,
            a in 0.0f32..=1.0
        ) {
            let original = hsla(h, s, l, a);
            let dimmed = apply_dim(original);

            prop_assert_eq!(dimmed.h, original.h, "Hue should be preserved");
        }

        /// Property: apply_dim preserves saturation
        #[test]
        fn prop_apply_dim_preserves_saturation_1000(
            h in 0.0f32..=1.0,
            s in 0.0f32..=1.0,
            l in 0.0f32..=1.0,
            a in 0.0f32..=1.0
        ) {
            let original = hsla(h, s, l, a);
            let dimmed = apply_dim(original);

            prop_assert_eq!(dimmed.s, original.s, "Saturation should be preserved");
        }

        /// Property: apply_dim preserves alpha
        #[test]
        fn prop_apply_dim_preserves_alpha_1000(
            h in 0.0f32..=1.0,
            s in 0.0f32..=1.0,
            l in 0.0f32..=1.0,
            a in 0.0f32..=1.0
        ) {
            let original = hsla(h, s, l, a);
            let dimmed = apply_dim(original);

            prop_assert_eq!(dimmed.a, original.a, "Alpha should be preserved");
        }

        /// Property: apply_dim on zero lightness produces zero lightness
        #[test]
        fn prop_apply_dim_zero_stays_zero_1000(
            h in 0.0f32..=1.0,
            s in 0.0f32..=1.0,
            a in 0.0f32..=1.0
        ) {
            let original = hsla(h, s, 0.0, a);
            let dimmed = apply_dim(original);

            prop_assert_eq!(dimmed.l, 0.0, "Dimming black should stay black");
        }

        /// Property: apply_dim is NOT idempotent (applying twice reduces further)
        #[test]
        fn prop_apply_dim_not_idempotent_1000(
            h in 0.0f32..=1.0,
            s in 0.0f32..=1.0,
            l in 0.01f32..=1.0,  // Avoid zero to see the effect
            a in 0.0f32..=1.0
        ) {
            let original = hsla(h, s, l, a);
            let dimmed_once = apply_dim(original);
            let dimmed_twice = apply_dim(dimmed_once);

            // Dimming twice should reduce lightness further
            prop_assert!(dimmed_twice.l < dimmed_once.l + FLOAT_EPSILON,
                "Double dim {} should be < single dim {}", dimmed_twice.l, dimmed_once.l);
        }

        // ==================== Color Conversion Idempotency Tests ====================

        /// Property: rgb_to_hsla with same input produces same output (deterministic)
        #[test]
        fn prop_rgb_to_hsla_deterministic_1000(rgb in arb_rgb()) {
            let result1 = rgb_to_hsla(rgb);
            let result2 = rgb_to_hsla(rgb);

            prop_assert_eq!(result1.h, result2.h, "Hue should be deterministic");
            prop_assert_eq!(result1.s, result2.s, "Saturation should be deterministic");
            prop_assert_eq!(result1.l, result2.l, "Lightness should be deterministic");
            prop_assert_eq!(result1.a, result2.a, "Alpha should be deterministic");
        }

        /// Property: indexed_color_to_hsla is deterministic
        #[test]
        fn prop_indexed_color_deterministic_1000(idx in 0u8..=255) {
            let colors = TerminalColors::default();
            let result1 = indexed_color_to_hsla(idx, &colors);
            let result2 = indexed_color_to_hsla(idx, &colors);

            prop_assert_eq!(result1.h, result2.h, "Hue should be deterministic for idx {}", idx);
            prop_assert_eq!(result1.s, result2.s, "Saturation should be deterministic for idx {}", idx);
            prop_assert_eq!(result1.l, result2.l, "Lightness should be deterministic for idx {}", idx);
            prop_assert_eq!(result1.a, result2.a, "Alpha should be deterministic for idx {}", idx);
        }

        // ==================== Color Operations Consistency Tests ====================

        /// Property: color_to_hsla for Spec colors matches rgb_to_hsla
        #[test]
        fn prop_color_to_hsla_spec_matches_rgb_1000(rgb in arb_rgb()) {
            let term_colors = TermColors::default();
            let theme = TerminalColors::default();

            let direct = rgb_to_hsla(rgb);
            let via_color = color_to_hsla(Color::Spec(rgb), &term_colors, &theme);

            prop_assert_eq!(direct.h, via_color.h, "Hue should match");
            prop_assert_eq!(direct.s, via_color.s, "Saturation should match");
            prop_assert_eq!(direct.l, via_color.l, "Lightness should match");
            prop_assert_eq!(direct.a, via_color.a, "Alpha should match");
        }

        /// Property: color_to_hsla for indexed colors matches indexed_color_to_hsla (without custom colors)
        #[test]
        fn prop_color_to_hsla_indexed_matches_1000(idx in 0u8..=255) {
            let term_colors = TermColors::default();
            let theme = TerminalColors::default();

            let direct = indexed_color_to_hsla(idx, &theme);
            let via_color = color_to_hsla(Color::Indexed(idx), &term_colors, &theme);

            prop_assert_eq!(direct.h, via_color.h, "Hue should match for idx {}", idx);
            prop_assert_eq!(direct.s, via_color.s, "Saturation should match for idx {}", idx);
            prop_assert_eq!(direct.l, via_color.l, "Lightness should match for idx {}", idx);
            prop_assert_eq!(direct.a, via_color.a, "Alpha should match for idx {}", idx);
        }

        /// Property: get_bright_color for indexed 0-7 produces indexed 8-15
        #[test]
        fn prop_bright_color_indexed_shift_1000(idx in 0u8..8) {
            let term_colors = TermColors::default();
            let theme = TerminalColors::default();

            let bright = get_bright_color(Color::Indexed(idx), &term_colors, &theme);
            let expected = indexed_color_to_hsla(idx + 8, &theme);

            prop_assert_eq!(bright.h, expected.h, "Bright hue should match for idx {}", idx);
            prop_assert_eq!(bright.s, expected.s, "Bright saturation should match for idx {}", idx);
            prop_assert_eq!(bright.l, expected.l, "Bright lightness should match for idx {}", idx);
        }

        /// Property: get_bright_color for indexed >= 8 passes through unchanged
        #[test]
        fn prop_bright_color_passthrough_1000(idx in 8u8..=255) {
            let term_colors = TermColors::default();
            let theme = TerminalColors::default();

            let bright = get_bright_color(Color::Indexed(idx), &term_colors, &theme);
            let expected = color_to_hsla(Color::Indexed(idx), &term_colors, &theme);

            prop_assert_eq!(bright.h, expected.h, "Should pass through for idx {}", idx);
            prop_assert_eq!(bright.s, expected.s, "Should pass through for idx {}", idx);
            prop_assert_eq!(bright.l, expected.l, "Should pass through for idx {}", idx);
        }

        /// Property: get_bright_color for Spec colors passes through unchanged
        #[test]
        fn prop_bright_color_spec_passthrough_1000(rgb in arb_rgb()) {
            let term_colors = TermColors::default();
            let theme = TerminalColors::default();

            let bright = get_bright_color(Color::Spec(rgb), &term_colors, &theme);
            let expected = rgb_to_hsla(rgb);

            prop_assert_eq!(bright.h, expected.h, "Spec color should pass through");
            prop_assert_eq!(bright.s, expected.s, "Spec color should pass through");
            prop_assert_eq!(bright.l, expected.l, "Spec color should pass through");
        }

        // ==================== Edge Case Tests ====================

        /// Property: Color cube corners have correct RGB values
        #[test]
        fn prop_color_cube_corners_1000(r in 0u8..6, g in 0u8..6, b in 0u8..6) {
            let colors = TerminalColors::default();
            let idx = 16 + 36 * r + 6 * g + b;
            let result = indexed_color_to_hsla(idx, &colors);

            // Convert back to RGB to verify
            let rgba = Rgba::from(result);
            let expected_r = r as f32 / 5.0;
            let expected_g = g as f32 / 5.0;
            let expected_b = b as f32 / 5.0;

            prop_assert!((rgba.r - expected_r).abs() < ROUNDTRIP_EPSILON,
                "Red mismatch for cube({},{},{}): expected {}, got {}", r, g, b, expected_r, rgba.r);
            prop_assert!((rgba.g - expected_g).abs() < ROUNDTRIP_EPSILON,
                "Green mismatch for cube({},{},{}): expected {}, got {}", r, g, b, expected_g, rgba.g);
            prop_assert!((rgba.b - expected_b).abs() < ROUNDTRIP_EPSILON,
                "Blue mismatch for cube({},{},{}): expected {}, got {}", r, g, b, expected_b, rgba.b);
        }

        /// Property: Higher RGB values generally produce higher lightness
        #[test]
        fn prop_rgb_lightness_correlation_1000(
            r1 in 0u8..128,
            g1 in 0u8..128,
            b1 in 0u8..128,
            offset in 64u8..128
        ) {
            // Create a darker and lighter version
            let dark = Rgb { r: r1, g: g1, b: b1 };
            let light = Rgb {
                r: r1.saturating_add(offset),
                g: g1.saturating_add(offset),
                b: b1.saturating_add(offset),
            };

            let dark_hsla = rgb_to_hsla(dark);
            let light_hsla = rgb_to_hsla(light);

            // Lighter RGB should have higher lightness
            prop_assert!(light_hsla.l >= dark_hsla.l - 0.01,
                "Lighter RGB should have >= lightness: dark {} vs light {}",
                dark_hsla.l, light_hsla.l);
        }
    }
}
