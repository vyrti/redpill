use std::borrow::Cow;

use alacritty_terminal::term::TermMode;
use gpui::Keystroke;

#[derive(Debug, PartialEq, Eq)]
enum Modifiers {
    None,
    Alt,
    Ctrl,
    Shift,
    CtrlShift,
    AltShift,
    CtrlAlt,
    CtrlAltShift,
    Other,
}

impl Modifiers {
    fn from_keystroke(ks: &Keystroke) -> Self {
        match (
            ks.modifiers.alt,
            ks.modifiers.control,
            ks.modifiers.shift,
            ks.modifiers.platform,
        ) {
            (false, false, false, false) => Modifiers::None,
            (true, false, false, false) => Modifiers::Alt,
            (false, true, false, false) => Modifiers::Ctrl,
            (false, false, true, false) => Modifiers::Shift,
            (false, true, true, false) => Modifiers::CtrlShift,
            (true, false, true, false) => Modifiers::AltShift,
            (true, true, false, false) => Modifiers::CtrlAlt,
            (true, true, true, false) => Modifiers::CtrlAltShift,
            _ => Modifiers::Other,
        }
    }

    fn has_any(&self) -> bool {
        !matches!(self, Modifiers::None)
    }
}

/// Convert a keystroke to terminal escape sequence
/// This function is terminal mode aware - arrow keys and other special keys
/// send different sequences depending on whether APP_CURSOR mode is active
pub fn keystroke_to_escape(
    keystroke: &Keystroke,
    mode: &TermMode,
    option_as_meta: bool,
) -> Option<Cow<'static, str>> {
    // Debug logging for key events
    eprintln!("[KEY] key='{}' modifiers={:?}", keystroke.key, keystroke.modifiers);

    let modifiers = Modifiers::from_keystroke(keystroke);

    // Handle special keys with specific modifier combinations
    let special_key_result: Option<&'static str> = match (keystroke.key.as_ref(), &modifiers) {
        // Basic keys
        ("tab", Modifiers::None) => Some("\x09"),
        ("tab", Modifiers::Shift) => Some("\x1b[Z"),
        ("escape", Modifiers::None) => Some("\x1b"),
        ("enter", Modifiers::None) => Some("\x0d"),
        ("enter", Modifiers::Shift) => Some("\x0a"),
        ("enter", Modifiers::Alt) => Some("\x1b\x0d"),
        ("backspace", Modifiers::None) => Some("\x7f"),
        ("backspace", Modifiers::Ctrl) => Some("\x08"),
        ("backspace", Modifiers::Alt) => Some("\x1b\x7f"),
        ("backspace", Modifiers::Shift) => Some("\x7f"),
        ("space", Modifiers::Ctrl) => Some("\x00"),

        // Shift + navigation keys when in alt screen mode (vim, less, etc.)
        ("home", Modifiers::Shift) if mode.contains(TermMode::ALT_SCREEN) => Some("\x1b[1;2H"),
        ("end", Modifiers::Shift) if mode.contains(TermMode::ALT_SCREEN) => Some("\x1b[1;2F"),
        ("pageup", Modifiers::Shift) if mode.contains(TermMode::ALT_SCREEN) => Some("\x1b[5;2~"),
        ("pagedown", Modifiers::Shift) if mode.contains(TermMode::ALT_SCREEN) => Some("\x1b[6;2~"),

        // Home/End - different in APP_CURSOR mode
        ("home", Modifiers::None) if mode.contains(TermMode::APP_CURSOR) => Some("\x1bOH"),
        ("home", Modifiers::None) => Some("\x1b[H"),
        ("end", Modifiers::None) if mode.contains(TermMode::APP_CURSOR) => Some("\x1bOF"),
        ("end", Modifiers::None) => Some("\x1b[F"),

        // Arrow keys - different in APP_CURSOR mode (critical for vim, etc.)
        ("up", Modifiers::None) if mode.contains(TermMode::APP_CURSOR) => Some("\x1bOA"),
        ("up", Modifiers::None) => Some("\x1b[A"),
        ("down", Modifiers::None) if mode.contains(TermMode::APP_CURSOR) => Some("\x1bOB"),
        ("down", Modifiers::None) => Some("\x1b[B"),
        ("right", Modifiers::None) if mode.contains(TermMode::APP_CURSOR) => Some("\x1bOC"),
        ("right", Modifiers::None) => Some("\x1b[C"),
        ("left", Modifiers::None) if mode.contains(TermMode::APP_CURSOR) => Some("\x1bOD"),
        ("left", Modifiers::None) => Some("\x1b[D"),

        // Other navigation keys
        ("insert", Modifiers::None) => Some("\x1b[2~"),
        ("delete", Modifiers::None) => Some("\x1b[3~"),
        ("pageup", Modifiers::None) => Some("\x1b[5~"),
        ("pagedown", Modifiers::None) => Some("\x1b[6~"),

        // Function keys
        ("f1", Modifiers::None) => Some("\x1bOP"),
        ("f2", Modifiers::None) => Some("\x1bOQ"),
        ("f3", Modifiers::None) => Some("\x1bOR"),
        ("f4", Modifiers::None) => Some("\x1bOS"),
        ("f5", Modifiers::None) => Some("\x1b[15~"),
        ("f6", Modifiers::None) => Some("\x1b[17~"),
        ("f7", Modifiers::None) => Some("\x1b[18~"),
        ("f8", Modifiers::None) => Some("\x1b[19~"),
        ("f9", Modifiers::None) => Some("\x1b[20~"),
        ("f10", Modifiers::None) => Some("\x1b[21~"),
        ("f11", Modifiers::None) => Some("\x1b[23~"),
        ("f12", Modifiers::None) => Some("\x1b[24~"),
        ("f13", Modifiers::None) => Some("\x1b[25~"),
        ("f14", Modifiers::None) => Some("\x1b[26~"),
        ("f15", Modifiers::None) => Some("\x1b[28~"),
        ("f16", Modifiers::None) => Some("\x1b[29~"),
        ("f17", Modifiers::None) => Some("\x1b[31~"),
        ("f18", Modifiers::None) => Some("\x1b[32~"),
        ("f19", Modifiers::None) => Some("\x1b[33~"),
        ("f20", Modifiers::None) => Some("\x1b[34~"),

        // Ctrl+letter combinations (caret notation)
        ("a", Modifiers::Ctrl) | ("A", Modifiers::CtrlShift) => Some("\x01"),
        ("b", Modifiers::Ctrl) | ("B", Modifiers::CtrlShift) => Some("\x02"),
        ("c", Modifiers::Ctrl) | ("C", Modifiers::CtrlShift) => Some("\x03"),
        ("d", Modifiers::Ctrl) | ("D", Modifiers::CtrlShift) => Some("\x04"),
        ("e", Modifiers::Ctrl) | ("E", Modifiers::CtrlShift) => Some("\x05"),
        ("f", Modifiers::Ctrl) | ("F", Modifiers::CtrlShift) => Some("\x06"),
        ("g", Modifiers::Ctrl) | ("G", Modifiers::CtrlShift) => Some("\x07"),
        ("h", Modifiers::Ctrl) | ("H", Modifiers::CtrlShift) => Some("\x08"),
        ("i", Modifiers::Ctrl) | ("I", Modifiers::CtrlShift) => Some("\x09"),
        ("j", Modifiers::Ctrl) | ("J", Modifiers::CtrlShift) => Some("\x0a"),
        ("k", Modifiers::Ctrl) | ("K", Modifiers::CtrlShift) => Some("\x0b"),
        ("l", Modifiers::Ctrl) | ("L", Modifiers::CtrlShift) => Some("\x0c"),
        ("m", Modifiers::Ctrl) | ("M", Modifiers::CtrlShift) => Some("\x0d"),
        ("n", Modifiers::Ctrl) | ("N", Modifiers::CtrlShift) => Some("\x0e"),
        ("o", Modifiers::Ctrl) | ("O", Modifiers::CtrlShift) => Some("\x0f"),
        ("p", Modifiers::Ctrl) | ("P", Modifiers::CtrlShift) => Some("\x10"),
        ("q", Modifiers::Ctrl) | ("Q", Modifiers::CtrlShift) => Some("\x11"),
        ("r", Modifiers::Ctrl) | ("R", Modifiers::CtrlShift) => Some("\x12"),
        ("s", Modifiers::Ctrl) | ("S", Modifiers::CtrlShift) => Some("\x13"),
        ("t", Modifiers::Ctrl) | ("T", Modifiers::CtrlShift) => Some("\x14"),
        ("u", Modifiers::Ctrl) | ("U", Modifiers::CtrlShift) => Some("\x15"),
        ("v", Modifiers::Ctrl) | ("V", Modifiers::CtrlShift) => Some("\x16"),
        ("w", Modifiers::Ctrl) | ("W", Modifiers::CtrlShift) => Some("\x17"),
        ("x", Modifiers::Ctrl) | ("X", Modifiers::CtrlShift) => Some("\x18"),
        ("y", Modifiers::Ctrl) | ("Y", Modifiers::CtrlShift) => Some("\x19"),
        ("z", Modifiers::Ctrl) | ("Z", Modifiers::CtrlShift) => Some("\x1a"),

        // Ctrl+special characters
        ("@", Modifiers::Ctrl) => Some("\x00"),
        ("[", Modifiers::Ctrl) => Some("\x1b"),
        ("\\", Modifiers::Ctrl) => Some("\x1c"),
        ("]", Modifiers::Ctrl) => Some("\x1d"),
        ("^", Modifiers::Ctrl) => Some("\x1e"),
        ("_", Modifiers::Ctrl) => Some("\x1f"),
        ("?", Modifiers::Ctrl) => Some("\x7f"),

        _ => None,
    };

    if let Some(esc_str) = special_key_result {
        return Some(Cow::Borrowed(esc_str));
    }

    // Handle modifier combinations for navigation/function keys
    if modifiers.has_any() {
        let modifier_code = compute_modifier_code(keystroke);
        let modified_result = match keystroke.key.as_ref() {
            "up" => Some(format!("\x1b[1;{}A", modifier_code)),
            "down" => Some(format!("\x1b[1;{}B", modifier_code)),
            "right" => Some(format!("\x1b[1;{}C", modifier_code)),
            "left" => Some(format!("\x1b[1;{}D", modifier_code)),
            "f1" => Some(format!("\x1b[1;{}P", modifier_code)),
            "f2" => Some(format!("\x1b[1;{}Q", modifier_code)),
            "f3" => Some(format!("\x1b[1;{}R", modifier_code)),
            "f4" => Some(format!("\x1b[1;{}S", modifier_code)),
            "f5" => Some(format!("\x1b[15;{}~", modifier_code)),
            "f6" => Some(format!("\x1b[17;{}~", modifier_code)),
            "f7" => Some(format!("\x1b[18;{}~", modifier_code)),
            "f8" => Some(format!("\x1b[19;{}~", modifier_code)),
            "f9" => Some(format!("\x1b[20;{}~", modifier_code)),
            "f10" => Some(format!("\x1b[21;{}~", modifier_code)),
            "f11" => Some(format!("\x1b[23;{}~", modifier_code)),
            "f12" => Some(format!("\x1b[24;{}~", modifier_code)),
            "f13" => Some(format!("\x1b[25;{}~", modifier_code)),
            "f14" => Some(format!("\x1b[26;{}~", modifier_code)),
            "f15" => Some(format!("\x1b[28;{}~", modifier_code)),
            "f16" => Some(format!("\x1b[29;{}~", modifier_code)),
            "f17" => Some(format!("\x1b[31;{}~", modifier_code)),
            "f18" => Some(format!("\x1b[32;{}~", modifier_code)),
            "f19" => Some(format!("\x1b[33;{}~", modifier_code)),
            "f20" => Some(format!("\x1b[34;{}~", modifier_code)),
            _ if modifier_code == 2 => None, // Shift-only, don't apply for non-navigation keys
            "insert" => Some(format!("\x1b[2;{}~", modifier_code)),
            "delete" => Some(format!("\x1b[3;{}~", modifier_code)),
            "pageup" => Some(format!("\x1b[5;{}~", modifier_code)),
            "pagedown" => Some(format!("\x1b[6;{}~", modifier_code)),
            "end" => Some(format!("\x1b[1;{}F", modifier_code)),
            "home" => Some(format!("\x1b[1;{}H", modifier_code)),
            _ => None,
        };

        if let Some(esc_str) = modified_result {
            return Some(Cow::Owned(esc_str));
        }
    }

    // Handle Alt as meta key (sends ESC + character)
    if !cfg!(target_os = "macos") || option_as_meta {
        let is_alt_only = modifiers == Modifiers::Alt && keystroke.key.is_ascii();
        let is_alt_shift = keystroke.modifiers.alt && keystroke.modifiers.shift && keystroke.key.is_ascii();

        if is_alt_only || is_alt_shift {
            let key = if is_alt_shift {
                keystroke.key.to_ascii_uppercase()
            } else {
                keystroke.key.clone()
            };
            return Some(Cow::Owned(format!("\x1b{}", key)));
        }
    }

    None
}

/// Compute the modifier code for xterm-style escape sequences
/// Based on: https://invisible-island.net/xterm/ctlseqs/ctlseqs.html#h2-PC-Style-Function-Keys
///
///   Code     Modifiers
/// ---------+---------------------------
///    2     | Shift
///    3     | Alt
///    4     | Shift + Alt
///    5     | Control
///    6     | Shift + Control
///    7     | Alt + Control
///    8     | Shift + Alt + Control
fn compute_modifier_code(keystroke: &Keystroke) -> u32 {
    let mut code = 0u32;
    if keystroke.modifiers.shift {
        code |= 1;
    }
    if keystroke.modifiers.alt {
        code |= 2;
    }
    if keystroke.modifiers.control {
        code |= 4;
    }
    code + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::Modifiers as GpuiModifiers;

    fn make_keystroke(key: &str, ctrl: bool, alt: bool, shift: bool) -> Keystroke {
        Keystroke {
            modifiers: GpuiModifiers {
                control: ctrl,
                alt,
                shift,
                platform: false,
                function: false,
            },
            key: key.to_string(),
            key_char: None,
        }
    }

    #[test]
    fn test_arrow_keys_normal_mode() {
        let mode = TermMode::NONE;
        assert_eq!(
            keystroke_to_escape(&make_keystroke("up", false, false, false), &mode, false),
            Some(Cow::Borrowed("\x1b[A"))
        );
        assert_eq!(
            keystroke_to_escape(&make_keystroke("down", false, false, false), &mode, false),
            Some(Cow::Borrowed("\x1b[B"))
        );
        assert_eq!(
            keystroke_to_escape(&make_keystroke("right", false, false, false), &mode, false),
            Some(Cow::Borrowed("\x1b[C"))
        );
        assert_eq!(
            keystroke_to_escape(&make_keystroke("left", false, false, false), &mode, false),
            Some(Cow::Borrowed("\x1b[D"))
        );
    }

    #[test]
    fn test_arrow_keys_app_cursor_mode() {
        let mode = TermMode::APP_CURSOR;
        assert_eq!(
            keystroke_to_escape(&make_keystroke("up", false, false, false), &mode, false),
            Some(Cow::Borrowed("\x1bOA"))
        );
        assert_eq!(
            keystroke_to_escape(&make_keystroke("down", false, false, false), &mode, false),
            Some(Cow::Borrowed("\x1bOB"))
        );
        assert_eq!(
            keystroke_to_escape(&make_keystroke("right", false, false, false), &mode, false),
            Some(Cow::Borrowed("\x1bOC"))
        );
        assert_eq!(
            keystroke_to_escape(&make_keystroke("left", false, false, false), &mode, false),
            Some(Cow::Borrowed("\x1bOD"))
        );
    }

    #[test]
    fn test_ctrl_c() {
        let mode = TermMode::NONE;
        assert_eq!(
            keystroke_to_escape(&make_keystroke("c", true, false, false), &mode, false),
            Some(Cow::Borrowed("\x03"))
        );
    }

    #[test]
    fn test_enter_and_backspace() {
        let mode = TermMode::NONE;
        assert_eq!(
            keystroke_to_escape(&make_keystroke("enter", false, false, false), &mode, false),
            Some(Cow::Borrowed("\x0d"))
        );
        assert_eq!(
            keystroke_to_escape(&make_keystroke("backspace", false, false, false), &mode, false),
            Some(Cow::Borrowed("\x7f"))
        );
    }

    #[test]
    fn test_modifier_code() {
        assert_eq!(compute_modifier_code(&make_keystroke("a", false, false, true)), 2);  // Shift
        assert_eq!(compute_modifier_code(&make_keystroke("a", false, true, false)), 3);  // Alt
        assert_eq!(compute_modifier_code(&make_keystroke("a", false, true, true)), 4);   // Shift+Alt
        assert_eq!(compute_modifier_code(&make_keystroke("a", true, false, false)), 5);  // Ctrl
        assert_eq!(compute_modifier_code(&make_keystroke("a", true, false, true)), 6);   // Shift+Ctrl
        assert_eq!(compute_modifier_code(&make_keystroke("a", true, true, false)), 7);   // Alt+Ctrl
        assert_eq!(compute_modifier_code(&make_keystroke("a", true, true, true)), 8);    // Shift+Alt+Ctrl
    }
}
