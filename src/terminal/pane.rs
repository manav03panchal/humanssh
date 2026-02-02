//! Terminal pane component using alacritty_terminal.
//!
//! Uses GPUI's canvas for efficient GPU-accelerated rendering with:
//! - Batched text runs via StyledText
//! - Merged background regions via paint_quad
//! - Proper handling of TUI applications

use super::colors::{apply_dim, color_to_hsla, get_bright_color};
use super::types::{
    BgRegion, CursorInfo, DisplayState, MouseEscBuf, RenderCell, RenderData, TermSize,
};
use super::PtyHandler;
use crate::theme::{terminal_colors, TerminalColors};
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::index::{Column, Line, Point as TermPoint, Side};
use alacritty_terminal::selection::{Selection as TermSelection, SelectionType};
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{CursorShape, Processor};
use base64::Engine;
use gpui::*;
use parking_lot::{Mutex, RwLock};
use std::fmt::Write as FmtWrite;
use std::sync::Arc;

// Import centralized configuration
use crate::config::terminal::{
    DEFAULT_FONT_SIZE, FONT_FAMILY, MAX_FONT_SIZE, MIN_FONT_SIZE, PADDING,
};

/// Cache for cell dimensions to avoid recalculating every frame.
/// Key is font_size (as bits), value is (width, height).
static CELL_DIMS_CACHE: Mutex<Option<(u32, f32, f32)>> = Mutex::new(None);

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
            *self.title.lock() = Some(title);
        }
    }
}

/// Terminal pane that renders a PTY session.
///
/// Manages the PTY process, terminal emulator state, and rendering.
/// State is organized into groups to minimize lock contention:
/// - `pty`: Separate mutex for background I/O thread
/// - `term`/`processor`: Terminal emulation state
/// - `display`: Read-heavy display state (size, dims, bounds, font) uses RwLock
pub struct TerminalPane {
    /// PTY process handler for shell communication
    pty: Arc<Mutex<Option<PtyHandler>>>,
    /// Terminal emulator state (screen buffer, cursor, etc.)
    term: Arc<Mutex<Term<Listener>>>,
    /// VTE parser for processing escape sequences
    processor: Arc<Mutex<Processor>>,
    /// Event listener for terminal events (title changes, etc.)
    listener: Listener,
    /// Consolidated display state (size, cell_dims, bounds, font_size)
    /// Uses RwLock for better read concurrency during rendering
    display: Arc<RwLock<DisplayState>>,
    /// Whether we're currently dragging a selection
    dragging: bool,
    /// Focus handle for keyboard input routing
    pub focus_handle: FocusHandle,
}

impl TerminalPane {
    /// Create a new terminal pane with the user's default shell.
    ///
    /// Spawns a PTY process and starts polling for output. The terminal
    /// starts with default dimensions (80x24) and resizes when rendered.
    pub fn new(cx: &mut Context<Self>) -> Self {
        // Use reasonable defaults - will be resized when layout occurs
        let display_state = DisplayState::default();
        let size = display_state.size;

        // Create terminal with config and event listener
        let listener = Listener::new();
        let config = Config::default();
        let term = Term::new(config, &size, listener.clone());
        let term = Arc::new(Mutex::new(term));
        let processor = Arc::new(Mutex::new(Processor::new()));

        // Spawn PTY
        let (pty, spawn_error) = match PtyHandler::spawn(size.rows, size.cols) {
            Ok(pty) => (Some(pty), None),
            Err(e) => {
                tracing::error!("Failed to spawn PTY: {}", e);
                (None, Some(e.to_string()))
            }
        };

        let focus_handle = cx.focus_handle();

        let pane = Self {
            pty: Arc::new(Mutex::new(pty)),
            term,
            processor,
            listener,
            display: Arc::new(RwLock::new(display_state)),
            dragging: false,
            focus_handle,
        };

        // Display error message in terminal if PTY spawn failed
        if let Some(error) = spawn_error {
            let error_msg = format!(
                "\x1b[31m\x1b[1mError: Failed to spawn shell\x1b[0m\r\n\r\n{}\r\n\r\n\
                 \x1b[33mTroubleshooting:\x1b[0m\r\n\
                 - Check that your shell exists: echo $SHELL\r\n\
                 - Try setting SHELL=/bin/zsh or SHELL=/bin/bash\r\n",
                error
            );
            let mut term = pane.term.lock();
            let mut processor = pane.processor.lock();
            processor.advance(&mut *term, error_msg.as_bytes());
        }

        // Start event-driven PTY output processing with adaptive polling.
        // Uses short intervals when data is flowing, longer intervals when idle.
        // This reduces CPU usage from constant 16ms polling to near-zero when idle.
        let term_clone = pane.term.clone();
        let processor_clone = pane.processor.clone();
        let pty_clone = pane.pty.clone();

        cx.spawn(async move |this, cx| {
            // Adaptive polling intervals (in ms)
            const ACTIVE_INTERVAL: u64 = 8; // Fast when data flowing
            const IDLE_INTERVAL: u64 = 100; // Slow when idle
            const IDLE_THRESHOLD: u32 = 3; // Cycles without data before going idle

            let mut idle_count = 0u32;

            loop {
                // Choose interval based on recent activity
                let interval = if idle_count >= IDLE_THRESHOLD {
                    IDLE_INTERVAL
                } else {
                    ACTIVE_INTERVAL
                };

                cx.background_executor()
                    .timer(std::time::Duration::from_millis(interval))
                    .await;

                let (should_notify, has_data) = {
                    let pty_guard = pty_clone.lock();
                    if let Some(ref pty) = *pty_guard {
                        let output_chunks = pty.read_output();
                        drop(pty_guard);
                        if !output_chunks.is_empty() {
                            let mut term = term_clone.lock();
                            let mut processor = processor_clone.lock();
                            for chunk in output_chunks {
                                processor.advance(&mut *term, &chunk);
                            }
                            (true, true)
                        } else {
                            (false, false)
                        }
                    } else {
                        (false, false)
                    }
                };

                // Update idle tracking
                if has_data {
                    idle_count = 0; // Reset on data
                } else {
                    idle_count = idle_count.saturating_add(1);
                }

                if should_notify {
                    let _ = this.update(cx, |_, cx| cx.notify());
                }
            }
        })
        .detach();

        pane
    }

    /// Send keyboard input to the PTY
    pub fn send_input(&mut self, input: &str) {
        let mut pty_guard = self.pty.lock();
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
        let pty_guard = self.pty.lock();
        match &*pty_guard {
            None => true,
            Some(pty) => pty.has_exited(),
        }
    }

    /// Check if the terminal has running child processes
    pub fn has_running_processes(&self) -> bool {
        let pty_guard = self.pty.lock();
        match &*pty_guard {
            None => false,
            Some(pty) => pty.has_running_processes(),
        }
    }

    /// Get the name of any running foreground process
    pub fn get_running_process_name(&self) -> Option<String> {
        let pty_guard = self.pty.lock();
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
            .as_ref()
            .map(|s: &String| s.clone().into())
    }

    /// Convert pixel position (window coords) to terminal cell coordinates
    fn pixel_to_cell(&self, position: Point<Pixels>) -> Option<(usize, usize)> {
        // Get display state (single lock for all display-related fields)
        let display = self.display.read();
        let bounds = display.bounds.as_ref()?;

        let origin_x: f32 = bounds.origin.x.into();
        let origin_y: f32 = bounds.origin.y.into();
        let x: f32 = position.x.into();
        let y: f32 = position.y.into();

        // Convert to element-local coordinates
        let local_x = x - origin_x;
        let local_y = y - origin_y;

        let (cell_width, cell_height) = display.cell_dims;

        // Account for padding
        let cell_x = ((local_x - PADDING) / cell_width).floor() as i32;
        let cell_y = ((local_y - PADDING) / cell_height).floor() as i32;

        if cell_x >= 0
            && cell_y >= 0
            && cell_x < display.size.cols as i32
            && cell_y < display.size.rows as i32
        {
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
            let term = self.term.lock();
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

            let mut buf = MouseEscBuf::new();
            if mode.contains(TermMode::SGR_MOUSE) {
                let _ = write!(buf, "\x1b[<{};{};{}M", button, col + 1, row + 1);
            } else {
                let cb = (32 + button) as u8;
                let cx_val = (32 + col + 1).min(255) as u8;
                let cy = (32 + row + 1).min(255) as u8;
                let _ = write!(buf, "\x1b[M{}{}{}", cb as char, cx_val as char, cy as char);
            }
            self.send_input(buf.as_str());
        } else if event.button == MouseButton::Left {
            // Start text selection using alacritty's Selection
            let point = TermPoint::new(Line(row as i32), Column(col));
            let selection = TermSelection::new(SelectionType::Simple, point, Side::Left);
            self.term.lock().selection = Some(selection);
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
            let term = self.term.lock();
            *term.mode()
        };

        if mode.intersects(
            TermMode::MOUSE_REPORT_CLICK
                | TermMode::MOUSE_DRAG
                | TermMode::MOUSE_MOTION
                | TermMode::MOUSE_MODE,
        ) {
            // Send mouse release to PTY
            let mut buf = MouseEscBuf::new();
            if mode.contains(TermMode::SGR_MOUSE) {
                let button = match event.button {
                    MouseButton::Left => 0,
                    MouseButton::Middle => 1,
                    MouseButton::Right => 2,
                    _ => return,
                };
                let _ = write!(buf, "\x1b[<{};{};{}m", button, col + 1, row + 1);
            } else {
                let cb = (32 + 3) as u8;
                let cx_val = (32 + col + 1).min(255) as u8;
                let cy = (32 + row + 1).min(255) as u8;
                let _ = write!(buf, "\x1b[M{}{}{}", cb as char, cx_val as char, cy as char);
            }
            self.send_input(buf.as_str());
        } else if event.button == MouseButton::Left {
            // End text selection
            if self.dragging {
                let point = TermPoint::new(Line(row as i32), Column(col));
                if let Some(ref mut selection) = self.term.lock().selection {
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
            if let Some(ref mut selection) = self.term.lock().selection {
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
            let term = self.term.lock();
            *term.mode()
        };

        let (_, cell_height) = self.display.read().cell_dims;

        // If mouse reporting is enabled, send wheel events
        if mode.intersects(
            TermMode::MOUSE_REPORT_CLICK
                | TermMode::MOUSE_DRAG
                | TermMode::MOUSE_MOTION
                | TermMode::MOUSE_MODE,
        ) {
            let delta_y: f32 = event.delta.pixel_delta(px(cell_height)).y.into();
            let button = if delta_y < 0.0 { 64 } else { 65 }; // 64 = wheel up, 65 = wheel down

            let mut buf = MouseEscBuf::new();
            if mode.contains(TermMode::SGR_MOUSE) {
                let _ = write!(buf, "\x1b[<{};{};{}M", button, col + 1, row + 1);
            } else {
                let cb = (32 + button) as u8;
                let cx = (32 + col + 1).min(255) as u8;
                let cy = (32 + row + 1).min(255) as u8;
                let _ = write!(buf, "\x1b[M{}{}{}", cb as char, cx as char, cy as char);
            }
            self.send_input(buf.as_str());
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
            let term = self.term.lock();
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
                    let mut display = self.display.write();
                    display.font_size = (display.font_size + 1.0).min(MAX_FONT_SIZE);
                    cx.notify();
                    return;
                }
                "-" => {
                    // Zoom out (decrease font size)
                    let mut display = self.display.write();
                    display.font_size = (display.font_size - 1.0).max(MIN_FONT_SIZE);
                    cx.notify();
                    return;
                }
                "0" => {
                    // Reset to default font size
                    self.display.write().font_size = DEFAULT_FONT_SIZE;
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
                self.term.lock().selection = None;

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
                        self.term.lock().selection = None;
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
        let term = self.term.lock();
        let content = term.renderable_content();

        // Get selection range from renderable content
        let selection_range = content.selection?;
        let start = selection_range.start;
        let end = selection_range.end;

        let start_row = start.line.0 as usize;
        let start_col = start.column.0;
        let end_row = end.line.0 as usize;
        let end_col = end.column.0;

        // Stream directly from display_iter - no intermediate grid allocation
        let mut result = String::new();
        let mut current_row = start_row;
        let mut row_content = String::new();

        for cell in content.display_iter {
            let row = cell.point.line.0 as usize;
            let col = cell.point.column.0;

            // Skip cells outside selection
            if row < start_row || row > end_row {
                continue;
            }
            if row == start_row && col < start_col {
                continue;
            }
            if row == end_row && col > end_col {
                continue;
            }
            if cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                continue;
            }

            // Handle row transitions
            if row != current_row {
                // Flush previous row (trim trailing spaces, add newline)
                let trimmed = row_content.trim_end();
                result.push_str(trimmed);
                result.push('\n');
                row_content.clear();

                // Fill gaps if we skipped rows
                for _ in (current_row + 1)..row {
                    result.push('\n');
                }
                current_row = row;
            }

            // Pad with spaces if we skipped columns
            let target_col = if row == start_row {
                col - start_col
            } else {
                col
            };
            while row_content.chars().count() < target_col {
                row_content.push(' ');
            }

            row_content.push(cell.c);
        }

        // Flush last row
        let trimmed = row_content.trim_end();
        result.push_str(trimmed);

        // Trim trailing whitespace from entire result
        let result = result.trim_end().to_string();

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

    /// Handle dropped files - converts images to base64 data URLs for AI assistants
    fn handle_file_drop(&mut self, paths: &ExternalPaths, cx: &mut Context<Self>) {
        let paths = paths.paths();
        if paths.is_empty() {
            return;
        }

        let mut output = String::new();

        for path in paths {
            // Check if it's an image file by extension
            let is_image = path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| {
                    matches!(
                        ext.to_lowercase().as_str(),
                        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"
                    )
                })
                .unwrap_or(false);

            if is_image {
                // Read and base64 encode image for AI assistants
                match std::fs::read(path) {
                    Ok(data) => {
                        let mime = match path.extension().and_then(|e| e.to_str()) {
                            Some("png") => "image/png",
                            Some("jpg") | Some("jpeg") => "image/jpeg",
                            Some("gif") => "image/gif",
                            Some("webp") => "image/webp",
                            Some("bmp") => "image/bmp",
                            _ => "application/octet-stream",
                        };
                        let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
                        // Output as data URL (supported by Claude and other AI assistants)
                        if !output.is_empty() {
                            output.push(' ');
                        }
                        output.push_str(&format!("data:{};base64,{}", mime, encoded));
                        tracing::info!("Encoded dropped image: {:?} ({} bytes)", path, data.len());
                    }
                    Err(e) => {
                        tracing::warn!("Failed to read dropped file {:?}: {}", path, e);
                        // Fall back to path
                        if !output.is_empty() {
                            output.push(' ');
                        }
                        output.push_str(&path.to_string_lossy());
                    }
                }
            } else {
                // Non-image file: just paste the path
                if !output.is_empty() {
                    output.push(' ');
                }
                // Quote paths with spaces
                let path_str = path.to_string_lossy();
                if path_str.contains(' ') {
                    output.push('"');
                    output.push_str(&path_str);
                    output.push('"');
                } else {
                    output.push_str(&path_str);
                }
            }
        }

        if !output.is_empty() {
            // Use bracketed paste mode if enabled
            let bracketed_paste = self.term.lock().mode().contains(TermMode::BRACKETED_PASTE);
            self.term.lock().selection = None;

            if bracketed_paste {
                self.send_input("\x1b[200~");
                self.send_input(&output);
                self.send_input("\x1b[201~");
            } else {
                self.send_input(&output);
            }
            cx.notify();
        }
    }

    /// Paste from clipboard with bracketed paste mode support.
    /// Wraps pasted content with escape sequences if the terminal has
    /// bracketed paste mode enabled, preventing paste injection attacks.
    fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        if let Some(item) = cx.read_from_clipboard() {
            if let Some(text) = item.text() {
                // Clear selection
                let term_guard = self.term.lock();
                let bracketed_paste = term_guard.mode().contains(TermMode::BRACKETED_PASTE);
                drop(term_guard);

                self.term.lock().selection = None;

                // Wrap with bracketed paste escape sequences if mode is enabled
                if bracketed_paste {
                    // Start bracketed paste: ESC[200~
                    self.send_input("\x1b[200~");
                    self.send_input(&text);
                    // End bracketed paste: ESC[201~
                    self.send_input("\x1b[201~");
                } else {
                    self.send_input(&text);
                }
                cx.notify();
            }
        }
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

/// Shape text by rows (batched) to reduce allocations.
/// Instead of shaping each character individually (O(cells) allocations),
/// shapes entire rows (O(rows) allocations).
fn shape_rows_batched(
    cells: &[RenderCell],
    rows: usize,
    cols: usize,
    font_family: &SharedString,
    font_size: Pixels,
    window: &mut Window,
) -> Vec<ShapedLine> {
    // Group cells by row
    let mut row_cells: Vec<Vec<&RenderCell>> = vec![Vec::new(); rows];
    for cell in cells {
        if cell.row < rows {
            row_cells[cell.row].push(cell);
        }
    }

    // Shape each row
    row_cells
        .into_iter()
        .map(|mut row| {
            if row.is_empty() {
                // Empty row - shape a single space to maintain line height
                let run = create_text_run(1, font_family, Hsla::default(), CellFlags::empty());
                window
                    .text_system()
                    .shape_line(" ".into(), font_size, &[run], None)
            } else {
                // Sort by column to ensure correct order
                row.sort_by_key(|c| c.col);

                // Build the row string and runs
                // CRITICAL: TextRun.len is in BYTES, and runs MUST cover all bytes in text
                // Otherwise GPUI may skip rendering parts of the text
                let mut text = String::with_capacity(cols);
                let mut runs: Vec<TextRun> = Vec::new();

                let mut current_col = 0;
                for cell in row {
                    // Fill gaps with spaces - each space is 1 byte
                    let gap = cell.col.saturating_sub(current_col);
                    if gap > 0 {
                        // Add spaces to text
                        for _ in 0..gap {
                            text.push(' ');
                        }

                        // MUST add spaces to a run so they get rendered
                        // Use the upcoming cell's color so run can potentially merge
                        runs.push(create_text_run(
                            gap,
                            font_family,
                            cell.fg,
                            CellFlags::empty(),
                        ));
                    }

                    current_col = cell.col;

                    // Add the cell character
                    let char_len = cell.c.len_utf8();

                    // Check if we can extend the previous run (same color AND flags)
                    let can_extend = runs.last().is_some_and(|last_run| {
                        last_run.color == cell.fg
                            && last_run.font.weight
                                == if cell.flags.contains(CellFlags::BOLD) {
                                    FontWeight::BOLD
                                } else {
                                    FontWeight::NORMAL
                                }
                    });

                    if can_extend {
                        // Extend previous run
                        runs.last_mut().unwrap().len += char_len;
                    } else {
                        // Start new run for this character
                        runs.push(create_text_run(char_len, font_family, cell.fg, cell.flags));
                    }

                    text.push(cell.c);
                    current_col += 1;
                }

                // Handle empty text
                if text.is_empty() {
                    text.push(' ');
                    runs.push(create_text_run(
                        1,
                        font_family,
                        Hsla::default(),
                        CellFlags::empty(),
                    ));
                }

                window
                    .text_system()
                    .shape_line(text.into(), font_size, &runs, None)
            }
        })
        .collect()
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

    // Track current background region for on-the-fly merging (avoids grid allocation)
    // (row, col_start, col_end, color)
    let mut current_bg: Option<(usize, usize, usize, Hsla)> = None;

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

        // Merge non-default backgrounds on-the-fly
        if bg != default_bg {
            match &mut current_bg {
                Some((cur_row, _start, end, color))
                    if *cur_row == row && *end == col && *color == bg =>
                {
                    // Extend current region
                    *end = col + 1;
                }
                Some((cur_row, start, end, color)) => {
                    // Flush current region, start new
                    bg_regions.push(BgRegion {
                        row: *cur_row,
                        col_start: *start,
                        col_end: *end,
                        color: *color,
                    });
                    current_bg = Some((row, col, col + 1, bg));
                }
                None => {
                    // Start new region
                    current_bg = Some((row, col, col + 1, bg));
                }
            }
        } else if let Some((cur_row, start, end, color)) = current_bg.take() {
            // Non-default to default transition: flush region
            bg_regions.push(BgRegion {
                row: cur_row,
                col_start: start,
                col_end: end,
                color,
            });
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

    // Flush any remaining background region
    if let Some((row, col_start, col_end, color)) = current_bg {
        bg_regions.push(BgRegion {
            row,
            col_start,
            col_end,
            color,
        });
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
        let display_state = self.display.read();
        tracing::trace!(
            rows = display_state.size.rows,
            cols = display_state.size.cols,
            "Terminal render"
        );
        let bg_color = colors.background;
        let font_family: SharedString = FONT_FAMILY.into();

        // Get current font size and calculate cell dimensions
        let current_font_size = display_state.font_size;
        drop(display_state); // Release read lock before write

        // Calculate cell dimensions from actual font metrics
        let (cell_width, cell_height) = get_cell_dimensions(window, current_font_size);
        self.display.write().cell_dims = (cell_width, cell_height);

        // Clone data needed for canvas callbacks (resize happens in prepaint with actual bounds)
        let term = self.term.clone();
        let pty = self.pty.clone();
        let display_arc = self.display.clone();
        let colors_clone = colors;
        let font_family_clone = font_family.clone();

        // Main container with canvas for efficient rendering
        div()
            .id("terminal-pane")
            .key_context("terminal")
            .track_focus(&focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.modifiers.platform && event.keystroke.key.as_str() == "," {
                    crate::app::toggle_settings_dialog(window, cx);
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
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _window, cx| {
                this.handle_file_drop(paths, cx);
            }))
            .size_full()
            .bg(bg_color)
            .child(
                // Canvas for GPU-accelerated terminal rendering
                canvas(
                    // Prepaint: compute render data and shape individual cells
                    move |bounds, window, _cx| {
                        // Get display state and update bounds
                        let (cell_width, cell_height, current_font_size) = {
                            let mut display = display_arc.write();
                            display.bounds = Some(bounds);
                            (display.cell_dims.0, display.cell_dims.1, display.font_size)
                        };

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
                        let needs_resize = {
                            let display = display_arc.read();
                            new_cols != display.size.cols || new_rows != display.size.rows
                        };

                        if needs_resize {
                            // Update display state
                            {
                                let mut display = display_arc.write();
                                display.size.cols = new_cols;
                                display.size.rows = new_rows;
                            }

                            let new_size = TermSize {
                                cols: new_cols,
                                rows: new_rows,
                            };

                            // Resize PTY
                            {
                                let pty_guard = pty.lock();
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
                            {
                                let mut term_guard = term.lock();
                                term_guard.resize(new_size);
                            }
                        }

                        // Get current size for rendering
                        let (cols, rows) = {
                            let display = display_arc.read();
                            (display.size.cols as usize, display.size.rows as usize)
                        };

                        // Build render data from terminal state and get selection
                        let term_guard = term.lock();
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

                        // Shape text by row to reduce allocations (O(rows) instead of O(cells))
                        let font_size = px(current_font_size);
                        let shaped_rows = shape_rows_batched(
                            &render_data.cells,
                            rows,
                            cols,
                            &font_family_clone,
                            font_size,
                            window,
                        );

                        // Use theme selection color with alpha for transparency
                        let selection_color = colors_clone.selection;

                        (
                            render_data,
                            shaped_rows,
                            bounds,
                            cell_width,
                            cell_height,
                            selection_range,
                            cols,
                            selection_color,
                        )
                    },
                    // Paint: draw backgrounds and row-batched text
                    move |_bounds, data, window, cx| {
                        let (
                            render_data,
                            shaped_rows,
                            bounds,
                            cell_width,
                            cell_height,
                            selection_range,
                            cols,
                            selection_color,
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

                        // 2. Paint each row of text (batched for performance)
                        for (row_idx, shaped_line) in shaped_rows.iter().enumerate() {
                            let x = origin.x + px(PADDING);
                            let y = origin.y + px(PADDING + row_idx as f32 * cell_height);
                            let _ = shaped_line.paint(Point::new(x, y), line_height, window, cx);
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
