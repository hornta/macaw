# Design decisions

Why Macaw is built the way it is. Read this before changing behaviour; update it when a decision changes.
Decisions were made with the project owner in a design interview on 2026-09-29 and in testing afterwards.

## Product

| Decision | Why |
|---|---|
| **Mac mode = "swap + translations".** ⌘ acts as Ctrl immediately; a table translates macOS-only habits (⌘Tab, ⌘Q, ⌥/⌘+arrows...); terminal, File Explorer and browser profiles override parts of it. | A plain Cmd↔Ctrl swap breaks ⌘Tab, text navigation and terminals (Cmd+C would send SIGINT). Translating only listed shortcuts doesn't scale: every app's Ctrl shortcut would need an entry. Kinto and Toshy ended up with the same design. |
| **For everyone, built first for a daily Mac user.** | The owner uses a Mac for work. Presets for "PC positional" and "minimal" users can come later; the engine already supports Mac mode off. |
| **Free and open source, MIT.** | A keyboard hook can see every keystroke, so open source is the strongest trust signal. It also qualifies for free code signing (SignPath Foundation). |
| **No driver in v1.** The engine sits behind the hook backend so a signed driver can plug in later. | Measured: Windows never delivers fn/🌐 or Touch ID to user mode (see HARDWARE.md). A kernel filter driver would fix that, but needs an EV certificate, Microsoft attestation signing for every release, HVCI-compatible kernel code and carries crash risk. Not worth it before the product is proven. |
| **Caps Lock stands in for fn.** Tap = Caps Lock, or (setting `tap_switches_layout`) switch layout like 🌐, with a long press for capitals. | Left pinky, like the real fn corner; the fn functions are on the right side. Right ⌥ is needed as AltGr on European layouts; right ⌘+arrows is one-handed and awkward. |
| **F1–F12 behave like a PC keyboard.** fn(Caps)+F-keys give the printed functions. | The owner's explicit choice ("act normal as a normal PC keyboard"). |
| **The keyboard types what is printed on it** (U.S. for the owner). Macaw detects a mismatching Windows layout and adds the U.S. layout in one click. | The owner uses ABC/U.S. on the Mac; Windows was set to Swedish (`?` typed `-`). Only warn when no U.S. layout is installed: switching to another layout on purpose is fine. |
| **Works in admin apps** via a scheduled task that starts Macaw at sign-in with highest privileges. | Windows blocks input from normal apps to elevated windows (UIPI). A task needs one UAC prompt at setup and none afterwards. `uiAccess` would avoid full admin rights but needs a signed binary in Program Files: revisit once signing works. |
| **Only Apple keyboards are changed by default.** | Laptop keyboards and gaming keyboards must stay PC keyboards. |
| **Engine first, settings window later** (Tauri with plain TypeScript, no framework). | The owner wanted to try it soon. Web UI is where AI-written code is most reliable; no framework keeps npm dependencies minimal. |
| **AI maintains the code.** Every behaviour has a test; dependencies stay minimal (`windows-sys`, `serde`, `toml`; `proptest` for tests; `embed-resource` for the build). | The owner doesn't read Rust or TypeScript, so tests are the only review. |
| **Name: Macaw.** Never "Apple", "Magic" or "Magic Keyboard" in the product name; "for Apple Magic Keyboard" in descriptions is fine. | Trademarks. |
| **No telemetry, no network access.** The log never contains typed text. | Trust, and SignPath's privacy rule. |

## Engine (crates/macaw-core)

- **A pure state machine.** `Engine::process` takes a key event and returns a `Decision`: forward the original event
  and/or inject new input. No Windows code, so everything runs in tests through `sim::Sim`, a model of Windows.
- **Forward vs inject.** Windows handles injected input after the original event, so the original is forwarded only
  when nothing must come before it. Otherwise it is swallowed and everything is injected.
- **Modifiers are reconciled, not mirrored.** The engine knows which modifier keys it has made Windows hold and
  injects only the difference to what the next output needs. ⌘ maps to Ctrl immediately (so ⌘-click works);
  ⌥ waits for the next key (so tapping ⌥ never opens a menu bar). After a translated key is released, modifiers return
  to what is physically held.
- **Mask key.** Releasing a modifier no other key was pressed with would look like a tap (Alt opens the menu bar, Win
  the Start menu, double Ctrl starts PowerToys' Find My Mouse). The engine taps an unassigned key (VK 0xE8) first,
  like AutoHotkey does.
- **Reference counting.** Several physical keys can hold the same Windows key (Left, and ⌘[ in a browser). It goes up
  only when the last one is released. Modifiers held by other keyboards are tracked separately, so neither side
  releases the other's.
- **Stuck-key guard.** Holding fn/🌐 makes the keyboard lose the key-up of every key pressed meanwhile. Windows
  auto-repeats a held key, so the most recent key that neither repeats nor comes up within delay + 3 intervals has been
  released (other non-modifiers after 2 s). Off when Filter Keys disables repeat.
- **Device prediction.** The low-level hook runs before Windows reports which keyboard a key came from (measured:
  0 of 719 keys). The hook therefore uses the device of the previous key; only the first key after switching keyboards
  can be wrong. The guard waits for the real device.

## Not done yet

Settings window; signed releases (in progress); Bluetooth and other Magic Keyboard models (needs testers); ISO and JIS
keyboards; per-keyboard layout switching; Option characters for non-U.S. Mac layouts; battery level; `uiAccess`;
a driver for fn/🌐 and Touch ID.
