//! Starting Macaw at sign-in with administrator rights, through a scheduled task. Windows lets
//! users start their own "highest privileges" tasks without a UAC prompt, so only installing or
//! removing the task asks for permission. Admin rights let Macaw work in admin apps too.

use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, GetExitCodeProcess, WaitForSingleObject};
use windows_sys::Win32::UI::Shell::{SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

use crate::win::{Handle, wide};
use crate::{log_error, log_info};

const TASK: &str = "Macaw";

/// Handles `--install-autostart` and `--uninstall-autostart` (run elevated by the tray).
/// Returns the process exit code if one of them was given.
pub fn handle_cli(args: &[String]) -> Option<i32> {
    let ok = match args.first().map(String::as_str) {
        Some("--install-autostart") => {
            let user = args
                .iter()
                .skip_while(|a| *a != "--user")
                .nth(1)
                .cloned()
                .unwrap_or_else(current_user);
            install(&user)
        }
        Some("--uninstall-autostart") => uninstall(),
        _ => return None,
    };
    Some(match ok {
        Ok(()) => 0,
        Err(e) => {
            log_error!("{e}");
            1
        }
    })
}

pub fn current_user() -> String {
    let domain = std::env::var("USERDOMAIN").unwrap_or_default();
    let user = std::env::var("USERNAME").unwrap_or_default();
    if domain.is_empty() {
        user
    } else {
        format!("{domain}\\{user}")
    }
}

fn schtasks(args: &[&str]) -> bool {
    Command::new("schtasks.exe")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

pub fn exists() -> bool {
    schtasks(&["/Query", "/TN", TASK])
}

/// Starts the elevated instance now. Returns immediately.
pub fn start() -> bool {
    schtasks(&["/Run", "/TN", TASK])
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// The task definition. Unlike `schtasks /Create /SC ONLOGON`, this also lifts the 72-hour
/// run limit, lets it start on battery and keeps normal priority.
fn task_xml(user: &str, exe: &str) -> String {
    let user = escape_xml(user);
    let exe = escape_xml(exe);
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Starts Macaw (Mac keyboard helper) at sign-in with administrator rights, so it also works in admin apps.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>4</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>"{exe}"</Command>
      <Arguments>--from-task</Arguments>
    </Exec>
  </Actions>
</Task>
"#
    )
}

fn install(user: &str) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("can't find Macaw's own path: {e}"))?;
    let xml = task_xml(user, &exe.to_string_lossy());
    let path = std::env::temp_dir().join("macaw-task.xml");
    // schtasks wants UTF-16 with a byte-order mark for XML that says encoding="UTF-16".
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
    std::fs::write(&path, bytes).map_err(|e| format!("can't write the task file: {e}"))?;
    let created = schtasks(&["/Create", "/TN", TASK, "/XML", &path.to_string_lossy(), "/F"]);
    let _ = std::fs::remove_file(&path);
    if created {
        log_info!("start-at-sign-in task created for {user}");
        Ok(())
    } else {
        Err("schtasks could not create the start-at-sign-in task".into())
    }
}

fn uninstall() -> Result<(), String> {
    if schtasks(&["/Delete", "/TN", TASK, "/F"]) {
        log_info!("start-at-sign-in task removed");
        Ok(())
    } else {
        Err("schtasks could not remove the start-at-sign-in task".into())
    }
}

/// Runs this program with `args` as administrator (one UAC prompt) and waits for it.
/// `Ok(true)` if it succeeded, `Ok(false)` if it failed, `Err` if the prompt was declined.
pub fn run_elevated(args: &str) -> Result<bool, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = wide(&exe.to_string_lossy());
    let verb = wide("runas");
    let params = wide(args);
    // SAFETY: the strings outlive the call; the process handle is closed by `Handle`.
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
        info.lpVerb = verb.as_ptr();
        info.lpFile = exe.as_ptr();
        info.lpParameters = params.as_ptr();
        info.nShow = SW_HIDE;
        if ShellExecuteExW(&mut info) == 0 || info.hProcess.is_null() {
            return Err("the permission prompt was declined".into());
        }
        let process = Handle(info.hProcess);
        if WaitForSingleObject(process.0, 60_000) != WAIT_OBJECT_0 {
            return Ok(false);
        }
        let mut code = 1u32;
        GetExitCodeProcess(process.0, &mut code);
        Ok(code == 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_xml_escapes_and_disables_limits() {
        let xml = task_xml("PC\\me & you", "C:\\Apps\\Macaw\\macaw.exe");
        assert!(xml.contains("<UserId>PC\\me &amp; you</UserId>"));
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(xml.contains("<DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>"));
        assert!(xml.contains("<RunLevel>HighestAvailable</RunLevel>"));
        assert!(xml.contains("<Command>\"C:\\Apps\\Macaw\\macaw.exe\"</Command>"));
    }
}
