//! Mapping tables: Mac shortcut translations, the fn layer and the Option-key characters of
//! the Mac U.S. layout.

use crate::engine::Action;
use crate::key::{Key, WinMods};
use crate::profile::AppProfile;

/// The Mac modifiers held while a key is pressed, regardless of side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MacMods {
    pub cmd: bool,
    pub opt: bool,
    pub ctrl: bool,
    pub shift: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShiftRule {
    /// Shift must not be held.
    No,
    /// Shift must be held.
    Yes,
    /// Shift may be held and is added to the output (selection with arrows).
    Carry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleAction {
    /// Hold `key` with exactly `mods` for as long as the physical key is held.
    Chord {
        mods: WinMods,
        key: Key,
    },
    /// Tap each chord in turn when the key goes down.
    Seq(&'static [(WinMods, Key)]),
    /// Like `Chord`, but `mods` stay down until Command is released (the app switcher).
    Sticky {
        mods: WinMods,
        key: Key,
    },
    Action(Action),
    /// Ctrl+Cmd+key becomes Win+key, so every Windows-key shortcut stays reachable.
    WinKey,
}

#[derive(Debug, Clone, Copy)]
pub struct Rule {
    pub cmd: bool,
    pub opt: bool,
    pub ctrl: bool,
    pub shift: ShiftRule,
    pub key: Key,
    pub action: RuleAction,
}

impl Rule {
    fn matches(&self, m: MacMods, key: Key) -> bool {
        self.key == key
            && self.cmd == m.cmd
            && self.opt == m.opt
            && self.ctrl == m.ctrl
            && match self.shift {
                ShiftRule::No => !m.shift,
                ShiftRule::Yes => m.shift,
                ShiftRule::Carry => true,
            }
    }
}

const CMD: u8 = 1;
const OPT: u8 = 2;
const CTRL: u8 = 4;

const fn r(mods: u8, shift: ShiftRule, key: Key, action: RuleAction) -> Rule {
    Rule {
        cmd: mods & CMD != 0,
        opt: mods & OPT != 0,
        ctrl: mods & CTRL != 0,
        shift,
        key,
        action,
    }
}

const fn chord(mods: WinMods, key: Key) -> RuleAction {
    RuleAction::Chord { mods, key }
}

const NONE: WinMods = WinMods::NONE;
const W_CTRL: WinMods = WinMods::LCTRL;
const W_SHIFT: WinMods = WinMods::LSHIFT;
const W_ALT: WinMods = WinMods::LALT;
const W_WIN: WinMods = WinMods::LWIN;
const W_CTRL_SHIFT: WinMods = WinMods::LCTRL.union(WinMods::LSHIFT);
const W_WIN_SHIFT: WinMods = WinMods::LWIN.union(WinMods::LSHIFT);
const W_WIN_CTRL: WinMods = WinMods::LWIN.union(WinMods::LCTRL);

use ShiftRule::{Carry, No, Yes};

/// Translations that apply in every app.
pub const GLOBAL: &[Rule] = &[
    // Text navigation and editing.
    r(CMD, Carry, Key::LEFT, chord(NONE, Key::HOME)),
    r(CMD, Carry, Key::RIGHT, chord(NONE, Key::END)),
    r(CMD, Carry, Key::UP, chord(W_CTRL, Key::HOME)),
    r(CMD, Carry, Key::DOWN, chord(W_CTRL, Key::END)),
    r(OPT, Carry, Key::LEFT, chord(W_CTRL, Key::LEFT)),
    r(OPT, Carry, Key::RIGHT, chord(W_CTRL, Key::RIGHT)),
    r(OPT, Carry, Key::UP, chord(W_CTRL, Key::UP)),
    r(OPT, Carry, Key::DOWN, chord(W_CTRL, Key::DOWN)),
    r(OPT, No, Key::BACKSPACE, chord(W_CTRL, Key::BACKSPACE)),
    r(OPT, No, Key::DELETE, chord(W_CTRL, Key::DELETE)),
    r(
        CMD,
        No,
        Key::BACKSPACE,
        RuleAction::Seq(&[(W_SHIFT, Key::HOME), (NONE, Key::BACKSPACE)]),
    ),
    r(
        CMD,
        No,
        Key::DELETE,
        RuleAction::Seq(&[(W_SHIFT, Key::END), (NONE, Key::DELETE)]),
    ),
    r(CMD, Yes, Key::Z, chord(W_CTRL, Key::Y)),
    // Apps and windows.
    r(
        CMD,
        Carry,
        Key::TAB,
        RuleAction::Sticky {
            mods: W_ALT,
            key: Key::TAB,
        },
    ),
    r(CMD, No, Key::Q, chord(W_ALT, Key::F4)),
    r(CMD, No, Key::H, RuleAction::Action(Action::Minimize)),
    r(CMD, No, Key::M, RuleAction::Action(Action::Minimize)),
    r(CMD | OPT, No, Key::M, chord(W_WIN, Key::M)),
    r(CMD, No, Key::SPACE, chord(W_WIN, Key::S)),
    r(CMD | CTRL, No, Key::SPACE, chord(W_WIN, Key::PERIOD)),
    r(CMD | CTRL, No, Key::Q, RuleAction::Action(Action::Lock)),
    r(CMD | CTRL, No, Key::L, RuleAction::Action(Action::Lock)),
    r(CMD | CTRL, No, Key::F, chord(NONE, Key::F11)),
    r(CMD | OPT, No, Key::ESC, chord(W_CTRL_SHIFT, Key::ESC)),
    // Screenshots.
    r(CMD, Yes, Key::D3, chord(NONE, Key::PRINT_SCREEN)),
    r(CMD, Yes, Key::D4, chord(W_WIN_SHIFT, Key::S)),
    r(CMD, Yes, Key::D5, chord(W_WIN_SHIFT, Key::R)),
    // Desktops and window layout.
    r(CTRL, No, Key::LEFT, chord(W_WIN_CTRL, Key::LEFT)),
    r(CTRL, No, Key::RIGHT, chord(W_WIN_CTRL, Key::RIGHT)),
    r(CTRL, No, Key::UP, chord(W_WIN, Key::TAB)),
    r(CTRL | OPT, No, Key::LEFT, chord(W_WIN, Key::LEFT)),
    r(CTRL | OPT, No, Key::RIGHT, chord(W_WIN, Key::RIGHT)),
    r(CTRL | OPT, No, Key::UP, chord(W_WIN, Key::UP)),
    r(CTRL | OPT, No, Key::DOWN, chord(W_WIN, Key::DOWN)),
];

/// File Explorer behaves like Finder.
pub const EXPLORER: &[Rule] = &[
    r(CMD, No, Key::BACKSPACE, chord(NONE, Key::DELETE)),
    r(CMD, No, Key::DOWN, chord(NONE, Key::ENTER)),
    r(CMD, No, Key::UP, chord(W_ALT, Key::UP)),
    r(CMD, No, Key::I, chord(W_ALT, Key::ENTER)),
];

pub const BROWSER: &[Rule] = &[
    r(CMD, No, Key::LBRACKET, chord(W_ALT, Key::LEFT)),
    r(CMD, No, Key::RBRACKET, chord(W_ALT, Key::RIGHT)),
    r(CMD, Yes, Key::LBRACKET, chord(W_CTRL_SHIFT, Key::TAB)),
    r(CMD, Yes, Key::RBRACKET, chord(W_CTRL, Key::TAB)),
    r(CMD | OPT, No, Key::LEFT, chord(W_CTRL_SHIFT, Key::TAB)),
    r(CMD | OPT, No, Key::RIGHT, chord(W_CTRL, Key::TAB)),
    r(CMD | OPT, No, Key::I, chord(W_CTRL_SHIFT, Key::I)),
    r(CMD | OPT, No, Key::J, chord(W_CTRL_SHIFT, Key::J)),
    r(CMD | OPT, No, Key::C, chord(W_CTRL_SHIFT, Key::C)),
    r(CMD, No, Key::Y, chord(W_CTRL, Key::H)),
];

/// Finds the translation for a key pressed with Mac modifiers. Returns the action and whether
/// a held Shift should be added to its output.
pub fn lookup(profile: AppProfile, m: MacMods, key: Key, sticky_active: bool) -> Option<(RuleAction, bool)> {
    let found = |rules: &[Rule]| {
        rules
            .iter()
            .find(|rule| rule.matches(m, key))
            .map(|rule| (rule.action, rule.shift == Carry))
    };
    if sticky_active {
        // While the app switcher is open only Tab keeps its meaning; other keys go to the switcher.
        return found(GLOBAL).filter(|(action, _)| matches!(action, RuleAction::Sticky { .. }));
    }
    let specific: &[Rule] = match profile {
        AppProfile::Explorer => EXPLORER,
        AppProfile::Browser => BROWSER,
        AppProfile::Normal | AppProfile::Terminal => &[],
    };
    if let Some(hit) = found(specific).or_else(|| found(GLOBAL)) {
        return Some(hit);
    }
    if profile == AppProfile::Terminal && m.cmd && !m.opt && !m.ctrl && key.is_letter() {
        return Some((chord(W_CTRL_SHIFT, key), false));
    }
    if m.cmd && m.ctrl {
        return Some((RuleAction::WinKey, true));
    }
    None
}

/// What a key does while the fn stand-in is held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FnOut {
    /// Behaves as this key (held for as long as the physical key).
    Key(Key),
    /// Taps this chord once.
    Tap(WinMods, Key),
    Action(Action),
    TogglePause,
}

pub fn fn_layer(key: Key) -> Option<FnOut> {
    Some(match key {
        Key::BACKSPACE => FnOut::Key(Key::DELETE),
        Key::ENTER => FnOut::Key(Key::INSERT),
        Key::LEFT => FnOut::Key(Key::HOME),
        Key::RIGHT => FnOut::Key(Key::END),
        Key::UP => FnOut::Key(Key::PAGE_UP),
        Key::DOWN => FnOut::Key(Key::PAGE_DOWN),
        Key::F1 => FnOut::Action(Action::BrightnessDown),
        Key::F2 => FnOut::Action(Action::BrightnessUp),
        Key::F3 => FnOut::Tap(W_WIN, Key::TAB),
        Key::F4 => FnOut::Tap(W_WIN, Key::S),
        Key::F5 => FnOut::Tap(W_WIN, Key::H),
        Key::F6 => FnOut::Tap(W_WIN, Key::N),
        Key::F7 => FnOut::Key(Key::MEDIA_PREV),
        Key::F8 => FnOut::Key(Key::MEDIA_PLAY_PAUSE),
        Key::F9 => FnOut::Key(Key::MEDIA_NEXT),
        Key::F10 => FnOut::Key(Key::VOLUME_MUTE),
        Key::F11 => FnOut::Key(Key::VOLUME_DOWN),
        Key::F12 => FnOut::Key(Key::VOLUME_UP),
        Key::ESC => FnOut::TogglePause,
        _ => return None,
    })
}

/// Accent dead keys of the Mac U.S. layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dead {
    Grave,
    Acute,
    Circumflex,
    Umlaut,
    Tilde,
}

impl Dead {
    /// What the accent types on its own (before a space or a key it can't combine with).
    pub fn standalone(self) -> char {
        match self {
            Dead::Grave => '`',
            Dead::Acute => '´',
            Dead::Circumflex => 'ˆ',
            Dead::Umlaut => '¨',
            Dead::Tilde => '˜',
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptChar {
    Char(char),
    Dead(Dead),
}

/// Option (and Option+Shift) characters of the Mac U.S. layout, by physical key.
pub fn option_char(key: Key, shift: bool) -> Option<OptChar> {
    use OptChar::{Char as C, Dead as D};
    let pair = match key {
        Key::GRAVE => (D(Dead::Grave), C('`')),
        Key::D1 => (C('¡'), C('⁄')),
        Key::D2 => (C('™'), C('€')),
        Key::D3 => (C('£'), C('‹')),
        Key::D4 => (C('¢'), C('›')),
        Key::D5 => (C('∞'), C('ﬁ')),
        Key::D6 => (C('§'), C('ﬂ')),
        Key::D7 => (C('¶'), C('‡')),
        Key::D8 => (C('•'), C('°')),
        Key::D9 => (C('ª'), C('·')),
        Key::D0 => (C('º'), C('‚')),
        Key::MINUS => (C('–'), C('—')),
        Key::EQUAL => (C('≠'), C('±')),
        Key::Q => (C('œ'), C('Œ')),
        Key::W => (C('∑'), C('„')),
        Key::E => (D(Dead::Acute), C('´')),
        Key::R => (C('®'), C('‰')),
        Key::T => (C('†'), C('ˇ')),
        Key::Y => (C('¥'), C('Á')),
        Key::U => (D(Dead::Umlaut), C('¨')),
        Key::I => (D(Dead::Circumflex), C('ˆ')),
        Key::O => (C('ø'), C('Ø')),
        Key::P => (C('π'), C('∏')),
        Key::LBRACKET => (C('“'), C('”')),
        Key::RBRACKET => (C('‘'), C('’')),
        Key::BACKSLASH => (C('«'), C('»')),
        Key::A => (C('å'), C('Å')),
        Key::S => (C('ß'), C('Í')),
        Key::D => (C('∂'), C('Î')),
        Key::F => (C('ƒ'), C('Ï')),
        Key::G => (C('©'), C('˝')),
        Key::H => (C('˙'), C('Ó')),
        Key::J => (C('∆'), C('Ô')),
        // Option+Shift+K is the Apple logo, which Windows fonts don't have.
        Key::K => (C('˚'), C('\u{F8FF}')),
        Key::L => (C('¬'), C('Ò')),
        Key::SEMICOLON => (C('…'), C('Ú')),
        Key::APOSTROPHE => (C('æ'), C('Æ')),
        Key::Z => (C('Ω'), C('¸')),
        Key::X => (C('≈'), C('˛')),
        Key::C => (C('ç'), C('Ç')),
        Key::V => (C('√'), C('◊')),
        Key::B => (C('∫'), C('ı')),
        Key::N => (D(Dead::Tilde), C('˜')),
        Key::M => (C('µ'), C('Â')),
        Key::COMMA => (C('≤'), C('¯')),
        Key::PERIOD => (C('≥'), C('˘')),
        Key::SLASH => (C('÷'), C('¿')),
        Key::SPACE => (C('\u{A0}'), C('\u{A0}')),
        _ => return None,
    };
    let chosen = if shift { pair.1 } else { pair.0 };
    if chosen == C('\u{F8FF}') { None } else { Some(chosen) }
}

/// Combines an accent with a letter, e.g. umlaut + a = ä.
pub fn compose(dead: Dead, letter: char, upper: bool) -> Option<char> {
    let c = match (dead, letter) {
        (Dead::Grave, 'a') => 'à',
        (Dead::Grave, 'e') => 'è',
        (Dead::Grave, 'i') => 'ì',
        (Dead::Grave, 'o') => 'ò',
        (Dead::Grave, 'u') => 'ù',
        (Dead::Acute, 'a') => 'á',
        (Dead::Acute, 'e') => 'é',
        (Dead::Acute, 'i') => 'í',
        (Dead::Acute, 'o') => 'ó',
        (Dead::Acute, 'u') => 'ú',
        (Dead::Acute, 'y') => 'ý',
        (Dead::Circumflex, 'a') => 'â',
        (Dead::Circumflex, 'e') => 'ê',
        (Dead::Circumflex, 'i') => 'î',
        (Dead::Circumflex, 'o') => 'ô',
        (Dead::Circumflex, 'u') => 'û',
        (Dead::Umlaut, 'a') => 'ä',
        (Dead::Umlaut, 'e') => 'ë',
        (Dead::Umlaut, 'i') => 'ï',
        (Dead::Umlaut, 'o') => 'ö',
        (Dead::Umlaut, 'u') => 'ü',
        (Dead::Umlaut, 'y') => 'ÿ',
        (Dead::Tilde, 'a') => 'ã',
        (Dead::Tilde, 'n') => 'ñ',
        (Dead::Tilde, 'o') => 'õ',
        _ => return None,
    };
    if upper { c.to_uppercase().next() } else { Some(c) }
}

/// The letter a key types on the U.S. layout.
pub fn us_letter(key: Key) -> Option<char> {
    const ROWS: [(u16, &str); 3] = [(0x10, "qwertyuiop"), (0x1E, "asdfghjkl"), (0x2C, "zxcvbnm")];
    ROWS.iter().find_map(|(start, letters)| {
        let offset = key.0.checked_sub(*start)? as usize;
        letters.chars().nth(offset)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mods(cmd: bool, opt: bool, ctrl: bool, shift: bool) -> MacMods {
        MacMods { cmd, opt, ctrl, shift }
    }

    #[test]
    fn lookup_prefers_app_specific_rules() {
        let cmd = mods(true, false, false, false);
        let (explorer, _) = lookup(AppProfile::Explorer, cmd, Key::DOWN, false).unwrap();
        assert_eq!(explorer, chord(NONE, Key::ENTER));
        let (normal, _) = lookup(AppProfile::Normal, cmd, Key::DOWN, false).unwrap();
        assert_eq!(normal, chord(W_CTRL, Key::END));
    }

    #[test]
    fn shift_rules() {
        assert!(lookup(AppProfile::Normal, mods(true, false, false, false), Key::D3, false).is_none());
        assert!(lookup(AppProfile::Normal, mods(true, false, false, true), Key::D3, false).is_some());
        let (_, carry) = lookup(AppProfile::Normal, mods(true, false, false, true), Key::LEFT, false).unwrap();
        assert!(carry);
    }

    #[test]
    fn terminal_letters_add_shift() {
        let (action, _) = lookup(AppProfile::Terminal, mods(true, false, false, false), Key::C, false).unwrap();
        assert_eq!(action, chord(W_CTRL_SHIFT, Key::C));
        // Cmd+Q still quits in a terminal.
        let (quit, _) = lookup(AppProfile::Terminal, mods(true, false, false, false), Key::Q, false).unwrap();
        assert_eq!(quit, chord(W_ALT, Key::F4));
        // Letters in other apps fall through to the plain Cmd-as-Ctrl mapping.
        assert!(lookup(AppProfile::Normal, mods(true, false, false, false), Key::C, false).is_none());
    }

    #[test]
    fn ctrl_cmd_reaches_windows_key_shortcuts() {
        let (action, _) = lookup(AppProfile::Normal, mods(true, false, true, false), Key::E, false).unwrap();
        assert_eq!(action, RuleAction::WinKey);
        let (lock, _) = lookup(AppProfile::Normal, mods(true, false, true, false), Key::L, false).unwrap();
        assert_eq!(lock, RuleAction::Action(Action::Lock));
    }

    #[test]
    fn switcher_only_keeps_tab() {
        let cmd = mods(true, false, false, false);
        assert!(lookup(AppProfile::Normal, cmd, Key::TAB, true).is_some());
        assert!(lookup(AppProfile::Normal, cmd, Key::LEFT, true).is_none());
    }

    #[test]
    fn option_characters() {
        assert_eq!(option_char(Key::D2, false), Some(OptChar::Char('™')));
        assert_eq!(option_char(Key::D2, true), Some(OptChar::Char('€')));
        assert_eq!(option_char(Key::U, false), Some(OptChar::Dead(Dead::Umlaut)));
        assert_eq!(option_char(Key::K, true), None);
        assert_eq!(option_char(Key::F1, false), None);
    }

    #[test]
    fn composition() {
        assert_eq!(compose(Dead::Umlaut, 'a', false), Some('ä'));
        assert_eq!(compose(Dead::Umlaut, 'o', true), Some('Ö'));
        assert_eq!(compose(Dead::Umlaut, 'y', true), Some('Ÿ'));
        assert_eq!(compose(Dead::Tilde, 'e', false), None);
        assert_eq!(us_letter(Key::Q), Some('q'));
        assert_eq!(us_letter(Key::L), Some('l'));
        assert_eq!(us_letter(Key::M), Some('m'));
        assert_eq!(us_letter(Key::SEMICOLON), None);
    }

    #[test]
    fn fn_layer_covers_the_f_row() {
        for key in [
            Key::F1,
            Key::F2,
            Key::F3,
            Key::F4,
            Key::F5,
            Key::F6,
            Key::F7,
            Key::F8,
            Key::F9,
            Key::F10,
            Key::F11,
            Key::F12,
        ] {
            assert!(fn_layer(key).is_some(), "{key:?}");
        }
        assert_eq!(fn_layer(Key::BACKSPACE), Some(FnOut::Key(Key::DELETE)));
        assert_eq!(fn_layer(Key::A), None);
    }
}
