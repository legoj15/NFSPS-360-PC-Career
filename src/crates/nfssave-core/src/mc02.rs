//! MC02 save file format (shared container by PC and Xbox 360, different
//! endianness).
//!
//! Layout (0x1C-byte header, then two blobs):
//!
//! ```text
//!   0x00  u32  magic 0x4D433032 ('MC02')
//!   0x04  u32  total file size (== extra_size + tree_size + 0x1C)
//!   0x08  u32  extra blob size (career 28, alias 64)
//!   0x0C  u32  tree blob size (career 0xB6800, alias 0x5000)
//!   0x10  u32  CRC(extra blob)
//!   0x14  u32  CRC(tree blob)
//!   0x18  u32  CRC(header bytes 0x00..0x18)
//!   0x1C  extra blob  (fixed preamble: identity hash, used-tree size, version
//!         ints 1/7/0/100, floats; alias additionally the player name string)
//!   0x1C+extra_size  tree blob (chunk records; big-endian on 360, little-endian on PC)
//! ```
//!
//! All multi-byte fields native to the platform: big-endian on 360,
//! little-endian on PC. The CRC (see [`crate::crc`]) runs over raw bytes, so
//! the same code validates both.

use crate::crc::crc32_ea;
use crate::{format_err, Result};

pub const MAGIC: u32 = 0x4D43_3032;
pub const HEADER_SIZE: usize = 0x1C;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Endian {
    Big,
    Little,
}

impl Endian {
    fn rd(self, data: &[u8], off: usize) -> u32 {
        let b: [u8; 4] = data[off..off + 4].try_into().unwrap();
        match self {
            Endian::Big => u32::from_be_bytes(b),
            Endian::Little => u32::from_le_bytes(b),
        }
    }

    fn put(self, out: &mut Vec<u8>, v: u32) {
        match self {
            Endian::Big => out.extend_from_slice(&v.to_be_bytes()),
            Endian::Little => out.extend_from_slice(&v.to_le_bytes()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct MC02 {
    pub endian: Endian,
    pub extra: Vec<u8>,
    pub tree: Vec<u8>,
    /// declared (buffer) size; tree may be shorter
    pub tree_size: u32,
    /// total size as declared by a parsed file (0 for freshly built ones)
    pub total: u32,
    /// CRC triple read at parse time (`_stored` in the Python)
    stored: Option<(u32, u32, u32)>,
}

impl MC02 {
    pub fn new(endian: Endian, extra: Vec<u8>, tree: Vec<u8>, tree_size: u32) -> MC02 {
        MC02 {
            endian,
            extra,
            tree,
            tree_size,
            total: 0,
            stored: None,
        }
    }

    pub fn parse(data: &[u8]) -> Result<MC02> {
        if data.len() < HEADER_SIZE {
            return Err(format_err("MC02: truncated header"));
        }
        let magic_le = u32::from_le_bytes(data[0..4].try_into().unwrap());
        let endian = if magic_le == MAGIC {
            Endian::Little
        } else if u32::from_be_bytes(data[0..4].try_into().unwrap()) == MAGIC {
            Endian::Big
        } else {
            return Err(format_err(format!("MC02: bad magic {magic_le:#x}")));
        };
        let total = endian.rd(data, 0x04);
        let extra_size = endian.rd(data, 0x08);
        let tree_size = endian.rd(data, 0x0C);
        let crc_extra = endian.rd(data, 0x10);
        let crc_tree = endian.rd(data, 0x14);
        let crc_hdr = endian.rd(data, 0x18);
        if total as usize != data.len() {
            return Err(format_err(format!(
                "MC02: size field {total:#x} != file size {:#x}",
                data.len()
            )));
        }
        // Python slice semantics: clamp instead of failing on short blobs
        let extra_end = (HEADER_SIZE + extra_size as usize).min(data.len());
        let extra = data[HEADER_SIZE..extra_end].to_vec();
        let tree = data[extra_end..].to_vec();
        Ok(MC02 {
            endian,
            extra,
            tree,
            tree_size,
            total,
            stored: Some((crc_extra, crc_tree, crc_hdr)),
        })
    }

    /// Return a list of validation problems (empty == valid).
    pub fn check(&self) -> Vec<String> {
        let mut probs = Vec::new();
        let Some((crc_extra, crc_tree, crc_hdr)) = self.stored else {
            return probs;
        };
        if crc_hdr != crc32_ea(&self.header_bytes()[..0x18]) {
            probs.push("header CRC mismatch".to_string());
        }
        if crc_extra != crc32_ea(&self.extra) {
            probs.push("extra CRC mismatch".to_string());
        }
        if crc_tree != crc32_ea(&self.tree) {
            probs.push("tree CRC mismatch".to_string());
        }
        probs
    }

    pub fn header_bytes(&self) -> Vec<u8> {
        let mut tree = self.tree.clone();
        if tree.len() < self.tree_size as usize {
            tree.resize(self.tree_size as usize, 0);
        }
        let mut hdr: Vec<u8> = Vec::with_capacity(HEADER_SIZE);
        self.endian.put(&mut hdr, MAGIC);
        self.endian
            .put(&mut hdr, (HEADER_SIZE + self.extra.len() + tree.len()) as u32);
        self.endian.put(&mut hdr, self.extra.len() as u32);
        self.endian.put(&mut hdr, tree.len() as u32);
        self.endian.put(&mut hdr, crc32_ea(&self.extra));
        self.endian.put(&mut hdr, crc32_ea(&tree));
        self.endian.put(&mut hdr, 0); // header CRC patched below
        let crc = crc32_ea(&hdr[..0x18]);
        let patched = match self.endian {
            Endian::Big => crc.to_be_bytes(),
            Endian::Little => crc.to_le_bytes(),
        };
        hdr[0x18..0x1C].copy_from_slice(&patched);
        hdr
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        if self.tree.len() > self.tree_size as usize {
            return Err(format_err(format!(
                "tree data ({:#x} B) exceeds declared buffer {:#x} - refusing to truncate",
                self.tree.len(),
                self.tree_size
            )));
        }
        let mut tree = self.tree.clone();
        tree.resize(self.tree_size as usize, 0);
        let mut out = self.header_bytes();
        out.extend_from_slice(&self.extra);
        out.extend_from_slice(&tree);
        Ok(out)
    }

    // -- extra blob accessors ----------------------------------------------

    pub fn extra_field(&self, index: usize) -> Option<u32> {
        let off = index * 4;
        let b: [u8; 4] = self.extra.get(off..off + 4)?.try_into().ok()?;
        Some(match self.endian {
            Endian::Big => u32::from_be_bytes(b),
            Endian::Little => u32::from_le_bytes(b),
        })
    }

    /// Returns false (and changes nothing) when the index is out of range;
    /// the Python `struct.pack_into` raises there.
    pub fn set_extra_field(&mut self, index: usize, value: u32) -> bool {
        let off = index * 4;
        if off + 4 > self.extra.len() {
            return false;
        }
        let bytes = match self.endian {
            Endian::Big => value.to_be_bytes(),
            Endian::Little => value.to_le_bytes(),
        };
        self.extra[off..off + 4].copy_from_slice(&bytes);
        true
    }

    /// Used tree size as declared in the extra blob (word 1).
    pub fn used_tree_size(&self) -> Option<u32> {
        self.extra_field(1)
    }
}
