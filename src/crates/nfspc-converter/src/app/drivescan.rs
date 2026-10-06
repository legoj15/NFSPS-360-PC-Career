//! Background drive scans.
//!
//! Default: every mounted drive letter is checked for a console-written
//! `Content\` tree at its root. Console-formatted USB sticks are plain FAT32
//! (measured on real hardware 2026-10-05, fatx SPEC.md §5.1), so this needs
//! no elevation and finds the saves directly.
//!
//! Opt-in ([`scan_drives`] with `include_fatx`): raw FATX scan that probes
//! `\\.\PhysicalDrive0..=15` read-only; every drive that opens is checked for the Xbox 360 FATX Data partition and walked for ProStreet
//! saves. Normal (non-Xbox) disks are skipped silently; access-denied
//! drives are summarized into one note so the UI can tell the user to
//! relaunch elevated.

use std::fs;
use std::path::{Path, PathBuf};

use fatx::device::{DeviceSource, MAX_DRIVE_INDEX, WindowsPhysicalDrives};
use fatx::discovery::{CONTENT_ROOT, SAVE_TYPE_DIRS, is_save_name};
use fatx::stfs::file_table_name;
use fatx::{DiscoveredSave, FatxVolume, XboxDriveImage, discover_prostreet_saves_noted};

/// Result of one full drive scan.
#[derive(Debug, Clone, Default)]
pub struct DriveScanReport {
    /// ProStreet saves found (CON containers with full bytes).
    pub saves: Vec<DiscoveredSave>,
    /// Human-readable notes worth surfacing (permission problems, FATX
    /// errors). An empty report with no notes simply means no Xbox media.
    pub notes: Vec<String>,
}

const ERROR_FILE_NOT_FOUND: i32 = 2;
const ERROR_PATH_NOT_FOUND: i32 = 3;
const ERROR_ACCESS_DENIED: i32 = 5;

/// Full scan: mounted volumes always, raw FATX drives when `include_fatx`.
/// Blocking - run it off the UI thread.
pub fn scan_drives(include_fatx: bool) -> DriveScanReport {
    suppress_no_disk_dialogs();
    let mut report = scan_volume_roots(&mounted_volume_roots());
    if include_fatx {
        let mut raw = scan_physical_drives();
        report.saves.append(&mut raw.saves);
        report.notes.append(&mut raw.notes);
        report
            .saves
            .sort_by(|a, b| a.source_path.cmp(&b.source_path));
    }
    report
}

/// Touching an empty card-reader slot can raise a modal "There is no disk
/// in the drive" system dialog. Turn those into plain I/O errors for the
/// calling (scan worker) thread only.
fn suppress_no_disk_dialogs() {
    use windows::Win32::System::Diagnostics::Debug::{
        SEM_FAILCRITICALERRORS, SEM_NOOPENFILEERRORBOX, SetThreadErrorMode,
    };
    // SAFETY: thread-local flag change; the previous mode is not needed
    // because the worker thread exits after the scan.
    let _ = unsafe { SetThreadErrorMode(SEM_FAILCRITICALERRORS | SEM_NOOPENFILEERRORBOX, None) };
}

/// Drive-letter roots worth scanning: removable, fixed and RAM disks.
/// Network and optical drives are skipped (slow, never console-written).
pub fn mounted_volume_roots() -> Vec<PathBuf> {
    use windows::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    use windows::core::PCWSTR;
    const DRIVE_REMOVABLE: u32 = 2;
    const DRIVE_FIXED: u32 = 3;
    const DRIVE_RAMDISK: u32 = 6;

    // SAFETY: plain Win32 queries; the root string is NUL-terminated and
    // outlives the call.
    let mask = unsafe { GetLogicalDrives() };
    let mut roots = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let root = format!("{}:\\", (b'A' + i as u8) as char);
        let wide: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
        let kind = unsafe { GetDriveTypeW(PCWSTR(wide.as_ptr())) };
        if matches!(kind, DRIVE_REMOVABLE | DRIVE_FIXED | DRIVE_RAMDISK) {
            roots.push(PathBuf::from(root));
        }
    }
    roots
}

/// Scans each root for `Content/<profile>/<titleID>/0000000{1,2}/*`, keeping
/// files by the same rule as the FATX discovery ([`is_save_name`]). Missing or unreadable roots (empty card readers) are silent.
pub fn scan_volume_roots(roots: &[PathBuf]) -> DriveScanReport {
    let mut report = DriveScanReport::default();
    for root in roots {
        scan_one_root(root, &mut report);
    }
    report
        .saves
        .sort_by(|a, b| a.source_path.cmp(&b.source_path));
    report
}

fn scan_one_root(root: &Path, report: &mut DriveScanReport) {
    let label = root.display().to_string().replace('\\', "/");
    let label = label.trim_end_matches('/');

    if root.join("Xbox360").join("Data0000").is_file() {
        report.notes.push(format!(
            "{label}: older Xbox 360 storage format (Xbox360/Data0000...) \
             found; saves inside it cannot be read yet"
        ));
    }

    let Ok(profiles) = fs::read_dir(root.join(CONTENT_ROOT)) else {
        return; // no Content folder (or no media): not console storage
    };
    for profile in profiles.filter_map(|e| e.ok()) {
        if !profile.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let Ok(titles) = fs::read_dir(profile.path()) else {
            continue;
        };
        for title in titles.filter_map(|e| e.ok()) {
            if !title.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            for save_type in SAVE_TYPE_DIRS {
                let dir = title.path().join(save_type);
                let Ok(files) = fs::read_dir(&dir) else {
                    continue; // absent save-type folder is normal
                };
                for file in files.filter_map(|e| e.ok()) {
                    if !file.file_type().is_ok_and(|t| t.is_file()) {
                        continue;
                    }
                    let name = file.file_name().to_string_lossy().into_owned();
                    let source_path = format!(
                        "{label}/{CONTENT_ROOT}/{}/{}/{save_type}/{name}",
                        profile.file_name().to_string_lossy(),
                        title.file_name().to_string_lossy(),
                    );
                    if !is_save_name(&name) {
                        continue;
                    }
                    let bytes = match read_save_package(&file.path()) {
                        Ok(b) => b,
                        Err(e) => {
                            report.notes.push(format!("{source_path}: skipped ({e})"));
                            continue;
                        }
                    };
                    let friendly_name = file_table_name(&bytes).unwrap_or_else(|_| name.clone());
                    report.saves.push(DiscoveredSave {
                        friendly_name,
                        source_path,
                        bytes,
                    });
                }
            }
        }
    }
}

/// Upper bound for a save package read whole. Real careers are 823,296
/// bytes and aliases 81,920; anything near this cap is not a save.
pub const MAX_SAVE_BYTES: u64 = 16 * 1024 * 1024;

/// Reads a save-named file only if it is a plausible save package: at most
/// [`MAX_SAVE_BYTES`] and starting with the `CON ` magic.
fn read_save_package(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::{Error, ErrorKind};
    let len = fs::metadata(path)?.len();
    if len > MAX_SAVE_BYTES {
        return Err(Error::new(
            ErrorKind::InvalidData,
            format!("{len} bytes is too large for a save"),
        ));
    }
    let bytes = fs::read(path)?;
    if !bytes.starts_with(b"CON ") {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "not an Xbox 360 save package (no CON header)",
        ));
    }
    Ok(bytes)
}

/// Scan every physical drive for raw FATX. Needs elevation. Blocking.
pub fn scan_physical_drives() -> DriveScanReport {
    let source = WindowsPhysicalDrives::new();
    let mut report = DriveScanReport::default();
    let mut denied: Vec<u32> = Vec::new();

    for index in 0..=MAX_DRIVE_INDEX {
        let device = match source.open(index) {
            Ok(d) => d,
            Err(e) => match e.raw_os_error() {
                Some(ERROR_FILE_NOT_FOUND) | Some(ERROR_PATH_NOT_FOUND) => continue,
                Some(ERROR_ACCESS_DENIED) => {
                    denied.push(index);
                    continue;
                }
                _ => {
                    report.notes.push(format!("PhysicalDrive{index}: {e}"));
                    continue;
                }
            },
        };
        let mut reader = device.reader;
        // Normal PC disks are not Xbox media and are expected everywhere:
        // skip them without a note.
        let image = match XboxDriveImage::probe(&mut reader, device.length) {
            Ok(img) => img,
            Err(_) => continue,
        };
        let mut volume = match FatxVolume::open(
            reader,
            image.data_partition.offset,
            image.data_partition.length,
        ) {
            Ok(v) => v,
            Err(e) => {
                report
                    .notes
                    .push(format!("PhysicalDrive{index}: FATX open failed: {e}"));
                continue;
            }
        };
        match discover_prostreet_saves_noted(&mut volume) {
            Ok(mut found) => {
                report.saves.append(&mut found.saves);
                report.notes.append(&mut found.notes);
            }
            Err(e) => report
                .notes
                .push(format!("PhysicalDrive{index}: scan failed: {e}")),
        }
    }

    if !denied.is_empty() {
        report.notes.push(format!(
            "PhysicalDrive{} could not be opened without administrator rights; \
             relaunch the app elevated to scan {}.",
            denied
                .iter()
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join(", "),
            if denied.len() == 1 { "it" } else { "them" }
        ));
    }
    report
}
