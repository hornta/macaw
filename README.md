<p align="center"><img src="assets/macaw-256.png" width="96" alt="Macaw icon"></p>

# Macaw

**Use an Apple Magic Keyboard on Windows the way you use it on a Mac.**

Macaw is a small, free, open-source tray app for Windows 10 and 11. It makes ⌘ Command, ⌥ Option and the
fn key behave the Mac way, and fixes a Windows bug that leaves keys stuck after fn combinations. No drivers, no
network access, and it never records what you type.

> **Status: early preview (0.1).** Tested with the Magic Keyboard with Touch ID (USB-C, 2024) over USB. Other
> Magic Keyboards and Bluetooth should work; please [report](https://github.com/hornta/macaw/issues) how yours behaves.

## What it does

- **Mac shortcuts.** ⌘ acts as Ctrl, so ⌘C, ⌘V, ⌘Z, ⌘S, ⌘T and every other Ctrl shortcut work in every app.
  On top of that, macOS habits work too:
  | Mac | Does |
  |---|---|
  | ⌘Tab (hold ⌘) | switch apps |
  | ⌘Q · ⌘H · ⌘M | quit · hide · minimize |
  | ⌘Space | Windows Search |
  | ⌘⇧3 · ⌘⇧4 · ⌘⇧5 | screenshot · snip · screen recording |
  | ⌘← ⌘→ · ⌘↑ ⌘↓ | start/end of line · of document |
  | ⌥← ⌥→ · ⌥⌫ · ⌘⌫ | word jumps · delete word · delete to line start |
  | ⌃← ⌃→ · ⌃↑ | switch desktop · Task View |
  | ⌃⌥ + arrows | snap windows |
  | ⌃⌘Q | lock the screen |
  | ⌃⌘ + any key | the Windows key with that key (⌃⌘E Explorer, ⌃⌘V clipboard history) |

  In terminals ⌘C copies and ⌃C interrupts; in File Explorer ⌘⌫ moves to the Recycle Bin and ⌘↓ opens; in browsers
  ⌘[ and ⌘] go back and forward.
- **A working fn key.** Windows can't see the Magic Keyboard's fn/🌐 key, so **Caps Lock** stands in. Hold it and press
  ⌫ for Delete, the arrows for Home/End/Page Up/Page Down, or the F-keys for brightness (on monitors with DDC/CI),
  Task View, Search, voice typing, notifications, media and volume. Tap Caps Lock for capitals, or set it to switch
  keyboard layout like the 🌐 key does on a Mac.
- **Option characters** like a Mac's U.S. layout: ⌥2 = ™, ⌥A = å, ⌥E then E = é, ⌥U then O = ö.
- **Stuck-key fix.** Without Apple's drivers, Windows never sees keys pressed while fn/🌐 is held being released, and
  treats them as held down. Macaw notices and releases them.
- **Stays out of the way.** Only Apple keyboards are changed (other keyboards are untouched), Macaw steps aside in
  full-screen games, and Caps Lock + Esc pauses it.
- **Layout check.** If Windows types with a layout that doesn't match your keys (say Swedish on a U.S. keyboard),
  Macaw tells you and fixes it in one click.

## Install

Download **`Macaw-<version>-setup.exe`** from [Releases](https://github.com/hornta/macaw/releases) and run it. One
installer covers x64 and ARM64 PCs. By default it sets Macaw to start when you sign in, with admin rights, so it also
works in admin apps such as Task Manager. You can turn that off in the installer or later in Macaw's menu.

Prefer no installer? Each release also has a portable zip with just `macaw.exe`.

> Until code signing is active (see below), Windows SmartScreen may warn before the installer runs. Choose
> **More info**, then **Run anyway**.

## Using Macaw

Macaw lives in the tray as a red **M** (grey when paused). Right-click it to:

- pause Macaw (or press Caps Lock + Esc)
- turn Mac shortcuts on or off, or let Macaw change all keyboards instead of just Apple ones
- fix the keyboard layout, when Windows' layout doesn't match your keys
- start Macaw at sign-in with admin rights
- open the settings file or the log folder

Everything else is in the settings file (`%APPDATA%\Macaw\config.toml`). It explains each option and changes take
effect as soon as you save it.

## What Windows can't do without a driver

Windows never receives the fn/🌐 key or the Touch ID key from a Magic Keyboard, so no app can react to them. Macaw
uses Caps Lock as fn instead. Touch ID needs a Mac and can't work on Windows at all.

## Privacy

Macaw changes key presses as they happen and never stores or sends them. It has no network access at all.

This program will not transfer any information to other networked systems unless specifically requested by the user
or the person installing or operating it.

The log file (`%LOCALAPPDATA%\Macaw\macaw.log`) records what Macaw does, such as starting, devices and settings, and
never what you type. With the diagnostics option on, it also records key codes, but letters and digits are always
replaced by a placeholder.

## Code signing policy

Release builds are signed so that Windows can verify they come from this project. Signing is being set up with
[SignPath Foundation](https://signpath.org), which provides free code signing to open-source projects. See
[docs/CODE_SIGNING.md](docs/CODE_SIGNING.md) for the policy, the team roles and what gets signed.

## Building from source

Needs Rust (stable, MSVC toolchain) and the Visual Studio Build Tools.

```
cargo test --workspace
cargo build --release      # target\release\macaw.exe
```

`crates/macaw-core` is the remapping engine: pure Rust, no Windows code, covered by scenario, property and replay
tests. `crates/macaw` is the Windows app: keyboard hook, tray, settings. [docs/DECISIONS.md](docs/DECISIONS.md)
explains the design and [docs/HARDWARE.md](docs/HARDWARE.md) what we measured on the keyboard.

## License

[MIT](LICENSE). Macaw is not affiliated with Apple. Apple, Magic Keyboard and Touch ID are trademarks of Apple Inc.
