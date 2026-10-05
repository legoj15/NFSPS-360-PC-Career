//! Partition-level auto-detection for raw Xbox 360 drive images
//! (the "Horizon-style" entry point).
//!
//! Retail media has **no raw partition table** (see `SPEC.md` §5): the Data
//! partition lives at fixed offsets that we probe and validate against the
//! `XTAF` magic. Devkit HDDs carry a small little-endian table at LBA 0 which
//! we parse when the devkit magic is present. The community-lore
//! `MICROSOFT*XBOX360` sector-0 signature is tolerated as an optional hint
//! only.

use std::io::{Read, Seek, SeekFrom};

use crate::error::{Error, Result};
use crate::fatx::MAGIC;

/// Signature string associated with Xbox 360 media in community tools.
/// **UNVERIFIED** against a primary source (SPEC.md §5.4) — never required.
pub const USB_SIGNATURE: &[u8] = b"MICROSOFT*XBOX360";
/// Byte offset of the FATX Data partition on retail USB sticks.
pub const RETAIL_USB_DATA_OFFSET: u64 = 0x2000_0000;
/// Byte offset of the FATX Data partition on a stock retail 20 GB HDD.
/// Larger HDDs use different offsets; those need devkit tables or manual
/// offsets today.
pub const RETAIL_HDD_DATA_OFFSET: u64 = 0x130E_B0000;
/// Little-endian u32 at LBA 0 identifying a devkit HDD partition table.
pub const DEVKIT_MAGIC: u32 = 0x0002_0000;

/// Candidate retail layouts in probe order.
const RETAIL_CANDIDATES: [(DriveLayout, u64); 2] = [
    (DriveLayout::RetailUsb, RETAIL_USB_DATA_OFFSET),
    (DriveLayout::RetailHdd, RETAIL_HDD_DATA_OFFSET),
];

/// What a partition region is used for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartitionKind {
    /// The `Content/` FATX volume we scan for saves.
    Data,
    /// System Extended sub-partition (Kinect/Avatar data).
    SystemExtended,
    /// Cache volume.
    SystemCache,
    /// Devkit dashboard volume.
    Dashboard,
}

/// A located partition: byte offset and length within the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartitionRegion {
    /// Kind of the partition.
    pub kind: PartitionKind,
    /// Byte offset within the source.
    pub offset: u64,
    /// Length in bytes.
    pub length: u64,
}

/// One entry of a devkit HDD partition table (little-endian, LBA 0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevPartitionEntry {
    /// Stable label ("Content", "Dashboard").
    pub name: &'static str,
    /// Start LBA.
    pub start_lba: u32,
    /// Length in 0x200-byte sectors.
    pub length_sectors: u32,
}

impl DevPartitionEntry {
    /// Byte offset of this region (`start_lba * 0x200`).
    pub fn offset(&self) -> u64 {
        self.start_lba as u64 * 0x200
    }

    /// Byte length of this region (`length_sectors * 0x200`).
    pub fn length(&self) -> u64 {
        self.length_sectors as u64 * 0x200
    }
}

/// How the Data partition was located.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriveLayout {
    /// Retail USB stick: fixed offset 0x20000000, validated by magic.
    RetailUsb,
    /// Retail HDD: fixed offset (stock geometry), validated by magic.
    RetailHdd,
    /// Devkit HDD: entries parsed from the LBA-0 table.
    DevkitTable(Vec<DevPartitionEntry>),
}

/// Result of probing a raw image for Xbox 360 storage.
#[derive(Debug, Clone)]
pub struct XboxDriveImage {
    /// Whether `MICROSOFT*XBOX360` was found anywhere in the first 0x400
    /// bytes (informational only).
    pub signature_found: bool,
    /// How the Data partition was located.
    pub layout: DriveLayout,
    /// The `Content/` FATX partition.
    pub data_partition: PartitionRegion,
}

impl XboxDriveImage {
    /// Probes `src` (an entire raw drive image or device) of `total_len`
    /// bytes for the Xbox 360 Data partition.
    ///
    /// Detection order:
    /// 1. devkit table at LBA 0 (`0x00020000` magic, entries at 0x8),
    /// 2. retail USB fixed offset [`RETAIL_USB_DATA_OFFSET`],
    /// 3. retail HDD fixed offset [`RETAIL_HDD_DATA_OFFSET`],
    ///
    /// each validated by the `XTAF` superblock magic at the candidate offset.
    pub fn probe<R: Read + Seek>(src: &mut R, total_len: u64) -> Result<Self> {
        const SECTOR0_SCAN: usize = 0x400;

        let mut head = [0u8; SECTOR0_SCAN];
        let got = read_some_at(src, 0, &mut head)?;
        if got < 0x18 {
            return Err(Error::NotXboxImage(format!(
                "source too small to hold any Xbox 360 partition table ({got} bytes)"
            )));
        }
        let head = &head[..got];
        let signature_found = find_subslice(head, USB_SIGNATURE).is_some();

        // 1) Devkit HDD partition table (little-endian, magic at LBA 0).
        let magic_le = u32::from_le_bytes(head[..4].try_into().expect("4 bytes"));
        if magic_le == DEVKIT_MAGIC {
            let content_lba = u32::from_le_bytes(head[0x8..0xC].try_into().expect("4 bytes"));
            let content_sectors =
                u32::from_le_bytes(head[0xC..0x10].try_into().expect("4 bytes"));
            let dash_lba =
                u32::from_le_bytes(head[0x10..0x14].try_into().expect("4 bytes"));
            let dash_sectors =
                u32::from_le_bytes(head[0x14..0x18].try_into().expect("4 bytes"));

            if content_sectors > 0 {
                let entry = DevPartitionEntry {
                    name: "Content",
                    start_lba: content_lba,
                    length_sectors: content_sectors,
                };
                let offset = entry.offset();
                if has_xtaf_magic(src, offset)? {
                    let mut entries = vec![entry];
                    if dash_sectors > 0 {
                        entries.push(DevPartitionEntry {
                            name: "Dashboard",
                            start_lba: dash_lba,
                            length_sectors: dash_sectors,
                        });
                    }
                    return Ok(Self {
                        signature_found,
                        layout: DriveLayout::DevkitTable(entries),
                        data_partition: PartitionRegion {
                            kind: PartitionKind::Data,
                            offset,
                            length: content_sectors as u64 * 0x200,
                        },
                    });
                }
                return Err(Error::NotXboxImage(format!(
                    "devkit table points at {offset:#x} but no FATX superblock is there"
                )));
            }
        }

        // 2) Retail fixed offsets, validated by the FATX magic.
        for (layout, offset) in RETAIL_CANDIDATES {
            if total_len > offset && has_xtaf_magic(src, offset)? {
                log::debug!("detected {layout:?} Data partition at {offset:#x}");
                return Ok(Self {
                    signature_found,
                    layout,
                    data_partition: PartitionRegion {
                        kind: PartitionKind::Data,
                        offset,
                        length: total_len - offset,
                    },
                });
            }
        }

        Err(Error::NotXboxImage(
            "no FATX superblock at any known offset (USB 0x20000000, HDD 0x130EB0000) \
             and no devkit partition table"
                .into(),
        ))
    }
}

fn read_some_at<R: Read + Seek>(src: &mut R, offset: u64, buf: &mut [u8]) -> Result<usize> {
    src.seek(SeekFrom::Start(offset))?;
    let mut filled = 0;
    while filled < buf.len() {
        match src.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

fn has_xtaf_magic<R: Read + Seek>(src: &mut R, offset: u64) -> Result<bool> {
    let mut magic = [0u8; 4];
    let got = read_some_at(src, offset, &mut magic)?;
    if got < 4 {
        return Ok(false);
    }
    Ok(magic == MAGIC)
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
