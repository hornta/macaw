//! The input thread. It owns the low-level keyboard hook, Raw Input device tracking,
//! foreground-app tracking and the engine, all on one thread with its own message loop:
//! Windows calls a low-level hook on the thread that installed it, and removes the hook if
//! that thread is slow to answer. Nothing here may block.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;

use macaw_core::config::{ApplyTo, PrintedLayout};
use macaw_core::profile::{Fullscreen, classify, is_game};
use macaw_core::{Action, AppProfile, Config, Ctx, Engine, Input, Key, Out, Typematic};
use windows_sys::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
};
use windows_sys::Win32::System::Shutdown::LockWorkStation;
use windows_sys::Win32::System::Threading::{
    GetCurrentThread, GetCurrentThreadId, SetThreadPriority, THREAD_PRIORITY_HIGHEST,
};
use windows_sys::Win32::UI::Accessibility::{FILTERKEYS, HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, GetKeyboardLayout, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    KEYEVENTF_SCANCODE, KEYEVENTF_UNICODE, SendInput,
};
use windows_sys::Win32::UI::Input::{
    GetRawInputData, GetRawInputDeviceInfoW, GetRawInputDeviceList, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE,
    RAWINPUTDEVICELIST, RAWINPUTHEADER, RID_INPUT, RIDEV_DEVNOTIFY, RIDEV_INPUTSINK, RIDI_DEVICENAME, RIM_TYPEKEYBOARD,
    RegisterRawInputDevices,
};
use windows_sys::Win32::UI::Shell::{QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EVENT_SYSTEM_FOREGROUND,
    GWL_STYLE, GetForegroundWindow, GetMessageW, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, HC_ACTION,
    IsZoomed, KBDLLHOOKSTRUCT, LLKHF_EXTENDED, LLKHF_INJECTED, LLKHF_UP, MSG, PM_REMOVE, PeekMessageW, PostMessageW,
    PostQuitMessage, RegisterClassExW, SC_MINIMIZE, SPI_GETFILTERKEYS, SPI_GETKEYBOARDDELAY, SPI_GETKEYBOARDSPEED,
    SetTimer, SetWindowsHookExW, SystemParametersInfoW, UnhookWindowsHookEx, WH_KEYBOARD_LL, WINEVENT_OUTOFCONTEXT,
    WINEVENT_SKIPOWNPROCESS, WM_APP, WM_CLOSE, WM_INPUT, WM_INPUT_DEVICE_CHANGE, WM_POWERBROADCAST, WM_SETTINGCHANGE,
    WM_SYSCOMMAND, WM_TIMER, WM_WTSSESSION_CHANGE, WNDCLASSEXW, WS_CAPTION,
};

use crate::brightness::Brightness;
use crate::shared::{Command, Notice, Shared, WM_APP_COMMAND};
use crate::win::{self, now_ms, wide};
use crate::{layout, log_error, log_info, log_warn};

/// Marks input Macaw injects, so the hook lets it through untouched.
const MAGIC: usize = 0x4D41_4341;
/// An unassigned virtual key, tapped to keep Windows from treating a modifier press as a tap.
const VK_MASK: u16 = 0xE8;

const WM_APP_ACTION: u32 = WM_APP + 3;
const ACTION_LOCK: usize = 1;
const ACTION_MINIMIZE: usize = 2;

const TIMER_GUARD: usize = 1;
const TIMER_FOREGROUND: usize = 2;
const GUARD_EVERY_MS: u32 = 50;
const FOREGROUND_EVERY_MS: u32 = 1_500;

const RI_KEY_BREAK: u16 = 1;
const RI_KEY_E0: u16 = 2;
const GIDC_REMOVAL: usize = 2;
const WTS_SESSION_LOCK: usize = 7;
const WTS_SESSION_UNLOCK: usize = 8;
const PBT_APMRESUMESUSPEND: usize = 7;
const PBT_APMRESUMEAUTOMATIC: usize = 18;
const VK_CAPITAL: i32 = 0x14;
const FKF_FILTERKEYSON: u32 = 1;

/// Hardware IDs of Apple keyboards: USB (vendor 05AC) and Bluetooth (company 004C).
const APPLE_IDS: [&str; 3] = ["VID_05AC", "VID&0001004C", "VID&000205AC"];

pub fn is_apple_device(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    APPLE_IDS.iter().any(|id| name.contains(id))
}

#[derive(Default)]
struct Foreground {
    profile: AppProfile,
    game: bool,
    /// Elevated app in front while Macaw isn't: our injected input wouldn't reach it.
    blocked: bool,
}

struct State {
    shared: Arc<Shared>,
    hwnd: HWND,
    engine: RefCell<Engine>,
    config: RefCell<Config>,
    /// Raw Input device handle -> whether it is an Apple keyboard.
    devices: RefCell<HashMap<usize, bool>>,
    /// Whether the last key Raw Input reported came from an Apple keyboard. The hook runs
    /// before Windows reports the device of the current key, so this predicts it.
    last_was_apple: Cell<Option<bool>>,
    foreground: RefCell<Foreground>,
    brightness: Brightness,
    reported_layout: Cell<u32>,
}

thread_local! {
    static STATE: OnceCell<Rc<State>> = const { OnceCell::new() };
}

fn state() -> Option<Rc<State>> {
    STATE.with(|s| s.get().cloned())
}

pub fn spawn(shared: Arc<Shared>, config: Config) -> std::io::Result<JoinHandle<()>> {
    std::thread::Builder::new().name("macaw-input".into()).spawn(move || {
        if catch_unwind(AssertUnwindSafe(|| run(shared, config))).is_err() {
            log_error!("the input thread crashed");
        }
    })
}

fn run(shared: Arc<Shared>, config: Config) {
    // SAFETY: raising the priority of the current thread.
    unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST) };
    let Some(hwnd) = create_window() else {
        log_error!("could not create the input window");
        return;
    };
    let mut engine = Engine::new(config.clone());
    engine.set_typematic(read_typematic());
    let state = Rc::new(State {
        shared: shared.clone(),
        hwnd,
        engine: RefCell::new(engine),
        config: RefCell::new(config),
        devices: RefCell::new(HashMap::new()),
        last_was_apple: Cell::new(None),
        foreground: RefCell::new(Foreground::default()),
        brightness: Brightness::start(),
        reported_layout: Cell::new(0),
    });
    STATE.with(|s| {
        let _ = s.set(state.clone());
    });

    if !register_raw_input(hwnd) {
        log_warn!("Raw Input registration failed: Apple keyboards can't be told apart from others");
    }
    // SAFETY: the callbacks are plain functions; the window lives until the end of `run`.
    let (hook, win_event) = unsafe {
        let hook = SetWindowsHookExW(
            WH_KEYBOARD_LL,
            Some(keyboard_proc),
            GetModuleHandleW(std::ptr::null()),
            0,
        );
        let win_event = SetWinEventHook(
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_FOREGROUND,
            std::ptr::null_mut(),
            Some(foreground_changed),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);
        SetTimer(hwnd, TIMER_GUARD, GUARD_EVERY_MS, None);
        SetTimer(hwnd, TIMER_FOREGROUND, FOREGROUND_EVERY_MS, None);
        (hook, win_event)
    };
    if hook.is_null() {
        log_error!("could not install the keyboard hook");
    }
    state.refresh_devices();
    state.refresh_foreground(None);
    // SAFETY: plain call.
    shared.set_input_thread(unsafe { GetCurrentThreadId() });
    state.handle_commands();
    log_info!("input thread ready");

    // SAFETY: standard message loop on this thread's queue.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if msg.hwnd.is_null() && msg.message == WM_APP_COMMAND {
                state.handle_commands();
                continue;
            }
            DispatchMessageW(&msg);
        }
    }

    // Leave nothing held down.
    let out = state.engine.borrow_mut().reset();
    state.emit(&out);
    // SAFETY: undoing the registrations made above.
    unsafe {
        UnhookWindowsHookEx(hook);
        UnhookWinEvent(win_event);
        WTSUnRegisterSessionNotification(hwnd);
        DestroyWindow(hwnd);
    }
    log_info!("input thread stopped");
}

fn create_window() -> Option<HWND> {
    let class = wide("MacawInput");
    // SAFETY: the class name outlives registration and creation; the window stays hidden.
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassExW(&wc);
        // A hidden top-level window (not message-only): it must receive session, power and
        // settings broadcasts.
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

fn register_raw_input(hwnd: HWND) -> bool {
    let device = RAWINPUTDEVICE {
        usUsagePage: 0x01,
        usUsage: 0x06, // keyboards
        dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY,
        hwndTarget: hwnd,
    };
    // SAFETY: one valid RAWINPUTDEVICE.
    unsafe { RegisterRawInputDevices(&device, 1, size_of::<RAWINPUTDEVICE>() as u32) != 0 }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        // SAFETY: for HC_ACTION, lparam points to a KBDLLHOOKSTRUCT.
        let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        // A panic must never take the keyboard down with it: on failure, let the key through.
        if catch_unwind(AssertUnwindSafe(|| on_key(info))).unwrap_or(false) {
            return 1;
        }
    }
    // SAFETY: passing the event on to the next hook.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// Returns true to swallow the event.
fn on_key(info: &KBDLLHOOKSTRUCT) -> bool {
    if info.dwExtraInfo == MAGIC || info.flags & LLKHF_INJECTED != 0 || info.scanCode == 0 {
        return false;
    }
    let Some(state) = state() else { return false };
    state.drain_raw_input();
    let key = Key::from_scan(info.scanCode, info.flags & LLKHF_EXTENDED != 0);
    let down = info.flags & LLKHF_UP == 0;
    let ctx = state.ctx();
    let decision = match state.engine.try_borrow_mut() {
        Ok(mut engine) => engine.process(
            Input {
                key,
                down,
                time: now_ms(),
            },
            &ctx,
        ),
        Err(_) => return false,
    };
    if state.config.borrow().diagnostics.log_key_events {
        let out: Vec<String> = decision.out.iter().map(describe_out).collect();
        log_info!(
            "key {}{} remap={} bypass={} {:?} -> forward={} [{}]",
            if down { '+' } else { '-' },
            describe(key),
            ctx.remap,
            ctx.bypass,
            ctx.profile,
            decision.forward,
            out.join(" ")
        );
    }
    state.emit(&decision.out);
    !decision.forward
}

fn describe(key: Key) -> String {
    if key.is_letter() || key.is_digit() {
        "letter/digit".into()
    } else {
        format!("{key:?}")
    }
}

fn describe_out(out: &Out) -> String {
    match out {
        Out::Key { key, down } => format!("{}{}", if *down { '+' } else { '-' }, describe(*key)),
        Out::Text(_) => "text".into(),
        other => format!("{other:?}"),
    }
}

unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let handled = catch_unwind(AssertUnwindSafe(|| window_message(msg, wparam, lparam))).unwrap_or(false);
    // WM_INPUT always goes on to DefWindowProc, which frees the raw input data.
    if handled && msg != WM_INPUT {
        return 0;
    }
    // SAFETY: default processing.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn window_message(msg: u32, wparam: WPARAM, lparam: LPARAM) -> bool {
    let Some(state) = state() else { return false };
    match msg {
        WM_INPUT => state.raw_input(lparam as HRAWINPUT),
        WM_INPUT_DEVICE_CHANGE => {
            if wparam == GIDC_REMOVAL {
                state.devices.borrow_mut().remove(&(lparam as usize));
            }
            state.refresh_devices();
        }
        WM_TIMER if wparam == TIMER_GUARD => state.tick(),
        WM_TIMER if wparam == TIMER_FOREGROUND => state.refresh_foreground(None),
        WM_WTSSESSION_CHANGE => {
            if matches!(wparam, WTS_SESSION_LOCK | WTS_SESSION_UNLOCK) {
                state.reset("the session was locked or unlocked");
            }
        }
        WM_POWERBROADCAST => {
            if matches!(wparam, PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND) {
                state.reset("the computer woke up");
            }
            return false;
        }
        WM_SETTINGCHANGE => {
            state.engine.borrow_mut().set_typematic(read_typematic());
            return false;
        }
        WM_APP_ACTION => run_action(wparam),
        // The tray window handles closing; this window must stay alive until then.
        WM_CLOSE => {}
        _ => return false,
    }
    true
}

unsafe extern "system" fn foreground_changed(
    _hook: HWINEVENTHOOK,
    _event: u32,
    hwnd: HWND,
    _object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if let Some(state) = state() {
            state.refresh_foreground(Some(hwnd));
        }
    }));
}

fn run_action(action: usize) {
    // SAFETY: plain calls; the foreground window may vanish, which PostMessage tolerates.
    unsafe {
        match action {
            ACTION_LOCK => {
                LockWorkStation();
            }
            ACTION_MINIMIZE => {
                let window = GetForegroundWindow();
                if !window.is_null() {
                    PostMessageW(window, WM_SYSCOMMAND, SC_MINIMIZE as usize, 0);
                }
            }
            _ => {}
        }
    }
}

impl State {
    fn ctx(&self) -> Ctx {
        let config = self.config.borrow();
        let foreground = self.foreground.borrow();
        let remap = match config.apply_to {
            ApplyTo::All => true,
            ApplyTo::Apple => self
                .last_was_apple
                .get()
                .unwrap_or_else(|| self.shared.apple_connected.load(Ordering::Relaxed)),
        };
        // SAFETY: plain call.
        let caps_lock_on = unsafe { GetKeyState(VK_CAPITAL) } & 1 != 0;
        Ctx {
            remap,
            bypass: foreground.game || foreground.blocked,
            profile: foreground.profile,
            caps_lock_on,
        }
    }

    /// Handles Raw Input that is already queued, so the device of the previous key is known.
    fn drain_raw_input(&self) {
        // SAFETY: standard peek loop on our own window; DefWindowProc frees each message's data.
        unsafe {
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, self.hwnd, WM_INPUT, WM_INPUT, PM_REMOVE) != 0 {
                self.raw_input(msg.lParam as HRAWINPUT);
                DefWindowProcW(self.hwnd, msg.message, msg.wParam, msg.lParam);
            }
        }
    }

    fn raw_input(&self, handle: HRAWINPUT) {
        // SAFETY: keyboard Raw Input fits in RAWINPUT; the union is read as keyboard data
        // only after checking the type.
        let raw = unsafe {
            let mut raw: RAWINPUT = std::mem::zeroed();
            let mut size = size_of::<RAWINPUT>() as u32;
            let read = GetRawInputData(
                handle,
                RID_INPUT,
                &mut raw as *mut _ as *mut c_void,
                &mut size,
                size_of::<RAWINPUTHEADER>() as u32,
            );
            if read == u32::MAX || raw.header.dwType != RIM_TYPEKEYBOARD || raw.header.hDevice.is_null() {
                return;
            }
            raw
        };
        let apple = self.is_apple(raw.header.hDevice);
        self.last_was_apple.set(Some(apple));
        // SAFETY: dwType is RIM_TYPEKEYBOARD.
        let keyboard = unsafe { raw.data.keyboard };
        let down = keyboard.Flags & RI_KEY_BREAK == 0;
        if down && !apple && self.config.borrow().apply_to == ApplyTo::Apple {
            let key = Key::from_scan(keyboard.MakeCode as u32, keyboard.Flags & RI_KEY_E0 != 0);
            if let Ok(mut engine) = self.engine.try_borrow_mut() {
                engine.note_other_device(key);
            }
        }
    }

    fn is_apple(&self, device: HANDLE) -> bool {
        if let Some(&apple) = self.devices.borrow().get(&(device as usize)) {
            return apple;
        }
        let apple = device_name(device).is_some_and(|name| is_apple_device(&name));
        self.devices.borrow_mut().insert(device as usize, apple);
        apple
    }

    fn refresh_devices(&self) {
        let present = keyboard_devices().into_iter().any(|device| self.is_apple(device));
        let before = self.shared.apple_connected.swap(present, Ordering::Relaxed);
        if before != present {
            log_info!("Apple keyboard {}", if present { "connected" } else { "disconnected" });
            self.shared.notify(if present {
                Notice::KeyboardConnected
            } else {
                Notice::KeyboardDisconnected
            });
        }
    }

    fn refresh_foreground(&self, hwnd: Option<HWND>) {
        // SAFETY: plain queries about a window that may already be gone (then they fail).
        let (hwnd, pid, thread) = unsafe {
            let hwnd = hwnd.filter(|h| !h.is_null()).unwrap_or_else(|| GetForegroundWindow());
            if hwnd.is_null() {
                return;
            }
            let mut pid = 0u32;
            let thread = GetWindowThreadProcessId(hwnd, &mut pid);
            (hwnd, pid, thread)
        };
        let path = win::process_image_path(pid).unwrap_or_default();
        let exe = win::file_name(&path);
        let class = win::window_class(hwnd);
        let (profile, game) = {
            let c = self.config.borrow();
            let profile = classify(&exe, &class, &c.mac_mode.terminal_apps, &c.mac_mode.browser_apps);
            let game = c.game_mode.enabled
                && is_game(
                    &path,
                    profile,
                    fullscreen(hwnd),
                    &c.game_mode.always_off_in,
                    &c.game_mode.never_off_in,
                );
            (profile, game)
        };
        let blocked = !self.shared.elevated && pid != 0 && win::process_is_elevated(pid);
        let changed = {
            let mut fg = self.foreground.borrow_mut();
            let changed = fg.game != game || fg.blocked != blocked;
            *fg = Foreground { profile, game, blocked };
            changed
        };
        if changed {
            self.shared.game_in_front.store(game, Ordering::Relaxed);
            self.shared.admin_app_in_front.store(blocked, Ordering::Relaxed);
            let name = if exe.is_empty() { "an app" } else { exe.as_str() };
            if game {
                log_info!("stepping aside for {name} (game)");
            } else if blocked {
                log_info!("{name} runs as administrator; Macaw can't change keys there without admin rights");
            }
            self.shared.notify(Notice::State);
        }
        // SAFETY: plain call.
        let hkl = unsafe { GetKeyboardLayout(thread) } as usize;
        self.check_layout(hkl);
    }

    fn check_layout(&self, hkl: usize) {
        if hkl == 0 {
            return;
        }
        let wants_us = self.config.borrow().keyboard_layout == PrintedLayout::Us;
        let mismatch = if wants_us && self.shared.apple_connected.load(Ordering::Relaxed) && !layout::us_installed() {
            layout::mismatch(hkl)
        } else {
            None
        };
        let layout_id = mismatch.unwrap_or(0);
        self.shared
            .layout_language
            .store(layout::language(hkl), Ordering::Relaxed);
        self.shared.layout_mismatch.store(layout_id, Ordering::Relaxed);
        if layout_id != 0 && self.reported_layout.get() != layout_id {
            self.reported_layout.set(layout_id);
            log_info!(
                "Windows types with the {} layout, but the keyboard is printed U.S.",
                layout::name(layout_id)
            );
            self.shared.notify(Notice::LayoutMismatch);
        }
    }

    fn tick(&self) {
        let Ok(mut engine) = self.engine.try_borrow_mut() else {
            return;
        };
        let out = engine.tick(now_ms());
        drop(engine);
        if !out.is_empty() {
            let released = out.iter().filter(|o| matches!(o, Out::Key { down: false, .. })).count();
            log_info!("released {released} key(s) that Windows thought were still held");
            self.emit(&out);
        }
    }

    fn reset(&self, why: &str) {
        let out = self.engine.borrow_mut().reset();
        if !out.is_empty() {
            log_info!("released held keys because {why}");
        }
        self.emit(&out);
    }

    fn handle_commands(&self) {
        for command in self.shared.take_commands() {
            match command {
                Command::Config(config) => {
                    let out = self.engine.borrow_mut().set_config((*config).clone());
                    self.emit(&out);
                    *self.config.borrow_mut() = *config;
                    self.devices.borrow_mut().clear();
                    self.refresh_devices();
                    self.refresh_foreground(None);
                }
                Command::Pause(paused) => {
                    let out = self.engine.borrow_mut().set_paused(paused);
                    self.emit(&out);
                    self.set_paused(paused);
                }
                // SAFETY: ends this thread's message loop.
                Command::Quit => unsafe { PostQuitMessage(0) },
            }
        }
    }

    fn set_paused(&self, paused: bool) {
        self.shared.paused.store(paused, Ordering::Relaxed);
        log_info!("{}", if paused { "paused" } else { "resumed" });
        self.shared.notify(Notice::State);
    }

    /// Carries out the engine's output. Key input is batched into one SendInput call, so
    /// nothing else can slip in between; actions run after the hook returns.
    fn emit(&self, outs: &[Out]) {
        let mut inputs: Vec<INPUT> = Vec::with_capacity(outs.len() * 2);
        for out in outs {
            match *out {
                Out::Key { key, down } => inputs.push(key_input(key, down)),
                Out::Mask => {
                    inputs.push(keyboard_input(VK_MASK, 0, 0));
                    inputs.push(keyboard_input(VK_MASK, 0, KEYEVENTF_KEYUP));
                }
                Out::Text(c) => {
                    let mut buf = [0u16; 2];
                    for &unit in c.encode_utf16(&mut buf).iter() {
                        inputs.push(keyboard_input(0, unit, KEYEVENTF_UNICODE));
                        inputs.push(keyboard_input(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
                    }
                }
                Out::Action(action) => {
                    send(&mut inputs);
                    self.action(action);
                }
                Out::Paused(paused) => self.set_paused(paused),
            }
        }
        send(&mut inputs);
    }

    fn action(&self, action: Action) {
        let step = i32::from(self.config.borrow().brightness.step_percent);
        match action {
            Action::BrightnessUp => self.brightness.change(step),
            Action::BrightnessDown => self.brightness.change(-step),
            // SAFETY: posting to our own window.
            Action::Lock => unsafe {
                PostMessageW(self.hwnd, WM_APP_ACTION, ACTION_LOCK, 0);
            },
            // SAFETY: posting to our own window.
            Action::Minimize => unsafe {
                PostMessageW(self.hwnd, WM_APP_ACTION, ACTION_MINIMIZE, 0);
            },
        }
    }
}

fn send(inputs: &mut Vec<INPUT>) {
    if inputs.is_empty() {
        return;
    }
    // SAFETY: a valid array of INPUT structures.
    let sent = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        log_warn!("Windows accepted {sent} of {} injected key events", inputs.len());
    }
    inputs.clear();
}

fn keyboard_input(vk: u16, scan: u16, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: MAGIC,
            },
        },
    }
}

/// Media keys are injected by virtual key, everything else by scan code (layout independent,
/// and what games and apps that read scan codes expect).
fn key_input(key: Key, down: bool) -> INPUT {
    let extended = if key.is_extended() { KEYEVENTF_EXTENDEDKEY } else { 0 };
    let up = if down { 0 } else { KEYEVENTF_KEYUP };
    let vk = match key {
        Key::MEDIA_NEXT => 0xB0,
        Key::MEDIA_PREV => 0xB1,
        Key::MEDIA_STOP => 0xB2,
        Key::MEDIA_PLAY_PAUSE => 0xB3,
        Key::VOLUME_MUTE => 0xAD,
        Key::VOLUME_DOWN => 0xAE,
        Key::VOLUME_UP => 0xAF,
        Key::PRINT_SCREEN => 0x2C,
        _ => return keyboard_input(0, key.scan_code(), KEYEVENTF_SCANCODE | extended | up),
    };
    keyboard_input(vk, key.scan_code(), extended | up)
}

fn keyboard_devices() -> Vec<HANDLE> {
    // SAFETY: the list is sized by the count Windows reports.
    unsafe {
        let item = size_of::<RAWINPUTDEVICELIST>() as u32;
        let mut count = 0u32;
        GetRawInputDeviceList(std::ptr::null_mut(), &mut count, item);
        let mut list: Vec<RAWINPUTDEVICELIST> = vec![std::mem::zeroed(); count as usize];
        let got = GetRawInputDeviceList(list.as_mut_ptr(), &mut count, item);
        if got == u32::MAX {
            return Vec::new();
        }
        list.truncate(got as usize);
        list.into_iter()
            .filter(|d| d.dwType == RIM_TYPEKEYBOARD)
            .map(|d| d.hDevice)
            .collect()
    }
}

fn device_name(device: HANDLE) -> Option<String> {
    // SAFETY: the buffer is sized (in characters) by the length Windows reports.
    unsafe {
        let mut len = 0u32;
        GetRawInputDeviceInfoW(device, RIDI_DEVICENAME, std::ptr::null_mut(), &mut len);
        if len == 0 {
            return None;
        }
        let mut buf = vec![0u16; len as usize];
        let got = GetRawInputDeviceInfoW(device, RIDI_DEVICENAME, buf.as_mut_ptr() as *mut c_void, &mut len);
        if got == u32::MAX {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]))
    }
}

/// Exclusive full-screen (D3D), a borderless window covering its whole monitor, or neither.
fn fullscreen(hwnd: HWND) -> Fullscreen {
    // SAFETY: plain queries with properly sized out-structures.
    unsafe {
        let mut state = 0;
        if SHQueryUserNotificationState(&mut state) == 0 && state == QUNS_RUNNING_D3D_FULL_SCREEN {
            return Fullscreen::Exclusive;
        }
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
        if style & WS_CAPTION == WS_CAPTION || IsZoomed(hwnd) != 0 {
            return Fullscreen::No;
        }
        let mut rect: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut rect) == 0 {
            return Fullscreen::No;
        }
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = size_of::<MONITORINFO>() as u32;
        if GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info) == 0 {
            return Fullscreen::No;
        }
        let m = info.rcMonitor;
        if rect.left <= m.left && rect.top <= m.top && rect.right >= m.right && rect.bottom >= m.bottom {
            Fullscreen::Borderless
        } else {
            Fullscreen::No
        }
    }
}

/// Windows' key auto-repeat settings, which the stuck-key guard depends on.
fn read_typematic() -> Typematic {
    // SAFETY: each call writes one properly sized value.
    unsafe {
        let mut delay = 1u32;
        let mut speed = 31u32;
        SystemParametersInfoW(SPI_GETKEYBOARDDELAY, 0, &mut delay as *mut u32 as *mut c_void, 0);
        SystemParametersInfoW(SPI_GETKEYBOARDSPEED, 0, &mut speed as *mut u32 as *mut c_void, 0);
        let mut filter: FILTERKEYS = std::mem::zeroed();
        filter.cbSize = size_of::<FILTERKEYS>() as u32;
        SystemParametersInfoW(
            SPI_GETFILTERKEYS,
            filter.cbSize,
            &mut filter as *mut _ as *mut c_void,
            0,
        );
        // Delay 0..3 means 250..1000 ms; speed 0..31 means about 2.5..30 repeats per second.
        let rate = 2.5 + f64::from(speed.min(31)) * 27.5 / 31.0;
        let mut typematic = Typematic {
            delay_ms: (u64::from(delay.min(3)) + 1) * 250,
            interval_ms: (1000.0 / rate).round() as u64,
            repeats: true,
        };
        if filter.dwFlags & FKF_FILTERKEYSON != 0 {
            if filter.iRepeatMSec == 0 {
                typematic.repeats = false;
            } else {
                typematic.delay_ms = u64::from(filter.iDelayMSec);
                typematic.interval_ms = u64::from(filter.iRepeatMSec);
            }
        }
        typematic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_apple_keyboards() {
        assert!(is_apple_device(
            r"\\?\HID#VID_05AC&PID_0321&MI_01&Col01#9&330cc40b&0&0000#{884b96c3-56ef-11d1-bc8c-00a0c91405dd}"
        ));
        assert!(is_apple_device(
            r"\\?\HID#{00001124-0000-1000-8000-00805f9b34fb}_VID&0001004c_PID&0267&Col01#8&1"
        ));
        assert!(!is_apple_device(
            r"\\?\HID#VID_1038&PID_1614&MI_00&Col02#8&72d1027&0&0001#{884b96c3}"
        ));
        assert!(!is_apple_device(
            r"\\?\HID#VID_046D&PID_C53A&MI_00#9&2726929c&0&0000#{884b96c3}"
        ));
    }
}
