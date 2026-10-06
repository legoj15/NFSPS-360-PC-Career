//! Elevation for the opt-in raw FATX scan.
//!
//! The exe stays `asInvoker` (see build.rs). Raw `\\.\PhysicalDriveN` reads
//! need administrator rights, so the "scan for FATX drives" button either
//! scans in place (already elevated) or relaunches this exe through UAC with
//! [`SCAN_FATX_FLAG`].

use super::cli::SCAN_FATX_FLAG;

/// What the FATX button should do in the current process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FatxAction {
    /// Already elevated: run the raw scan here.
    ScanHere,
    /// Not elevated: relaunch through UAC, then close this window.
    Relaunch,
}

pub fn fatx_action(elevated: bool) -> FatxAction {
    if elevated {
        FatxAction::ScanHere
    } else {
        FatxAction::Relaunch
    }
}

/// True when this process token is elevated. Any query failure reads as
/// "not elevated" (the relaunch path is the safe default).
pub fn is_elevated() -> bool {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    // SAFETY: the token handle is checked and closed; the out buffer is a
    // correctly sized TOKEN_ELEVATION.
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elevation.TokenIsElevated != 0
    }
}

/// Starts this exe elevated with [`SCAN_FATX_FLAG`]. `Err` carries a
/// user-facing reason (UAC declined, exe path unknown, ...).
pub fn relaunch_elevated() -> Result<(), String> {
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{PCWSTR, w};

    let exe = std::env::current_exe()
        .map_err(|e| format!("cannot locate this program to relaunch it: {e}"))?;
    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(Some(0)).collect() };
    let exe_w = wide(&exe.to_string_lossy());
    let args_w = wide(SCAN_FATX_FLAG);

    // SAFETY: all strings are NUL-terminated and outlive the call.
    let rc = unsafe {
        ShellExecuteW(
            None,
            w!("runas"),
            PCWSTR(exe_w.as_ptr()),
            PCWSTR(args_w.as_ptr()),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecute returns a value > 32 on success.
    if rc.0 as usize > 32 {
        Ok(())
    } else {
        Err("administrator access was declined or failed; \
             the FATX scan did not run"
            .into())
    }
}
