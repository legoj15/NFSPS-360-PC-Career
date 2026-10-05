//! Xbox 360 save container reader (STFS 'CON ' package).
//!
//! The saves are standard STFS packages (console-signed CON, unsigned content
//! works the same way):
//!
//! ```text
//!   0x0000  'CON ' header + certificate, metadata, PNG icons
//!   0x0340  BE u32 header size -> first hash table at (size + 0xFFF) & ~0xFFF
//!           (0xA000 for these saves)
//!   0x0379  volume descriptor; byte 0x37B bit 0 = block separation. Bit clear
//!           -> two copies of every hash table per level (table shift 1)
//!   data block 0 = file table (one 0x40-byte entry per file):
//!           +0x00 name (0x28, NUL-padded), +0x28 flags (0x40 = contiguous,
//!           low 6 bits = name length), +0x29 block count (LE24),
//!           +0x2F starting block (LE24), +0x34 BE u32 file size,
//!           +0x38/+0x3C update/access timestamps
//! ```
//!
//! Data blocks are interleaved with hash tables: a level-0 table group
//! precedes every 170 data blocks (and a level-1 group every 170*170), so a
//! payload larger than ~0xA9000 bytes is NOT one contiguous slice of the
//! file. Reading it as one (as earlier versions did) pulls hash tables into
//! the save and truncates its tail.

use std::fs;
use std::path::Path;

use crate::{Result, format_err};

pub const BLOCK: usize = 0x1000;
/// 170 data blocks per level-0 hash table
pub const HASHES_PER_TABLE: usize = 0xAA;
/// 170 * 170 data blocks per level-1 hash table
pub const L1_SPAN: usize = 0x70E4;
pub const FLAG_CONTIGUOUS: u8 = 0x40;

/// File offset of data block `block` (Free60/Velocity backing-block math).
///
/// `shift` is 1 when every hash table is stored twice, else 0.
pub fn stfs_block_offset(block: usize, first_table: usize, shift: u32) -> usize {
    let shift = shift as usize;
    let mut backing = (((block + HASHES_PER_TABLE) / HASHES_PER_TABLE) << shift) + block;
    if block >= HASHES_PER_TABLE {
        backing += ((block + L1_SPAN) / L1_SPAN) << shift;
        if block >= L1_SPAN {
            backing += 1usize << shift;
        }
    }
    first_table + backing * BLOCK
}

#[derive(Clone, Debug)]
pub struct Container360 {
    pub data: Vec<u8>,
    pub name: String,
    pub payload: Vec<u8>,
}

fn le24(b: &[u8]) -> usize {
    (b[0] as usize) | ((b[1] as usize) << 8) | ((b[2] as usize) << 16)
}

fn block_at<'a>(
    data: &'a [u8],
    n: usize,
    first_table: usize,
    shift: u32,
    label: &str,
) -> Result<&'a [u8]> {
    let off = stfs_block_offset(n, first_table, shift);
    if off >= data.len() {
        return Err(format_err(format!(
            "{label}: data block {n} at {off:#x} beyond file end"
        )));
    }
    Ok(&data[off..(off + BLOCK).min(data.len())])
}

pub fn parse_container(data: &[u8], label: &str) -> Result<Container360> {
    if data.get(..4) != Some(b"CON ") {
        return Err(format_err(format!(
            "{label}: not a CON container (magic {:x?})",
            data.get(..4)
        )));
    }
    if data.len() < 0x381 {
        return Err(format_err(format!(
            "{label}: truncated CON header ({:#x} B)",
            data.len()
        )));
    }
    let header_size = u32::from_be_bytes(data[0x340..0x344].try_into().unwrap()) as usize;
    let first_table = (header_size + BLOCK - 1) & !(BLOCK - 1);
    let shift = if data[0x37B] & 1 != 0 { 0 } else { 1 };

    let table_block = le24(&data[0x37E..0x381]);
    let block = block_at(data, table_block, first_table, shift, label)?;
    // Python clamps the block slice to EOF; every field this parser reads
    // sits in the first 0x38 bytes of the entry, so a shorter table block
    // means the file table itself is truncated (Python's unpack_from fails
    // there too). Refuse instead of panicking on the [..0x40] slice.
    if block.len() < 0x38 {
        return Err(format_err(format!(
            "{label}: STFS file table block truncated ({:#x} B)",
            block.len()
        )));
    }
    let entry = &block[..0x40.min(block.len())];
    let nul = entry[..0x28].iter().position(|&b| b == 0);
    let name_end = nul.unwrap_or(0x28);
    let name = String::from_utf8_lossy(&entry[..name_end]).into_owned();
    if name.is_empty() {
        return Err(format_err(format!("{label}: empty STFS file table")));
    }
    let flags = entry[0x28];
    let n_blocks = le24(&entry[0x29..0x2C]);
    let start = le24(&entry[0x2F..0x32]);
    let size = u32::from_be_bytes(entry[0x34..0x38].try_into().unwrap()) as usize;
    if flags & FLAG_CONTIGUOUS == 0 {
        return Err(format_err(format!(
            "{label}: non-contiguous STFS file '{name}' is not supported"
        )));
    }
    if n_blocks * BLOCK < size {
        return Err(format_err(format!(
            "{label}: '{name}' size {size:#x} exceeds its {n_blocks} blocks"
        )));
    }
    // Never preallocate on the corruption-controlled 24-bit block count: a
    // crafted count of 0xFFFFFF used to abort the process on a ~64 GiB
    // allocation. The appended bytes are bounded by the file size anyway
    // (block offsets are strictly increasing), and the loop still refuses
    // at the first block beyond EOF.
    let mut payload = Vec::with_capacity((n_blocks * BLOCK).min(data.len()));
    for i in 0..n_blocks {
        payload.extend_from_slice(block_at(data, start + i, first_table, shift, label)?);
    }
    payload.truncate(size);
    if payload.len() != size {
        return Err(format_err(format!(
            "{label}: '{name}' truncated ({:#x} of {size:#x} B)",
            payload.len()
        )));
    }
    Ok(Container360 {
        data: data.to_vec(),
        name,
        payload,
    })
}

pub fn read_container(path: impl AsRef<Path>) -> Result<Container360> {
    let p = path.as_ref();
    let data = fs::read(p)?;
    parse_container(&data, &p.display().to_string())
}
