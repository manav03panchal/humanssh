use super::*;

impl TerminalPane {
    /// Convert GPUI modifiers to termwiz Modifiers
    fn gpui_mods_to_termwiz(mods: &gpui::Modifiers) -> TermwizMods {
        let mut tm = TermwizMods::NONE;
        if mods.shift {
            tm |= TermwizMods::SHIFT;
        }
        // On macOS, only pass Alt through if OPTION_AS_ALT is enabled
        #[cfg(target_os = "macos")]
        if mods.alt && OPTION_AS_ALT.load(Ordering::Relaxed) {
            tm |= TermwizMods::ALT;
        }
        #[cfg(not(target_os = "macos"))]
        if mods.alt {
            tm |= TermwizMods::ALT;
        }
        if mods.control {
            tm |= TermwizMods::CTRL;
        }
        tm
    }

    /// Convert GPUI key string to termwiz KeyCode
    fn gpui_key_to_termwiz(key: &str) -> Option<KeyCode> {
        match key {
            // Arrow keys
            "up" => Some(KeyCode::UpArrow),
            "down" => Some(KeyCode::DownArrow),
            "left" => Some(KeyCode::LeftArrow),
            "right" => Some(KeyCode::RightArrow),

            // Navigation
            "home" => Some(KeyCode::Home),
            "end" => Some(KeyCode::End),
            "pageup" => Some(KeyCode::PageUp),
            "pagedown" => Some(KeyCode::PageDown),
            "insert" => Some(KeyCode::Insert),
            "delete" => Some(KeyCode::Delete),

            // Special keys
            "tab" => Some(KeyCode::Tab),
            "enter" => Some(KeyCode::Enter),
            "escape" => Some(KeyCode::Escape),
            "backspace" => Some(KeyCode::Backspace),
            "space" => Some(KeyCode::Char(' ')),

            // Function keys
            "f1" => Some(KeyCode::Function(1)),
            "f2" => Some(KeyCode::Function(2)),
            "f3" => Some(KeyCode::Function(3)),
            "f4" => Some(KeyCode::Function(4)),
            "f5" => Some(KeyCode::Function(5)),
            "f6" => Some(KeyCode::Function(6)),
            "f7" => Some(KeyCode::Function(7)),
            "f8" => Some(KeyCode::Function(8)),
            "f9" => Some(KeyCode::Function(9)),
            "f10" => Some(KeyCode::Function(10)),
            "f11" => Some(KeyCode::Function(11)),
            "f12" => Some(KeyCode::Function(12)),

            // Single character
            k if k.len() == 1 => k.chars().next().map(KeyCode::Char),

            _ => None,
        }
    }

    /// Handle a key event using termwiz for escape sequence encoding
    pub(super) fn handle_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let mode = {
            let term = self.term.lock();
            *term.mode()
        };

        let key = event.keystroke.key.as_str();
        let mods = &event.keystroke.modifiers;

        // === Platform shortcuts - handled by the app, not sent to PTY ===
        // On macOS: Cmd key (mods.platform)
        // On Windows/Linux: Ctrl key (mods.control) for common shortcuts
        #[cfg(target_os = "macos")]
        let is_command_key = mods.platform;
        #[cfg(not(target_os = "macos"))]
        let is_command_key = mods.control;

        if is_command_key {
            if mods.shift {
                match key {
                    "left" => {
                        self.handle_cmd_shift_arrow("left", cx);
                        return;
                    }
                    "right" => {
                        self.handle_cmd_shift_arrow("right", cx);
                        return;
                    }
                    _ => return,
                }
            } else {
                match key {
                    "a" => {
                        self.select_all();
                        cx.notify();
                        return;
                    }
                    "c" => {
                        // On macOS, Cmd+C always copies (even if empty)
                        // On Windows/Linux, Ctrl+C copies only if selection exists,
                        // otherwise we need to send SIGINT (handled below)
                        #[cfg(target_os = "macos")]
                        {
                            self.copy_selection(cx);
                            return;
                        }
                        #[cfg(not(target_os = "macos"))]
                        if self.get_selected_text().is_some() {
                            self.copy_selection(cx);
                            return;
                        }
                        // Fall through - Ctrl+C with no selection needs SIGINT
                    }
                    "v" => {
                        self.paste_clipboard(cx);
                        return;
                    }
                    "k" => {
                        self.send_input("\x0c");
                        return;
                    }
                    "backspace" => {
                        self.send_input("\x15");
                        return;
                    }
                    "left" => {
                        self.send_input("\x01");
                        return;
                    }
                    "right" => {
                        self.send_input("\x05");
                        return;
                    }
                    "up" => {
                        self.term.lock().scroll_display(Scroll::Top);
                        cx.notify();
                        return;
                    }
                    "down" => {
                        self.term.lock().scroll_display(Scroll::Bottom);
                        cx.notify();
                        return;
                    }
                    "=" | "+" => {
                        let mut display = self.display.write();
                        display.font_size = (display.font_size + 1.0).min(MAX_FONT_SIZE);
                        cx.notify();
                        return;
                    }
                    "-" => {
                        let mut display = self.display.write();
                        display.font_size = (display.font_size - 1.0).max(MIN_FONT_SIZE);
                        cx.notify();
                        return;
                    }
                    "0" => {
                        self.display.write().font_size = DEFAULT_FONT_SIZE;
                        cx.notify();
                        return;
                    }
                    _ => return,
                }
            }
        }

        // === Selection shortcuts (handled by terminal UI, not sent to PTY) ===
        if mods.alt && mods.shift && !mods.control {
            match key {
                "left" | "right" => {
                    self.handle_option_shift_arrow(key, cx);
                    return;
                }
                _ => {}
            }
        }

        if mods.shift && !mods.alt && !mods.control {
            match key {
                "left" | "right" | "up" | "down" => {
                    self.handle_shift_arrow(key, cx);
                    return;
                }
                _ => {}
            }
        }

        // Clear selection on typing (except for modifier-only keys)
        if !key.is_empty() && key != "shift" && key != "control" && key != "alt" {
            self.term.lock().selection = None;
        }

        // === Use termwiz to encode escape sequences ===
        let app_cursor = mode.contains(TermMode::APP_CURSOR);

        // Build termwiz encoding modes
        let encode_modes = KeyCodeEncodeModes {
            encoding: KeyboardEncoding::Xterm,
            application_cursor_keys: app_cursor,
            newline_mode: false,
            modify_other_keys: None,
        };

        // Convert GPUI key to termwiz KeyCode
        if let Some(keycode) = Self::gpui_key_to_termwiz(key) {
            let termwiz_mods = Self::gpui_mods_to_termwiz(mods);

            // Special case: Alt+Arrow for word movement in shells
            if mods.alt && !mods.control && !mods.shift {
                match key {
                    "left" => {
                        self.send_input("\x1bb"); // backward-word
                        return;
                    }
                    "right" => {
                        self.send_input("\x1bf"); // forward-word
                        return;
                    }
                    _ => {}
                }
            }

            // CSI u encoding for shifted navigation keys
            if mods.shift && !mods.control && !mods.alt {
                match key {
                    "home" => {
                        self.send_input("\x1b[1;2H"); // Shift+Home
                        return;
                    }
                    "end" => {
                        self.send_input("\x1b[1;2F"); // Shift+End
                        return;
                    }
                    _ => {}
                }
            }

            // Use termwiz to encode the key
            if let Ok(seq) = keycode.encode(termwiz_mods, encode_modes, true) {
                if !seq.is_empty() {
                    self.send_input(&seq);
                }
            }
        }
    }

    /// Select all terminal content (visible + scrollback history)
    pub(super) fn select_all(&mut self) {
        let mut term = self.term.lock();
        let cols = term.columns();

        // topmost_line returns negative Line for scrollback history
        // bottommost_line returns the last visible line
        let topmost = term.topmost_line();
        let bottommost = term.bottommost_line();

        // Create selection from top-left to bottom-right
        let start = TermPoint::new(topmost, Column(0));
        let end = TermPoint::new(bottommost, Column(cols.saturating_sub(1)));

        let mut selection = TermSelection::new(SelectionType::Simple, start, Side::Left);
        selection.update(end, Side::Right);
        term.selection = Some(selection);

        // Scroll to bottom to show current content is selected
        term.scroll_display(Scroll::Bottom);
    }

    /// Get selected text from terminal using alacritty's selection
    pub(super) fn get_selected_text(&self) -> Option<String> {
        let term = self.term.lock();
        term.selection_to_string()
    }

    /// Copy selection to clipboard
    pub(super) fn copy_selection(&self, cx: &mut Context<Self>) {
        if let Some(text) = self.get_selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    /// Handle Cmd+Shift+Arrow for line-level selection
    fn handle_cmd_shift_arrow(&mut self, direction: &str, cx: &mut Context<Self>) {
        let mut term = self.term.lock();
        let cols = term.columns();

        let content = term.renderable_content();
        let cursor = content.cursor.point;
        let (start_point, current_end) = if let Some(sel_range) = content.selection {
            let sel_start = TermPoint::new(sel_range.start.line, sel_range.start.column);
            if sel_start == cursor {
                (
                    sel_start,
                    TermPoint::new(sel_range.end.line, sel_range.end.column),
                )
            } else {
                (cursor, cursor)
            }
        } else {
            (cursor, cursor)
        };

        let new_end = match direction {
            "left" => TermPoint::new(current_end.line, Column(0)),
            "right" => TermPoint::new(current_end.line, Column(cols.saturating_sub(1))),
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

        let content = term.renderable_content();
        let cursor = content.cursor.point;
        let (start_point, current_end) = if let Some(sel_range) = content.selection {
            let sel_start = TermPoint::new(sel_range.start.line, sel_range.start.column);
            if sel_start == cursor {
                (
                    sel_start,
                    TermPoint::new(sel_range.end.line, sel_range.end.column),
                )
            } else {
                (cursor, cursor)
            }
        } else {
            (cursor, cursor)
        };

        let topmost = term.topmost_line();
        let bottommost = term.bottommost_line();

        let new_end = match direction {
            "left" => {
                let mut col = current_end.column.0;
                let mut line = current_end.line;

                while col > 0 {
                    col -= 1;
                    if col == 0 {
                        break;
                    }
                }
                while col > 0 {
                    col -= 1;
                }

                if col == 0 && line.0 > topmost.0 {
                    line = Line(line.0 - 1);
                    col = cols.saturating_sub(1);
                }

                TermPoint::new(line, Column(col))
            }
            "right" => {
                let mut col = current_end.column.0;
                let mut line = current_end.line;

                col = (col + 5).min(cols.saturating_sub(1));

                if col >= cols.saturating_sub(1) && line.0 < bottommost.0 {
                    line = Line(line.0 + 1);
                    col = 0;
                }

                TermPoint::new(line, Column(col))
            }
            _ => current_end,
        };

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

        let content = term.renderable_content();
        let cursor = content.cursor.point;
        let (start_point, current_end) = if let Some(sel_range) = content.selection {
            let sel_start = TermPoint::new(sel_range.start.line, sel_range.start.column);
            if sel_start == cursor {
                (
                    sel_start,
                    TermPoint::new(sel_range.end.line, sel_range.end.column),
                )
            } else {
                (cursor, cursor)
            }
        } else {
            (cursor, cursor)
        };

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

        let mut selection = TermSelection::new(SelectionType::Simple, start_point, Side::Left);
        selection.update(new_end, Side::Right);
        term.selection = Some(selection);

        drop(term);
        cx.notify();
    }

    /// Paste from clipboard with bracketed paste mode support.
    pub(super) fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        if let Some(item) = cx.read_from_clipboard() {
            if let Some(text) = item.text() {
                let term_guard = self.term.lock();
                let bracketed_paste = term_guard.mode().contains(TermMode::BRACKETED_PASTE);
                drop(term_guard);

                self.term.lock().selection = None;

                if bracketed_paste {
                    self.send_input("\x1b[200~");
                    self.send_input(&text);
                    self.send_input("\x1b[201~");
                } else {
                    self.send_input(&text);
                }
                cx.notify();
            }
        }
    }

    /// Handle dropped files - pastes file paths for AI assistants to read directly
    pub(super) fn handle_file_drop(&mut self, paths: &ExternalPaths, cx: &mut Context<Self>) {
        let paths = paths.paths();
        if paths.is_empty() {
            return;
        }

        let mut output = String::new();

        for path in paths {
            if !output.is_empty() {
                output.push(' ');
            }
            let path_str = path.to_string_lossy();
            if path_str.contains(' ') {
                output.push('"');
                output.push_str(&path_str);
                output.push('"');
            } else {
                output.push_str(&path_str);
            }
        }

        if !output.is_empty() {
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
}
