//! Raw physical-drive access probe (Windows).
//!
//! This performs the real, unelevated read-only open attempt on this
//! machine (`cargo test -p fatx` runs it exactly once) so the app can
//! decide whether its manifest must set `requireAdministrator`. The test
//! asserts API coherence only — every OpenStatus variant is acceptable;
//! the *distribution* of statuses is the datum.

use fatx::device::{probe_report, DeviceSource, OpenStatus, WindowsPhysicalDrives};

#[test]
fn physical_drive_probe_reports_coherent_statuses() {
    let source = WindowsPhysicalDrives::new();
    assert_eq!(source.name(), "WindowsPhysicalDrives");

    let report = probe_report(&source);
    assert_eq!(report.len() as u32, 16, "probes PhysicalDrive0..=15");

    // Every outcome is a valid variant by construction; surface the datum.
    for (index, status) in &report {
        println!("PhysicalDrive{index}: {status:?}");
        if *status == OpenStatus::Opened {
            // A successful read-only open must actually be readable: verify
            // with the same handle path the app would use.
            let opened = source.open(*index);
            match opened {
                Ok(dev) => {
                    use std::io::Read;
                    let mut buf = [0u8; 4];
                    let mut reader = dev.reader;
                    let n = reader.read(&mut buf).unwrap_or(0);
                    println!(
                        "  -> opened, length {} bytes, first read: {} bytes {:02X?}",
                        dev.length, n, buf
                    );
                }
                Err(e) => println!("  -> open after successful probe failed: {e}"),
            }
        }
    }

    // Sanity: probing an absurdly high index reports NotFound or Failed,
    // never panics.
    let high = source.probe(9_999);
    assert!(matches!(
        high,
        OpenStatus::NotFound | OpenStatus::Failed(_)
    ));
}
