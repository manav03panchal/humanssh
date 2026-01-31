//! Terminal pane component using alacritty_terminal.

use super::PtyHandler;
use crate::theme::{terminal_colors, TerminalColors};
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{Color, NamedColor, Processor};
use gpui::*;
use std::sync::{Arc, Mutex};

// Cell dimensions for monospace font at 14px
// These should match the actual rendered glyph size
const CELL_WIDTH: f32 = 8.4;
const CELL_HEIGHT: f32 = 17.0;
const PADDING: f32 = 8.0; // p_2 = 8px padding on each side

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

/// Terminal pane that renders a PTY session
pub struct TerminalPane {
    pty: Option<PtyHandler>,
    term: Arc<Mutex<Term<Listener>>>,
    processor: Arc<Mutex<Processor>>,
    listener: Listener,
    size: TermSize,
    pub focus_handle: FocusHandle,
}

impl TerminalPane {
    pub fn new(cx: &mut Context<Self>) -> Self {
        // Use large defaults to fill most windows
        let cols = 160;
        let rows = 50;

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
            pty,
            term,
            processor,
            listener,
            size,
            focus_handle,
        };

        // Start polling for PTY output
        let term_clone = pane.term.clone();
        let processor_clone = pane.processor.clone();

        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(16))
                    .await;

                let should_notify = this
                    .update(cx, |pane, _cx| {
                        if let Some(ref pty) = pane.pty {
                            let output_chunks = pty.read_output();
                            if !output_chunks.is_empty() {
                                let mut term = term_clone.lock().unwrap();
                                let mut processor = processor_clone.lock().unwrap();
                                for chunk in output_chunks {
                                    processor.advance(&mut *term, &chunk);
                                }
                                return true;
                            }
                        }
                        false
                    })
                    .unwrap_or(false);

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
        if let Some(ref mut pty) = self.pty {
            if let Err(e) = pty.write(input.as_bytes()) {
                tracing::error!("Failed to write to PTY: {}", e);
                // Write failed - mark as exited
                self.pty = None;
            }
        }
    }

    /// Check if the shell has exited
    pub fn has_exited(&self) -> bool {
        match &self.pty {
            None => true,
            Some(pty) => pty.has_exited(),
        }
    }

    /// Get the terminal title (set by OSC escape sequences)
    pub fn title(&self) -> Option<String> {
        self.listener.title.lock().unwrap().clone()
    }

    /// Handle a key event
    fn handle_key(&mut self, event: &KeyDownEvent) {
        // Ignore shortcuts (Cmd+key) - let workspace handle them
        if event.keystroke.modifiers.platform {
            return;
        }

        let mode = {
            let term = self.term.lock().unwrap();
            *term.mode()
        };

        let input = match &event.keystroke.key {
            key if key.len() == 1 => {
                if event.keystroke.modifiers.control {
                    let c = key.chars().next().unwrap();
                    if c.is_ascii_alphabetic() {
                        let ctrl_char = (c.to_ascii_lowercase() as u8 - b'a' + 1) as char;
                        ctrl_char.to_string()
                    } else {
                        key.clone()
                    }
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
                    "escape" => "\x1b".to_string(),
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

}

/// Convert alacritty color to GPUI Hsla using theme colors
fn color_to_hsla(color: Color, colors: &TerminalColors) -> Hsla {
    match color {
        Color::Named(named) => named_color_to_hsla(named, colors),
        Color::Spec(rgb) => Hsla::from(Rgba {
            r: rgb.r as f32 / 255.0,
            g: rgb.g as f32 / 255.0,
            b: rgb.b as f32 / 255.0,
            a: 1.0,
        }),
        Color::Indexed(idx) => indexed_color_to_hsla(idx, colors),
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

/// A segment of text with uniform styling
struct TextSegment {
    text: String,
    fg: Hsla,
    bg: Hsla,
}

impl Render for TerminalPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus_handle = self.focus_handle.clone();

        // Get actual viewport size and calculate terminal dimensions
        let viewport = window.viewport_size();
        let vp_width: f32 = viewport.width.into();
        let vp_height: f32 = viewport.height.into();

        // Calculate max possible cols/rows for the viewport
        let max_cols = ((vp_width - PADDING * 2.0) / CELL_WIDTH).floor() as u16;
        let max_rows = ((vp_height - PADDING * 2.0 - 40.0) / CELL_HEIGHT).floor() as u16; // 40 for tab bar

        // Update terminal size if needed
        if max_cols != self.size.cols || max_rows != self.size.rows {
            let cols = max_cols.max(10);
            let rows = max_rows.max(3);

            if cols != self.size.cols || rows != self.size.rows {
                self.size = TermSize { cols, rows };

                // Resize PTY
                if let Some(ref pty) = self.pty {
                    let _ = pty.resize(rows, cols);
                }

                // Resize terminal emulator
                let mut term = self.term.lock().unwrap();
                term.resize(self.size);
            }
        }

        let cols = self.size.cols as usize;
        let rows = self.size.rows as usize;

        // Get theme colors
        let colors = terminal_colors(cx);

        // Get terminal content
        let term = self.term.lock().unwrap();
        let content = term.renderable_content();

        let default_fg = colors.foreground;
        let default_bg = colors.background;

        // Collect cells into lines
        let mut lines: Vec<Vec<(char, Hsla, Hsla)>> =
            vec![vec![(' ', default_fg, default_bg); cols]; rows];

        for cell in content.display_iter {
            let point = cell.point;
            let row = point.line.0 as usize;
            let col = point.column.0;

            if row < rows && col < cols {
                let c = cell.c;
                let fg = color_to_hsla(cell.fg, &colors);
                let bg = color_to_hsla(cell.bg, &colors);
                lines[row][col] = (c, fg, bg);
            }
        }

        // Get cursor position
        let cursor = content.cursor;
        let cursor_row = cursor.point.line.0 as usize;
        let cursor_col = cursor.point.column.0;

        drop(term);

        // Cursor colors from theme
        let cursor_bg = colors.cursor;
        let cursor_fg = colors.background;

        // Convert lines to segments for efficient rendering
        let rendered_lines: Vec<Vec<TextSegment>> = lines
            .into_iter()
            .enumerate()
            .map(|(row_idx, line)| {
                let mut segments: Vec<TextSegment> = Vec::new();
                let mut current_text = String::new();
                let mut current_fg = default_fg;
                let mut current_bg = default_bg;

                for (col_idx, (c, mut fg, mut bg)) in line.into_iter().enumerate() {
                    // Apply cursor styling
                    if row_idx == cursor_row && col_idx == cursor_col {
                        fg = cursor_fg;
                        bg = cursor_bg;
                    }

                    // If colors changed, flush the current segment
                    if fg != current_fg || bg != current_bg {
                        if !current_text.is_empty() {
                            segments.push(TextSegment {
                                text: current_text,
                                fg: current_fg,
                                bg: current_bg,
                            });
                            current_text = String::new();
                        }
                        current_fg = fg;
                        current_bg = bg;
                    }

                    current_text.push(c);
                }

                // Flush remaining text
                if !current_text.is_empty() {
                    segments.push(TextSegment {
                        text: current_text,
                        fg: current_fg,
                        bg: current_bg,
                    });
                }

                segments
            })
            .collect();

        let bg_color = colors.background;

        div()
            .id("terminal-pane")
            .key_context("terminal")
            .track_focus(&focus_handle)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                // Handle Cmd+, for settings - open dialog directly
                if event.keystroke.modifiers.platform && event.keystroke.key.as_str() == "," {
                    crate::app::open_settings_dialog(window, cx);
                    return;
                }
                this.handle_key(event);
            }))
            .on_click(cx.listener(|_this, _event: &ClickEvent, window, cx| {
                window.focus(&cx.focus_handle());
            }))
            .size_full()
            .bg(bg_color)
            .p_2()
            .overflow_hidden()
            .font_family("Iosevka Nerd Font")
            .text_size(px(14.0))
            .child(
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .children(rendered_lines.into_iter().enumerate().map(|(row_idx, segments)| {
                        div()
                            .id(ElementId::Integer(row_idx as u64))
                            .h(px(CELL_HEIGHT))
                            .w_full()
                            .flex()
                            .whitespace_nowrap()
                            .children(segments.into_iter().map(|seg| {
                                div()
                                    .bg(seg.bg)
                                    .text_color(seg.fg)
                                    .child(seg.text)
                            }))
                    }))
                    // Fill remaining space with background
                    .child(div().flex_1().w_full().bg(bg_color)),
            )
    }
}

impl Focusable for TerminalPane {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
