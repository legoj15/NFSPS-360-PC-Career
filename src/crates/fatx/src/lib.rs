//! Xbox 360 USB / FATX scanner for the NFSPS-360-PC-Career converter.
//!
//! This crate is the Horizon-style front end: given a raw drive image (or a
//! `\\.\PhysicalDriveN` handle on Windows) it finds the FATX Data partition,
//! walks `Content/<profile>/<titleID>/0000000{1,2}/*`, and returns matching
//! Need for Speed: ProStreet save files with their bytes.
//!
//! The on-disk format is documented in `SPEC.md` next to this crate's
//! `Cargo.toml`; every layout constant cites its sources there.
//!
//! Typical flow over a whole raw image:
//!
//! ```no_run
//! # use std::io::{Read, Seek};
//! fn scan<R: Read + Seek>(img: &mut R, len: u64) -> fatx::Result<()> {
//!     let drive = fatx::XboxDriveImage::probe(img, len)?;
//!     let mut vol = fatx::FatxVolume::open(
//!         img, drive.data_partition.offset, drive.data_partition.length)?;
//!     for save in fatx::discover_prostreet_saves(&mut vol)? {
//!         println!("{} ({} bytes) at {}", save.friendly_name,
//!                  save.bytes.len(), save.source_path);
//!     }
//!     Ok(())
//! }
//! ```
//!
//! Feature `test-util` enables [`test_util`], which synthesizes complete
//! FATX USB images for hardware-free round-trip tests.

pub mod device;
pub mod discovery;
pub mod error;
pub mod fatx;
pub mod partition;
pub mod stfs;
#[cfg(feature = "test-util")]
pub mod test_util;

pub use device::{DeviceSource, OpenStatus, OpenedDevice, WindowsPhysicalDrives};
pub use discovery::{
    DiscoveredSave, DiscoveryReport, discover_prostreet_saves, discover_prostreet_saves_noted,
};
pub use error::{Error, Result};
pub use fatx::{DirEntry, FatxVolume, Superblock};
pub use partition::{DriveLayout, XboxDriveImage};
pub use stfs::ConHeader;
