//! Macaw: makes Apple Magic Keyboards work the Mac way on Windows.
//!
//! Two threads: the input thread (keyboard hook and the engine, see `hook`) and the main
//! thread (tray icon and settings, see `tray`).

#![windows_subsystem = "windows"]

mod autostart;
mod brightness;
mod hook;
mod layout;
mod logging;
mod paths;
mod shared;
mod tray;
mod win;

use std::sync::Arc;
use std::time::{Duration, Instant};

use macaw_core::Config;
use macaw_core::config::TEMPLATE;
use windows_sys::Win32::System::Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx};

use crate::shared::{Command, Shared};

const INSTANCE_MUTEX: &str = "Local\\Macaw.Instance";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    logging::init(&paths::log_file());
    if let Some(code) = autostart::handle_cli(&args) {
        std::process::exit(code);
    }
    let elevated = win::is_elevated();
    let from_task = args.iter().any(|a| a == "--from-task");
    log_info!(
        "Macaw {} starting (admin rights: {elevated})",
        env!("CARGO_PKG_VERSION")
    );

    // Started normally but set up to run with admin rights: let the task start that instance.
    if !elevated && !from_task && autostart::exists() && autostart::start() {
        log_info!("handed over to the start-at-sign-in task");
        return;
    }
    let Some(_instance) = acquire_instance(from_task) else {
        log_info!("Macaw is already running");
        return;
    };
    win::setup_process();
    // SAFETY: initialising COM once for this (main) thread, as ShellExecuteEx expects.
    unsafe {
        CoInitializeEx(
            std::ptr::null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
        )
    };

    let (config, first_run, config_error) = load_config();
    let shared = Arc::new(Shared::new(elevated));
    let input = match hook::spawn(shared.clone(), config.clone()) {
        Ok(handle) => handle,
        Err(e) => {
            log_error!("could not start the input thread: {e}");
            return;
        }
    };
    tray::run(shared.clone(), config, first_run, config_error);
    shared.send(Command::Quit);
    let _ = input.join();
    log_info!("Macaw stopped");
}

/// The start-at-sign-in task may start us while the instance that set it up is still quitting,
/// so a task-started instance waits a little for the other one to go.
fn acquire_instance(wait: bool) -> Option<win::SingleInstance> {
    let deadline = Instant::now() + Duration::from_secs(if wait { 5 } else { 0 });
    loop {
        if let Some(instance) = win::single_instance(INSTANCE_MUTEX) {
            return Some(instance);
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Reads the settings file, creating it from the commented template on first run.
/// Returns the settings, whether this is the first run, and a settings-file error if any.
fn load_config() -> (Config, bool, Option<String>) {
    let path = paths::config_file();
    match std::fs::read_to_string(&path) {
        Ok(text) => match Config::parse(&text) {
            Ok(config) => (config, false, None),
            Err(error) => {
                log_warn!("settings file error, using defaults: {error}");
                (Config::default(), false, Some(error))
            }
        },
        Err(_) => {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            match std::fs::write(&path, TEMPLATE) {
                Ok(()) => log_info!("created {}", path.display()),
                Err(e) => log_warn!("could not create {}: {e}", path.display()),
            }
            (Config::default(), true, None)
        }
    }
}
