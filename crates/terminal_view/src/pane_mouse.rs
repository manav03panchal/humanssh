use super::*;

impl TerminalPane {
    /// Convert pixel position (window coords) to terminal cell coordinates
    pub(super) fn pixel_to_cell(&self, position: Point<Pixels>) -> Option<(usize, usize)> {
        let display = self.display.read();
        let bounds = display.bounds.as_ref()?;

        let origin_x: f32 = bounds.origin.x.into();
        let origin_y: f32 = bounds.origin.y.into();
        let x: f32 = position.x.into();
        let y: f32 = position.y.into();

        let local_x = x - origin_x;
        let local_y = y - origin_y;

        let (cell_width, cell_height) = display.cell_dims;

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

    /// Find the column span (start_col, end_col) of the URL at a given column position.
    pub(super) fn find_url_span_at_position(line: &str, col: usize) -> Option<(usize, usize)> {
        const URL_CHARS: &str =
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~:/?#[]@!$&'()*+,;=%";

        let chars: Vec<char> = line.chars().collect();
        let line_len = chars.len();
        let prefix_chars_https: Vec<char> = "https://".chars().collect();
        let prefix_chars_http: Vec<char> = "http://".chars().collect();

        for prefix_chars in [&prefix_chars_https, &prefix_chars_http] {
            let prefix_len = prefix_chars.len();

            let mut search_start = 0;
            while search_start + prefix_len <= line_len {
                let url_start = (search_start..=line_len - prefix_len).find(|&i| {
                    chars[i..i + prefix_len]
                        .iter()
                        .zip(prefix_chars.iter())
                        .all(|(a, b)| a == b)
                });

                let Some(url_start) = url_start else {
                    break;
                };

                let mut url_end = url_start + prefix_len;
                while url_end < line_len && URL_CHARS.contains(chars[url_end]) {
                    url_end += 1;
                }

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

                if col >= url_start && col < url_end {
                    return Some((url_start, url_end));
                }

                search_start = url_end;
            }
        }
        None
    }

    /// Extract URL at the given column position from a line of text.
    pub(super) fn find_url_at_position(line: &str, col: usize) -> Option<String> {
        let (start, end) = Self::find_url_span_at_position(line, col)?;
        Some(line.chars().skip(start).take(end - start).collect())
    }

    /// Extract text content from a visual terminal row (accounting for scroll).
    pub(super) fn get_row_text(&self, visual_row: usize) -> String {
        let term = self.term.lock();
        let grid = term.grid();
        let display_offset = grid.display_offset() as i32;

        let line = Line(visual_row as i32 - display_offset);

        let total_lines = grid.total_lines();
        let screen_lines = grid.screen_lines() as i32;
        let min_line = -(total_lines as i32 - screen_lines);

        if line.0 < min_line || line.0 >= screen_lines {
            return String::new();
        }

        let cols = grid.columns();
        let row_data = &grid[line];
        (0..cols).map(|c| row_data[Column(c)].c).collect()
    }

    /// Handle mouse down event
    pub(super) fn handle_mouse_down(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let Some((col, row)) = self.pixel_to_cell(event.position) else {
            return;
        };

        if event.modifiers.platform && event.button == MouseButton::Left {
            let line_text = self.get_row_text(row);
            if let Some(url) = Self::find_url_at_position(&line_text, col) {
                #[allow(clippy::disallowed_methods)]
                #[cfg(target_os = "macos")]
                {
                    let _ = std::process::Command::new("open").arg(&url).spawn();
                }
                #[allow(clippy::disallowed_methods)]
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

        if mode.intersects(
            TermMode::MOUSE_REPORT_CLICK
                | TermMode::MOUSE_DRAG
                | TermMode::MOUSE_MOTION
                | TermMode::MOUSE_MODE,
        ) {
            let button = match event.button {
                MouseButton::Left => 0,
                MouseButton::Middle => 1,
                MouseButton::Right => 2,
                _ => return,
            };

            let seq = Self::encode_mouse_event(
                button,
                col,
                row,
                mode.contains(TermMode::SGR_MOUSE),
                false,
            );
            self.send_input(seq.as_str());
        } else if event.button == MouseButton::Left {
            let mut term = self.term.lock();
            let display_offset = term.grid().display_offset() as i32;
            let line = Line(row as i32 - display_offset);
            let point = TermPoint::new(line, Column(col));

            let selection_type = if event.modifiers.alt {
                SelectionType::Block
            } else {
                SelectionType::Simple
            };
            let selection = TermSelection::new(selection_type, point, Side::Left);
            term.selection = Some(selection);
            drop(term);
            self.dragging = true;
            cx.notify();
        }
    }

    /// Handle mouse up event
    pub(super) fn handle_mouse_up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
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
            let button = match event.button {
                MouseButton::Left => 0,
                MouseButton::Middle => 1,
                MouseButton::Right => 2,
                _ => return,
            };
            let seq = Self::encode_mouse_event(
                button,
                col,
                row,
                mode.contains(TermMode::SGR_MOUSE),
                true,
            );
            self.send_input(seq.as_str());
        } else if event.button == MouseButton::Left {
            if self.dragging {
                let mut term = self.term.lock();
                let display_offset = term.grid().display_offset() as i32;
                let line = Line(row as i32 - display_offset);
                let point = TermPoint::new(line, Column(col));
                if let Some(ref mut selection) = term.selection {
                    selection.update(point, Side::Right);
                }
                drop(term);
                self.dragging = false;
                cx.notify();
            }
        }
    }

    /// Handle mouse move/drag event
    pub(super) fn handle_mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let new_hover = if event.modifiers.platform {
            self.pixel_to_cell(event.position).and_then(|(col, row)| {
                let line_text = self.get_row_text(row);
                Self::find_url_span_at_position(&line_text, col)
                    .map(|(start, end)| (row, start, end))
            })
        } else {
            None
        };
        if new_hover != self.hovered_url {
            self.hovered_url = new_hover;
            cx.notify();
        }

        let Some((col, row)) = self.pixel_to_cell(event.position) else {
            return;
        };

        let mode = {
            let term = self.term.lock();
            *term.mode()
        };

        if self.dragging
            && mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION | TermMode::MOUSE_MODE)
        {
            let seq = Self::encode_mouse_event(
                32, // left button drag
                col,
                row,
                mode.contains(TermMode::SGR_MOUSE),
                false,
            );
            self.send_input(seq.as_str());
        } else if mode.contains(TermMode::MOUSE_MOTION) && !self.dragging {
            let seq = Self::encode_mouse_event(
                35, // no button (motion only)
                col,
                row,
                mode.contains(TermMode::SGR_MOUSE),
                false,
            );
            self.send_input(seq.as_str());
        } else if self.dragging {
            let mut term = self.term.lock();
            let display_offset = term.grid().display_offset() as i32;
            let line = Line(row as i32 - display_offset);
            let point = TermPoint::new(line, Column(col));
            if let Some(ref mut selection) = term.selection {
                selection.update(point, Side::Right);
            }
            cx.notify();
        }
    }

    /// Handle scroll event
    pub(super) fn handle_scroll(&mut self, event: &ScrollWheelEvent) {
        let Some((col, row)) = self.pixel_to_cell(event.position) else {
            return;
        };

        let mode = {
            let term = self.term.lock();
            *term.mode()
        };

        let (_, cell_height) = self.display.read().cell_dims;

        let raw_delta_y: f32 = event.delta.pixel_delta(px(cell_height)).y.into();
        let delta_y = if self.scroll_reverse {
            -raw_delta_y
        } else {
            raw_delta_y
        };

        if mode.intersects(
            TermMode::MOUSE_REPORT_CLICK
                | TermMode::MOUSE_DRAG
                | TermMode::MOUSE_MOTION
                | TermMode::MOUSE_MODE,
        ) {
            let button = if delta_y < 0.0 { 64 } else { 65 };
            let seq = Self::encode_mouse_event(
                button,
                col,
                row,
                mode.contains(TermMode::SGR_MOUSE),
                false,
            );
            self.send_input(seq.as_str());
        } else if mode.contains(TermMode::ALT_SCREEN) {
            let lines = (delta_y.abs() / cell_height).ceil() as usize;
            let key = if delta_y < 0.0 { "\x1b[A" } else { "\x1b[B" };

            for _ in 0..lines.min(5) {
                self.send_input(key);
            }
        } else {
            let lines = (delta_y.abs() / cell_height).ceil() as i32;

            if lines > 0 {
                let scroll = if delta_y < 0.0 {
                    Scroll::Delta(lines)
                } else {
                    Scroll::Delta(-lines)
                };
                self.term.lock().scroll_display(scroll);
            }
        }
    }

    /// Encode a mouse event for the PTY (SGR or legacy X11 format).
    pub(super) fn encode_mouse_event(
        button: u8,
        col: usize,
        row: usize,
        sgr_mode: bool,
        release: bool,
    ) -> MouseEscBuf {
        let mut buf = MouseEscBuf::new();
        if sgr_mode {
            let terminator = if release { 'm' } else { 'M' };
            let _ = write!(
                buf,
                "\x1b[<{};{};{}{}",
                button,
                col.saturating_add(1),
                row.saturating_add(1),
                terminator
            );
        } else {
            let cb: u8 = if release {
                35
            } else {
                button.saturating_add(32)
            };
            let cx = (col.min(222) as u8).saturating_add(33);
            let cy = (row.min(222) as u8).saturating_add(33);
            let _ = write!(buf, "\x1b[M{}{}{}", cb as char, cx as char, cy as char);
        }
        buf
    }
}
