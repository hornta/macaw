//! The keyboard layout Windows types with, compared with what is printed on the keyboard.

use windows_sys::Win32::Foundation::{FreeLibrary, HWND};
use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardLayoutList, HKL};
use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_INPUTLANGCHANGEREQUEST};

use crate::win::wide;

const US: u32 = 0x0409;

/// The layout id of a keyboard layout handle, if it clearly isn't U.S. English. Layout variants
/// (ids 0xFxxx, such as U.S. International) are not reported: we can't tell what they are.
pub fn mismatch(hkl: usize) -> Option<u32> {
    let layout = ((hkl >> 16) & 0xFFFF) as u32;
    (layout != 0 && layout != US && layout & 0xF000 != 0xF000).then_some(layout)
}

/// The language half of a keyboard layout handle.
pub fn language(hkl: usize) -> u32 {
    (hkl & 0xFFFF) as u32
}

pub fn name(layout: u32) -> String {
    let known = match layout {
        0x041D => "Swedish",
        0x0406 => "Danish",
        0x0414 => "Norwegian",
        0x040B => "Finnish",
        0x0407 => "German",
        0x0807 => "Swiss German",
        0x040C => "French",
        0x080C => "Belgian French",
        0x0410 => "Italian",
        0x040A | 0x0C0A => "Spanish",
        0x0816 => "Portuguese",
        0x0413 => "Dutch",
        0x0415 => "Polish",
        0x0809 => "UK English",
        0x1009 => "Canadian French",
        0x0411 => "Japanese",
        _ => return format!("layout {layout:04X}"),
    };
    known.to_string()
}

type LayoutFn = unsafe extern "system" fn(*const u16, u32) -> i32;

/// Adds the U.S. keyboard layout to `language` and makes it the default input method.
/// Uses the documented input.dll functions behind the Settings app (no import library exists).
pub fn use_us_layout(language: u32) -> Result<(), String> {
    let profile = wide(&format!("0x{language:04X}:0x{US:08X}"));
    let dll_name = wide("input.dll");
    // SAFETY: input.dll is loaded from System32 only; both functions take a NUL-terminated
    // profile string and flags, and the library is freed after use.
    unsafe {
        let dll = LoadLibraryExW(dll_name.as_ptr(), std::ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32);
        if dll.is_null() {
            return Err("input.dll could not be loaded".into());
        }
        let install = GetProcAddress(dll, c"InstallLayoutOrTip".as_ptr() as *const u8);
        let set_default = GetProcAddress(dll, c"SetDefaultLayoutOrTip".as_ptr() as *const u8);
        let result = match (install, set_default) {
            (Some(install), Some(set_default)) => {
                let install: LayoutFn = std::mem::transmute(install);
                let set_default: LayoutFn = std::mem::transmute(set_default);
                if install(profile.as_ptr(), 0) == 0 {
                    Err("Windows refused to add the U.S. layout".into())
                } else if set_default(profile.as_ptr(), 0) == 0 {
                    Err("the U.S. layout was added but could not be made the default".into())
                } else {
                    Ok(())
                }
            }
            _ => Err("input.dll lacks the layout functions".into()),
        };
        FreeLibrary(dll);
        if result.is_ok() {
            switch_open_windows(language);
        }
        result
    }
}

fn installed_layouts() -> Vec<usize> {
    // SAFETY: the buffer holds as many handles as we say.
    unsafe {
        let mut layouts: [HKL; 64] = [std::ptr::null_mut(); 64];
        let count = GetKeyboardLayoutList(layouts.len() as i32, layouts.as_mut_ptr()).max(0) as usize;
        layouts[..count.min(layouts.len())]
            .iter()
            .map(|&hkl| hkl as usize)
            .collect()
    }
}

/// Whether a U.S. layout is installed. Then another layout in use is a deliberate choice
/// (switching between layouts), not a mistake worth a warning.
pub fn us_installed() -> bool {
    installed_layouts()
        .into_iter()
        .any(|hkl| ((hkl >> 16) & 0xFFFF) as u32 == US)
}

/// Asks every open window to switch to the U.S. layout for `language` (new windows get it as
/// the default). Best effort: some apps ignore the request, and then Win+Space switches.
fn switch_open_windows(lang: u32) {
    const HWND_BROADCAST: HWND = 0xFFFF as HWND;
    let us = installed_layouts()
        .into_iter()
        .find(|&hkl| language(hkl) == lang && ((hkl >> 16) & 0xFFFF) as u32 == US);
    if let Some(hkl) = us {
        // SAFETY: posting carries no pointers.
        unsafe { PostMessageW(HWND_BROADCAST, WM_INPUTLANGCHANGEREQUEST, 0, hkl as isize) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_mismatches() {
        assert_eq!(mismatch(0x041D_0809), Some(0x041D)); // Swedish keyboard, UK English language
        assert_eq!(mismatch(0x0409_0809), None); // U.S. keyboard
        assert_eq!(mismatch(0xF001_0409), None); // a variant we can't identify
        assert_eq!(language(0x041D_0809), 0x0809);
        assert_eq!(name(0x041D), "Swedish");
    }
}
