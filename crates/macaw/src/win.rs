//! Small safe wrappers around the Win32 calls used in several places.

use std::ffi::c_void;
use std::path::Path;

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND};
use windows_sys::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows_sys::Win32::System::SystemInformation::GetTickCount64;
use windows_sys::Win32::System::Threading::{
    ABOVE_NORMAL_PRIORITY_CLASS, CreateMutexW, GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32,
    PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
    PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION, PROCESS_POWER_THROTTLING_STATE,
    PROCESS_QUERY_LIMITED_INFORMATION, ProcessPowerThrottling, QueryFullProcessImageNameW, SetPriorityClass,
    SetProcessInformation,
};
use windows_sys::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{GetClassNameW, SW_SHOWNORMAL};

/// A NUL-terminated UTF-16 copy of `s` for Win32.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Copies `s` into a fixed-size UTF-16 buffer, truncating and NUL-terminating it.
pub fn copy_wide(dst: &mut [u16], s: &str) {
    let mut n = 0;
    let limit = dst.len().saturating_sub(1);
    for (slot, unit) in dst.iter_mut().zip(s.encode_utf16()).take(limit) {
        *slot = unit;
        n += 1;
    }
    if let Some(end) = dst.get_mut(n) {
        *end = 0;
    }
}

pub fn now_ms() -> u64 {
    // SAFETY: no arguments, no side effects.
    unsafe { GetTickCount64() }
}

/// Closes a Win32 handle when dropped.
pub struct Handle(pub HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: we own the handle.
            unsafe { CloseHandle(self.0) };
        }
    }
}

fn token_is_elevated(process: HANDLE) -> Option<bool> {
    // SAFETY: plain out-parameters; the token handle is closed by `Handle`.
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
            return None;
        }
        let token = Handle(token);
        let mut elevation: TOKEN_ELEVATION = std::mem::zeroed();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token.0,
            TokenElevation,
            &mut elevation as *mut _ as *mut c_void,
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        (ok != 0).then_some(elevation.TokenIsElevated != 0)
    }
}

/// Whether this process runs with administrator rights.
pub fn is_elevated() -> bool {
    // SAFETY: GetCurrentProcess returns a pseudo handle that needs no closing.
    token_is_elevated(unsafe { GetCurrentProcess() }).unwrap_or(false)
}

/// Whether another process runs elevated. If we may not even look, it is (or is protected),
/// which for our purposes is the same: our injected input would not reach it.
pub fn process_is_elevated(pid: u32) -> bool {
    // SAFETY: the process handle is closed by `Handle`.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return true;
        }
        let process = Handle(process);
        token_is_elevated(process.0).unwrap_or(true)
    }
}

/// The full path of a process's program, e.g. `C:\Program Files\Google\Chrome\Application\chrome.exe`.
pub fn process_image_path(pid: u32) -> Option<String> {
    // SAFETY: bounded buffer; the handle is closed by `Handle`.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return None;
        }
        let process = Handle(process);
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        if QueryFullProcessImageNameW(process.0, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len) == 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// The file name part of a path, e.g. `chrome.exe`.
pub fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

pub fn window_class(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    // SAFETY: bounded buffer.
    let len = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    String::from_utf16_lossy(&buf[..len.max(0) as usize])
}

/// Opens a file, folder or URL with its default handler. Returns false if Windows couldn't.
pub fn shell_open(target: &Path) -> bool {
    let verb = wide("open");
    let file = wide(&target.to_string_lossy());
    // SAFETY: NUL-terminated strings that outlive the call.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    result as isize > 32
}

/// Process-wide settings: sharp tray icons on high-DPI screens, and no CPU throttling,
/// because every key press waits for our hook.
pub fn setup_process() {
    // SAFETY: plain calls on the current process with valid arguments.
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        SetPriorityClass(GetCurrentProcess(), ABOVE_NORMAL_PRIORITY_CLASS);
        let state = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
            StateMask: 0,
        };
        SetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            &state as *const _ as *const c_void,
            size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        );
    }
}

/// Holds a named mutex for the lifetime of the process, so only one Macaw runs per session.
pub struct SingleInstance(#[allow(dead_code)] Handle);

pub fn single_instance(name: &str) -> Option<SingleInstance> {
    let name = wide(name);
    // SAFETY: NUL-terminated name. An elevated instance's mutex may deny us access, which
    // (like ERROR_ALREADY_EXISTS) means Macaw is already running.
    unsafe {
        let mutex = CreateMutexW(std::ptr::null(), 0, name.as_ptr());
        if mutex.is_null() {
            return None;
        }
        let mutex = Handle(mutex);
        (GetLastError() != ERROR_ALREADY_EXISTS).then_some(SingleInstance(mutex))
    }
}
