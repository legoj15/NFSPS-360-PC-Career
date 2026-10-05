//! FATX ("XTAF") volume reader for the Xbox 360 variant of the filesystem.
//!
//! Layout summary (see `SPEC.md` in this crate for sources):
//!
//! ```text
//! partition start
//!   0x0000  0x1000  superblock region (magic 'XTAF', big-endian fields)
//!   0x1000         FAT ("chainmap"), 2- or 4-byte big-endian entries,
//!                  size rounded up to a 0x1000 page boundary
//!   0x1000 + FAT   data area; cluster N at data + (N-1) * cluster_size
//! ```
//!
//! Cluster numbering starts at 1; FAT[0] holds the media descriptor
//! (`0xFFFFFFF8` / `0xFFF8`), chains terminate with values `>= 0xFFFFFFF0`
//! (`>= 0xFFF0` for 16-bit FATs).

use std::io::{Read, Seek, SeekFrom};

use crate::error::{Error, Result};

/// Magic bytes at the start of every Xbox 360 FATX partition.
pub const MAGIC: [u8; 4] = *b"XTAF";
/// Size of one directory entry in bytes.
pub const DIRENT_SIZE: usize = 0x40;
/// Maximum file-name length in bytes.
pub const MAX_FILE_NAME: usize = 42;
/// Attribute bit marking a directory.
pub const ATTR_DIRECTORY: u8 = 0x10;
/// Attribute bit marking an archive file.
pub const ATTR_ARCHIVE: u8 = 0x20;
/// Name-length byte value marking a deleted entry.
pub const DELETED: u8 = 0xE5;
/// Name-length byte values marking an unused slot (end of used entries).
pub const NEVER_USED: [u8; 2] = [0x00, 0xFF];
/// Size of the superblock region at the start of a partition.
pub const SUPERBLOCK_SIZE: u64 = 0x1000;
/// Sector size implied by the format.
pub const SECTOR_SIZE: u32 = 0x200;
/// Cluster count at or above which the FAT uses 4-byte entries.
pub const FAT16_MAX_CLUSTERS: u32 = 0xFFF0;
/// 32-bit FAT values at or above this terminate a chain.
pub const CHAIN_END_32: u32 = 0xFFFF_FFF0;
/// 16-bit FAT values at or above this terminate a chain.
pub const CHAIN_END_16: u16 = 0xFFF0;
/// Canonical last-cluster value written at the end of a 32-bit chain.
pub const LAST_CLUSTER_32: u32 = 0xFFFF_FFFF;
/// Canonical last-cluster value written at the end of a 16-bit chain.
pub const LAST_CLUSTER_16: u16 = 0xFFFF;
/// FAT[0] media descriptor on a 32-bit FAT.
pub const MEDIA_32: u32 = 0xFFFF_FFF8;
/// FAT[0] media descriptor on a 16-bit FAT.
pub const MEDIA_16: u16 = 0xFFF8;

/// Parsed superblock plus the derived volume geometry.
#[derive(Debug, Clone)]
pub struct Superblock {
    /// Random volume/partition id from the superblock.
    pub volume_id: u32,
    /// Sectors per cluster straight from the superblock.
    pub sectors_per_cluster: u32,
    /// Root directory first cluster straight from the superblock (usually 1).
    pub root_cluster: u32,
    /// Bytes per cluster (`sectors_per_cluster * 0x200`).
    pub cluster_size: u32,
    /// FAT entry width in bytes (2 or 4).
    pub fat_entry_width: usize,
    /// Number of entries the FAT covers (cluster 0 ..= cluster count).
    pub fat_entries: u32,
    /// Highest valid data cluster number.
    pub max_cluster: u32,
    /// Partition-relative offset of the FAT (always 0x1000).
    pub fat_offset: u64,
    /// FAT size in bytes including page rounding.
    pub fat_size: u64,
    /// Partition-relative offset of the data area.
    pub data_offset: u64,
}

/// One 0x40-byte directory entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// File or directory name (deleted entries keep a best-effort name).
    pub name: String,
    /// Raw attribute byte (`ATTR_DIRECTORY`, `ATTR_ARCHIVE`, ...).
    pub attributes: u8,
    /// First cluster of the file (0 for empty files).
    pub first_cluster: u32,
    /// Declared size in bytes.
    pub size: u32,
    /// True when the name-length byte is [`DELETED`].
    pub deleted: bool,
}

impl DirEntry {
    /// Whether the entry has the directory attribute bit set.
    pub fn is_directory(&self) -> bool {
        self.attributes & ATTR_DIRECTORY != 0
    }
}

/// A mounted FATX volume over any [`Read`] + [`Seek`] source.
///
/// The source may be an entire raw drive image; `partition_offset` /
/// `partition_length` then select one FATX partition (see
/// [`crate::partition`]). For a bare partition image pass `0` and the
/// image length.
pub struct FatxVolume<R> {
    src: R,
    base: u64,
    length: u64,
    sb: Superblock,
    fat: Vec<u8>,
}

impl<R: Read + Seek> FatxVolume<R> {
    /// Reads and validates the superblock and the FAT.
    pub fn open(src: R, partition_offset: u64, partition_length: u64) -> Result<Self> {
        let mut vol = Self {
            src,
            base: partition_offset,
            length: partition_length,
            sb: Superblock {
                volume_id: 0,
                sectors_per_cluster: 0,
                root_cluster: 0,
                cluster_size: 0,
                fat_entry_width: 0,
                fat_entries: 0,
                max_cluster: 0,
                fat_offset: SUPERBLOCK_SIZE,
                fat_size: 0,
                data_offset: 0,
            },
            fat: Vec::new(),
        };
        vol.sb = vol.parse_superblock()?;
        vol.fat = vol.read_fat()?;
        vol.validate_media_entry()?;
        Ok(vol)
    }

    /// The parsed volume geometry.
    pub fn superblock(&self) -> &Superblock {
        &self.sb
    }

    /// The partition length this volume was mounted with.
    pub fn partition_length(&self) -> u64 {
        self.length
    }

    /// Raw FAT entry for `cluster` (big-endian decoded), for diagnostics.
    pub fn fat_entry(&self, cluster: u32) -> Option<u32> {
        if cluster as u64 >= self.sb.fat_entries as u64 {
            return None;
        }
        let idx = cluster as usize * self.sb.fat_entry_width;
        Some(match self.sb.fat_entry_width {
            2 => u16::from_be_bytes([self.fat[idx], self.fat[idx + 1]]) as u32,
            _ => u32::from_be_bytes(
                self.fat[idx..idx + 4].try_into().expect("4-byte slice"),
            ),
        })
    }

    /// Follows the FAT chain starting at `first`.
    pub fn cluster_chain(&self, first: u32) -> Result<Vec<u32>> {
        if first == 0 {
            return Ok(Vec::new());
        }
        let mut chain = Vec::new();
        let mut seen = vec![false; self.sb.fat_entries as usize];
        let mut cur = first;
        loop {
            if !(1..=self.sb.max_cluster).contains(&cur) {
                return Err(Error::CorruptChain {
                    cluster: cur,
                    reason: format!(
                        "cluster out of range (max {})",
                        self.sb.max_cluster
                    ),
                });
            }
            if seen.get(cur as usize).copied().unwrap_or(true) {
                return Err(Error::CorruptChain {
                    cluster: cur,
                    reason: "cluster chain loops".into(),
                });
            }
            seen[cur as usize] = true;
            chain.push(cur);

            let next = self.fat_entry(cur).ok_or(Error::CorruptChain {
                cluster: cur,
                reason: "cluster beyond FAT".into(),
            })?;
            let terminal = match self.sb.fat_entry_width {
                2 => next >= CHAIN_END_16 as u32,
                _ => next >= CHAIN_END_32,
            };
            if terminal {
                return Ok(chain);
            }
            if next == 0 {
                return Err(Error::CorruptChain {
                    cluster: cur,
                    reason: "chain runs into a free cluster".into(),
                });
            }
            cur = next;
        }
    }

    /// Lists live (non-deleted) entries of the directory at `path`
    /// (`"/"` or `""` is the root).
    pub fn list_dir(&mut self, path: &str) -> Result<Vec<DirEntry>> {
        let cluster = self.resolve_dir(path)?;
        Ok(self.read_directory(cluster)?.into_iter().filter(|e| !e.deleted).collect())
    }

    /// Like [`FatxVolume::list_dir`] but also returns deleted entries,
    /// flagged with `deleted: true` (forensics / tests).
    pub fn list_dir_with_deleted(&mut self, path: &str) -> Result<Vec<DirEntry>> {
        let cluster = self.resolve_dir(path)?;
        self.read_directory(cluster)
    }

    /// Resolves `path` to a single live entry.
    pub fn lookup(&mut self, path: &str) -> Result<DirEntry> {
        let (parent, name) = split_last_component(path)?;
        if name.is_empty() {
            return Err(Error::NotFound(path.to_string()));
        }
        let cluster = self.resolve_dir(parent)?;
        let entries = self.read_directory(cluster)?;
        entries
            .into_iter()
            .find(|e| !e.deleted && e.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| Error::NotFound(path.to_string()))
    }

    /// Reads the full contents of the file at `path`.
    pub fn read_file(&mut self, path: &str) -> Result<Vec<u8>> {
        let entry = self.lookup(path)?;
        if entry.is_directory() {
            return Err(Error::NotAFile(path.to_string()));
        }
        self.read_entry(&entry)
    }

    /// Reads the full contents of an already-resolved entry.
    pub fn read_entry(&mut self, entry: &DirEntry) -> Result<Vec<u8>> {
        if entry.size == 0 {
            return Ok(Vec::new());
        }
        self.read_entry_partial(entry, usize::MAX)
    }

    /// Reads the first `max_bytes` of an already-resolved entry (or fewer
    /// when the file is shorter). Cheaper than [`FatxVolume::read_entry`]
    /// when only a prefix is needed (e.g. CON headers).
    ///
    /// Truncation is always measured against the file's declared size: a
    /// chain that cannot cover `entry.size` bytes is an error even when
    /// `max_bytes` is smaller.
    pub fn read_entry_partial(&mut self, entry: &DirEntry, max_bytes: usize) -> Result<Vec<u8>> {
        let want = (entry.size as usize).min(max_bytes);
        if want == 0 {
            return Ok(Vec::new());
        }
        let chain = self.cluster_chain(entry.first_cluster)?;
        let cluster_size = self.sb.cluster_size as u64;
        let provided = chain.len() as u64 * cluster_size;
        if provided < entry.size as u64 {
            return Err(Error::Truncated {
                wanted: entry.size as u64,
                got: provided,
            });
        }

        let mut out = Vec::with_capacity(want);
        let mut remaining = want as u64;
        for cluster in chain {
            let take = remaining.min(cluster_size) as usize;
            let start = out.len();
            out.resize(start + take, 0);
            self.read_exact_at(self.cluster_offset(cluster), &mut out[start..start + take])?;
            remaining -= take as u64;
            if remaining == 0 {
                break;
            }
        }
        Ok(out)
    }

    /// Consumes the volume and returns the underlying source.
    pub fn into_inner(self) -> R {
        self.src
    }

    // ---- internals ----

    fn parse_superblock(&mut self) -> Result<Superblock> {
        let base = self.base;
        let bad = move |reason: &str| Error::BadSuperblock {
            offset: base,
            reason: reason.to_string(),
        };

        if self.length < SUPERBLOCK_SIZE {
            return Err(bad("partition shorter than the superblock region"));
        }
        let mut header = [0u8; SUPERBLOCK_SIZE as usize];
        self.read_exact_at(0, &mut header)?;

        if header[..4] != MAGIC {
            return Err(bad(&format!(
                "bad magic {:02X?} (this crate only supports the 360 'XTAF' variant)",
                &header[..4]
            )));
        }
        let volume_id = u32::from_be_bytes(header[4..8].try_into().expect("4 bytes"));
        let sectors_per_cluster =
            u32::from_be_bytes(header[8..12].try_into().expect("4 bytes"));
        let root_cluster =
            u32::from_be_bytes(header[12..16].try_into().expect("4 bytes"));

        if !sectors_per_cluster.is_power_of_two() || !(2..=0x80).contains(&sectors_per_cluster)
        {
            return Err(bad(&format!(
                "implausible sectors-per-cluster {sectors_per_cluster}"
            )));
        }
        let cluster_size = sectors_per_cluster * SECTOR_SIZE;
        if cluster_size < 0x1000 {
            return Err(bad(&format!(
                "cluster size {cluster_size:#x} below the 4 KiB minimum"
            )));
        }

        let cluster_count = (self.length / cluster_size as u64) as u32;
        let fat_entry_width = if cluster_count >= FAT16_MAX_CLUSTERS { 4 } else { 2 };
        let fat_entries = cluster_count.saturating_add(1);
        let fat_size = (fat_entries as u64 * fat_entry_width as u64).div_ceil(0x1000) * 0x1000;
        let data_offset = SUPERBLOCK_SIZE + fat_size;
        if self.length <= data_offset {
            return Err(bad("partition too small for header + FAT"));
        }
        let max_cluster = ((self.length - data_offset) / cluster_size as u64) as u32;
        if !(1..=max_cluster).contains(&root_cluster) {
            return Err(bad(&format!(
                "root directory cluster {root_cluster} out of range (max {max_cluster})"
            )));
        }

        Ok(Superblock {
            volume_id,
            sectors_per_cluster,
            root_cluster,
            cluster_size,
            fat_entry_width,
            fat_entries,
            max_cluster,
            fat_offset: SUPERBLOCK_SIZE,
            fat_size,
            data_offset,
        })
    }

    fn read_fat(&mut self) -> Result<Vec<u8>> {
        let mut fat = vec![0u8; self.sb.fat_size as usize];
        self.read_exact_at(self.sb.fat_offset, &mut fat)?;
        Ok(fat)
    }

    fn validate_media_entry(&self) -> Result<()> {
        let expected = match self.sb.fat_entry_width {
            2 => MEDIA_16 as u32,
            _ => MEDIA_32,
        };
        if self.fat_entry(0) != Some(expected) {
            return Err(Error::BadSuperblock {
                offset: self.base,
                reason: format!(
                    "FAT[0] media descriptor is {:08X}, expected {expected:08X}",
                    self.fat_entry(0).unwrap_or(0)
                ),
            });
        }
        Ok(())
    }

    fn read_exact_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        let absolute = self.base + offset;
        if absolute >= self.base + self.length {
            return Err(Error::ShortRead {
                offset: absolute,
                wanted: buf.len(),
                got: 0,
            });
        }
        self.src.seek(SeekFrom::Start(absolute))?;
        let mut filled = 0;
        while filled < buf.len() {
            match self.src.read(&mut buf[filled..])? {
                0 => {
                    return Err(Error::ShortRead {
                        offset: absolute + filled as u64,
                        wanted: buf.len(),
                        got: filled,
                    });
                }
                n => filled += n,
            }
        }
        Ok(())
    }

    fn cluster_offset(&self, cluster: u32) -> u64 {
        self.sb.data_offset + (cluster as u64 - 1) * self.sb.cluster_size as u64
    }

    /// Resolves a path to a directory's first cluster (root = superblock's).
    fn resolve_dir(&mut self, path: &str) -> Result<u32> {
        if path.is_empty() || path == "/" {
            return Ok(self.sb.root_cluster);
        }
        let entry = self.lookup(path)?;
        if !entry.is_directory() {
            return Err(Error::NotAFile(format!("{path} is not a directory")));
        }
        Ok(entry.first_cluster)
    }

    /// Reads and parses every dirent in the chain behind `first_cluster`.
    fn read_directory(&mut self, first_cluster: u32) -> Result<Vec<DirEntry>> {
        let chain = self.cluster_chain(first_cluster)?;
        let dirents_per_cluster = self.sb.cluster_size as usize / DIRENT_SIZE;
        let mut entries = Vec::new();
        'clusters: for cluster in chain {
            let mut data = vec![0u8; self.sb.cluster_size as usize];
            self.read_exact_at(self.cluster_offset(cluster), &mut data)?;
            for i in 0..dirents_per_cluster {
                let raw = &data[i * DIRENT_SIZE..(i + 1) * DIRENT_SIZE];
                let name_len = raw[0];
                if NEVER_USED.contains(&name_len) {
                    // Slot never used: everything after it is empty too.
                    break 'clusters;
                }
                entries.push(parse_dirent(raw)?);
            }
        }
        Ok(entries)
    }
}

/// Parses one raw 0x40-byte entry.
fn parse_dirent(raw: &[u8]) -> Result<DirEntry> {
    debug_assert_eq!(raw.len(), DIRENT_SIZE);
    let name_len = raw[0];
    let attributes = raw[1];
    let deleted = name_len == DELETED;

    let name_bytes = &raw[2..2 + MAX_FILE_NAME];
    let name_end = if deleted {
        // Deleted entries keep the original name; XTAF tools recover it from
        // the 0xFF padding. Anything printable up to the first 0x00/0xFF.
        name_bytes
            .iter()
            .position(|&b| b == 0x00 || b == 0xFF)
            .unwrap_or(MAX_FILE_NAME)
    } else {
        (name_len as usize).min(MAX_FILE_NAME)
    };
    let name = String::from_utf8_lossy(&name_bytes[..name_end]).into_owned();

    let first_cluster =
        u32::from_be_bytes(raw[0x2C..0x30].try_into().expect("4 bytes"));
    let size = u32::from_be_bytes(raw[0x30..0x34].try_into().expect("4 bytes"));

    if !deleted && !name.bytes().all(|b| b.is_ascii_graphic() || b == b' ') {
        return Err(Error::BadEntry(format!(
            "entry name {name:?} contains non-printable bytes"
        )));
    }

    Ok(DirEntry {
        name,
        attributes,
        first_cluster,
        size,
        deleted,
    })
}

/// Splits `/a/b/c` into (`/a/b`, `c`).
fn split_last_component(path: &str) -> Result<(&str, &str)> {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        return Ok(("", ""));
    }
    match trimmed.rfind('/') {
        Some(pos) => Ok((&trimmed[..pos], &trimmed[pos + 1..])),
        None => Ok(("", trimmed)),
    }
}
