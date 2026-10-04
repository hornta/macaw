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

/// How the foreground window fills the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fullscreen {
    No,
    /// A borderless window covering its whole monitor: games, but also chat apps, players and viewers.
    Borderless,
    /// Exclusive full-screen (Direct3D): games and the like.
    Exclusive,
}

/// Folders that game stores and launchers install games into (lower case).
const GAME_FOLDERS: &[&str] = &[
    "\\steamapps\\common\\",
    "\\epic games\\",
    "\\riot games\\",
    "\\xboxgames\\",
    "\\gog galaxy\\games\\",
    "\\gog games\\",
    "\\ea games\\",
    "\\origin games\\",
    "\\ubisoft game launcher\\games\\",
    "\\rockstar games\\",
    "\\battle.net\\",
    "\\games\\",
];

/// Whether a program is installed in a game library.
pub fn in_game_folder(path: &str) -> bool {
    let path = path.to_ascii_lowercase().replace('/', "\\");
    GAME_FOLDERS.iter().any(|folder| path.contains(folder))
}

fn file_name(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

/// Whether the app in front should put Macaw into game mode. Apps listed in `always` always do,
/// even when not full screen. Otherwise exclusive full-screen counts, and borderless full-screen
/// only for programs installed in a game library: many other apps go borderless full-screen too.
pub fn is_game(
    exe_path: &str,
    profile: AppProfile,
    fullscreen: Fullscreen,
    always: &[String],
    never: &[String],
) -> bool {
    let exe = normalize_exe(file_name(exe_path));
    if listed(&exe, always) {
        return true;
    }
    if fullscreen == Fullscreen::No
        || listed(&exe, never)
        || profile != AppProfile::Normal
        || NOT_GAMES.contains(&exe.as_str())
    {
        return false;
    }
    fullscreen == Fullscreen::Exclusive || in_game_folder(exe_path)
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

    const VALORANT: &str = r"C:\Riot Games\VALORANT\live\VALORANT.exe";
    const CS2: &str = r"D:\SteamLibrary\steamapps\common\Counter-Strike Global Offensive\game\bin\win64\cs2.exe";
    const TELEGRAM: &str = r"C:\Users\me\AppData\Roaming\Telegram Desktop\Telegram.exe";

    fn game(path: &str, fullscreen: Fullscreen) -> bool {
        is_game(path, AppProfile::Normal, fullscreen, &[], &[])
    }

    #[test]
    fn borderless_games_are_recognised_by_their_library() {
        assert!(game(VALORANT, Fullscreen::Borderless));
        assert!(game(CS2, Fullscreen::Borderless));
        assert!(game(
            r"C:\XboxGames\Halo Infinite\Content\HaloInfinite.exe",
            Fullscreen::Borderless
        ));
        assert!(game(r"E:\Games\Factorio\bin\x64\factorio.exe", Fullscreen::Borderless));
    }

    #[test]
    fn borderless_apps_are_not_games() {
        // Telegram's main window and media viewer are borderless and can fill the screen.
        assert!(!game(TELEGRAM, Fullscreen::Borderless));
        assert!(!game(r"C:\Program Files\VideoLAN\VLC\vlc.exe", Fullscreen::Borderless));
    }

    #[test]
    fn exclusive_full_screen_is_a_game() {
        assert!(game(r"C:\Tools\SomeGame\game.exe", Fullscreen::Exclusive));
        assert!(!game(VALORANT, Fullscreen::No));
        assert!(!is_game(
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            AppProfile::Browser,
            Fullscreen::Exclusive,
            &[],
            &[]
        ));
    }

    #[test]
    fn settings_lists_win() {
        let listed = ["telegram".to_string()];
        assert!(is_game(TELEGRAM, AppProfile::Normal, Fullscreen::No, &listed, &[]));
        assert!(!is_game(
            VALORANT,
            AppProfile::Normal,
            Fullscreen::Exclusive,
            &[],
            &["valorant.exe".into()]
        ));
    }
}
