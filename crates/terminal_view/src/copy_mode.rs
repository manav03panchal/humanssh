use std::cmp;

/// Selection type in copy mode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CopyModeSelection {
    None,
    Character,
    Line,
    Block,
}

/// Computed selection range for rendering and text extraction.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectionRange {
    pub start_row: usize,
    pub start_col: usize,
    pub end_row: usize,
    pub end_col: usize,
    pub selection_type: CopyModeSelection,
}

/// Copy mode state machine.
///
/// Tracks cursor position, anchor position (selection start), and selection type.
/// All coordinates are zero-indexed into the visible grid.
pub struct CopyModeState {
    pub active: bool,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub selection: CopyModeSelection,
    pub anchor_row: usize,
    pub anchor_col: usize,
    pub grid_rows: usize,
    pub grid_cols: usize,
}

impl CopyModeState {
    pub fn new(grid_rows: usize, grid_cols: usize) -> Self {
        Self {
            active: false,
            cursor_row: 0,
            cursor_col: 0,
            selection: CopyModeSelection::None,
            anchor_row: 0,
            anchor_col: 0,
            grid_rows,
            grid_cols,
        }
    }

    pub fn enter(&mut self, cursor_row: usize, cursor_col: usize) {
        self.active = true;
        self.cursor_row = clamp(cursor_row, self.grid_rows);
        self.cursor_col = clamp(cursor_col, self.grid_cols);
        self.selection = CopyModeSelection::None;
        self.anchor_row = self.cursor_row;
        self.anchor_col = self.cursor_col;
    }

    pub fn cancel(&mut self) {
        self.active = false;
        self.selection = CopyModeSelection::None;
    }

    pub fn update_dimensions(&mut self, rows: usize, cols: usize) {
        self.grid_rows = rows;
        self.grid_cols = cols;
        self.cursor_row = clamp(self.cursor_row, rows);
        self.cursor_col = clamp(self.cursor_col, cols);
        self.anchor_row = clamp(self.anchor_row, rows);
        self.anchor_col = clamp(self.anchor_col, cols);
    }

    // -- Movement --

    pub fn move_left(&mut self) {
        self.cursor_col = self.cursor_col.saturating_sub(1);
    }

    pub fn move_down(&mut self) {
        if self.grid_rows > 0 && self.cursor_row < self.grid_rows - 1 {
            self.cursor_row += 1;
        }
    }

    pub fn move_up(&mut self) {
        self.cursor_row = self.cursor_row.saturating_sub(1);
    }

    pub fn move_right(&mut self) {
        if self.grid_cols > 0 && self.cursor_col < self.grid_cols - 1 {
            self.cursor_col += 1;
        }
    }

    pub fn move_to_line_start(&mut self) {
        self.cursor_col = 0;
    }

    pub fn move_to_line_end(&mut self) {
        if self.grid_cols > 0 {
            self.cursor_col = self.grid_cols - 1;
        }
    }

    pub fn move_to_top(&mut self) {
        self.cursor_row = 0;
    }

    pub fn move_to_bottom(&mut self) {
        if self.grid_rows > 0 {
            self.cursor_row = self.grid_rows - 1;
        }
    }

    pub fn page_up(&mut self, page_size: usize) {
        self.cursor_row = self.cursor_row.saturating_sub(page_size);
    }

    pub fn page_down(&mut self, page_size: usize) {
        if self.grid_rows > 0 {
            self.cursor_row = cmp::min(self.cursor_row + page_size, self.grid_rows - 1);
        }
    }

    pub fn move_word_forward(&mut self, grid: &[Vec<char>]) {
        if grid.is_empty() || self.grid_cols == 0 {
            return;
        }
        let mut row = self.cursor_row;
        let mut col = self.cursor_col;

        // Skip current word (non-whitespace characters)
        while row < grid.len() {
            if col < grid[row].len() && !grid[row][col].is_whitespace() {
                col += 1;
                if col >= self.grid_cols {
                    col = 0;
                    row += 1;
                }
            } else {
                break;
            }
        }

        // Skip whitespace to reach the start of the next word
        while row < grid.len() {
            if col < grid[row].len() && grid[row][col].is_whitespace() {
                col += 1;
                if col >= self.grid_cols {
                    col = 0;
                    row += 1;
                }
            } else {
                break;
            }
        }

        if row < self.grid_rows {
            self.cursor_row = row;
            self.cursor_col = col;
        } else if self.grid_rows > 0 {
            self.cursor_row = self.grid_rows - 1;
            self.cursor_col = if self.grid_cols > 0 {
                self.grid_cols - 1
            } else {
                0
            };
        }
    }

    pub fn move_word_backward(&mut self, grid: &[Vec<char>]) {
        if grid.is_empty() || self.grid_cols == 0 {
            return;
        }
        let mut row = self.cursor_row;
        let mut col = self.cursor_col;

        // Move back one position to get off the current word start
        if col == 0 {
            if row == 0 {
                return;
            }
            row -= 1;
            col = self.grid_cols.saturating_sub(1);
        } else {
            col -= 1;
        }

        // Skip whitespace backward
        loop {
            if row < grid.len() && col < grid[row].len() && grid[row][col].is_whitespace() {
                if col == 0 {
                    if row == 0 {
                        self.cursor_row = 0;
                        self.cursor_col = 0;
                        return;
                    }
                    row -= 1;
                    col = self.grid_cols.saturating_sub(1);
                } else {
                    col -= 1;
                }
            } else {
                break;
            }
        }

        // Skip non-whitespace backward to find word start
        loop {
            if col == 0 {
                break;
            }
            let prev = col - 1;
            if row < grid.len() && prev < grid[row].len() && !grid[row][prev].is_whitespace() {
                col = prev;
            } else {
                break;
            }
        }

        self.cursor_row = row;
        self.cursor_col = col;
    }

    // -- Selection --

    pub fn start_char_selection(&mut self) {
        self.toggle_selection_type(CopyModeSelection::Character);
    }

    pub fn start_line_selection(&mut self) {
        self.toggle_selection_type(CopyModeSelection::Line);
    }

    pub fn start_block_selection(&mut self) {
        self.toggle_selection_type(CopyModeSelection::Block);
    }

    pub fn toggle_selection_type(&mut self, new_type: CopyModeSelection) {
        if self.selection == new_type {
            self.selection = CopyModeSelection::None;
        } else {
            if self.selection == CopyModeSelection::None {
                self.anchor_row = self.cursor_row;
                self.anchor_col = self.cursor_col;
            }
            self.selection = new_type;
        }
    }

    // -- Range computation --

    pub fn selected_range(&self) -> Option<SelectionRange> {
        match self.selection {
            CopyModeSelection::None => None,
            CopyModeSelection::Character => {
                let (start_row, start_col, end_row, end_col) = normalize_positions(
                    self.anchor_row,
                    self.anchor_col,
                    self.cursor_row,
                    self.cursor_col,
                );
                Some(SelectionRange {
                    start_row,
                    start_col,
                    end_row,
                    end_col,
                    selection_type: CopyModeSelection::Character,
                })
            }
            CopyModeSelection::Line => {
                let start_row = cmp::min(self.anchor_row, self.cursor_row);
                let end_row = cmp::max(self.anchor_row, self.cursor_row);
                Some(SelectionRange {
                    start_row,
                    start_col: 0,
                    end_row,
                    end_col: self.grid_cols.saturating_sub(1),
                    selection_type: CopyModeSelection::Line,
                })
            }
            CopyModeSelection::Block => {
                let start_row = cmp::min(self.anchor_row, self.cursor_row);
                let end_row = cmp::max(self.anchor_row, self.cursor_row);
                let start_col = cmp::min(self.anchor_col, self.cursor_col);
                let end_col = cmp::max(self.anchor_col, self.cursor_col);
                Some(SelectionRange {
                    start_row,
                    start_col,
                    end_row,
                    end_col,
                    selection_type: CopyModeSelection::Block,
                })
            }
        }
    }

    // -- Text extraction --

    pub fn extract_text(&self, grid: &[Vec<char>]) -> String {
        let range = match self.selected_range() {
            Some(range) => range,
            None => return String::new(),
        };

        match range.selection_type {
            CopyModeSelection::None => String::new(),
            CopyModeSelection::Character => extract_character_text(grid, &range),
            CopyModeSelection::Line => extract_line_text(grid, &range),
            CopyModeSelection::Block => extract_block_text(grid, &range),
        }
    }
}

/// Clamp a coordinate to be within [0, dimension - 1], handling the zero-size case.
fn clamp(value: usize, dimension: usize) -> usize {
    if dimension == 0 {
        0
    } else {
        cmp::min(value, dimension - 1)
    }
}

/// Normalize anchor/cursor positions so that (start_row, start_col) <= (end_row, end_col).
fn normalize_positions(
    anchor_row: usize,
    anchor_col: usize,
    cursor_row: usize,
    cursor_col: usize,
) -> (usize, usize, usize, usize) {
    if anchor_row < cursor_row || (anchor_row == cursor_row && anchor_col <= cursor_col) {
        (anchor_row, anchor_col, cursor_row, cursor_col)
    } else {
        (cursor_row, cursor_col, anchor_row, anchor_col)
    }
}

fn extract_character_text(grid: &[Vec<char>], range: &SelectionRange) -> String {
    let mut result = String::new();
    for row in range.start_row..=range.end_row {
        if row >= grid.len() {
            break;
        }
        let col_start = if row == range.start_row {
            range.start_col
        } else {
            0
        };
        let col_end = if row == range.end_row {
            range.end_col
        } else {
            grid[row].len().saturating_sub(1)
        };
        for col in col_start..=col_end {
            if col < grid[row].len() {
                result.push(grid[row][col]);
            }
        }
        let is_last_row = row == range.end_row || row + 1 >= grid.len();
        if !is_last_row {
            result.push('\n');
        }
    }
    result
}

fn extract_line_text(grid: &[Vec<char>], range: &SelectionRange) -> String {
    let mut lines: Vec<String> = Vec::new();
    for row in range.start_row..=range.end_row {
        if row >= grid.len() {
            break;
        }
        let line: String = grid[row].iter().collect();
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n")
}

fn extract_block_text(grid: &[Vec<char>], range: &SelectionRange) -> String {
    let mut lines: Vec<String> = Vec::new();
    for row in range.start_row..=range.end_row {
        if row >= grid.len() {
            lines.push(String::new());
            continue;
        }
        let mut line = String::new();
        for col in range.start_col..=range.end_col {
            if col < grid[row].len() {
                line.push(grid[row][col]);
            }
        }
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_grid(lines: &[&str]) -> Vec<Vec<char>> {
        lines.iter().map(|line| line.chars().collect()).collect()
    }

    // -- Construction and enter/cancel --

    #[test]
    fn new_state_is_inactive() {
        let state = CopyModeState::new(24, 80);
        assert!(!state.active);
        assert_eq!(state.selection, CopyModeSelection::None);
    }

    #[test]
    fn enter_sets_cursor_and_activates() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(10, 20);
        assert!(state.active);
        assert_eq!(state.cursor_row, 10);
        assert_eq!(state.cursor_col, 20);
        assert_eq!(state.anchor_row, 10);
        assert_eq!(state.anchor_col, 20);
        assert_eq!(state.selection, CopyModeSelection::None);
    }

    #[test]
    fn enter_clamps_to_grid_bounds() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(100, 200);
        assert_eq!(state.cursor_row, 23);
        assert_eq!(state.cursor_col, 79);
    }

    #[test]
    fn cancel_deactivates() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(5, 5);
        state.start_char_selection();
        state.cancel();
        assert!(!state.active);
        assert_eq!(state.selection, CopyModeSelection::None);
    }

    // -- Movement bounds checking --

    #[test]
    fn move_left_stops_at_zero() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(0, 0);
        state.move_left();
        assert_eq!(state.cursor_col, 0);
    }

    #[test]
    fn move_left_decrements() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(0, 5);
        state.move_left();
        assert_eq!(state.cursor_col, 4);
    }

    #[test]
    fn move_right_stops_at_last_col() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(0, 79);
        state.move_right();
        assert_eq!(state.cursor_col, 79);
    }

    #[test]
    fn move_right_increments() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(0, 5);
        state.move_right();
        assert_eq!(state.cursor_col, 6);
    }

    #[test]
    fn move_up_stops_at_zero() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(0, 5);
        state.move_up();
        assert_eq!(state.cursor_row, 0);
    }

    #[test]
    fn move_up_decrements() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(5, 5);
        state.move_up();
        assert_eq!(state.cursor_row, 4);
    }

    #[test]
    fn move_down_stops_at_last_row() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(23, 5);
        state.move_down();
        assert_eq!(state.cursor_row, 23);
    }

    #[test]
    fn move_down_increments() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(5, 5);
        state.move_down();
        assert_eq!(state.cursor_row, 6);
    }

    #[test]
    fn move_to_line_start() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(5, 40);
        state.move_to_line_start();
        assert_eq!(state.cursor_col, 0);
    }

    #[test]
    fn move_to_line_end() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(5, 0);
        state.move_to_line_end();
        assert_eq!(state.cursor_col, 79);
    }

    #[test]
    fn move_to_top() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(15, 40);
        state.move_to_top();
        assert_eq!(state.cursor_row, 0);
    }

    #[test]
    fn move_to_bottom() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(5, 40);
        state.move_to_bottom();
        assert_eq!(state.cursor_row, 23);
    }

    #[test]
    fn page_up_clamps() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(5, 0);
        state.page_up(12);
        assert_eq!(state.cursor_row, 0);
    }

    #[test]
    fn page_up_moves() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(20, 0);
        state.page_up(12);
        assert_eq!(state.cursor_row, 8);
    }

    #[test]
    fn page_down_clamps() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(20, 0);
        state.page_down(12);
        assert_eq!(state.cursor_row, 23);
    }

    #[test]
    fn page_down_moves() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(5, 0);
        state.page_down(12);
        assert_eq!(state.cursor_row, 17);
    }

    // -- Word movement --

    #[test]
    fn word_forward_basic() {
        let grid = make_grid(&["hello world foo"]);
        let mut state = CopyModeState::new(1, 15);
        state.enter(0, 0);
        state.move_word_forward(&grid);
        assert_eq!(state.cursor_col, 6);
    }

    #[test]
    fn word_forward_at_end_stays() {
        let grid = make_grid(&["hello"]);
        let mut state = CopyModeState::new(1, 5);
        state.enter(0, 4);
        state.move_word_forward(&grid);
        assert_eq!(state.cursor_row, 0);
        assert_eq!(state.cursor_col, 4);
    }

    #[test]
    fn word_forward_crosses_rows() {
        let grid = make_grid(&["hello ", "world "]);
        let mut state = CopyModeState::new(2, 6);
        state.enter(0, 0);
        state.move_word_forward(&grid);
        assert_eq!(state.cursor_row, 1);
        assert_eq!(state.cursor_col, 0);
    }

    #[test]
    fn word_backward_basic() {
        let grid = make_grid(&["hello world"]);
        let mut state = CopyModeState::new(1, 11);
        state.enter(0, 6);
        state.move_word_backward(&grid);
        assert_eq!(state.cursor_col, 0);
    }

    #[test]
    fn word_backward_at_start_stays() {
        let grid = make_grid(&["hello"]);
        let mut state = CopyModeState::new(1, 5);
        state.enter(0, 0);
        state.move_word_backward(&grid);
        assert_eq!(state.cursor_row, 0);
        assert_eq!(state.cursor_col, 0);
    }

    #[test]
    fn word_backward_crosses_rows() {
        let grid = make_grid(&["hello ", "world "]);
        let mut state = CopyModeState::new(2, 6);
        state.enter(1, 0);
        state.move_word_backward(&grid);
        assert_eq!(state.cursor_row, 0);
        assert_eq!(state.cursor_col, 0);
    }

    #[test]
    fn word_forward_empty_grid() {
        let grid: Vec<Vec<char>> = Vec::new();
        let mut state = CopyModeState::new(0, 0);
        state.move_word_forward(&grid);
        assert_eq!(state.cursor_row, 0);
        assert_eq!(state.cursor_col, 0);
    }

    #[test]
    fn word_backward_empty_grid() {
        let grid: Vec<Vec<char>> = Vec::new();
        let mut state = CopyModeState::new(0, 0);
        state.move_word_backward(&grid);
        assert_eq!(state.cursor_row, 0);
        assert_eq!(state.cursor_col, 0);
    }

    // -- Selection toggling --

    #[test]
    fn toggle_char_selection_on_and_off() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(5, 10);
        state.start_char_selection();
        assert_eq!(state.selection, CopyModeSelection::Character);
        assert_eq!(state.anchor_row, 5);
        assert_eq!(state.anchor_col, 10);

        state.start_char_selection();
        assert_eq!(state.selection, CopyModeSelection::None);
    }

    #[test]
    fn toggle_switches_selection_type() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(5, 10);
        state.start_char_selection();
        assert_eq!(state.selection, CopyModeSelection::Character);

        state.start_line_selection();
        assert_eq!(state.selection, CopyModeSelection::Line);

        state.start_block_selection();
        assert_eq!(state.selection, CopyModeSelection::Block);

        state.start_block_selection();
        assert_eq!(state.selection, CopyModeSelection::None);
    }

    #[test]
    fn anchor_set_on_first_selection_preserved_on_switch() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(5, 10);
        state.move_down();
        state.move_right();
        // cursor is now at (6, 11), anchor should still be (5, 10) once set
        state.start_char_selection();
        assert_eq!(state.anchor_row, 6);
        assert_eq!(state.anchor_col, 11);

        state.move_down();
        // switching type should NOT reset anchor
        state.start_line_selection();
        assert_eq!(state.anchor_row, 6);
        assert_eq!(state.anchor_col, 11);
    }

    // -- Character selection range --

    #[test]
    fn char_selection_range_forward() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(2, 5);
        state.start_char_selection();
        state.cursor_row = 4;
        state.cursor_col = 10;
        let range = state.selected_range().expect("should have range");
        assert_eq!(range.start_row, 2);
        assert_eq!(range.start_col, 5);
        assert_eq!(range.end_row, 4);
        assert_eq!(range.end_col, 10);
        assert_eq!(range.selection_type, CopyModeSelection::Character);
    }

    #[test]
    fn char_selection_range_backward() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(4, 10);
        state.start_char_selection();
        state.cursor_row = 2;
        state.cursor_col = 5;
        let range = state.selected_range().expect("should have range");
        assert_eq!(range.start_row, 2);
        assert_eq!(range.start_col, 5);
        assert_eq!(range.end_row, 4);
        assert_eq!(range.end_col, 10);
    }

    #[test]
    fn char_selection_same_row() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(3, 10);
        state.start_char_selection();
        state.cursor_col = 5;
        let range = state.selected_range().expect("should have range");
        assert_eq!(range.start_col, 5);
        assert_eq!(range.end_col, 10);
    }

    // -- Line selection range --

    #[test]
    fn line_selection_range() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(3, 10);
        state.start_line_selection();
        state.cursor_row = 6;
        let range = state.selected_range().expect("should have range");
        assert_eq!(range.start_row, 3);
        assert_eq!(range.start_col, 0);
        assert_eq!(range.end_row, 6);
        assert_eq!(range.end_col, 79);
        assert_eq!(range.selection_type, CopyModeSelection::Line);
    }

    #[test]
    fn line_selection_single_row() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(5, 10);
        state.start_line_selection();
        let range = state.selected_range().expect("should have range");
        assert_eq!(range.start_row, 5);
        assert_eq!(range.end_row, 5);
        assert_eq!(range.start_col, 0);
        assert_eq!(range.end_col, 79);
    }

    // -- Block selection range --

    #[test]
    fn block_selection_range() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(2, 10);
        state.start_block_selection();
        state.cursor_row = 5;
        state.cursor_col = 20;
        let range = state.selected_range().expect("should have range");
        assert_eq!(range.start_row, 2);
        assert_eq!(range.start_col, 10);
        assert_eq!(range.end_row, 5);
        assert_eq!(range.end_col, 20);
        assert_eq!(range.selection_type, CopyModeSelection::Block);
    }

    #[test]
    fn block_selection_range_reversed_cols() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(2, 20);
        state.start_block_selection();
        state.cursor_row = 5;
        state.cursor_col = 10;
        let range = state.selected_range().expect("should have range");
        assert_eq!(range.start_col, 10);
        assert_eq!(range.end_col, 20);
    }

    #[test]
    fn no_selection_returns_none() {
        let state = CopyModeState::new(24, 80);
        assert!(state.selected_range().is_none());
    }

    // -- Text extraction: Character --

    #[test]
    fn extract_char_single_row() {
        let grid = make_grid(&["hello world"]);
        let mut state = CopyModeState::new(1, 11);
        state.enter(0, 0);
        state.start_char_selection();
        state.cursor_col = 4;
        assert_eq!(state.extract_text(&grid), "hello");
    }

    #[test]
    fn extract_char_multi_row() {
        let grid = make_grid(&["hello", "world", "foo  "]);
        let mut state = CopyModeState::new(3, 5);
        state.enter(0, 2);
        state.start_char_selection();
        state.cursor_row = 1;
        state.cursor_col = 2;
        assert_eq!(state.extract_text(&grid), "llo\nwor");
    }

    #[test]
    fn extract_no_selection() {
        let grid = make_grid(&["hello"]);
        let state = CopyModeState::new(1, 5);
        assert_eq!(state.extract_text(&grid), "");
    }

    // -- Text extraction: Line --

    #[test]
    fn extract_line_single() {
        let grid = make_grid(&["hello world   "]);
        let mut state = CopyModeState::new(1, 14);
        state.enter(0, 5);
        state.start_line_selection();
        assert_eq!(state.extract_text(&grid), "hello world");
    }

    #[test]
    fn extract_line_multiple() {
        let grid = make_grid(&["first   ", "second  ", "third   "]);
        let mut state = CopyModeState::new(3, 8);
        state.enter(0, 0);
        state.start_line_selection();
        state.cursor_row = 2;
        assert_eq!(state.extract_text(&grid), "first\nsecond\nthird");
    }

    // -- Text extraction: Block --

    #[test]
    fn extract_block_basic() {
        let grid = make_grid(&["abcdef", "ghijkl", "mnopqr"]);
        let mut state = CopyModeState::new(3, 6);
        state.enter(0, 1);
        state.start_block_selection();
        state.cursor_row = 2;
        state.cursor_col = 3;
        assert_eq!(state.extract_text(&grid), "bcd\nhij\nnop");
    }

    #[test]
    fn extract_block_with_trailing_spaces() {
        let grid = make_grid(&["ab  ef", "gh  kl", "mn  qr"]);
        let mut state = CopyModeState::new(3, 6);
        state.enter(0, 1);
        state.start_block_selection();
        state.cursor_row = 2;
        state.cursor_col = 3;
        // Columns 1..=3 are "b  " "h  " "n  " which get trimmed
        assert_eq!(state.extract_text(&grid), "b\nh\nn");
    }

    #[test]
    fn extract_block_single_column() {
        let grid = make_grid(&["abc", "def", "ghi"]);
        let mut state = CopyModeState::new(3, 3);
        state.enter(0, 1);
        state.start_block_selection();
        state.cursor_row = 2;
        state.cursor_col = 1;
        assert_eq!(state.extract_text(&grid), "b\ne\nh");
    }

    // -- update_dimensions --

    #[test]
    fn update_dimensions_clamps_cursor_and_anchor() {
        let mut state = CopyModeState::new(24, 80);
        state.enter(20, 70);
        state.start_char_selection();
        state.cursor_row = 23;
        state.cursor_col = 79;
        state.update_dimensions(10, 40);
        assert_eq!(state.cursor_row, 9);
        assert_eq!(state.cursor_col, 39);
        assert_eq!(state.anchor_row, 9);
        assert_eq!(state.anchor_col, 39);
        assert_eq!(state.grid_rows, 10);
        assert_eq!(state.grid_cols, 40);
    }

    // -- Zero-size grid safety --

    #[test]
    fn zero_size_grid_movements_do_not_panic() {
        let mut state = CopyModeState::new(0, 0);
        state.move_left();
        state.move_right();
        state.move_up();
        state.move_down();
        state.move_to_line_start();
        state.move_to_line_end();
        state.move_to_top();
        state.move_to_bottom();
        state.page_up(12);
        state.page_down(12);
        assert_eq!(state.cursor_row, 0);
        assert_eq!(state.cursor_col, 0);
    }

    #[test]
    fn extract_text_with_grid_shorter_than_selection() {
        let grid = make_grid(&["ab"]);
        let mut state = CopyModeState::new(5, 5);
        state.enter(0, 0);
        state.start_char_selection();
        state.cursor_row = 3;
        state.cursor_col = 4;
        let text = state.extract_text(&grid);
        assert_eq!(text, "ab");
    }

    #[test]
    fn word_forward_multiple_spaces() {
        let grid = make_grid(&["hello    world"]);
        let mut state = CopyModeState::new(1, 14);
        state.enter(0, 0);
        state.move_word_forward(&grid);
        assert_eq!(state.cursor_col, 9);
    }

    #[test]
    fn word_backward_from_middle_of_word() {
        let grid = make_grid(&["hello world"]);
        let mut state = CopyModeState::new(1, 11);
        state.enter(0, 8);
        state.move_word_backward(&grid);
        assert_eq!(state.cursor_col, 6);
    }
}
