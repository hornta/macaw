//! What the input thread and the tray (main) thread share, and how they talk:
//! the tray sends [`Command`]s to the input thread; the input thread posts [`Notice`]s to the tray.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use macaw_core::Config;
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, PostThreadMessageW, WM_APP};

/// Thread message to the input thread: commands are waiting.
pub const WM_APP_COMMAND: u32 = WM_APP + 1;
/// Window message to the tray: something changed (wparam is a [`Notice`]).
pub const WM_APP_NOTICE: u32 = WM_APP + 2;

pub enum Command {
    Config(Box<Config>),
    Pause(bool),
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum Notice {
    State = 1,
    LayoutMismatch = 2,
    KeyboardConnected = 3,
    KeyboardDisconnected = 4,
}

impl Notice {
    const ALL: [Notice; 4] = [
        Notice::State,
        Notice::LayoutMismatch,
        Notice::KeyboardConnected,
        Notice::KeyboardDisconnected,
    ];

    pub fn from_raw(raw: usize) -> Option<Notice> {
        Notice::ALL.into_iter().find(|n| *n as usize == raw)
    }
}

pub struct Shared {
    pub elevated: bool,
    pub paused: AtomicBool,
    pub apple_connected: AtomicBool,
    pub game_in_front: AtomicBool,
    /// An elevated app is in front and Macaw isn't elevated, so it can't change keys there.
    pub admin_app_in_front: AtomicBool,
    /// Windows layout id (like 0x041D, Swedish) that doesn't match the keyboard; 0 if none.
    pub layout_mismatch: AtomicU32,
    /// Language of the layout in use, for adding the matching layout to it.
    pub layout_language: AtomicU32,
    tray_hwnd: AtomicUsize,
    /// Notices sent before the tray window existed (one bit per notice).
    pending: AtomicUsize,
    input_thread: AtomicU32,
    commands: Mutex<Vec<Command>>,
}

impl Shared {
    pub fn new(elevated: bool) -> Shared {
        Shared {
            elevated,
            paused: AtomicBool::new(false),
            apple_connected: AtomicBool::new(false),
            game_in_front: AtomicBool::new(false),
            admin_app_in_front: AtomicBool::new(false),
            layout_mismatch: AtomicU32::new(0),
            layout_language: AtomicU32::new(0),
            tray_hwnd: AtomicUsize::new(0),
            pending: AtomicUsize::new(0),
            input_thread: AtomicU32::new(0),
            commands: Mutex::new(Vec::new()),
        }
    }

    /// Makes the tray window the target of notices, and re-sends any that arrived earlier.
    pub fn set_tray_window(&self, hwnd: HWND) {
        self.tray_hwnd.store(hwnd as usize, Ordering::Release);
        let pending = self.pending.swap(0, Ordering::AcqRel);
        for notice in Notice::ALL {
            if pending & (1 << notice as usize) != 0 {
                self.notify(notice);
            }
        }
    }

    /// Called by the input thread once its message queue exists.
    pub fn set_input_thread(&self, id: u32) {
        self.input_thread.store(id, Ordering::Release);
    }

    /// Queues a command for the input thread and wakes it. Commands sent before it is ready
    /// wait in the queue; it drains them on start.
    pub fn send(&self, command: Command) {
        self.commands.lock().unwrap_or_else(|e| e.into_inner()).push(command);
        let thread = self.input_thread.load(Ordering::Acquire);
        if thread != 0 {
            // SAFETY: posting a message with no pointers in it.
            unsafe { PostThreadMessageW(thread, WM_APP_COMMAND, 0, 0) };
        }
    }

    pub fn take_commands(&self) -> Vec<Command> {
        std::mem::take(&mut *self.commands.lock().unwrap_or_else(|e| e.into_inner()))
    }

    pub fn notify(&self, notice: Notice) {
        let hwnd = self.tray_hwnd.load(Ordering::Acquire) as HWND;
        if hwnd.is_null() {
            self.pending.fetch_or(1 << notice as usize, Ordering::AcqRel);
        } else {
            // SAFETY: posting a message with no pointers in it.
            unsafe { PostMessageW(hwnd, WM_APP_NOTICE, notice as usize, 0) };
        }
    }
}
