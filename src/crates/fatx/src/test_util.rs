//! Hardware-free test support (feature `test-util`).
//!
//! Synthesizes complete Xbox 360 USB images — sector-0 signature, retail
//! fixed offsets (or a devkit partition table), and a real FATX/XTAF volume
//! with configurable cluster size, fragmented chains, deleted entries and
//! long file names — from the bytes of the tracked oracle saves under
//! `docs/re/` when asked for real CON content.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::fatx::{
    ATTR_ARCHIVE, ATTR_DIRECTORY, DELETED, FAT16_MAX_CLUSTERS, LAST_CLUSTER_16, LAST_CLUSTER_32,
    MAGIC, MEDIA_16, MEDIA_32, SECTOR_SIZE, SUPERBLOCK_SIZE,
};
use crate::partition::{DEVKIT_MAGIC, RETAIL_USB_DATA_OFFSET, USB_SIGNATURE};

/// Result of [`FatxImageBuilder::build_volume`]: a standalone FATX
/// partition image (starts directly at the superblock).
#[derive(Debug, Clone)]
pub struct VolumeImage {
    /// The partition bytes (superblock at offset 0).
    pub bytes: Vec<u8>,
    /// Bytes per cluster used by the volume.
    pub cluster_size: u32,
    /// FAT entry width in bytes (2 or 4) chosen from the cluster count.
    pub fat_width: usize,
    /// FAT size in bytes (page-rounded), for assertions.
    pub fat_size: u64,
    /// Data-area offset within the partition, for assertions.
    pub data_offset: u64,
    /// Highest valid cluster number.
    pub max_cluster: u32,
    /// Map of file path (e.g. `Content/E.../45410822/00000001/CAREER_01_360`)
    /// to the first cluster of its chain.
    pub first_clusters: BTreeMap<String, u32>,
}

/// A complete raw USB image: FATX Data partition placed at the retail
/// offset 0x20000000, optional `MICROSOFT*XBOX360` signature at 0x1FF.
#[derive(Debug, Clone)]
pub struct SyntheticUsbImage {
    /// Raw image bytes.
    pub image: Vec<u8>,
    /// Byte offset of the FATX Data partition (0x20000000).
    pub data_offset: u64,
    /// Length of the FATX Data partition.
    pub data_length: u64,
    /// Bytes per cluster used by the volume.
    pub cluster_size: u32,
    /// FAT entry width in bytes.
    pub fat_width: usize,
    /// Map of file path to first cluster.
    pub first_clusters: BTreeMap<String, u32>,
}

/// A devkit-HDD-style image: little-endian partition table at LBA 0 and the
/// FATX volume placed at a chosen LBA.
#[derive(Debug, Clone)]
pub struct SyntheticDevkitImage {
    /// Raw image bytes.
    pub image: Vec<u8>,
    /// Byte offset of the FATX Data partition.
    pub data_offset: u64,
    /// Length of the FATX Data partition.
    pub data_length: u64,
}

#[derive(Debug, Clone)]
struct Planned {
    path: String,
    bytes: Vec<u8>,
    deleted: bool,
}

#[derive(Debug, Clone)]
struct DirentSpec {
    name: String,
    deleted: bool,
    directory: bool,
    first_cluster: u32,
    size: u32,
}

/// Builder for synthetic FATX volumes and whole-drive images.
///
/// The geometry math mirrors [`crate::fatx`] exactly (FAT entry count,
/// page-rounded FAT size, 1-based clusters) so builder and reader can never
/// disagree about where a cluster lives.
#[derive(Debug, Clone)]
pub struct FatxImageBuilder {
    sectors_per_cluster: u32,
    volume_id: u32,
    fragment_stride: u32,
    with_signature: bool,
    min_clusters: u32,
    files: Vec<Planned>,
}

impl Default for FatxImageBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl FatxImageBuilder {
    /// New builder with 4 KiB clusters (8 sectors), deterministic volume id.
    pub fn new() -> Self {
        Self {
            sectors_per_cluster: 8,
            volume_id: 0x3600_0001,
            fragment_stride: 1,
            with_signature: true,
            min_clusters: 0,
            files: Vec::new(),
        }
    }

    /// Sets the cluster size in bytes (must be a power of two in
    /// 0x1000..=0x10000, matching real 360 cluster sizes).
    pub fn cluster_size(mut self, bytes: u32) -> Self {
        self.sectors_per_cluster = bytes / SECTOR_SIZE;
        self
    }

    /// Overrides the superblock volume id.
    pub fn volume_id(mut self, id: u32) -> Self {
        self.volume_id = id;
        self
    }

    /// Spreads each file's clusters `stride` apart (2 = every other cluster),
    /// producing deliberately fragmented chains. Default 1 (contiguous).
    pub fn fragment_stride(mut self, stride: u32) -> Self {
        self.fragment_stride = stride.max(1);
        self
    }

    /// Omits the `MICROSOFT*XBOX360` sector-0 signature from
    /// [`FatxImageBuilder::build_usb_image`] output.
    pub fn no_signature(mut self) -> Self {
        self.with_signature = false;
        self
    }

    /// Forces the volume to span at least `clusters` clusters (e.g.
    /// `>= 0xFFF0` to exercise the 32-bit FAT path).
    pub fn min_clusters(mut self, clusters: u32) -> Self {
        self.min_clusters = clusters;
        self
    }

    /// Adds a file at a volume-relative path with `/` separators.
    /// Directories along the path are created implicitly.
    pub fn file(mut self, path: impl Into<String>, bytes: Vec<u8>) -> Self {
        self.files.push(Planned {
            path: path.into(),
            bytes,
            deleted: false,
        });
        self
    }

    /// Adds a file whose dirent is marked deleted (name-length byte 0xE5)
    /// and whose FAT chain is freed, while the bytes stay on disk.
    pub fn deleted_file(mut self, path: impl Into<String>, bytes: Vec<u8>) -> Self {
        self.files.push(Planned {
            path: path.into(),
            bytes,
            deleted: true,
        });
        self
    }

    /// Builds a standalone FATX partition image (superblock at offset 0).
    pub fn build_volume(&self) -> VolumeImage {
        let cluster_size = self.sectors_per_cluster * SECTOR_SIZE;
        assert!(
            cluster_size.is_power_of_two() && (0x1000..=0x10000).contains(&cluster_size),
            "cluster size must be a power of two in 0x1000..=0x10000, got {cluster_size:#x}"
        );
        assert!(
            self.files
                .iter()
                .all(|f| { !f.path.starts_with('/') && !f.path.split('/').any(|c| c.is_empty()) }),
            "paths must be volume-relative without empty components"
        );
        for f in &self.files {
            assert!(
                f.path.len() <= 240,
                "path {} exceeds the 240-char limit",
                f.path
            );
            assert!(
                f.path.rsplit('/').next().unwrap().len() <= 42,
                "file name in {} exceeds the 42-char limit",
                f.path
            );
        }

        // Directory set = ancestors of every file path; "" is the root.
        let dirs: BTreeSet<String> = self.files.iter().flat_map(|f| ancestors(&f.path)).collect();

        let clusters_for =
            |bytes: &[u8]| -> u32 { bytes.len().div_ceil(cluster_size as usize) as u32 };
        let needed: u32 = dirs.len() as u32
            + self
                .files
                .iter()
                .map(|f| clusters_for(&f.bytes))
                .sum::<u32>();
        let span = needed
            .saturating_mul(self.fragment_stride)
            .max(self.min_clusters)
            + 8;

        // Converge on a volume length whose max cluster comfortably covers
        // the allocation span, mirroring the reader's geometry formulas.
        // Each round grows the volume by the current deficit; the FAT grows
        // sublinearly (4 bytes per added cluster), so this converges fast.
        let mut volume_len = span as u64 * cluster_size as u64;
        let (mut fat_size, mut data_offset, mut max_cluster) = (0u64, 0u64, 0u32);
        for _ in 0..64 {
            let cluster_count = (volume_len / cluster_size as u64) as u32;
            fat_size = (cluster_count.saturating_add(1) as u64
                * fat_entry_width(cluster_count) as u64)
                .div_ceil(0x1000)
                * 0x1000;
            data_offset = SUPERBLOCK_SIZE + fat_size;
            max_cluster = ((volume_len - data_offset) / cluster_size as u64) as u32;
            if max_cluster >= span {
                break;
            }
            let deficit = (span - max_cluster).max(1) as u64;
            volume_len += deficit * cluster_size as u64;
        }
        assert!(max_cluster >= span, "could not size the synthetic volume");
        let fat_width = fat_entry_width((volume_len / cluster_size as u64) as u32);
        let chain_end = if fat_width == 2 {
            LAST_CLUSTER_16 as u32
        } else {
            LAST_CLUSTER_32
        };
        let media = if fat_width == 2 {
            MEDIA_16 as u32
        } else {
            MEDIA_32
        };

        let mut bytes = vec![0u8; volume_len as usize];

        // Superblock: magic, volume id, sectors per cluster, root cluster 1.
        bytes[..4].copy_from_slice(&MAGIC);
        bytes[4..8].copy_from_slice(&self.volume_id.to_be_bytes());
        bytes[8..12].copy_from_slice(&self.sectors_per_cluster.to_be_bytes());
        bytes[12..16].copy_from_slice(&1u32.to_be_bytes());

        // FAT.
        let mut fat = vec![0u8; fat_size as usize];
        let set_fat = |fat: &mut [u8], cluster: u32, value: u32| {
            let idx = cluster as usize * fat_width;
            if fat_width == 2 {
                fat[idx..idx + 2].copy_from_slice(&(value as u16).to_be_bytes());
            } else {
                fat[idx..idx + 4].copy_from_slice(&value.to_be_bytes());
            }
        };
        set_fat(&mut fat, 0, media);
        set_fat(&mut fat, 1, chain_end); // root: single-cluster chain

        // Cluster allocator: monotonic cursor with the configured stride.
        let mut cursor: u32 = 2;
        let mut allocate = |count: u32| -> Vec<u32> {
            if count == 0 {
                return Vec::new();
            }
            let chain: Vec<u32> = (0..count)
                .map(|i| cursor + i * self.fragment_stride)
                .collect();
            let last = *chain.last().expect("count > 0");
            assert!(
                last <= max_cluster,
                "synthetic volume ran out of clusters (need {last}, max {max_cluster})"
            );
            cursor += count * self.fragment_stride;
            chain
        };

        // One cluster per directory (sorted for determinism); root is 1.
        let mut dir_cluster: BTreeMap<String, u32> = BTreeMap::new();
        for dir in &dirs {
            let chain = allocate(1);
            set_fat(&mut fat, chain[0], chain_end); // single-cluster chains
            dir_cluster.insert(dir.clone(), chain[0]);
        }

        // File chains (deleted files keep their dirent cluster but no chain).
        struct Placement {
            planned: Planned,
            first: u32,
            clusters: Vec<u32>,
        }
        let mut placements: Vec<Placement> = Vec::new();
        for planned in self.files.clone() {
            let clusters = allocate(clusters_for(&planned.bytes));
            placements.push(Placement {
                first: clusters.first().copied().unwrap_or(0),
                planned,
                clusters,
            });
        }

        // Directory tables: sub-dirs sorted first, then files in insertion
        // order, 0xFF filler after the last entry (fresh-format style).
        let dirents_per_cluster = cluster_size as usize / 0x40;
        let mut write_dir = |cluster: u32, entries: &[DirentSpec]| {
            assert!(
                entries.len() < dirents_per_cluster,
                "test-util: directory with {} entries exceeds {} slots; raise the cluster size",
                entries.len() + 1,
                dirents_per_cluster
            );
            let base = (data_offset + (cluster as u64 - 1) * cluster_size as u64) as usize;
            bytes[base..base + cluster_size as usize].fill(0xFF);
            for (i, e) in entries.iter().enumerate() {
                let raw = &mut bytes[base + i * 0x40..base + (i + 1) * 0x40];
                raw[0] = if e.deleted {
                    DELETED
                } else {
                    e.name.len() as u8
                };
                raw[1] = if e.directory {
                    ATTR_DIRECTORY
                } else {
                    ATTR_ARCHIVE
                };
                raw[2..2 + e.name.len()].copy_from_slice(e.name.as_bytes());
                raw[0x2C..0x30].copy_from_slice(&e.first_cluster.to_be_bytes());
                raw[0x30..0x34].copy_from_slice(&e.size.to_be_bytes());
            }
        };

        let root = String::new();
        let all_dirs = dirs.iter().chain(std::iter::once(&root));
        for dir in all_dirs {
            let cluster = if dir.is_empty() { 1 } else { dir_cluster[dir] };
            let mut children: Vec<DirentSpec> = Vec::new();
            for sub in dirs.iter().filter(|d| parent_of(d) == *dir) {
                children.push(DirentSpec {
                    name: file_name_of(sub).to_string(),
                    deleted: false,
                    directory: true,
                    first_cluster: dir_cluster[sub],
                    size: cluster_size, // directories report allocated size
                });
            }
            for p in &placements {
                if parent_of(&p.planned.path) == *dir {
                    children.push(DirentSpec {
                        name: file_name_of(&p.planned.path).to_string(),
                        deleted: p.planned.deleted,
                        directory: false,
                        first_cluster: p.first,
                        size: p.planned.bytes.len() as u32,
                    });
                }
            }
            write_dir(cluster, &children);
        }

        // FAT chains + data placement. Deleted files leave their FAT entries
        // free (console deletion semantics) while the bytes stay on disk.
        for p in &placements {
            if !p.planned.deleted && !p.clusters.is_empty() {
                for pair in p.clusters.windows(2) {
                    set_fat(&mut fat, pair[0], pair[1]);
                }
                set_fat(&mut fat, *p.clusters.last().unwrap(), chain_end);
            }
            let mut remaining = p.planned.bytes.as_slice();
            for cluster in &p.clusters {
                let base = (data_offset + (*cluster as u64 - 1) * cluster_size as u64) as usize;
                let take = remaining.len().min(cluster_size as usize);
                bytes[base..base + take].copy_from_slice(&remaining[..take]);
                remaining = &remaining[take..];
            }
        }

        // Splice the FAT in behind the superblock.
        bytes[SUPERBLOCK_SIZE as usize..SUPERBLOCK_SIZE as usize + fat.len()].copy_from_slice(&fat);

        VolumeImage {
            bytes,
            cluster_size,
            fat_width,
            fat_size,
            data_offset,
            max_cluster,
            first_clusters: placements
                .iter()
                .map(|p| (p.planned.path.clone(), p.first))
                .collect(),
        }
    }

    /// Builds a complete retail-USB-style raw image (signature + fixed
    /// Data-partition offset 0x20000000).
    pub fn build_usb_image(&self) -> SyntheticUsbImage {
        let volume = self.build_volume();
        let total = RETAIL_USB_DATA_OFFSET + volume.bytes.len() as u64;
        let mut image = vec![0u8; total as usize];
        if self.with_signature {
            // Community-lore placement: 17 bytes starting at 0x1FF, spanning
            // the sector-0/sector-1 boundary (see SPEC.md §5.4).
            image[0x1FF..0x1FF + USB_SIGNATURE.len()].copy_from_slice(USB_SIGNATURE);
        }
        image[RETAIL_USB_DATA_OFFSET as usize..].copy_from_slice(&volume.bytes);
        SyntheticUsbImage {
            image,
            data_offset: RETAIL_USB_DATA_OFFSET,
            data_length: volume.bytes.len() as u64,
            cluster_size: volume.cluster_size,
            fat_width: volume.fat_width,
            first_clusters: volume.first_clusters,
        }
    }

    /// Builds a devkit-HDD-style raw image with the volume at `data_lba`.
    ///
    /// Writes both entries of the LBA-0 table like a real devkit drive: the
    /// Content volume at `data_lba`, plus a nominal Dashboard entry right
    /// behind it (no volume is placed there; only the Content entry is
    /// validated by [`crate::partition::XboxDriveImage::probe`]).
    pub fn build_devkit_image(&self, data_lba: u32) -> SyntheticDevkitImage {
        let volume = self.build_volume();
        let data_offset = data_lba as u64 * SECTOR_SIZE as u64;
        let total = data_offset + volume.bytes.len() as u64;
        let mut image = vec![0u8; total as usize];
        let volume_sectors = (volume.bytes.len() / SECTOR_SIZE as usize) as u32;
        image[..4].copy_from_slice(&DEVKIT_MAGIC.to_le_bytes());
        image[0x8..0xC].copy_from_slice(&data_lba.to_le_bytes());
        image[0xC..0x10].copy_from_slice(&volume_sectors.to_le_bytes());
        image[0x10..0x14].copy_from_slice(&(data_lba + volume_sectors).to_le_bytes());
        image[0x14..0x18].copy_from_slice(&0x8000u32.to_le_bytes());
        image[data_offset as usize..].copy_from_slice(&volume.bytes);
        SyntheticDevkitImage {
            image,
            data_offset,
            data_length: volume.bytes.len() as u64,
        }
    }
}

fn fat_entry_width(cluster_count: u32) -> usize {
    if cluster_count >= FAT16_MAX_CLUSTERS {
        4
    } else {
        2
    }
}

/// All ancestor paths of a slash path (the root "" is never included).
fn ancestors(path: &str) -> Vec<String> {
    let parts: Vec<&str> = path.split('/').collect();
    (1..parts.len()).map(|i| parts[..i].join("/")).collect()
}

/// Parent path of a slash path ("" for top-level entries).
fn parent_of(path: &str) -> String {
    match path.rfind('/') {
        Some(pos) => path[..pos].to_string(),
        None => String::new(),
    }
}

/// Final component of a slash path.
fn file_name_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Canonical save path inside a Data partition.
pub fn content_save_path(profile: &str, title: &str, save_type: &str, file: &str) -> String {
    format!("Content/{profile}/{title}/{save_type}/{file}")
}

/// Absolute path of a tracked oracle file under `docs/re/`.
pub fn oracle_path(relative: &str) -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../docs/re")).join(relative)
}

/// Reads the tracked oracle `docs/re/c1_latest/CAREER_01_360`.
pub fn oracle_career_latest() -> Vec<u8> {
    std::fs::read(oracle_path("c1_latest/CAREER_01_360"))
        .expect("tracked oracle docs/re/c1_latest/CAREER_01_360 must exist")
}

/// Reads the tracked oracle `docs/re/pair/CAREER_02_360_fresh`.
pub fn oracle_career_fresh() -> Vec<u8> {
    std::fs::read(oracle_path("pair/CAREER_02_360_fresh"))
        .expect("tracked oracle docs/re/pair/CAREER_02_360_fresh must exist")
}

/// Returns a copy of `bytes` with the CON title ID at offset 0x360 replaced
/// (used to synthesize saves that belong to other titles).
pub fn with_title_id(mut bytes: Vec<u8>, id: [u8; 4]) -> Vec<u8> {
    bytes[0x360..0x364].copy_from_slice(&id);
    bytes
}
