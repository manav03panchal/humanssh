//! Tests for TerminalPane — extracted to avoid rustc stack overflow on large files.

use super::*;
use pretty_assertions::assert_eq;
use test_case::test_case;

/// Create a TextRun with proper styling based on cell flags (test helper)
fn create_text_run(len: usize, font_family: &SharedString, fg: Hsla, flags: CellFlags) -> TextRun {
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
            features: ligature_features(),
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
// Display State Tests (No GPUI required)
// ============================================================================

#[::core::prelude::v1::test]
fn test_display_state_default() {
    let display = DisplayState::default();

    assert_eq!(display.size.cols, 80, "Default columns should be 80");
    assert_eq!(display.size.rows, 24, "Default rows should be 24");
    assert!(display.cell_dims.0 > 0.0, "Cell width should be positive");
    assert!(display.cell_dims.1 > 0.0, "Cell height should be positive");
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

// ============================================================================
// MouseEscBuf SGR Format Test (pane-specific: tests coordinate offset logic)
// ============================================================================

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

#[::core::prelude::v1::test]
fn test_create_text_run_all_flags_combined() {
    let font_family: SharedString = "Test Font".into();
    let fg = Hsla::default();

    let all_flags = CellFlags::BOLD
        | CellFlags::ITALIC
        | CellFlags::UNDERLINE
        | CellFlags::STRIKEOUT
        | CellFlags::DIM
        | CellFlags::INVERSE
        | CellFlags::HIDDEN;

    let run = create_text_run(5, &font_family, fg, all_flags);

    assert_eq!(run.font.weight, FontWeight::BOLD);
    assert_eq!(run.font.style, FontStyle::Italic);
    assert!(run.underline.is_some());
    assert!(run.strikethrough.is_some());
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

    listener.send_event(alacritty_terminal::event::Event::Title(
        "Test Title".to_string(),
    ));

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
    assert_eq!(cloned.title.lock().as_deref(), Some("Original"));
}

#[::core::prelude::v1::test]
fn test_listener_empty_title() {
    use alacritty_terminal::event::EventListener;
    let listener = Listener::new();

    listener.send_event(alacritty_terminal::event::Event::Title(String::new()));

    let title = listener.title.lock();
    assert_eq!(title.as_deref(), Some(""));
}

#[::core::prelude::v1::test]
fn test_listener_very_long_title() {
    use alacritty_terminal::event::EventListener;
    let listener = Listener::new();

    let long_title = "A".repeat(10000);
    listener.send_event(alacritty_terminal::event::Event::Title(long_title.clone()));

    let title = listener.title.lock();
    assert_eq!(title.as_deref(), Some(long_title.as_str()));
}

#[::core::prelude::v1::test]
fn test_listener_unicode_title() {
    use alacritty_terminal::event::EventListener;
    let listener = Listener::new();

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

    listener.send_event(alacritty_terminal::event::Event::Title("First".to_string()));
    assert_eq!(listener.title.lock().as_deref(), Some("First"));

    listener.send_event(alacritty_terminal::event::Event::Title(
        "Second".to_string(),
    ));
    assert_eq!(listener.title.lock().as_deref(), Some("Second"));
}

#[::core::prelude::v1::test]
fn test_listener_reset_title_event() {
    use alacritty_terminal::event::EventListener;
    let listener = Listener::new();

    listener.send_event(alacritty_terminal::event::Event::Title(
        "My Title".to_string(),
    ));
    assert_eq!(listener.title.lock().as_deref(), Some("My Title"));

    listener.send_event(alacritty_terminal::event::Event::ResetTitle);
    assert!(listener.title.lock().is_none());
}

#[::core::prelude::v1::test]
fn test_listener_new_has_no_cwd() {
    let listener = Listener::new();
    assert!(listener.cwd.lock().is_none());
}

#[::core::prelude::v1::test]
fn test_listener_new_has_no_prompt_line() {
    let listener = Listener::new();
    assert!(listener.last_prompt_line.lock().is_none());
}

#[::core::prelude::v1::test]
fn test_listener_cwd_direct_write() {
    let listener = Listener::new();
    *listener.cwd.lock() = Some("/home/user/project".to_string());
    assert_eq!(listener.cwd.lock().as_deref(), Some("/home/user/project"));
}

#[::core::prelude::v1::test]
fn test_listener_cwd_overwrite() {
    let listener = Listener::new();
    *listener.cwd.lock() = Some("/first".to_string());
    *listener.cwd.lock() = Some("/second".to_string());
    assert_eq!(listener.cwd.lock().as_deref(), Some("/second"));
}

#[::core::prelude::v1::test]
fn test_listener_prompt_line_direct_write() {
    let listener = Listener::new();
    *listener.last_prompt_line.lock() = Some(42);
    assert_eq!(*listener.last_prompt_line.lock(), Some(42));
}

#[::core::prelude::v1::test]
fn test_listener_prompt_line_overwrite() {
    let listener = Listener::new();
    *listener.last_prompt_line.lock() = Some(10);
    *listener.last_prompt_line.lock() = Some(25);
    assert_eq!(*listener.last_prompt_line.lock(), Some(25));
}

#[::core::prelude::v1::test]
fn test_listener_clone_preserves_cwd() {
    let listener = Listener::new();
    *listener.cwd.lock() = Some("/tmp/test".to_string());

    let cloned = listener.clone();
    assert_eq!(cloned.cwd.lock().as_deref(), Some("/tmp/test"));

    // Arc-shared: mutation through clone is visible in original.
    *cloned.cwd.lock() = Some("/tmp/other".to_string());
    assert_eq!(listener.cwd.lock().as_deref(), Some("/tmp/other"));
}

#[::core::prelude::v1::test]
fn test_listener_clone_preserves_prompt_line() {
    let listener = Listener::new();
    *listener.last_prompt_line.lock() = Some(7);

    let cloned = listener.clone();
    assert_eq!(*cloned.last_prompt_line.lock(), Some(7));
}

// ============================================================================
// Pixel to Cell Conversion Logic Tests
// ============================================================================

#[::core::prelude::v1::test]
fn test_pixel_to_cell_calculation() {
    let cell_width = 10.0_f32;
    let cell_height = 20.0_f32;
    let padding = PADDING;

    let local_x = 25.0_f32;
    let local_y = 45.0_f32;

    let cell_x = ((local_x - padding) / cell_width).floor() as i32;
    let cell_y = ((local_y - padding) / cell_height).floor() as i32;

    assert_eq!(cell_x, 2);
    assert_eq!(cell_y, 2);
}

#[::core::prelude::v1::test]
fn test_pixel_to_cell_negative_result() {
    let cell_width = 10.0_f32;
    let cell_height = 20.0_f32;
    let padding = PADDING;

    let local_x = 0.0_f32;
    let local_y = 0.0_f32;

    let cell_x = ((local_x - padding) / cell_width).floor() as i32;
    let cell_y = ((local_y - padding) / cell_height).floor() as i32;

    assert!(cell_x < 0);
    assert!(cell_y < 0);
}

#[::core::prelude::v1::test]
fn test_pixel_to_cell_calculation_zero_cell_size() {
    let cell_width = 0.0_f32;
    let cell_height = 0.0_f32;

    let cell_x = if cell_width > 0.0 {
        ((100.0_f32 - PADDING) / cell_width).floor() as i32
    } else {
        0
    };

    let cell_y = if cell_height > 0.0 {
        ((100.0_f32 - PADDING) / cell_height).floor() as i32
    } else {
        0
    };

    assert_eq!(cell_x, 0);
    assert_eq!(cell_y, 0);
}

// ============================================================================
// Mouse Escape Buffer Edge Cases (pane-specific: all buttons, release format)
// ============================================================================

#[::core::prelude::v1::test]
fn test_mouse_esc_buf_all_buttons() {
    use std::fmt::Write;

    for button in [0, 1, 2, 64, 65] {
        let mut buf = MouseEscBuf::new();
        write!(buf, "\x1b[<{};10;10M", button).unwrap();
        assert!(buf.as_str().contains(&format!("{}", button)));
    }
}

#[::core::prelude::v1::test]
fn test_mouse_esc_buf_release_format() {
    use std::fmt::Write;
    let mut buf = MouseEscBuf::new();
    write!(buf, "\x1b[<0;10;10m").unwrap();
    assert!(buf.as_str().ends_with('m'));
}

#[::core::prelude::v1::test]
fn test_mouse_esc_buf_negative_coordinate_handling() {
    use std::fmt::Write;
    let mut buf = MouseEscBuf::new();

    let large_num = u32::MAX;
    let _ = write!(buf, "\x1b[<0;{};1M", large_num);
    assert!(buf.as_str().len() <= 32);
}

// ============================================================================
// Malformed Escape Sequences
// ============================================================================

#[::core::prelude::v1::test]
fn test_escape_sequence_incomplete() {
    use std::fmt::Write;
    let mut buf = MouseEscBuf::new();

    write!(buf, "\x1b[<0;10;10").unwrap();
    assert!(buf.as_str().starts_with("\x1b"));
}

#[::core::prelude::v1::test]
fn test_escape_sequence_wrong_terminator() {
    use std::fmt::Write;
    let mut buf = MouseEscBuf::new();

    write!(buf, "\x1b[<0;10;10Z").unwrap();
    assert_eq!(buf.as_str(), "\x1b[<0;10;10Z");
}

#[::core::prelude::v1::test]
fn test_escape_sequence_extra_semicolons() {
    use std::fmt::Write;
    let mut buf = MouseEscBuf::new();

    write!(buf, "\x1b[<0;;10;10M").unwrap();
    assert!(buf.as_str().contains(";;"));
}

#[::core::prelude::v1::test]
fn test_escape_sequence_missing_bracket() {
    use std::fmt::Write;
    let mut buf = MouseEscBuf::new();

    write!(buf, "\x1b<0;10;10M").unwrap();
    assert!(!buf.as_str().contains("["));
}

// ============================================================================
// Terminal Size Calculation Overflow Test
// ============================================================================

#[::core::prelude::v1::test]
fn test_terminal_size_calculation_prevents_overflow() {
    let bounds_width = 10000.0_f32;
    let bounds_height = 10000.0_f32;
    let cell_width = 0.01_f32;
    let cell_height = 0.01_f32;
    let padding = PADDING;

    let cols = ((bounds_width - padding * 2.0).max(0.0) / cell_width).floor() as u16;
    let rows = ((bounds_height - padding * 2.0).max(0.0) / cell_height).floor() as u16;

    assert!(cols > 0);
    assert!(rows > 0);
}

// ============================================================================
// Mouse Event Coordinate Edge Cases
// ============================================================================

#[::core::prelude::v1::test]
fn test_mouse_coordinate_at_origin() {
    let cell_width = 10.0_f32;
    let cell_height = 20.0_f32;
    let padding = PADDING;

    let cell_x = ((padding - padding) / cell_width).floor() as i32;
    let cell_y = ((padding - padding) / cell_height).floor() as i32;

    assert_eq!(cell_x, 0);
    assert_eq!(cell_y, 0);
}

#[::core::prelude::v1::test]
fn test_mouse_coordinate_at_max_cell() {
    let cols = 80;
    let rows = 24;
    let cell_width = 10.0_f32;
    let cell_height = 20.0_f32;
    let padding = PADDING;

    let local_x = padding + (cols as f32 - 0.5) * cell_width;
    let local_y = padding + (rows as f32 - 0.5) * cell_height;

    let cell_x = ((local_x - padding) / cell_width).floor() as i32;
    let cell_y = ((local_y - padding) / cell_height).floor() as i32;

    assert_eq!(cell_x, cols - 1);
    assert_eq!(cell_y, rows - 1);
}

#[::core::prelude::v1::test]
fn test_mouse_coordinate_past_terminal() {
    let cols = 80;
    let rows = 24;
    let cell_width = 10.0_f32;
    let cell_height = 20.0_f32;
    let padding = PADDING;

    let local_x = padding + (cols as f32 + 10.0) * cell_width;
    let local_y = padding + (rows as f32 + 10.0) * cell_height;

    let cell_x = ((local_x - padding) / cell_width).floor() as i32;
    let cell_y = ((local_y - padding) / cell_height).floor() as i32;

    assert!(cell_x >= cols);
    assert!(cell_y >= rows);
}

// ============================================================================
// Wide Character Placeholder Test
// ============================================================================

#[::core::prelude::v1::test]
fn test_render_cell_wide_character_placeholder() {
    let cell = RenderCell {
        row: 0,
        col: 1,
        c: ' ',
        fg: Hsla::default(),
        flags: CellFlags::WIDE_CHAR_SPACER,
    };

    assert!(cell.flags.contains(CellFlags::WIDE_CHAR_SPACER));
}

// ============================================================================
// BgRegion Edge Cases (pane-specific: inverted columns with saturating_sub)
// ============================================================================

#[::core::prelude::v1::test]
fn test_bg_region_inverted_columns() {
    let region = BgRegion {
        row: 0,
        col_start: 20,
        col_end: 10,
        color: Hsla::default(),
    };

    let width = region.col_end.saturating_sub(region.col_start);
    assert_eq!(width, 0);
}

// ============================================================================
// CursorInfo Edge Cases (pane-specific: Hidden and HollowBlock shapes)
// ============================================================================

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
    let line = "Check https://example.com.";
    assert_eq!(
        TerminalPane::find_url_at_position(line, 10),
        Some("https://example.com".to_string())
    );

    let line = "See https://example.com, then continue";
    assert_eq!(
        TerminalPane::find_url_at_position(line, 10),
        Some("https://example.com".to_string())
    );

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
    assert_eq!(TerminalPane::find_url_at_position(line, 0), None);
    assert_eq!(TerminalPane::find_url_at_position(line, 28), None);
}

#[::core::prelude::v1::test]
fn test_find_url_multiple_urls() {
    let line = "First https://a.com then https://b.com end";
    assert_eq!(
        TerminalPane::find_url_at_position(line, 8),
        Some("https://a.com".to_string())
    );
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
    let line = "\u{1F680} Check http://localhost:4321/ for updates";
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
    let line = "~/projects \u{279C} http://localhost:8080/api";
    assert_eq!(
        TerminalPane::find_url_at_position(line, 15),
        Some("http://localhost:8080/api".to_string())
    );
}

// ============================================================================
// Search State Tests
// ============================================================================

#[::core::prelude::v1::test]
fn test_search_state_default() {
    let state = SearchState::new();
    assert!(!state.active);
    assert!(state.query.is_empty());
    assert!(state.matches.is_empty());
    assert_eq!(state.current_match, 0);
    assert!(!state.regex_mode);
    assert!(state.compiled_regex.is_none());
}

#[::core::prelude::v1::test]
fn test_search_state_recompile_regex_valid_pattern() {
    let mut state = SearchState::new();
    state.regex_mode = true;
    state.query = "foo.*bar".to_string();
    state.recompile_regex();
    assert!(state.compiled_regex.is_some());
}

#[::core::prelude::v1::test]
fn test_search_state_recompile_regex_invalid_pattern() {
    let mut state = SearchState::new();
    state.regex_mode = true;
    state.query = "[invalid(regex".to_string();
    state.recompile_regex();
    assert!(state.compiled_regex.is_none());
}

#[::core::prelude::v1::test]
fn test_search_state_recompile_regex_empty_query() {
    let mut state = SearchState::new();
    state.regex_mode = true;
    state.query = String::new();
    state.recompile_regex();
    assert!(state.compiled_regex.is_none());
}

#[::core::prelude::v1::test]
fn test_search_state_recompile_regex_not_in_regex_mode() {
    let mut state = SearchState::new();
    state.regex_mode = false;
    state.query = "foo".to_string();
    state.recompile_regex();
    assert!(state.compiled_regex.is_none());
}

#[::core::prelude::v1::test]
fn test_search_state_regex_is_case_insensitive() {
    let mut state = SearchState::new();
    state.regex_mode = true;
    state.query = "hello".to_string();
    state.recompile_regex();
    let re = state.compiled_regex.as_ref().unwrap();
    assert!(re.is_match("HELLO"));
    assert!(re.is_match("Hello"));
    assert!(re.is_match("hello"));
}

#[::core::prelude::v1::test]
fn test_search_state_regex_character_class() {
    let mut state = SearchState::new();
    state.regex_mode = true;
    state.query = r"\d+".to_string();
    state.recompile_regex();
    let re = state.compiled_regex.as_ref().unwrap();
    assert!(re.is_match("abc123def"));
    assert!(!re.is_match("abcdef"));
}

#[::core::prelude::v1::test]
fn test_search_state_regex_alternation() {
    let mut state = SearchState::new();
    state.regex_mode = true;
    state.query = "foo|bar".to_string();
    state.recompile_regex();
    let re = state.compiled_regex.as_ref().unwrap();
    assert!(re.is_match("foo"));
    assert!(re.is_match("bar"));
    assert!(!re.is_match("baz"));
}

#[::core::prelude::v1::test]
fn test_search_state_regex_find_positions() {
    let mut state = SearchState::new();
    state.regex_mode = true;
    state.query = r"h.llo".to_string();
    state.recompile_regex();
    let re = state.compiled_regex.as_ref().unwrap();
    let text = "hello world";
    let matched = re.find(text).unwrap();
    assert_eq!(matched.start(), 0);
    assert_eq!(matched.end(), 5);
}

#[::core::prelude::v1::test]
fn test_search_state_regex_multibyte_byte_to_col_mapping() {
    let chars: Vec<char> = "ab\u{00E9}cd".chars().collect();
    let line_string: String = chars.iter().collect();

    let mut byte_to_col: Vec<usize> = Vec::new();
    for (col_idx, ch) in chars.iter().enumerate() {
        for _ in 0..ch.len_utf8() {
            byte_to_col.push(col_idx);
        }
    }
    byte_to_col.push(chars.len());

    assert_eq!(byte_to_col[0], 0);
    assert_eq!(byte_to_col[1], 1);
    assert_eq!(byte_to_col[2], 2);
    assert_eq!(byte_to_col[3], 2);
    assert_eq!(byte_to_col[4], 3);
    assert_eq!(byte_to_col[5], 4);

    let re = regex::Regex::new(r"\u{00E9}c").unwrap();
    let matched = re.find(&line_string).unwrap();
    let start_col = byte_to_col[matched.start()];
    let end_col = byte_to_col[matched.end()];
    assert_eq!(start_col, 2);
    assert_eq!(end_col, 4);
}

#[::core::prelude::v1::test]
fn test_search_state_regex_toggle_clears_compiled() {
    let mut state = SearchState::new();
    state.regex_mode = true;
    state.query = "test".to_string();
    state.recompile_regex();
    assert!(state.compiled_regex.is_some());

    state.regex_mode = false;
    state.recompile_regex();
    assert!(state.compiled_regex.is_none());
}

#[::core::prelude::v1::test]
fn test_search_state_regex_multiple_matches_in_line() {
    let mut state = SearchState::new();
    state.regex_mode = true;
    state.query = r"\d+".to_string();
    state.recompile_regex();
    let re = state.compiled_regex.as_ref().unwrap();

    let text = "abc 123 def 456 ghi";
    let matches: Vec<_> = re.find_iter(text).collect();
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].as_str(), "123");
    assert_eq!(matches[1].as_str(), "456");
}

#[::core::prelude::v1::test]
fn test_search_state_regex_empty_match_skipped() {
    let chars: Vec<char> = "hello".chars().collect();
    let line_string: String = chars.iter().collect();

    let mut byte_to_col: Vec<usize> = Vec::new();
    for (col_idx, ch) in chars.iter().enumerate() {
        for _ in 0..ch.len_utf8() {
            byte_to_col.push(col_idx);
        }
    }
    byte_to_col.push(chars.len());

    let re = regex::Regex::new("(?i)h?").unwrap();
    let mut results = Vec::new();
    for matched in re.find_iter(&line_string) {
        let start_col = byte_to_col[matched.start()];
        let end_col = byte_to_col[matched.end()];
        if start_col < end_col {
            results.push((start_col, end_col));
        }
    }
    assert!(results.contains(&(0, 1)));
    for (start, end) in &results {
        assert!(start < end);
    }
}

// ========================================================================
// TabBadge Tests
// ========================================================================

#[test]
fn test_tab_badge_variants() {
    let running = TabBadge::Running;
    let success = TabBadge::Success;
    let failed = TabBadge::Failed(1);

    assert_eq!(running, TabBadge::Running);
    assert_eq!(success, TabBadge::Success);
    assert_eq!(failed, TabBadge::Failed(1));
    assert_ne!(running, success);
    assert_ne!(success, failed);
}

#[test]
fn test_tab_badge_failed_preserves_code() {
    let badge = TabBadge::Failed(42);
    if let TabBadge::Failed(code) = badge {
        assert_eq!(code, 42);
    } else {
        panic!("Expected Failed variant");
    }
}

#[test]
fn test_tab_badge_clone_and_copy() {
    let badge = TabBadge::Running;
    let cloned = badge;
    assert_eq!(badge, cloned);
}
