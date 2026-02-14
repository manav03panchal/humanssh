//! Terminal pane component using alacritty_terminal.
//!
//! Uses GPUI's canvas for efficient GPU-accelerated rendering with:
//! - Batched text runs via StyledText
//! - Merged background regions via paint_quad
//! - Proper handling of TUI applications

use crate::colors::{apply_dim, color_to_hsla, get_bright_color};
use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point as TermPoint, Side};
use alacritty_terminal::selection::{Selection as TermSelection, SelectionType};
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::{CursorShape, Processor, Rgb};
use gpui::prelude::FluentBuilder;
use gpui::*;
use gpui_component::ActiveTheme;
use terminal::types::{
    BgRegion, CursorInfo, DisplayState, MouseEscBuf, ProgressState, RenderCell, RenderData,
    TermSize,
};
use terminal::PtyHandler;
use termwiz::input::{KeyCode, KeyCodeEncodeModes, KeyboardEncoding, Modifiers as TermwizMods};
use theme::{terminal_colors, TerminalColors};

/// Visual badge state for tab annotations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TabBadge {
    /// Process is running
    Running,
    /// Process exited successfully (code 0)
    Success,
    /// Process exited with error
    Failed(i32),
}

/// Replay playback state for a loaded .cast recording.
pub struct ReplayState {
    events: Vec<terminal::recording::ReplayEvent>,
    current_index: usize,
    speed: f32,
    playing: bool,
    /// Total duration of the recording in seconds.
    total_duration: f64,
    /// Current playback position in seconds (virtual time).
    position: f64,
}

impl ReplayState {
    fn new(events: Vec<terminal::recording::ReplayEvent>) -> Self {
        let total_duration = events.last().map(|e| e.timestamp).unwrap_or(0.0);
        Self {
            events,
            current_index: 0,
            speed: 1.0,
            playing: true,
            total_duration,
            position: 0.0,
        }
    }

    fn toggle_play(&mut self) {
        self.playing = !self.playing;
    }

    fn set_speed(&mut self, speed: f32) {
        self.speed = speed.clamp(0.25, 8.0);
    }

    fn progress_fraction(&self) -> f32 {
        if self.total_duration <= 0.0 {
            return 1.0;
        }
        (self.position / self.total_duration).min(1.0) as f32
    }

    fn is_finished(&self) -> bool {
        self.current_index >= self.events.len()
    }

    /// Seek to a fraction (0.0..=1.0) of the total duration.
    /// Sets position and rewinds current_index so the timer replays from the right spot.
    fn seek_fraction(&mut self, fraction: f32) {
        let fraction = fraction.clamp(0.0, 1.0) as f64;
        self.position = self.total_duration * fraction;
        // Rewind index to the first event at or after the new position
        self.current_index = self.events.partition_point(|e| e.timestamp < self.position);
    }
}

use crate::copy_mode::CopyModeState;
use actions::{
    EnterCopyMode, ExitCopyMode, SearchNext, SearchPrev, SearchToggle, SearchToggleRegex,
    SendShiftTab, SendTab, StartRecording, StopRecording, OPTION_AS_ALT,
};
use parking_lot::{Mutex, RwLock};
use std::fmt::Write as FmtWrite;
use std::sync::atomic::Ordering;
use std::sync::Arc;

// Import centralized configuration
// FONT_FAMILY used in tests via super::*
#[allow(unused_imports)]
use settings::constants::terminal::{
    DEFAULT_FONT_SIZE, FONT_FAMILY, MAX_FONT_SIZE, MIN_FONT_SIZE, PADDING,
};

// Cell dimension caching is handled per-instance in DisplayState.cached_font_key

/// Font features enabling ligatures for fonts that support them (e.g., Fira Code, JetBrains Mono).
/// Enables standard ligatures (liga), contextual alternates (calt), and contextual ligatures (clig).
fn ligature_features() -> FontFeatures {
    FontFeatures(Arc::new(vec![
        ("liga".into(), 1),
        ("calt".into(), 1),
        ("clig".into(), 1),
    ]))
}

/// Actually calculate cell dimensions from font metrics.
/// Uses the same shaping system as rendering for consistency.
fn calculate_cell_dimensions(
    window: &mut Window,
    font_size: f32,
    font_family: &SharedString,
    font_fallbacks: &Option<FontFallbacks>,
) -> (f32, f32) {
    let font = Font {
        family: font_family.clone(),
        features: ligature_features(),
        fallbacks: font_fallbacks.clone(),
        weight: FontWeight::NORMAL,
        style: FontStyle::Normal,
    };
    let font_size_px = px(font_size);
    let text_system = window.text_system();

    // Get font ID for metrics
    let font_id = text_system.resolve_font(&font);

    // Cell width: Use GPUI's advance() for the actual glyph advance width
    // This is the proper monospace cell width from font metrics
    let cell_width: f32 = match text_system.advance(font_id, font_size_px, '0') {
        Ok(size) => size.width.into(),
        Err(_) => {
            // Fallback: shape a character if advance fails
            let run = TextRun {
                len: 1,
                font: font.clone(),
                color: black(),
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let shaped = text_system.shape_line("0".into(), font_size_px, &[run], None);
            shaped.width.into()
        }
    };

    // Cell height = ascent + |descent| from proper font metrics
    let ascent: f32 = text_system.ascent(font_id, font_size_px).into();
    let descent: f32 = text_system.descent(font_id, font_size_px).into();
    let cell_height = ascent + descent.abs();

    tracing::debug!(
        font = %font_family,
        size = font_size,
        cell_width = cell_width,
        cell_height = cell_height,
        ascent = ascent,
        descent = descent,
        "Cell dimensions calculated"
    );

    (cell_width, cell_height)
}

/// Event listener that captures terminal events (like title changes).
///
/// Also stores shell integration state for OSC 7 (current working directory)
/// and OSC 133 (semantic prompt marking). Note that alacritty_terminal 0.25 and
/// vte 0.15 do **not** parse OSC 7 or OSC 133 sequences — they fall through to
/// vte's `unhandled` path. The fields below are ready for when we add a custom
/// VTE pre-parser to intercept these sequences from raw PTY output before
/// alacritty processes them. In the meantime, CWD can be obtained via the
/// existing `PtyHandler::get_current_directory()` OS-level fallback.
#[derive(Clone)]
struct Listener {
    title: Arc<Mutex<Option<String>>>,
    /// Current working directory reported by the shell via OSC 7.
    /// Populated when a custom pre-parser intercepts `\e]7;file://host/path\a`.
    cwd: Arc<Mutex<Option<String>>>,
    /// Line number of the most recent prompt start (OSC 133;A).
    /// Used for prompt-to-prompt navigation and command output selection.
    last_prompt_line: Arc<Mutex<Option<i32>>>,
    /// PTY handle for writing terminal query responses back (CSI 6n, OSC 11, etc.)
    pty: Arc<Mutex<Option<PtyHandler>>>,
}

impl Listener {
    fn new(pty: Arc<Mutex<Option<PtyHandler>>>) -> Self {
        Self {
            title: Arc::new(Mutex::new(None)),
            cwd: Arc::new(Mutex::new(None)),
            last_prompt_line: Arc::new(Mutex::new(None)),
            pty,
        }
    }

    fn pty_write(&self, data: &[u8]) {
        let mut pty_guard = self.pty.lock();
        if let Some(ref mut pty) = *pty_guard {
            if let Err(e) = pty.write(data) {
                tracing::warn!(error = %e, "PTY write-back failed");
            }
        }
    }
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        match event {
            Event::Title(title) => *self.title.lock() = Some(title),
            Event::ResetTitle => *self.title.lock() = None,
            Event::PtyWrite(text) => self.pty_write(text.as_bytes()),
            Event::ColorRequest(_index, formatter) => {
                // Respond with dark background color for OSC 10/11/12 queries.
                // TUI apps (BubbleTea/lipgloss) use this to detect dark/light mode.
                let response = formatter(Rgb { r: 0, g: 0, b: 0 });
                self.pty_write(response.as_bytes());
            }
            Event::TextAreaSizeRequest(formatter) => {
                let response = formatter(alacritty_terminal::event::WindowSize {
                    num_lines: 24,
                    num_cols: 80,
                    cell_width: 8,
                    cell_height: 16,
                });
                self.pty_write(response.as_bytes());
            }
            _ => {}
        }
    }
}

/// Event emitted when the terminal process exits.
/// Workspace subscribes to this to automatically clean up dead panes.
#[derive(Clone, Debug)]
pub struct TerminalExitEvent;

struct SearchState {
    active: bool,
    query: String,
    matches: Vec<(i32, usize, usize)>,
    current_match: usize,
    regex_mode: bool,
    compiled_regex: Option<regex::Regex>,
}

impl SearchState {
    fn new() -> Self {
        Self {
            active: false,
            query: String::new(),
            matches: Vec::new(),
            current_match: 0,
            regex_mode: false,
            compiled_regex: None,
        }
    }

    fn recompile_regex(&mut self) {
        if !self.regex_mode || self.query.is_empty() {
            self.compiled_regex = None;
            return;
        }
        let pattern = format!("(?i){}", &self.query);
        match regex::Regex::new(&pattern) {
            Ok(re) => self.compiled_regex = Some(re),
            Err(_) => self.compiled_regex = None,
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
    /// In-buffer search state
    search: SearchState,
    /// URL span under cursor when Cmd is held: (visual_row, start_col, end_col)
    hovered_url: Option<(usize, usize, usize)>,
    /// Configured font fallback chain for glyphs not in the primary font
    font_fallbacks: Option<FontFallbacks>,
    /// Reverse scroll direction ("natural" scrolling)
    scroll_reverse: bool,
    /// Vi-style copy mode state
    copy_mode: CopyModeState,
    /// Dedicated VT processing thread (kept alive for Drop cleanup)
    _vt_processor: Option<terminal::TerminalProcessor>,
    /// Progress bar state from OSC 9;4 sequences
    progress: ProgressState,
    /// Replay state (Some when this pane is playing back a .cast recording).
    replay: Option<ReplayState>,
}

impl EventEmitter<TerminalExitEvent> for TerminalPane {}

impl TerminalPane {
    /// Create a new terminal pane with the user's default shell.
    ///
    /// Spawns a PTY process and starts polling for output. The terminal
    /// starts with default dimensions (80x24) and resizes when rendered.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self::new_in_dir(cx, None)
    }

    /// Create a new terminal pane with the user's default shell in a specific directory.
    ///
    /// # Arguments
    /// * `cx` - GPUI context
    /// * `working_dir` - Optional working directory for the new shell
    ///
    /// This is used when creating new tabs/splits to inherit the current terminal's
    /// working directory for better UX.
    pub fn new_in_dir(cx: &mut Context<Self>, working_dir: Option<std::path::PathBuf>) -> Self {
        // Use reasonable defaults - will be resized when layout occurs
        let display_state = DisplayState::default();
        let size = display_state.size;

        // Spawn PTY first so Listener can hold a write-back reference
        let (pty, spawn_error) =
            match PtyHandler::spawn_in_dir(size.rows, size.cols, working_dir.as_deref()) {
                Ok(pty) => (Some(pty), None),
                Err(e) => {
                    tracing::error!("Failed to spawn PTY: {}", e);
                    (None, Some(e.to_string()))
                }
            };
        let pty_arc = Arc::new(Mutex::new(pty));

        // Create terminal with config and event listener
        let listener = Listener::new(pty_arc.clone());
        let config = Config::default();
        let term = Term::new(config, &size, listener.clone());
        let term = Arc::new(Mutex::new(term));
        let processor = Arc::new(Mutex::new(Processor::new()));

        // Disable tab stop so Tab key passes through to the terminal instead of
        // being consumed by GPUI's focus navigation system
        let focus_handle = cx.focus_handle().tab_stop(false);

        let user_config = settings::load_config();
        let font_fallbacks = if user_config.font_fallbacks.is_empty() {
            None
        } else {
            Some(FontFallbacks::from_fonts(
                user_config.font_fallbacks.clone(),
            ))
        };

        // Inject error message before starting VT thread (so it appears immediately)
        if let Some(error) = spawn_error {
            let error_msg = format!(
                "\x1b[31m\x1b[1mError: Failed to spawn shell\x1b[0m\r\n\r\n{}\r\n\r\n\
                 \x1b[33mTroubleshooting:\x1b[0m\r\n\
                 - Check that your shell exists: echo $SHELL\r\n\
                 - Try setting SHELL=/bin/zsh or SHELL=/bin/bash\r\n",
                error
            );
            let mut term_guard = term.lock();
            let mut proc_guard = processor.lock();
            proc_guard.advance(&mut *term_guard, error_msg.as_bytes());
        }

        let vt_processor = Self::start_vt_processor(&pty_arc, term.clone(), processor.clone(), cx);

        Self {
            pty: pty_arc,
            term,
            listener,
            display: Arc::new(RwLock::new(display_state)),
            dragging: false,
            focus_handle,
            exit_emitted: false,
            search: SearchState::new(),
            hovered_url: None,
            font_fallbacks,
            scroll_reverse: user_config.scroll_reverse,
            copy_mode: CopyModeState::new(size.rows as usize, size.cols as usize),
            _vt_processor: vt_processor,
            progress: ProgressState::default(),
            replay: None,
        }
    }

    /// Create a new terminal pane running a specific command.
    ///
    /// # Arguments
    /// * `cx` - GPUI context
    /// * `command` - The command to run (e.g., "btop", "neofetch")
    /// * `args` - Arguments to pass to the command
    ///
    /// This is used for opening system utilities from the status bar.
    pub fn new_with_command(cx: &mut Context<Self>, command: &str, args: &[&str]) -> Self {
        let display_state = DisplayState::default();
        let size = display_state.size;

        let (pty, spawn_error) =
            match PtyHandler::spawn_command(size.rows, size.cols, command, args, None) {
                Ok(pty) => (Some(pty), None),
                Err(e) => {
                    tracing::error!("Failed to spawn command {}: {}", command, e);
                    (None, Some(e.to_string()))
                }
            };
        let pty_arc = Arc::new(Mutex::new(pty));

        let listener = Listener::new(pty_arc.clone());
        let config = Config::default();
        let term = Term::new(config, &size, listener.clone());
        let term = Arc::new(Mutex::new(term));
        let processor = Arc::new(Mutex::new(Processor::new()));

        let focus_handle = cx.focus_handle().tab_stop(false);

        let user_config = settings::load_config();
        let font_fallbacks = if user_config.font_fallbacks.is_empty() {
            None
        } else {
            Some(FontFallbacks::from_fonts(
                user_config.font_fallbacks.clone(),
            ))
        };

        // Inject error message before starting VT thread
        if let Some(error) = spawn_error {
            let error_msg = format!(
                "\x1b[31m\x1b[1mError: Failed to run '{}'\x1b[0m\r\n\r\n{}\r\n\r\n\
                 \x1b[33mTip:\x1b[0m Install the command with: brew install {}\r\n",
                command, error, command
            );
            let mut term_guard = term.lock();
            let mut proc_guard = processor.lock();
            proc_guard.advance(&mut *term_guard, error_msg.as_bytes());
        }

        let vt_processor = Self::start_vt_processor(&pty_arc, term.clone(), processor.clone(), cx);

        Self {
            pty: pty_arc,
            term,
            listener,
            display: Arc::new(RwLock::new(display_state)),
            dragging: false,
            focus_handle,
            exit_emitted: false,
            search: SearchState::new(),
            hovered_url: None,
            font_fallbacks,
            scroll_reverse: user_config.scroll_reverse,
            copy_mode: CopyModeState::new(size.rows as usize, size.cols as usize),
            _vt_processor: vt_processor,
            progress: ProgressState::default(),
            replay: None,
        }
    }

    /// Start the dedicated VT processing thread and spawn a GPUI timer task
    /// that polls the render-needed flag.
    ///
    /// Returns `None` if the PTY has no output receiver (already taken or no PTY).
    fn start_vt_processor(
        pty_arc: &Arc<Mutex<Option<PtyHandler>>>,
        term: Arc<Mutex<Term<Listener>>>,
        processor: Arc<Mutex<Processor>>,
        cx: &mut Context<Self>,
    ) -> Option<terminal::TerminalProcessor> {
        let (output_rx, exited) = {
            let mut pty_guard = pty_arc.lock();
            if let Some(ref mut pty) = *pty_guard {
                match pty.take_output_receiver() {
                    Some(rx) => (rx, pty.exited_flag()),
                    None => return None,
                }
            } else {
                return None;
            }
        };

        let vt_processor = terminal::TerminalProcessor::start(output_rx, term, processor, exited);

        // Poll the VT thread's render-needed and exited flags via GPUI timer.
        // Accesses `_vt_processor` through the entity, so no Arc sharing needed.
        cx.spawn(async move |this, cx| {
            const ACTIVE_INTERVAL: u64 = 4;
            const IDLE_INTERVAL: u64 = 100;
            const IDLE_THRESHOLD: u32 = 5;
            let mut idle_count = 0u32;

            loop {
                let interval = if idle_count >= IDLE_THRESHOLD {
                    IDLE_INTERVAL
                } else {
                    ACTIVE_INTERVAL
                };

                cx.background_executor()
                    .timer(std::time::Duration::from_millis(interval))
                    .await;

                let should_break = this
                    .update(cx, |pane, cx| {
                        let vt = match pane._vt_processor.as_ref() {
                            Some(vt) => vt,
                            None => return (true, false),
                        };
                        let needs_render = vt.take_render_needed();
                        let is_exited = vt.has_exited();

                        // Update progress bar state from VT processor
                        let new_progress = vt.progress();
                        if pane.progress != new_progress {
                            pane.progress = new_progress;
                            cx.notify();
                        }

                        if needs_render {
                            cx.notify();
                        }
                        if is_exited && !pane.exit_emitted {
                            pane.exit_emitted = true;
                            cx.emit(TerminalExitEvent);
                            return (true, needs_render);
                        }
                        (false, needs_render)
                    })
                    .unwrap_or((true, false));

                let (should_exit, had_data) = should_break;

                if had_data {
                    idle_count = 0;
                } else {
                    idle_count = idle_count.saturating_add(1);
                }

                if should_exit {
                    break;
                }
            }
        })
        .detach();

        Some(vt_processor)
    }

    /// Create a replay pane that plays back a .cast recording file.
    pub fn new_replay(
        cx: &mut Context<Self>,
        path: std::path::PathBuf,
    ) -> Result<Self, anyhow::Error> {
        let (header, events) = terminal::recording::parse_cast_file(&path)?;

        let display_state = DisplayState::default();

        let pty_arc: Arc<Mutex<Option<PtyHandler>>> = Arc::new(Mutex::new(None));
        let listener = Listener::new(pty_arc.clone());
        let config = Config::default();
        let size = TermSize {
            rows: header.height,
            cols: header.width,
        };
        let term = Term::new(config, &size, listener.clone());
        let term = Arc::new(Mutex::new(term));
        let processor: Arc<Mutex<Processor>> = Arc::new(Mutex::new(Processor::new()));

        let focus_handle = cx.focus_handle().tab_stop(false);
        let user_config = settings::load_config();
        let font_fallbacks = if user_config.font_fallbacks.is_empty() {
            None
        } else {
            Some(FontFallbacks::from_fonts(
                user_config.font_fallbacks.clone(),
            ))
        };

        let replay = ReplayState::new(events);

        // Start playback timer
        let term_clone = term.clone();
        let processor_clone = processor.clone();
        cx.spawn(async move |this, cx| {
            const TICK_MS: u64 = 16; // ~60fps
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(TICK_MS))
                    .await;

                let should_stop = this
                    .update(cx, |pane, cx| {
                        let replay = match pane.replay.as_mut() {
                            Some(r) => r,
                            None => return true,
                        };
                        if !replay.playing || replay.is_finished() {
                            return replay.is_finished();
                        }

                        let delta = (TICK_MS as f64 / 1000.0) * replay.speed as f64;
                        replay.position += delta;

                        let mut fed_data = false;
                        while replay.current_index < replay.events.len() {
                            let event = &replay.events[replay.current_index];
                            if event.timestamp > replay.position {
                                break;
                            }
                            {
                                let mut term_guard = term_clone.lock();
                                let mut proc_guard = processor_clone.lock();
                                proc_guard.advance(&mut *term_guard, &event.data);
                            }
                            replay.current_index += 1;
                            fed_data = true;
                        }

                        if fed_data {
                            cx.notify();
                        }
                        false
                    })
                    .unwrap_or(true);

                if should_stop {
                    break;
                }
            }
        })
        .detach();

        Ok(Self {
            pty: pty_arc,
            term,
            listener,
            display: Arc::new(RwLock::new(display_state)),
            dragging: false,
            focus_handle,
            exit_emitted: false,
            search: SearchState::new(),
            hovered_url: None,
            font_fallbacks,
            scroll_reverse: user_config.scroll_reverse,
            copy_mode: CopyModeState::new(size.rows as usize, size.cols as usize),
            _vt_processor: None,
            progress: ProgressState::default(),
            replay: Some(replay),
        })
    }

    /// Whether this pane is in replay mode.
    pub fn is_replay(&self) -> bool {
        self.replay.is_some()
    }

    /// Toggle replay play/pause.
    pub fn toggle_replay_playback(&mut self) {
        if let Some(replay) = self.replay.as_mut() {
            replay.toggle_play();
        }
    }

    /// Adjust replay speed.
    pub fn set_replay_speed(&mut self, speed: f32) {
        if let Some(replay) = self.replay.as_mut() {
            replay.set_speed(speed);
        }
    }

    /// Get the current replay progress fraction (0.0 - 1.0).
    pub fn replay_progress(&self) -> f32 {
        self.replay
            .as_ref()
            .map(|r| r.progress_fraction())
            .unwrap_or(0.0)
    }

    /// Seek replay to a fraction (0.0..=1.0) of total duration.
    /// Replays all events from the beginning up to the target position.
    fn seek_replay(&mut self, fraction: f32) {
        let replay = match self.replay.as_mut() {
            Some(r) => r,
            None => return,
        };
        replay.seek_fraction(fraction);
        let target_index = replay.current_index;

        // Rebuild terminal from scratch and replay events up to the target
        {
            let display = self.display.read();
            let size = TermSize {
                cols: display.size.cols,
                rows: display.size.rows,
            };
            drop(display);

            let config = Config::default();
            let new_term = Term::new(config, &size, self.listener.clone());
            let mut term_guard = self.term.lock();
            *term_guard = new_term;

            let mut processor: Processor = Processor::new();
            for event in &replay.events[..target_index] {
                processor.advance(&mut *term_guard, &event.data);
            }
        }
    }

    /// Send keyboard input to the PTY.
    ///
    /// If the write fails (e.g., broken pipe because the process exited),
    /// the PTY handler is dropped so subsequent operations treat it as exited.
    pub fn send_input(&mut self, input: &str) {
        let mut pty_guard = self.pty.lock();
        if let Some(ref mut pty) = *pty_guard {
            if let Err(e) = pty.write(input.as_bytes()) {
                tracing::warn!(
                    error = %e,
                    input_len = input.len(),
                    "PTY write failed, shell process likely exited"
                );
                *pty_guard = None;
            }
        }
    }

    /// Check if the shell has exited
    pub fn has_exited(&self) -> bool {
        if self.replay.is_some() {
            return false;
        }
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

    /// Get the current working directory of the terminal's foreground process
    pub fn get_current_directory(&self) -> Option<std::path::PathBuf> {
        let pty_guard = self.pty.lock();
        match &*pty_guard {
            None => None,
            Some(pty) => pty.get_current_directory(),
        }
    }

    /// Get the name of the shell being used (e.g., "zsh", "bash").
    pub fn shell_name(&self) -> Option<String> {
        std::env::var("SHELL").ok().and_then(|s| {
            std::path::Path::new(&s)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        })
    }

    /// Get the current progress bar state (from OSC 9;4 sequences).
    pub fn progress(&self) -> ProgressState {
        self.progress
    }

    /// Start recording the current session to an asciinema v2 .cast file.
    pub fn start_recording(&mut self) -> Result<(), anyhow::Error> {
        if self.is_recording() {
            return Ok(());
        }
        let display = self.display.read();
        let recorder =
            terminal::recording::SessionRecorder::new(display.size.cols, display.size.rows)?;
        if let Some(ref vt) = self._vt_processor {
            *vt.recorder().lock() = Some(recorder);
        }
        Ok(())
    }

    /// Stop recording the current session.
    pub fn stop_recording(&mut self) {
        if let Some(ref vt) = self._vt_processor {
            let mut recorder_guard = vt.recorder().lock();
            if let Some(mut recorder) = recorder_guard.take() {
                if let Err(error) = recorder.finish() {
                    tracing::warn!("Failed to stop recording: {}", error);
                }
            }
        }
    }

    /// Whether a recording is currently active.
    pub fn is_recording(&self) -> bool {
        self._vt_processor
            .as_ref()
            .is_some_and(|vt| vt.recorder().lock().as_ref().is_some_and(|r| r.is_active()))
    }

    /// Get the current badge state for this pane.
    pub fn badge(&self) -> TabBadge {
        let mut pty_guard = self.pty.lock();
        match &mut *pty_guard {
            None => TabBadge::Success,
            Some(handler) => match handler.exit_code() {
                Some(0) => TabBadge::Success,
                Some(code) => TabBadge::Failed(code),
                None => TabBadge::Running,
            },
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

    /// Get the current working directory reported by the shell via OSC 7.
    ///
    /// Falls back to the OS-level `PtyHandler::get_current_directory()` when
    /// OSC 7 data is not available (which is the current state, since
    /// alacritty_terminal/vte do not parse OSC 7).
    pub fn current_working_directory(&self) -> Option<std::path::PathBuf> {
        // Prefer OSC 7 value if available (will be populated once we add a pre-parser).
        if let Some(cwd) = self.listener.cwd.lock().as_ref() {
            return Some(std::path::PathBuf::from(cwd));
        }
        // Fallback: query the OS for the foreground process CWD.
        self.get_current_directory()
    }

    /// Get the line number of the most recent shell prompt (OSC 133;A).
    ///
    /// Returns `None` until a custom pre-parser is added to intercept OSC 133
    /// sequences from raw PTY output.
    pub fn last_prompt_line(&self) -> Option<i32> {
        *self.listener.last_prompt_line.lock()
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

    /// Find the column span (start_col, end_col) of the URL at a given column position.
    /// Returns `None` if the column is not within a URL.
    fn find_url_span_at_position(line: &str, col: usize) -> Option<(usize, usize)> {
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

                // Check if column is within this URL
                if col >= url_start && col < url_end {
                    return Some((url_start, url_end));
                }

                search_start = url_end;
            }
        }
        None
    }

    /// Extract URL at the given column position from a line of text.
    /// Returns the URL string if the column is within a URL boundary.
    fn find_url_at_position(line: &str, col: usize) -> Option<String> {
        let (start, end) = Self::find_url_span_at_position(line, col)?;
        Some(line.chars().skip(start).take(end - start).collect())
    }

    /// Extract text content from a visual terminal row (accounting for scroll).
    /// `visual_row` is 0 = top of viewport, not the grid line number.
    fn get_row_text(&self, visual_row: usize) -> String {
        let term = self.term.lock();
        let grid = term.grid();
        let display_offset = grid.display_offset() as i32;

        // Convert visual row to grid line (accounting for scroll)
        let line = Line(visual_row as i32 - display_offset);

        // Check bounds: grid supports negative lines for scrollback
        // The valid range is roughly -history_size to screen_lines-1
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
    fn handle_mouse_down(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let Some((col, row)) = self.pixel_to_cell(event.position) else {
            return;
        };

        // Handle Cmd+Click for URL opening (before other handlers)
        if event.modifiers.platform && event.button == MouseButton::Left {
            let line_text = self.get_row_text(row);
            if let Some(url) = Self::find_url_at_position(&line_text, col) {
                // Open URL in default browser (fire-and-forget, blocking is fine)
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

            let seq = Self::encode_mouse_event(
                button,
                col,
                row,
                mode.contains(TermMode::SGR_MOUSE),
                false,
            );
            self.send_input(seq.as_str());
        } else if event.button == MouseButton::Left {
            // Start text selection using alacritty's Selection
            // Convert visual row to grid line (accounting for scroll offset)
            let mut term = self.term.lock();
            let display_offset = term.grid().display_offset() as i32;
            let line = Line(row as i32 - display_offset);
            let point = TermPoint::new(line, Column(col));

            // Cmd+Option (macOS) or Alt (other) triggers rectangular/block selection
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
            // End text selection
            if self.dragging {
                // Convert visual row to grid line (accounting for scroll offset)
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
    fn handle_mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        // Update URL hover state (Cmd held = show URL underline)
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

        // Report drag/motion events to the terminal when requested
        if self.dragging
            && mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION | TermMode::MOUSE_MODE)
        {
            // Drag events use button code + 32 (left drag = 32, middle = 33, right = 34)
            let seq = Self::encode_mouse_event(
                32, // left button drag
                col,
                row,
                mode.contains(TermMode::SGR_MOUSE),
                false,
            );
            self.send_input(seq.as_str());
        } else if mode.contains(TermMode::MOUSE_MOTION) && !self.dragging {
            // Motion mode (1003): report all mouse movements, even without buttons
            let seq = Self::encode_mouse_event(
                35, // no button (motion only)
                col,
                row,
                mode.contains(TermMode::SGR_MOUSE),
                false,
            );
            self.send_input(seq.as_str());
        } else if self.dragging {
            // Update text selection if dragging without mouse reporting
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
    fn handle_scroll(&mut self, event: &ScrollWheelEvent) {
        let Some((col, row)) = self.pixel_to_cell(event.position) else {
            return;
        };

        let mode = {
            let term = self.term.lock();
            *term.mode()
        };

        let (_, cell_height) = self.display.read().cell_dims;

        // Compute scroll delta, reversing direction for "natural" scrolling if configured
        let raw_delta_y: f32 = event.delta.pixel_delta(px(cell_height)).y.into();
        let delta_y = if self.scroll_reverse {
            -raw_delta_y
        } else {
            raw_delta_y
        };

        // If mouse reporting is enabled, send wheel events
        if mode.intersects(
            TermMode::MOUSE_REPORT_CLICK
                | TermMode::MOUSE_DRAG
                | TermMode::MOUSE_MOTION
                | TermMode::MOUSE_MODE,
        ) {
            let button = if delta_y < 0.0 { 64 } else { 65 }; // 64 = wheel up, 65 = wheel down
            let seq = Self::encode_mouse_event(
                button,
                col,
                row,
                mode.contains(TermMode::SGR_MOUSE),
                false,
            );
            self.send_input(seq.as_str());
        } else if mode.contains(TermMode::ALT_SCREEN) {
            // In alternate screen without mouse mode, send arrow keys for scrolling
            let lines = (delta_y.abs() / cell_height).ceil() as usize;
            let key = if delta_y < 0.0 { "\x1b[A" } else { "\x1b[B" }; // Up or Down

            for _ in 0..lines.min(5) {
                self.send_input(key);
            }
        } else {
            // Normal mode: scroll through terminal history (scrollback buffer)
            let lines = (delta_y.abs() / cell_height).ceil() as i32;

            if lines > 0 {
                // GPUI: delta_y < 0 = scroll up gesture, delta_y > 0 = scroll down
                // Scroll::Delta(positive) = scroll viewport up (show older content)
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
    ///
    /// Button codes follow the xterm protocol:
    /// - 0=left, 1=middle, 2=right (press/release)
    /// - 32/33/34=left/middle/right drag
    /// - 64=wheel up, 65=wheel down
    fn encode_mouse_event(
        button: u8,
        col: usize,
        row: usize,
        sgr_mode: bool,
        release: bool,
    ) -> MouseEscBuf {
        let mut buf = MouseEscBuf::new();
        if sgr_mode {
            // SGR 1006 format: ESC [ < button ; col ; row M/m
            // SGR supports arbitrarily large coordinates (no 255 limit)
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
            // Legacy X11 format: ESC [ M Cb Cx Cy
            // All values are encoded as single bytes with +32 offset.
            // Coordinates are 1-based and capped at 223 (255 - 32) to fit in a byte.
            let cb: u8 = if release {
                35
            } else {
                button.saturating_add(32)
            };
            let cx = (col.min(222) as u8).saturating_add(33); // (col+1)+32, max 255
            let cy = (row.min(222) as u8).saturating_add(33); // (row+1)+32, max 255
            let _ = write!(buf, "\x1b[M{}{}{}", cb as char, cx as char, cy as char);
        }
        buf
    }

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

    fn find_matches(&mut self) {
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
    fn toggle_search(&mut self, cx: &mut Context<Self>) {
        self.search.active = !self.search.active;
        if !self.search.active {
            self.search.query.clear();
            self.search.matches.clear();
            self.search.compiled_regex = None;
        }
        cx.notify();
    }

    fn toggle_regex(&mut self, cx: &mut Context<Self>) {
        self.search.regex_mode = !self.search.regex_mode;
        self.search.recompile_regex();
        self.find_matches();
        cx.notify();
    }

    fn search_next(&mut self, cx: &mut Context<Self>) {
        if !self.search.matches.is_empty() {
            self.search.current_match = (self.search.current_match + 1) % self.search.matches.len();
            self.scroll_to_match(cx);
        }
    }

    /// Move to the previous match.
    fn search_prev(&mut self, cx: &mut Context<Self>) {
        if !self.search.matches.is_empty() {
            self.search.current_match = if self.search.current_match == 0 {
                self.search.matches.len() - 1
            } else {
                self.search.current_match - 1
            };
            self.scroll_to_match(cx);
        }
    }

    fn enter_copy_mode(&mut self, cx: &mut Context<Self>) {
        if self.copy_mode.active {
            return;
        }
        let term = self.term.lock();
        let cursor = term.grid().cursor.point;
        let display_offset = term.grid().display_offset();
        let rows = term.grid().screen_lines();
        let cols = term.grid().columns();
        drop(term);
        self.copy_mode.update_dimensions(rows, cols);
        // Place cursor at the terminal cursor's visible position
        let visual_row = (cursor.line.0 + display_offset as i32).max(0) as usize;
        self.copy_mode.enter(visual_row, cursor.column.0);
        cx.notify();
    }

    fn exit_copy_mode(&mut self, cx: &mut Context<Self>) {
        self.copy_mode.cancel();
        cx.notify();
    }

    fn handle_copy_mode_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let mods = &event.keystroke.modifiers;

        match key {
            "escape" => self.copy_mode.cancel(),
            "h" if !mods.control => self.copy_mode.move_left(),
            "j" if !mods.control => self.copy_mode.move_down(),
            "k" if !mods.control => self.copy_mode.move_up(),
            "l" if !mods.control => self.copy_mode.move_right(),
            "0" => self.copy_mode.move_to_line_start(),
            "4" if mods.shift => self.copy_mode.move_to_line_end(), // $
            "g" if !mods.control => self.copy_mode.move_to_top(),   // gg (simplified)
            "g" if mods.shift => self.copy_mode.move_to_bottom(),   // G
            "u" if mods.control => {
                let page = self.copy_mode.grid_rows / 2;
                self.copy_mode.page_up(page);
            }
            "d" if mods.control => {
                let page = self.copy_mode.grid_rows / 2;
                self.copy_mode.page_down(page);
            }
            "v" if !mods.control && !mods.shift => {
                self.copy_mode
                    .toggle_selection_type(crate::copy_mode::CopyModeSelection::Character);
            }
            "v" if mods.shift => {
                self.copy_mode
                    .toggle_selection_type(crate::copy_mode::CopyModeSelection::Line);
            }
            "v" if mods.control => {
                self.copy_mode
                    .toggle_selection_type(crate::copy_mode::CopyModeSelection::Block);
            }
            "y" if !mods.control => {
                // Yank selected text to clipboard
                let grid = self.build_copy_mode_grid();
                let text = self.copy_mode.extract_text(&grid);
                if !text.is_empty() {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                }
                self.copy_mode.cancel();
            }
            _ => {} // Ignore unbound keys
        }
        cx.notify();
    }

    /// Build a grid of chars from the terminal for copy mode text extraction.
    fn build_copy_mode_grid(&self) -> Vec<Vec<char>> {
        let term = self.term.lock();
        let rows = term.grid().screen_lines();
        let cols = term.grid().columns();
        let display_offset = term.grid().display_offset();
        let mut grid = Vec::with_capacity(rows);
        for row_idx in 0..rows {
            let line = alacritty_terminal::index::Line(row_idx as i32 - display_offset as i32);
            let mut row = Vec::with_capacity(cols);
            for col_idx in 0..cols {
                let col = alacritty_terminal::index::Column(col_idx);
                row.push(term.grid()[line][col].c);
            }
            grid.push(row);
        }
        grid
    }

    /// Scroll the terminal to make the current match visible.
    fn scroll_to_match(&mut self, cx: &mut Context<Self>) {
        if let Some(&(line, _, _)) = self.search.matches.get(self.search.current_match) {
            let mut term = self.term.lock();
            let screen_lines = term.grid().screen_lines() as i32;
            let display_offset = term.grid().display_offset() as i32;
            // Visible lines in grid coordinates: from -display_offset to -display_offset + screen_lines - 1
            let visible_top = -display_offset;
            let visible_bottom = visible_top + screen_lines - 1;
            if line < visible_top || line > visible_bottom {
                // Scroll so match is centered in viewport.
                // display_offset = -(line - screen_lines / 2) means
                // the visible top is at line - screen_lines/2
                let target_offset = -(line - screen_lines / 2);
                let max_offset = (term.grid().total_lines() as i32 - screen_lines).max(0);
                let clamped = target_offset.max(0).min(max_offset);
                // Use scroll_display to set the offset: first go to top, then delta down
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

    /// Handle a key event using termwiz for escape sequence encoding
    fn handle_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
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

    /// Get selected text from terminal using alacritty's selection
    fn get_selected_text(&self) -> Option<String> {
        // Use alacritty_terminal's built-in selection_to_string which properly
        // handles all selection types and iterates through the full grid (including scrollback)
        let term = self.term.lock();
        term.selection_to_string()
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

    /// Handle Cmd+Shift+Arrow for line-level selection
    fn handle_cmd_shift_arrow(&mut self, direction: &str, cx: &mut Context<Self>) {
        let mut term = self.term.lock();
        let cols = term.columns();

        // Get current selection or start from cursor position
        // For keyboard selection, always anchor at the cursor, not a mouse position
        let content = term.renderable_content();
        let cursor = content.cursor.point;
        let (start_point, current_end) = if let Some(sel_range) = content.selection {
            // If selection start matches cursor, extend from there
            // Otherwise start fresh from cursor (user clicked elsewhere with mouse)
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

        // Get current selection or start from cursor position
        // For keyboard selection, always anchor at the cursor, not a mouse position
        let content = term.renderable_content();
        let cursor = content.cursor.point;
        let (start_point, current_end) = if let Some(sel_range) = content.selection {
            // If selection start matches cursor, extend from there
            // Otherwise start fresh from cursor (user clicked elsewhere with mouse)
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

        // Get current selection or start from cursor position
        // For keyboard selection, always anchor at the cursor, not a mouse position
        let content = term.renderable_content();
        let cursor = content.cursor.point;
        let (start_point, current_end) = if let Some(sel_range) = content.selection {
            let sel_start = TermPoint::new(sel_range.start.line, sel_range.start.column);
            // Only extend existing selection if it started at cursor
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

    /// Handle dropped files - pastes file paths for AI assistants to read directly
    fn handle_file_drop(&mut self, paths: &ExternalPaths, cx: &mut Context<Self>) {
        let paths = paths.paths();
        if paths.is_empty() {
            return;
        }

        let mut output = String::new();

        for path in paths {
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

    // Use terminal's actual dimensions for capacity hints and bounds checks
    let term_cols = term.columns();
    let term_rows = term.screen_lines();

    // Pre-allocate with capacity estimates based on terminal dimensions:
    // - cells: ~1/3 of grid tends to be non-space characters
    // - bg_regions: typically a few per row at most
    let estimated_cells = (term_rows * term_cols) / 3;
    let estimated_bg_regions = term_rows * 2;
    let mut cells: Vec<RenderCell> = Vec::with_capacity(estimated_cells);
    let mut bg_regions: Vec<BgRegion> = Vec::with_capacity(estimated_bg_regions);

    // Track current background region for on-the-fly merging (avoids grid allocation)
    // (row, col_start, col_end, color)
    let mut current_bg: Option<(usize, usize, usize, Hsla)> = None;

    // Get cursor info with shape
    let cursor_line = content.cursor.point.line.0;
    let cursor_col = content.cursor.point.column.0;
    let cursor_shape = content.cursor.shape;
    let display_offset = content.display_offset as i32;

    // Convert cursor line to visual row (accounting for scroll position)
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
        // Cursor is scrolled off screen
        None
    };

    // Process cells from terminal content
    for cell in content.display_iter {
        let point = cell.point;
        // Convert line number to visual row: when scrolled, line numbers can be negative
        // Visual row = line + display_offset (e.g., Line(-5) + offset 5 = row 0)
        let row = (point.line.0 + display_offset) as usize;
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

        // Get font family from theme (user-configurable) with fallback to default
        let font_family: SharedString = cx.theme().font_family.clone();

        // Get current font size and check if cell dimensions need recalculation
        let current_font_size = display_state.font_size;
        let font_size_bits = current_font_size.to_bits();
        let needs_recalc = match &display_state.cached_font_key {
            Some((cached_bits, ref cached_family)) => {
                *cached_bits != font_size_bits || *cached_family != font_family
            }
            None => true,
        };
        drop(display_state); // Release read lock before potential write

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

        // Clone data needed for canvas callbacks (resize happens in prepaint with actual bounds)
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
                // When in replay mode, handle replay-specific keys
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

                // When copy mode is active, route keys to copy mode handler
                if this.copy_mode.active {
                    this.handle_copy_mode_key(event, cx);
                    return;
                }

                // When search is active, route typing to search query
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
                    // If it's a printable character (single char, no modifiers except shift)
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
                    // Space key
                    if key == "space" {
                        this.search.query.push(' ');
                        this.find_matches();
                        cx.notify();
                        return;
                    }
                    return; // Consume all other keys while search is active
                }

                // Handle keys that GPUI might intercept for focus/navigation
                // These must be caught early before GPUI consumes them
                let key = event.keystroke.key.as_str();
                let mods = &event.keystroke.modifiers;

                match key {
                    // Tab/Shift+Tab - GPUI uses for focus navigation
                    "tab" => {
                        if mods.shift {
                            this.send_input("\x1b[Z"); // Shift-Tab (backtab)
                        } else {
                            this.send_input("\t");
                        }
                        return;
                    }
                    // Escape - GPUI might use for closing dialogs
                    "escape" => {
                        this.send_input("\x1b");
                        return;
                    }
                    // Enter - GPUI might use for form submission
                    "enter" => {
                        if mods.shift {
                            this.send_input("\x1b[13;2u"); // CSI u: Shift+Enter
                        } else if !mods.control && !mods.alt {
                            this.send_input("\r");
                        } else {
                            // Ctrl+Enter, Alt+Enter etc. -> let handle_key encode
                            this.handle_key(event, cx);
                        }
                        return;
                    }
                    // Backspace - handle Shift+Backspace as DEL
                    "backspace" if mods.shift && !mods.control && !mods.alt && !mods.platform => {
                        this.send_input("\x7f"); // DEL
                        return;
                    }
                    // Space - GPUI might use for button activation
                    "space" if mods.shift && !mods.control && !mods.alt && !mods.platform => {
                        this.send_input("\x1b[32;2u"); // CSI u: Shift+Space
                        return;
                    }
                    "space" if !mods.control && !mods.alt && !mods.platform => {
                        this.send_input(" ");
                        return;
                    }
                    _ => {}
                }

                // Cmd+, to open config file - handled by workspace layer
                // (The workspace crate will bind this action)
                if mods.platform && key == "," {
                    // TODO: This will be wired up via the workspace crate's action binding.
                    // For now, try to open the config file directly via settings.
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
                cx.notify(); // Redraw after scrolling
            }))
            .on_drop(cx.listener(|this, paths: &ExternalPaths, _window, cx| {
                this.handle_file_drop(paths, cx);
            }))
            .size_full()
            .bg(bg_color)
            // Search bar overlay (rendered above terminal content)
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
            // Replay control bar overlay (rendered at bottom when in replay mode)
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
                            // Progress track (clickable for seeking)
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
                                // Play/pause button
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
                                // Speed down button
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
                                // Speed label
                                .child(
                                    div()
                                        .text_size(px(11.0))
                                        .text_color(hsla(0.55, 0.5, 0.7, 1.0))
                                        .child(speed_label),
                                )
                                // Speed up button
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
                                // Time label
                                .child(
                                    div()
                                        .flex_1()
                                        .text_size(px(11.0))
                                        .text_color(hsla(0.0, 0.0, 0.6, 1.0))
                                        .child(time_label),
                                )
                                // Progress percentage
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
                // Clone search state and hover state for the canvas closure
                let search_matches = self.search.matches.clone();
                let hovered_url = self.hovered_url;
                let search_current = self.search.current_match;
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

                            // Resize PTY (pass pixel dimensions for proper rendering support)
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
                            font_family_clone.clone(),
                        );
                        // Get selection from renderable content (already normalized)
                        let selection_range = term_guard.renderable_content().selection;
                        // Get display offset for converting selection coordinates to visual rows
                        let display_offset = term_guard.grid().display_offset() as i32;
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
                    // Paint: draw backgrounds and cell-by-cell text
                    move |_bounds, data, window, cx| {
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
                                // SelectionRange uses absolute line coordinates
                                // Convert to visual rows by adding display_offset
                                let start_line = sel.start.line.0;
                                let end_line = sel.end.line.0;

                                // Convert to visual rows (like we do for cell rendering)
                                let start_visual = start_line + display_offset;
                                let end_visual = end_line + display_offset;

                                // Clamp to visible viewport (0..rows)
                                let visible_start_row = start_visual.max(0) as usize;
                                let visible_end_row =
                                    (end_visual.max(0) as usize).min(rows.saturating_sub(1));

                                // Only render if any part is visible
                                if visible_start_row <= visible_end_row
                                    && end_visual >= 0
                                    && start_visual < rows as i32
                                {
                                    let start_col = sel.start.column.0;
                                    let end_col = sel.end.column.0;

                                    for row in visible_start_row..=visible_end_row {
                                        let (col_start, col_end) = if sel.is_block {
                                            // Block/rectangular: same column range on every row
                                            (start_col, end_col + 1)
                                        } else {
                                            // Normal: first row partial, middle rows full, last row partial
                                            let cs =
                                                if row == visible_start_row && start_visual >= 0 {
                                                    start_col
                                                } else {
                                                    0
                                                };
                                            let ce = if row == visible_end_row
                                                && end_visual == row as i32
                                            {
                                                end_col + 1
                                            } else {
                                                cols
                                            };
                                            (cs, ce)
                                        };

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

                        // 1.75. Highlight search matches (paint colored overlay rectangles)
                        if !search_matches.is_empty() {
                            for (idx, &(match_line, start_col, end_col)) in
                                search_matches.iter().enumerate()
                            {
                                // Convert grid line to visual row
                                let visual_row = match_line + display_offset;
                                if visual_row < 0 || visual_row >= rows as i32 {
                                    continue;
                                }
                                let row = visual_row as usize;
                                let x = origin.x + px(PADDING + start_col as f32 * cell_width);
                                let y = origin.y + px(PADDING + row as f32 * cell_height);
                                let w = (end_col - start_col) as f32 * cell_width;

                                let highlight_color = if idx == search_current {
                                    hsla(0.14, 0.9, 0.5, 0.6) // Orange for current match
                                } else {
                                    hsla(0.14, 0.9, 0.5, 0.25) // Dim orange for other matches
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

                        // 1.9. Paint URL hover underline
                        if let Some((hover_row, hover_start, hover_end)) = hovered_url {
                            if hover_row < rows {
                                let x = origin.x + px(PADDING + hover_start as f32 * cell_width);
                                let y = origin.y
                                    + px(PADDING + (hover_row as f32 + 1.0) * cell_height - 1.0);
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

                        // 2. Paint each cell at its exact grid position.
                        // Per-cell positioning guarantees cursor-text alignment since
                        // both use the same coordinate: col * cell_width.
                        //
                        // Ligatures are handled by shaping small runs of adjacent
                        // same-style characters together (so the shaper can substitute
                        // ligature glyphs), but positioning each run at its starting
                        // cell's grid coordinate. This limits any advance drift to
                        // within a single run while enabling ligatures like => -> !=.
                        let font_size_px = px(font_size);

                        // Pre-build font variants to avoid per-cell Font construction
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

                        // Shape consecutive same-row, same-style, adjacent cells as
                        // small runs. Each run is positioned at its first cell's grid
                        // coordinate, so cursor alignment is never lost. The shaper
                        // sees 2+ adjacent characters and can form ligatures.
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

                            // Extend run while cells are adjacent, same row, same style
                            while i < cells.len() {
                                let cell = &cells[i];
                                let expected_next =
                                    if cells[i - 1].flags.contains(CellFlags::WIDE_CHAR) {
                                        run_end_col + 2
                                    } else {
                                        run_end_col + 1
                                    };
                                if cell.row != run_row
                                    || cell.col != expected_next
                                    || cell.fg != run_fg
                                    || cell.flags.intersection(CellFlags::BOLD | CellFlags::ITALIC)
                                        != run_flags
                                            .intersection(CellFlags::BOLD | CellFlags::ITALIC)
                                {
                                    break;
                                }
                                run_text.push(cell.c);
                                run_end_col = cell.col;
                                i += 1;
                            }

                            // Shape and paint this run at its grid position
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

                        // 5. Paint progress bar at bottom of terminal area (OSC 9;4)
                        if progress_state.is_visible() {
                            let bar_height = px(3.0);
                            let bar_y = bounds.origin.y + bounds.size.height - bar_height;
                            let total_width: f32 = bounds.size.width.into();

                            let (bar_color, bar_width) = match progress_state {
                                ProgressState::Normal(pct) => {
                                    // Green
                                    let color = hsla(0.33, 0.8, 0.45, 1.0);
                                    let width = total_width * (pct as f32 / 100.0);
                                    (color, width)
                                }
                                ProgressState::Error(pct) => {
                                    // Red
                                    let color = hsla(0.0, 0.8, 0.45, 1.0);
                                    let width = total_width * (pct as f32 / 100.0);
                                    (color, width)
                                }
                                ProgressState::Paused(pct) => {
                                    // Yellow
                                    let color = hsla(0.13, 0.8, 0.50, 1.0);
                                    let width = total_width * (pct as f32 / 100.0);
                                    (color, width)
                                }
                                ProgressState::Indeterminate => {
                                    // Dim full-width bar as MVP
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
                    },
                )
                .size_full()
            })
    }
}

impl Focusable for TerminalPane {
    fn focus_handle(&self, _cx: &gpui::App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[cfg(test)]
#[allow(clippy::unnecessary_literal_unwrap)]
#[path = "pane_tests.rs"]
mod tests;
