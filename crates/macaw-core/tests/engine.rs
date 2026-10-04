//! Scenario tests: for a sequence of physical key events on the Magic Keyboard, exactly what
//! Windows receives. Tokens: `+X`/`-X` key down/up, `mask` (lone-modifier guard), `'c'` typed
//! text, action names, `paused`/`resumed`.

use macaw_core::config::{Config, OptionKey};
use macaw_core::sim::Sim;
use macaw_core::{Action, AppProfile, Key};

fn sim() -> Sim {
    Sim::new(Config::default())
}

fn sim_with(edit: impl FnOnce(&mut Config)) -> Sim {
    let mut config = Config::default();
    edit(&mut config);
    Sim::new(config)
}

/// Nothing is left held and every key-up matched a key-down.
fn assert_clean(sim: &Sim) {
    assert!(sim.down.is_empty(), "keys still down: {:?}", sim.down);
    assert_eq!(sim.stray_ups, 0, "stray key-ups");
}

const CMD: Key = Key::LWIN;
const OPT: Key = Key::LALT;
const CTRL: Key = Key::LCTRL;
const SHIFT: Key = Key::LSHIFT;

// Command acts as Ctrl.

#[test]
fn cmd_c_copies() {
    let mut s = sim();
    s.chord(&[CMD], Key::C);
    assert_eq!(s.take(), "+LCtrl +C -C -LCtrl");
    assert_clean(&s);
}

#[test]
fn command_alone_does_nothing() {
    let mut s = sim();
    s.tap(CMD);
    assert_eq!(s.take(), "+LCtrl mask -LCtrl");
    assert_clean(&s);
}

#[test]
fn right_command_acts_as_right_ctrl() {
    let mut s = sim();
    s.chord(&[Key::RWIN], Key::V);
    assert_eq!(s.take(), "+RCtrl +V -V -RCtrl");
    assert_clean(&s);
}

// Text navigation.

#[test]
fn cmd_left_goes_to_line_start() {
    let mut s = sim();
    s.chord(&[CMD], Key::LEFT);
    assert_eq!(s.take(), "+LCtrl mask -LCtrl +Home -Home +LCtrl mask -LCtrl");
    assert_clean(&s);
}

#[test]
fn cmd_shift_left_selects_to_line_start() {
    let mut s = sim();
    s.press(CMD).press(SHIFT).tap(Key::LEFT).release(SHIFT).release(CMD);
    assert_eq!(
        s.take(),
        "+LCtrl +LShift mask -LCtrl +Home -Home +LCtrl -LShift mask -LCtrl"
    );
    assert_clean(&s);
}

#[test]
fn option_left_jumps_a_word() {
    let mut s = sim();
    s.chord(&[OPT], Key::LEFT);
    assert_eq!(s.take(), "+LCtrl +Left -Left -LCtrl");
    assert_clean(&s);
}

#[test]
fn option_backspace_deletes_a_word() {
    let mut s = sim();
    s.chord(&[OPT], Key::BACKSPACE);
    assert_eq!(s.take(), "+LCtrl +Backspace -Backspace -LCtrl");
    assert_clean(&s);
}

#[test]
fn cmd_backspace_deletes_to_line_start() {
    let mut s = sim();
    s.chord(&[CMD], Key::BACKSPACE);
    assert_eq!(
        s.take(),
        "+LCtrl mask -LCtrl +LShift +Home -Home -LShift +Backspace -Backspace +LCtrl mask -LCtrl"
    );
    assert_clean(&s);
}

#[test]
fn held_translated_key_repeats() {
    let mut s = sim();
    s.press(CMD)
        .press(Key::LEFT)
        .press(Key::LEFT)
        .release(Key::LEFT)
        .release(CMD);
    assert_eq!(s.take(), "+LCtrl mask -LCtrl +Home +Home -Home +LCtrl mask -LCtrl");
    assert_clean(&s);
}

#[test]
fn option_alone_does_nothing() {
    let mut s = sim();
    s.tap(OPT).tap(Key::RALT);
    assert_eq!(s.take(), "");
    assert_clean(&s);
}

// Apps and windows.

#[test]
fn cmd_tab_holds_alt_until_command_is_released() {
    let mut s = sim();
    s.press(CMD).tap(Key::TAB).tap(Key::TAB).release(CMD);
    assert_eq!(s.take(), "+LCtrl mask -LCtrl +LAlt +Tab -Tab +Tab -Tab -LAlt");
    assert_clean(&s);
}

#[test]
fn cmd_q_quits() {
    let mut s = sim();
    s.chord(&[CMD], Key::Q);
    assert_eq!(s.take(), "+LCtrl mask -LCtrl +LAlt +F4 -F4 -LAlt +LCtrl mask -LCtrl");
    assert_clean(&s);
}

#[test]
fn cmd_space_opens_search() {
    let mut s = sim();
    s.chord(&[CMD], Key::SPACE);
    assert_eq!(s.take(), "+LCtrl mask -LCtrl +LWin +S -S -LWin +LCtrl mask -LCtrl");
    assert_clean(&s);
}

#[test]
fn ctrl_cmd_q_locks_the_screen() {
    let mut s = sim();
    s.press(CTRL).press(CMD).tap(Key::Q).release(CMD).release(CTRL);
    assert_eq!(s.take(), "+LCtrl Lock -LCtrl");
    assert_eq!(s.actions, vec![Action::Lock]);
    assert_clean(&s);
}

#[test]
fn ctrl_cmd_letter_is_the_windows_key() {
    let mut s = sim();
    s.press(CTRL).press(CMD).tap(Key::E).release(CMD).release(CTRL);
    assert_eq!(s.take(), "+LCtrl mask -LCtrl +LWin +E -E -LWin +LCtrl mask -LCtrl");
    assert_clean(&s);
}

#[test]
fn ctrl_left_switches_desktop() {
    let mut s = sim();
    s.chord(&[CTRL], Key::LEFT);
    assert_eq!(s.take(), "+LCtrl +LWin +Left -Left -LWin -LCtrl");
    assert_clean(&s);
}

#[test]
fn cmd_shift_3_takes_a_screenshot() {
    let mut s = sim();
    s.press(CMD).press(SHIFT).tap(Key::D3).release(SHIFT).release(CMD);
    assert_eq!(
        s.take(),
        "+LCtrl +LShift mask -LCtrl -LShift +PrintScreen -PrintScreen +LCtrl +LShift -LShift mask -LCtrl"
    );
    assert_clean(&s);
}

// App profiles.

#[test]
fn terminal_cmd_c_copies_and_ctrl_c_interrupts() {
    let mut s = sim();
    s.ctx.profile = AppProfile::Terminal;
    s.chord(&[CMD], Key::C);
    assert_eq!(s.take(), "+LCtrl +LShift +C -C -LShift -LCtrl");
    s.chord(&[CTRL], Key::C);
    assert_eq!(s.take(), "+LCtrl +C -C -LCtrl");
    assert_clean(&s);
}

#[test]
fn explorer_cmd_backspace_moves_to_recycle_bin() {
    let mut s = sim();
    s.ctx.profile = AppProfile::Explorer;
    s.chord(&[CMD], Key::BACKSPACE);
    assert_eq!(s.take(), "+LCtrl mask -LCtrl +Delete -Delete +LCtrl mask -LCtrl");
    assert_clean(&s);
}

#[test]
fn browser_cmd_bracket_goes_back() {
    let mut s = sim();
    s.ctx.profile = AppProfile::Browser;
    s.chord(&[CMD], Key::LBRACKET);
    assert_eq!(
        s.take(),
        "+LCtrl mask -LCtrl +LAlt +Left -Left -LAlt +LCtrl mask -LCtrl"
    );
    assert_clean(&s);
}

// The fn stand-in (Caps Lock).

#[test]
fn caps_lock_tap_still_toggles_caps() {
    let mut s = sim();
    s.tap(Key::CAPS_LOCK);
    assert_eq!(s.take(), "+CapsLock -CapsLock");
    assert_clean(&s);
}

#[test]
fn long_caps_lock_press_does_not_toggle() {
    let mut s = sim();
    s.press(Key::CAPS_LOCK).wait(500).release(Key::CAPS_LOCK);
    assert_eq!(s.take(), "");
    assert_clean(&s);
}

#[test]
fn caps_tap_can_switch_layout_like_the_globe_key() {
    let mut s = sim_with(|c| c.fn_key.tap_switches_layout = true);
    s.tap(Key::CAPS_LOCK);
    assert_eq!(s.take(), "+LWin +Space -Space -LWin");
    // A long press on its own toggles capitals instead.
    s.press(Key::CAPS_LOCK).wait(500).release(Key::CAPS_LOCK);
    assert_eq!(s.take(), "+CapsLock -CapsLock");
    // Held with another key it is still the fn key.
    s.chord(&[Key::CAPS_LOCK], Key::BACKSPACE);
    assert_eq!(s.take(), "+Delete -Delete");
    assert_clean(&s);
}

#[test]
fn caps_lock_can_be_kept_off_entirely() {
    let mut s = sim_with(|c| {
        c.fn_key.tap_switches_layout = true;
        c.fn_key.tap_toggles_caps_lock = false;
    });
    s.press(Key::CAPS_LOCK).wait(500).release(Key::CAPS_LOCK);
    assert_eq!(s.take(), "");
    assert_clean(&s);
}

#[test]
fn caps_backspace_is_forward_delete() {
    let mut s = sim();
    s.chord(&[Key::CAPS_LOCK], Key::BACKSPACE);
    assert_eq!(s.take(), "+Delete -Delete");
    assert_clean(&s);
}

#[test]
fn caps_shift_left_selects_to_line_start() {
    let mut s = sim();
    s.chord(&[Key::CAPS_LOCK, SHIFT], Key::LEFT);
    assert_eq!(s.take(), "+LShift +Home -Home -LShift");
    assert_clean(&s);
}

#[test]
fn caps_f_keys_are_media_keys() {
    let mut s = sim();
    s.chord(&[Key::CAPS_LOCK], Key::F12);
    assert_eq!(s.take(), "+VolumeUp -VolumeUp");
    s.chord(&[Key::CAPS_LOCK], Key::F8);
    assert_eq!(s.take(), "+PlayPause -PlayPause");
    s.chord(&[Key::CAPS_LOCK], Key::F3);
    assert_eq!(s.take(), "+LWin +Tab -Tab -LWin");
    s.chord(&[Key::CAPS_LOCK], Key::F1);
    assert_eq!(s.take(), "BrightnessDown");
    assert_clean(&s);
}

#[test]
fn f_keys_are_plain_f_keys() {
    let mut s = sim();
    s.tap(Key::F5);
    assert_eq!(s.take(), "+F5 -F5");
    assert_clean(&s);
}

#[test]
fn caps_esc_pauses_and_resumes() {
    let mut s = sim();
    s.chord(&[Key::CAPS_LOCK], Key::ESC);
    assert_eq!(s.take(), "paused");
    s.chord(&[CMD], Key::C);
    assert_eq!(s.take(), "+LWin +C -C -LWin");
    s.tap(Key::CAPS_LOCK);
    assert_eq!(s.take(), "+CapsLock -CapsLock");
    s.chord(&[Key::CAPS_LOCK], Key::ESC);
    assert_eq!(s.take(), "resumed");
    s.chord(&[CMD], Key::C);
    assert_eq!(s.take(), "+LCtrl +C -C -LCtrl");
    assert_clean(&s);
}

// Option characters (Mac U.S. layout).

#[test]
fn option_2_types_trademark() {
    let mut s = sim();
    s.chord(&[OPT], Key::D2);
    assert_eq!(s.take(), "'™'");
    assert_eq!(s.typed, "™");
    assert_clean(&s);
}

#[test]
fn option_e_then_e_types_e_acute() {
    let mut s = sim();
    s.chord(&[OPT], Key::E).tap(Key::E);
    assert_eq!(s.typed, "é");
    assert_clean(&s);
}

#[test]
fn option_u_then_shift_a_types_capital_a_umlaut() {
    let mut s = sim();
    s.chord(&[OPT], Key::U).chord(&[SHIFT], Key::A);
    assert_eq!(s.take(), "+LShift 'Ä' -LShift");
    assert_clean(&s);
}

#[test]
fn accent_before_space_types_the_accent() {
    let mut s = sim();
    s.chord(&[OPT], Key::E).tap(Key::SPACE);
    assert_eq!(s.take(), "'´'");
    assert_clean(&s);
}

#[test]
fn accent_before_other_key_types_both() {
    let mut s = sim();
    s.chord(&[OPT], Key::E).tap(Key::D1);
    assert_eq!(s.take(), "'´' +1 -1");
    assert_clean(&s);
}

#[test]
fn accent_then_backspace_types_nothing() {
    let mut s = sim();
    s.chord(&[OPT], Key::E).tap(Key::BACKSPACE);
    assert_eq!(s.take(), "");
    assert_clean(&s);
}

#[test]
fn altgr_fake_ctrl_never_reaches_apps() {
    let mut s = sim();
    s.press(Key::ALTGR_CTRL)
        .press(Key::RALT)
        .tap(Key::D2)
        .release(Key::RALT)
        .release(Key::ALTGR_CTRL);
    assert_eq!(s.take(), "'™'");
    assert_clean(&s);
}

#[test]
fn option_as_altgr() {
    let mut s = sim_with(|c| c.mac_mode.option_key = OptionKey::AltGr);
    s.chord(&[OPT], Key::D2);
    assert_eq!(s.take(), "+LCtrl +RAlt +2 -2 -LCtrl -RAlt");
    assert_clean(&s);
}

// Leaving things alone.

#[test]
fn other_keyboards_pass_through() {
    let mut s = sim();
    s.ctx.remap = false;
    s.chord(&[CMD], Key::C).tap(Key::CAPS_LOCK);
    assert_eq!(s.take(), "+LWin +C -C -LWin +CapsLock -CapsLock");
    assert_clean(&s);
}

#[test]
fn games_pass_through() {
    let mut s = sim();
    s.ctx.bypass = true;
    s.chord(&[CMD], Key::C);
    assert_eq!(s.take(), "+LWin +C -C -LWin");
    assert_clean(&s);
}

#[test]
fn game_starting_while_command_is_held_releases_cleanly() {
    let mut s = sim();
    s.press(CMD);
    s.ctx.bypass = true;
    s.tap(Key::C).release(CMD);
    assert_eq!(s.take(), "+LCtrl +C -C mask -LCtrl");
    assert_clean(&s);
}

#[test]
fn mac_mode_off_keeps_windows_keys() {
    let mut s = sim_with(|c| c.mac_mode.enabled = false);
    s.chord(&[CMD], Key::C);
    assert_eq!(s.take(), "+LWin +C -C -LWin");
    s.chord(&[Key::CAPS_LOCK], Key::BACKSPACE);
    assert_eq!(s.take(), "+Delete -Delete");
    assert_clean(&s);
}

#[test]
fn disabled_passes_everything_through() {
    let mut s = sim_with(|c| c.enabled = false);
    s.chord(&[CMD], Key::C).tap(Key::CAPS_LOCK);
    assert_eq!(s.take(), "+LWin +C -C -LWin +CapsLock -CapsLock");
    assert_clean(&s);
}

// Stuck keys.

#[test]
fn guard_releases_keys_that_lost_their_key_up() {
    let mut s = sim();
    s.press(Key::F1).wait(600);
    assert_eq!(s.take(), "+F1 -F1");
    assert_clean(&s);
}

#[test]
fn guard_leaves_held_modifiers_alone() {
    let mut s = sim();
    s.press(SHIFT).tap(Key::A).wait(3_000);
    assert_eq!(s.down.iter().copied().collect::<Vec<_>>(), vec![Key::LSHIFT]);
    s.release(SHIFT);
    assert_clean(&s);
}

#[test]
fn guard_leaves_repeating_keys_alone() {
    let mut s = sim();
    s.press(Key::A);
    for _ in 0..30 {
        s.wait(40).press(Key::A);
    }
    s.release(Key::A);
    assert_clean(&s);
}

#[test]
fn guard_can_be_turned_off() {
    let mut s = sim_with(|c| c.fixes.stuck_keys = false);
    s.press(Key::F1).wait(3_000);
    assert!(s.down.contains(&Key::F1));
}

#[test]
fn reset_releases_everything() {
    let mut s = sim();
    s.press(CMD).press(Key::A).press(Key::CAPS_LOCK);
    s.reset();
    assert!(s.down.is_empty(), "{:?}", s.down);
}
