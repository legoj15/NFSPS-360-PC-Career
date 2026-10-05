//! Minimal STFS `CON ` package header parsing — just enough to identify the
//! title a save belongs to and read its display name.
//!
//! Offsets are locked by the tracked oracles `docs/re/c1_latest/CAREER_01_360`
//! and `docs/re/pair/CAREER_02_360_fresh` and match Party Buffalo's
//! `STFSOffsets` and `scripts/python/nfssave/container360.py` (SPEC.md §7).

use crate::error::{Error, Result};

/// Package magic for console-signed saves.
pub const CON_MAGIC: &[u8; 4] = b"CON ";
/// Offset of the big-endian u32 header size (start of the first hash table
/// once page-aligned; `0x971A` -> `0xA000` for the tracked oracles).
pub const HEADER_SIZE_OFFSET: usize = 0x340;
/// Offset of the 4 raw title-ID bytes (`45 41 08 22` for ProStreet).
pub const TITLE_ID_OFFSET: usize = 0x360;
/// Offset of the UTF-16BE display name ("NFS ProStreet").
pub const DISPLAY_NAME_OFFSET: usize = 0x1691;
/// Length reserved for the display name.
pub const DISPLAY_NAME_LEN: usize = 0x80;
/// Need for Speed: ProStreet (verified against both tracked oracles and
/// community title-ID lists).
pub const TITLE_ID_NFS_PROSTREET: [u8; 4] = [0x45, 0x41, 0x08, 0x22];

/// Minimum input length [`ConHeader::parse`] accepts: one byte past the
/// display-name field.
const MIN_LEN: usize = DISPLAY_NAME_OFFSET + DISPLAY_NAME_LEN;

/// The subset of the CON header this crate cares about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConHeader {
    /// Big-endian u32 at [`HEADER_SIZE_OFFSET`].
    pub header_size: u32,
    /// Raw title-ID bytes at [`TITLE_ID_OFFSET`].
    pub title_id: [u8; 4],
    /// Decoded display name (empty when the field is all NUL).
    pub display_name: String,
}

impl ConHeader {
    /// Parses the header fields out of a package's leading bytes.
    ///
    /// `data` must be at least `0x1711` bytes (a full header + metadata
    /// block); truncated input is rejected rather than partially parsed.
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.len() < 4 || &data[..4] != CON_MAGIC {
            let mut magic = [0u8; 4];
            let n = data.len().min(4);
            magic[..n].copy_from_slice(&data[..n]);
            return Err(Error::NotCon(magic));
        }
        if data.len() < MIN_LEN {
            let mut magic = [0u8; 4];
            magic.copy_from_slice(&data[..4]);
            return Err(Error::NotCon(magic));
        }

        let header_size = u32::from_be_bytes(
            data[HEADER_SIZE_OFFSET..HEADER_SIZE_OFFSET + 4]
                .try_into()
                .expect("4-byte slice"),
        );
        let mut title_id = [0u8; 4];
        title_id.copy_from_slice(&data[TITLE_ID_OFFSET..TITLE_ID_OFFSET + 4]);
        let display_name = decode_utf16be_nul_padded(
            &data[DISPLAY_NAME_OFFSET..DISPLAY_NAME_OFFSET + DISPLAY_NAME_LEN],
        );

        Ok(Self {
            header_size,
            title_id,
            display_name,
        })
    }

    /// Whether the title ID matches any of `ids`.
    pub fn title_id_matches(&self, ids: &[[u8; 4]]) -> bool {
        ids.contains(&self.title_id)
    }
}

/// Decodes a fixed-size UTF-16BE field trimmed at the first NUL code unit,
/// dropping unpaired surrogates lossily.
fn decode_utf16be_nul_padded(field: &[u8]) -> String {
    let (units, _remainder) = field.as_chunks::<2>();
    let units: Vec<u16> = units
        .iter()
        .map(|pair| u16::from_be_bytes(*pair))
        .take_while(|&unit| unit != 0)
        .collect();
    String::from_utf16_lossy(&units)
}
