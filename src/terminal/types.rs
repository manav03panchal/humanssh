//! Terminal data types.
//!
//! This module contains the core data structures used by the terminal emulator.
//! Separating types from behavior allows for:
//! - Easier testing (types can be constructed without GPUI context)
//! - Cleaner module boundaries
//! - Better documentation of the data model

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::vte::ansi::CursorShape;
use gpui::{Hsla, SharedString};
use std::fmt::Write as FmtWrite;

use crate::config::terminal::DEFAULT_FONT_SIZE;

/// Terminal dimensions in rows and columns.
///
/// Implements `Dimensions` trait for alacritty compatibility.
#[derive(Clone, Copy, Debug)]
pub struct TermSize {
    pub cols: u16,
    pub rows: u16,
}

impl Default for TermSize {
    fn default() -> Self {
        Self { cols: 80, rows: 24 }
    }
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.rows as usize
    }

    fn screen_lines(&self) -> usize {
        self.rows as usize
    }

    fn columns(&self) -> usize {
        self.cols as usize
    }
}

/// Consolidated display state for rendering.
///
/// Groups read-heavy fields to minimize lock contention. Uses RwLock
/// for better read concurrency since most access is read-only during render.
#[derive(Clone)]
pub struct DisplayState {
    /// Terminal dimensions in rows/columns
    pub size: TermSize,
    /// Cell dimensions (width, height) - calculated from font metrics
    pub cell_dims: (f32, f32),
    /// Element bounds in window coordinates (for mouse position conversion)
    pub bounds: Option<gpui::Bounds<gpui::Pixels>>,
    /// Current font size
    pub font_size: f32,
    /// Font size (as bits) and family used for the cached cell_dims, to detect when recalculation is needed
    pub cached_font_key: Option<(u32, SharedString)>,
}

impl Default for DisplayState {
    fn default() -> Self {
        Self {
            size: TermSize::default(),
            cell_dims: (8.4, 17.0),
            bounds: None,
            font_size: DEFAULT_FONT_SIZE,
            cached_font_key: None,
        }
    }
}

/// A single cell to render.
#[derive(Clone)]
pub struct RenderCell {
    pub row: usize,
    pub col: usize,
    pub c: char,
    pub fg: Hsla,
    pub flags: CellFlags,
}

/// A background region to be painted.
#[derive(Clone)]
pub struct BgRegion {
    pub row: usize,
    pub col_start: usize,
    pub col_end: usize,
    pub color: Hsla,
}

/// Cursor rendering info.
#[derive(Clone, Copy)]
pub struct CursorInfo {
    pub row: usize,
    pub col: usize,
    pub shape: CursorShape,
    pub color: Hsla,
}

/// Pre-computed render data for a single frame.
pub struct RenderData {
    /// Cells to render (non-space cells only)
    pub cells: Vec<RenderCell>,
    /// Background regions (non-default backgrounds only)
    pub bg_regions: Vec<BgRegion>,
    /// Cursor info (separate from bg_regions for proper shape handling)
    pub cursor: Option<CursorInfo>,
}

/// Stack-allocated buffer for mouse escape sequences.
///
/// Avoids heap allocation for mouse events. Max SGR sequence:
/// `\x1b[<999;9999;9999M` = ~20 bytes, so 32 is plenty.
pub struct MouseEscBuf {
    buf: [u8; 32],
    len: usize,
}

impl MouseEscBuf {
    pub fn new() -> Self {
        Self {
            buf: [0; 32],
            len: 0,
        }
    }

    pub fn as_str(&self) -> &str {
        // Safety: we only write valid UTF-8 (ASCII escape sequences)
        unsafe { std::str::from_utf8_unchecked(&self.buf[..self.len]) }
    }
}

impl Default for MouseEscBuf {
    fn default() -> Self {
        Self::new()
    }
}

impl FmtWrite for MouseEscBuf {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        let bytes = s.as_bytes();
        let remaining = 32 - self.len;
        let to_write = bytes.len().min(remaining);
        self.buf[self.len..self.len + to_write].copy_from_slice(&bytes[..to_write]);
        self.len += to_write;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::clone_on_copy, clippy::unnecessary_literal_unwrap)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use proptest::prelude::*;
    use std::fmt::Write;
    use test_case::test_case;

    // ==================== TermSize Tests ====================

    #[test]
    fn test_term_size_default() {
        let size = TermSize::default();
        assert_eq!(size.cols, 80);
        assert_eq!(size.rows, 24);
    }

    #[test]
    fn test_term_size_dimensions() {
        let size = TermSize {
            cols: 100,
            rows: 50,
        };
        assert_eq!(size.total_lines(), 50);
        assert_eq!(size.screen_lines(), 50);
        assert_eq!(size.columns(), 100);
    }

    #[test_case(0, 0 ; "zero dimensions")]
    #[test_case(1, 1 ; "minimum dimensions")]
    #[test_case(u16::MAX, u16::MAX ; "maximum dimensions")]
    #[test_case(80, 24 ; "standard terminal")]
    #[test_case(120, 40 ; "large terminal")]
    #[test_case(40, 10 ; "small terminal")]
    fn test_term_size_dimensions_edge_cases(cols: u16, rows: u16) {
        let size = TermSize { cols, rows };
        assert_eq!(size.columns(), cols as usize);
        assert_eq!(size.total_lines(), rows as usize);
        assert_eq!(size.screen_lines(), rows as usize);
    }

    // ==================== DisplayState Tests ====================

    #[test]
    fn test_display_state_default() {
        let state = DisplayState::default();
        assert_eq!(state.size.cols, 80);
        assert_eq!(state.size.rows, 24);
        assert_eq!(state.cell_dims, (8.4, 17.0));
        assert!(state.bounds.is_none());
        assert_eq!(state.font_size, 14.0); // DEFAULT_FONT_SIZE
    }

    #[test]
    fn test_display_state_with_custom_size() {
        let custom_size = TermSize {
            cols: 132,
            rows: 43,
        };
        let state = DisplayState {
            size: custom_size,
            cell_dims: (9.0, 18.0),
            bounds: None,
            font_size: 12.0,
            cached_font_key: None,
        };

        assert_eq!(state.size.cols, 132);
        assert_eq!(state.size.rows, 43);
        assert_eq!(state.size.columns(), 132);
        assert_eq!(state.size.total_lines(), 43);
    }

    #[test_case(8.0, 16.0 ; "standard cell size")]
    #[test_case(0.0, 0.0 ; "zero cell size")]
    #[test_case(100.0, 200.0 ; "large cell size")]
    #[test_case(0.5, 1.0 ; "small cell size")]
    fn test_display_state_cell_dims(width: f32, height: f32) {
        let state = DisplayState {
            cell_dims: (width, height),
            ..Default::default()
        };
        assert_eq!(state.cell_dims.0, width);
        assert_eq!(state.cell_dims.1, height);
    }

    #[test_case(8.0 ; "small font")]
    #[test_case(14.0 ; "default font")]
    #[test_case(24.0 ; "large font")]
    #[test_case(72.0 ; "very large font")]
    fn test_display_state_font_size(font_size: f32) {
        let state = DisplayState {
            font_size,
            ..Default::default()
        };
        assert_eq!(state.font_size, font_size);
    }

    // ==================== RenderCell Tests ====================

    #[test]
    fn test_render_cell_basic() {
        let cell = RenderCell {
            row: 5,
            col: 10,
            c: 'A',
            fg: Hsla::default(),
            flags: CellFlags::empty(),
        };
        assert_eq!(cell.row, 5);
        assert_eq!(cell.col, 10);
        assert_eq!(cell.c, 'A');
    }

    #[test]
    fn test_render_cell_with_bold_flag() {
        let cell = RenderCell {
            row: 0,
            col: 0,
            c: 'B',
            fg: Hsla::default(),
            flags: CellFlags::BOLD,
        };
        assert!(cell.flags.contains(CellFlags::BOLD));
    }

    #[test]
    fn test_render_cell_with_italic_flag() {
        let cell = RenderCell {
            row: 0,
            col: 0,
            c: 'I',
            fg: Hsla::default(),
            flags: CellFlags::ITALIC,
        };
        assert!(cell.flags.contains(CellFlags::ITALIC));
    }

    #[test]
    fn test_render_cell_with_multiple_flags() {
        let cell = RenderCell {
            row: 0,
            col: 0,
            c: 'X',
            fg: Hsla::default(),
            flags: CellFlags::BOLD | CellFlags::ITALIC | CellFlags::UNDERLINE,
        };
        assert!(cell.flags.contains(CellFlags::BOLD));
        assert!(cell.flags.contains(CellFlags::ITALIC));
        assert!(cell.flags.contains(CellFlags::UNDERLINE));
    }

    // ==================== BgRegion Tests ====================

    #[test]
    fn test_bg_region_basic() {
        let region = BgRegion {
            row: 3,
            col_start: 5,
            col_end: 15,
            color: Hsla::default(),
        };
        assert_eq!(region.row, 3);
        assert_eq!(region.col_start, 5);
        assert_eq!(region.col_end, 15);
    }

    #[test_case(0, 0, 0 ; "zero width at start")]
    #[test_case(0, 0, 80 ; "full line")]
    #[test_case(0, 40, 80 ; "half line")]
    #[test_case(100, 0, 1 ; "single column")]
    fn test_bg_region_dimensions(row: usize, col_start: usize, col_end: usize) {
        let region = BgRegion {
            row,
            col_start,
            col_end,
            color: Hsla::default(),
        };
        assert_eq!(region.row, row);
        assert_eq!(region.col_start, col_start);
        assert_eq!(region.col_end, col_end);
    }

    #[test]
    fn test_bg_region_width_calculation() {
        let region = BgRegion {
            row: 0,
            col_start: 10,
            col_end: 20,
            color: Hsla::default(),
        };
        let width = region.col_end - region.col_start;
        assert_eq!(width, 10);
    }

    // ==================== CursorInfo Tests ====================

    #[test]
    fn test_cursor_info_basic() {
        let cursor = CursorInfo {
            row: 10,
            col: 20,
            shape: CursorShape::Block,
            color: Hsla::default(),
        };
        assert_eq!(cursor.row, 10);
        assert_eq!(cursor.col, 20);
    }

    #[test]
    fn test_cursor_info_block_shape() {
        let cursor = CursorInfo {
            row: 0,
            col: 0,
            shape: CursorShape::Block,
            color: Hsla::default(),
        };
        assert!(matches!(cursor.shape, CursorShape::Block));
    }

    #[test]
    fn test_cursor_info_underline_shape() {
        let cursor = CursorInfo {
            row: 0,
            col: 0,
            shape: CursorShape::Underline,
            color: Hsla::default(),
        };
        assert!(matches!(cursor.shape, CursorShape::Underline));
    }

    #[test]
    fn test_cursor_info_beam_shape() {
        let cursor = CursorInfo {
            row: 0,
            col: 0,
            shape: CursorShape::Beam,
            color: Hsla::default(),
        };
        assert!(matches!(cursor.shape, CursorShape::Beam));
    }

    // ==================== RenderData Tests ====================

    #[test]
    fn test_render_data_empty() {
        let data = RenderData {
            cells: Vec::new(),
            bg_regions: Vec::new(),
            cursor: None,
        };
        assert!(data.cells.is_empty());
        assert!(data.bg_regions.is_empty());
        assert!(data.cursor.is_none());
    }

    #[test]
    fn test_render_data_with_cells() {
        let cells = vec![
            RenderCell {
                row: 0,
                col: 0,
                c: 'H',
                fg: Hsla::default(),
                flags: CellFlags::empty(),
            },
            RenderCell {
                row: 0,
                col: 1,
                c: 'i',
                fg: Hsla::default(),
                flags: CellFlags::empty(),
            },
        ];
        let data = RenderData {
            cells,
            bg_regions: Vec::new(),
            cursor: None,
        };
        assert_eq!(data.cells.len(), 2);
        assert_eq!(data.cells[0].c, 'H');
        assert_eq!(data.cells[1].c, 'i');
    }

    #[test]
    fn test_render_data_with_cursor() {
        let cursor = CursorInfo {
            row: 5,
            col: 10,
            shape: CursorShape::Block,
            color: Hsla::default(),
        };
        let data = RenderData {
            cells: Vec::new(),
            bg_regions: Vec::new(),
            cursor: Some(cursor),
        };
        assert!(data.cursor.is_some());
        assert_eq!(data.cursor.unwrap().row, 5);
    }

    #[test]
    fn test_render_data_with_bg_regions() {
        let regions = vec![
            BgRegion {
                row: 0,
                col_start: 0,
                col_end: 10,
                color: Hsla::default(),
            },
            BgRegion {
                row: 1,
                col_start: 5,
                col_end: 15,
                color: Hsla::default(),
            },
        ];
        let data = RenderData {
            cells: Vec::new(),
            bg_regions: regions,
            cursor: None,
        };
        assert_eq!(data.bg_regions.len(), 2);
    }

    #[test]
    fn test_render_data_complete_frame() {
        let cells = vec![
            RenderCell {
                row: 0,
                col: 0,
                c: '$',
                fg: Hsla::default(),
                flags: CellFlags::BOLD,
            },
            RenderCell {
                row: 0,
                col: 2,
                c: 'l',
                fg: Hsla::default(),
                flags: CellFlags::empty(),
            },
            RenderCell {
                row: 0,
                col: 3,
                c: 's',
                fg: Hsla::default(),
                flags: CellFlags::empty(),
            },
        ];
        let bg_regions = vec![BgRegion {
            row: 0,
            col_start: 0,
            col_end: 4,
            color: Hsla::default(),
        }];
        let cursor = CursorInfo {
            row: 0,
            col: 4,
            shape: CursorShape::Block,
            color: Hsla::default(),
        };

        let data = RenderData {
            cells,
            bg_regions,
            cursor: Some(cursor),
        };

        assert_eq!(data.cells.len(), 3);
        assert_eq!(data.bg_regions.len(), 1);
        assert!(data.cursor.is_some());
    }

    // ==================== MouseEscBuf Tests ====================

    #[test]
    fn test_mouse_esc_buf() {
        let mut buf = MouseEscBuf::new();
        write!(buf, "\x1b[<0;10;20M").unwrap();
        assert_eq!(buf.as_str(), "\x1b[<0;10;20M");
    }

    #[test]
    fn test_mouse_esc_buf_overflow() {
        let mut buf = MouseEscBuf::new();
        for _ in 0..10 {
            let _ = write!(buf, "12345");
        }
        assert_eq!(buf.len, 32);
    }

    #[test]
    fn test_mouse_esc_buf_exact_capacity() {
        let mut buf = MouseEscBuf::new();
        write!(buf, "12345678901234567890123456789012").unwrap();
        assert_eq!(buf.len, 32);
        assert_eq!(buf.as_str(), "12345678901234567890123456789012");
    }

    #[test]
    fn test_mouse_esc_buf_truncation() {
        let mut buf = MouseEscBuf::new();
        write!(buf, "123456789012345678901234567890123").unwrap();
        assert_eq!(buf.len, 32);
        assert_eq!(buf.as_str(), "12345678901234567890123456789012");
    }

    #[test]
    fn test_mouse_esc_buf_multiple_writes() {
        let mut buf = MouseEscBuf::new();
        write!(buf, "Hello").unwrap();
        write!(buf, " ").unwrap();
        write!(buf, "World").unwrap();
        assert_eq!(buf.as_str(), "Hello World");
        assert_eq!(buf.len, 11);
    }

    #[test]
    fn test_mouse_esc_buf_sgr_mouse_press() {
        let mut buf = MouseEscBuf::new();
        write!(buf, "\x1b[<0;50;25M").unwrap();
        assert_eq!(buf.as_str(), "\x1b[<0;50;25M");
    }

    #[test]
    fn test_mouse_esc_buf_sgr_mouse_release() {
        let mut buf = MouseEscBuf::new();
        write!(buf, "\x1b[<0;50;25m").unwrap();
        assert_eq!(buf.as_str(), "\x1b[<0;50;25m");
    }

    #[test]
    fn test_mouse_esc_buf_sgr_mouse_drag() {
        let mut buf = MouseEscBuf::new();
        write!(buf, "\x1b[<32;100;50M").unwrap();
        assert_eq!(buf.as_str(), "\x1b[<32;100;50M");
    }

    #[test_case("\x1b[<0;1;1M" ; "small coordinates")]
    #[test_case("\x1b[<0;999;999M" ; "large coordinates")]
    #[test_case("\x1b[<64;50;25M" ; "scroll up")]
    #[test_case("\x1b[<65;50;25M" ; "scroll down")]
    fn test_mouse_esc_buf_sgr_sequences(expected: &str) {
        let mut buf = MouseEscBuf::new();
        write!(buf, "{}", expected).unwrap();
        assert_eq!(buf.as_str(), expected);
        assert_eq!(buf.len, expected.len());
    }

    #[test]
    fn test_mouse_esc_buf_max_sgr_coordinates() {
        let mut buf = MouseEscBuf::new();
        write!(buf, "\x1b[<999;9999;9999M").unwrap();
        assert_eq!(buf.as_str(), "\x1b[<999;9999;9999M");
        assert!(buf.len <= 32);
    }

    // ==================== MouseEscBuf Property Tests ====================

    proptest! {
        #[test]
        fn prop_mouse_esc_buf_never_exceeds_capacity(s in "\\PC{0,100}") {
            let mut buf = MouseEscBuf::new();
            let _ = write!(buf, "{}", s);
            prop_assert!(buf.len <= 32);
        }

        #[test]
        fn prop_mouse_esc_buf_len_matches_content(s in "[a-zA-Z0-9]{0,32}") {
            let mut buf = MouseEscBuf::new();
            let _ = write!(buf, "{}", s);
            prop_assert_eq!(buf.len, buf.as_str().len());
        }

        #[test]
        fn prop_mouse_esc_buf_ascii_roundtrip(s in "[a-zA-Z0-9]{0,32}") {
            let mut buf = MouseEscBuf::new();
            let _ = write!(buf, "{}", s);
            prop_assert_eq!(buf.as_str(), &s[..s.len().min(32)]);
        }

        #[test]
        fn prop_mouse_esc_buf_capacity_never_exceeded(writes in proptest::collection::vec("[a-zA-Z0-9]{0,100}", 0..20)) {
            let mut buf = MouseEscBuf::new();
            for s in writes {
                let _ = write!(buf, "{}", s);
            }
            prop_assert!(buf.len <= 32, "Buffer exceeded capacity: {}", buf.len);
        }

        #[test]
        fn prop_mouse_esc_buf_sgr_arbitrary_coords(
            button in 0u8..=127,
            x in 1u16..=9999,
            y in 1u16..=9999,
            is_press: bool
        ) {
            let mut buf = MouseEscBuf::new();
            let suffix = if is_press { 'M' } else { 'm' };
            let _ = write!(buf, "\x1b[<{};{};{}{}", button, x, y, suffix);

            let s = buf.as_str();
            prop_assert!(s.starts_with("\x1b[<"));
            prop_assert!(s.ends_with(suffix));
            prop_assert!(buf.len <= 32);
        }

        #[test]
        fn prop_mouse_esc_buf_multiple_writes(
            parts in proptest::collection::vec("[a-z]{1,5}", 1..=6)
        ) {
            let mut buf = MouseEscBuf::new();
            let mut expected = String::new();

            for part in &parts {
                let _ = write!(buf, "{}", part);
                expected.push_str(part);
            }

            let expected_truncated = &expected[..expected.len().min(32)];
            prop_assert_eq!(buf.as_str(), expected_truncated);
        }
    }
}
