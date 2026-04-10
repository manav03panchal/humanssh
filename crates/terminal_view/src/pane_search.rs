use super::*;

impl TerminalPane {
    pub(super) fn find_matches(&mut self) {
        self.search.matches.clear();
        if self.search.query.is_empty() {
            return;
        }

        self.search.recompile_regex();

        let term = self.term.lock();
        let grid = term.grid();
        let screen_lines = grid.screen_lines() as i32;
        let total_lines = grid.total_lines() as i32;
        let cols = grid.columns();

        let start_line = -(total_lines - screen_lines);

        if self.search.regex_mode {
            let compiled = match self.search.compiled_regex.as_ref() {
                Some(re) => re,
                None => return,
            };

            for line_idx in start_line..screen_lines {
                let row = &grid[Line(line_idx)];
                let chars: Vec<char> = (0..cols).map(|c| row[Column(c)].c).collect();
                let line_string: String = chars.iter().collect();

                let mut byte_to_col: Vec<usize> = Vec::with_capacity(line_string.len() + 1);
                for (col_idx, ch) in chars.iter().enumerate() {
                    for _ in 0..ch.len_utf8() {
                        byte_to_col.push(col_idx);
                    }
                }
                byte_to_col.push(chars.len());

                for matched in compiled.find_iter(&line_string) {
                    let start_col = byte_to_col[matched.start()];
                    let end_col = byte_to_col[matched.end()];
                    if start_col < end_col {
                        self.search.matches.push((line_idx, start_col, end_col));
                    }
                }
            }
        } else {
            let query_chars: Vec<char> = self.search.query.to_lowercase().chars().collect();
            let query_len = query_chars.len();

            for line_idx in start_line..screen_lines {
                let row = &grid[Line(line_idx)];
                let chars: Vec<char> = (0..cols).map(|c| row[Column(c)].c).collect();

                let mut col = 0;
                while col + query_len <= chars.len() {
                    let found = chars[col..col + query_len]
                        .iter()
                        .zip(query_chars.iter())
                        .all(|(grid_char, query_char)| {
                            grid_char.to_lowercase().eq(query_char.to_lowercase())
                        });

                    if found {
                        self.search.matches.push((line_idx, col, col + query_len));
                    }
                    col += 1;
                }
            }
        }

        if !self.search.matches.is_empty() {
            self.search.current_match = 0;
        }
    }

    /// Toggle search bar visibility.
    pub(super) fn toggle_search(&mut self, cx: &mut Context<Self>) {
        self.search.active = !self.search.active;
        if !self.search.active {
            self.search.query.clear();
            self.search.matches.clear();
            self.search.compiled_regex = None;
        }
        cx.notify();
    }

    pub(super) fn toggle_regex(&mut self, cx: &mut Context<Self>) {
        self.search.regex_mode = !self.search.regex_mode;
        self.search.recompile_regex();
        self.find_matches();
        cx.notify();
    }

    pub(super) fn search_next(&mut self, cx: &mut Context<Self>) {
        if !self.search.matches.is_empty() {
            self.search.current_match = (self.search.current_match + 1) % self.search.matches.len();
            self.scroll_to_match(cx);
        }
    }

    /// Move to the previous match.
    pub(super) fn search_prev(&mut self, cx: &mut Context<Self>) {
        if !self.search.matches.is_empty() {
            self.search.current_match = if self.search.current_match == 0 {
                self.search.matches.len() - 1
            } else {
                self.search.current_match - 1
            };
            self.scroll_to_match(cx);
        }
    }

    /// Scroll the terminal to make the current match visible.
    pub(super) fn scroll_to_match(&mut self, cx: &mut Context<Self>) {
        if let Some(&(line, _, _)) = self.search.matches.get(self.search.current_match) {
            let mut term = self.term.lock();
            let screen_lines = term.grid().screen_lines() as i32;
            let display_offset = term.grid().display_offset() as i32;
            let visible_top = -display_offset;
            let visible_bottom = visible_top + screen_lines - 1;
            if line < visible_top || line > visible_bottom {
                let target_offset = -(line - screen_lines / 2);
                let max_offset = (term.grid().total_lines() as i32 - screen_lines).max(0);
                let clamped = target_offset.max(0).min(max_offset);
                term.scroll_display(Scroll::Top);
                let current_offset = term.grid().display_offset() as i32;
                let delta = clamped - current_offset;
                if delta != 0 {
                    term.scroll_display(Scroll::Delta(-delta));
                }
            }
            drop(term);
            cx.notify();
        }
    }
}
