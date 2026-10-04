//! The tray icon, its menu and notifications, on the main thread. It also watches the settings
//! file and hands new settings to the input thread.

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::SystemTime;

use macaw_core::Config;
use macaw_core::config::{ApplyTo, TEMPLATE};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_INFO, NIIF_WARNING, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NIM_SETVERSION, NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu,
    DispatchMessageW, GetCursorPos, GetMessageW, GetSystemMetrics, HICON, LR_DEFAULTCOLOR, MF_CHECKED, MF_GRAYED,
    MF_SEPARATOR, MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, SM_CXSMICON,
    SetForegroundWindow, SetTimer, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx, TranslateMessage,
    WM_APP, WM_CLOSE, WM_CONTEXTMENU, WM_ENDSESSION, WM_NULL, WM_TIMER, WM_USER, WNDCLASSEXW,
};

use crate::shared::{Command, Notice, Shared, WM_APP_NOTICE};
use crate::win::{self, copy_wide, wide};
use crate::{autostart, layout, log_info, log_warn, paths};

static ICON_ON: &[u8] = include_bytes!("../../../assets/macaw.ico");
static ICON_OFF: &[u8] = include_bytes!("../../../assets/macaw-paused.ico");

const WM_APP_TRAY: u32 = WM_APP + 10;
const NIN_SELECT: u32 = WM_USER;
const NIN_KEYSELECT: u32 = WM_USER + 1;
const ICON_ID: u32 = 1;
const TIMER_SETTINGS: usize = 1;

const ID_PAUSE: usize = 1;
const ID_MAC: usize = 2;
const ID_ALL_KEYBOARDS: usize = 3;
const ID_LAYOUT: usize = 4;
const ID_AUTOSTART: usize = 5;
const ID_SETTINGS: usize = 6;
const ID_LOGS: usize = 7;
const ID_QUIT: usize = 8;

/// Explorer broadcasts this after restarting; the icon must then be added again.
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

struct Tray {
    shared: Arc<Shared>,
    hwnd: HWND,
    icon_on: HICON,
    icon_off: HICON,
    /// Settings as saved in the file.
    file_config: Config,
    /// Menu toggles for this session, on top of the file.
    mac_override: Option<bool>,
    all_override: Option<bool>,
    config_path: PathBuf,
    config_stamp: Option<SystemTime>,
    autostart: bool,
}

enum MenuItem {
    Entry {
        id: usize,
        text: String,
        checked: bool,
        enabled: bool,
    },
    Separator,
}

thread_local! {
    static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
}

/// Runs `f` on the tray unless it is already in use further up the stack (a nested message
/// during a menu or a permission prompt), in which case the message is skipped.
fn with_tray<R>(f: impl FnOnce(&mut Tray) -> R) -> Option<R> {
    TRAY.with(|cell| cell.try_borrow_mut().ok().and_then(|mut tray| tray.as_mut().map(f)))
}

pub fn run(shared: Arc<Shared>, config: Config, first_run: bool, config_error: Option<String>) {
    let Some(hwnd) = create_window() else {
        crate::log_error!("could not create the tray window");
        return;
    };
    // SAFETY: plain calls.
    let (taskbar_created, icon_size) = unsafe {
        (
            RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
            GetSystemMetrics(SM_CXSMICON),
        )
    };
    TASKBAR_CREATED.store(taskbar_created, Ordering::Relaxed);
    let config_path = paths::config_file();
    let tray = Tray {
        shared: shared.clone(),
        hwnd,
        icon_on: load_icon(ICON_ON, icon_size),
        icon_off: load_icon(ICON_OFF, icon_size),
        file_config: config,
        mac_override: None,
        all_override: None,
        config_stamp: modified(&config_path),
        config_path,
        autostart: autostart::exists(),
    };
    tray.add_icon();
    if let Some(error) = &config_error {
        tray.balloon("Settings file has a problem", &first_line(error), true);
    } else if first_run {
        tray.balloon(
            "Macaw is running",
            "Hold Caps Lock for the fn keys; tap it for capitals. Right-click the M icon for options.",
            false,
        );
    }
    TRAY.with(|cell| *cell.borrow_mut() = Some(tray));
    shared.set_tray_window(hwnd);
    // SAFETY: timer on our own window; standard message loop.
    unsafe {
        SetTimer(hwnd, TIMER_SETTINGS, 1_000, None);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    with_tray(|tray| tray.remove_icon());
}

fn create_window() -> Option<HWND> {
    let class = wide("MacawTray");
    // SAFETY: the class name outlives registration and creation; the window stays hidden.
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(tray_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassExW(&wc);
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        (!hwnd.is_null()).then_some(hwnd)
    }
}

/// Picks the best image from an .ico file for the requested size.
fn load_icon(ico: &[u8], size: i32) -> HICON {
    let read_u16 = |at: usize| u16::from_le_bytes([ico[at], ico[at + 1]]) as usize;
    let read_u32 = |at: usize| u32::from_le_bytes([ico[at], ico[at + 1], ico[at + 2], ico[at + 3]]) as usize;
    let count = read_u16(4);
    let entries = (0..count).map(|i| {
        let at = 6 + i * 16;
        let dim = if ico[at] == 0 { 256 } else { i32::from(ico[at]) };
        (dim, read_u32(at + 8), read_u32(at + 12))
    });
    // The smallest image at least as large as asked for, else the largest.
    let best = entries.fold(None::<(i32, usize, usize)>, |best, entry| match best {
        None => Some(entry),
        Some(b) if (entry.0 >= size && (b.0 < size || entry.0 < b.0)) || (b.0 < size && entry.0 > b.0) => Some(entry),
        keep => keep,
    });
    let Some((_, len, offset)) = best else {
        return std::ptr::null_mut();
    };
    // SAFETY: the slice lies within the embedded icon file.
    unsafe {
        CreateIconFromResourceEx(
            ico[offset..offset + len].as_ptr(),
            len as u32,
            1,
            0x0003_0000,
            size,
            size,
            LR_DEFAULTCOLOR,
        )
    }
}

fn modified(path: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn first_line(text: &str) -> String {
    text.lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or(text)
        .trim()
        .to_string()
}

unsafe extern "system" fn tray_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let handled = catch_unwind(AssertUnwindSafe(|| tray_message(hwnd, msg, wparam, lparam))).unwrap_or(false);
    if handled {
        return 0;
    }
    // SAFETY: default processing.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn tray_message(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> bool {
    match msg {
        WM_APP_TRAY => {
            let event = (lparam & 0xFFFF) as u32;
            if matches!(event, WM_CONTEXTMENU | NIN_SELECT | NIN_KEYSELECT) {
                show_menu(hwnd);
            }
        }
        WM_APP_NOTICE => {
            if let Some(notice) = Notice::from_raw(wparam) {
                with_tray(|tray| tray.notice(notice));
            }
        }
        WM_TIMER if wparam == TIMER_SETTINGS => {
            with_tray(|tray| tray.poll_settings());
        }
        // Asked to close (installer upgrades use `taskkill`), or Windows is signing out.
        WM_CLOSE => {
            with_tray(|tray| tray.quit());
        }
        WM_ENDSESSION if wparam != 0 => {
            with_tray(|tray| tray.quit());
        }
        _ if msg != 0 && msg == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            with_tray(|tray| tray.add_icon());
        }
        _ => return false,
    }
    true
}

fn show_menu(hwnd: HWND) {
    let Some(items) = with_tray(|tray| tray.menu_items()) else {
        return;
    };
    // SAFETY: the menu is built, shown and destroyed here; item strings outlive AppendMenuW,
    // which copies them. No tray borrow is held while the menu's modal loop runs.
    let command = unsafe {
        let menu = CreatePopupMenu();
        for item in &items {
            match item {
                MenuItem::Separator => {
                    AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
                }
                MenuItem::Entry {
                    id,
                    text,
                    checked,
                    enabled,
                } => {
                    let flags =
                        MF_STRING | if *checked { MF_CHECKED } else { 0 } | if *enabled { 0 } else { MF_GRAYED };
                    let text = wide(text);
                    AppendMenuW(menu, flags, *id, text.as_ptr());
                }
            }
        }
        let mut cursor = POINT { x: 0, y: 0 };
        GetCursorPos(&mut cursor);
        SetForegroundWindow(hwnd);
        let command = TrackPopupMenuEx(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
            cursor.x,
            cursor.y,
            hwnd,
            std::ptr::null(),
        );
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        command
    };
    if command > 0 {
        with_tray(|tray| tray.command(command as usize));
    }
}

impl Tray {
    fn effective(&self) -> Config {
        let mut config = self.file_config.clone();
        if let Some(mac) = self.mac_override {
            config.mac_mode.enabled = mac;
        }
        if let Some(all) = self.all_override {
            config.apply_to = if all { ApplyTo::All } else { ApplyTo::Apple };
        }
        config
    }

    fn status(&self) -> String {
        let s = &self.shared;
        let text = if s.paused.load(Ordering::Relaxed) {
            "Macaw is paused"
        } else if s.game_in_front.load(Ordering::Relaxed) {
            "Macaw steps aside for this game"
        } else if s.admin_app_in_front.load(Ordering::Relaxed) {
            "Macaw can't change keys in this admin app"
        } else if !s.apple_connected.load(Ordering::Relaxed) && self.effective().apply_to == ApplyTo::Apple {
            "No Apple keyboard connected"
        } else {
            "Macaw is on"
        };
        text.to_string()
    }

    fn inactive(&self) -> bool {
        let s = &self.shared;
        s.paused.load(Ordering::Relaxed) || s.game_in_front.load(Ordering::Relaxed)
    }

    fn base(&self) -> NOTIFYICONDATAW {
        // SAFETY: an all-zero NOTIFYICONDATAW is valid.
        let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = self.hwnd;
        data.uID = ICON_ID;
        data
    }

    fn add_icon(&self) {
        let mut data = self.base();
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP;
        data.uCallbackMessage = WM_APP_TRAY;
        data.hIcon = if self.inactive() { self.icon_off } else { self.icon_on };
        copy_wide(&mut data.szTip, &self.status());
        data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        // SAFETY: a fully initialised NOTIFYICONDATAW.
        unsafe {
            Shell_NotifyIconW(NIM_ADD, &data);
            Shell_NotifyIconW(NIM_SETVERSION, &data);
        }
    }

    fn update_icon(&self) {
        let mut data = self.base();
        data.uFlags = NIF_ICON | NIF_TIP | NIF_SHOWTIP;
        data.hIcon = if self.inactive() { self.icon_off } else { self.icon_on };
        copy_wide(&mut data.szTip, &self.status());
        // SAFETY: a fully initialised NOTIFYICONDATAW.
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
    }

    fn remove_icon(&self) {
        let data = self.base();
        // SAFETY: removing our own icon and freeing our icons.
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &data);
            DestroyIcon(self.icon_on);
            DestroyIcon(self.icon_off);
        }
    }

    fn balloon(&self, title: &str, text: &str, warning: bool) {
        let mut data = self.base();
        data.uFlags = NIF_INFO;
        copy_wide(&mut data.szInfoTitle, title);
        copy_wide(&mut data.szInfo, text);
        data.dwInfoFlags = if warning { NIIF_WARNING } else { NIIF_INFO };
        // SAFETY: a fully initialised NOTIFYICONDATAW.
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
    }

    fn menu_items(&self) -> Vec<MenuItem> {
        let config = self.effective();
        let s = &self.shared;
        let entry = |id, text: &str, checked, enabled| MenuItem::Entry {
            id,
            text: text.to_string(),
            checked,
            enabled,
        };
        let mut status = self.status();
        if !s.elevated {
            status.push_str(" (without admin rights)");
        }
        let mut items = vec![
            entry(0, &status, false, false),
            MenuItem::Separator,
            entry(
                ID_PAUSE,
                "Pause Macaw\tCaps Lock+Esc",
                s.paused.load(Ordering::Relaxed),
                true,
            ),
            entry(ID_MAC, "Mac shortcuts", config.mac_mode.enabled, true),
            entry(
                ID_ALL_KEYBOARDS,
                "Change all keyboards, not just Apple ones",
                config.apply_to == ApplyTo::All,
                true,
            ),
            MenuItem::Separator,
        ];
        let mismatch = s.layout_mismatch.load(Ordering::Relaxed);
        if mismatch != 0 {
            let text = format!("Type U.S. symbols (Windows uses {})", layout::name(mismatch));
            items.push(entry(ID_LAYOUT, &text, false, true));
        }
        items.extend([
            entry(
                ID_AUTOSTART,
                "Start at sign-in, with admin rights",
                self.autostart,
                true,
            ),
            entry(ID_SETTINGS, "Open settings file", false, true),
            entry(ID_LOGS, "Open log folder", false, true),
            MenuItem::Separator,
            entry(ID_QUIT, "Quit Macaw", false, true),
        ]);
        items
    }

    fn command(&mut self, id: usize) {
        match id {
            ID_PAUSE => self
                .shared
                .send(Command::Pause(!self.shared.paused.load(Ordering::Relaxed))),
            ID_MAC => {
                self.mac_override = Some(!self.effective().mac_mode.enabled);
                self.apply();
            }
            ID_ALL_KEYBOARDS => {
                self.all_override = Some(self.effective().apply_to != ApplyTo::All);
                self.apply();
            }
            ID_LAYOUT => self.fix_layout(),
            ID_AUTOSTART => self.toggle_autostart(),
            ID_SETTINGS => self.open_settings(),
            ID_LOGS => {
                if let Some(dir) = paths::log_file().parent() {
                    win::shell_open(dir);
                }
            }
            ID_QUIT => self.quit(),
            _ => {}
        }
    }

    fn apply(&self) {
        self.shared.send(Command::Config(Box::new(self.effective())));
        self.update_icon();
    }

    fn notice(&mut self, notice: Notice) {
        self.update_icon();
        if notice == Notice::LayoutMismatch {
            let layout = self.shared.layout_mismatch.load(Ordering::Relaxed);
            if layout != 0 {
                let text = format!(
                    "Windows types with the {} layout, but your keyboard is printed U.S. Choose \"Type U.S. symbols\" in the Macaw menu to fix it.",
                    layout::name(layout)
                );
                self.balloon("Keys may type the wrong symbols", &text, true);
            }
        }
    }

    fn poll_settings(&mut self) {
        let stamp = modified(&self.config_path);
        if stamp == self.config_stamp {
            return;
        }
        self.config_stamp = stamp;
        let Ok(text) = std::fs::read_to_string(&self.config_path) else {
            return;
        };
        match Config::parse(&text) {
            Ok(config) => {
                if config != self.file_config {
                    self.file_config = config;
                    self.apply();
                    log_info!("settings reloaded");
                }
            }
            Err(error) => {
                log_warn!("settings file error: {error}");
                self.balloon("Settings file has a problem", &first_line(&error), true);
            }
        }
    }

    fn fix_layout(&mut self) {
        let language = match self.shared.layout_language.load(Ordering::Relaxed) {
            0 => 0x0409,
            language => language,
        };
        match layout::use_us_layout(language) {
            Ok(()) => {
                log_info!("added the U.S. layout for language {language:04X} and made it the default");
                self.shared.layout_mismatch.store(0, Ordering::Relaxed);
                self.balloon(
                    "U.S. layout added",
                    "Your keys now type what's printed on them. If a window still types the old symbols, press Win+Space in it.",
                    false,
                );
            }
            Err(error) => {
                log_warn!("could not add the U.S. layout: {error}");
                self.balloon("Couldn't change the layout", &error, true);
            }
        }
    }

    fn toggle_autostart(&mut self) {
        if self.autostart {
            match autostart::run_elevated("--uninstall-autostart") {
                Ok(true) => {
                    self.autostart = false;
                    self.balloon(
                        "Start at sign-in turned off",
                        "Macaw no longer starts by itself.",
                        false,
                    );
                }
                Ok(false) => self.balloon("Couldn't turn off start at sign-in", "See the log for details.", true),
                Err(_) => {}
            }
            return;
        }
        let args = format!("--install-autostart --user \"{}\"", autostart::current_user());
        match autostart::run_elevated(&args) {
            Ok(true) => {
                self.autostart = true;
                if self.shared.elevated {
                    self.balloon(
                        "Start at sign-in turned on",
                        "Macaw now starts with admin rights when you sign in.",
                        false,
                    );
                } else if autostart::start() {
                    // The task starts an elevated Macaw, which takes over once this one quits.
                    log_info!("restarting with admin rights");
                    self.quit();
                }
            }
            Ok(false) => self.balloon("Couldn't turn on start at sign-in", "See the log for details.", true),
            Err(_) => {}
        }
    }

    fn open_settings(&self) {
        if !self.config_path.exists() {
            if let Some(dir) = self.config_path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&self.config_path, TEMPLATE);
        }
        if !win::shell_open(&self.config_path) {
            let _ = std::process::Command::new("notepad.exe").arg(&self.config_path).spawn();
        }
    }

    fn quit(&self) {
        self.shared.send(Command::Quit);
        // SAFETY: ends the main message loop.
        unsafe { PostQuitMessage(0) };
    }
}
