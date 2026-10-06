//! Minimal STFS `CON ` package header parsing — just enough to identify the
//! title a save belongs to and read its title name.
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
/// Offset of the STFS display name: 18 locale slots of [`DISPLAY_NAME_LEN`]
/// bytes, UTF-16BE, NUL-padded. Slot 0 carries the per-save name ("Career
/// 01", an alias save's player name). Not parsed by [`ConHeader`].
pub const DISPLAY_NAME_OFFSET: usize = 0x411;
/// Length of one display-name locale slot.
pub const DISPLAY_NAME_LEN: usize = 0x80;
/// Offset of the UTF-16BE title name — the game title ("NFS ProStreet"),
/// identical on every save.
pub const TITLE_NAME_OFFSET: usize = 0x1691;
/// Length reserved for the title name.
pub const TITLE_NAME_LEN: usize = 0x80;
/// Volume-descriptor byte whose bit 0 selects the hash-table layout
/// (clear = every hash table stored twice -> backing-block shift 1).
pub const TABLE_SHIFT_OFFSET: usize = 0x37B;
/// Offset of the LE24 file-table block number inside the volume descriptor.
pub const FILE_TABLE_BLOCK_OFFSET: usize = 0x37E;
/// STFS data block size.
pub const STFS_BLOCK: usize = 0x1000;
/// Data blocks per level-0 hash table.
pub const HASHES_PER_TABLE: usize = 0xAA;
/// Data blocks per level-1 hash table (170 * 170).
pub const L1_SPAN: usize = 0x70E4;
/// Need for Speed: ProStreet (verified against both tracked oracles and
/// community title-ID lists).
pub const TITLE_ID_NFS_PROSTREET: [u8; 4] = [0x45, 0x41, 0x08, 0x22];

/// Minimum input length [`ConHeader::parse`] accepts: one byte past the
/// title-name field.
const MIN_LEN: usize = TITLE_NAME_OFFSET + TITLE_NAME_LEN;

/// The subset of the CON header this crate cares about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConHeader {
    /// Big-endian u32 at [`HEADER_SIZE_OFFSET`].
    pub header_size: u32,
    /// Raw title-ID bytes at [`TITLE_ID_OFFSET`].
    pub title_id: [u8; 4],
    /// Decoded title name at [`TITLE_NAME_OFFSET`] (empty when the field
    /// is all NUL).
    pub title_name: String,
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
        let title_name =
            decode_utf16be_nul_padded(&data[TITLE_NAME_OFFSET..TITLE_NAME_OFFSET + TITLE_NAME_LEN]);

        Ok(Self {
            header_size,
            title_id,
            title_name,
        })
    }

    /// Whether the title ID matches any of `ids`.
    pub fn title_id_matches(&self, ids: &[[u8; 4]]) -> bool {
        ids.contains(&self.title_id)
    }
}

/// File offset of STFS data block `block` given the page-aligned first hash
/// table and the backing-block shift (1 when every hash table is stored
/// twice). Mirrors `nfssave_core::container360::stfs_block_offset` and the
/// Python spec `scripts/python/nfssave/container360.py`.
fn stfs_block_offset(block: usize, first_table: usize, shift: usize) -> usize {
    let mut backing = (((block + HASHES_PER_TABLE) / HASHES_PER_TABLE) << shift) + block;
    if block >= HASHES_PER_TABLE {
        backing += ((block + L1_SPAN) / L1_SPAN) << shift;
        if block >= L1_SPAN {
            backing += 1usize << shift;
        }
    }
    first_table + backing * STFS_BLOCK
}

/// Extracts the per-save name from a CON package's STFS file table (see
/// [`ConHeader::parse`] for the header fields used to locate it). Returns
/// an error when the package is too short or the table block is beyond EOF.
pub fn file_table_name(data: &[u8]) -> Result<String> {
    let bad = |why: &str| Error::BadEntry(format!("STFS file table unreadable: {why}"));
    if data.len() < MIN_LEN {
        return Err(bad("package shorter than a CON header"));
    }
    let header_size = u32::from_be_bytes(
        data[HEADER_SIZE_OFFSET..HEADER_SIZE_OFFSET + 4]
            .try_into()
            .expect("4-byte slice"),
    ) as usize;
    let first_table = header_size.div_ceil(STFS_BLOCK) * STFS_BLOCK;
    let shift = if data[TABLE_SHIFT_OFFSET] & 1 != 0 { 0 } else { 1 };
    let b = &data[FILE_TABLE_BLOCK_OFFSET..FILE_TABLE_BLOCK_OFFSET + 3];
    let table_block = b[0] as usize | ((b[1] as usize) << 8) | ((b[2] as usize) << 16);
    let off = stfs_block_offset(table_block, first_table, shift);
    let Some(entry) = data.get(off..off + 0x40) else {
        return Err(bad(&format!("block {table_block} at {off:#x} beyond EOF")));
    };
    let nul = entry[..0x28].iter().position(|&x| x == 0).unwrap_or(0x28);
    let name: String = entry[..nul]
        .iter()
        .map(|&x| if x.is_ascii() { x as char } else { char::REPLACEMENT_CHARACTER })
        .collect();
    if name.is_empty() {
        return Err(bad("empty file-table entry"));
    }
    Ok(name)
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
