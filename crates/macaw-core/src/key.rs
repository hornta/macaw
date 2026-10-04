//! Keys are identified by the scan code Windows reports for them (set 1). A scan code names a
//! physical position, independent of the keyboard layout. Extended keys carry an 0xE000 prefix.

use std::fmt;
use std::ops::{BitOr, BitOrAssign};

#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Key(pub u16);

macro_rules! keys {
    ($($name:ident = $code:expr, $label:expr;)*) => {
        impl Key {
            $(pub const $name: Key = Key($code);)*
        }
        const NAMES: &[(u16, &str)] = &[$(($code, $label)),*];
    };
}

keys! {
    ESC = 0x01, "Esc";
    D1 = 0x02, "1"; D2 = 0x03, "2"; D3 = 0x04, "3"; D4 = 0x05, "4"; D5 = 0x06, "5";
    D6 = 0x07, "6"; D7 = 0x08, "7"; D8 = 0x09, "8"; D9 = 0x0A, "9"; D0 = 0x0B, "0";
    MINUS = 0x0C, "Minus"; EQUAL = 0x0D, "Equal"; BACKSPACE = 0x0E, "Backspace"; TAB = 0x0F, "Tab";
    Q = 0x10, "Q"; W = 0x11, "W"; E = 0x12, "E"; R = 0x13, "R"; T = 0x14, "T";
    Y = 0x15, "Y"; U = 0x16, "U"; I = 0x17, "I"; O = 0x18, "O"; P = 0x19, "P";
    LBRACKET = 0x1A, "LBracket"; RBRACKET = 0x1B, "RBracket"; ENTER = 0x1C, "Enter"; LCTRL = 0x1D, "LCtrl";
    A = 0x1E, "A"; S = 0x1F, "S"; D = 0x20, "D"; F = 0x21, "F"; G = 0x22, "G";
    H = 0x23, "H"; J = 0x24, "J"; K = 0x25, "K"; L = 0x26, "L";
    SEMICOLON = 0x27, "Semicolon"; APOSTROPHE = 0x28, "Apostrophe"; GRAVE = 0x29, "Grave";
    LSHIFT = 0x2A, "LShift"; BACKSLASH = 0x2B, "Backslash";
    Z = 0x2C, "Z"; X = 0x2D, "X"; C = 0x2E, "C"; V = 0x2F, "V"; B = 0x30, "B"; N = 0x31, "N"; M = 0x32, "M";
    COMMA = 0x33, "Comma"; PERIOD = 0x34, "Period"; SLASH = 0x35, "Slash"; RSHIFT = 0x36, "RShift";
    LALT = 0x38, "LAlt"; SPACE = 0x39, "Space"; CAPS_LOCK = 0x3A, "CapsLock";
    F1 = 0x3B, "F1"; F2 = 0x3C, "F2"; F3 = 0x3D, "F3"; F4 = 0x3E, "F4"; F5 = 0x3F, "F5";
    F6 = 0x40, "F6"; F7 = 0x41, "F7"; F8 = 0x42, "F8"; F9 = 0x43, "F9"; F10 = 0x44, "F10";
    INTL_BACKSLASH = 0x56, "IntlBackslash"; F11 = 0x57, "F11"; F12 = 0x58, "F12";
    ALTGR_CTRL = 0x021D, "AltGrCtrl";
    MEDIA_PREV = 0xE010, "MediaPrev"; MEDIA_NEXT = 0xE019, "MediaNext"; RCTRL = 0xE01D, "RCtrl";
    VOLUME_MUTE = 0xE020, "VolumeMute"; MEDIA_PLAY_PAUSE = 0xE022, "PlayPause"; MEDIA_STOP = 0xE024, "MediaStop";
    VOLUME_DOWN = 0xE02E, "VolumeDown"; VOLUME_UP = 0xE030, "VolumeUp";
    PRINT_SCREEN = 0xE037, "PrintScreen"; RALT = 0xE038, "RAlt";
    HOME = 0xE047, "Home"; UP = 0xE048, "Up"; PAGE_UP = 0xE049, "PageUp"; LEFT = 0xE04B, "Left";
    RIGHT = 0xE04D, "Right"; END = 0xE04F, "End"; DOWN = 0xE050, "Down"; PAGE_DOWN = 0xE051, "PageDown";
    INSERT = 0xE052, "Insert"; DELETE = 0xE053, "Delete";
    LWIN = 0xE05B, "LWin"; RWIN = 0xE05C, "RWin"; APPS = 0xE05D, "Apps";
}

impl Key {
    /// Builds a key from what a low-level keyboard hook reports.
    ///
    /// Scan code 0x21D is the fake left Ctrl that Windows synthesizes in front of right Alt on
    /// layouts with AltGr; it keeps its own identity so it can't be mistaken for the real key.
    pub fn from_scan(scan_code: u32, extended: bool) -> Key {
        if scan_code == 0x21D {
            return Key::ALTGR_CTRL;
        }
        let base = (scan_code & 0xFF) as u16;
        Key(if extended { 0xE000 | base } else { base })
    }

    /// The scan code without the extended prefix.
    pub fn scan_code(self) -> u16 {
        self.0 & 0xFF
    }

    pub fn is_extended(self) -> bool {
        self.0 & 0xFF00 == 0xE000
    }

    pub fn is_modifier(self) -> bool {
        WinMods::from_key(self).is_some()
    }

    pub fn is_letter(self) -> bool {
        matches!(self.0, 0x10..=0x19 | 0x1E..=0x26 | 0x2C..=0x32)
    }

    pub fn is_digit(self) -> bool {
        matches!(self.0, 0x02..=0x0B)
    }

    pub fn name(self) -> Option<&'static str> {
        NAMES.iter().find(|(code, _)| *code == self.0).map(|(_, name)| *name)
    }
}

impl fmt::Debug for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(name) => f.write_str(name),
            None => write!(f, "sc{:04X}", self.0),
        }
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

/// A set of the eight Windows modifier keys (left and right Ctrl, Shift, Alt and Win).
#[derive(Copy, Clone, PartialEq, Eq, Default, Hash)]
pub struct WinMods(u8);

impl WinMods {
    pub const NONE: WinMods = WinMods(0);
    pub const LCTRL: WinMods = WinMods(1 << 0);
    pub const RCTRL: WinMods = WinMods(1 << 1);
    pub const LSHIFT: WinMods = WinMods(1 << 2);
    pub const RSHIFT: WinMods = WinMods(1 << 3);
    pub const LALT: WinMods = WinMods(1 << 4);
    pub const RALT: WinMods = WinMods(1 << 5);
    pub const LWIN: WinMods = WinMods(1 << 6);
    pub const RWIN: WinMods = WinMods(1 << 7);

    /// Every modifier with the key that produces it, in bit order.
    pub const KEYS: [(WinMods, Key); 8] = [
        (WinMods::LCTRL, Key::LCTRL),
        (WinMods::RCTRL, Key::RCTRL),
        (WinMods::LSHIFT, Key::LSHIFT),
        (WinMods::RSHIFT, Key::RSHIFT),
        (WinMods::LALT, Key::LALT),
        (WinMods::RALT, Key::RALT),
        (WinMods::LWIN, Key::LWIN),
        (WinMods::RWIN, Key::RWIN),
    ];

    pub const fn union(self, other: WinMods) -> WinMods {
        WinMods(self.0 | other.0)
    }

    pub const fn without(self, other: WinMods) -> WinMods {
        WinMods(self.0 & !other.0)
    }

    pub const fn intersect(self, other: WinMods) -> WinMods {
        WinMods(self.0 & other.0)
    }

    pub const fn contains(self, other: WinMods) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn from_key(key: Key) -> Option<WinMods> {
        WinMods::KEYS.iter().find(|(_, k)| *k == key).map(|(m, _)| *m)
    }

    /// Position of a single-modifier set in [`WinMods::KEYS`].
    pub fn index(self) -> usize {
        debug_assert_eq!(self.0.count_ones(), 1, "index() needs exactly one modifier");
        self.0.trailing_zeros() as usize
    }
}

impl BitOr for WinMods {
    type Output = WinMods;
    fn bitor(self, rhs: WinMods) -> WinMods {
        self.union(rhs)
    }
}

impl BitOrAssign for WinMods {
    fn bitor_assign(&mut self, rhs: WinMods) {
        *self = self.union(rhs);
    }
}

impl fmt::Debug for WinMods {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<String> = WinMods::KEYS
            .iter()
            .filter(|(m, _)| self.contains(*m))
            .map(|(_, k)| format!("{k:?}"))
            .collect();
        write!(f, "{{{}}}", names.join("+"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_codes_round_trip() {
        assert_eq!(Key::from_scan(0x4B, true), Key::LEFT);
        assert_eq!(Key::from_scan(0x1D, false), Key::LCTRL);
        assert_eq!(Key::from_scan(0x21D, false), Key::ALTGR_CTRL);
        assert_eq!(Key::LEFT.scan_code(), 0x4B);
        assert!(Key::LEFT.is_extended());
        assert!(!Key::A.is_extended());
    }

    #[test]
    fn classification() {
        assert!(Key::LWIN.is_modifier());
        assert!(!Key::CAPS_LOCK.is_modifier());
        assert!(!Key::ALTGR_CTRL.is_modifier());
        assert!(Key::Q.is_letter() && Key::M.is_letter() && Key::A.is_letter());
        assert!(!Key::SEMICOLON.is_letter() && !Key::D1.is_letter());
    }

    #[test]
    fn modifier_sets() {
        let m = WinMods::LCTRL | WinMods::LSHIFT;
        assert!(m.contains(WinMods::LCTRL));
        assert!(!m.contains(WinMods::LALT));
        assert_eq!(m.without(WinMods::LCTRL), WinMods::LSHIFT);
        assert_eq!(WinMods::LWIN.index(), 6);
        assert_eq!(WinMods::from_key(Key::RALT), Some(WinMods::RALT));
        assert_eq!(format!("{m:?}"), "{LCtrl+LShift}");
    }
}
