//! Scan the machine's real physical drives for an Xbox-360-formatted stick,
//! list every ProStreet save it finds, and dump each save's CON bytes to a
//! directory for inspection or conversion by other tools.
//!
//! Usage: cargo run -p fatx --example real_scan -- [dump-dir]
//! (defaults to src/target/realdump)
//!
//! Also doubles as the unprivileged raw-access probe: per-drive open outcomes
//! are printed, so `AccessDenied` vs `Opened` on a removable stick settles
//! whether the app needs an elevated manifest on a given machine.

#[cfg(windows)]
fn main() {
    use fatx::{
        DeviceSource, FatxVolume, OpenStatus, WindowsPhysicalDrives, XboxDriveImage,
        discover_prostreet_saves_noted,
    };

    let dump_dir =
        std::env::args().nth(1).unwrap_or_else(|| "src/target/realdump".to_string());
    std::fs::create_dir_all(&dump_dir).expect("create dump dir");

    let source = WindowsPhysicalDrives::new();
    let mut found = 0usize;

    for index in 0..=fatx::device::MAX_DRIVE_INDEX {
        match source.probe(index) {
            OpenStatus::Opened => {}
            status => {
                println!("drive {index}: {status:?} - skipped");
                continue;
            }
        }
        let device = match source.open(index) {
            Ok(device) => device,
            Err(err) => {
                println!("drive {index}: opened by probe but failed to open: {err}");
                continue;
            }
        };
        let mut reader = device.reader;
        let image = match XboxDriveImage::probe(&mut reader, device.length) {
            Ok(image) => image,
            Err(err) => {
                println!("drive {index}: not a 360-formatted drive ({err})");
                continue;
            }
        };
        println!(
            "drive {index}: 360 image, data partition at {:#x} ({} MiB)",
            image.data_partition.offset,
            image.data_partition.length / (1024 * 1024)
        );
        let mut volume =
            match FatxVolume::open(&mut reader, image.data_partition.offset,
                                   image.data_partition.length) {
                Ok(volume) => volume,
                Err(err) => {
                    println!("  FATX open failed: {err}");
                    continue;
                }
            };
        let report = match discover_prostreet_saves_noted(&mut volume, &[]) {
            Ok(report) => report,
            Err(err) => {
                println!("  scan failed: {err}");
                continue;
            }
        };
        for note in &report.notes {
            println!("  note: {note}");
        }
        if report.saves.is_empty() {
            println!("  no ProStreet saves on this drive");
        }
        for save in &report.saves {
            println!(
                "  {} ({} bytes) at {}",
                save.friendly_name,
                save.bytes.len(),
                save.source_path
            );
            let stem: String = save
                .friendly_name
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
                .collect();
            let target = format!("{dump_dir}/{}.CON", stem);
            std::fs::write(&target, &save.bytes).expect("dump write");
            println!("    dumped -> {target}");
            found += 1;
        }
    }
    println!("done: {found} save(s) dumped under {dump_dir}");
}

#[cfg(not(windows))]
fn main() {
    println!("real_scan only supports Windows (raw PhysicalDrive access)");
    std::process::exit(1);
}
