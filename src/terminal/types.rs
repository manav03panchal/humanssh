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

    #[test]
    fn test_mouse_esc_buf() {
        use std::fmt::Write;
        let mut buf = MouseEscBuf::new();
        write!(buf, "\x1b[<0;10;20M").unwrap();
        assert_eq!(buf.as_str(), "\x1b[<0;10;20M");
    }

    #[test]
    fn test_mouse_esc_buf_overflow() {
        use std::fmt::Write;
        let mut buf = MouseEscBuf::new();
        // Write more than 32 bytes - should truncate without panicking
        for _ in 0..10 {
            let _ = write!(buf, "12345");
        }
        assert_eq!(buf.len, 32);
    }
}
