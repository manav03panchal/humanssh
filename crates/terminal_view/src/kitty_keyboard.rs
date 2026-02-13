use alacritty_terminal::term::TermMode;
use gpui::{Keystroke, Modifiers};
use std::fmt::Write;

/// Kitty keyboard protocol progressive enhancement flags.
///
/// These correspond directly to the flags in the Kitty keyboard protocol spec
/// and to `alacritty_terminal::term::TermMode` kitty-related bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KittyFlags(u8);

impl KittyFlags {
    pub const NONE: Self = Self(0);
    pub const DISAMBIGUATE_ESCAPE_CODES: Self = Self(1);
    pub const REPORT_EVENT_TYPES: Self = Self(2);
    pub const REPORT_ALTERNATE_KEYS: Self = Self(4);
    pub const REPORT_ALL_KEYS_AS_ESCAPE_CODES: Self = Self(8);
    pub const REPORT_ASSOCIATED_TEXT: Self = Self(16);

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl std::ops::BitOr for KittyFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl From<TermMode> for KittyFlags {
    fn from(mode: TermMode) -> Self {
        let mut flags = Self::NONE;
        if mode.contains(TermMode::DISAMBIGUATE_ESC_CODES) {
            flags = flags | Self::DISAMBIGUATE_ESCAPE_CODES;
        }
        if mode.contains(TermMode::REPORT_EVENT_TYPES) {
            flags = flags | Self::REPORT_EVENT_TYPES;
        }
        if mode.contains(TermMode::REPORT_ALTERNATE_KEYS) {
            flags = flags | Self::REPORT_ALTERNATE_KEYS;
        }
        if mode.contains(TermMode::REPORT_ALL_KEYS_AS_ESC) {
            flags = flags | Self::REPORT_ALL_KEYS_AS_ESCAPE_CODES;
        }
        if mode.contains(TermMode::REPORT_ASSOCIATED_TEXT) {
            flags = flags | Self::REPORT_ASSOCIATED_TEXT;
        }
        flags
    }
}

/// Tracks whether the Kitty keyboard protocol is active and with which flags.
#[derive(Debug, Clone)]
pub struct KittyKeyboardState {
    pub flags: KittyFlags,
}

impl Default for KittyKeyboardState {
    fn default() -> Self {
        Self {
            flags: KittyFlags::NONE,
        }
    }
}

impl KittyKeyboardState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_active(&self) -> bool {
        !self.flags.is_empty()
    }

    pub fn update_from_mode(&mut self, mode: TermMode) {
        self.flags = KittyFlags::from(mode);
    }
}

/// Encodes a GPUI keystroke into a Kitty keyboard protocol escape sequence.
///
/// Returns `None` if the key cannot be mapped to a Kitty protocol encoding.
/// The encoding follows the CSI u format: `\x1b[{codepoint};{modifiers}u`
///
/// For keys that have legacy CSI encodings (arrow keys, function keys, etc.),
/// those legacy forms are used with modifier parameters when needed.
pub fn encode_key_event(keystroke: &Keystroke, flags: KittyFlags) -> Option<String> {
    if flags.is_empty() {
        return None;
    }

    let key = keystroke.key.as_str();
    let modifiers = &keystroke.modifiers;

    let modifier_value = encode_modifiers(modifiers);
    let report_all = flags.contains(KittyFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES);

    // Try functional keys first (these have special CSI encodings)
    if let Some(encoded) = encode_functional_key(key, modifier_value) {
        return Some(encoded);
    }

    // Try keys that use CSI u format
    if let Some(codepoint) = key_to_csi_u_codepoint(key, report_all) {
        return Some(format_csi_u(codepoint, modifier_value));
    }

    // Single printable character
    if let Some(character) = single_char(key) {
        let codepoint = character as u32;

        if modifier_value > 1 || report_all {
            return Some(format_csi_u(codepoint, modifier_value));
        }

        // Plain character with no modifiers: return as-is
        return Some(character.to_string());
    }

    None
}

/// Formats a CSI u escape sequence.
///
/// If modifier_value is 1 (no modifiers), omits the modifier parameter.
fn format_csi_u(codepoint: u32, modifier_value: u32) -> String {
    let mut result = String::with_capacity(16);
    result.push_str("\x1b[");
    let _ = write!(result, "{codepoint}");
    if modifier_value > 1 {
        let _ = write!(result, ";{modifier_value}");
    }
    result.push('u');
    result
}

/// Encodes GPUI modifiers into the Kitty modifier value (1 + bit flags).
///
/// Kitty modifier bits: shift=1, alt=2, ctrl=4, super=8.
/// The wire value is 1 + the sum of active modifier bits.
fn encode_modifiers(modifiers: &Modifiers) -> u32 {
    let mut bits: u32 = 0;
    if modifiers.shift {
        bits |= 1;
    }
    if modifiers.alt {
        bits |= 2;
    }
    if modifiers.control {
        bits |= 4;
    }
    if modifiers.platform {
        bits |= 8;
    }
    1 + bits
}

/// Extracts a single character from a GPUI key string.
fn single_char(key: &str) -> Option<char> {
    let mut chars = key.chars();
    let character = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    Some(character)
}

/// Maps a GPUI key name to a codepoint for CSI u encoding.
///
/// This covers keys that use the `CSI codepoint ; modifiers u` format
/// rather than legacy CSI sequences.
fn key_to_csi_u_codepoint(key: &str, report_all: bool) -> Option<u32> {
    match key {
        "escape" => Some(27),
        "space" => Some(32),

        // Enter, Tab, Backspace: in basic DISAMBIGUATE mode these send legacy
        // bytes. Under REPORT_ALL_KEYS_AS_ESCAPE_CODES they use CSI u.
        "enter" if report_all => Some(13),
        "tab" if report_all => Some(9),
        "backspace" if report_all => Some(127),

        // Keypad keys (Kitty Private Use Area codepoints)
        "kp0" => Some(57399),
        "kp1" => Some(57400),
        "kp2" => Some(57401),
        "kp3" => Some(57402),
        "kp4" => Some(57403),
        "kp5" => Some(57404),
        "kp6" => Some(57405),
        "kp7" => Some(57406),
        "kp8" => Some(57407),
        "kp9" => Some(57408),
        "kp_decimal" => Some(57409),
        "kp_divide" => Some(57410),
        "kp_multiply" => Some(57411),
        "kp_subtract" => Some(57412),
        "kp_add" => Some(57413),
        "kp_enter" => Some(57414),
        "kp_equal" => Some(57415),

        // Modifier keys (only sent under REPORT_ALL_KEYS_AS_ESCAPE_CODES)
        "shift" if report_all => Some(57441),
        "control" if report_all => Some(57443),
        "alt" if report_all => Some(57445),

        // Lock keys
        "caps_lock" => Some(57358),
        "scroll_lock" => Some(57359),
        "num_lock" => Some(57360),

        // Menu / context menu
        "menu" => Some(57363),

        _ => None,
    }
}

/// Encodes functional keys that use legacy CSI sequences with modifier parameters.
///
/// These keys keep their traditional encoding format but add modifier
/// parameters when modifiers are present. The formats are:
/// - Arrow keys: `CSI 1 ; modifiers {A-D}`
/// - Home/End: `CSI 1 ; modifiers {H,F}`
/// - Function keys: `CSI {number} ; modifiers ~`
/// - Insert/Delete/PageUp/PageDown: `CSI {number} ; modifiers ~`
fn encode_functional_key(key: &str, modifier_value: u32) -> Option<String> {
    // Arrow keys: CSI 1 ; modifiers {letter}
    let arrow_suffix = match key {
        "up" => Some('A'),
        "down" => Some('B'),
        "right" => Some('C'),
        "left" => Some('D'),
        "home" => Some('H'),
        "end" => Some('F'),
        _ => None,
    };

    if let Some(suffix) = arrow_suffix {
        let mut result = String::with_capacity(12);
        result.push_str("\x1b[1");
        if modifier_value > 1 {
            let _ = write!(result, ";{modifier_value}");
        }
        result.push(suffix);
        return Some(result);
    }

    // Keys using CSI {number} ; modifiers ~ format
    let tilde_number = match key {
        "insert" => Some(2),
        "delete" => Some(3),
        "pageup" => Some(5),
        "pagedown" => Some(6),
        "f1" => Some(11),
        "f2" => Some(12),
        "f3" => Some(13),
        "f4" => Some(14),
        "f5" => Some(15),
        "f6" => Some(17),
        "f7" => Some(18),
        "f8" => Some(19),
        "f9" => Some(20),
        "f10" => Some(21),
        "f11" => Some(23),
        "f12" => Some(24),
        _ => None,
    };

    if let Some(number) = tilde_number {
        let mut result = String::with_capacity(12);
        result.push_str("\x1b[");
        let _ = write!(result, "{number}");
        if modifier_value > 1 {
            let _ = write!(result, ";{modifier_value}");
        }
        result.push('~');
        return Some(result);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_keystroke(key: &str, modifiers: Modifiers) -> Keystroke {
        Keystroke {
            key: key.into(),
            modifiers,
            key_char: None,
        }
    }

    fn no_mods() -> Modifiers {
        Modifiers::default()
    }

    fn shift() -> Modifiers {
        Modifiers {
            shift: true,
            ..Default::default()
        }
    }

    fn ctrl() -> Modifiers {
        Modifiers {
            control: true,
            ..Default::default()
        }
    }

    fn alt() -> Modifiers {
        Modifiers {
            alt: true,
            ..Default::default()
        }
    }

    fn ctrl_shift() -> Modifiers {
        Modifiers {
            control: true,
            shift: true,
            ..Default::default()
        }
    }

    fn super_mod() -> Modifiers {
        Modifiers {
            platform: true,
            ..Default::default()
        }
    }

    // === KittyFlags tests ===

    #[test]
    fn flags_none_is_empty() {
        assert!(KittyFlags::NONE.is_empty());
    }

    #[test]
    fn flags_contains() {
        let flags = KittyFlags::DISAMBIGUATE_ESCAPE_CODES | KittyFlags::REPORT_EVENT_TYPES;
        assert!(flags.contains(KittyFlags::DISAMBIGUATE_ESCAPE_CODES));
        assert!(flags.contains(KittyFlags::REPORT_EVENT_TYPES));
        assert!(!flags.contains(KittyFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES));
    }

    #[test]
    fn flags_from_term_mode() {
        let mode = TermMode::DISAMBIGUATE_ESC_CODES | TermMode::REPORT_ALL_KEYS_AS_ESC;
        let flags = KittyFlags::from(mode);
        assert!(flags.contains(KittyFlags::DISAMBIGUATE_ESCAPE_CODES));
        assert!(flags.contains(KittyFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES));
        assert!(!flags.contains(KittyFlags::REPORT_EVENT_TYPES));
    }

    #[test]
    fn flags_from_term_mode_all() {
        let mode = TermMode::KITTY_KEYBOARD_PROTOCOL;
        let flags = KittyFlags::from(mode);
        assert!(flags.contains(KittyFlags::DISAMBIGUATE_ESCAPE_CODES));
        assert!(flags.contains(KittyFlags::REPORT_EVENT_TYPES));
        assert!(flags.contains(KittyFlags::REPORT_ALTERNATE_KEYS));
        assert!(flags.contains(KittyFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES));
        assert!(flags.contains(KittyFlags::REPORT_ASSOCIATED_TEXT));
    }

    // === KittyKeyboardState tests ===

    #[test]
    fn state_initially_inactive() {
        let state = KittyKeyboardState::new();
        assert!(!state.is_active());
    }

    #[test]
    fn state_active_after_update() {
        let mut state = KittyKeyboardState::new();
        state.update_from_mode(TermMode::DISAMBIGUATE_ESC_CODES);
        assert!(state.is_active());
    }

    // === Modifier encoding tests ===

    #[test]
    fn encode_no_modifiers() {
        assert_eq!(encode_modifiers(&no_mods()), 1);
    }

    #[test]
    fn encode_shift_modifier() {
        assert_eq!(encode_modifiers(&shift()), 2);
    }

    #[test]
    fn encode_alt_modifier() {
        assert_eq!(encode_modifiers(&alt()), 3);
    }

    #[test]
    fn encode_ctrl_modifier() {
        assert_eq!(encode_modifiers(&ctrl()), 5);
    }

    #[test]
    fn encode_super_modifier() {
        assert_eq!(encode_modifiers(&super_mod()), 9);
    }

    #[test]
    fn encode_ctrl_shift_modifiers() {
        assert_eq!(encode_modifiers(&ctrl_shift()), 6);
    }

    // === Returns None for empty flags ===

    #[test]
    fn returns_none_when_flags_empty() {
        let keystroke = make_keystroke("a", no_mods());
        assert_eq!(encode_key_event(&keystroke, KittyFlags::NONE), None);
    }

    // === Simple key encoding (letters) ===

    #[test]
    fn simple_letter_no_modifiers() {
        let keystroke = make_keystroke("a", no_mods());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "a");
    }

    #[test]
    fn simple_letter_z_no_modifiers() {
        let keystroke = make_keystroke("z", no_mods());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "z");
    }

    // === Modified letter keys ===

    #[test]
    fn ctrl_a_encoding() {
        let keystroke = make_keystroke("a", ctrl());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        // codepoint 97 (a), modifiers 5 (1 + ctrl=4)
        assert_eq!(result, "\x1b[97;5u");
    }

    #[test]
    fn alt_b_encoding() {
        let keystroke = make_keystroke("b", alt());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        // codepoint 98 (b), modifiers 3 (1 + alt=2)
        assert_eq!(result, "\x1b[98;3u");
    }

    #[test]
    fn ctrl_shift_c_encoding() {
        let keystroke = make_keystroke("c", ctrl_shift());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        // codepoint 99 (c), modifiers 6 (1 + ctrl=4 + shift=1)
        assert_eq!(result, "\x1b[99;6u");
    }

    #[test]
    fn super_d_encoding() {
        let keystroke = make_keystroke("d", super_mod());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        // codepoint 100 (d), modifiers 9 (1 + super=8)
        assert_eq!(result, "\x1b[100;9u");
    }

    // === Number keys ===

    #[test]
    fn digit_no_modifiers() {
        let keystroke = make_keystroke("5", no_mods());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "5");
    }

    #[test]
    fn ctrl_digit() {
        let keystroke = make_keystroke("3", ctrl());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[51;5u"); // '3' = codepoint 51
    }

    // === Special keys ===

    #[test]
    fn escape_key_disambiguate() {
        let keystroke = make_keystroke("escape", no_mods());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[27u");
    }

    #[test]
    fn escape_key_with_modifiers() {
        let keystroke = make_keystroke("escape", shift());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[27;2u");
    }

    #[test]
    fn space_no_modifiers() {
        let keystroke = make_keystroke("space", no_mods());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[32u");
    }

    #[test]
    fn ctrl_space() {
        let keystroke = make_keystroke("space", ctrl());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[32;5u");
    }

    // === Enter, Tab, Backspace under DISAMBIGUATE only (legacy bytes, not CSI u) ===

    #[test]
    fn enter_disambiguate_only_returns_none() {
        let keystroke = make_keystroke("enter", no_mods());
        let result = encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES);
        // Under basic DISAMBIGUATE, Enter/Tab/Backspace keep legacy encoding,
        // so we return None to let the caller fall through to legacy handling.
        assert_eq!(result, None);
    }

    #[test]
    fn tab_disambiguate_only_returns_none() {
        let keystroke = make_keystroke("tab", no_mods());
        let result = encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES);
        assert_eq!(result, None);
    }

    #[test]
    fn backspace_disambiguate_only_returns_none() {
        let keystroke = make_keystroke("backspace", no_mods());
        let result = encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES);
        assert_eq!(result, None);
    }

    // === Enter, Tab, Backspace under REPORT_ALL_KEYS (CSI u encoding) ===

    #[test]
    fn enter_report_all_keys() {
        let keystroke = make_keystroke("enter", no_mods());
        let flags =
            KittyFlags::DISAMBIGUATE_ESCAPE_CODES | KittyFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES;
        let result = encode_key_event(&keystroke, flags).expect("encoded");
        assert_eq!(result, "\x1b[13u");
    }

    #[test]
    fn tab_report_all_keys() {
        let keystroke = make_keystroke("tab", no_mods());
        let flags =
            KittyFlags::DISAMBIGUATE_ESCAPE_CODES | KittyFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES;
        let result = encode_key_event(&keystroke, flags).expect("encoded");
        assert_eq!(result, "\x1b[9u");
    }

    #[test]
    fn backspace_report_all_keys() {
        let keystroke = make_keystroke("backspace", no_mods());
        let flags =
            KittyFlags::DISAMBIGUATE_ESCAPE_CODES | KittyFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES;
        let result = encode_key_event(&keystroke, flags).expect("encoded");
        assert_eq!(result, "\x1b[127u");
    }

    #[test]
    fn shift_tab_report_all_keys() {
        let keystroke = make_keystroke("tab", shift());
        let flags =
            KittyFlags::DISAMBIGUATE_ESCAPE_CODES | KittyFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES;
        let result = encode_key_event(&keystroke, flags).expect("encoded");
        assert_eq!(result, "\x1b[9;2u");
    }

    // === Arrow keys ===

    #[test]
    fn arrow_up_no_modifiers() {
        let keystroke = make_keystroke("up", no_mods());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[1A");
    }

    #[test]
    fn arrow_down_no_modifiers() {
        let keystroke = make_keystroke("down", no_mods());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[1B");
    }

    #[test]
    fn arrow_right_no_modifiers() {
        let keystroke = make_keystroke("right", no_mods());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[1C");
    }

    #[test]
    fn arrow_left_no_modifiers() {
        let keystroke = make_keystroke("left", no_mods());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[1D");
    }

    #[test]
    fn shift_arrow_up() {
        let keystroke = make_keystroke("up", shift());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[1;2A");
    }

    #[test]
    fn ctrl_arrow_left() {
        let keystroke = make_keystroke("left", ctrl());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[1;5D");
    }

    #[test]
    fn alt_arrow_right() {
        let keystroke = make_keystroke("right", alt());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[1;3C");
    }

    // === Home / End ===

    #[test]
    fn home_no_modifiers() {
        let keystroke = make_keystroke("home", no_mods());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[1H");
    }

    #[test]
    fn end_no_modifiers() {
        let keystroke = make_keystroke("end", no_mods());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[1F");
    }

    #[test]
    fn shift_home() {
        let keystroke = make_keystroke("home", shift());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[1;2H");
    }

    #[test]
    fn shift_end() {
        let keystroke = make_keystroke("end", shift());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[1;2F");
    }

    // === Insert, Delete, PageUp, PageDown ===

    #[test]
    fn insert_no_modifiers() {
        let keystroke = make_keystroke("insert", no_mods());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[2~");
    }

    #[test]
    fn delete_no_modifiers() {
        let keystroke = make_keystroke("delete", no_mods());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[3~");
    }

    #[test]
    fn pageup_no_modifiers() {
        let keystroke = make_keystroke("pageup", no_mods());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[5~");
    }

    #[test]
    fn pagedown_no_modifiers() {
        let keystroke = make_keystroke("pagedown", no_mods());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[6~");
    }

    #[test]
    fn ctrl_delete() {
        let keystroke = make_keystroke("delete", ctrl());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[3;5~");
    }

    #[test]
    fn shift_insert() {
        let keystroke = make_keystroke("insert", shift());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[2;2~");
    }

    // === Function keys ===

    #[test]
    fn f1_no_modifiers() {
        let keystroke = make_keystroke("f1", no_mods());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[11~");
    }

    #[test]
    fn f5_no_modifiers() {
        let keystroke = make_keystroke("f5", no_mods());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[15~");
    }

    #[test]
    fn f12_no_modifiers() {
        let keystroke = make_keystroke("f12", no_mods());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[24~");
    }

    #[test]
    fn shift_f1() {
        let keystroke = make_keystroke("f1", shift());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[11;2~");
    }

    #[test]
    fn ctrl_f5() {
        let keystroke = make_keystroke("f5", ctrl());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[15;5~");
    }

    #[test]
    fn alt_f12() {
        let keystroke = make_keystroke("f12", alt());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[24;3~");
    }

    // === All function keys codepoint verification ===

    #[test]
    fn function_keys_all_codepoints() {
        let expected = [
            ("f1", 11),
            ("f2", 12),
            ("f3", 13),
            ("f4", 14),
            ("f5", 15),
            ("f6", 17),
            ("f7", 18),
            ("f8", 19),
            ("f9", 20),
            ("f10", 21),
            ("f11", 23),
            ("f12", 24),
        ];
        for (key_name, number) in expected {
            let keystroke = make_keystroke(key_name, no_mods());
            let result = encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES)
                .unwrap_or_else(|| panic!("failed to encode {key_name}"));
            assert_eq!(
                result,
                format!("\x1b[{number}~"),
                "wrong encoding for {key_name}"
            );
        }
    }

    // === Report all keys mode: plain letters get CSI u ===

    #[test]
    fn report_all_keys_plain_letter() {
        let keystroke = make_keystroke("a", no_mods());
        let flags =
            KittyFlags::DISAMBIGUATE_ESCAPE_CODES | KittyFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES;
        let result = encode_key_event(&keystroke, flags).expect("encoded");
        assert_eq!(result, "\x1b[97u");
    }

    #[test]
    fn report_all_keys_digit() {
        let keystroke = make_keystroke("0", no_mods());
        let flags =
            KittyFlags::DISAMBIGUATE_ESCAPE_CODES | KittyFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES;
        let result = encode_key_event(&keystroke, flags).expect("encoded");
        assert_eq!(result, "\x1b[48u");
    }

    // === Unknown key returns None ===

    #[test]
    fn unknown_key_returns_none() {
        let keystroke = make_keystroke("some_unknown_key", no_mods());
        let result = encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES);
        assert_eq!(result, None);
    }

    // === Punctuation and symbols ===

    #[test]
    fn semicolon_no_modifiers() {
        let keystroke = make_keystroke(";", no_mods());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, ";");
    }

    #[test]
    fn ctrl_semicolon() {
        let keystroke = make_keystroke(";", ctrl());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        // ';' = codepoint 59
        assert_eq!(result, "\x1b[59;5u");
    }

    #[test]
    fn bracket_no_modifiers() {
        let keystroke = make_keystroke("[", no_mods());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "[");
    }

    #[test]
    fn ctrl_bracket() {
        let keystroke = make_keystroke("[", ctrl());
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        // '[' = codepoint 91
        assert_eq!(result, "\x1b[91;5u");
    }

    // === Combined modifier flags ===

    #[test]
    fn all_modifiers_combined() {
        let modifiers = Modifiers {
            shift: true,
            alt: true,
            control: true,
            platform: true,
            ..Default::default()
        };
        // shift=1 + alt=2 + ctrl=4 + super=8 = 15; wire value = 16
        assert_eq!(encode_modifiers(&modifiers), 16);

        let keystroke = make_keystroke("a", modifiers);
        let result =
            encode_key_event(&keystroke, KittyFlags::DISAMBIGUATE_ESCAPE_CODES).expect("encoded");
        assert_eq!(result, "\x1b[97;16u");
    }
}
