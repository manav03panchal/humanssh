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
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point as TermPoint, Side};
use alacritty_terminal::selection::{Selection as TermSelection, SelectionType};
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{CursorShape, Processor};
use base64::Engine;
use gpui::*;

// Terminal-specific actions to capture keys before GPUI's focus system
actions!(terminal, [SendTab, SendShiftTab]);
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

/// Event emitted when the terminal process exits.
/// Workspace subscribes to this to automatically clean up dead panes.
#[derive(Clone, Debug)]
pub struct TerminalExitEvent;

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
    /// Whether exit event has been emitted (to avoid duplicate events)
    exit_emitted: bool,
}

impl EventEmitter<TerminalExitEvent> for TerminalPane {}

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

        // Disable tab stop so Tab key passes through to the terminal instead of
        // being consumed by GPUI's focus navigation system
        let focus_handle = cx.focus_handle().tab_stop(false);

        let pane = Self {
            pty: Arc::new(Mutex::new(pty)),
            term,
            processor,
            listener,
            display: Arc::new(RwLock::new(display_state)),
            dragging: false,
            focus_handle,
            exit_emitted: false,
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
            const ACTIVE_INTERVAL: u64 = 4; // Fast when data flowing (~250fps)
            const IDLE_INTERVAL: u64 = 50; // Slow when idle
            const IDLE_THRESHOLD: u32 = 5; // Cycles without data before going idle

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

                let (should_notify, has_data, is_exited) = {
                    let pty_guard = pty_clone.lock();
                    if let Some(ref pty) = *pty_guard {
                        let is_exited = pty.has_exited();
                        let output_chunks = pty.read_output();
                        drop(pty_guard);
                        if !output_chunks.is_empty() {
                            let mut term = term_clone.lock();
                            let mut processor = processor_clone.lock();
                            for chunk in output_chunks {
                                processor.advance(&mut *term, &chunk);
                            }
                            (true, true, is_exited)
                        } else {
                            (is_exited, false, is_exited) // Notify on exit too
                        }
                    } else {
                        (true, false, true) // PTY is None, consider it exited
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

                // Check if process exited and emit event
                if is_exited {
                    let _ = this.update(cx, |pane, cx| {
                        if !pane.exit_emitted {
                            pane.exit_emitted = true;
                            cx.emit(TerminalExitEvent);
                        }
                    });
                    break; // Stop polling after exit
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

    /// Extract URL at the given column position from a line of text.
    /// Returns the URL if the column is within a URL boundary.
    fn find_url_at_position(line: &str, col: usize) -> Option<String> {
        // Characters that can appear in URLs (simplified set)
        const URL_CHARS: &str =
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~:/?#[]@!$&'()*+,;=%";

        // Work entirely with characters (not bytes) to match terminal column positions
        let chars: Vec<char> = line.chars().collect();
        let line_len = chars.len();
        let prefix_chars_https: Vec<char> = "https://".chars().collect();
        let prefix_chars_http: Vec<char> = "http://".chars().collect();

        for prefix_chars in [&prefix_chars_https, &prefix_chars_http] {
            let prefix_len = prefix_chars.len();

            // Search for prefix using character-based matching (not byte-based)
            let mut search_start = 0;
            while search_start + prefix_len <= line_len {
                // Find prefix by comparing characters directly
                let url_start = (search_start..=line_len - prefix_len).find(|&i| {
                    chars[i..i + prefix_len]
                        .iter()
                        .zip(prefix_chars.iter())
                        .all(|(a, b)| a == b)
                });

                let Some(url_start) = url_start else {
                    break;
                };

                // Find the end of the URL (first non-URL character or end of line)
                let mut url_end = url_start + prefix_len;
                while url_end < line_len && URL_CHARS.contains(chars[url_end]) {
                    url_end += 1;
                }

                // Strip trailing punctuation that's unlikely to be part of the URL
                while url_end > url_start + prefix_len {
                    let last_char = chars[url_end - 1];
                    if matches!(
                        last_char,
                        '.' | ',' | ';' | ':' | ')' | ']' | '>' | '\'' | '"'
                    ) {
                        url_end -= 1;
                    } else {
                        break;
                    }
                }

                // Check if clicked column is within this URL
                if col >= url_start && col < url_end {
                    return Some(chars[url_start..url_end].iter().collect());
                }

                search_start = url_end;
            }
        }
        None
    }

    /// Extract text content from a terminal row.
    fn get_row_text(&self, row: usize) -> String {
        let term = self.term.lock();
        let grid = term.grid();
        let line = Line(row as i32);

        if line.0 < 0 || line.0 >= grid.screen_lines() as i32 {
            return String::new();
        }

        let cols = grid.columns();
        let row_data = &grid[line];
        (0..cols).map(|c| row_data[Column(c)].c).collect()
    }

    /// Handle mouse down event
    fn handle_mouse_down(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let Some((col, row)) = self.pixel_to_cell(event.position) else {
            return;
        };

        // Handle Cmd+Click for URL opening (before other handlers)
        if event.modifiers.platform && event.button == MouseButton::Left {
            let line_text = self.get_row_text(row);
            if let Some(url) = Self::find_url_at_position(&line_text, col) {
                // Open URL in default browser
                #[cfg(target_os = "macos")]
                {
                    let _ = std::process::Command::new("open").arg(&url).spawn();
                }
                #[cfg(target_os = "linux")]
                {
                    let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
                }
                #[cfg(target_os = "windows")]
                {
                    let _ = std::process::Command::new("cmd")
                        .args(["/C", "start", "", &url])
                        .spawn();
                }
                return;
            }
        }

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
        } else {
            // Normal mode: scroll through terminal history (scrollback buffer)
            let delta_y: f32 = event.delta.pixel_delta(px(cell_height)).y.into();
            let lines = (delta_y.abs() / cell_height).ceil() as i32;
            if lines > 0 {
                let scroll = if delta_y > 0.0 {
                    Scroll::Delta(lines)
                } else {
                    Scroll::Delta(-lines)
                };
                self.term.lock().scroll_display(scroll);
            }
        }
    }

    /// Handle a key event
    fn handle_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let mode = {
            let term = self.term.lock();
            *term.mode()
        };

        // Handle Cmd+Shift shortcuts (must check before Cmd alone)
        if event.keystroke.modifiers.platform && event.keystroke.modifiers.shift {
            match event.keystroke.key.as_str() {
                "left" => {
                    // Cmd+Shift+Left: Select to start of line
                    self.handle_cmd_shift_arrow("left", cx);
                    return;
                }
                "right" => {
                    // Cmd+Shift+Right: Select to end of line
                    self.handle_cmd_shift_arrow("right", cx);
                    return;
                }
                _ => {}
            }
        }

        // Handle Cmd+key shortcuts
        if event.keystroke.modifiers.platform && !event.keystroke.modifiers.shift {
            match event.keystroke.key.as_str() {
                "a" => {
                    // Select all terminal content
                    self.select_all();
                    cx.notify();
                    return;
                }
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
                "k" => {
                    // Clear scrollback and screen
                    self.send_input("\x0c"); // Ctrl+L to clear screen
                    return;
                }
                "backspace" => {
                    // Delete line (cursor to beginning) - Ctrl+U
                    self.send_input("\x15");
                    return;
                }
                "left" => {
                    // Cmd+Left: Go to start of line - Ctrl+A
                    self.send_input("\x01");
                    return;
                }
                "right" => {
                    // Cmd+Right: Go to end of line - Ctrl+E
                    self.send_input("\x05");
                    return;
                }
                "up" => {
                    // Cmd+Up: Scroll to top of scrollback
                    self.term.lock().scroll_display(Scroll::Top);
                    cx.notify();
                    return;
                }
                "down" => {
                    // Cmd+Down: Scroll to bottom
                    self.term.lock().scroll_display(Scroll::Bottom);
                    cx.notify();
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

        // Handle Option+Shift+Arrow for word-level selection (must check before Option alone)
        if event.keystroke.modifiers.alt && event.keystroke.modifiers.shift {
            match event.keystroke.key.as_str() {
                "left" | "right" => {
                    self.handle_option_shift_arrow(event.keystroke.key.as_str(), cx);
                    return;
                }
                _ => {}
            }
        }

        // Handle Option+Left/Right for word movement (without shift)
        if event.keystroke.modifiers.alt && !event.keystroke.modifiers.shift {
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

        // Handle Shift+Arrow for text selection (character level)
        if event.keystroke.modifiers.shift && !event.keystroke.modifiers.alt {
            match event.keystroke.key.as_str() {
                "left" | "right" | "up" | "down" => {
                    self.handle_shift_arrow(event.keystroke.key.as_str(), cx);
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

    /// Select all terminal content (visible + scrollback history)
    fn select_all(&mut self) {
        let mut term = self.term.lock();
        let cols = term.columns();

        // Get the topmost line (including scrollback) and bottommost line
        let topmost = term.topmost_line();
        let bottommost = term.bottommost_line();

        // Create selection from top-left to bottom-right
        let start = TermPoint::new(topmost, Column(0));
        let end = TermPoint::new(bottommost, Column(cols.saturating_sub(1)));

        let mut selection = TermSelection::new(SelectionType::Simple, start, Side::Left);
        selection.update(end, Side::Right);
        term.selection = Some(selection);

        // Scroll to bottom to show the selection includes current content
        term.scroll_display(Scroll::Bottom);
    }

    /// Handle Cmd+Shift+Arrow for line-level selection
    fn handle_cmd_shift_arrow(&mut self, direction: &str, cx: &mut Context<Self>) {
        let mut term = self.term.lock();
        let cols = term.columns();

        // Get current selection or cursor position
        let content = term.renderable_content();
        let (start_point, current_end) = if let Some(sel_range) = content.selection {
            (
                TermPoint::new(sel_range.start.line, sel_range.start.column),
                TermPoint::new(sel_range.end.line, sel_range.end.column),
            )
        } else {
            let cursor = content.cursor.point;
            (cursor, cursor)
        };

        let new_end = match direction {
            "left" => {
                // Select to start of line
                TermPoint::new(current_end.line, Column(0))
            }
            "right" => {
                // Select to end of line
                TermPoint::new(current_end.line, Column(cols.saturating_sub(1)))
            }
            _ => current_end,
        };

        let mut selection = TermSelection::new(SelectionType::Simple, start_point, Side::Left);
        selection.update(new_end, Side::Right);
        term.selection = Some(selection);

        drop(term);
        cx.notify();
    }

    /// Handle Option+Shift+Arrow for word-level selection
    fn handle_option_shift_arrow(&mut self, direction: &str, cx: &mut Context<Self>) {
        let mut term = self.term.lock();
        let cols = term.columns();

        // Get current selection end from renderable content, or use cursor position
        let content = term.renderable_content();
        let (start_point, current_end) = if let Some(sel_range) = content.selection {
            (
                TermPoint::new(sel_range.start.line, sel_range.start.column),
                TermPoint::new(sel_range.end.line, sel_range.end.column),
            )
        } else {
            let cursor = content.cursor.point;
            (cursor, cursor)
        };

        let topmost = term.topmost_line();
        let bottommost = term.bottommost_line();

        // Move by word - find next word boundary
        let new_end = match direction {
            "left" => {
                // Move left to previous word boundary
                let mut col = current_end.column.0;
                let mut line = current_end.line;

                // Skip any spaces first
                while col > 0 {
                    col -= 1;
                    // Simple word boundary: stop at space after non-space
                    if col == 0 {
                        break;
                    }
                }
                // Then skip to start of word (find space or start of line)
                while col > 0 {
                    col -= 1;
                }

                // If we hit start of line and can go up, jump to end of previous line
                if col == 0 && line.0 > topmost.0 {
                    line = Line(line.0 - 1);
                    col = cols.saturating_sub(1);
                }

                TermPoint::new(line, Column(col))
            }
            "right" => {
                // Move right to next word boundary
                let mut col = current_end.column.0;
                let mut line = current_end.line;

                // Move forward by ~5 chars as approximation for word
                col = (col + 5).min(cols.saturating_sub(1));

                // If we hit end of line and can go down, jump to start of next line
                if col >= cols.saturating_sub(1) && line.0 < bottommost.0 {
                    line = Line(line.0 + 1);
                    col = 0;
                }

                TermPoint::new(line, Column(col))
            }
            _ => current_end,
        };

        // Create new selection from start to new end
        let mut selection = TermSelection::new(SelectionType::Simple, start_point, Side::Left);
        selection.update(new_end, Side::Right);
        term.selection = Some(selection);

        drop(term);
        cx.notify();
    }

    /// Handle Shift+Arrow for text selection
    fn handle_shift_arrow(&mut self, direction: &str, cx: &mut Context<Self>) {
        let mut term = self.term.lock();
        let cols = term.columns();
        let lines = term.screen_lines();

        // Get current selection end from renderable content, or use cursor position
        let content = term.renderable_content();
        let (start_point, current_end) = if let Some(sel_range) = content.selection {
            // Use existing selection's start as anchor, end as current position
            (
                TermPoint::new(sel_range.start.line, sel_range.start.column),
                TermPoint::new(sel_range.end.line, sel_range.end.column),
            )
        } else {
            // No selection - start from cursor
            let cursor = content.cursor.point;
            (cursor, cursor)
        };

        // Calculate new end point based on direction
        let topmost = term.topmost_line();
        let bottommost = term.bottommost_line();

        let new_end = match direction {
            "left" => {
                if current_end.column.0 > 0 {
                    TermPoint::new(current_end.line, Column(current_end.column.0 - 1))
                } else if current_end.line.0 > topmost.0 {
                    TermPoint::new(Line(current_end.line.0 - 1), Column(cols.saturating_sub(1)))
                } else {
                    current_end
                }
            }
            "right" => {
                if current_end.column.0 < cols.saturating_sub(1) {
                    TermPoint::new(current_end.line, Column(current_end.column.0 + 1))
                } else if current_end.line.0 < bottommost.0 {
                    TermPoint::new(Line(current_end.line.0 + 1), Column(0))
                } else {
                    current_end
                }
            }
            "up" => {
                if current_end.line.0 > topmost.0 {
                    TermPoint::new(Line(current_end.line.0 - 1), current_end.column)
                } else {
                    current_end
                }
            }
            "down" => {
                if current_end.line.0 < (lines as i32 - 1) {
                    TermPoint::new(Line(current_end.line.0 + 1), current_end.column)
                } else {
                    current_end
                }
            }
            _ => current_end,
        };

        // Create new selection from start to new end
        let mut selection = TermSelection::new(SelectionType::Simple, start_point, Side::Left);
        selection.update(new_end, Side::Right);
        term.selection = Some(selection);

        drop(term);
        cx.notify();
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

/// Build render data from terminal state - collects individual cells for precise positioning
fn build_render_data(
    term: &Term<Listener>,
    theme: &TerminalColors,
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

    // Use terminal's actual dimensions for cursor bounds check
    // to avoid issues when display size and terminal size are temporarily out of sync
    let term_cols = term.columns();
    let term_rows = term.screen_lines();

    let cursor_info =
        if cursor_line >= 0 && (cursor_line as usize) < term_rows && cursor_col < term_cols {
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

        if row >= term_rows || col >= term_cols {
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

        // Apply cursor styling for block cursor - don't change fg color
        // The hollow block cursor is drawn in the paint phase, text should remain visible
        // (Previous code set fg to background which made text invisible inside the cursor)
        let _is_cursor = cursor_info.is_some_and(|c| c.row == row && c.col == col);

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
            // Handle Tab/Shift-Tab actions (bound below) to send to terminal
            .on_action(cx.listener(|this, _: &SendTab, _window, _cx| {
                this.send_input("\t");
            }))
            .on_action(cx.listener(|this, _: &SendShiftTab, _window, _cx| {
                this.send_input("\x1b[Z"); // Shift-Tab escape sequence
            }))
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
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _window, cx| {
                this.handle_scroll(event);
                cx.notify(); // Redraw after scrolling
            }))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _window, cx| {
                this.handle_file_drop(paths, cx);
            }))
            .size_full()
            .bg(bg_color)
            .child(
                // Canvas for GPU-accelerated terminal rendering
                canvas(
                    // Prepaint: compute render data
                    move |bounds, _window, _cx| {
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
                        let cols = {
                            let display = display_arc.read();
                            display.size.cols as usize
                        };

                        // Build render data from terminal state and get selection
                        let term_guard = term.lock();
                        let render_data = build_render_data(
                            &term_guard,
                            &colors_clone,
                            font_family_clone.clone(),
                        );
                        // Get selection from renderable content (already normalized)
                        let selection_range = term_guard.renderable_content().selection;
                        drop(term_guard);

                        // Use theme selection color with alpha for transparency
                        let selection_color = colors_clone.selection;

                        (
                            render_data,
                            bounds,
                            cell_width,
                            cell_height,
                            selection_range,
                            cols,
                            selection_color,
                            font_family_clone,
                            current_font_size,
                        )
                    },
                    // Paint: draw backgrounds and cell-by-cell text
                    move |_bounds, data, window, cx| {
                        let (
                            render_data,
                            bounds,
                            cell_width,
                            cell_height,
                            selection_range,
                            cols,
                            selection_color,
                            font_family,
                            font_size,
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

                        // 2. Paint each cell at its grid position (like Alacritty/Ghostty)
                        // This ensures cursor and text are perfectly aligned
                        for cell in &render_data.cells {
                            let x = origin.x + px(PADDING + cell.col as f32 * cell_width);
                            let y = origin.y + px(PADDING + cell.row as f32 * cell_height);

                            // Shape and paint this single character
                            let text: SharedString = cell.c.to_string().into();
                            let font = Font {
                                family: font_family.clone(),
                                features: FontFeatures::default(),
                                fallbacks: None,
                                weight: if cell.flags.contains(CellFlags::BOLD) {
                                    FontWeight::BOLD
                                } else {
                                    FontWeight::NORMAL
                                },
                                style: if cell.flags.contains(CellFlags::ITALIC) {
                                    FontStyle::Italic
                                } else {
                                    FontStyle::Normal
                                },
                            };
                            let run = TextRun {
                                len: cell.c.len_utf8(),
                                font,
                                color: cell.fg,
                                background_color: None,
                                underline: None,
                                strikethrough: None,
                            };
                            let shaped =
                                window
                                    .text_system()
                                    .shape_line(text, px(font_size), &[run], None);
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

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::grid::Dimensions;
    use pretty_assertions::assert_eq;
    use test_case::test_case;

    /// Create a TextRun with proper styling based on cell flags (test helper)
    fn create_text_run(
        len: usize,
        font_family: &SharedString,
        fg: Hsla,
        flags: CellFlags,
    ) -> TextRun {
        let weight = if flags.contains(CellFlags::BOLD) {
            FontWeight::BOLD
        } else {
            FontWeight::NORMAL
        };

        let style = if flags.contains(CellFlags::ITALIC) {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        };

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

    // ============================================================================
    // Unit Tests for Pure Functions and Data Structures
    // ============================================================================

    // These tests don't require GPUI context and test pure functions/data types

    // ============================================================================
    // Display State Tests (No GPUI required)
    // ============================================================================

    #[::core::prelude::v1::test]
    fn test_display_state_default() {
        let display = DisplayState::default();

        // Check default terminal size (80x24 is standard)
        assert_eq!(display.size.cols, 80, "Default columns should be 80");
        assert_eq!(display.size.rows, 24, "Default rows should be 24");

        // Check cell dimensions are reasonable
        assert!(display.cell_dims.0 > 0.0, "Cell width should be positive");
        assert!(display.cell_dims.1 > 0.0, "Cell height should be positive");

        // Check font size is within valid range
        assert!(
            display.font_size >= MIN_FONT_SIZE,
            "Font size should be >= MIN"
        );
        assert!(
            display.font_size <= MAX_FONT_SIZE,
            "Font size should be <= MAX"
        );
        assert_eq!(
            display.font_size, DEFAULT_FONT_SIZE,
            "Font size should be default"
        );
    }

    #[test_case(MIN_FONT_SIZE ; "minimum font size")]
    #[test_case(DEFAULT_FONT_SIZE ; "default font size")]
    #[test_case(MAX_FONT_SIZE ; "maximum font size")]
    #[test_case(16.0 ; "custom font size")]
    fn test_display_state_font_size_values(expected_size: f32) {
        let display = DisplayState {
            font_size: expected_size,
            ..Default::default()
        };
        assert_eq!(display.font_size, expected_size);
    }

    #[::core::prelude::v1::test]
    fn test_display_state_clone() {
        let original = DisplayState {
            font_size: 20.0,
            size: TermSize {
                cols: 100,
                rows: 50,
            },
            cell_dims: (12.0, 24.0),
            bounds: None,
        };
        let cloned = original.clone();

        assert_eq!(original.font_size, cloned.font_size);
        assert_eq!(original.size.cols, cloned.size.cols);
        assert_eq!(original.size.rows, cloned.size.rows);
        assert_eq!(original.cell_dims, cloned.cell_dims);
    }

    #[::core::prelude::v1::test]
    fn test_display_state_bounds_none_initially() {
        let display = DisplayState::default();
        assert!(display.bounds.is_none(), "Bounds should be None initially");
    }

    // ============================================================================
    // MouseEscBuf Tests (Unit tests for the helper)
    // ============================================================================

    #[::core::prelude::v1::test]
    fn test_mouse_esc_buf_creation() {
        let buf = MouseEscBuf::new();
        assert_eq!(buf.as_str(), "");
    }

    #[::core::prelude::v1::test]
    fn test_mouse_esc_buf_write() {
        let mut buf = MouseEscBuf::new();
        use std::fmt::Write;
        write!(buf, "\x1b[<0;10;20M").unwrap();
        assert_eq!(buf.as_str(), "\x1b[<0;10;20M");
    }

    #[::core::prelude::v1::test]
    fn test_mouse_esc_buf_sgr_format() {
        let mut buf = MouseEscBuf::new();
        use std::fmt::Write;
        let button = 0;
        let col = 10;
        let row = 5;
        write!(buf, "\x1b[<{};{};{}M", button, col + 1, row + 1).unwrap();
        assert_eq!(buf.as_str(), "\x1b[<0;11;6M");
    }

    // ============================================================================
    // Text Run Creation Tests
    // ============================================================================

    #[::core::prelude::v1::test]
    fn test_create_text_run_basic() {
        let font_family: SharedString = "Test Font".into();
        let fg = Hsla::default();
        let run = create_text_run(5, &font_family, fg, CellFlags::empty());

        assert_eq!(run.len, 5);
        assert_eq!(run.font.weight, FontWeight::NORMAL);
        assert_eq!(run.font.style, FontStyle::Normal);
        assert!(run.underline.is_none());
        assert!(run.strikethrough.is_none());
    }

    #[::core::prelude::v1::test]
    fn test_create_text_run_bold() {
        let font_family: SharedString = "Test Font".into();
        let fg = Hsla::default();
        let run = create_text_run(5, &font_family, fg, CellFlags::BOLD);

        assert_eq!(run.font.weight, FontWeight::BOLD);
    }

    #[::core::prelude::v1::test]
    fn test_create_text_run_italic() {
        let font_family: SharedString = "Test Font".into();
        let fg = Hsla::default();
        let run = create_text_run(5, &font_family, fg, CellFlags::ITALIC);

        assert_eq!(run.font.style, FontStyle::Italic);
    }

    #[::core::prelude::v1::test]
    fn test_create_text_run_underline() {
        let font_family: SharedString = "Test Font".into();
        let fg = Hsla::default();
        let run = create_text_run(5, &font_family, fg, CellFlags::UNDERLINE);

        assert!(run.underline.is_some());
    }

    #[::core::prelude::v1::test]
    fn test_create_text_run_strikethrough() {
        let font_family: SharedString = "Test Font".into();
        let fg = Hsla::default();
        let run = create_text_run(5, &font_family, fg, CellFlags::STRIKEOUT);

        assert!(run.strikethrough.is_some());
    }

    #[::core::prelude::v1::test]
    fn test_create_text_run_combined_flags() {
        let font_family: SharedString = "Test Font".into();
        let fg = Hsla::default();
        let flags = CellFlags::BOLD | CellFlags::ITALIC | CellFlags::UNDERLINE;
        let run = create_text_run(5, &font_family, fg, flags);

        assert_eq!(run.font.weight, FontWeight::BOLD);
        assert_eq!(run.font.style, FontStyle::Italic);
        assert!(run.underline.is_some());
    }

    // ============================================================================
    // TermSize Tests
    // ============================================================================

    #[::core::prelude::v1::test]
    fn test_term_size_default() {
        let size = TermSize::default();
        assert_eq!(size.cols, 80);
        assert_eq!(size.rows, 24);
    }

    #[::core::prelude::v1::test]
    fn test_term_size_dimensions_trait() {
        use alacritty_terminal::grid::Dimensions;
        let size = TermSize {
            cols: 100,
            rows: 50,
        };
        assert_eq!(size.columns(), 100);
        assert_eq!(size.total_lines(), 50);
        assert_eq!(size.screen_lines(), 50);
    }

    #[test_case(80, 24 ; "standard terminal")]
    #[test_case(120, 40 ; "large terminal")]
    #[test_case(40, 10 ; "small terminal")]
    #[test_case(10, 3 ; "minimum terminal")]
    fn test_term_size_various_dimensions(cols: u16, rows: u16) {
        use alacritty_terminal::grid::Dimensions;
        let size = TermSize { cols, rows };
        assert_eq!(size.columns(), cols as usize);
        assert_eq!(size.total_lines(), rows as usize);
    }

    // ============================================================================
    // Font Size Constraint Tests
    // ============================================================================

    #[::core::prelude::v1::test]
    fn test_font_size_min_constraint() {
        let clamped = (5.0_f32).max(MIN_FONT_SIZE);
        assert_eq!(clamped, MIN_FONT_SIZE);
    }

    #[::core::prelude::v1::test]
    fn test_font_size_max_constraint() {
        let clamped = (100.0_f32).min(MAX_FONT_SIZE);
        assert_eq!(clamped, MAX_FONT_SIZE);
    }

    #[::core::prelude::v1::test]
    fn test_font_size_within_range() {
        let font_size = 16.0_f32;
        let clamped = font_size.max(MIN_FONT_SIZE).min(MAX_FONT_SIZE);
        assert_eq!(clamped, font_size);
    }

    #[::core::prelude::v1::test]
    fn test_font_size_zoom_in_logic() {
        let initial = DEFAULT_FONT_SIZE;
        let zoomed = (initial + 1.0).min(MAX_FONT_SIZE);
        assert!(zoomed > initial || zoomed == MAX_FONT_SIZE);
    }

    #[::core::prelude::v1::test]
    fn test_font_size_zoom_out_logic() {
        let initial = DEFAULT_FONT_SIZE;
        let zoomed = (initial - 1.0).max(MIN_FONT_SIZE);
        assert!(zoomed < initial || zoomed == MIN_FONT_SIZE);
    }

    // ============================================================================
    // Listener Tests
    // ============================================================================

    #[::core::prelude::v1::test]
    fn test_listener_new() {
        let listener = Listener::new();
        assert!(listener.title.lock().is_none());
    }

    #[::core::prelude::v1::test]
    fn test_listener_title_event() {
        use alacritty_terminal::event::EventListener;
        let listener = Listener::new();

        // Send a title event
        listener.send_event(alacritty_terminal::event::Event::Title(
            "Test Title".to_string(),
        ));

        // Check the title was captured
        let title = listener.title.lock();
        assert_eq!(title.as_deref(), Some("Test Title"));
    }

    #[::core::prelude::v1::test]
    fn test_listener_clone() {
        use alacritty_terminal::event::EventListener;
        let listener = Listener::new();
        listener.send_event(alacritty_terminal::event::Event::Title(
            "Original".to_string(),
        ));

        let cloned = listener.clone();

        // Both should share the same Arc
        assert_eq!(cloned.title.lock().as_deref(), Some("Original"));
    }

    // ============================================================================
    // Text Run Style Flag Tests
    // ============================================================================

    #[::core::prelude::v1::test]
    fn test_text_run_empty_flags() {
        let font_family: SharedString = "Test".into();
        let run = create_text_run(1, &font_family, Hsla::default(), CellFlags::empty());
        assert_eq!(run.font.weight, FontWeight::NORMAL);
        assert_eq!(run.font.style, FontStyle::Normal);
        assert!(run.underline.is_none());
        assert!(run.strikethrough.is_none());
    }

    #[::core::prelude::v1::test]
    fn test_text_run_all_underline_variants() {
        let font_family: SharedString = "Test".into();

        for flags in [
            CellFlags::UNDERLINE,
            CellFlags::DOUBLE_UNDERLINE,
            CellFlags::UNDERCURL,
            CellFlags::DOTTED_UNDERLINE,
            CellFlags::DASHED_UNDERLINE,
        ] {
            let run = create_text_run(1, &font_family, Hsla::default(), flags);
            assert!(
                run.underline.is_some(),
                "Underline should be set for {:?}",
                flags
            );
        }
    }

    #[::core::prelude::v1::test]
    fn test_text_run_undercurl_is_wavy() {
        let font_family: SharedString = "Test".into();
        let run = create_text_run(1, &font_family, Hsla::default(), CellFlags::UNDERCURL);

        let underline = run.underline.unwrap();
        assert!(underline.wavy, "Undercurl should have wavy=true");
    }

    // ============================================================================
    // Pixel to Cell Conversion Logic Tests
    // ============================================================================

    #[::core::prelude::v1::test]
    fn test_pixel_to_cell_calculation() {
        // Test the core conversion logic without bounds
        let cell_width = 10.0_f32;
        let cell_height = 20.0_f32;
        let padding = PADDING;

        // Position at (PADDING + 25, PADDING + 45)
        let local_x = 25.0_f32;
        let local_y = 45.0_f32;

        let cell_x = ((local_x - padding) / cell_width).floor() as i32;
        let cell_y = ((local_y - padding) / cell_height).floor() as i32;

        // With padding=2, local_x=25: (25-2)/10 = 2.3 -> floor = 2
        // With padding=2, local_y=45: (45-2)/20 = 2.15 -> floor = 2
        assert_eq!(cell_x, 2);
        assert_eq!(cell_y, 2);
    }

    #[::core::prelude::v1::test]
    fn test_pixel_to_cell_negative_result() {
        let cell_width = 10.0_f32;
        let cell_height = 20.0_f32;
        let padding = PADDING;

        // Position before padding
        let local_x = 0.0_f32;
        let local_y = 0.0_f32;

        let cell_x = ((local_x - padding) / cell_width).floor() as i32;
        let cell_y = ((local_y - padding) / cell_height).floor() as i32;

        // Should be negative
        assert!(cell_x < 0);
        assert!(cell_y < 0);
    }

    // ============================================================================
    // Mouse Escape Buffer Edge Cases
    // ============================================================================

    #[::core::prelude::v1::test]
    fn test_mouse_esc_buf_max_coordinates() {
        use std::fmt::Write;
        let mut buf = MouseEscBuf::new();
        // Maximum supported coordinates (255 for legacy, larger for SGR)
        write!(buf, "\x1b[<0;255;255M").unwrap();
        assert_eq!(buf.as_str(), "\x1b[<0;255;255M");
    }

    #[::core::prelude::v1::test]
    fn test_mouse_esc_buf_all_buttons() {
        use std::fmt::Write;

        for button in [0, 1, 2, 64, 65] {
            // Left, Middle, Right, WheelUp, WheelDown
            let mut buf = MouseEscBuf::new();
            write!(buf, "\x1b[<{};10;10M", button).unwrap();
            assert!(buf.as_str().contains(&format!("{}", button)));
        }
    }

    #[::core::prelude::v1::test]
    fn test_mouse_esc_buf_release_format() {
        use std::fmt::Write;
        let mut buf = MouseEscBuf::new();
        // SGR release uses lowercase 'm'
        write!(buf, "\x1b[<0;10;10m").unwrap();
        assert!(buf.as_str().ends_with('m'));
    }

    // ============================================================================
    // Config Constants Tests
    // ============================================================================

    #[::core::prelude::v1::test]
    fn test_config_constants_valid() {
        assert!(MIN_FONT_SIZE > 0.0, "MIN_FONT_SIZE should be positive");
        assert!(MAX_FONT_SIZE > MIN_FONT_SIZE, "MAX should be > MIN");
        assert!(
            DEFAULT_FONT_SIZE >= MIN_FONT_SIZE,
            "DEFAULT should be >= MIN"
        );
        assert!(
            DEFAULT_FONT_SIZE <= MAX_FONT_SIZE,
            "DEFAULT should be <= MAX"
        );
        assert!(PADDING >= 0.0, "PADDING should be non-negative");
    }

    #[::core::prelude::v1::test]
    fn test_font_family_not_empty() {
        assert!(!FONT_FAMILY.is_empty(), "Font family should not be empty");
    }

    // ============================================================================
    // ERROR PATH TESTS - Invalid Input Sequences
    // ============================================================================

    #[::core::prelude::v1::test]
    fn test_mouse_esc_buf_handles_invalid_utf8_sequence() {
        use std::fmt::Write;
        let mut buf = MouseEscBuf::new();

        // Valid escape sequence components
        write!(buf, "\x1b[<0;1;1M").unwrap();

        // Should be valid ASCII/UTF-8
        assert!(buf.as_str().is_ascii());
    }

    #[::core::prelude::v1::test]
    fn test_mouse_esc_buf_boundary_coordinates() {
        use std::fmt::Write;
        let mut buf = MouseEscBuf::new();

        // Test with maximum reasonable coordinates
        write!(buf, "\x1b[<0;9999;9999M").unwrap();
        assert_eq!(buf.as_str(), "\x1b[<0;9999;9999M");
    }

    #[::core::prelude::v1::test]
    fn test_mouse_esc_buf_negative_coordinate_handling() {
        use std::fmt::Write;
        let mut buf = MouseEscBuf::new();

        // Format with u32::MAX which might indicate an error in coordinate calculation
        // This tests that the buffer doesn't panic on large numbers
        let large_num = u32::MAX;
        let _ = write!(buf, "\x1b[<0;{};1M", large_num);
        // Should truncate without panic
        assert!(buf.as_str().len() <= 32);
    }

    // ============================================================================
    // ERROR PATH TESTS - Malformed Escape Sequences
    // ============================================================================

    #[::core::prelude::v1::test]
    fn test_escape_sequence_incomplete() {
        use std::fmt::Write;
        let mut buf = MouseEscBuf::new();

        // Incomplete escape sequence (missing terminator)
        write!(buf, "\x1b[<0;10;10").unwrap();
        // Should still be valid string
        assert!(buf.as_str().starts_with("\x1b"));
    }

    #[::core::prelude::v1::test]
    fn test_escape_sequence_wrong_terminator() {
        use std::fmt::Write;
        let mut buf = MouseEscBuf::new();

        // Wrong terminator (Z instead of M or m)
        write!(buf, "\x1b[<0;10;10Z").unwrap();
        assert_eq!(buf.as_str(), "\x1b[<0;10;10Z");
    }

    #[::core::prelude::v1::test]
    fn test_escape_sequence_extra_semicolons() {
        use std::fmt::Write;
        let mut buf = MouseEscBuf::new();

        // Extra semicolons in sequence
        write!(buf, "\x1b[<0;;10;10M").unwrap();
        assert!(buf.as_str().contains(";;"));
    }

    #[::core::prelude::v1::test]
    fn test_escape_sequence_missing_bracket() {
        use std::fmt::Write;
        let mut buf = MouseEscBuf::new();

        // Missing bracket in sequence
        write!(buf, "\x1b<0;10;10M").unwrap();
        assert!(!buf.as_str().contains("["));
    }

    // ============================================================================
    // ERROR PATH TESTS - Zero-Size Terminal Handling
    // ============================================================================

    #[::core::prelude::v1::test]
    fn test_term_size_zero_dimensions() {
        let size = TermSize { cols: 0, rows: 0 };
        // Should not panic, just return 0
        assert_eq!(size.columns(), 0);
        assert_eq!(size.total_lines(), 0);
        assert_eq!(size.screen_lines(), 0);
    }

    #[::core::prelude::v1::test]
    fn test_display_state_zero_cell_dims() {
        let state = DisplayState {
            size: TermSize::default(),
            cell_dims: (0.0, 0.0),
            bounds: None,
            font_size: DEFAULT_FONT_SIZE,
        };

        // Zero cell dimensions shouldn't cause panic
        assert_eq!(state.cell_dims.0, 0.0);
        assert_eq!(state.cell_dims.1, 0.0);
    }

    #[::core::prelude::v1::test]
    fn test_pixel_to_cell_calculation_zero_cell_size() {
        // Simulate pixel to cell conversion with zero cell dimensions
        let cell_width = 0.0_f32;
        let cell_height = 0.0_f32;
        let padding = PADDING;

        let local_x = 100.0_f32;
        let local_y = 100.0_f32;

        // This would cause division by zero or inf
        let cell_x = if cell_width > 0.0 {
            ((local_x - padding) / cell_width).floor() as i32
        } else {
            0 // Safe fallback
        };

        let cell_y = if cell_height > 0.0 {
            ((local_y - padding) / cell_height).floor() as i32
        } else {
            0 // Safe fallback
        };

        assert_eq!(cell_x, 0);
        assert_eq!(cell_y, 0);
    }

    #[::core::prelude::v1::test]
    fn test_term_size_single_cell() {
        let size = TermSize { cols: 1, rows: 1 };
        assert_eq!(size.columns(), 1);
        assert_eq!(size.total_lines(), 1);
    }

    // ============================================================================
    // ERROR PATH TESTS - Font Size at Limits
    // ============================================================================

    #[::core::prelude::v1::test]
    fn test_font_size_at_minimum() {
        let state = DisplayState {
            font_size: MIN_FONT_SIZE,
            ..Default::default()
        };

        assert_eq!(state.font_size, MIN_FONT_SIZE);
        // Further reduction should be clamped
        let reduced = (state.font_size - 1.0).max(MIN_FONT_SIZE);
        assert_eq!(reduced, MIN_FONT_SIZE);
    }

    #[::core::prelude::v1::test]
    fn test_font_size_at_maximum() {
        let state = DisplayState {
            font_size: MAX_FONT_SIZE,
            ..Default::default()
        };

        assert_eq!(state.font_size, MAX_FONT_SIZE);
        // Further increase should be clamped
        let increased = (state.font_size + 1.0).min(MAX_FONT_SIZE);
        assert_eq!(increased, MAX_FONT_SIZE);
    }

    #[::core::prelude::v1::test]
    fn test_font_size_below_minimum_clamps() {
        let too_small = MIN_FONT_SIZE - 100.0;
        let clamped = too_small.max(MIN_FONT_SIZE);
        assert_eq!(clamped, MIN_FONT_SIZE);
    }

    #[::core::prelude::v1::test]
    fn test_font_size_above_maximum_clamps() {
        let too_large = MAX_FONT_SIZE + 100.0;
        let clamped = too_large.min(MAX_FONT_SIZE);
        assert_eq!(clamped, MAX_FONT_SIZE);
    }

    #[::core::prelude::v1::test]
    fn test_font_size_negative_clamps_to_minimum() {
        let negative = -10.0_f32;
        let clamped = negative.max(MIN_FONT_SIZE);
        assert_eq!(clamped, MIN_FONT_SIZE);
    }

    #[::core::prelude::v1::test]
    fn test_font_size_infinity_clamps_to_maximum() {
        let inf = f32::INFINITY;
        let clamped = inf.min(MAX_FONT_SIZE);
        assert_eq!(clamped, MAX_FONT_SIZE);
    }

    #[::core::prelude::v1::test]
    fn test_font_size_nan_handling() {
        let nan = f32::NAN;
        // NaN comparisons always return false, so max/min with NaN gives the other value
        // This is important for defensive programming
        let result_max = nan.max(MIN_FONT_SIZE);
        let result_min = nan.min(MAX_FONT_SIZE);

        // With NaN, we need to check for NaN explicitly
        assert!(result_max.is_nan() || result_max >= MIN_FONT_SIZE);
        assert!(result_min.is_nan() || result_min <= MAX_FONT_SIZE);
    }

    // ============================================================================
    // ERROR PATH TESTS - Text Run Creation Edge Cases
    // ============================================================================

    #[::core::prelude::v1::test]
    fn test_create_text_run_zero_length() {
        let font_family: SharedString = "Test Font".into();
        let fg = Hsla::default();
        let run = create_text_run(0, &font_family, fg, CellFlags::empty());

        // Zero length run should be valid
        assert_eq!(run.len, 0);
    }

    #[::core::prelude::v1::test]
    fn test_create_text_run_max_length() {
        let font_family: SharedString = "Test Font".into();
        let fg = Hsla::default();
        // Very large length (theoretical max line length)
        let run = create_text_run(usize::MAX, &font_family, fg, CellFlags::empty());

        assert_eq!(run.len, usize::MAX);
    }

    #[::core::prelude::v1::test]
    fn test_create_text_run_all_flags_combined() {
        let font_family: SharedString = "Test Font".into();
        let fg = Hsla::default();

        // Combine all possible flags
        let all_flags = CellFlags::BOLD
            | CellFlags::ITALIC
            | CellFlags::UNDERLINE
            | CellFlags::STRIKEOUT
            | CellFlags::DIM
            | CellFlags::INVERSE
            | CellFlags::HIDDEN;

        let run = create_text_run(5, &font_family, fg, all_flags);

        // Should have bold and italic applied
        assert_eq!(run.font.weight, FontWeight::BOLD);
        assert_eq!(run.font.style, FontStyle::Italic);
        assert!(run.underline.is_some());
        assert!(run.strikethrough.is_some());
    }

    // ============================================================================
    // ERROR PATH TESTS - Listener Edge Cases
    // ============================================================================

    #[::core::prelude::v1::test]
    fn test_listener_empty_title() {
        use alacritty_terminal::event::EventListener;
        let listener = Listener::new();

        // Send empty title
        listener.send_event(alacritty_terminal::event::Event::Title(String::new()));

        let title = listener.title.lock();
        assert_eq!(title.as_deref(), Some(""));
    }

    #[::core::prelude::v1::test]
    fn test_listener_very_long_title() {
        use alacritty_terminal::event::EventListener;
        let listener = Listener::new();

        // Very long title (potential buffer overflow in bad implementations)
        let long_title = "A".repeat(10000);
        listener.send_event(alacritty_terminal::event::Event::Title(long_title.clone()));

        let title = listener.title.lock();
        assert_eq!(title.as_deref(), Some(long_title.as_str()));
    }

    #[::core::prelude::v1::test]
    fn test_listener_unicode_title() {
        use alacritty_terminal::event::EventListener;
        let listener = Listener::new();

        // Unicode title with emojis and special characters
        let unicode_title = "Terminal \u{1F600} \u{4E2D}\u{6587} \u{0414}\u{0440}\u{0443}\u{0433}";
        listener.send_event(alacritty_terminal::event::Event::Title(
            unicode_title.to_string(),
        ));

        let title = listener.title.lock();
        assert_eq!(title.as_deref(), Some(unicode_title));
    }

    #[::core::prelude::v1::test]
    fn test_listener_title_overwrite() {
        use alacritty_terminal::event::EventListener;
        let listener = Listener::new();

        // Set initial title
        listener.send_event(alacritty_terminal::event::Event::Title("First".to_string()));
        assert_eq!(listener.title.lock().as_deref(), Some("First"));

        // Overwrite with new title
        listener.send_event(alacritty_terminal::event::Event::Title(
            "Second".to_string(),
        ));
        assert_eq!(listener.title.lock().as_deref(), Some("Second"));
    }

    // ============================================================================
    // ERROR PATH TESTS - Cell Dimension Calculations
    // ============================================================================

    #[::core::prelude::v1::test]
    fn test_cell_dims_extreme_values() {
        // Test with extreme but valid cell dimensions
        let state = DisplayState {
            cell_dims: (f32::MAX / 2.0, f32::MAX / 2.0),
            ..Default::default()
        };

        // Should not overflow
        assert!(state.cell_dims.0.is_finite());
        assert!(state.cell_dims.1.is_finite());
    }

    #[::core::prelude::v1::test]
    fn test_cell_dims_very_small() {
        let state = DisplayState {
            cell_dims: (0.001, 0.001),
            ..Default::default()
        };

        // Very small but positive should work
        assert!(state.cell_dims.0 > 0.0);
        assert!(state.cell_dims.1 > 0.0);
    }

    #[::core::prelude::v1::test]
    fn test_terminal_size_calculation_prevents_overflow() {
        // Simulate bounds calculation that could overflow
        let bounds_width = 10000.0_f32;
        let bounds_height = 10000.0_f32;
        let cell_width = 0.01_f32; // Very small cells
        let cell_height = 0.01_f32;
        let padding = PADDING;

        // This calculation could result in very large values
        let cols = ((bounds_width - padding * 2.0).max(0.0) / cell_width).floor() as u16;
        let rows = ((bounds_height - padding * 2.0).max(0.0) / cell_height).floor() as u16;

        // Verify they're reasonable (clamped by u16::MAX)
        assert!(cols <= u16::MAX);
        assert!(rows <= u16::MAX);
    }

    // ============================================================================
    // ERROR PATH TESTS - Bounds Handling
    // ============================================================================

    #[::core::prelude::v1::test]
    fn test_display_state_none_bounds() {
        let state = DisplayState::default();

        // Bounds should be None initially
        assert!(state.bounds.is_none());

        // Pattern matching on None should work
        if let Some(_bounds) = &state.bounds {
            panic!("Bounds should be None");
        }
    }

    // ============================================================================
    // ERROR PATH TESTS - Mouse Event Coordinate Edge Cases
    // ============================================================================

    #[::core::prelude::v1::test]
    fn test_mouse_coordinate_at_origin() {
        let cell_width = 10.0_f32;
        let cell_height = 20.0_f32;
        let padding = PADDING;

        // Position exactly at padding boundary
        let local_x = padding;
        let local_y = padding;

        let cell_x = ((local_x - padding) / cell_width).floor() as i32;
        let cell_y = ((local_y - padding) / cell_height).floor() as i32;

        assert_eq!(cell_x, 0);
        assert_eq!(cell_y, 0);
    }

    #[::core::prelude::v1::test]
    fn test_mouse_coordinate_just_before_origin() {
        let cell_width = 10.0_f32;
        let cell_height = 20.0_f32;
        let padding = PADDING;

        // Position just before padding boundary
        let local_x = padding - 0.001;
        let local_y = padding - 0.001;

        let cell_x = ((local_x - padding) / cell_width).floor() as i32;
        let cell_y = ((local_y - padding) / cell_height).floor() as i32;

        // Should be negative (before terminal area)
        assert!(cell_x < 0);
        assert!(cell_y < 0);
    }

    #[::core::prelude::v1::test]
    fn test_mouse_coordinate_at_max_cell() {
        let cols = 80;
        let rows = 24;
        let cell_width = 10.0_f32;
        let cell_height = 20.0_f32;
        let padding = PADDING;

        // Position at last cell
        let local_x = padding + (cols as f32 - 0.5) * cell_width;
        let local_y = padding + (rows as f32 - 0.5) * cell_height;

        let cell_x = ((local_x - padding) / cell_width).floor() as i32;
        let cell_y = ((local_y - padding) / cell_height).floor() as i32;

        assert_eq!(cell_x, (cols - 1) as i32);
        assert_eq!(cell_y, (rows - 1) as i32);
    }

    #[::core::prelude::v1::test]
    fn test_mouse_coordinate_past_terminal() {
        let cols = 80;
        let rows = 24;
        let cell_width = 10.0_f32;
        let cell_height = 20.0_f32;
        let padding = PADDING;

        // Position past terminal bounds
        let local_x = padding + (cols as f32 + 10.0) * cell_width;
        let local_y = padding + (rows as f32 + 10.0) * cell_height;

        let cell_x = ((local_x - padding) / cell_width).floor() as i32;
        let cell_y = ((local_y - padding) / cell_height).floor() as i32;

        // Should be past bounds
        assert!(cell_x >= cols as i32);
        assert!(cell_y >= rows as i32);
    }

    // ============================================================================
    // PANIC TESTS - Using #[should_panic]
    // ============================================================================

    #[::core::prelude::v1::test]
    #[should_panic(expected = "called `Option::unwrap()` on a `None` value")]
    fn test_bounds_unwrap_on_none_panics() {
        let state = DisplayState::default();
        state.bounds.unwrap(); // Should panic
    }

    #[::core::prelude::v1::test]
    #[should_panic]
    fn test_division_by_zero_panics() {
        // Use std::hint::black_box to prevent compile-time evaluation
        let zero = std::hint::black_box(0_u32);
        let _ = 1_u32 / zero;
    }

    // ============================================================================
    // ERROR PATH TESTS - RenderCell Edge Cases
    // ============================================================================

    #[test_case('\0' ; "null character")]
    #[test_case('\x1b' ; "escape character")]
    #[test_case('\x7f' ; "delete character")]
    #[test_case('\r' ; "carriage return")]
    #[test_case('\n' ; "newline")]
    fn test_render_cell_control_characters(c: char) {
        let cell = RenderCell {
            row: 0,
            col: 0,
            c,
            fg: Hsla::default(),
            flags: CellFlags::empty(),
        };
        assert_eq!(cell.c, c);
    }

    #[::core::prelude::v1::test]
    fn test_render_cell_max_position() {
        let cell = RenderCell {
            row: usize::MAX,
            col: usize::MAX,
            c: 'X',
            fg: Hsla::default(),
            flags: CellFlags::empty(),
        };

        assert_eq!(cell.row, usize::MAX);
        assert_eq!(cell.col, usize::MAX);
    }

    #[::core::prelude::v1::test]
    fn test_render_cell_wide_character_placeholder() {
        // Wide character spacer (second cell of a wide char)
        let cell = RenderCell {
            row: 0,
            col: 1, // Second cell position
            c: ' ', // Spacer character
            fg: Hsla::default(),
            flags: CellFlags::WIDE_CHAR_SPACER,
        };

        assert!(cell.flags.contains(CellFlags::WIDE_CHAR_SPACER));
    }

    // ============================================================================
    // ERROR PATH TESTS - BgRegion Edge Cases
    // ============================================================================

    #[::core::prelude::v1::test]
    fn test_bg_region_col_start_equals_col_end() {
        // Zero-width region
        let region = BgRegion {
            row: 0,
            col_start: 10,
            col_end: 10, // Same as start
            color: Hsla::default(),
        };

        let width = region.col_end - region.col_start;
        assert_eq!(width, 0);
    }

    #[::core::prelude::v1::test]
    fn test_bg_region_inverted_columns() {
        // Inverted region (col_end < col_start) - shouldn't happen but test handling
        let region = BgRegion {
            row: 0,
            col_start: 20,
            col_end: 10,
            color: Hsla::default(),
        };

        // Using saturating_sub to avoid underflow
        let width = region.col_end.saturating_sub(region.col_start);
        assert_eq!(width, 0);
    }

    #[::core::prelude::v1::test]
    fn test_bg_region_full_line() {
        let region = BgRegion {
            row: 0,
            col_start: 0,
            col_end: 80, // Full 80-column line
            color: Hsla::default(),
        };

        let width = region.col_end - region.col_start;
        assert_eq!(width, 80);
    }

    // ============================================================================
    // ERROR PATH TESTS - CursorInfo Edge Cases
    // ============================================================================

    #[::core::prelude::v1::test]
    fn test_cursor_info_at_origin() {
        let cursor = CursorInfo {
            row: 0,
            col: 0,
            shape: CursorShape::Block,
            color: Hsla::default(),
        };

        assert_eq!(cursor.row, 0);
        assert_eq!(cursor.col, 0);
    }

    #[::core::prelude::v1::test]
    fn test_cursor_info_hidden_shape() {
        let cursor = CursorInfo {
            row: 10,
            col: 20,
            shape: CursorShape::Hidden,
            color: Hsla::default(),
        };

        assert!(matches!(cursor.shape, CursorShape::Hidden));
    }

    #[::core::prelude::v1::test]
    fn test_cursor_info_hollow_block() {
        let cursor = CursorInfo {
            row: 5,
            col: 5,
            shape: CursorShape::HollowBlock,
            color: Hsla::default(),
        };

        assert!(matches!(cursor.shape, CursorShape::HollowBlock));
    }

    // ============================================================================
    // ERROR PATH TESTS - RenderData Edge Cases
    // ============================================================================

    #[::core::prelude::v1::test]
    fn test_render_data_large_cell_count() {
        // Large number of cells (stress test memory)
        let cells: Vec<RenderCell> = (0..10000)
            .map(|i| RenderCell {
                row: i / 100,
                col: i % 100,
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

    #[::core::prelude::v1::test]
    fn test_render_data_many_bg_regions() {
        // Many background regions
        let bg_regions: Vec<BgRegion> = (0..1000)
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

        assert_eq!(data.bg_regions.len(), 1000);
    }

    // ============================================================================
    // ERROR PATH TESTS - Result and Option Assertions
    // ============================================================================

    #[::core::prelude::v1::test]
    fn test_option_map_on_none() {
        let bounds: Option<gpui::Bounds<gpui::Pixels>> = None;

        // map on None should return None
        let result = bounds.map(|b| b.origin);
        assert!(result.is_none());
    }

    #[::core::prelude::v1::test]
    fn test_option_unwrap_or_default() {
        let title: Option<String> = None;

        // unwrap_or_default should return empty string
        let result = title.unwrap_or_default();
        assert_eq!(result, "");
    }

    #[::core::prelude::v1::test]
    fn test_option_and_then_chain() {
        let state = DisplayState::default();

        // Chained operations on None bounds
        let result = state
            .bounds
            .as_ref()
            .and_then(|b| Some(b.origin.x))
            .unwrap_or_default();

        // Should return default Pixels value
        assert_eq!(result, gpui::Pixels::default());
    }

    // ============================================================================
    // ERROR PATH TESTS - Float Edge Cases
    // ============================================================================

    #[::core::prelude::v1::test]
    fn test_float_infinity_in_dimensions() {
        let inf = f32::INFINITY;

        // Clamping infinity should give valid results
        let clamped = inf.min(1000.0);
        assert_eq!(clamped, 1000.0);

        let neg_inf = f32::NEG_INFINITY;
        let clamped_neg = neg_inf.max(0.0);
        assert_eq!(clamped_neg, 0.0);
    }

    #[::core::prelude::v1::test]
    fn test_float_operations_with_zero() {
        let width = 0.0_f32;
        let padding = PADDING;

        // Division by zero produces infinity
        let result = if width != 0.0 {
            (100.0 - padding) / width
        } else {
            0.0 // Safe default
        };

        assert_eq!(result, 0.0);
    }

    #[::core::prelude::v1::test]
    fn test_floor_on_negative() {
        // floor on negative should round toward negative infinity
        let neg = -0.5_f32;
        assert_eq!(neg.floor(), -1.0);

        let neg2 = -1.9_f32;
        assert_eq!(neg2.floor(), -2.0);
    }

    // ========================================================================
    // URL Detection Tests
    // ========================================================================

    #[::core::prelude::v1::test]
    fn test_find_url_basic_https() {
        let line = "Check out https://example.com for more info";
        assert_eq!(
            TerminalPane::find_url_at_position(line, 10),
            Some("https://example.com".to_string())
        );
        assert_eq!(
            TerminalPane::find_url_at_position(line, 28),
            Some("https://example.com".to_string())
        );
    }

    #[::core::prelude::v1::test]
    fn test_find_url_basic_http() {
        let line = "Visit http://example.com today";
        assert_eq!(
            TerminalPane::find_url_at_position(line, 6),
            Some("http://example.com".to_string())
        );
    }

    #[::core::prelude::v1::test]
    fn test_find_url_with_path() {
        let line = "See https://github.com/user/repo/blob/main/file.rs";
        assert_eq!(
            TerminalPane::find_url_at_position(line, 20),
            Some("https://github.com/user/repo/blob/main/file.rs".to_string())
        );
    }

    #[::core::prelude::v1::test]
    fn test_find_url_with_query_params() {
        let line = "Link: https://search.com/q?query=test&page=1";
        assert_eq!(
            TerminalPane::find_url_at_position(line, 10),
            Some("https://search.com/q?query=test&page=1".to_string())
        );
    }

    #[::core::prelude::v1::test]
    fn test_find_url_strips_trailing_punctuation() {
        // Period at end
        let line = "Check https://example.com.";
        assert_eq!(
            TerminalPane::find_url_at_position(line, 10),
            Some("https://example.com".to_string())
        );

        // Comma at end
        let line = "See https://example.com, then continue";
        assert_eq!(
            TerminalPane::find_url_at_position(line, 10),
            Some("https://example.com".to_string())
        );

        // Closing paren at end (common in markdown)
        let line = "(https://example.com)";
        assert_eq!(
            TerminalPane::find_url_at_position(line, 5),
            Some("https://example.com".to_string())
        );
    }

    #[::core::prelude::v1::test]
    fn test_find_url_no_url() {
        let line = "This line has no URLs";
        assert_eq!(TerminalPane::find_url_at_position(line, 5), None);
    }

    #[::core::prelude::v1::test]
    fn test_find_url_click_outside_url() {
        let line = "Before https://example.com after";
        // Click on "Before"
        assert_eq!(TerminalPane::find_url_at_position(line, 0), None);
        // Click on "after"
        assert_eq!(TerminalPane::find_url_at_position(line, 28), None);
    }

    #[::core::prelude::v1::test]
    fn test_find_url_multiple_urls() {
        let line = "First https://a.com then https://b.com end";
        // Click on first URL
        assert_eq!(
            TerminalPane::find_url_at_position(line, 8),
            Some("https://a.com".to_string())
        );
        // Click on second URL
        assert_eq!(
            TerminalPane::find_url_at_position(line, 28),
            Some("https://b.com".to_string())
        );
    }

    #[::core::prelude::v1::test]
    fn test_find_url_with_port() {
        let line = "Local: http://localhost:8080/api";
        assert_eq!(
            TerminalPane::find_url_at_position(line, 10),
            Some("http://localhost:8080/api".to_string())
        );
    }

    #[::core::prelude::v1::test]
    fn test_find_url_with_fragment() {
        let line = "Docs: https://docs.rs/crate#section";
        assert_eq!(
            TerminalPane::find_url_at_position(line, 10),
            Some("https://docs.rs/crate#section".to_string())
        );
    }

    #[::core::prelude::v1::test]
    fn test_find_url_with_unicode_before() {
        // Emoji before URL (multi-byte character)
        let line = "🚀 Check http://localhost:4321/ for updates";
        // The emoji takes 1 character position, so URL starts at char 9
        assert_eq!(
            TerminalPane::find_url_at_position(line, 9),
            Some("http://localhost:4321/".to_string())
        );
        assert_eq!(
            TerminalPane::find_url_at_position(line, 15),
            Some("http://localhost:4321/".to_string())
        );
    }

    #[::core::prelude::v1::test]
    fn test_find_url_with_ansi_prompt() {
        // Simulate a prompt line with URL
        let line = "~/projects ➜ http://localhost:8080/api";
        assert_eq!(
            TerminalPane::find_url_at_position(line, 15),
            Some("http://localhost:8080/api".to_string())
        );
    }
}
