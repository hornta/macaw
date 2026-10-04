# What the Magic Keyboard sends to Windows

Measured on 2026-09-29 with the tools in [`tools/`](../tools). Keyboard: **Magic Keyboard with Touch ID**, USB-C
(2024), USB VID 05AC PID 0321, bcdDevice 0520, connected by cable, U.S. English (ANSI) layout. Windows 11 25H2, no
Apple drivers.

## HID layout (as parsed by Windows)

| USB interface / collection | What it is |
|---|---|
| MI_00 "Device Management" | HID interface that fails to start on Windows (Code 10). Unused. |
| MI_01 Col01, Keyboard (0001:0006) | Report 0x01: modifiers, key array, and four extra bits: Consumer 0x00B8 (Eject), vendor 0x00FF:0x0003 (**fn/🌐**), Consumer 0x0040 (Menu), Consumer 0x019E (**Touch ID/lock**). Windows owns keyboard collections exclusively. |
| MI_01 Col02, Consumer Control (000C:0001) | Report 0x52: Play/Pause, Fast Forward, Rewind, Next, Previous. Feature report 0x09: one value (vendor 0xFF01:0x000B), 1 by default. |
| MI_01 Col03, vendor (FF00:0006) | Report 0x3F, 64 bytes. Purpose unknown. |
| MI_02 "Touch ID" (FF00:004B) | Reports 0x20, 0x21, 0x22. |

## Observations

- **fn/🌐 and Touch ID are invisible.** Pressing them produces no low-level hook event, no Raw Input and no report on
  any collection an app can open. Their bits live in the keyboard report, which Windows' keyboard driver ignores.
- **The F-row sends plain F1–F12.** Setting feature report 0x09 to 0 changed nothing observable; its purpose is
  unknown.
- **Keys pressed while fn is held lose their key-up.** Even when fn is released last, Windows receives the key-down
  but never the key-up, and keeps the key logically held, with no auto-repeat, until it is pressed again. F1–F12 were
  still "held" minutes later; Backspace for 21 seconds. Recordings: `crates/macaw-core/tests/fixtures/`.
- **The device is reported after the hook runs.** For every key, Raw Input (which says which keyboard sent it) arrived
  about 0.5 ms after the low-level hook returned (0 of 719 keys known in time).
- **Modifiers:** ⌘ = left/right Win, ⌥ = left/right Alt, ⌃ = left Ctrl (no right Ctrl), Caps Lock = Caps Lock. On
  layouts with AltGr, Windows inserts a fake left Ctrl (scan code 0x21D) before right ⌥.
- **Key left of 1** sends scan code 0x29 (the PC "`" position). There is no key between left Shift and Z (ANSI).
- **Auto-repeat** applies to every key, modifiers included (delay about 260 ms, interval about 31 ms on the test PC).
- An unrelated app had F10 registered as a global hotkey: F10 key-downs reached the hook but never Raw Input.

## Unknown, needs testing

- Bluetooth: does the keyboard report fn/🌐 in a separate collection there (older Apple keyboards did)?
- Other models (2015 Magic Keyboard, numeric keypad and 2021 versions) and ISO/JIS layouts (§ and < swapped?).
- Battery level, the Caps Lock delay, and what Col03 and MI_00 are for.
