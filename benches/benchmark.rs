//! Criterion benchmarks for humanssh hot-path operations.
//!
//! Run with: `cargo bench`
//!
//! Benchmarks cover:
//! - Color conversion functions (RGB, indexed, named colors)
//! - Terminal size operations
//! - Mouse escape sequence buffer operations

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::fmt::Write;

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::vte::ansi::{NamedColor, Rgb};
use gpui::{hsla, Hsla, Rgba};

use humanssh::terminal::types::{MouseEscBuf, TermSize};
use humanssh::theme::TerminalColors;

// ============================================================================
// Color Conversion Functions (inlined from terminal/colors.rs for benchmarking)
// These are the same implementations - we inline them to avoid visibility issues
// ============================================================================

/// Convert RGB to Hsla.
fn rgb_to_hsla(rgb: Rgb) -> Hsla {
    Hsla::from(Rgba {
        r: rgb.r as f32 / 255.0,
        g: rgb.g as f32 / 255.0,
        b: rgb.b as f32 / 255.0,
        a: 1.0,
    })
}

/// Convert a named ANSI color to Hsla using theme colors.
fn named_color_to_hsla(color: NamedColor, colors: &TerminalColors) -> Hsla {
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
fn indexed_color_to_hsla(idx: u8, colors: &TerminalColors) -> Hsla {
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
fn apply_dim(color: Hsla) -> Hsla {
    hsla(color.h, color.s, color.l * 0.66, color.a)
}

// ============================================================================
// Color Conversion Benchmarks
// ============================================================================

fn bench_rgb_to_hsla(c: &mut Criterion) {
    let mut group = c.benchmark_group("color_conversion");

    // Test various RGB values
    let test_cases = [
        ("red", Rgb { r: 255, g: 0, b: 0 }),
        ("green", Rgb { r: 0, g: 255, b: 0 }),
        ("blue", Rgb { r: 0, g: 0, b: 255 }),
        (
            "white",
            Rgb {
                r: 255,
                g: 255,
                b: 255,
            },
        ),
        ("black", Rgb { r: 0, g: 0, b: 0 }),
        (
            "gray",
            Rgb {
                r: 128,
                g: 128,
                b: 128,
            },
        ),
        (
            "catppuccin_rosewater",
            Rgb {
                r: 245,
                g: 224,
                b: 220,
            },
        ),
    ];

    for (name, rgb) in test_cases {
        group.bench_with_input(BenchmarkId::new("rgb_to_hsla", name), &rgb, |b, rgb| {
            b.iter(|| rgb_to_hsla(black_box(*rgb)))
        });
    }

    group.finish();
}

fn bench_indexed_color_to_hsla(c: &mut Criterion) {
    let mut group = c.benchmark_group("indexed_color");
    let colors = TerminalColors::default();

    // Test different regions of the 256-color palette
    let test_cases = [
        ("ansi_black", 0u8),
        ("ansi_red", 1u8),
        ("ansi_bright_white", 15u8),
        ("cube_start", 16u8),
        ("cube_middle", 123u8),
        ("cube_end", 231u8),
        ("grayscale_dark", 232u8),
        ("grayscale_mid", 243u8),
        ("grayscale_light", 255u8),
    ];

    for (name, idx) in test_cases {
        group.bench_with_input(
            BenchmarkId::new("indexed_color_to_hsla", name),
            &idx,
            |b, idx| b.iter(|| indexed_color_to_hsla(black_box(*idx), black_box(&colors))),
        );
    }

    group.finish();
}

fn bench_named_color_to_hsla(c: &mut Criterion) {
    let mut group = c.benchmark_group("named_color");
    let colors = TerminalColors::default();

    let named_colors = [
        ("black", NamedColor::Black),
        ("red", NamedColor::Red),
        ("green", NamedColor::Green),
        ("yellow", NamedColor::Yellow),
        ("blue", NamedColor::Blue),
        ("magenta", NamedColor::Magenta),
        ("cyan", NamedColor::Cyan),
        ("white", NamedColor::White),
        ("bright_black", NamedColor::BrightBlack),
        ("foreground", NamedColor::Foreground),
        ("background", NamedColor::Background),
        ("cursor", NamedColor::Cursor),
    ];

    for (name, color) in named_colors {
        group.bench_with_input(
            BenchmarkId::new("named_color_to_hsla", name),
            &color,
            |b, color| b.iter(|| named_color_to_hsla(black_box(*color), black_box(&colors))),
        );
    }

    group.finish();
}

fn bench_apply_dim(c: &mut Criterion) {
    let mut group = c.benchmark_group("color_effects");

    let test_colors = [
        ("red", hsla(0.0, 1.0, 0.5, 1.0)),
        ("white", hsla(0.0, 0.0, 1.0, 1.0)),
        ("dim_gray", hsla(0.0, 0.0, 0.3, 1.0)),
    ];

    for (name, color) in test_colors {
        group.bench_with_input(BenchmarkId::new("apply_dim", name), &color, |b, color| {
            b.iter(|| apply_dim(black_box(*color)))
        });
    }

    group.finish();
}

// ============================================================================
// TermSize Benchmarks
// ============================================================================

fn bench_term_size(c: &mut Criterion) {
    let mut group = c.benchmark_group("term_size");

    // Default construction
    group.bench_function("default", |b| b.iter(TermSize::default));

    // Custom construction
    group.bench_function("custom", |b| {
        b.iter(|| TermSize {
            cols: black_box(120),
            rows: black_box(40),
        })
    });

    // Dimensions trait methods (hot path during resize/render)
    let size = TermSize {
        cols: 120,
        rows: 40,
    };

    group.bench_function("total_lines", |b| b.iter(|| black_box(&size).total_lines()));

    group.bench_function("screen_lines", |b| {
        b.iter(|| black_box(&size).screen_lines())
    });

    group.bench_function("columns", |b| b.iter(|| black_box(&size).columns()));

    // All dimensions together (common pattern)
    group.bench_function("all_dimensions", |b| {
        b.iter(|| {
            let s = black_box(&size);
            (s.total_lines(), s.screen_lines(), s.columns())
        })
    });

    group.finish();
}

// ============================================================================
// MouseEscBuf Benchmarks (hot path for mouse events)
// ============================================================================

fn bench_mouse_esc_buf(c: &mut Criterion) {
    let mut group = c.benchmark_group("mouse_esc_buf");

    // Construction
    group.bench_function("new", |b| b.iter(MouseEscBuf::new));

    group.bench_function("default", |b| b.iter(MouseEscBuf::default));

    // SGR mouse sequence writing (common mouse click)
    group.bench_function("write_sgr_click", |b| {
        b.iter(|| {
            let mut buf = MouseEscBuf::new();
            write!(
                buf,
                "\x1b[<{};{};{}M",
                black_box(0),
                black_box(10),
                black_box(20)
            )
            .unwrap();
            buf.as_str().len()
        })
    });

    // SGR mouse sequence with large coordinates
    group.bench_function("write_sgr_large_coords", |b| {
        b.iter(|| {
            let mut buf = MouseEscBuf::new();
            write!(
                buf,
                "\x1b[<{};{};{}M",
                black_box(999),
                black_box(9999),
                black_box(9999)
            )
            .unwrap();
            buf.as_str().len()
        })
    });

    // Mouse release sequence
    group.bench_function("write_sgr_release", |b| {
        b.iter(|| {
            let mut buf = MouseEscBuf::new();
            write!(
                buf,
                "\x1b[<{};{};{}m",
                black_box(0),
                black_box(50),
                black_box(25)
            )
            .unwrap();
            buf.as_str().len()
        })
    });

    // Throughput benchmark for mouse events
    group.throughput(Throughput::Elements(1000));
    group.bench_function("1000_mouse_events", |b| {
        b.iter(|| {
            for i in 0..1000u32 {
                let mut buf = MouseEscBuf::new();
                write!(buf, "\x1b[<{};{};{}M", i % 4, i % 200, i % 50).unwrap();
                black_box(buf.as_str());
            }
        })
    });

    group.finish();
}

// ============================================================================
// Batch Color Conversion Benchmarks (realistic rendering workload)
// ============================================================================

fn bench_batch_color_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("batch_operations");
    let colors = TerminalColors::default();

    // Simulate rendering a row of 80 cells with mixed colors
    group.throughput(Throughput::Elements(80));
    group.bench_function("render_row_80_cells", |b| {
        let rgb_values: Vec<Rgb> = (0..80)
            .map(|i| Rgb {
                r: (i * 3) as u8,
                g: (i * 2) as u8,
                b: i as u8,
            })
            .collect();

        b.iter(|| {
            for rgb in &rgb_values {
                black_box(rgb_to_hsla(*rgb));
            }
        })
    });

    // Simulate rendering a full 80x24 terminal
    group.throughput(Throughput::Elements(80 * 24));
    group.bench_function("render_full_terminal_80x24", |b| {
        let indexed_colors: Vec<u8> = (0..80 * 24).map(|i| (i % 256) as u8).collect();

        b.iter(|| {
            for idx in &indexed_colors {
                black_box(indexed_color_to_hsla(*idx, &colors));
            }
        })
    });

    // Large terminal (120x40)
    group.throughput(Throughput::Elements(120 * 40));
    group.bench_function("render_full_terminal_120x40", |b| {
        let indexed_colors: Vec<u8> = (0..120 * 40).map(|i| (i % 256) as u8).collect();

        b.iter(|| {
            for idx in &indexed_colors {
                black_box(indexed_color_to_hsla(*idx, &colors));
            }
        })
    });

    group.finish();
}

// ============================================================================
// TerminalColors Default Construction Benchmark
// ============================================================================

fn bench_terminal_colors(c: &mut Criterion) {
    let mut group = c.benchmark_group("terminal_colors");

    group.bench_function("default", |b| b.iter(TerminalColors::default));

    // Copy operation (common when passing colors around)
    let colors = TerminalColors::default();
    group.bench_function("copy_via_deref", |b| b.iter(|| *black_box(&colors)));

    // Copy operation
    group.bench_function("copy", |b| {
        b.iter(|| {
            let c: TerminalColors = *black_box(&colors);
            c
        })
    });

    group.finish();
}

// ============================================================================
// Criterion Groups and Main
// ============================================================================

criterion_group!(
    color_benches,
    bench_rgb_to_hsla,
    bench_indexed_color_to_hsla,
    bench_named_color_to_hsla,
    bench_apply_dim,
    bench_batch_color_operations,
    bench_terminal_colors,
);

criterion_group!(term_size_benches, bench_term_size,);

criterion_group!(mouse_benches, bench_mouse_esc_buf,);

criterion_main!(color_benches, term_size_benches, mouse_benches);
