//! Background scan of physical drives via the `fatx` crate.
//!
//! Probes `\\.\PhysicalDrive0..=15` read-only; every drive that opens is
//! checked for the Xbox 360 FATX Data partition and walked for ProStreet
//! saves. Normal (non-Xbox) disks are skipped silently; access-denied
//! drives are summarized into one note so the UI can tell the user to
//! relaunch elevated.

use fatx::device::{DeviceSource, MAX_DRIVE_INDEX, WindowsPhysicalDrives};
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

/// Scan every physical drive. Blocking - run it off the UI thread.
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
        match discover_prostreet_saves_noted(&mut volume, &[]) {
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
