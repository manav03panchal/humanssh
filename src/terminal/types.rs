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
use gpui::Hsla;
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
}

impl Default for DisplayState {
    fn default() -> Self {
        Self {
            size: TermSize::default(),
            cell_dims: (8.4, 17.0),
            bounds: None,
            font_size: DEFAULT_FONT_SIZE,
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

    #[test]
    fn test_term_size_clone() {
        let size = TermSize {
            cols: 100,
            rows: 50,
        };
        let cloned = size.clone();
        assert_eq!(size.cols, cloned.cols);
        assert_eq!(size.rows, cloned.rows);
    }

    #[test]
    fn test_term_size_copy() {
        let size = TermSize {
            cols: 100,
            rows: 50,
        };
        let copied = size;
        // Original still accessible (Copy trait)
        assert_eq!(size.cols, 100);
        assert_eq!(copied.cols, 100);
    }

    #[test]
    fn test_term_size_debug() {
        let size = TermSize { cols: 80, rows: 24 };
        let debug_str = format!("{:?}", size);
        assert!(debug_str.contains("80"));
        assert!(debug_str.contains("24"));
        assert!(debug_str.contains("TermSize"));
    }

    proptest! {
        #[test]
        fn prop_term_size_dimensions_consistency(cols in 0u16..=u16::MAX, rows in 0u16..=u16::MAX) {
            let size = TermSize { cols, rows };
            // total_lines and screen_lines should always be equal
            prop_assert_eq!(size.total_lines(), size.screen_lines());
            // columns should match cols
            prop_assert_eq!(size.columns(), cols as usize);
            // total_lines should match rows
            prop_assert_eq!(size.total_lines(), rows as usize);
        }

        #[test]
        fn prop_term_size_clone_equals_original(cols in 0u16..=u16::MAX, rows in 0u16..=u16::MAX) {
            let original = TermSize { cols, rows };
            let cloned = original.clone();
            prop_assert_eq!(original.cols, cloned.cols);
            prop_assert_eq!(original.rows, cloned.rows);
        }

        #[test]
        fn prop_term_size_usize_conversion_no_truncation(cols in 0u16..=u16::MAX, rows in 0u16..=u16::MAX) {
            let size = TermSize { cols, rows };
            // Ensure no data loss when converting to usize
            prop_assert!(size.columns() <= usize::MAX);
            prop_assert!(size.total_lines() <= usize::MAX);
            prop_assert_eq!(size.columns() as u16, cols);
            prop_assert_eq!(size.total_lines() as u16, rows);
        }
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
    fn test_display_state_clone() {
        let state = DisplayState {
            size: TermSize {
                cols: 120,
                rows: 40,
            },
            cell_dims: (10.0, 20.0),
            bounds: None,
            font_size: 16.0,
        };
        let cloned = state.clone();
        assert_eq!(state.size.cols, cloned.size.cols);
        assert_eq!(state.size.rows, cloned.size.rows);
        assert_eq!(state.cell_dims, cloned.cell_dims);
        assert_eq!(state.font_size, cloned.font_size);
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
    fn test_render_cell_clone() {
        let cell = RenderCell {
            row: 5,
            col: 10,
            c: 'X',
            fg: Hsla::default(),
            flags: CellFlags::BOLD,
        };
        let cloned = cell.clone();
        assert_eq!(cell.row, cloned.row);
        assert_eq!(cell.col, cloned.col);
        assert_eq!(cell.c, cloned.c);
    }

    #[test_case('A' ; "uppercase letter")]
    #[test_case('z' ; "lowercase letter")]
    #[test_case('0' ; "digit")]
    #[test_case(' ' ; "space")]
    #[test_case('\t' ; "tab")]
    #[test_case('\n' ; "newline")]
    #[test_case('\0' ; "null")]
    #[test_case('\u{1F600}' ; "emoji")]
    #[test_case('\u{4E2D}' ; "chinese character")]
    fn test_render_cell_character_types(c: char) {
        let cell = RenderCell {
            row: 0,
            col: 0,
            c,
            fg: Hsla::default(),
            flags: CellFlags::empty(),
        };
        assert_eq!(cell.c, c);
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

    #[test]
    fn test_bg_region_clone() {
        let region = BgRegion {
            row: 10,
            col_start: 0,
            col_end: 80,
            color: Hsla::default(),
        };
        let cloned = region.clone();
        assert_eq!(region.row, cloned.row);
        assert_eq!(region.col_start, cloned.col_start);
        assert_eq!(region.col_end, cloned.col_end);
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
    fn test_cursor_info_clone() {
        let cursor = CursorInfo {
            row: 5,
            col: 15,
            shape: CursorShape::Underline,
            color: Hsla::default(),
        };
        let cloned = cursor.clone();
        assert_eq!(cursor.row, cloned.row);
        assert_eq!(cursor.col, cloned.col);
    }

    #[test]
    fn test_cursor_info_copy() {
        let cursor = CursorInfo {
            row: 5,
            col: 15,
            shape: CursorShape::Block,
            color: Hsla::default(),
        };
        let copied = cursor;
        // Original still accessible (Copy trait)
        assert_eq!(cursor.row, 5);
        assert_eq!(copied.row, 5);
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
        // Write more than 32 bytes - should truncate without panicking
        for _ in 0..10 {
            let _ = write!(buf, "12345");
        }
        assert_eq!(buf.len, 32);
    }

    #[test]
    fn test_mouse_esc_buf_new() {
        let buf = MouseEscBuf::new();
        assert_eq!(buf.len, 0);
        assert_eq!(buf.as_str(), "");
    }

    #[test]
    fn test_mouse_esc_buf_default() {
        let buf = MouseEscBuf::default();
        assert_eq!(buf.len, 0);
        assert_eq!(buf.as_str(), "");
    }

    #[test]
    fn test_mouse_esc_buf_default_equals_new() {
        let new = MouseEscBuf::new();
        let default = MouseEscBuf::default();
        assert_eq!(new.len, default.len);
        assert_eq!(new.as_str(), default.as_str());
    }

    #[test]
    fn test_mouse_esc_buf_empty_write() {
        let mut buf = MouseEscBuf::new();
        write!(buf, "").unwrap();
        assert_eq!(buf.len, 0);
        assert_eq!(buf.as_str(), "");
    }

    #[test]
    fn test_mouse_esc_buf_single_char() {
        let mut buf = MouseEscBuf::new();
        write!(buf, "X").unwrap();
        assert_eq!(buf.len, 1);
        assert_eq!(buf.as_str(), "X");
    }

    #[test]
    fn test_mouse_esc_buf_exact_capacity() {
        let mut buf = MouseEscBuf::new();
        // Write exactly 32 bytes
        write!(buf, "12345678901234567890123456789012").unwrap();
        assert_eq!(buf.len, 32);
        assert_eq!(buf.as_str(), "12345678901234567890123456789012");
    }

    #[test]
    fn test_mouse_esc_buf_truncation() {
        let mut buf = MouseEscBuf::new();
        // Write 33 bytes - should truncate to 32
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
        // SGR mouse press format: \x1b[<button;x;yM
        write!(buf, "\x1b[<0;50;25M").unwrap();
        assert_eq!(buf.as_str(), "\x1b[<0;50;25M");
    }

    #[test]
    fn test_mouse_esc_buf_sgr_mouse_release() {
        let mut buf = MouseEscBuf::new();
        // SGR mouse release format: \x1b[<button;x;ym
        write!(buf, "\x1b[<0;50;25m").unwrap();
        assert_eq!(buf.as_str(), "\x1b[<0;50;25m");
    }

    #[test]
    fn test_mouse_esc_buf_sgr_mouse_drag() {
        let mut buf = MouseEscBuf::new();
        // SGR mouse drag format: button + 32
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
    fn test_mouse_esc_buf_write_result_always_ok() {
        let mut buf = MouseEscBuf::new();
        // Even overflow writes return Ok
        for _ in 0..100 {
            let result = write!(buf, "test");
            assert!(result.is_ok());
        }
    }

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
    }

    // ==================== Integration Tests ====================

    #[test]
    fn test_render_data_complete_frame() {
        // Simulate a complete render frame with all components
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
        };

        assert_eq!(state.size.cols, 132);
        assert_eq!(state.size.rows, 43);
        assert_eq!(state.size.columns(), 132);
        assert_eq!(state.size.total_lines(), 43);
    }

    // ==================== Boundary Condition Tests ====================

    // --- TermSize Boundary Tests ---

    #[test]
    fn test_term_size_zero_dimensions() {
        let size = TermSize { cols: 0, rows: 0 };
        assert_eq!(size.cols, 0);
        assert_eq!(size.rows, 0);
        assert_eq!(size.columns(), 0);
        assert_eq!(size.total_lines(), 0);
        assert_eq!(size.screen_lines(), 0);
    }

    #[test]
    fn test_term_size_one_dimension() {
        let size = TermSize { cols: 1, rows: 1 };
        assert_eq!(size.cols, 1);
        assert_eq!(size.rows, 1);
        assert_eq!(size.columns(), 1);
        assert_eq!(size.total_lines(), 1);
        assert_eq!(size.screen_lines(), 1);
    }

    #[test]
    fn test_term_size_max_dimensions() {
        let size = TermSize {
            cols: u16::MAX,
            rows: u16::MAX,
        };
        assert_eq!(size.cols, u16::MAX);
        assert_eq!(size.rows, u16::MAX);
        assert_eq!(size.columns(), u16::MAX as usize);
        assert_eq!(size.total_lines(), u16::MAX as usize);
        assert_eq!(size.screen_lines(), u16::MAX as usize);
    }

    #[test]
    fn test_term_size_asymmetric_zero_cols() {
        let size = TermSize { cols: 0, rows: 100 };
        assert_eq!(size.columns(), 0);
        assert_eq!(size.total_lines(), 100);
    }

    #[test]
    fn test_term_size_asymmetric_zero_rows() {
        let size = TermSize { cols: 100, rows: 0 };
        assert_eq!(size.columns(), 100);
        assert_eq!(size.total_lines(), 0);
    }

    #[test]
    fn test_term_size_max_cols_zero_rows() {
        let size = TermSize {
            cols: u16::MAX,
            rows: 0,
        };
        assert_eq!(size.columns(), u16::MAX as usize);
        assert_eq!(size.total_lines(), 0);
    }

    #[test]
    fn test_term_size_zero_cols_max_rows() {
        let size = TermSize {
            cols: 0,
            rows: u16::MAX,
        };
        assert_eq!(size.columns(), 0);
        assert_eq!(size.total_lines(), u16::MAX as usize);
    }

    #[test_case(0, 0, 0, 0 ; "zero_zero")]
    #[test_case(1, 0, 1, 0 ; "one_zero")]
    #[test_case(0, 1, 0, 1 ; "zero_one")]
    #[test_case(1, 1, 1, 1 ; "one_one")]
    #[test_case(u16::MAX, 0, u16::MAX as usize, 0 ; "max_zero")]
    #[test_case(0, u16::MAX, 0, u16::MAX as usize ; "zero_max")]
    #[test_case(u16::MAX, u16::MAX, u16::MAX as usize, u16::MAX as usize ; "max_max")]
    #[test_case(u16::MAX, 1, u16::MAX as usize, 1 ; "max_one")]
    #[test_case(1, u16::MAX, 1, u16::MAX as usize ; "one_max")]
    fn test_term_size_boundary_matrix(
        cols: u16,
        rows: u16,
        expected_cols: usize,
        expected_rows: usize,
    ) {
        let size = TermSize { cols, rows };
        assert_eq!(size.columns(), expected_cols);
        assert_eq!(size.total_lines(), expected_rows);
        assert_eq!(size.screen_lines(), expected_rows);
    }

    // --- MouseEscBuf Boundary Tests ---

    #[test]
    fn test_mouse_esc_buf_exactly_32_bytes() {
        let mut buf = MouseEscBuf::new();
        let data = "01234567890123456789012345678901"; // Exactly 32 bytes
        write!(buf, "{}", data).unwrap();
        assert_eq!(buf.len, 32);
        assert_eq!(buf.as_str(), data);
    }

    #[test]
    fn test_mouse_esc_buf_31_bytes() {
        let mut buf = MouseEscBuf::new();
        let data = "0123456789012345678901234567890"; // 31 bytes
        write!(buf, "{}", data).unwrap();
        assert_eq!(buf.len, 31);
        assert_eq!(buf.as_str(), data);
    }

    #[test]
    fn test_mouse_esc_buf_33_bytes_truncates() {
        let mut buf = MouseEscBuf::new();
        let data = "012345678901234567890123456789012"; // 33 bytes
        write!(buf, "{}", data).unwrap();
        assert_eq!(buf.len, 32);
        assert_eq!(buf.as_str(), &data[..32]);
    }

    #[test]
    fn test_mouse_esc_buf_incremental_fill_to_capacity() {
        let mut buf = MouseEscBuf::new();
        for i in 0..32 {
            write!(buf, "{}", (i % 10)).unwrap();
        }
        assert_eq!(buf.len, 32);
        // Adding more should not increase length
        write!(buf, "X").unwrap();
        assert_eq!(buf.len, 32);
    }

    #[test]
    fn test_mouse_esc_buf_large_single_write() {
        let mut buf = MouseEscBuf::new();
        let large = "A".repeat(1000);
        write!(buf, "{}", large).unwrap();
        assert_eq!(buf.len, 32);
        assert_eq!(buf.as_str(), "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
    }

    #[test]
    fn test_mouse_esc_buf_max_sgr_coordinates() {
        let mut buf = MouseEscBuf::new();
        // Maximum realistic SGR coordinates: \x1b[<999;9999;9999M
        write!(buf, "\x1b[<999;9999;9999M").unwrap();
        assert_eq!(buf.as_str(), "\x1b[<999;9999;9999M");
        assert!(buf.len <= 32);
    }

    #[test]
    fn test_mouse_esc_buf_edge_coordinates() {
        let mut buf = MouseEscBuf::new();
        // Minimum coordinates
        write!(buf, "\x1b[<0;1;1M").unwrap();
        assert_eq!(buf.as_str(), "\x1b[<0;1;1M");
    }

    #[test_case("\x1b[<0;0;0M", 9 ; "zero_coordinates")]
    #[test_case("\x1b[<0;1;1M", 9 ; "min_valid_coordinates")]
    #[test_case("\x1b[<999;999;999M", 15 ; "three_digit_coords")]
    #[test_case("\x1b[<999;9999;9999M", 17 ; "four_digit_coords")]
    fn test_mouse_esc_buf_sgr_coordinate_sizes(seq: &str, expected_len: usize) {
        let mut buf = MouseEscBuf::new();
        write!(buf, "{}", seq).unwrap();
        assert_eq!(buf.len, expected_len);
        assert_eq!(buf.as_str(), seq);
    }

    // --- Coordinate Boundary Tests (RenderCell, BgRegion, CursorInfo) ---

    #[test_case(0, 0 ; "origin")]
    #[test_case(0, 1 ; "first_col")]
    #[test_case(1, 0 ; "first_row")]
    #[test_case(usize::MAX, 0 ; "max_row_zero_col")]
    #[test_case(0, usize::MAX ; "zero_row_max_col")]
    #[test_case(usize::MAX, usize::MAX ; "max_max")]
    fn test_render_cell_coordinate_boundaries(row: usize, col: usize) {
        let cell = RenderCell {
            row,
            col,
            c: 'X',
            fg: Hsla::default(),
            flags: CellFlags::empty(),
        };
        assert_eq!(cell.row, row);
        assert_eq!(cell.col, col);
    }

    #[test_case(0, 0, 0 ; "zero_width_at_origin")]
    #[test_case(0, 0, 1 ; "single_cell_at_origin")]
    #[test_case(0, 0, usize::MAX ; "max_width_at_origin")]
    #[test_case(usize::MAX, 0, 1 ; "max_row_single_cell")]
    #[test_case(0, usize::MAX - 1, usize::MAX ; "near_max_col")]
    fn test_bg_region_coordinate_boundaries(row: usize, col_start: usize, col_end: usize) {
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

    #[test_case(0, 0 ; "cursor_at_origin")]
    #[test_case(0, 1 ; "cursor_first_col")]
    #[test_case(1, 0 ; "cursor_first_row")]
    #[test_case(usize::MAX, usize::MAX ; "cursor_at_max")]
    fn test_cursor_info_coordinate_boundaries(row: usize, col: usize) {
        let cursor = CursorInfo {
            row,
            col,
            shape: CursorShape::Block,
            color: Hsla::default(),
        };
        assert_eq!(cursor.row, row);
        assert_eq!(cursor.col, col);
    }

    // --- String/Character Boundary Tests ---

    #[test]
    fn test_render_cell_empty_char() {
        let cell = RenderCell {
            row: 0,
            col: 0,
            c: '\0',
            fg: Hsla::default(),
            flags: CellFlags::empty(),
        };
        assert_eq!(cell.c, '\0');
    }

    #[test]
    fn test_render_cell_max_char() {
        let cell = RenderCell {
            row: 0,
            col: 0,
            c: char::MAX,
            fg: Hsla::default(),
            flags: CellFlags::empty(),
        };
        assert_eq!(cell.c, char::MAX);
    }

    #[test]
    fn test_render_cell_replacement_char() {
        let cell = RenderCell {
            row: 0,
            col: 0,
            c: char::REPLACEMENT_CHARACTER,
            fg: Hsla::default(),
            flags: CellFlags::empty(),
        };
        assert_eq!(cell.c, char::REPLACEMENT_CHARACTER);
    }

    #[test_case('\0' ; "null_char")]
    #[test_case('\x01' ; "control_char_soh")]
    #[test_case('\x1b' ; "escape_char")]
    #[test_case('\x7f' ; "delete_char")]
    #[test_case(' ' ; "space")]
    #[test_case('~' ; "tilde_last_printable_ascii")]
    #[test_case('\u{0080}' ; "first_extended_ascii")]
    #[test_case('\u{FFFF}' ; "bmp_max")]
    #[test_case('\u{10000}' ; "first_supplementary")]
    #[test_case('\u{10FFFF}' ; "max_unicode")]
    fn test_render_cell_char_range(c: char) {
        let cell = RenderCell {
            row: 0,
            col: 0,
            c,
            fg: Hsla::default(),
            flags: CellFlags::empty(),
        };
        assert_eq!(cell.c, c);
    }

    #[test]
    fn test_mouse_esc_buf_with_empty_writes() {
        let mut buf = MouseEscBuf::new();
        for _ in 0..100 {
            write!(buf, "").unwrap();
        }
        assert_eq!(buf.len, 0);
        assert_eq!(buf.as_str(), "");
    }

    #[test]
    fn test_mouse_esc_buf_single_char_repeated() {
        let mut buf = MouseEscBuf::new();
        for _ in 0..50 {
            write!(buf, "X").unwrap();
        }
        assert_eq!(buf.len, 32);
        assert_eq!(buf.as_str(), "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX");
    }

    // --- DisplayState Boundary Tests ---

    #[test]
    fn test_display_state_zero_cell_dims() {
        let state = DisplayState {
            size: TermSize::default(),
            cell_dims: (0.0, 0.0),
            bounds: None,
            font_size: 14.0,
        };
        assert_eq!(state.cell_dims.0, 0.0);
        assert_eq!(state.cell_dims.1, 0.0);
    }

    #[test]
    fn test_display_state_max_cell_dims() {
        let state = DisplayState {
            size: TermSize::default(),
            cell_dims: (f32::MAX, f32::MAX),
            bounds: None,
            font_size: 14.0,
        };
        assert_eq!(state.cell_dims.0, f32::MAX);
        assert_eq!(state.cell_dims.1, f32::MAX);
    }

    #[test]
    fn test_display_state_inf_cell_dims() {
        let state = DisplayState {
            size: TermSize::default(),
            cell_dims: (f32::INFINITY, f32::INFINITY),
            bounds: None,
            font_size: 14.0,
        };
        assert!(state.cell_dims.0.is_infinite());
        assert!(state.cell_dims.1.is_infinite());
    }

    #[test]
    fn test_display_state_nan_cell_dims() {
        let state = DisplayState {
            size: TermSize::default(),
            cell_dims: (f32::NAN, f32::NAN),
            bounds: None,
            font_size: 14.0,
        };
        assert!(state.cell_dims.0.is_nan());
        assert!(state.cell_dims.1.is_nan());
    }

    #[test_case(0.0 ; "zero_font")]
    #[test_case(0.001 ; "tiny_font")]
    #[test_case(1.0 ; "one_point")]
    #[test_case(72.0 ; "inch_font")]
    #[test_case(f32::MAX ; "max_font")]
    fn test_display_state_font_size_boundaries(font_size: f32) {
        let state = DisplayState {
            size: TermSize::default(),
            cell_dims: (8.0, 16.0),
            bounds: None,
            font_size,
        };
        assert_eq!(state.font_size, font_size);
    }

    // --- RenderData Boundary Tests ---

    #[test]
    fn test_render_data_large_cells_vec() {
        let cells: Vec<RenderCell> = (0..10000)
            .map(|i| RenderCell {
                row: i,
                col: i,
                c: 'X',
                fg: Hsla::default(),
                flags: CellFlags::empty(),
            })
            .collect();
        let data = RenderData {
            cells,
            bg_regions: Vec::new(),
            cursor: None,
        };
        assert_eq!(data.cells.len(), 10000);
    }

    #[test]
    fn test_render_data_large_bg_regions_vec() {
        let regions: Vec<BgRegion> = (0..10000)
            .map(|i| BgRegion {
                row: i,
                col_start: 0,
                col_end: 80,
                color: Hsla::default(),
            })
            .collect();
        let data = RenderData {
            cells: Vec::new(),
            bg_regions: regions,
            cursor: None,
        };
        assert_eq!(data.bg_regions.len(), 10000);
    }

    // --- Additional Proptest Boundary Tests ---

    proptest! {
        #[test]
        fn prop_mouse_esc_buf_capacity_never_exceeded(writes in proptest::collection::vec("[a-zA-Z0-9]{0,100}", 0..20)) {
            let mut buf = MouseEscBuf::new();
            for s in writes {
                let _ = write!(buf, "{}", s);
            }
            prop_assert!(buf.len <= 32, "Buffer exceeded capacity: {}", buf.len);
        }

        #[test]
        fn prop_term_size_dimensions_trait_consistency(cols in 0u16..=u16::MAX, rows in 0u16..=u16::MAX) {
            let size = TermSize { cols, rows };
            // The Dimensions trait methods should be consistent
            prop_assert_eq!(size.total_lines(), size.screen_lines());
            prop_assert_eq!(size.columns(), cols as usize);
            prop_assert_eq!(size.total_lines(), rows as usize);
        }

        #[test]
        fn prop_render_cell_row_col_preserved(row in 0usize..=1000000, col in 0usize..=1000000) {
            let cell = RenderCell {
                row,
                col,
                c: 'X',
                fg: Hsla::default(),
                flags: CellFlags::empty(),
            };
            prop_assert_eq!(cell.row, row);
            prop_assert_eq!(cell.col, col);
        }

        #[test]
        fn prop_bg_region_dimensions_preserved(row in 0usize..=1000000, col_start in 0usize..=1000000, col_end in 0usize..=1000000) {
            let region = BgRegion {
                row,
                col_start,
                col_end,
                color: Hsla::default(),
            };
            prop_assert_eq!(region.row, row);
            prop_assert_eq!(region.col_start, col_start);
            prop_assert_eq!(region.col_end, col_end);
        }
    }

    // ==================== Expanded Property-Based Tests (1000 cases) ====================

    /// Strategy for generating arbitrary CellFlags combinations
    fn arb_cell_flags() -> impl Strategy<Value = CellFlags> {
        prop::bits::u16::ANY.prop_map(|bits| {
            // CellFlags is a bitflags type, create from raw bits
            // Only use known flag bits to avoid undefined behavior
            let mut flags = CellFlags::empty();
            if bits & 0x0001 != 0 {
                flags |= CellFlags::INVERSE;
            }
            if bits & 0x0002 != 0 {
                flags |= CellFlags::BOLD;
            }
            if bits & 0x0004 != 0 {
                flags |= CellFlags::ITALIC;
            }
            if bits & 0x0008 != 0 {
                flags |= CellFlags::BOLD_ITALIC;
            }
            if bits & 0x0010 != 0 {
                flags |= CellFlags::UNDERLINE;
            }
            if bits & 0x0020 != 0 {
                flags |= CellFlags::DOUBLE_UNDERLINE;
            }
            if bits & 0x0040 != 0 {
                flags |= CellFlags::UNDERCURL;
            }
            if bits & 0x0080 != 0 {
                flags |= CellFlags::DOTTED_UNDERLINE;
            }
            if bits & 0x0100 != 0 {
                flags |= CellFlags::DASHED_UNDERLINE;
            }
            if bits & 0x0200 != 0 {
                flags |= CellFlags::DIM;
            }
            if bits & 0x0400 != 0 {
                flags |= CellFlags::HIDDEN;
            }
            if bits & 0x0800 != 0 {
                flags |= CellFlags::STRIKEOUT;
            }
            if bits & 0x1000 != 0 {
                flags |= CellFlags::WRAPLINE;
            }
            if bits & 0x2000 != 0 {
                flags |= CellFlags::WIDE_CHAR;
            }
            if bits & 0x4000 != 0 {
                flags |= CellFlags::WIDE_CHAR_SPACER;
            }
            flags
        })
    }

    /// Strategy for generating arbitrary HSLA colors
    fn arb_hsla() -> impl Strategy<Value = Hsla> {
        (0.0f32..=1.0, 0.0f32..=1.0, 0.0f32..=1.0, 0.0f32..=1.0)
            .prop_map(|(h, s, l, a)| gpui::hsla(h, s, l, a))
    }

    /// Strategy for generating arbitrary cursor shapes
    fn arb_cursor_shape() -> impl Strategy<Value = CursorShape> {
        prop_oneof![
            Just(CursorShape::Block),
            Just(CursorShape::Underline),
            Just(CursorShape::Beam),
            Just(CursorShape::HollowBlock),
            Just(CursorShape::Hidden),
        ]
    }

    proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(1000))]

        // ==================== TermSize Property Tests ====================

        /// Property: TermSize dimensions should always be valid u16 values
        #[test]
        fn prop_term_size_any_valid_dimensions(cols in 0u16..=u16::MAX, rows in 0u16..=u16::MAX) {
            let size = TermSize { cols, rows };

            // Invariant: dimensions must match input
            prop_assert_eq!(size.cols, cols);
            prop_assert_eq!(size.rows, rows);

            // Invariant: Dimensions trait methods consistent
            prop_assert_eq!(size.columns(), cols as usize);
            prop_assert_eq!(size.total_lines(), rows as usize);
            prop_assert_eq!(size.screen_lines(), rows as usize);

            // Invariant: total_lines always equals screen_lines
            prop_assert_eq!(size.total_lines(), size.screen_lines());
        }

        /// Property: TermSize Clone produces identical copies
        #[test]
        fn prop_term_size_clone_identity(cols in 0u16..=u16::MAX, rows in 0u16..=u16::MAX) {
            let original = TermSize { cols, rows };
            let cloned = original.clone();

            prop_assert_eq!(original.cols, cloned.cols);
            prop_assert_eq!(original.rows, cloned.rows);
            prop_assert_eq!(original.columns(), cloned.columns());
            prop_assert_eq!(original.total_lines(), cloned.total_lines());
        }

        /// Property: TermSize Copy semantics work correctly
        #[test]
        fn prop_term_size_copy_semantics(cols in 0u16..=u16::MAX, rows in 0u16..=u16::MAX) {
            let original = TermSize { cols, rows };
            let copied = original; // Copy, not move

            // Both should be accessible and equal
            prop_assert_eq!(original.cols, copied.cols);
            prop_assert_eq!(original.rows, copied.rows);
        }

        /// Property: TermSize usize conversions never truncate
        #[test]
        fn prop_term_size_usize_no_truncation(cols in 0u16..=u16::MAX, rows in 0u16..=u16::MAX) {
            let size = TermSize { cols, rows };

            // usize is always >= u16, so no truncation should occur
            let cols_usize = size.columns();
            let rows_usize = size.total_lines();

            // Verify roundtrip works (within u16 range)
            prop_assert_eq!(cols_usize as u16, cols);
            prop_assert_eq!(rows_usize as u16, rows);
        }

        // ==================== RenderCell Property Tests ====================

        /// Property: RenderCell with arbitrary characters and flags maintains validity
        #[test]
        fn prop_render_cell_arbitrary_char_and_flags(
            row in 0usize..10000,
            col in 0usize..10000,
            c in proptest::char::any(),
            flags in arb_cell_flags(),
            fg in arb_hsla()
        ) {
            let cell = RenderCell { row, col, c, fg, flags };

            // All values should be preserved exactly
            prop_assert_eq!(cell.row, row);
            prop_assert_eq!(cell.col, col);
            prop_assert_eq!(cell.c, c);
            prop_assert_eq!(cell.flags, flags);
        }

        /// Property: RenderCell Clone preserves all fields
        #[test]
        fn prop_render_cell_clone_preserves_all(
            row in 0usize..10000,
            col in 0usize..10000,
            c in proptest::char::any(),
            flags in arb_cell_flags()
        ) {
            let original = RenderCell {
                row,
                col,
                c,
                fg: Hsla::default(),
                flags,
            };
            let cloned = original.clone();

            prop_assert_eq!(original.row, cloned.row);
            prop_assert_eq!(original.col, cloned.col);
            prop_assert_eq!(original.c, cloned.c);
            prop_assert_eq!(original.flags, cloned.flags);
        }

        /// Property: RenderCell handles all Unicode codepoints
        #[test]
        fn prop_render_cell_unicode_chars(c in proptest::char::any()) {
            let cell = RenderCell {
                row: 0,
                col: 0,
                c,
                fg: Hsla::default(),
                flags: CellFlags::empty(),
            };

            prop_assert_eq!(cell.c, c);
            prop_assert!(cell.c.len_utf8() >= 1 && cell.c.len_utf8() <= 4);
        }

        /// Property: RenderCell flag combinations are valid
        #[test]
        fn prop_render_cell_flag_combinations(flags in arb_cell_flags()) {
            let cell = RenderCell {
                row: 0,
                col: 0,
                c: 'X',
                fg: Hsla::default(),
                flags,
            };

            // Verify flags are stored correctly
            prop_assert_eq!(cell.flags, flags);

            // Test flag containment operations
            if flags.contains(CellFlags::BOLD) {
                prop_assert!(cell.flags.contains(CellFlags::BOLD));
            }
            if flags.contains(CellFlags::ITALIC) {
                prop_assert!(cell.flags.contains(CellFlags::ITALIC));
            }
        }

        // ==================== BgRegion Property Tests ====================

        /// Property: BgRegion with arbitrary coordinates is always valid
        #[test]
        fn prop_bg_region_arbitrary_coords(
            row in 0usize..10000,
            col_start in 0usize..10000,
            col_end in 0usize..10000,
            h in 0.0f32..=1.0,
            s in 0.0f32..=1.0,
            l in 0.0f32..=1.0,
            a in 0.0f32..=1.0
        ) {
            let color = gpui::hsla(h, s, l, a);
            let region = BgRegion { row, col_start, col_end, color };

            // All values should be preserved
            prop_assert_eq!(region.row, row);
            prop_assert_eq!(region.col_start, col_start);
            prop_assert_eq!(region.col_end, col_end);
        }

        /// Property: BgRegion Clone preserves all fields
        #[test]
        fn prop_bg_region_clone_preserves_all(
            row in 0usize..10000,
            col_start in 0usize..5000,
            col_end in 0usize..5000
        ) {
            let region = BgRegion {
                row,
                col_start,
                col_end,
                color: Hsla::default(),
            };
            let cloned = region.clone();

            prop_assert_eq!(region.row, cloned.row);
            prop_assert_eq!(region.col_start, cloned.col_start);
            prop_assert_eq!(region.col_end, cloned.col_end);
        }

        /// Property: BgRegion width calculation is consistent
        #[test]
        fn prop_bg_region_width_consistency(
            row in 0usize..10000,
            col_start in 0usize..5000,
            width in 0usize..5000
        ) {
            let col_end = col_start + width;
            let region = BgRegion {
                row,
                col_start,
                col_end,
                color: Hsla::default(),
            };

            // Width should be col_end - col_start
            let calculated_width = region.col_end - region.col_start;
            prop_assert_eq!(calculated_width, width);
        }

        /// Property: BgRegion handles zero-width regions
        #[test]
        fn prop_bg_region_zero_width(row in 0usize..10000, col in 0usize..10000) {
            let region = BgRegion {
                row,
                col_start: col,
                col_end: col, // Zero width
                color: Hsla::default(),
            };

            prop_assert_eq!(region.col_end - region.col_start, 0);
        }

        // ==================== CursorInfo Property Tests ====================

        /// Property: CursorInfo handles all cursor shapes correctly
        #[test]
        fn prop_cursor_info_all_shapes(
            row in 0usize..10000,
            col in 0usize..10000,
            shape in arb_cursor_shape(),
            color in arb_hsla()
        ) {
            let cursor = CursorInfo { row, col, shape, color };

            prop_assert_eq!(cursor.row, row);
            prop_assert_eq!(cursor.col, col);

            // Verify shape is one of the valid variants
            match cursor.shape {
                CursorShape::Block |
                CursorShape::Underline |
                CursorShape::Beam |
                CursorShape::HollowBlock |
                CursorShape::Hidden => { /* Valid */ }
            }
        }

        /// Property: CursorInfo Clone preserves all fields
        #[test]
        fn prop_cursor_info_clone_preserves_all(
            row in 0usize..10000,
            col in 0usize..10000,
            shape in arb_cursor_shape()
        ) {
            let original = CursorInfo {
                row,
                col,
                shape,
                color: Hsla::default(),
            };
            let cloned = original.clone();

            prop_assert_eq!(original.row, cloned.row);
            prop_assert_eq!(original.col, cloned.col);
        }

        /// Property: CursorInfo Copy semantics work correctly
        #[test]
        fn prop_cursor_info_copy_semantics(
            row in 0usize..10000,
            col in 0usize..10000,
            shape in arb_cursor_shape()
        ) {
            let original = CursorInfo {
                row,
                col,
                shape,
                color: Hsla::default(),
            };
            let copied = original; // Copy, not move

            // Both should be accessible
            prop_assert_eq!(original.row, copied.row);
            prop_assert_eq!(original.col, copied.col);
        }

        // ==================== MouseEscBuf Extended Property Tests ====================

        /// Property: MouseEscBuf handles SGR mouse sequences with arbitrary coordinates
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

            // Buffer should contain valid UTF-8
            let s = buf.as_str();
            prop_assert!(s.starts_with("\x1b[<"));
            prop_assert!(s.ends_with(suffix));

            // Length should not exceed capacity
            prop_assert!(buf.len <= 32);
        }

        /// Property: MouseEscBuf multiple writes accumulate correctly
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

            // Result should be truncated to 32 chars
            let expected_truncated = &expected[..expected.len().min(32)];
            prop_assert_eq!(buf.as_str(), expected_truncated);
        }

        // ==================== DisplayState Property Tests ====================

        /// Property: DisplayState with arbitrary dimensions is valid
        #[test]
        fn prop_display_state_arbitrary_dims(
            cols in 1u16..=1000,
            rows in 1u16..=500,
            cell_w in 1.0f32..100.0,
            cell_h in 1.0f32..100.0,
            font_size in 6.0f32..72.0
        ) {
            let state = DisplayState {
                size: TermSize { cols, rows },
                cell_dims: (cell_w, cell_h),
                bounds: None,
                font_size,
            };

            prop_assert_eq!(state.size.cols, cols);
            prop_assert_eq!(state.size.rows, rows);
            prop_assert_eq!(state.cell_dims.0, cell_w);
            prop_assert_eq!(state.cell_dims.1, cell_h);
            prop_assert_eq!(state.font_size, font_size);
        }

        /// Property: DisplayState Clone preserves all fields
        #[test]
        fn prop_display_state_clone_preserves_all(
            cols in 1u16..=1000,
            rows in 1u16..=500,
            font_size in 6.0f32..72.0
        ) {
            let original = DisplayState {
                size: TermSize { cols, rows },
                cell_dims: (10.0, 20.0),
                bounds: None,
                font_size,
            };
            let cloned = original.clone();

            prop_assert_eq!(original.size.cols, cloned.size.cols);
            prop_assert_eq!(original.size.rows, cloned.size.rows);
            prop_assert_eq!(original.cell_dims, cloned.cell_dims);
            prop_assert_eq!(original.font_size, cloned.font_size);
        }

        // ==================== RenderData Property Tests ====================

        /// Property: RenderData with arbitrary cell counts is valid
        #[test]
        fn prop_render_data_arbitrary_cell_count(cell_count in 0usize..1000) {
            let cells: Vec<RenderCell> = (0..cell_count)
                .map(|i| RenderCell {
                    row: i / 80,
                    col: i % 80,
                    c: 'X',
                    fg: Hsla::default(),
                    flags: CellFlags::empty(),
                })
                .collect();

            let data = RenderData {
                cells,
                bg_regions: Vec::new(),
                cursor: None,
            };

            prop_assert_eq!(data.cells.len(), cell_count);
        }

        /// Property: RenderData with arbitrary bg_region counts is valid
        #[test]
        fn prop_render_data_arbitrary_bg_regions(region_count in 0usize..100) {
            let bg_regions: Vec<BgRegion> = (0..region_count)
                .map(|i| BgRegion {
                    row: i,
                    col_start: 0,
                    col_end: 80,
                    color: Hsla::default(),
                })
                .collect();

            let data = RenderData {
                cells: Vec::new(),
                bg_regions,
                cursor: None,
            };

            prop_assert_eq!(data.bg_regions.len(), region_count);
        }

        /// Property: RenderData with all components maintains integrity
        #[test]
        fn prop_render_data_full_frame(
            cell_count in 0usize..100,
            region_count in 0usize..20,
            cursor_row in 0usize..100,
            cursor_col in 0usize..100,
            cursor_shape in arb_cursor_shape()
        ) {
            let cells: Vec<RenderCell> = (0..cell_count)
                .map(|i| RenderCell {
                    row: i / 80,
                    col: i % 80,
                    c: 'X',
                    fg: Hsla::default(),
                    flags: CellFlags::empty(),
                })
                .collect();

            let bg_regions: Vec<BgRegion> = (0..region_count)
                .map(|i| BgRegion {
                    row: i,
                    col_start: 0,
                    col_end: 80,
                    color: Hsla::default(),
                })
                .collect();

            let cursor = CursorInfo {
                row: cursor_row,
                col: cursor_col,
                shape: cursor_shape,
                color: Hsla::default(),
            };

            let data = RenderData {
                cells,
                bg_regions,
                cursor: Some(cursor),
            };

            prop_assert_eq!(data.cells.len(), cell_count);
            prop_assert_eq!(data.bg_regions.len(), region_count);
            prop_assert!(data.cursor.is_some());
            prop_assert_eq!(data.cursor.unwrap().row, cursor_row);
            prop_assert_eq!(data.cursor.unwrap().col, cursor_col);
        }
    }
}
