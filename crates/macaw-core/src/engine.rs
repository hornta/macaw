//! The remapping engine: a pure state machine with no Windows dependencies.
//!
//! The Windows backend passes it every physical key event and does what the returned
//! [`Decision`] says: let the original event through (`forward`) and/or inject new input (`out`).
//! Injected input is processed by Windows after the original event, so whenever the output must
//! differ from the original, the engine swallows the original and injects everything itself.
//!
//! Modifiers are reconciled rather than mirrored: the engine tracks which Windows modifier keys
//! it has made Windows hold, computes which ones the next output needs, and injects only the
//! difference. Other keys are reference-counted, so a Windows key stays down while any physical
//! key still holds it.

use std::collections::{BTreeMap, BTreeSet};

use crate::config::{Config, OptionKey, StandIn};
use crate::key::{Key, WinMods};
use crate::profile::AppProfile;
use crate::tables::{self, Dead, FnOut, MacMods, OptChar, RuleAction};

/// Things Macaw does that aren't key presses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Lock,
    Minimize,
    BrightnessUp,
    BrightnessDown,
}

impl Action {
    fn repeats(self) -> bool {
        matches!(self, Action::BrightnessUp | Action::BrightnessDown)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    Key {
        key: Key,
        down: bool,
    },
    /// Tap an unassigned virtual key, so Windows doesn't see a lone press of Alt (menu bar),
    /// Win (Start menu) or Ctrl (Find My Mouse) that the user never made.
    Mask,
    Text(char),
    Action(Action),
    /// Macaw was paused or resumed from the keyboard.
    Paused(bool),
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Decision {
    /// Let the original event through (before anything in `out`).
    pub forward: bool,
    pub out: Vec<Out>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Input {
    pub key: Key,
    pub down: bool,
    /// Milliseconds, from any monotonic clock that `tick` also uses.
    pub time: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Ctx {
    /// The event comes from a keyboard Macaw should change.
    pub remap: bool,
    /// Let new key presses through untouched (a game is in front, or the foreground app is
    /// elevated and Macaw isn't, so injected input wouldn't reach it).
    pub bypass: bool,
    pub profile: AppProfile,
    /// Caps Lock is on (decides the case of composed accented letters).
    pub caps_lock_on: bool,
}

/// Windows' key auto-repeat settings. The stuck-key guard relies on repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Typematic {
    pub delay_ms: u64,
    pub interval_ms: u64,
    /// False when auto-repeat is off (Filter Keys), which disables the guard.
    pub repeats: bool,
}

impl Default for Typematic {
    fn default() -> Typematic {
        Typematic {
            delay_ms: 500,
            interval_ms: 33,
            repeats: true,
        }
    }
}

/// A held key that isn't the most recent one gets no repeats; after this long without
/// activity it is assumed to be stuck (modifiers excepted).
const STUCK_NON_CANDIDATE_MS: u64 = 2_000;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Emission {
    /// Not ours to change (another keyboard, a game, paused): the original went through and
    /// its repeats and key-up go through too, whatever happens meanwhile.
    Passthrough,
    /// The original went through as part of our output; it holds its own Windows key.
    Forwarded,
    /// We injected this key down; it holds that Windows key.
    Injected(Key),
    /// A physical modifier; its effect lives in the modifier state.
    Modifier,
    /// The fn stand-in key.
    StandIn,
    /// Nothing to release.
    Consumed,
    /// Typed a character; repeats type it again.
    Text(char),
    /// Ran an action; repeats run it again.
    Repeat(Action),
    /// Tapped chords; repeats tap them again.
    Taps(Vec<(WinMods, Key)>),
}

#[derive(Debug, Clone)]
struct Held {
    last: u64,
    emission: Emission,
    /// Whether the stuck-key guard may release this key.
    guard: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StandInState {
    Idle,
    Held { since: u64, used: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Sticky {
    /// The Command key whose release ends the sticky chord.
    trigger: Key,
    mods: WinMods,
}

#[derive(Debug, Clone, Copy, Default)]
struct ModState {
    /// The modifier is down because its physical key was forwarded (not injected).
    natural: bool,
    /// Some other key was pressed since the modifier went down.
    key_since: bool,
}

enum Composed {
    Char(char),
    /// Type the accent on its own; `true` if the key was used up doing so (Space).
    Accent(char, bool),
    Cancel,
}

pub struct Engine {
    cfg: Config,
    typematic: Typematic,
    paused: bool,
    held: BTreeMap<Key, Held>,
    /// Modifier keys Windows holds down because of us (forwarded or injected).
    mods_down: BTreeSet<Key>,
    mods: [ModState; 8],
    /// Other keys Windows holds down because of us, with how many physical keys hold each.
    keys_down: BTreeMap<Key, u32>,
    /// Modifier keys held by input we pass through untouched (another keyboard, a game).
    /// Windows has one state per key, so we must not release these under their holder.
    ext_mods: BTreeMap<Key, u32>,
    stand_in: StandInState,
    sticky: Option<Sticky>,
    dead: Option<Dead>,
    /// The most recently pressed key: the only one Windows auto-repeats.
    candidate: Option<Key>,
}

impl Engine {
    pub fn new(cfg: Config) -> Engine {
        Engine {
            cfg,
            typematic: Typematic::default(),
            paused: false,
            held: BTreeMap::new(),
            mods_down: BTreeSet::new(),
            mods: [ModState::default(); 8],
            keys_down: BTreeMap::new(),
            ext_mods: BTreeMap::new(),
            stand_in: StandInState::Idle,
            sticky: None,
            dead: None,
            candidate: None,
        }
    }

    pub fn config(&self) -> &Config {
        &self.cfg
    }

    /// Applies new settings. Everything held is released first.
    pub fn set_config(&mut self, cfg: Config) -> Vec<Out> {
        let out = self.reset();
        self.cfg = cfg;
        out
    }

    pub fn set_typematic(&mut self, typematic: Typematic) {
        self.typematic = typematic;
    }

    pub fn paused(&self) -> bool {
        self.paused
    }

    pub fn set_paused(&mut self, paused: bool) -> Vec<Out> {
        let mut out = Vec::new();
        if self.paused != paused {
            self.paused = paused;
            self.sticky = None;
            self.dead = None;
            let target = self.base_target();
            self.reconcile(target, &mut out);
        }
        out
    }

    pub fn process(&mut self, ev: Input, ctx: &Ctx) -> Decision {
        let mut d = Decision::default();
        if ev.down {
            self.key_down(ev, ctx, &mut d);
        } else {
            self.key_up(ev, &mut d);
        }
        d
    }

    /// Raw input revealed that a held key came from a keyboard Macaw shouldn't change
    /// (the live prediction was wrong). Keeps the stuck-key guard away from it.
    pub fn note_other_device(&mut self, key: Key) {
        if let Some(held) = self.held.get_mut(&key) {
            held.guard = false;
        }
    }

    /// Releases keys whose key-up Windows never received. Holding fn/globe on a Magic Keyboard
    /// swallows the key-up of every key pressed meanwhile; since Windows auto-repeats a key
    /// that is really held, a key that neither repeats nor comes up has been let go.
    pub fn tick(&mut self, now: u64) -> Vec<Out> {
        let mut out = Vec::new();
        if !self.cfg.fixes.stuck_keys || !self.typematic.repeats {
            return out;
        }
        let threshold = self.typematic.delay_ms + 3 * self.typematic.interval_ms + 80;
        let stuck: Vec<Key> = self
            .held
            .iter()
            .filter(|(key, held)| {
                if !held.guard || held.emission == Emission::StandIn {
                    return false;
                }
                let idle = now.saturating_sub(held.last);
                if self.candidate == Some(**key) {
                    idle > threshold
                } else {
                    held.emission != Emission::Modifier && idle > STUCK_NON_CANDIDATE_MS
                }
            })
            .map(|(key, _)| *key)
            .collect();
        for key in stuck {
            let mut d = Decision::default();
            self.key_up(
                Input {
                    key,
                    down: false,
                    time: now,
                },
                &mut d,
            );
            if d.forward {
                out.push(Out::Key { key, down: false });
            }
            out.extend(d.out);
        }
        out
    }

    /// Releases every key Windows holds because of Macaw and forgets all state. Used when the
    /// session is locked or unlocked, after sleep and on shutdown.
    pub fn reset(&mut self) -> Vec<Out> {
        let mut out = Vec::new();
        self.held.clear();
        for key in std::mem::take(&mut self.keys_down).into_keys() {
            out.push(Out::Key { key, down: false });
        }
        self.stand_in = StandInState::Idle;
        self.sticky = None;
        self.dead = None;
        self.candidate = None;
        let external = std::mem::take(&mut self.ext_mods);
        let ours = self.mods_down.clone();
        self.reconcile(WinMods::NONE, &mut out);
        for key in external.into_keys().filter(|key| !ours.contains(key)) {
            out.push(Out::Key { key, down: false });
        }
        out
    }

    fn mac(&self) -> bool {
        self.cfg.mac_mode.enabled && !self.paused
    }

    fn stand_in_key(&self) -> Option<Key> {
        match self.cfg.fn_key.stand_in {
            StandIn::CapsLock => Some(Key::CAPS_LOCK),
            StandIn::RightCommand => Some(Key::RWIN),
            StandIn::None => None,
        }
    }

    fn fn_held(&self) -> bool {
        matches!(self.stand_in, StandInState::Held { .. })
    }

    fn mark_stand_in_used(&mut self) {
        if let StandInState::Held { used, .. } = &mut self.stand_in {
            *used = true;
        }
    }

    fn modifier_held(&self, key: Key) -> bool {
        matches!(
            self.held.get(&key),
            Some(Held {
                emission: Emission::Modifier,
                ..
            })
        )
    }

    fn mac_mods(&self) -> MacMods {
        let held = |key| self.modifier_held(key);
        MacMods {
            cmd: held(Key::LWIN) || held(Key::RWIN),
            opt: held(Key::LALT) || held(Key::RALT),
            ctrl: held(Key::LCTRL) || held(Key::RCTRL),
            shift: held(Key::LSHIFT) || held(Key::RSHIFT),
        }
    }

    fn shift_mods(&self) -> WinMods {
        let mut m = WinMods::NONE;
        if self.modifier_held(Key::LSHIFT) {
            m |= WinMods::LSHIFT;
        }
        if self.modifier_held(Key::RSHIFT) {
            m |= WinMods::RSHIFT;
        }
        m
    }

    /// The Windows modifier a physical modifier stands for right now.
    fn contribution(&self, key: Key) -> WinMods {
        let mac = self.mac();
        match key {
            Key::LWIN if mac => {
                if self.sticky.is_some() {
                    WinMods::NONE
                } else {
                    WinMods::LCTRL
                }
            }
            Key::RWIN if mac => {
                if self.sticky.is_some() {
                    WinMods::NONE
                } else {
                    WinMods::RCTRL
                }
            }
            // Option waits for the next key, so pressing it alone never activates a menu bar.
            Key::LALT | Key::RALT if mac => WinMods::NONE,
            other => WinMods::from_key(other).unwrap_or(WinMods::NONE),
        }
    }

    /// The Windows modifiers that should be down when no translation is in effect.
    fn base_target(&self) -> WinMods {
        let mut t = WinMods::NONE;
        for (_, key) in WinMods::KEYS {
            if self.modifier_held(key) {
                t |= self.contribution(key);
            }
        }
        if let Some(sticky) = self.sticky
            && self.mac()
        {
            t |= sticky.mods;
        }
        t
    }

    /// What Option adds to a key it doesn't translate.
    fn option_mods(&self) -> WinMods {
        match self.cfg.mac_mode.option_key {
            OptionKey::AltGr => WinMods::LCTRL | WinMods::RALT,
            OptionKey::Alt | OptionKey::MacCharacters => {
                let mut m = WinMods::NONE;
                if self.modifier_held(Key::LALT) {
                    m |= WinMods::LALT;
                }
                if self.modifier_held(Key::RALT) {
                    m |= WinMods::RALT;
                }
                m
            }
        }
    }

    fn current_mods(&self) -> WinMods {
        WinMods::KEYS
            .iter()
            .filter(|(_, key)| self.mods_down.contains(key))
            .fold(WinMods::NONE, |acc, (bit, _)| acc | *bit)
    }

    fn note_key_emitted(&mut self) {
        for (bit, key) in WinMods::KEYS {
            if self.mods_down.contains(&key) {
                self.mods[bit.index()].key_since = true;
            }
        }
    }

    fn ext_held(&self, key: Key) -> bool {
        self.ext_mods.contains_key(&key)
    }

    /// Injects the modifier changes that make Windows hold exactly `target` for us. Modifiers
    /// another keyboard holds are left down (and not pressed again) whatever we need.
    fn reconcile(&mut self, target: WinMods, out: &mut Vec<Out>) {
        for (bit, key) in WinMods::KEYS {
            if !target.contains(bit) && self.mods_down.contains(&key) {
                self.mods_down.remove(&key);
                if self.ext_held(key) {
                    continue;
                }
                let lone = !self.mods[bit.index()].key_since;
                if lone && !matches!(key, Key::LSHIFT | Key::RSHIFT) {
                    out.push(Out::Mask);
                    self.note_key_emitted();
                }
                out.push(Out::Key { key, down: false });
            }
        }
        for (bit, key) in WinMods::KEYS {
            if target.contains(bit) && !self.mods_down.contains(&key) {
                if !self.ext_held(key) {
                    out.push(Out::Key { key, down: true });
                }
                self.mods_down.insert(key);
                self.mods[bit.index()] = ModState {
                    natural: false,
                    key_since: false,
                };
            }
        }
    }

    /// One more physical key holds `key` down in Windows.
    fn add_holder(&mut self, key: Key) {
        *self.keys_down.entry(key).or_insert(0) += 1;
    }

    /// One physical key stopped holding `key`; true if nothing holds it any more.
    fn remove_holder(&mut self, key: Key) -> bool {
        match self.keys_down.get_mut(&key) {
            Some(count) if *count > 1 => {
                *count -= 1;
                false
            }
            Some(_) => {
                self.keys_down.remove(&key);
                true
            }
            None => false,
        }
    }

    fn hold(&mut self, key: Key, time: u64, emission: Emission) {
        self.held.insert(
            key,
            Held {
                last: time,
                emission,
                guard: true,
            },
        );
    }

    /// Lets a key we don't change through. It still counts as holding its Windows key, so our
    /// own output and its key-up can't cancel each other.
    fn pass_through(&mut self, key: Key, time: u64, d: &mut Decision) {
        d.forward = true;
        if key.is_modifier() {
            *self.ext_mods.entry(key).or_insert(0) += 1;
        } else {
            self.add_holder(key);
        }
        self.held.insert(
            key,
            Held {
                last: time,
                emission: Emission::Passthrough,
                guard: false,
            },
        );
    }

    fn release_pass_through(&mut self, key: Key, d: &mut Decision) {
        if key.is_modifier() {
            match self.ext_mods.get_mut(&key) {
                Some(count) if *count > 1 => *count -= 1,
                _ => {
                    self.ext_mods.remove(&key);
                }
            }
            // If Macaw itself still holds this modifier, Windows must keep it down.
            d.forward = !self.mods_down.contains(&key);
        } else {
            d.forward = self.remove_holder(key);
        }
    }

    fn key_down(&mut self, ev: Input, ctx: &Ctx, d: &mut Decision) {
        let key = ev.key;
        if let Some(held) = self.held.get_mut(&key) {
            held.last = ev.time;
            if held.guard {
                self.candidate = Some(key);
            }
            let emission = held.emission.clone();
            self.repeat(key, emission, d);
            return;
        }
        if !self.cfg.enabled || !ctx.remap || ctx.bypass {
            self.pass_through(key, ev.time, d);
            return;
        }
        self.candidate = Some(key);
        if Some(key) == self.stand_in_key() {
            self.stand_in = StandInState::Held {
                since: ev.time,
                used: false,
            };
            self.hold(key, ev.time, Emission::StandIn);
            return;
        }
        if self.paused {
            if self.fn_held() {
                self.mark_stand_in_used();
                if key == Key::ESC {
                    self.hold(key, ev.time, Emission::Consumed);
                    d.out.extend(self.set_paused(false));
                    d.out.push(Out::Paused(false));
                    return;
                }
            }
            self.pass_through(key, ev.time, d);
            return;
        }
        if key == Key::ALTGR_CTRL {
            // Windows fakes this Ctrl in front of right Alt on AltGr layouts. Mac mode handles
            // Option itself, so the fake must not reach apps.
            if self.mac() {
                self.hold(key, ev.time, Emission::Consumed);
            } else {
                self.pass_through(key, ev.time, d);
            }
            return;
        }
        if key.is_modifier() {
            self.hold(key, ev.time, Emission::Modifier);
            self.modifier_changed(key, true, d);
            return;
        }
        self.key_down_normal(key, ev, ctx, d);
    }

    fn key_down_normal(&mut self, key: Key, ev: Input, ctx: &Ctx, d: &mut Decision) {
        let mut logical = key;
        if self.fn_held() {
            self.mark_stand_in_used();
            match tables::fn_layer(key) {
                Some(FnOut::Key(k)) => logical = k,
                Some(FnOut::Tap(mods, k)) => {
                    self.tap(mods, k, &mut d.out);
                    self.hold(key, ev.time, Emission::Consumed);
                    self.restore_base(&mut d.out);
                    return;
                }
                Some(FnOut::Action(action)) => {
                    d.out.push(Out::Action(action));
                    let emission = if action.repeats() {
                        Emission::Repeat(action)
                    } else {
                        Emission::Consumed
                    };
                    self.hold(key, ev.time, emission);
                    return;
                }
                Some(FnOut::TogglePause) => {
                    self.hold(key, ev.time, Emission::Consumed);
                    d.out.extend(self.set_paused(true));
                    d.out.push(Out::Paused(true));
                    return;
                }
                None => {}
            }
        }

        if let Some(dead) = self.dead.take() {
            match self.compose(dead, logical, ctx) {
                Composed::Char(c) => {
                    d.out.push(Out::Text(c));
                    self.note_key_emitted();
                    self.hold(key, ev.time, Emission::Consumed);
                    return;
                }
                Composed::Cancel => {
                    self.hold(key, ev.time, Emission::Consumed);
                    return;
                }
                Composed::Accent(c, used_up) => {
                    d.out.push(Out::Text(c));
                    self.note_key_emitted();
                    if used_up {
                        self.hold(key, ev.time, Emission::Consumed);
                        return;
                    }
                }
            }
        }

        if !self.mac() {
            let target = self.base_target();
            self.emit(key, logical, target, ev.time, d);
            return;
        }

        let m = self.mac_mods();
        if let Some((action, carry_shift)) = tables::lookup(ctx.profile, m, logical, self.sticky.is_some()) {
            self.apply_rule(key, logical, action, carry_shift, ev.time, d);
            return;
        }
        if m.opt && !m.cmd && !m.ctrl && self.cfg.mac_mode.option_key == OptionKey::MacCharacters {
            match tables::option_char(logical, m.shift) {
                Some(OptChar::Char(c)) => {
                    d.out.push(Out::Text(c));
                    self.note_key_emitted();
                    self.hold(key, ev.time, Emission::Text(c));
                    return;
                }
                Some(OptChar::Dead(dead)) => {
                    self.dead = Some(dead);
                    self.hold(key, ev.time, Emission::Consumed);
                    return;
                }
                None => {}
            }
        }
        let mut target = self.base_target();
        if m.opt {
            target |= self.option_mods();
        }
        self.emit(key, logical, target, ev.time, d);
    }

    fn compose(&self, dead: Dead, key: Key, ctx: &Ctx) -> Composed {
        if key == Key::BACKSPACE || key == Key::ESC {
            return Composed::Cancel;
        }
        let m = self.mac_mods();
        if m.cmd || m.ctrl {
            return Composed::Accent(dead.standalone(), false);
        }
        if key == Key::SPACE {
            return Composed::Accent(dead.standalone(), true);
        }
        if !m.opt
            && let Some(letter) = tables::us_letter(key)
            && let Some(c) = tables::compose(dead, letter, m.shift != ctx.caps_lock_on)
        {
            return Composed::Char(c);
        }
        Composed::Accent(dead.standalone(), false)
    }

    fn apply_rule(
        &mut self,
        key: Key,
        logical: Key,
        action: RuleAction,
        carry_shift: bool,
        time: u64,
        d: &mut Decision,
    ) {
        let shift = if carry_shift { self.shift_mods() } else { WinMods::NONE };
        match action {
            RuleAction::Chord { mods, key: out_key } => self.emit(key, out_key, mods | shift, time, d),
            RuleAction::Seq(steps) => {
                for &(mods, k) in steps {
                    self.tap(mods, k, &mut d.out);
                }
                self.hold(key, time, Emission::Taps(steps.to_vec()));
                self.restore_base(&mut d.out);
            }
            RuleAction::Sticky { mods, key: out_key } => {
                if self.sticky.is_none() {
                    let trigger = if self.modifier_held(Key::LWIN) {
                        Key::LWIN
                    } else {
                        Key::RWIN
                    };
                    self.sticky = Some(Sticky { trigger, mods });
                }
                let target = self.base_target();
                self.emit(key, out_key, target, time, d);
            }
            RuleAction::Action(a) => {
                d.out.push(Out::Action(a));
                let emission = if a.repeats() {
                    Emission::Repeat(a)
                } else {
                    Emission::Consumed
                };
                self.hold(key, time, emission);
            }
            RuleAction::WinKey => self.emit(key, logical, WinMods::LWIN | shift, time, d),
        }
    }

    /// Makes Windows see `out_key` go down with exactly `target` held, for as long as the
    /// physical key `phys` is held. Forwards the original event when that is already the case
    /// and nothing has to come before it (Windows handles injected input after the original).
    fn emit(&mut self, phys: Key, out_key: Key, target: WinMods, time: u64, d: &mut Decision) {
        if out_key == phys && target == self.current_mods() && d.out.is_empty() {
            d.forward = true;
            self.add_holder(phys);
            self.note_key_emitted();
            self.hold(phys, time, Emission::Forwarded);
            return;
        }
        self.reconcile(target, &mut d.out);
        d.out.push(Out::Key {
            key: out_key,
            down: true,
        });
        self.add_holder(out_key);
        self.note_key_emitted();
        self.hold(phys, time, Emission::Injected(out_key));
    }

    /// Taps `key` with exactly `mods`. A key some physical key is holding gets only a key-down
    /// (like a repeat), so it stays down for its holder.
    fn tap(&mut self, mods: WinMods, key: Key, out: &mut Vec<Out>) {
        self.reconcile(mods, out);
        out.push(Out::Key { key, down: true });
        if !self.keys_down.contains_key(&key) {
            out.push(Out::Key { key, down: false });
        }
        self.note_key_emitted();
    }

    /// Once no translated key is held, Windows' modifiers go back to what the physical
    /// modifiers stand for, so nothing a translation pressed (like Win) lingers.
    fn restore_base(&mut self, out: &mut Vec<Out>) {
        let translating = self
            .held
            .values()
            .any(|held| matches!(held.emission, Emission::Injected(_)));
        if !translating {
            let target = self.base_target();
            self.reconcile(target, out);
        }
    }

    fn repeat(&mut self, key: Key, emission: Emission, d: &mut Decision) {
        match emission {
            Emission::Passthrough => d.forward = true,
            Emission::Forwarded => {
                d.forward = true;
                self.note_key_emitted();
            }
            Emission::Injected(k) => {
                d.out.push(Out::Key { key: k, down: true });
                self.note_key_emitted();
            }
            Emission::Text(c) => d.out.push(Out::Text(c)),
            Emission::Repeat(action) => d.out.push(Out::Action(action)),
            Emission::Taps(steps) => {
                for (mods, k) in steps {
                    self.tap(mods, k, &mut d.out);
                }
                self.restore_base(&mut d.out);
            }
            Emission::Modifier => {
                // Natural modifiers keep behaving naturally: their repeats go through.
                if let Some(bit) = WinMods::from_key(key)
                    && self.mods[bit.index()].natural
                    && self.mods_down.contains(&key)
                {
                    d.forward = true;
                }
            }
            Emission::StandIn | Emission::Consumed => {}
        }
    }

    /// A modifier press adds what it stands for; a release drops whatever is no longer
    /// justified by the modifiers still held. Neither restores anything a translation removed,
    /// so modifier changes don't cause stray presses.
    fn modifier_changed(&mut self, key: Key, down: bool, d: &mut Decision) {
        let current = self.current_mods();
        let target = if down {
            current | self.contribution(key)
        } else {
            current.intersect(self.base_target())
        };
        if let Some(bit) = WinMods::from_key(key) {
            let exactly_this = if down {
                !current.contains(bit) && target == current | bit
            } else {
                current.contains(bit) && target == current.without(bit) && self.mods[bit.index()].natural
            };
            if exactly_this {
                if down {
                    d.forward = true;
                    self.mods_down.insert(key);
                    self.mods[bit.index()] = ModState {
                        natural: true,
                        key_since: false,
                    };
                } else {
                    // Another keyboard holding the same modifier keeps it down.
                    d.forward = !self.ext_held(key);
                    self.mods_down.remove(&key);
                }
                return;
            }
        }
        self.reconcile(target, &mut d.out);
    }

    fn key_up(&mut self, ev: Input, d: &mut Decision) {
        let key = ev.key;
        if self.candidate == Some(key) {
            self.candidate = None;
        }
        let Some(held) = self.held.remove(&key) else {
            d.forward = true;
            return;
        };
        match held.emission {
            Emission::Passthrough => self.release_pass_through(key, d),
            Emission::Modifier => {
                if self.sticky.is_some_and(|s| s.trigger == key) {
                    self.sticky = None;
                }
                self.modifier_changed(key, false, d);
            }
            Emission::StandIn => {
                if let StandInState::Held { since, used } = self.stand_in
                    && !used
                    && key == Key::CAPS_LOCK
                {
                    let fn_key = &self.cfg.fn_key;
                    let (switches, toggles) = (fn_key.tap_switches_layout, fn_key.tap_toggles_caps_lock);
                    let quick = ev.time.saturating_sub(since) <= fn_key.tap_timeout_ms;
                    if quick && switches {
                        // Win+Space: Windows' own "next keyboard layout".
                        self.tap(WinMods::LWIN, Key::SPACE, &mut d.out);
                        self.restore_base(&mut d.out);
                    } else if toggles && quick != switches {
                        // Capitals toggle on a tap, or on a long press when a tap switches layout.
                        d.out.push(Out::Key {
                            key: Key::CAPS_LOCK,
                            down: true,
                        });
                        d.out.push(Out::Key {
                            key: Key::CAPS_LOCK,
                            down: false,
                        });
                        self.note_key_emitted();
                    }
                }
                self.stand_in = StandInState::Idle;
            }
            Emission::Forwarded => d.forward = self.remove_holder(key),
            Emission::Injected(k) => {
                if self.remove_holder(k) {
                    d.out.push(Out::Key { key: k, down: false });
                }
                self.restore_base(&mut d.out);
            }
            Emission::Consumed | Emission::Text(_) | Emission::Repeat(_) | Emission::Taps(_) => {}
        }
    }
}
