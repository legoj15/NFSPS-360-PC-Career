//! Console plumbing for the windowed release exe.
//!
//! Release builds use the Windows GUI subsystem (main.rs), so no console
//! window appears behind the app. The headless and help paths still need to
//! print: when stdout/stderr were redirected (pipes, files, `Command::output`)
//! the inherited handles already work; otherwise the process borrows the
//! console of the terminal that started it. Double-clicked, there is none and
//! output is dropped, which only matters for the GUI error path - that one
//! uses [`error_box`].

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Console::{
    ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_HANDLE,
    STD_OUTPUT_HANDLE, SetStdHandle,
};

fn usable(h: windows::core::Result<HANDLE>) -> Option<HANDLE> {
    h.ok().filter(|h| !h.is_invalid() && !h.0.is_null())
}

/// Points stdout/stderr at the parent terminal's console unless they are
/// already redirected. A no-op in console-subsystem (debug) builds, where
/// both handles are always set.
pub fn attach_parent_console() {
    // SAFETY: plain Win32 handle queries/assignments; no memory is shared.
    unsafe {
        let out = usable(GetStdHandle(STD_OUTPUT_HANDLE));
        let err = usable(GetStdHandle(STD_ERROR_HANDLE));
        if out.is_some() && err.is_some() {
            return;
        }
        if AttachConsole(ATTACH_PARENT_PROCESS).is_err() {
            return;
        }
        // Attaching may rebind every standard handle to the console; keep
        // whichever stream the caller had redirected.
        let keep = |id: STD_HANDLE, h: Option<HANDLE>| {
            if let Some(h) = h {
                let _ = SetStdHandle(id, h);
            }
        };
        keep(STD_OUTPUT_HANDLE, out);
        keep(STD_ERROR_HANDLE, err);
    }
}

/// Modal error dialog for failures a windowed exe cannot print.
pub fn error_box(text: &str) {
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    use windows::core::{PCWSTR, w};

    let text_w: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    // SAFETY: NUL-terminated string that outlives the call.
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text_w.as_ptr()),
            w!("NFSPS-SaveConverter"),
            MB_OK | MB_ICONERROR,
        );
    }
}
