//! Replays real recordings from a Magic Keyboard with Touch ID on Windows (see tests/fixtures).
//! Keys pressed while fn/globe was held never produced a key-up; the stuck-key guard must
//! release them, and without it they would stay held.

use macaw_core::Key;
use macaw_core::config::Config;
use macaw_core::sim::Sim;

struct Event {
    time: u64,
    key: Key,
    down: bool,
}

fn load(name: &str) -> Vec<Event> {
    let path = format!("{}/tests/fixtures/{name}.txt", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    text.lines()
        .map(|line| line.trim_start_matches('\u{feff}'))
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            assert_eq!(parts.len(), 4, "bad fixture line: {line}");
            let scan = u32::from_str_radix(parts[2], 16).expect("scan code");
            Event {
                time: parts[0].parse().expect("time"),
                key: Key::from_scan(scan, parts[3] == "e"),
                down: parts[1] == "d",
            }
        })
        .collect()
}

/// Replays the events up to `until` ms (all of them if `None`), then lets `wait` ms pass.
fn replay_until(name: &str, config: Config, until: Option<u64>, wait: u64) -> Sim {
    let events = load(name);
    assert!(!events.is_empty());
    let mut sim = Sim::new(config);
    sim.now = 0;
    for event in events.iter().filter(|e| until.is_none_or(|t| e.time <= t)) {
        sim.replay(event.time, event.key, event.down);
    }
    sim.wait(wait);
    sim
}

fn replay(name: &str, config: Config) -> Sim {
    replay_until(name, config, None, 3_000)
}

const RECORDINGS: [&str; 2] = ["magic-keyboard-usb-fn-combos-1", "magic-keyboard-usb-fn-combos-2"];

fn without_guard() -> Config {
    let mut config = Config::default();
    config.fixes.stuck_keys = false;
    config.mac_mode.enabled = false;
    config
}

#[test]
fn first_recording_ends_with_keys_stuck_without_the_guard() {
    let sim = replay(RECORDINGS[0], without_guard());
    for key in [Key::F1, Key::F9, Key::BACKSPACE, Key::ENTER, Key::LEFT, Key::UP] {
        assert!(sim.down.contains(&key), "{key:?} should be stuck: {:?}", sim.down);
    }
}

/// At 306.3 s in the second recording fn+Backspace was pressed. Windows then treated Backspace
/// as held for 21 seconds, until it was pressed again.
#[test]
fn guard_releases_fn_backspace_within_a_second() {
    const FN_BACKSPACE_MS: u64 = 306_400;
    let stuck = replay_until(RECORDINGS[1], without_guard(), Some(FN_BACKSPACE_MS), 1_000);
    assert!(stuck.down.contains(&Key::BACKSPACE), "{:?}", stuck.down);

    let fixed = replay_until(RECORDINGS[1], Config::default(), Some(FN_BACKSPACE_MS), 1_000);
    assert!(!fixed.down.contains(&Key::BACKSPACE), "{:?}", fixed.down);
}

#[test]
fn guard_releases_every_lost_key() {
    for mac_mode in [false, true] {
        for name in RECORDINGS {
            let mut config = Config::default();
            config.mac_mode.enabled = mac_mode;
            let sim = replay(name, config);
            assert!(
                sim.down.is_empty(),
                "{name} (mac mode {mac_mode}): still held {:?}",
                sim.down
            );
        }
    }
}
