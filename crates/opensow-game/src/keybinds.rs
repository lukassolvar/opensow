//! Rebindable keys (not in the exe): every mission hotkey of docs/hotkeys.md is an [`Act`] with two key slots, stored in
//! `opensow.cfg` as `key_<name>=<code>,<code>` lines. Keys are physical (winit [`KeyCode`]), like the exe's DirectInput scan
//! codes, so a binding does not move with the keyboard layout. The mouse buttons past the left and right one (middle, back,
//! forward, others) can be bound too. Esc (the menu) and the left / right mouse buttons are fixed. Notes:
//! docs/hotkeys.md, docs/options.md ("Controls").

use winit::event::MouseButton;
use winit::keyboard::{Key, KeyCode, NamedKey};

/// What a binding slot holds: a physical key or a mouse button (not the left or right one).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Input {
    Key(KeyCode),
    Mouse(MouseButton),
}

impl From<KeyCode> for Input {
    fn from(code: KeyCode) -> Input {
        Input::Key(code)
    }
}

impl From<MouseButton> for Input {
    fn from(b: MouseButton) -> Input {
        Input::Mouse(b)
    }
}

/// Something a key does in a mission. The order is the order of the rows on the Controls pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Act {
    ScrollUp,
    ScrollDown,
    ScrollLeft,
    ScrollRight,
    LookAtHq,
    LookAtEvent,
    CentreSelection,
    /// Camera bookmark 1..=9 (index 0..=8); with Shift it is set.
    Bookmark(u8),
    NextUnit,
    /// The next Advancer; with Shift the next idle one.
    NextAdvancer,
    SelectVisible,
    SelectVisibleAntiAir,
    SelectVisibleGround,
    SelectAll,
    Scatter,
    RallyOrRepair,
    /// Turret tier 1..=5 (index 0..=4).
    Turret(u8),
    StrikeBomber,
    StrikeAsteroid,
    StrikeCarryall,
    StrikeTripler,
    StrikeFighter,
    /// Buys every tech-level upgrade the research points pay for (`World::tech_up_all`, like the exe's AI `0x42f490`).
    TechUp,
    Pause,
    Chat,
    KeyHelp,
    Abandon,
    SpeedDown,
    SpeedUp,
    /// Quick save; with Shift quick load.
    QuickSave,
    Fullscreen,
}

/// Every action in row order.
pub const ALL: [Act; 43] = [
    Act::ScrollUp,
    Act::ScrollDown,
    Act::ScrollLeft,
    Act::ScrollRight,
    Act::LookAtHq,
    Act::LookAtEvent,
    Act::CentreSelection,
    Act::Bookmark(0),
    Act::Bookmark(1),
    Act::Bookmark(2),
    Act::Bookmark(3),
    Act::Bookmark(4),
    Act::Bookmark(5),
    Act::Bookmark(6),
    Act::Bookmark(7),
    Act::Bookmark(8),
    Act::NextUnit,
    Act::NextAdvancer,
    Act::SelectVisible,
    Act::SelectVisibleAntiAir,
    Act::SelectVisibleGround,
    Act::SelectAll,
    Act::Scatter,
    Act::RallyOrRepair,
    Act::Turret(0),
    Act::Turret(1),
    Act::Turret(2),
    Act::Turret(3),
    Act::Turret(4),
    Act::StrikeBomber,
    Act::StrikeAsteroid,
    Act::StrikeCarryall,
    Act::StrikeTripler,
    Act::StrikeFighter,
    Act::TechUp,
    Act::Pause,
    Act::Chat,
    Act::KeyHelp,
    Act::Abandon,
    Act::SpeedDown,
    Act::SpeedUp,
    Act::QuickSave,
    Act::Fullscreen,
];

/// Number of actions.
pub const COUNT: usize = ALL.len();

/// Rows of each Controls page (ranges into [`ALL`]): camera, selection and orders, the rest.
pub const PAGE_ROWS: [std::ops::Range<usize>; 3] = [0..16, 16..29, 29..COUNT];

impl Act {
    /// Row index in [`ALL`].
    pub fn index(self) -> usize {
        ALL.iter().position(|&a| a == self).unwrap_or(0)
    }

    /// The name in `opensow.cfg` (after `key_`).
    pub fn cfg_name(self) -> String {
        match self {
            Act::ScrollUp => "scroll_up".into(),
            Act::ScrollDown => "scroll_down".into(),
            Act::ScrollLeft => "scroll_left".into(),
            Act::ScrollRight => "scroll_right".into(),
            Act::LookAtHq => "look_at_hq".into(),
            Act::LookAtEvent => "look_at_event".into(),
            Act::CentreSelection => "centre_selection".into(),
            Act::Bookmark(k) => format!("bookmark_{}", k + 1),
            Act::NextUnit => "next_unit".into(),
            Act::NextAdvancer => "next_advancer".into(),
            Act::SelectVisible => "select_visible".into(),
            Act::SelectVisibleAntiAir => "select_visible_anti_air".into(),
            Act::SelectVisibleGround => "select_visible_ground".into(),
            Act::SelectAll => "select_all".into(),
            Act::Scatter => "scatter".into(),
            Act::RallyOrRepair => "rally_or_repair".into(),
            Act::Turret(k) => format!("turret_{}", k + 1),
            Act::StrikeBomber => "strike_bomber".into(),
            Act::StrikeAsteroid => "strike_asteroid".into(),
            Act::StrikeCarryall => "strike_carryall".into(),
            Act::StrikeTripler => "strike_tripler".into(),
            Act::StrikeFighter => "strike_fighter".into(),
            Act::TechUp => "tech_up".into(),
            Act::Pause => "pause".into(),
            Act::Chat => "chat".into(),
            Act::KeyHelp => "key_help".into(),
            Act::Abandon => "abandon_mission".into(),
            Act::SpeedDown => "speed_down".into(),
            Act::SpeedUp => "speed_up".into(),
            Act::QuickSave => "quick_save".into(),
            Act::Fullscreen => "fullscreen".into(),
        }
    }

    /// The keys of the original (docs/hotkeys.md) and of this port's own shortcuts.
    pub fn defaults(self) -> [Option<Input>; 2] {
        use KeyCode as K;
        const NUM: [KeyCode; 9] = [K::Numpad1, K::Numpad2, K::Numpad3, K::Numpad4, K::Numpad5, K::Numpad6, K::Numpad7, K::Numpad8, K::Numpad9];
        const TOP: [KeyCode; 5] = [K::Digit1, K::Digit2, K::Digit3, K::Digit4, K::Digit5];
        let (a, b) = match self {
            Act::ScrollUp => (K::ArrowUp, Some(K::KeyW)),
            Act::ScrollDown => (K::ArrowDown, Some(K::KeyS)),
            Act::ScrollLeft => (K::ArrowLeft, Some(K::KeyA)),
            Act::ScrollRight => (K::ArrowRight, Some(K::KeyD)),
            Act::LookAtHq => (K::Backspace, None),
            Act::LookAtEvent => (K::Space, Some(K::Numpad0)),
            Act::CentreSelection => (K::NumpadEnter, None),
            Act::Bookmark(k) => (NUM[k as usize % 9], None),
            Act::NextUnit => (K::Tab, None),
            Act::NextAdvancer => (K::KeyH, Some(K::NumpadAdd)),
            Act::SelectVisible => (K::NumpadMultiply, None),
            Act::SelectVisibleAntiAir => (K::NumpadDivide, None),
            Act::SelectVisibleGround => (K::NumpadSubtract, None),
            Act::SelectAll => (K::KeyQ, None),
            Act::Scatter => (K::KeyX, None),
            Act::RallyOrRepair => (K::KeyR, None),
            Act::Turret(k) => (TOP[k as usize % 5], None),
            Act::StrikeBomber => (K::KeyB, None),
            Act::StrikeAsteroid => (K::KeyM, None),
            Act::StrikeCarryall => (K::KeyC, None),
            Act::StrikeTripler => (K::KeyY, None),
            Act::StrikeFighter => (K::KeyF, None),
            Act::TechUp => (K::KeyT, None),
            Act::Pause => (K::KeyP, None),
            Act::Chat => (K::Enter, None),
            Act::KeyHelp => (K::F1, None),
            Act::Abandon => (K::F2, None),
            Act::SpeedDown => (K::F3, None),
            Act::SpeedUp => (K::F4, None),
            Act::QuickSave => (K::F5, None),
            Act::Fullscreen => (K::F11, None),
        };
        [Some(Input::Key(a)), b.map(Input::Key)]
    }
}

/// The keys that can be bound: (code, name in `opensow.cfg` = winit's name, name shown on the Controls pages).
const KEYS: &[(KeyCode, &str, &str)] = {
    use KeyCode as K;
    &[
        (K::KeyA, "KeyA", "A"),
        (K::KeyB, "KeyB", "B"),
        (K::KeyC, "KeyC", "C"),
        (K::KeyD, "KeyD", "D"),
        (K::KeyE, "KeyE", "E"),
        (K::KeyF, "KeyF", "F"),
        (K::KeyG, "KeyG", "G"),
        (K::KeyH, "KeyH", "H"),
        (K::KeyI, "KeyI", "I"),
        (K::KeyJ, "KeyJ", "J"),
        (K::KeyK, "KeyK", "K"),
        (K::KeyL, "KeyL", "L"),
        (K::KeyM, "KeyM", "M"),
        (K::KeyN, "KeyN", "N"),
        (K::KeyO, "KeyO", "O"),
        (K::KeyP, "KeyP", "P"),
        (K::KeyQ, "KeyQ", "Q"),
        (K::KeyR, "KeyR", "R"),
        (K::KeyS, "KeyS", "S"),
        (K::KeyT, "KeyT", "T"),
        (K::KeyU, "KeyU", "U"),
        (K::KeyV, "KeyV", "V"),
        (K::KeyW, "KeyW", "W"),
        (K::KeyX, "KeyX", "X"),
        (K::KeyY, "KeyY", "Y"),
        (K::KeyZ, "KeyZ", "Z"),
        (K::Digit0, "Digit0", "0"),
        (K::Digit1, "Digit1", "1"),
        (K::Digit2, "Digit2", "2"),
        (K::Digit3, "Digit3", "3"),
        (K::Digit4, "Digit4", "4"),
        (K::Digit5, "Digit5", "5"),
        (K::Digit6, "Digit6", "6"),
        (K::Digit7, "Digit7", "7"),
        (K::Digit8, "Digit8", "8"),
        (K::Digit9, "Digit9", "9"),
        (K::Numpad0, "Numpad0", "Num 0"),
        (K::Numpad1, "Numpad1", "Num 1"),
        (K::Numpad2, "Numpad2", "Num 2"),
        (K::Numpad3, "Numpad3", "Num 3"),
        (K::Numpad4, "Numpad4", "Num 4"),
        (K::Numpad5, "Numpad5", "Num 5"),
        (K::Numpad6, "Numpad6", "Num 6"),
        (K::Numpad7, "Numpad7", "Num 7"),
        (K::Numpad8, "Numpad8", "Num 8"),
        (K::Numpad9, "Numpad9", "Num 9"),
        (K::NumpadAdd, "NumpadAdd", "Num +"),
        (K::NumpadSubtract, "NumpadSubtract", "Num -"),
        (K::NumpadMultiply, "NumpadMultiply", "Num *"),
        (K::NumpadDivide, "NumpadDivide", "Num /"),
        (K::NumpadDecimal, "NumpadDecimal", "Num ."),
        (K::NumpadEnter, "NumpadEnter", "Num Enter"),
        (K::F1, "F1", "F1"),
        (K::F2, "F2", "F2"),
        (K::F3, "F3", "F3"),
        (K::F4, "F4", "F4"),
        (K::F5, "F5", "F5"),
        (K::F6, "F6", "F6"),
        (K::F7, "F7", "F7"),
        (K::F8, "F8", "F8"),
        (K::F9, "F9", "F9"),
        (K::F10, "F10", "F10"),
        (K::F11, "F11", "F11"),
        (K::F12, "F12", "F12"),
        (K::ArrowUp, "ArrowUp", "Up"),
        (K::ArrowDown, "ArrowDown", "Down"),
        (K::ArrowLeft, "ArrowLeft", "Left"),
        (K::ArrowRight, "ArrowRight", "Right"),
        (K::Space, "Space", "Space"),
        (K::Tab, "Tab", "Tab"),
        (K::Enter, "Enter", "Enter"),
        (K::Backspace, "Backspace", "Backspace"),
        (K::Insert, "Insert", "Insert"),
        (K::Delete, "Delete", "Delete"),
        (K::Home, "Home", "Home"),
        (K::End, "End", "End"),
        (K::PageUp, "PageUp", "Page Up"),
        (K::PageDown, "PageDown", "Page Down"),
        (K::CapsLock, "CapsLock", "Caps Lock"),
        (K::Minus, "Minus", "-"),
        (K::Equal, "Equal", "="),
        (K::BracketLeft, "BracketLeft", "["),
        (K::BracketRight, "BracketRight", "]"),
        (K::Backslash, "Backslash", "\\"),
        (K::IntlBackslash, "IntlBackslash", "< >"),
        (K::Semicolon, "Semicolon", ";"),
        (K::Quote, "Quote", "'"),
        (K::Backquote, "Backquote", "`"),
        (K::Comma, "Comma", ","),
        (K::Period, "Period", "."),
        (K::Slash, "Slash", "/"),
    ]
};

/// The name of a key or button on the Controls pages (US layout names for keys, which are the physical ones; mouse buttons by
/// number: middle 3, back 4, forward 5).
pub fn key_name(input: Input) -> String {
    match input {
        Input::Key(code) => KEYS.iter().find(|k| k.0 == code).map_or("?", |k| k.2).to_owned(),
        Input::Mouse(b) => format!("Mouse {}", mouse_number(b)),
    }
}

/// The usual number of a mouse button (left 1, right 2, middle 3, back 4, forward 5; winit's `Other(n)` counts from 0).
fn mouse_number(b: MouseButton) -> u32 {
    match b {
        MouseButton::Left => 1,
        MouseButton::Right => 2,
        MouseButton::Middle => 3,
        MouseButton::Back => 4,
        MouseButton::Forward => 5,
        MouseButton::Other(n) => n as u32 + 1,
    }
}

fn from_cfg(name: &str) -> Option<Input> {
    let mouse = match name {
        "MouseMiddle" => Some(MouseButton::Middle),
        "MouseBack" => Some(MouseButton::Back),
        "MouseForward" => Some(MouseButton::Forward),
        _ => name.strip_prefix("MouseOther").and_then(|n| n.parse().ok()).map(MouseButton::Other),
    };
    mouse.map(Input::Mouse).or_else(|| KEYS.iter().find(|k| k.1 == name).map(|k| Input::Key(k.0)))
}

fn to_cfg(input: Input) -> String {
    match input {
        Input::Key(code) => KEYS.iter().find(|k| k.0 == code).map_or("", |k| k.1).to_owned(),
        Input::Mouse(MouseButton::Middle) => "MouseMiddle".to_owned(),
        Input::Mouse(MouseButton::Back) => "MouseBack".to_owned(),
        Input::Mouse(MouseButton::Forward) => "MouseForward".to_owned(),
        Input::Mouse(MouseButton::Other(n)) => format!("MouseOther{n}"),
        Input::Mouse(MouseButton::Left | MouseButton::Right) => String::new(),
    }
}

/// Whether a key or button may be bound: a key of [`KEYS`] that is not fixed, or a mouse button other than left and right
/// (they select and command). Esc always opens the menu; F12 and the backquote open the dev panel in development builds.
/// Shift, Control, Alt and the system keys are modifiers and never in [`KEYS`].
pub fn bindable(input: impl Into<Input>) -> bool {
    match input.into() {
        Input::Key(code) => {
            let dev_key = crate::DEV && matches!(code, KeyCode::F12 | KeyCode::Backquote);
            !dev_key && KEYS.iter().any(|k| k.0 == code)
        }
        Input::Mouse(b) => !matches!(b, MouseButton::Left | MouseButton::Right),
    }
}

/// The physical key of a logical one, for key events that come without one (tests, headless runs): `digit` is the
/// physical digit when known, `numpad` says the key is on the keypad.
pub fn code_of(key: &Key, numpad: bool, digit: Option<u8>) -> Option<KeyCode> {
    use KeyCode as K;
    if let Some(d) = digit.filter(|&d| d <= 9) {
        let top = [K::Digit0, K::Digit1, K::Digit2, K::Digit3, K::Digit4, K::Digit5, K::Digit6, K::Digit7, K::Digit8, K::Digit9];
        let pad = [K::Numpad0, K::Numpad1, K::Numpad2, K::Numpad3, K::Numpad4, K::Numpad5, K::Numpad6, K::Numpad7, K::Numpad8, K::Numpad9];
        return Some(if numpad { pad } else { top }[d as usize]);
    }
    match key {
        Key::Named(n) => Some(match n {
            NamedKey::ArrowUp => K::ArrowUp,
            NamedKey::ArrowDown => K::ArrowDown,
            NamedKey::ArrowLeft => K::ArrowLeft,
            NamedKey::ArrowRight => K::ArrowRight,
            NamedKey::Space => K::Space,
            NamedKey::Tab => K::Tab,
            NamedKey::Enter if numpad => K::NumpadEnter,
            NamedKey::Enter => K::Enter,
            NamedKey::Backspace => K::Backspace,
            NamedKey::Escape => K::Escape,
            NamedKey::Shift => K::ShiftLeft,
            NamedKey::Insert => K::Insert,
            NamedKey::Delete => K::Delete,
            NamedKey::Home => K::Home,
            NamedKey::End => K::End,
            NamedKey::PageUp => K::PageUp,
            NamedKey::PageDown => K::PageDown,
            NamedKey::CapsLock => K::CapsLock,
            NamedKey::F1 => K::F1,
            NamedKey::F2 => K::F2,
            NamedKey::F3 => K::F3,
            NamedKey::F4 => K::F4,
            NamedKey::F5 => K::F5,
            NamedKey::F6 => K::F6,
            NamedKey::F7 => K::F7,
            NamedKey::F8 => K::F8,
            NamedKey::F9 => K::F9,
            NamedKey::F10 => K::F10,
            NamedKey::F11 => K::F11,
            NamedKey::F12 => K::F12,
            _ => return None,
        }),
        Key::Character(c) => {
            let s = c.as_str();
            let mut chars = s.chars();
            let (Some(ch), None) = (chars.next(), chars.next()) else { return None };
            let ch = ch.to_ascii_lowercase();
            if numpad {
                let pad = match ch {
                    '+' => Some(K::NumpadAdd),
                    '-' => Some(K::NumpadSubtract),
                    '*' => Some(K::NumpadMultiply),
                    '/' => Some(K::NumpadDivide),
                    '.' | ',' => Some(K::NumpadDecimal),
                    d @ '0'..='9' => return code_of(key, true, Some(d as u8 - b'0')),
                    _ => None,
                };
                if pad.is_some() {
                    return pad;
                }
            }
            if ch.is_ascii_digit() {
                return code_of(key, false, Some(ch as u8 - b'0'));
            }
            if ch.is_ascii_lowercase() {
                let name = format!("Key{}", ch.to_ascii_uppercase());
                return KEYS.iter().find(|k| k.1 == name).map(|k| k.0);
            }
            Some(match ch {
                '-' => K::Minus,
                '=' => K::Equal,
                '[' => K::BracketLeft,
                ']' => K::BracketRight,
                '\\' => K::Backslash,
                ';' => K::Semicolon,
                '\'' => K::Quote,
                '`' => K::Backquote,
                ',' => K::Comma,
                '.' => K::Period,
                '/' => K::Slash,
                _ => return None,
            })
        }
        _ => None,
    }
}

/// Two slots per action, each a key or a mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Keybinds {
    pub slots: [[Option<Input>; 2]; COUNT],
}

impl Default for Keybinds {
    fn default() -> Self {
        let mut slots = [[None; 2]; COUNT];
        for (i, a) in ALL.iter().enumerate() {
            slots[i] = a.defaults();
        }
        Keybinds { slots }
    }
}

impl Keybinds {
    /// The action a key or button is bound to.
    pub fn action(&self, input: impl Into<Input>) -> Option<Act> {
        let input = input.into();
        self.slots.iter().position(|s| s.contains(&Some(input))).map(|i| ALL[i])
    }

    /// The two slots of an action.
    pub fn keys(&self, act: Act) -> [Option<Input>; 2] {
        self.slots[act.index()]
    }

    /// Puts `code` into slot `slot` of `act`. A key is bound to one slot only: if it was elsewhere, that slot is emptied and
    /// returned (the action it was taken from).
    pub fn bind(&mut self, act: Act, slot: usize, code: impl Into<Input>) -> Option<Act> {
        let code = code.into();
        let i = act.index();
        let slot = slot.min(1);
        let mut taken = None;
        for (j, s) in self.slots.iter_mut().enumerate() {
            for (k, c) in s.iter_mut().enumerate() {
                if *c == Some(code) && (j, k) != (i, slot) {
                    *c = None;
                    if j != i {
                        taken = Some(ALL[j]);
                    }
                }
            }
        }
        self.slots[i][slot] = Some(code);
        taken
    }

    /// Empties slot `slot` of `act`.
    pub fn clear(&mut self, act: Act, slot: usize) {
        self.slots[act.index()][slot.min(1)] = None;
    }

    /// Reads the `key_<name>=<code>,<code>` lines of `opensow.cfg`; a missing line keeps the defaults, an empty value or an
    /// unknown key name leaves that slot empty, and a key bound twice keeps only its first place.
    pub fn parse(text: &str) -> Keybinds {
        let mut kb = Keybinds::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let Some(name) = k.trim().strip_prefix("key_") else { continue };
            let Some(i) = ALL.iter().position(|a| a.cfg_name() == name) else { continue };
            let mut parts = v.split(',').map(|p| from_cfg(p.trim()).filter(|&c| bindable(c)));
            kb.slots[i] = [parts.next().flatten(), parts.next().flatten()];
        }
        let mut seen = Vec::new();
        for s in kb.slots.iter_mut() {
            for c in s.iter_mut() {
                if let Some(code) = *c {
                    if seen.contains(&code) {
                        *c = None;
                    } else {
                        seen.push(code);
                    }
                }
            }
        }
        kb
    }

    /// The bindings as `key_` lines for `opensow.cfg`.
    pub fn write(&self) -> String {
        let mut out = String::new();
        for (i, a) in ALL.iter().enumerate() {
            let [x, y] = self.slots[i].map(|c| c.map_or_else(String::new, to_cfg));
            out.push_str(&format!("key_{}={x},{y}\n", a.cfg_name()));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::SmolStr;

    #[test]
    fn defaults_are_the_exes_keys_and_unique() {
        let kb = Keybinds::default();
        assert_eq!(kb.action(KeyCode::KeyP), Some(Act::Pause));
        assert_eq!(kb.action(KeyCode::Numpad0), Some(Act::LookAtEvent));
        assert_eq!(kb.action(KeyCode::Numpad7), Some(Act::Bookmark(6)));
        assert_eq!(kb.action(KeyCode::Digit3), Some(Act::Turret(2)));
        assert_eq!(kb.action(KeyCode::KeyW), Some(Act::ScrollUp));
        assert_eq!(kb.action(KeyCode::Escape), None, "Esc is never bound");
        let mut all: Vec<Input> = kb.slots.iter().flatten().flatten().copied().collect();
        let n = all.len();
        all.sort_by_key(|c| to_cfg(*c));
        all.dedup();
        assert_eq!(all.len(), n, "no key is bound twice");
        assert!(kb.slots.iter().flatten().flatten().all(|&c| bindable(c)), "every default can be rebound");
        let mut names: Vec<String> = ALL.iter().map(|a| a.cfg_name()).collect();
        names.dedup();
        assert_eq!(names.len(), COUNT);
        assert_eq!(PAGE_ROWS.iter().map(|r| r.len()).sum::<usize>(), COUNT);
    }

    #[test]
    fn binding_moves_a_key_from_its_old_action() {
        let mut kb = Keybinds::default();
        assert_eq!(kb.bind(Act::Pause, 0, KeyCode::KeyX), Some(Act::Scatter));
        assert_eq!(kb.action(KeyCode::KeyX), Some(Act::Pause));
        assert_eq!(kb.keys(Act::Scatter), [None, None]);
        assert_eq!(kb.action(KeyCode::KeyP), None, "P is free now");
        // moving a key between the two slots of one action reports nothing
        assert_eq!(kb.bind(Act::Pause, 1, KeyCode::KeyX), None);
        assert_eq!(kb.keys(Act::Pause), [None, Some(Input::Key(KeyCode::KeyX))]);
        kb.clear(Act::Pause, 1);
        assert_eq!(kb.action(KeyCode::KeyX), None);
    }

    #[test]
    fn cfg_round_trips_and_ignores_junk() {
        let mut kb = Keybinds::default();
        kb.bind(Act::QuickSave, 1, KeyCode::KeyK);
        kb.bind(Act::Scatter, 1, MouseButton::Middle);
        kb.bind(Act::Pause, 1, MouseButton::Back);
        kb.bind(Act::Chat, 1, MouseButton::Other(7));
        kb.clear(Act::ScrollUp, 0);
        assert!(kb.write().contains("key_scatter=KeyX,MouseMiddle\n"));
        assert_eq!(Keybinds::parse(&kb.write()), kb);
        assert_eq!(Keybinds::parse(""), Keybinds::default(), "a missing file keeps the defaults");
        let k = Keybinds::parse("enhance_textures=0\nkey_pause=KeyL,Nonsense\nkey_bogus=KeyA\n");
        assert_eq!(k.keys(Act::Pause), [Some(Input::Key(KeyCode::KeyL)), None]);
        let k = Keybinds::parse("key_pause=Escape,\n");
        assert_eq!(k.keys(Act::Pause), [None, None], "Esc cannot be bound from the file either");
        let k = Keybinds::parse("key_pause=KeyW,\n");
        assert_eq!(k.action(KeyCode::KeyW), Some(Act::ScrollUp), "a key bound twice keeps its first place");
        assert_eq!(k.keys(Act::Pause), [None, None]);
    }

    #[test]
    fn logical_keys_map_to_physical_ones() {
        let ch = |s: &str| Key::Character(SmolStr::new(s));
        assert_eq!(code_of(&ch("H"), false, None), Some(KeyCode::KeyH));
        assert_eq!(code_of(&ch("š"), false, Some(3)), Some(KeyCode::Digit3), "the physical digit wins (Czech layout)");
        assert_eq!(code_of(&ch("3"), true, None), Some(KeyCode::Numpad3));
        assert_eq!(code_of(&ch("*"), true, None), Some(KeyCode::NumpadMultiply));
        assert_eq!(code_of(&ch("-"), false, None), Some(KeyCode::Minus));
        assert_eq!(code_of(&Key::Named(NamedKey::Enter), true, None), Some(KeyCode::NumpadEnter));
        assert_eq!(code_of(&Key::Named(NamedKey::F5), false, None), Some(KeyCode::F5));
        assert!(!bindable(KeyCode::Escape) && !bindable(KeyCode::ShiftLeft) && !bindable(KeyCode::ControlLeft));
        assert!(bindable(KeyCode::KeyA));
    }

    #[test]
    fn extra_mouse_buttons_bind_but_left_and_right_do_not() {
        assert!(!bindable(MouseButton::Left) && !bindable(MouseButton::Right));
        assert!(bindable(MouseButton::Middle) && bindable(MouseButton::Back) && bindable(MouseButton::Other(9)));
        let mut kb = Keybinds::default();
        assert_eq!(kb.bind(Act::Scatter, 0, MouseButton::Middle), None);
        assert_eq!(kb.action(MouseButton::Middle), Some(Act::Scatter));
        assert_eq!(kb.bind(Act::Pause, 1, MouseButton::Middle), Some(Act::Scatter), "a button moves like a key");
        assert_eq!(key_name(Input::Mouse(MouseButton::Middle)), "Mouse 3");
        assert_eq!(key_name(Input::Mouse(MouseButton::Forward)), "Mouse 5");
        assert_eq!(key_name(Input::Key(KeyCode::Numpad1)), "Num 1");
        let k = Keybinds::parse("key_pause=MouseLeft,MouseBack\n");
        assert_eq!(k.keys(Act::Pause), [None, Some(Input::Mouse(MouseButton::Back))], "left / right never come from the file");
    }
}
