# Changelog

## [0.1.1] - 2026-10-04

- **Fixed:** Telegram and other apps that fill the screen without a border were mistaken for games, which turned
  Mac shortcuts off while they were in front. Borderless full-screen now counts as a game only for programs
  installed by a game store or in a "Games" folder; exclusive full-screen still always counts.
- Macaw now closes cleanly when the installer upgrades it or Windows signs out.

## [0.1.0] - 2026-10-04

First preview.

- **Mac shortcuts:** ⌘ acts as Ctrl, plus macOS habits: ⌘Tab, ⌘Q, ⌘H/⌘M, ⌘Space, ⌘⇧3/4/5, ⌘/⌥ + arrows,
  ⌘⌫/⌥⌫, ⌃←/→ desktops, ⌃⌥ + arrows to snap windows, ⌃⌘Q to lock, ⌃⌘ + key for Windows-key shortcuts.
  Terminal, File Explorer and browser specifics.
- **Caps Lock as fn:** Delete, Home/End, Page Up/Down, and brightness, Task View, Search, voice typing,
  notifications, media and volume on the F-keys. Tap for capitals, or switch keyboard layout like the 🌐 key.
- **Option characters** of the Mac U.S. layout, including accents (⌥E, ⌥U, ⌥I, ⌥N, ⌥`).
- **Stuck-key fix** for keys Windows thinks are still held after fn/🌐 combinations.
- Changes only Apple keyboards by default, steps aside in full-screen games, pauses with Caps Lock + Esc.
- Warns when the Windows layout doesn't match the keyboard, and adds the U.S. layout in one click.
- Start at sign-in with admin rights, so Macaw also works in admin apps.
- Brightness control for external monitors over DDC/CI.
- Installer for x64 and ARM64 PCs, plus portable zips.
