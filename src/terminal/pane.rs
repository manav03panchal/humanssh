//! Terminal pane component using alacritty_terminal.
//!
//! Uses GPUI's canvas for efficient GPU-accelerated rendering with:
//! - Batched text runs via StyledText
//! - Merged background regions via paint_quad
//! - Proper handling of TUI applications

use super::PtyHandler;
use crate::theme::{terminal_colors, TerminalColors};
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point as TermPoint, Side};
use alacritty_terminal::selection::{Selection as TermSelection, SelectionType};
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::color::Colors as TermColors;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Processor, Rgb};
use gpui::*;
use parking_lot::Mutex as ParkingMutex;
use std::sync::{Arc, Mutex};

// Font configuration
const DEFAULT_FONT_SIZE: f32 = 14.0;
const MIN_FONT_SIZE: f32 = 8.0;
const MAX_FONT_SIZE: f32 = 32.0;
const FONT_FAMILY: &str = "Iosevka Nerd Font";
const PADDING: f32 = 2.0;

/// Cache for cell dimensions to avoid recalculating every frame.
/// Key is font_size (as bits), value is (width, height).
static CELL_DIMS_CACHE: ParkingMutex<Option<(u32, f32, f32)>> = ParkingMutex::new(None);

/// Calculate cell dimensions from actual font metrics (cached).
fn get_cell_dimensions(window: &mut Window, font_size: f32) -> (f32, f32) {
    let font_size_bits = font_size.to_bits();

    // Fast path: return cached value if font size matches
    {
        let cache = CELL_DIMS_CACHE.lock();
        if let Some((cached_size, width, height)) = *cache {
            if cached_size == font_size_bits {
                return (width, height);
            }
        }
    }

    // Slow path: calculate and cache
    let (width, height) = calculate_cell_dimensions(window, font_size);
    *CELL_DIMS_CACHE.lock() = Some((font_size_bits, width, height));
    (width, height)
}

/// Actually calculate cell dimensions from font metrics.
fn calculate_cell_dimensions(window: &mut Window, font_size: f32) -> (f32, f32) {
    let font = Font {
        family: FONT_FAMILY.into(),
        features: FontFeatures::default(),
        fallbacks: None,
        weight: FontWeight::NORMAL,
        style: FontStyle::Normal,
    };
    let font_size_px = px(font_size);

    // Measure a single character to get the cell width
    let runs = vec![TextRun {
        len: 1,
        font: font.clone(),
        color: black(),
        background_color: None,
        underline: None,
        strikethrough: None,
    }];

    // Shape a single 'M' character (full-width in monospace)
    let shaped = window
        .text_system()
        .shape_line("M".into(), font_size_px, &runs, None);
    let cell_width = shaped.width.into();

    // Use ascent + descent + some line spacing for cell height
    let cell_height = font_size * 1.2;

    (cell_width, cell_height)
}

/// A background region to be painted
#[derive(Clone)]
struct BgRegion {
    row: usize,
    col_start: usize,
    col_end: usize,
    color: Hsla,
}

/// Cursor rendering info
#[derive(Clone, Copy)]
struct CursorInfo {
    row: usize,
    col: usize,
    shape: CursorShape,
    color: Hsla,
}

/// A single cell to render
#[derive(Clone)]
struct RenderCell {
    row: usize,
    col: usize,
    c: char,
    fg: Hsla,
    flags: CellFlags,
}

/// Pre-computed render data for a single frame
struct RenderData {
    /// Cells to render (non-space cells only)
    cells: Vec<RenderCell>,
    /// Background regions (non-default backgrounds only)
    bg_regions: Vec<BgRegion>,
    /// Cursor info (separate from bg_regions for proper shape handling)
    cursor: Option<CursorInfo>,
}

/// Event listener that captures terminal events (like title changes)
#[derive(Clone)]
struct Listener {
    title: Arc<Mutex<Option<String>>>,
}

impl Listener {
    fn new() -> Self {
        Self {
            title: Arc::new(Mutex::new(None)),
        }
    }
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        if let Event::Title(title) = event {
            *self.title.lock().unwrap() = Some(title);
        }
    }
}

/// Size provider for the terminal
#[derive(Clone, Copy, Debug)]
struct TermSize {
    cols: u16,
    rows: u16,
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

/// Terminal pane that renders a PTY session.
///
/// Manages the PTY process, terminal emulator state, and rendering.
pub struct TerminalPane {
    /// PTY process handler for shell communication
    pty: Arc<Mutex<Option<PtyHandler>>>,
    /// Terminal emulator state (screen buffer, cursor, etc.)
    term: Arc<Mutex<Term<Listener>>>,
    /// VTE parser for processing escape sequences
    processor: Arc<Mutex<Processor>>,
    /// Event listener for terminal events (title changes, etc.)
    listener: Listener,
    /// Terminal dimensions in rows/columns
    size: Arc<Mutex<TermSize>>,
    /// Cell dimensions (width, height) - calculated from font metrics
    cell_dims: Arc<Mutex<(f32, f32)>>,
    /// Element bounds in window coordinates (for mouse position conversion)
    bounds: Arc<Mutex<Option<Bounds<Pixels>>>>,
    /// Current font size
    font_size: Arc<Mutex<f32>>,
    /// Whether we're currently dragging a selection
    dragging: bool,
    /// Focus handle for keyboard input routing
    pub focus_handle: FocusHandle,
}

impl TerminalPane {
    pub fn new(cx: &mut Context<Self>) -> Self {
        // Use reasonable defaults - will be resized when layout occurs
        let cols = 80;
        let rows = 24;

        let size = TermSize { cols, rows };

        // Create terminal with config and event listener
        let listener = Listener::new();
        let config = Config::default();
        let term = Term::new(config, &size, listener.clone());
        let term = Arc::new(Mutex::new(term));
        let processor = Arc::new(Mutex::new(Processor::new()));

        // Spawn PTY
        let pty = match PtyHandler::spawn(rows, cols) {
            Ok(pty) => Some(pty),
            Err(e) => {
                tracing::error!("Failed to spawn PTY: {}", e);
                None
            }
        };

        let focus_handle = cx.focus_handle();

        let pane = Self {
            pty: Arc::new(Mutex::new(pty)),
            term,
            processor,
            listener,
            size: Arc::new(Mutex::new(size)),
            // Default cell dims - will be calculated from font metrics on first render
            cell_dims: Arc::new(Mutex::new((8.4, 17.0))),
            bounds: Arc::new(Mutex::new(None)),
            font_size: Arc::new(Mutex::new(DEFAULT_FONT_SIZE)),
            dragging: false,
            focus_handle,
        };

        // Start polling for PTY output
        let term_clone = pane.term.clone();
        let processor_clone = pane.processor.clone();

        let pty_clone = pane.pty.clone();
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(16))
                .await;

            let should_notify = {
                let pty_guard = pty_clone.lock().unwrap();
                if let Some(ref pty) = *pty_guard {
                    let output_chunks = pty.read_output();
                    drop(pty_guard);
                    if !output_chunks.is_empty() {
                        let mut term = term_clone.lock().unwrap();
                        let mut processor = processor_clone.lock().unwrap();
                        for chunk in output_chunks {
                            processor.advance(&mut *term, &chunk);
                        }
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            };

            if should_notify {
                let _ = this.update(cx, |_, cx| cx.notify());
            }
        })
        .detach();

        pane
    }

    /// Send keyboard input to the PTY
    pub fn send_input(&mut self, input: &str) {
        let mut pty_guard = self.pty.lock().unwrap();
        if let Some(ref mut pty) = *pty_guard {
            if let Err(e) = pty.write(input.as_bytes()) {
                tracing::error!("Failed to write to PTY: {}", e);
                // Write failed - mark as exited
                *pty_guard = None;
            }
        }
    }

    /// Check if the shell has exited
    pub fn has_exited(&self) -> bool {
        let pty_guard = self.pty.lock().unwrap();
        match &*pty_guard {
            None => true,
            Some(pty) => pty.has_exited(),
        }
    }

    /// Check if the terminal has running child processes
    pub fn has_running_processes(&self) -> bool {
        let pty_guard = self.pty.lock().unwrap();
        match &*pty_guard {
            None => false,
            Some(pty) => pty.has_running_processes(),
        }
    }

    /// Get the name of any running foreground process
    pub fn get_running_process_name(&self) -> Option<String> {
        let pty_guard = self.pty.lock().unwrap();
        match &*pty_guard {
            None => None,
            Some(pty) => pty.get_running_process_name(),
        }
    }

    /// Get the terminal title (set by OSC escape sequences)
    pub fn title(&self) -> Option<SharedString> {
        self.listener
            .title
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| s.clone().into())
    }

    /// Convert pixel position (window coords) to terminal cell coordinates
    fn pixel_to_cell(&self, position: Point<Pixels>) -> Option<(usize, usize)> {
        // Get element bounds to convert from window coords to element-local coords
        let bounds = self.bounds.lock().unwrap();
        let bounds = bounds.as_ref()?;

        let origin_x: f32 = bounds.origin.x.into();
        let origin_y: f32 = bounds.origin.y.into();
        let x: f32 = position.x.into();
        let y: f32 = position.y.into();

        // Convert to element-local coordinates
        let local_x = x - origin_x;
        let local_y = y - origin_y;

        let (cell_width, cell_height) = *self.cell_dims.lock().unwrap();

        // Account for padding
        let cell_x = ((local_x - PADDING) / cell_width).floor() as i32;
        let cell_y = ((local_y - PADDING) / cell_height).floor() as i32;

        let size = self.size.lock().unwrap();
        if cell_x >= 0 && cell_y >= 0 && cell_x < size.cols as i32 && cell_y < size.rows as i32 {
            Some((cell_x as usize, cell_y as usize))
        } else {
            None
        }
    }

    /// Handle mouse down event
    fn handle_mouse_down(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let Some((col, row)) = self.pixel_to_cell(event.position) else {
            return;
        };

        let mode = {
            let term = self.term.lock().unwrap();
            *term.mode()
        };

        // Check if mouse reporting is enabled
        if mode.intersects(
            TermMode::MOUSE_REPORT_CLICK
                | TermMode::MOUSE_DRAG
                | TermMode::MOUSE_MOTION
                | TermMode::MOUSE_MODE,
        ) {
            // Send mouse event to PTY
            let button = match event.button {
                MouseButton::Left => 0,
                MouseButton::Middle => 1,
                MouseButton::Right => 2,
                _ => return,
            };

            let seq = if mode.contains(TermMode::SGR_MOUSE) {
                format!("\x1b[<{};{};{}M", button, col + 1, row + 1)
            } else {
                let cb = (32 + button) as u8;
                let cx_val = (32 + col + 1).min(255) as u8;
                let cy = (32 + row + 1).min(255) as u8;
                format!("\x1b[M{}{}{}", cb as char, cx_val as char, cy as char)
            };

            self.send_input(&seq);
        } else if event.button == MouseButton::Left {
            // Start text selection using alacritty's Selection
            let point = TermPoint::new(Line(row as i32), Column(col));
            let selection = TermSelection::new(SelectionType::Simple, point, Side::Left);
            self.term.lock().unwrap().selection = Some(selection);
            self.dragging = true;
            cx.notify();
        }
    }

    /// Handle mouse up event
    fn handle_mouse_up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        let Some((col, row)) = self.pixel_to_cell(event.position) else {
            return;
        };

        let mode = {
            let term = self.term.lock().unwrap();
            *term.mode()
        };

        if mode.intersects(
            TermMode::MOUSE_REPORT_CLICK
                | TermMode::MOUSE_DRAG
                | TermMode::MOUSE_MOTION
                | TermMode::MOUSE_MODE,
        ) {
            // Send mouse release to PTY
            if mode.contains(TermMode::SGR_MOUSE) {
                let button = match event.button {
                    MouseButton::Left => 0,
                    MouseButton::Middle => 1,
                    MouseButton::Right => 2,
                    _ => return,
                };
                let seq = format!("\x1b[<{};{};{}m", button, col + 1, row + 1);
                self.send_input(&seq);
            } else {
                let cb = (32 + 3) as u8;
                let cx_val = (32 + col + 1).min(255) as u8;
                let cy = (32 + row + 1).min(255) as u8;
                let seq = format!("\x1b[M{}{}{}", cb as char, cx_val as char, cy as char);
                self.send_input(&seq);
            }
        } else if event.button == MouseButton::Left {
            // End text selection
            if self.dragging {
                let point = TermPoint::new(Line(row as i32), Column(col));
                if let Some(ref mut selection) = self.term.lock().unwrap().selection {
                    selection.update(point, Side::Right);
                }
                self.dragging = false;
                cx.notify();
            }
        }
    }

    /// Handle mouse move/drag event
    fn handle_mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some((col, row)) = self.pixel_to_cell(event.position) else {
            return;
        };

        // Update selection if dragging
        if self.dragging {
            let point = TermPoint::new(Line(row as i32), Column(col));
            if let Some(ref mut selection) = self.term.lock().unwrap().selection {
                selection.update(point, Side::Right);
            }
            cx.notify();
        }
    }

    /// Handle scroll event
    fn handle_scroll(&mut self, event: &ScrollWheelEvent) {
        let Some((col, row)) = self.pixel_to_cell(event.position) else {
            return;
        };

        let mode = {
            let term = self.term.lock().unwrap();
            *term.mode()
        };

        let (_, cell_height) = *self.cell_dims.lock().unwrap();

        // If mouse reporting is enabled, send wheel events
        if mode.intersects(
            TermMode::MOUSE_REPORT_CLICK
                | TermMode::MOUSE_DRAG
                | TermMode::MOUSE_MOTION
                | TermMode::MOUSE_MODE,
        ) {
            let delta_y: f32 = event.delta.pixel_delta(px(cell_height)).y.into();
            let button = if delta_y < 0.0 { 64 } else { 65 }; // 64 = wheel up, 65 = wheel down

            let seq = if mode.contains(TermMode::SGR_MOUSE) {
                format!("\x1b[<{};{};{}M", button, col + 1, row + 1)
            } else {
                let cb = (32 + button) as u8;
                let cx = (32 + col + 1).min(255) as u8;
                let cy = (32 + row + 1).min(255) as u8;
                format!("\x1b[M{}{}{}", cb as char, cx as char, cy as char)
            };

            self.send_input(&seq);
        } else if mode.contains(TermMode::ALT_SCREEN) {
            // In alternate screen without mouse mode, send arrow keys for scrolling
            let delta_y: f32 = event.delta.pixel_delta(px(cell_height)).y.into();
            let lines = (delta_y.abs() / cell_height).ceil() as usize;
            let key = if delta_y < 0.0 { "\x1b[A" } else { "\x1b[B" }; // Up or Down

            for _ in 0..lines.min(5) {
                self.send_input(key);
            }
        }
    }

    /// Handle a key event
    fn handle_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let mode = {
            let term = self.term.lock().unwrap();
            *term.mode()
        };

        // Handle Cmd+key shortcuts
        if event.keystroke.modifiers.platform {
            match event.keystroke.key.as_str() {
                "c" => {
                    // Copy selection
                    self.copy_selection(cx);
                    return;
                }
                "v" => {
                    // Paste from clipboard
                    self.paste_clipboard(cx);
                    return;
                }
                "backspace" => {
                    // Delete line (cursor to beginning) - Ctrl+U
                    self.send_input("\x15");
                    return;
                }
                "=" | "+" => {
                    // Zoom in (increase font size)
                    let mut font_size = self.font_size.lock().unwrap();
                    *font_size = (*font_size + 1.0).min(MAX_FONT_SIZE);
                    cx.notify();
                    return;
                }
                "-" => {
                    // Zoom out (decrease font size)
                    let mut font_size = self.font_size.lock().unwrap();
                    *font_size = (*font_size - 1.0).max(MIN_FONT_SIZE);
                    cx.notify();
                    return;
                }
                "0" => {
                    // Reset to default font size
                    *self.font_size.lock().unwrap() = DEFAULT_FONT_SIZE;
                    cx.notify();
                    return;
                }
                _ => return, // Let other Cmd shortcuts pass through
            }
        }

        // Handle Option+Backspace for word deletion
        if event.keystroke.modifiers.alt && event.keystroke.key == "backspace" {
            // Send ESC + DEL for word deletion (works in most shells)
            self.send_input("\x1b\x7f");
            return;
        }

        // Handle Option+Left/Right for word movement
        if event.keystroke.modifiers.alt {
            match event.keystroke.key.as_str() {
                "left" => {
                    self.send_input("\x1bb"); // ESC + b = backward word
                    return;
                }
                "right" => {
                    self.send_input("\x1bf"); // ESC + f = forward word
                    return;
                }
                _ => {}
            }
        }

        let input = match &event.keystroke.key {
            key if key.len() == 1 => {
                // Clear selection on typing
                self.term.lock().unwrap().selection = None;

                let c = key.chars().next().unwrap();

                if event.keystroke.modifiers.control {
                    if c.is_ascii_alphabetic() {
                        let ctrl_char = (c.to_ascii_lowercase() as u8 - b'a' + 1) as char;
                        ctrl_char.to_string()
                    } else {
                        key.clone()
                    }
                } else if event.keystroke.modifiers.shift && c.is_ascii_alphabetic() {
                    // Handle shift for uppercase letters
                    c.to_ascii_uppercase().to_string()
                } else {
                    key.clone()
                }
            }
            key => {
                let app_cursor = mode.contains(TermMode::APP_CURSOR);
                match key.as_str() {
                    "enter" => "\r".to_string(),
                    "backspace" => "\x7f".to_string(),
                    "tab" => "\t".to_string(),
                    "escape" => {
                        // Clear selection on escape
                        self.term.lock().unwrap().selection = None;
                        cx.notify();
                        "\x1b".to_string()
                    }
                    "up" => {
                        if app_cursor {
                            "\x1bOA".to_string()
                        } else {
                            "\x1b[A".to_string()
                        }
                    }
                    "down" => {
                        if app_cursor {
                            "\x1bOB".to_string()
                        } else {
                            "\x1b[B".to_string()
                        }
                    }
                    "right" => {
                        if app_cursor {
                            "\x1bOC".to_string()
                        } else {
                            "\x1b[C".to_string()
                        }
                    }
                    "left" => {
                        if app_cursor {
                            "\x1bOD".to_string()
                        } else {
                            "\x1b[D".to_string()
                        }
                    }
                    "home" => "\x1b[H".to_string(),
                    "end" => "\x1b[F".to_string(),
                    "pageup" => "\x1b[5~".to_string(),
                    "pagedown" => "\x1b[6~".to_string(),
                    "delete" => "\x1b[3~".to_string(),
                    "space" => " ".to_string(),
                    _ => return,
                }
            }
        };

        self.send_input(&input);
    }

    /// Get selected text from terminal using alacritty's selection
    fn get_selected_text(&self) -> Option<String> {
        let term = self.term.lock().unwrap();
        let content = term.renderable_content();

        // Get selection range from renderable content
        let selection_range = content.selection?;
        let start = selection_range.start;
        let end = selection_range.end;

        let size = self.size.lock().unwrap();
        let cols = size.cols as usize;
        drop(size);

        // Build a grid of characters
        let rows = term.screen_lines();
        let mut grid: Vec<Vec<char>> = vec![vec![' '; cols]; rows];

        for cell in content.display_iter {
            let row = cell.point.line.0 as usize;
            let col = cell.point.column.0;
            if row < rows && col < cols && !cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                grid[row][col] = cell.c;
            }
        }
        drop(term);

        // Extract selected text
        let start_row = start.line.0 as usize;
        let start_col = start.column.0;
        let end_row = end.line.0 as usize;
        let end_col = end.column.0;

        let mut result = String::new();
        for row in start_row..=end_row {
            if row >= grid.len() {
                break;
            }
            let col_start = if row == start_row { start_col } else { 0 };
            let col_end = if row == end_row { end_col + 1 } else { cols };
            let col_end = col_end.min(grid[row].len());

            for col in col_start..col_end {
                result.push(grid[row][col]);
            }

            // Add newline between rows (but not after the last row)
            if row < end_row {
                // Trim trailing spaces before newline
                while result.ends_with(' ') {
                    result.pop();
                }
                result.push('\n');
            }
        }

        // Trim trailing spaces
        while result.ends_with(' ') {
            result.pop();
        }

        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }

    /// Copy selection to clipboard
    fn copy_selection(&self, cx: &mut Context<Self>) {
        if let Some(text) = self.get_selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    /// Paste from clipboard
    fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        if let Some(item) = cx.read_from_clipboard() {
            if let Some(text) = item.text() {
                // Clear selection
                self.term.lock().unwrap().selection = None;
                // Paste text to terminal
                self.send_input(&text);
                cx.notify();
            }
        }
    }
}

/// Convert RGB to Hsla
fn rgb_to_hsla(rgb: Rgb) -> Hsla {
    Hsla::from(Rgba {
        r: rgb.r as f32 / 255.0,
        g: rgb.g as f32 / 255.0,
        b: rgb.b as f32 / 255.0,
        a: 1.0,
    })
}

/// Convert alacritty color to GPUI Hsla using terminal colors with theme fallbacks
fn color_to_hsla(color: Color, term_colors: &TermColors, theme: &TerminalColors) -> Hsla {
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

/// Apply DIM flag - reduce brightness by 33%
fn apply_dim(color: Hsla) -> Hsla {
    hsla(color.h, color.s, color.l * 0.66, color.a)
}

/// Get bright variant of a named color
fn get_bright_color(color: Color, term_colors: &TermColors, theme: &TerminalColors) -> Hsla {
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

/// Create a TextRun with proper styling based on cell flags
fn create_text_run(len: usize, font_family: &SharedString, fg: Hsla, flags: CellFlags) -> TextRun {
    // Determine font weight
    let weight = if flags.contains(CellFlags::BOLD) {
        FontWeight::BOLD
    } else {
        FontWeight::NORMAL
    };

    // Determine font style
    let style = if flags.contains(CellFlags::ITALIC) {
        FontStyle::Italic
    } else {
        FontStyle::Normal
    };

    // Determine underline style
    let underline = if flags.intersects(
        CellFlags::UNDERLINE
            | CellFlags::DOUBLE_UNDERLINE
            | CellFlags::UNDERCURL
            | CellFlags::DOTTED_UNDERLINE
            | CellFlags::DASHED_UNDERLINE,
    ) {
        Some(UnderlineStyle {
            thickness: px(1.0),
            color: Some(fg),
            wavy: flags.contains(CellFlags::UNDERCURL),
        })
    } else {
        None
    };

    // Determine strikethrough style
    let strikethrough = if flags.contains(CellFlags::STRIKEOUT) {
        Some(StrikethroughStyle {
            thickness: px(1.0),
            color: Some(fg),
        })
    } else {
        None
    };

    TextRun {
        len,
        font: Font {
            family: font_family.clone(),
            features: FontFeatures::default(),
            fallbacks: None,
            weight,
            style,
        },
        color: fg,
        background_color: None,
        underline,
        strikethrough,
    }
}

/// Build render data from terminal state - collects individual cells for precise positioning
fn build_render_data(
    term: &Term<Listener>,
    theme: &TerminalColors,
    cols: usize,
    rows: usize,
    _font_family: SharedString,
) -> RenderData {
    let content = term.renderable_content();
    let term_colors = content.colors;
    let default_bg = theme.background;

    // Collect cells and background regions
    let mut cells: Vec<RenderCell> = Vec::new();
    let mut bg_regions: Vec<BgRegion> = Vec::new();

    // Grid for tracking backgrounds (we need to process in order for region merging)
    let mut grid_bg: Vec<Vec<Option<Hsla>>> = vec![vec![None; cols]; rows];

    // Get cursor info with shape
    let cursor_line = content.cursor.point.line.0;
    let cursor_col = content.cursor.point.column.0;
    let cursor_shape = content.cursor.shape;

    let cursor_info = if cursor_line >= 0 && (cursor_line as usize) < rows && cursor_col < cols {
        Some(CursorInfo {
            row: cursor_line as usize,
            col: cursor_col,
            shape: cursor_shape,
            color: theme.cursor,
        })
    } else {
        None
    };

    // Process cells from terminal content
    for cell in content.display_iter {
        let point = cell.point;
        let row = point.line.0 as usize;
        let col = point.column.0;

        if row >= rows || col >= cols {
            continue;
        }

        let flags = cell.flags;

        // Skip wide char spacer cells - they're placeholders
        if flags.contains(CellFlags::WIDE_CHAR_SPACER) {
            continue;
        }

        // Start with base colors using terminal colors with theme fallback
        let mut fg = color_to_hsla(cell.fg, term_colors, theme);
        let mut bg = color_to_hsla(cell.bg, term_colors, theme);

        // Handle BOLD flag - use bright color variant for named colors
        if flags.contains(CellFlags::BOLD) {
            fg = get_bright_color(cell.fg, term_colors, theme);
        }

        // Handle DIM flag - reduce brightness
        if flags.contains(CellFlags::DIM) {
            fg = apply_dim(fg);
        }

        // Handle INVERSE flag - swap fg and bg
        if flags.contains(CellFlags::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }

        // Handle HIDDEN flag - make text invisible
        if flags.contains(CellFlags::HIDDEN) {
            fg = bg;
        }

        // Apply cursor styling for block cursor
        let is_cursor = cursor_info.is_some_and(|c| c.row == row && c.col == col);
        let is_block_cursor = is_cursor
            && cursor_info
                .is_some_and(|c| matches!(c.shape, CursorShape::Block | CursorShape::HollowBlock));
        if is_block_cursor {
            fg = theme.background;
        }

        // Track non-default backgrounds
        if bg != default_bg {
            grid_bg[row][col] = Some(bg);
        }

        // Only add non-space characters to render list
        if cell.c != ' ' && cell.c != '\0' {
            cells.push(RenderCell {
                row,
                col,
                c: cell.c,
                fg,
                flags,
            });
        }
    }

    // Build merged background regions from grid
    for (row_idx, row) in grid_bg.into_iter().enumerate() {
        let mut bg_start: Option<(usize, Hsla)> = None;

        for (col_idx, bg_opt) in row.into_iter().enumerate() {
            match (&mut bg_start, bg_opt) {
                (None, Some(bg)) => {
                    bg_start = Some((col_idx, bg));
                }
                (Some((_start, color)), Some(bg)) if *color == bg => {
                    // Continue current region
                }
                (Some((start, color)), _) => {
                    // Flush region
                    bg_regions.push(BgRegion {
                        row: row_idx,
                        col_start: *start,
                        col_end: col_idx,
                        color: *color,
                    });
                    bg_start = bg_opt.map(|bg| (col_idx, bg));
                }
                (None, None) => {}
            }
        }

        // Flush remaining bg region
        if let Some((start, color)) = bg_start {
            bg_regions.push(BgRegion {
                row: row_idx,
                col_start: start,
                col_end: cols,
                color,
            });
        }
    }

    RenderData {
        cells,
        bg_regions,
        cursor: cursor_info,
    }
}

impl Render for TerminalPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus_handle = self.focus_handle.clone();

        // Get theme colors
        let colors = terminal_colors(cx);
        let bg_color = colors.background;
        let font_family: SharedString = FONT_FAMILY.into();

        // Get current font size
        let current_font_size = *self.font_size.lock().unwrap();

        // Calculate cell dimensions from actual font metrics
        let (cell_width, cell_height) = get_cell_dimensions(window, current_font_size);
        *self.cell_dims.lock().unwrap() = (cell_width, cell_height);

        // Clone data needed for canvas callbacks (resize happens in prepaint with actual bounds)
        let term = self.term.clone();
        let pty = self.pty.clone();
        let size = self.size.clone();
        let cell_dims = self.cell_dims.clone();
        let bounds_arc = self.bounds.clone();
        let font_size_arc = self.font_size.clone();
        let colors_clone = colors;
        let font_family_clone = font_family.clone();

        // Main container with canvas for efficient rendering
        div()
            .id("terminal-pane")
            .key_context("terminal")
            .track_focus(&focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.modifiers.platform && event.keystroke.key.as_str() == "," {
                    crate::app::open_settings_dialog(window, cx);
                    return;
                }
                this.handle_key(event, cx);
            }))
            .on_click(cx.listener(|_this, _event: &ClickEvent, window, cx| {
                window.focus(&cx.focus_handle());
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                    this.handle_mouse_down(event, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                    this.handle_mouse_down(event, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                    this.handle_mouse_down(event, cx);
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, _window, cx| {
                    this.handle_mouse_up(event, cx);
                }),
            )
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(|this, event: &MouseUpEvent, _window, cx| {
                    this.handle_mouse_up(event, cx);
                }),
            )
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(|this, event: &MouseUpEvent, _window, cx| {
                    this.handle_mouse_up(event, cx);
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                this.handle_mouse_move(event, cx);
            }))
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _window, _cx| {
                this.handle_scroll(event);
            }))
            .size_full()
            .bg(bg_color)
            .child(
                // Canvas for GPU-accelerated terminal rendering
                canvas(
                    // Prepaint: compute render data and shape individual cells
                    move |bounds, window, _cx| {
                        // Store bounds for mouse coordinate conversion
                        *bounds_arc.lock().unwrap() = Some(bounds);

                        // Get cell dimensions from font metrics
                        let (cell_width, cell_height) = *cell_dims.lock().unwrap();

                        // Calculate terminal size from actual element bounds
                        let bounds_width: f32 = bounds.size.width.into();
                        let bounds_height: f32 = bounds.size.height.into();

                        let new_cols =
                            ((bounds_width - PADDING * 2.0).max(0.0) / cell_width).floor() as u16;
                        let new_rows =
                            ((bounds_height - PADDING * 2.0).max(0.0) / cell_height).floor() as u16;
                        let new_cols = new_cols.max(10);
                        let new_rows = new_rows.max(3);

                        // Check if resize is needed
                        let mut size_guard = size.lock().unwrap();
                        if new_cols != size_guard.cols || new_rows != size_guard.rows {
                            size_guard.cols = new_cols;
                            size_guard.rows = new_rows;
                            let new_size = *size_guard;
                            drop(size_guard);

                            // Resize PTY
                            if let Ok(pty_guard) = pty.lock() {
                                if let Some(ref pty_inner) = *pty_guard {
                                    if let Err(e) = pty_inner.resize(new_rows, new_cols) {
                                        tracing::error!(
                                            "Failed to resize PTY to {}x{}: {}",
                                            new_cols,
                                            new_rows,
                                            e
                                        );
                                    }
                                }
                            }

                            // Resize terminal emulator
                            if let Ok(mut term_guard) = term.lock() {
                                term_guard.resize(new_size);
                            }
                        } else {
                            drop(size_guard);
                        }

                        // Get current size for rendering
                        let size_guard = size.lock().unwrap();
                        let cols = size_guard.cols as usize;
                        let rows = size_guard.rows as usize;
                        drop(size_guard);

                        // Build render data from terminal state and get selection
                        let term_guard = term.lock().unwrap();
                        let render_data = build_render_data(
                            &term_guard,
                            &colors_clone,
                            cols,
                            rows,
                            font_family_clone.clone(),
                        );
                        // Get selection from renderable content (already normalized)
                        let selection_range = term_guard.renderable_content().selection;
                        drop(term_guard);

                        // Shape each cell individually for precise positioning
                        let current_font_size = *font_size_arc.lock().unwrap();
                        let font_size = px(current_font_size);
                        let shaped_cells: Vec<_> = render_data
                            .cells
                            .iter()
                            .map(|cell| {
                                let text_run = create_text_run(
                                    cell.c.len_utf8(),
                                    &font_family_clone,
                                    cell.fg,
                                    cell.flags,
                                );
                                let shaped = window.text_system().shape_line(
                                    cell.c.to_string().into(),
                                    font_size,
                                    &[text_run],
                                    None,
                                );
                                (cell.row, cell.col, shaped)
                            })
                            .collect();

                        (
                            render_data,
                            shaped_cells,
                            bounds,
                            cell_width,
                            cell_height,
                            selection_range,
                            cols,
                        )
                    },
                    // Paint: draw backgrounds and individual cells at exact grid positions
                    move |_bounds, data, window, cx| {
                        let (
                            render_data,
                            shaped_cells,
                            bounds,
                            cell_width,
                            cell_height,
                            selection_range,
                            cols,
                        ) = data;

                        let origin = bounds.origin;
                        let line_height = px(cell_height);

                        // 1. Paint background regions (non-default colors only)
                        for region in &render_data.bg_regions {
                            let x = origin.x + px(PADDING + region.col_start as f32 * cell_width);
                            let y = origin.y + px(PADDING + region.row as f32 * cell_height);
                            let width = px((region.col_end - region.col_start) as f32 * cell_width);
                            let height = px(cell_height);

                            window.paint_quad(fill(
                                Bounds::new(Point::new(x, y), Size { width, height }),
                                region.color,
                            ));
                        }

                        // 1.5. Paint selection highlight using alacritty's SelectionRange
                        // Only render if selection spans more than one cell (skip single-click selections)
                        if let Some(sel) = selection_range {
                            let start_same_as_end = sel.start.line == sel.end.line
                                && sel.start.column == sel.end.column;

                            if !start_same_as_end {
                                let selection_color = hsla(210.0 / 360.0, 0.6, 0.5, 0.3);

                                // SelectionRange uses viewport coordinates (line.0 >= 0 for visible lines)
                                let start_line = sel.start.line.0;
                                let end_line = sel.end.line.0;

                                // Only render if within visible viewport
                                if start_line >= 0 && end_line >= 0 {
                                    let start_row = start_line as usize;
                                    let start_col = sel.start.column.0;
                                    let end_row = end_line as usize;
                                    let end_col = sel.end.column.0;

                                    for row in start_row..=end_row {
                                        let col_start =
                                            if row == start_row { start_col } else { 0 };
                                        let col_end =
                                            if row == end_row { end_col + 1 } else { cols };

                                        let x =
                                            origin.x + px(PADDING + col_start as f32 * cell_width);
                                        let y = origin.y + px(PADDING + row as f32 * cell_height);
                                        let width = px((col_end - col_start) as f32 * cell_width);
                                        let height = px(cell_height);

                                        window.paint_quad(fill(
                                            Bounds::new(Point::new(x, y), Size { width, height }),
                                            selection_color,
                                        ));
                                    }
                                }
                            }
                        }

                        // 2. Paint each cell at its exact grid position
                        for (row, col, shaped) in &shaped_cells {
                            let x = origin.x + px(PADDING + *col as f32 * cell_width);
                            let y = origin.y + px(PADDING + *row as f32 * cell_height);
                            let _ = shaped.paint(Point::new(x, y), line_height, window, cx);
                        }

                        // 3. Paint cursor based on shape
                        if let Some(cursor) = render_data.cursor {
                            let cursor_x = origin.x + px(PADDING + cursor.col as f32 * cell_width);
                            let cursor_y = origin.y + px(PADDING + cursor.row as f32 * cell_height);

                            match cursor.shape {
                                CursorShape::Block => {
                                    // Hollow block style for visibility (text is readable inside)
                                    let thickness = px(2.0);
                                    // Top
                                    window.paint_quad(fill(
                                        Bounds::new(
                                            Point::new(cursor_x, cursor_y),
                                            Size {
                                                width: px(cell_width),
                                                height: thickness,
                                            },
                                        ),
                                        cursor.color,
                                    ));
                                    // Bottom
                                    window.paint_quad(fill(
                                        Bounds::new(
                                            Point::new(
                                                cursor_x,
                                                cursor_y + px(cell_height) - thickness,
                                            ),
                                            Size {
                                                width: px(cell_width),
                                                height: thickness,
                                            },
                                        ),
                                        cursor.color,
                                    ));
                                    // Left
                                    window.paint_quad(fill(
                                        Bounds::new(
                                            Point::new(cursor_x, cursor_y),
                                            Size {
                                                width: thickness,
                                                height: px(cell_height),
                                            },
                                        ),
                                        cursor.color,
                                    ));
                                    // Right
                                    window.paint_quad(fill(
                                        Bounds::new(
                                            Point::new(
                                                cursor_x + px(cell_width) - thickness,
                                                cursor_y,
                                            ),
                                            Size {
                                                width: thickness,
                                                height: px(cell_height),
                                            },
                                        ),
                                        cursor.color,
                                    ));
                                }
                                CursorShape::HollowBlock => {
                                    // Outline only (draw 4 thin lines)
                                    let thickness = px(1.0);
                                    // Top
                                    window.paint_quad(fill(
                                        Bounds::new(
                                            Point::new(cursor_x, cursor_y),
                                            Size {
                                                width: px(cell_width),
                                                height: thickness,
                                            },
                                        ),
                                        cursor.color,
                                    ));
                                    // Bottom
                                    window.paint_quad(fill(
                                        Bounds::new(
                                            Point::new(
                                                cursor_x,
                                                cursor_y + px(cell_height) - thickness,
                                            ),
                                            Size {
                                                width: px(cell_width),
                                                height: thickness,
                                            },
                                        ),
                                        cursor.color,
                                    ));
                                    // Left
                                    window.paint_quad(fill(
                                        Bounds::new(
                                            Point::new(cursor_x, cursor_y),
                                            Size {
                                                width: thickness,
                                                height: px(cell_height),
                                            },
                                        ),
                                        cursor.color,
                                    ));
                                    // Right
                                    window.paint_quad(fill(
                                        Bounds::new(
                                            Point::new(
                                                cursor_x + px(cell_width) - thickness,
                                                cursor_y,
                                            ),
                                            Size {
                                                width: thickness,
                                                height: px(cell_height),
                                            },
                                        ),
                                        cursor.color,
                                    ));
                                }
                                CursorShape::Beam => {
                                    // Thin vertical bar at left edge
                                    window.paint_quad(fill(
                                        Bounds::new(
                                            Point::new(cursor_x, cursor_y),
                                            Size {
                                                width: px(2.0),
                                                height: px(cell_height),
                                            },
                                        ),
                                        cursor.color,
                                    ));
                                }
                                CursorShape::Underline => {
                                    // Thin horizontal bar at bottom
                                    window.paint_quad(fill(
                                        Bounds::new(
                                            Point::new(
                                                cursor_x,
                                                cursor_y + px(cell_height) - px(2.0),
                                            ),
                                            Size {
                                                width: px(cell_width),
                                                height: px(2.0),
                                            },
                                        ),
                                        cursor.color,
                                    ));
                                }
                                CursorShape::Hidden => {
                                    // Don't draw anything
                                }
                            }
                        }
                    },
                )
                .size_full(),
            )
    }
}

impl Focusable for TerminalPane {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
