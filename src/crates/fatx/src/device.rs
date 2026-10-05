//! Windows raw-device access behind a small trait.
//!
//! `\\.\PhysicalDriveN` is opened read-only (`GENERIC_READ`,
//! `FILE_SHARE_READ | FILE_SHARE_WRITE`, `OPEN_EXISTING`). Opening a raw
//! physical drive usually requires elevation on modern Windows;
//! [`DeviceSource::probe`] reports the precise outcome so the application
//! can decide whether its manifest must set `requireAdministrator`.

use std::io;
use std::io::{Read, Seek};

/// Highest `PhysicalDriveN` index probed by [`probe_report`].
pub const MAX_DRIVE_INDEX: u32 = 15;

/// Outcome of a read-only open attempt on one physical drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenStatus {
    /// The handle was granted; raw reads work without elevation.
    Opened,
    /// `ERROR_ACCESS_DENIED` — elevation is required on this machine.
    AccessDenied,
    /// No such drive (`ERROR_FILE_NOT_FOUND` / `ERROR_PATH_NOT_FOUND`).
    NotFound,
    /// `ERROR_SHARING_VIOLATION` — drive exists but is exclusively locked.
    Busy,
    /// Any other failure, with the OS error rendered into the string.
    Failed(String),
}

/// A readable + seekable + sendable byte source (what the FATX reader needs).
pub trait ReadSeek: Read + Seek + Send {}
impl<T: Read + Seek + Send> ReadSeek for T {}

/// An opened raw device together with its byte length.
pub struct OpenedDevice {
    /// Seekable reader over the whole device.
    pub reader: Box<dyn ReadSeek>,
    /// Device length in bytes (from `IOCTL_DISK_GET_LENGTH_INFO`).
    pub length: u64,
}

/// Abstraction over "where raw drives come from" so tests and alternative
/// platforms can substitute behavior.
pub trait DeviceSource: Send + Sync {
    /// Stable name for diagnostics.
    fn name(&self) -> &'static str;
    /// Attempts a read-only open of `\\.\PhysicalDrive{index}` and reports
    /// the outcome without keeping the handle.
    fn probe(&self, index: u32) -> OpenStatus;
    /// Opens `\\.\PhysicalDrive{index}` read-only.
    fn open(&self, index: u32) -> io::Result<OpenedDevice>;
}

/// Real Windows implementation on top of the `windows` crate.
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowsPhysicalDrives;

impl WindowsPhysicalDrives {
    /// Constructor (the type carries no state).
    pub fn new() -> Self {
        Self
    }
}

/// Probes `\\.\PhysicalDrive0` ..= `\\.\PhysicalDrive{MAX_DRIVE_INDEX}` and
/// returns the per-index outcome. This is the "can we work without admin"
/// datum: any `AccessDenied` on an existing drive means the app manifest
/// needs `requireAdministrator`.
pub fn probe_report(source: &dyn DeviceSource) -> Vec<(u32, OpenStatus)> {
    (0..=MAX_DRIVE_INDEX).map(|index| (index, source.probe(index))).collect()
}

// ---- platform implementations ----

#[cfg(windows)]
mod imp {
    use super::*;

    use std::os::windows::io::FromRawHandle;

    use windows::Win32::Foundation::{GENERIC_READ, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
        FILE_FLAGS_AND_ATTRIBUTES,
    };
    use windows::Win32::System::IO::DeviceIoControl;
    use windows::core::PCWSTR;

    /// `CTL_CODE(IOCTL_DISK_BASE, 0x0017, METHOD_BUFFERED, FILE_ANY_ACCESS)`
    /// = IOCTL_DISK_GET_LENGTH_INFO (0x0007405C); same constant the
    /// reference tools use.
    const IOCTL_DISK_GET_LENGTH_INFO: u32 = 0x0007_405C;

    const ERROR_ACCESS_DENIED: i32 = 5;
    const ERROR_FILE_NOT_FOUND: i32 = 2;
    const ERROR_PATH_NOT_FOUND: i32 = 3;
    const ERROR_SHARING_VIOLATION: i32 = 32;

    /// The `windows` crate reports failures as HRESULTs; errors coming from
    /// kernel32 arrive as `0x8007####` (FACILITY_WIN32 wrapping the real
    /// Win32 error). Unwrap those so `io::Error::raw_os_error` is the plain
    /// Win32 code our status mapping expects.
    fn into_io_error(e: windows::core::Error) -> io::Error {
        let code = e.code().0;
        let win32 = if (code as u32 & 0xFFFF_0000) == 0x8007_0000 {
            (code as u32 & 0xFFFF) as i32
        } else {
            code
        };
        io::Error::from_raw_os_error(win32)
    }

    fn create_handle(index: u32) -> io::Result<HANDLE> {
        let path: Vec<u16> = format!("\\\\.\\PhysicalDrive{}\0", index).encode_utf16().collect();
        // SAFETY: `path` is a valid NUL-terminated wide string for the
        // duration of the call; no output pointers are passed.
        let handle = unsafe {
            CreateFileW(
                PCWSTR(path.as_ptr()),
                GENERIC_READ.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAGS_AND_ATTRIBUTES(0),
                None,
            )
        }
        .map_err(into_io_error)?;
        Ok(handle)
    }

    fn status_from_err(err: &io::Error) -> OpenStatus {
        match err.raw_os_error() {
            Some(ERROR_ACCESS_DENIED) => OpenStatus::AccessDenied,
            Some(ERROR_FILE_NOT_FOUND) | Some(ERROR_PATH_NOT_FOUND) => OpenStatus::NotFound,
            Some(ERROR_SHARING_VIOLATION) => OpenStatus::Busy,
            _ => OpenStatus::Failed(err.to_string()),
        }
    }

    fn device_length(handle: HANDLE, file: &std::fs::File) -> io::Result<u64> {
        let mut length: u64 = 0;
        let mut returned: u32 = 0;
        // SAFETY: `length` is a valid 8-byte output buffer matching the
        // ioctl's contract; the handle is owned by `file` and stays alive
        // for the duration of the call.
        let ok = unsafe {
            DeviceIoControl(
                handle,
                IOCTL_DISK_GET_LENGTH_INFO,
                None,
                0,
                Some((&mut length as *mut u64).cast()),
                size_of::<u64>() as u32,
                Some(&mut returned),
                None,
            )
        };
        if ok.is_ok() && length > 0 {
            return Ok(length);
        }
        // Fallback for providers without ioctl support: seek to the end.
        let mut file = file;
        let end = file.seek(io::SeekFrom::End(0))?;
        file.seek(io::SeekFrom::Start(0))?;
        Ok(end)
    }

    impl DeviceSource for WindowsPhysicalDrives {
        fn name(&self) -> &'static str {
            "WindowsPhysicalDrives"
        }

        fn probe(&self, index: u32) -> OpenStatus {
            match create_handle(index) {
                Ok(_) => OpenStatus::Opened,
                Err(e) => status_from_err(&e),
            }
        }

        fn open(&self, index: u32) -> io::Result<OpenedDevice> {
            let handle = create_handle(index)?;
            // The std File takes ownership of the handle (HANDLE has no Drop
            // in the windows crate), so it will be closed exactly once.
            // SAFETY: the handle came straight from a successful CreateFileW
            // and is not owned by anything else.
            let file = unsafe { std::fs::File::from_raw_handle(handle.0 as _) };
            let length = device_length(handle, &file)?;
            Ok(OpenedDevice {
                reader: Box::new(file),
                length,
            })
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    impl DeviceSource for WindowsPhysicalDrives {
        fn name(&self) -> &'static str {
            "WindowsPhysicalDrives(unavailable)"
        }

        fn probe(&self, _index: u32) -> OpenStatus {
            OpenStatus::Failed("raw drives unsupported on this platform".into())
        }

        fn open(&self, _index: u32) -> io::Result<OpenedDevice> {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "raw drives unsupported on this platform",
            ))
        }
    }
}
