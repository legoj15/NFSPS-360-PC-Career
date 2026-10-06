//! Real-hardware check of the mounted-volume scan (no elevation needed):
//! prints the drive roots considered and every save found.
//!
//!     cargo run -p nfspc-converter --example volume_scan [-- --fatx]
//!
//! `--fatx` adds the raw FATX scan (run from an elevated shell).

use nfspc_converter::app::drivescan::{mounted_volume_roots, scan_drives};

fn main() {
    let fatx = std::env::args().any(|a| a == "--fatx");
    println!("roots: {:?}", mounted_volume_roots());
    let report = scan_drives(fatx);
    for s in &report.saves {
        println!(
            "save: {} ({}, {} bytes)",
            s.friendly_name,
            s.source_path,
            s.bytes.len()
        );
    }
    for n in &report.notes {
        println!("note: {n}");
    }
    println!(
        "{} save(s), {} note(s)",
        report.saves.len(),
        report.notes.len()
    );
}
