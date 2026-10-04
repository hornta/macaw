//! Which shortcut table applies to the app in front.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AppProfile {
    #[default]
    Normal,
    /// Terminals: Ctrl+C must stay "interrupt", so Cmd+letter becomes Ctrl+Shift+letter.
    Terminal,
    /// File Explorer windows and the desktop.
    Explorer,
    Browser,
}

const TERMINALS: &[&str] = &[
    "windowsterminal.exe",
    "openconsole.exe",
    "conhost.exe",
    "wezterm-gui.exe",
    "alacritty.exe",
    "mintty.exe",
    "hyper.exe",
    "tabby.exe",
    "warp.exe",
    "rio.exe",
    "ghostty.exe",
    "putty.exe",
];

const TERMINAL_CLASSES: &[&str] = &["ConsoleWindowClass", "CASCADIA_HOSTING_WINDOW_CLASS", "mintty", "PuTTY"];

const EXPLORER_CLASSES: &[&str] = &["CabinetWClass", "ExploreWClass", "Progman", "WorkerW"];

const BROWSERS: &[&str] = &[
    "chrome.exe",
    "msedge.exe",
    "firefox.exe",
    "brave.exe",
    "opera.exe",
    "vivaldi.exe",
    "arc.exe",
    "zen.exe",
    "librewolf.exe",
    "waterfox.exe",
    "floorp.exe",
    "thorium.exe",
    "chromium.exe",
];

/// Apps that often run full screen but are not games (so Macaw keeps working in them).
const NOT_GAMES: &[&str] = &[
    "explorer.exe",
    "applicationframehost.exe",
    "searchhost.exe",
    "startmenuexperiencehost.exe",
    "shellexperiencehost.exe",
    "lockapp.exe",
    "textinputhost.exe",
    "vlc.exe",
    "mpv.exe",
    "mpc-hc64.exe",
    "mpc-be64.exe",
    "potplayermini64.exe",
    "wmplayer.exe",
    "powerpnt.exe",
    "winword.exe",
    "excel.exe",
    "code.exe",
    "devenv.exe",
    "idea64.exe",
    "zoom.exe",
    "ms-teams.exe",
    "obs64.exe",
];

/// Lower-cases a process name and adds ".exe" if it is missing, so settings can say "game" or "Game.exe".
pub fn normalize_exe(name: &str) -> String {
    let lower = name.trim().to_ascii_lowercase();
    if lower.ends_with(".exe") {
        lower
    } else {
        format!("{lower}.exe")
    }
}

fn listed(exe: &str, extra: &[String]) -> bool {
    extra.iter().any(|e| normalize_exe(e) == exe)
}

/// `exe` is the process file name (any case); `class` is the window class of the foreground window.
pub fn classify(exe: &str, class: &str, extra_terminals: &[String], extra_browsers: &[String]) -> AppProfile {
    let exe = normalize_exe(exe);
    if TERMINALS.contains(&exe.as_str()) || TERMINAL_CLASSES.contains(&class) || listed(&exe, extra_terminals) {
        AppProfile::Terminal
    } else if exe == "explorer.exe" && EXPLORER_CLASSES.contains(&class) {
        AppProfile::Explorer
    } else if BROWSERS.contains(&exe.as_str()) || listed(&exe, extra_browsers) {
        AppProfile::Browser
    } else {
        AppProfile::Normal
    }
}

/// Whether a full-screen window of this app should put Macaw into game mode.
pub fn fullscreen_is_game(exe: &str, profile: AppProfile, always: &[String], never: &[String]) -> bool {
    let exe = normalize_exe(exe);
    if listed(&exe, always) {
        return true;
    }
    if listed(&exe, never) || profile != AppProfile::Normal {
        return false;
    }
    !NOT_GAMES.contains(&exe.as_str())
}

/// Apps named in `always_off_in` count as games even when they are not full screen.
pub fn always_game(exe: &str, always: &[String]) -> bool {
    listed(&normalize_exe(exe), always)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles() {
        assert_eq!(classify("WindowsTerminal.exe", "", &[], &[]), AppProfile::Terminal);
        assert_eq!(
            classify("cmd.exe", "ConsoleWindowClass", &[], &[]),
            AppProfile::Terminal
        );
        assert_eq!(
            classify("explorer.exe", "CabinetWClass", &[], &[]),
            AppProfile::Explorer
        );
        assert_eq!(classify("explorer.exe", "Shell_TrayWnd", &[], &[]), AppProfile::Normal);
        assert_eq!(
            classify("msedge.exe", "Chrome_WidgetWin_1", &[], &[]),
            AppProfile::Browser
        );
        assert_eq!(classify("notepad.exe", "Notepad", &[], &[]), AppProfile::Normal);
        assert_eq!(
            classify("mytool.exe", "X", &["MyTool".into()], &[]),
            AppProfile::Terminal
        );
    }

    #[test]
    fn games() {
        assert!(fullscreen_is_game("valorant.exe", AppProfile::Normal, &[], &[]));
        assert!(!fullscreen_is_game("vlc.exe", AppProfile::Normal, &[], &[]));
        assert!(!fullscreen_is_game("chrome.exe", AppProfile::Browser, &[], &[]));
        assert!(!fullscreen_is_game(
            "game.exe",
            AppProfile::Normal,
            &[],
            &["game".into()]
        ));
        assert!(fullscreen_is_game(
            "vlc.exe",
            AppProfile::Normal,
            &["VLC.exe".into()],
            &[]
        ));
        assert!(always_game("Game.EXE", &["game".into()]));
    }
}
