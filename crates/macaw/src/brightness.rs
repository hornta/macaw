//! External monitor brightness over DDC/CI. It runs on its own thread: monitors take tens of
//! milliseconds to answer, and the keyboard hook must never wait.

use std::collections::HashSet;
use std::sync::mpsc::{Receiver, Sender, channel};

use windows_sys::Win32::Devices::Display::{
    DestroyPhysicalMonitors, GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR,
    GetPhysicalMonitorsFromHMONITOR, PHYSICAL_MONITOR, SetMonitorBrightness,
};
use windows_sys::Win32::Foundation::{LPARAM, RECT};
use windows_sys::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
use windows_sys::core::BOOL;

use crate::{log_info, log_warn};

pub struct Brightness {
    tx: Sender<i32>,
}

impl Brightness {
    pub fn start() -> Brightness {
        let (tx, rx) = channel();
        let spawned = std::thread::Builder::new()
            .name("macaw-brightness".into())
            .spawn(move || run(rx));
        if spawned.is_err() {
            log_warn!("could not start the brightness thread");
        }
        Brightness { tx }
    }

    /// Changes the brightness of every monitor by `percent` (negative is darker).
    pub fn change(&self, percent: i32) {
        let _ = self.tx.send(percent);
    }
}

fn run(rx: Receiver<i32>) {
    let mut unsupported = HashSet::new();
    while let Ok(first) = rx.recv() {
        // Coalesce key repeats that arrived while the monitors were busy.
        let delta = first + rx.try_iter().sum::<i32>();
        for monitor in monitors() {
            adjust(monitor, delta, &mut unsupported);
        }
    }
}

unsafe extern "system" fn collect(monitor: HMONITOR, _dc: HDC, _rect: *mut RECT, data: LPARAM) -> BOOL {
    // SAFETY: `data` is the Vec passed by `monitors` below, alive for the whole enumeration.
    unsafe { (*(data as *mut Vec<HMONITOR>)).push(monitor) };
    1
}

fn monitors() -> Vec<HMONITOR> {
    let mut list: Vec<HMONITOR> = Vec::new();
    // SAFETY: the callback only pushes into `list`, which outlives the call.
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            Some(collect),
            &mut list as *mut _ as LPARAM,
        )
    };
    list
}

fn adjust(monitor: HMONITOR, delta: i32, unsupported: &mut HashSet<String>) {
    // SAFETY: arrays sized by the count Windows reports; handles are destroyed at the end.
    unsafe {
        let mut count = 0u32;
        if GetNumberOfPhysicalMonitorsFromHMONITOR(monitor, &mut count) == 0 || count == 0 {
            return;
        }
        let mut physical: Vec<PHYSICAL_MONITOR> = vec![std::mem::zeroed(); count as usize];
        if GetPhysicalMonitorsFromHMONITOR(monitor, count, physical.as_mut_ptr()) == 0 {
            return;
        }
        for pm in &physical {
            // PHYSICAL_MONITOR is packed: copy fields out instead of borrowing them.
            let (handle, description) = (pm.hPhysicalMonitor, pm.szPhysicalMonitorDescription);
            let len = description.iter().position(|&c| c == 0).unwrap_or(description.len());
            let name = String::from_utf16_lossy(&description[..len]);
            let (mut min, mut current, mut max) = (0u32, 0u32, 0u32);
            if GetMonitorBrightness(handle, &mut min, &mut current, &mut max) == 0 || max <= min {
                if unsupported.insert(name.clone()) {
                    log_warn!("{name}: brightness can't be controlled (DDC/CI off or unsupported)");
                }
                continue;
            }
            let range = (max - min) as i32;
            let step = (range * delta.abs() / 100).max(1) * delta.signum();
            let new = (current as i32 + step).clamp(min as i32, max as i32) as u32;
            if new != current && SetMonitorBrightness(handle, new) != 0 {
                log_info!("{name}: brightness {current} -> {new}");
            }
        }
        DestroyPhysicalMonitors(count, physical.as_ptr());
    }
}
