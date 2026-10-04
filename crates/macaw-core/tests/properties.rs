//! Property tests: whatever the user presses, in whatever order, with the app, keyboard and
//! game state changing underneath, Windows must end up with nothing held once every physical
//! key is released.

use std::collections::BTreeSet;

use macaw_core::config::{Config, OptionKey, StandIn};
use macaw_core::sim::Sim;
use macaw_core::{AppProfile, Key};
use proptest::prelude::*;

const KEYS: &[Key] = &[
    Key::LWIN,
    Key::RWIN,
    Key::LALT,
    Key::RALT,
    Key::ALTGR_CTRL,
    Key::LCTRL,
    Key::LSHIFT,
    Key::RSHIFT,
    Key::CAPS_LOCK,
    Key::A,
    Key::C,
    Key::E,
    Key::U,
    Key::Q,
    Key::D3,
    Key::TAB,
    Key::SPACE,
    Key::LEFT,
    Key::UP,
    Key::BACKSPACE,
    Key::ENTER,
    Key::ESC,
    Key::F1,
    Key::F3,
    Key::F12,
    Key::LBRACKET,
];

const PROFILES: [AppProfile; 4] = [
    AppProfile::Normal,
    AppProfile::Terminal,
    AppProfile::Explorer,
    AppProfile::Browser,
];

#[derive(Debug, Clone)]
enum Step {
    /// Press the key if it is up, release it if it is down.
    Toggle(usize),
    /// An auto-repeat of the key, if it is held.
    Repeat(usize),
    Wait(u64),
    Profile(usize),
    Remap(bool),
    Bypass(bool),
    /// Session lock/unlock or resume: the backend resets the engine.
    Reset,
}

fn step(with_reset: bool) -> impl Strategy<Value = Step> {
    prop_oneof![
        12 => (0..KEYS.len()).prop_map(Step::Toggle),
        2 => (0..KEYS.len()).prop_map(Step::Repeat),
        2 => (0u64..150).prop_map(Step::Wait),
        1 => (0..PROFILES.len()).prop_map(Step::Profile),
        1 => any::<bool>().prop_map(Step::Remap),
        1 => any::<bool>().prop_map(Step::Bypass),
        if with_reset { 1 } else { 0 } => Just(Step::Reset),
    ]
}

fn config() -> impl Strategy<Value = Config> {
    (any::<bool>(), 0..3usize, 0..3usize, any::<bool>(), any::<bool>()).prop_map(
        |(mac, option, stand_in, taps, switches)| {
            let mut c = Config::default();
            c.mac_mode.enabled = mac;
            c.mac_mode.option_key = [OptionKey::MacCharacters, OptionKey::AltGr, OptionKey::Alt][option];
            c.fn_key.stand_in = [StandIn::CapsLock, StandIn::RightCommand, StandIn::None][stand_in];
            c.fn_key.tap_toggles_caps_lock = taps;
            c.fn_key.tap_switches_layout = switches;
            c
        },
    )
}

fn run(sim: &mut Sim, steps: &[Step]) {
    let mut held = BTreeSet::new();
    for step in steps {
        match *step {
            Step::Toggle(i) => {
                let key = KEYS[i];
                if held.remove(&key) {
                    sim.release(key);
                } else {
                    held.insert(key);
                    sim.press(key);
                }
            }
            Step::Repeat(i) => {
                if held.contains(&KEYS[i]) {
                    sim.press(KEYS[i]);
                }
            }
            Step::Wait(ms) => {
                sim.wait(ms);
            }
            Step::Profile(i) => sim.ctx.profile = PROFILES[i],
            Step::Remap(on) => sim.ctx.remap = on,
            Step::Bypass(on) => sim.ctx.bypass = on,
            Step::Reset => {
                sim.reset();
            }
        }
    }
    for key in held {
        sim.release(key);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4_000))]

    /// Without the guard and without resets, every key-down Windows sees gets exactly one
    /// key-up, and nothing is left held.
    #[test]
    fn balanced_and_nothing_left_held(cfg in config(), steps in prop::collection::vec(step(false), 0..80)) {
        let mut cfg = cfg;
        cfg.fixes.stuck_keys = false;
        let mut sim = Sim::new(cfg);
        run(&mut sim, &steps);
        prop_assert!(sim.down.is_empty(), "still down: {:?}\ntrace: {}", sim.down, sim.trace.join(" "));
        prop_assert_eq!(sim.stray_ups, 0, "stray key-ups\ntrace: {}", sim.trace.join(" "));
    }

    /// With the guard and resets in play, nothing is left held either.
    #[test]
    fn nothing_left_held_with_guard_and_resets(cfg in config(), steps in prop::collection::vec(step(true), 0..80)) {
        let mut sim = Sim::new(cfg);
        run(&mut sim, &steps);
        sim.wait(3_000);
        prop_assert!(sim.down.is_empty(), "still down: {:?}\ntrace: {}", sim.down, sim.trace.join(" "));
    }

    /// Keys from other keyboards reach Windows exactly as they were pressed.
    #[test]
    fn other_keyboards_are_untouched(cfg in config(), toggles in prop::collection::vec(0..KEYS.len(), 0..60)) {
        let mut sim = Sim::new(cfg);
        sim.ctx.remap = false;
        let mut held = BTreeSet::new();
        let mut expected = Vec::new();
        for i in toggles {
            let key = KEYS[i];
            let down = !held.remove(&key);
            if down {
                held.insert(key);
            }
            expected.push(format!("{}{key:?}", if down { '+' } else { '-' }));
            sim.event(key, down);
        }
        prop_assert_eq!(sim.trace.join(" "), expected.join(" "));
    }
}
