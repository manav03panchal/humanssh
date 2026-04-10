use super::*;

/// Build render data from terminal state - collects individual cells for precise positioning
pub(super) fn build_render_data(
    term: &Term<Listener>,
    theme: &TerminalColors,
    _font_family: SharedString,
) -> RenderData {
    let content = term.renderable_content();
    let term_colors = content.colors;
    let default_bg = theme.background;

    let term_cols = term.columns();
    let term_rows = term.screen_lines();

    let estimated_cells = (term_rows * term_cols) / 3;
    let estimated_bg_regions = term_rows * 2;
    let mut cells: Vec<RenderCell> = Vec::with_capacity(estimated_cells);
    let mut bg_regions: Vec<BgRegion> = Vec::with_capacity(estimated_bg_regions);

    let mut current_bg: Option<(usize, usize, usize, Hsla)> = None;

    let cursor_line = content.cursor.point.line.0;
    let cursor_col = content.cursor.point.column.0;
    let cursor_shape = content.cursor.shape;
    let display_offset = content.display_offset as i32;

    let cursor_visual_row = cursor_line + display_offset;
    let cursor_info = if cursor_visual_row >= 0
        && (cursor_visual_row as usize) < term_rows
        && cursor_col < term_cols
    {
        Some(CursorInfo {
            row: cursor_visual_row as usize,
            col: cursor_col,
            shape: cursor_shape,
            color: theme.cursor,
        })
    } else {
        None
    };

    for cell in content.display_iter {
        let point = cell.point;
        let row = (point.line.0 + display_offset) as usize;
        let col = point.column.0;

        if row >= term_rows || col >= term_cols {
            continue;
        }

        let flags = cell.flags;

        if flags.contains(CellFlags::WIDE_CHAR_SPACER) {
            continue;
        }

        let mut fg = color_to_hsla(cell.fg, term_colors, theme);
        let mut bg = color_to_hsla(cell.bg, term_colors, theme);

        if flags.contains(CellFlags::BOLD) {
            fg = get_bright_color(cell.fg, term_colors, theme);
        }

        if flags.contains(CellFlags::DIM) {
            fg = apply_dim(fg);
        }

        if flags.contains(CellFlags::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }

        if flags.contains(CellFlags::HIDDEN) {
            fg = bg;
        }

        let _is_cursor = cursor_info.is_some_and(|c| c.row == row && c.col == col);

        if bg != default_bg {
            match &mut current_bg {
                Some((cur_row, _start, end, color))
                    if *cur_row == row && *end == col && *color == bg =>
                {
                    *end = col + 1;
                }
                Some((cur_row, start, end, color)) => {
                    bg_regions.push(BgRegion {
                        row: *cur_row,
                        col_start: *start,
                        col_end: *end,
                        color: *color,
                    });
                    current_bg = Some((row, col, col + 1, bg));
                }
                None => {
                    current_bg = Some((row, col, col + 1, bg));
                }
            }
        } else if let Some((cur_row, start, end, color)) = current_bg.take() {
            bg_regions.push(BgRegion {
                row: cur_row,
                col_start: start,
                col_end: end,
                color,
            });
        }

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

pub(super) type CanvasPaintData = (
    RenderData,
    Bounds<Pixels>,
    f32,
    f32,
    Option<alacritty_terminal::selection::SelectionRange>,
    usize,
    usize,
    Hsla,
    SharedString,
    f32,
    i32,
    Vec<(i32, usize, usize)>,
    usize,
    Option<(usize, usize, usize)>,
    Option<FontFallbacks>,
    ProgressState,
);

pub(super) fn paint_terminal_canvas(
    _bounds: Bounds<Pixels>,
    data: CanvasPaintData,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    let (
        render_data,
        bounds,
        cell_width,
        cell_height,
        selection_range,
        cols,
        rows,
        selection_color,
        font_family,
        font_size,
        display_offset,
        search_matches,
        search_current,
        hovered_url,
        font_fallbacks,
        progress_state,
    ) = data;

    let origin = bounds.origin;
    let line_height = px(cell_height);

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

    if let Some(sel) = selection_range {
        let start_same_as_end =
            sel.start.line == sel.end.line && sel.start.column == sel.end.column;

        if !start_same_as_end {
            let start_line = sel.start.line.0;
            let end_line = sel.end.line.0;

            let start_visual = start_line + display_offset;
            let end_visual = end_line + display_offset;

            let visible_start_row = start_visual.max(0) as usize;
            let visible_end_row = (end_visual.max(0) as usize).min(rows.saturating_sub(1));

            if visible_start_row <= visible_end_row
                && end_visual >= 0
                && start_visual < rows as i32
            {
                let start_col = sel.start.column.0;
                let end_col = sel.end.column.0;

                for row in visible_start_row..=visible_end_row {
                    let (col_start, col_end) = if sel.is_block {
                        (start_col, end_col + 1)
                    } else {
                        let cs = if row == visible_start_row && start_visual >= 0 {
                            start_col
                        } else {
                            0
                        };
                        let ce = if row == visible_end_row && end_visual == row as i32 {
                            end_col + 1
                        } else {
                            cols
                        };
                        (cs, ce)
                    };

                    let x = origin.x + px(PADDING + col_start as f32 * cell_width);
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

    if !search_matches.is_empty() {
        for (idx, &(match_line, start_col, end_col)) in search_matches.iter().enumerate() {
            let visual_row = match_line + display_offset;
            if visual_row < 0 || visual_row >= rows as i32 {
                continue;
            }
            let row = visual_row as usize;
            let x = origin.x + px(PADDING + start_col as f32 * cell_width);
            let y = origin.y + px(PADDING + row as f32 * cell_height);
            let w = (end_col - start_col) as f32 * cell_width;

            let highlight_color = if idx == search_current {
                hsla(0.14, 0.9, 0.5, 0.6)
            } else {
                hsla(0.14, 0.9, 0.5, 0.25)
            };

            window.paint_quad(fill(
                Bounds::new(
                    Point::new(x, y),
                    Size {
                        width: px(w),
                        height: px(cell_height),
                    },
                ),
                highlight_color,
            ));
        }
    }

    if let Some((hover_row, hover_start, hover_end)) = hovered_url {
        if hover_row < rows {
            let x = origin.x + px(PADDING + hover_start as f32 * cell_width);
            let y =
                origin.y + px(PADDING + (hover_row as f32 + 1.0) * cell_height - 1.0);
            let w = (hover_end - hover_start) as f32 * cell_width;
            window.paint_quad(fill(
                Bounds::new(
                    Point::new(x, y),
                    Size {
                        width: px(w),
                        height: px(1.0),
                    },
                ),
                hsla(0.58, 0.7, 0.65, 0.9),
            ));
        }
    }

    let font_size_px = px(font_size);

    let font_normal = Font {
        family: font_family.clone(),
        features: ligature_features(),
        fallbacks: font_fallbacks.clone(),
        weight: FontWeight::NORMAL,
        style: FontStyle::Normal,
    };
    let font_bold = Font {
        family: font_family.clone(),
        features: ligature_features(),
        fallbacks: font_fallbacks.clone(),
        weight: FontWeight::BOLD,
        style: FontStyle::Normal,
    };
    let font_italic = Font {
        family: font_family.clone(),
        features: ligature_features(),
        fallbacks: font_fallbacks.clone(),
        weight: FontWeight::NORMAL,
        style: FontStyle::Italic,
    };
    let font_bold_italic = Font {
        family: font_family.clone(),
        features: ligature_features(),
        fallbacks: font_fallbacks,
        weight: FontWeight::BOLD,
        style: FontStyle::Italic,
    };

    let pick_font = |flags: CellFlags| -> Font {
        match (
            flags.contains(CellFlags::BOLD),
            flags.contains(CellFlags::ITALIC),
        ) {
            (false, false) => font_normal.clone(),
            (true, false) => font_bold.clone(),
            (false, true) => font_italic.clone(),
            (true, true) => font_bold_italic.clone(),
        }
    };

    let mut run_text = String::with_capacity(32);
    let cells = &render_data.cells;
    let mut i = 0;

    while i < cells.len() {
        let start = &cells[i];
        let run_row = start.row;
        let run_col = start.col;
        let run_fg = start.fg;
        let run_flags = start.flags;

        run_text.clear();
        run_text.push(start.c);
        let mut run_end_col = run_col;
        i += 1;

        while i < cells.len() {
            let cell = &cells[i];
            let expected_next = if cells[i - 1].flags.contains(CellFlags::WIDE_CHAR) {
                run_end_col + 2
            } else {
                run_end_col + 1
            };
            if cell.row != run_row
                || cell.col != expected_next
                || cell.fg != run_fg
                || cell.flags.intersection(CellFlags::BOLD | CellFlags::ITALIC)
                    != run_flags.intersection(CellFlags::BOLD | CellFlags::ITALIC)
            {
                break;
            }
            run_text.push(cell.c);
            run_end_col = cell.col;
            i += 1;
        }

        let text: SharedString = run_text.clone().into();
        let font = pick_font(run_flags);
        let run = TextRun {
            len: text.len(),
            font,
            color: run_fg,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = {
            let text_system = window.text_system();
            text_system.shape_line(text, font_size_px, &[run], None)
        };
        let x = origin.x + px(PADDING + run_col as f32 * cell_width);
        let y = origin.y + px(PADDING + run_row as f32 * cell_height);
        let _ = shaped.paint(Point::new(x, y), line_height, window, cx);
    }

    if let Some(cursor) = render_data.cursor {
        let cursor_x = origin.x + px(PADDING + cursor.col as f32 * cell_width);
        let cursor_y = origin.y + px(PADDING + cursor.row as f32 * cell_height);

        match cursor.shape {
            CursorShape::Block => {
                let thickness = px(2.0);
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
                window.paint_quad(fill(
                    Bounds::new(
                        Point::new(cursor_x, cursor_y + px(cell_height) - thickness),
                        Size {
                            width: px(cell_width),
                            height: thickness,
                        },
                    ),
                    cursor.color,
                ));
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
                window.paint_quad(fill(
                    Bounds::new(
                        Point::new(cursor_x + px(cell_width) - thickness, cursor_y),
                        Size {
                            width: thickness,
                            height: px(cell_height),
                        },
                    ),
                    cursor.color,
                ));
            }
            CursorShape::HollowBlock => {
                let thickness = px(1.0);
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
                window.paint_quad(fill(
                    Bounds::new(
                        Point::new(cursor_x, cursor_y + px(cell_height) - thickness),
                        Size {
                            width: px(cell_width),
                            height: thickness,
                        },
                    ),
                    cursor.color,
                ));
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
                window.paint_quad(fill(
                    Bounds::new(
                        Point::new(cursor_x + px(cell_width) - thickness, cursor_y),
                        Size {
                            width: thickness,
                            height: px(cell_height),
                        },
                    ),
                    cursor.color,
                ));
            }
            CursorShape::Beam => {
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
                window.paint_quad(fill(
                    Bounds::new(
                        Point::new(cursor_x, cursor_y + px(cell_height) - px(2.0)),
                        Size {
                            width: px(cell_width),
                            height: px(2.0),
                        },
                    ),
                    cursor.color,
                ));
            }
            CursorShape::Hidden => {}
        }
    }

    if progress_state.is_visible() {
        let bar_height = px(3.0);
        let bar_y = bounds.origin.y + bounds.size.height - bar_height;
        let total_width: f32 = bounds.size.width.into();

        let (bar_color, bar_width) = match progress_state {
            ProgressState::Normal(pct) => {
                let color = hsla(0.33, 0.8, 0.45, 1.0);
                let width = total_width * (pct as f32 / 100.0);
                (color, width)
            }
            ProgressState::Error(pct) => {
                let color = hsla(0.0, 0.8, 0.45, 1.0);
                let width = total_width * (pct as f32 / 100.0);
                (color, width)
            }
            ProgressState::Paused(pct) => {
                let color = hsla(0.13, 0.8, 0.50, 1.0);
                let width = total_width * (pct as f32 / 100.0);
                (color, width)
            }
            ProgressState::Indeterminate => {
                let color = hsla(0.33, 0.5, 0.35, 0.6);
                (color, total_width)
            }
            ProgressState::Hidden => unreachable!(),
        };

        if bar_width > 0.0 {
            window.paint_quad(fill(
                Bounds::new(
                    Point::new(bounds.origin.x, bar_y),
                    Size {
                        width: px(bar_width),
                        height: bar_height,
                    },
                ),
                bar_color,
            ));
        }
    }
}

impl Render for TerminalPane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus_handle = self.focus_handle.clone();

        let colors = terminal_colors(cx);
        let display_state = self.display.read();
        tracing::trace!(
            rows = display_state.size.rows,
            cols = display_state.size.cols,
            "Terminal render"
        );
        let bg_color = colors.background;

        let font_family: SharedString = cx.theme().font_family.clone();

        let current_font_size = display_state.font_size;
        let font_size_bits = current_font_size.to_bits();
        let needs_recalc = match &display_state.cached_font_key {
            Some((cached_bits, ref cached_family)) => {
                *cached_bits != font_size_bits || *cached_family != font_family
            }
            None => true,
        };
        drop(display_state);

        if needs_recalc {
            let dims = calculate_cell_dimensions(
                window,
                current_font_size,
                &font_family,
                &self.font_fallbacks,
            );
            let mut display = self.display.write();
            display.cell_dims = dims;
            display.cached_font_key = Some((font_size_bits, font_family.clone()));
        }

        let term = self.term.clone();
        let pty = self.pty.clone();
        let display_arc = self.display.clone();
        let colors_clone = colors;
        let font_family_clone = font_family.clone();
        let font_fallbacks_clone = self.font_fallbacks.clone();

        let progress_state = self.progress;
        let show_pointer = self.hovered_url.is_some();

        div()
            .id("terminal-pane")
            .key_context("terminal")
            .track_focus(&focus_handle)
            .when(show_pointer, |d| d.cursor_pointer())
            .on_action(cx.listener(|this, _: &SendTab, _window, _cx| {
                this.send_input("\t");
            }))
            .on_action(cx.listener(|this, _: &SendShiftTab, _window, _cx| {
                this.send_input("\x1b[Z");
            }))
            .on_action(cx.listener(|this, _: &SearchToggle, _window, cx| {
                this.toggle_search(cx);
            }))
            .on_action(cx.listener(|this, _: &SearchNext, _window, cx| {
                this.search_next(cx);
            }))
            .on_action(cx.listener(|this, _: &SearchPrev, _window, cx| {
                this.search_prev(cx);
            }))
            .on_action(cx.listener(|this, _: &SearchToggleRegex, _window, cx| {
                this.toggle_regex(cx);
            }))
            .on_action(cx.listener(|this, _: &EnterCopyMode, _window, cx| {
                this.enter_copy_mode(cx);
            }))
            .on_action(cx.listener(|this, _: &ExitCopyMode, _window, cx| {
                this.exit_copy_mode(cx);
            }))
            .on_action(cx.listener(|this, _: &StartRecording, _window, cx| {
                if let Err(error) = this.start_recording() {
                    tracing::error!("Failed to start recording: {}", error);
                }
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &StopRecording, _window, cx| {
                this.stop_recording();
                cx.notify();
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _window, cx| {
                if this.replay.is_some() {
                    let key = event.keystroke.key.as_str();
                    match key {
                        "space" => {
                            this.toggle_replay_playback();
                            cx.notify();
                            return;
                        }
                        "+" | "=" => {
                            if let Some(r) = this.replay.as_ref() {
                                let new_speed = (r.speed * 2.0).min(8.0);
                                this.set_replay_speed(new_speed);
                            }
                            cx.notify();
                            return;
                        }
                        "-" => {
                            if let Some(r) = this.replay.as_ref() {
                                let new_speed = (r.speed / 2.0).max(0.25);
                                this.set_replay_speed(new_speed);
                            }
                            cx.notify();
                            return;
                        }
                        _ => return,
                    }
                }

                if this.copy_mode.active {
                    this.handle_copy_mode_key(event, cx);
                    return;
                }

                if this.search.active {
                    let key = event.keystroke.key.as_str();
                    let mods = &event.keystroke.modifiers;

                    match key {
                        "escape" => {
                            this.toggle_search(cx);
                            return;
                        }
                        "enter" => {
                            if mods.shift {
                                this.search_prev(cx);
                            } else {
                                this.search_next(cx);
                            }
                            return;
                        }
                        "backspace" => {
                            this.search.query.pop();
                            this.find_matches();
                            cx.notify();
                            return;
                        }
                        _ => {}
                    }
                    if key.len() == 1 && !mods.control && !mods.alt && !mods.platform {
                        let ch = if mods.shift {
                            key.to_uppercase()
                        } else {
                            key.to_string()
                        };
                        this.search.query.push_str(&ch);
                        this.find_matches();
                        cx.notify();
                        return;
                    }
                    if key == "space" {
                        this.search.query.push(' ');
                        this.find_matches();
                        cx.notify();
                        return;
                    }
                    return;
                }

                let key = event.keystroke.key.as_str();
                let mods = &event.keystroke.modifiers;

                match key {
                    "tab" => {
                        if mods.shift {
                            this.send_input("\x1b[Z");
                        } else {
                            this.send_input("\t");
                        }
                        return;
                    }
                    "escape" => {
                        this.send_input("\x1b");
                        return;
                    }
                    "enter" => {
                        if mods.shift {
                            this.send_input("\x1b[13;2u");
                        } else if !mods.control && !mods.alt {
                            this.send_input("\r");
                        } else {
                            this.handle_key(event, cx);
                        }
                        return;
                    }
                    "backspace" if mods.shift && !mods.control && !mods.alt && !mods.platform => {
                        this.send_input("\x7f");
                        return;
                    }
                    "space" if mods.shift && !mods.control && !mods.alt && !mods.platform => {
                        this.send_input("\x1b[32;2u");
                        return;
                    }
                    "space" if !mods.control && !mods.alt && !mods.platform => {
                        this.send_input(" ");
                        return;
                    }
                    _ => {}
                }

                if mods.platform && key == "," {
                    if let Some(path) = settings::config_path() {
                        #[allow(clippy::disallowed_methods)]
                        #[cfg(target_os = "macos")]
                        {
                            let editor =
                                std::env::var("EDITOR").unwrap_or_else(|_| "open -t".to_string());
                            let parts: Vec<&str> = editor.split_whitespace().collect();
                            if let Some((cmd, args)) = parts.split_first() {
                                let mut command = std::process::Command::new(cmd);
                                command.args(args);
                                command.arg(&path);
                                let _ = command.spawn();
                            }
                        }
                        #[allow(clippy::disallowed_methods)]
                        #[cfg(not(target_os = "macos"))]
                        {
                            let editor =
                                std::env::var("EDITOR").unwrap_or_else(|_| "xdg-open".to_string());
                            let _ = std::process::Command::new(&editor).arg(&path).spawn();
                        }
                    }
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
                cx.notify();
            }))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _window, cx| {
                this.handle_file_drop(paths, cx);
            }))
            .size_full()
            .bg(bg_color)
            .when(self.search.active, |d| {
                let match_info = if self.search.matches.is_empty() {
                    "No results".to_string()
                } else {
                    format!(
                        "{}/{}",
                        self.search.current_match + 1,
                        self.search.matches.len()
                    )
                };
                let query_display = if self.search.query.is_empty() {
                    if self.search.regex_mode {
                        "Regex...".to_string()
                    } else {
                        "Search...".to_string()
                    }
                } else {
                    self.search.query.clone()
                };
                let regex_mode = self.search.regex_mode;
                let regex_color = if regex_mode {
                    hsla(0.55, 0.6, 0.65, 1.0)
                } else {
                    hsla(0.0, 0.0, 0.4, 1.0)
                };
                d.child(
                    div()
                        .id("search-bar")
                        .absolute()
                        .top(px(0.0))
                        .right(px(0.0))
                        .w(px(350.0))
                        .h(px(36.0))
                        .bg(hsla(0.0, 0.0, 0.15, 0.95))
                        .border_1()
                        .border_color(hsla(0.0, 0.0, 0.3, 1.0))
                        .rounded_bl(px(6.0))
                        .flex()
                        .items_center()
                        .px(px(8.0))
                        .gap(px(4.0))
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(regex_color)
                                .when(regex_mode, |d| {
                                    d.bg(hsla(0.55, 0.3, 0.2, 1.0)).rounded(px(2.0)).px(px(3.0))
                                })
                                .child(".*"),
                        )
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(13.0))
                                .text_color(hsla(0.0, 0.0, 0.85, 1.0))
                                .child(query_display),
                        )
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(hsla(0.0, 0.0, 0.5, 1.0))
                                .child(match_info),
                        ),
                )
            })
            .when(self.replay.is_some(), |d| {
                let replay = self.replay.as_ref().expect("checked above");
                let play_icon = if replay.playing {
                    "\u{23F8}"
                } else {
                    "\u{25B6}"
                };
                let speed_label = format!("{:.1}x", replay.speed);
                let current_speed = replay.speed;
                let progress_pct = (replay.progress_fraction() * 100.0) as u32;
                let position_secs = replay.position;
                let total_secs = replay.total_duration;
                let time_label = format!(
                    "{:02}:{:02} / {:02}:{:02}",
                    position_secs as u64 / 60,
                    position_secs as u64 % 60,
                    total_secs as u64 / 60,
                    total_secs as u64 % 60,
                );
                let finished = replay.is_finished();
                let bar_fraction = replay.progress_fraction();
                d.child(
                    div()
                        .id("replay-bar")
                        .absolute()
                        .bottom(px(0.0))
                        .left(px(0.0))
                        .w_full()
                        .flex()
                        .flex_col()
                        .child(
                            div()
                                .id("replay-progress-track")
                                .w_full()
                                .h(px(6.0))
                                .bg(hsla(0.0, 0.0, 0.2, 0.8))
                                .cursor_pointer()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                                        let display = this.display.read();
                                        let bounds = match display.bounds {
                                            Some(b) => b,
                                            None => return,
                                        };
                                        drop(display);
                                        let click_x: f32 =
                                            (event.position.x - bounds.origin.x).into();
                                        let width: f32 = bounds.size.width.into();
                                        if width > 0.0 {
                                            let fraction = (click_x / width).clamp(0.0, 1.0);
                                            this.seek_replay(fraction);
                                            cx.notify();
                                        }
                                    }),
                                )
                                .child(
                                    div()
                                        .h_full()
                                        .bg(hsla(0.55, 0.7, 0.5, 0.9))
                                        .w(relative(bar_fraction)),
                                ),
                        )
                        .child(
                            div()
                                .w_full()
                                .h(px(28.0))
                                .bg(hsla(0.0, 0.0, 0.1, 0.92))
                                .flex()
                                .items_center()
                                .px(px(12.0))
                                .gap(px(10.0))
                                .child(
                                    div()
                                        .id("replay-play-btn")
                                        .text_size(px(14.0))
                                        .text_color(hsla(0.0, 0.0, 0.9, 1.0))
                                        .cursor_pointer()
                                        .hover(|s| s.text_color(hsla(0.55, 0.7, 0.8, 1.0)))
                                        .on_click(cx.listener(
                                            |this, _: &ClickEvent, _window, cx| {
                                                this.toggle_replay_playback();
                                                cx.notify();
                                            },
                                        ))
                                        .child(if finished { "\u{23F9}" } else { play_icon }),
                                )
                                .child(
                                    div()
                                        .id("replay-speed-down")
                                        .text_size(px(12.0))
                                        .text_color(hsla(0.0, 0.0, 0.6, 1.0))
                                        .cursor_pointer()
                                        .hover(|s| s.text_color(hsla(0.0, 0.0, 0.9, 1.0)))
                                        .on_click(cx.listener(
                                            move |this, _: &ClickEvent, _window, cx| {
                                                let new_speed = (current_speed / 2.0).max(0.25);
                                                this.set_replay_speed(new_speed);
                                                cx.notify();
                                            },
                                        ))
                                        .child("\u{25C0}"),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.0))
                                        .text_color(hsla(0.55, 0.5, 0.7, 1.0))
                                        .child(speed_label),
                                )
                                .child(
                                    div()
                                        .id("replay-speed-up")
                                        .text_size(px(12.0))
                                        .text_color(hsla(0.0, 0.0, 0.6, 1.0))
                                        .cursor_pointer()
                                        .hover(|s| s.text_color(hsla(0.0, 0.0, 0.9, 1.0)))
                                        .on_click(cx.listener(
                                            move |this, _: &ClickEvent, _window, cx| {
                                                let new_speed = (current_speed * 2.0).min(8.0);
                                                this.set_replay_speed(new_speed);
                                                cx.notify();
                                            },
                                        ))
                                        .child("\u{25B6}"),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .text_size(px(11.0))
                                        .text_color(hsla(0.0, 0.0, 0.6, 1.0))
                                        .child(time_label),
                                )
                                .child(
                                    div()
                                        .text_size(px(10.0))
                                        .text_color(hsla(0.0, 0.0, 0.45, 1.0))
                                        .child(format!("{}%", progress_pct)),
                                ),
                        ),
                )
            })
            .child({
                let search_matches = self.search.matches.clone();
                let hovered_url = self.hovered_url;
                let search_current = self.search.current_match;
                canvas(
                    move |bounds, _window, _cx| {
                        let (cell_width, cell_height, current_font_size) = {
                            let mut display = display_arc.write();
                            display.bounds = Some(bounds);
                            (display.cell_dims.0, display.cell_dims.1, display.font_size)
                        };

                        let bounds_width: f32 = bounds.size.width.into();
                        let bounds_height: f32 = bounds.size.height.into();

                        let new_cols =
                            ((bounds_width - PADDING * 2.0).max(0.0) / cell_width).floor() as u16;
                        let new_rows =
                            ((bounds_height - PADDING * 2.0).max(0.0) / cell_height).floor() as u16;
                        let new_cols = new_cols.max(10);
                        let new_rows = new_rows.max(3);

                        let needs_resize = {
                            let display = display_arc.read();
                            new_cols != display.size.cols || new_rows != display.size.rows
                        };

                        if needs_resize {
                            {
                                let mut display = display_arc.write();
                                display.size.cols = new_cols;
                                display.size.rows = new_rows;
                            }

                            let new_size = TermSize {
                                cols: new_cols,
                                rows: new_rows,
                            };

                            {
                                let pixel_width = bounds_width as u16;
                                let pixel_height = bounds_height as u16;
                                let pty_guard = pty.lock();
                                if let Some(ref pty_inner) = *pty_guard {
                                    if let Err(e) = pty_inner.resize(
                                        new_rows,
                                        new_cols,
                                        pixel_width,
                                        pixel_height,
                                    ) {
                                        tracing::warn!(
                                            cols = new_cols,
                                            rows = new_rows,
                                            error = %e,
                                            "PTY resize failed, shell may have exited"
                                        );
                                    }
                                }
                            }

                            {
                                let mut term_guard = term.lock();
                                term_guard.resize(new_size);
                            }
                        }

                        let (cols, rows) = {
                            let display = display_arc.read();
                            (display.size.cols as usize, display.size.rows as usize)
                        };

                        let term_guard = term.lock();
                        let render_data = build_render_data(
                            &term_guard,
                            &colors_clone,
                            font_family_clone.clone(),
                        );
                        let selection_range = term_guard.renderable_content().selection;
                        let display_offset = term_guard.grid().display_offset() as i32;
                        drop(term_guard);

                        let selection_color = colors_clone.selection;

                        (
                            render_data,
                            bounds,
                            cell_width,
                            cell_height,
                            selection_range,
                            cols,
                            rows,
                            selection_color,
                            font_family_clone,
                            current_font_size,
                            display_offset,
                            search_matches,
                            search_current,
                            hovered_url,
                            font_fallbacks_clone,
                            progress_state,
                        )
                    },
                    paint_terminal_canvas,
                )
                .size_full()
            })
    }
}
