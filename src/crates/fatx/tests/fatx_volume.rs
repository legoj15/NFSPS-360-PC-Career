//! FATX volume reader unit tests over synthetic volumes (feature test-util).

#![cfg(feature = "test-util")]

use std::io::{Cursor, Seek, SeekFrom};

use fatx::fatx::{FatxVolume, MEDIA_16, MEDIA_32, SUPERBLOCK_SIZE};
use fatx::test_util::{FatxImageBuilder, content_save_path};

fn profile_a() -> String {
    "E0001A2B3C4D5E6F".to_string() // 16 hex chars, like a real profile id
}

fn save_path(profile: &str, file: &str) -> String {
    content_save_path(profile, "45410822", "00000001", file)
}

#[test]
fn superblock_and_geometry_math_matches_spec() {
    // 4 KiB clusters, a handful of small files: cluster count stays far
    // below 0xFFF0, so the FAT must come out 16-bit and page-rounded.
    let vol = FatxImageBuilder::new()
        .volume_id(0x00C0_FFEE)
        .file(save_path(&profile_a(), "SMALL_1"), vec![1u8; 0x800])
        .build_volume();

    let mut cur = Cursor::new(vol.bytes.clone());
    let volume = FatxVolume::open(&mut cur, 0, vol.bytes.len() as u64).unwrap();
    let sb = volume.superblock();

    assert_eq!(sb.volume_id, 0x00C0_FFEE);
    assert_eq!(sb.sectors_per_cluster, 8);
    assert_eq!(sb.root_cluster, 1);
    assert_eq!(sb.cluster_size, 0x1000);
    assert_eq!(sb.fat_entry_width, 2, "small volume -> 16-bit FAT");

    let cluster_count = (vol.bytes.len() as u64 / sb.cluster_size as u64) as u32;
    let fat_entries = cluster_count + 1;
    let expected_fat = ((fat_entries as u64 * 2 + 0xFFF) / 0x1000) * 0x1000;
    assert_eq!(sb.fat_offset, SUPERBLOCK_SIZE);
    assert_eq!(sb.fat_size, expected_fat);
    assert_eq!(sb.data_offset, SUPERBLOCK_SIZE + expected_fat);
    assert_eq!(vol.fat_size, expected_fat);
    assert_eq!(vol.data_offset, sb.data_offset);

    // FAT[0] is the 16-bit media descriptor; FAT[1] terminates the root.
    assert_eq!(volume.fat_entry(0), Some(MEDIA_16 as u32));
    assert_eq!(volume.fat_entry(1), Some(0xFFFF));
    assert_eq!(vol.max_cluster, sb.max_cluster);
}

#[test]
fn directory_listing_and_path_resolution() {
    let builder = FatxImageBuilder::new()
        .file(save_path(&profile_a(), "FILE_A"), b"hello world".to_vec())
        .file("name.txt", vec![0, 0xFF, 0xFE, b'M', b'M', b'M', b'M']);

    let vol = builder.build_volume();
    let mut cur = Cursor::new(vol.bytes.clone());
    let mut volume = FatxVolume::open(&mut cur, 0, vol.bytes.len() as u64).unwrap();

    let root = volume.list_dir("/").unwrap();
    let names: Vec<&str> = root.iter().map(|e| e.name.as_str()).collect();
    assert!(
        names.contains(&"Content"),
        "root has Content, got {names:?}"
    );
    assert!(
        names.contains(&"name.txt"),
        "root has name.txt, got {names:?}"
    );
    assert!(root.iter().all(|e| !e.deleted));

    let content = volume.list_dir("/Content").unwrap();
    assert_eq!(content.len(), 1);
    assert_eq!(content[0].name, profile_a());
    assert!(content[0].is_directory());

    // Path walking is case-insensitive (console semantics).
    let good = volume
        .lookup(&format!(
            "/content/{}/45410822/00000001/file_a",
            profile_a()
        ))
        .unwrap();
    assert_eq!(good.name, "FILE_A");
    assert!(!good.is_directory());

    let missing = volume.lookup("/Content/NOPE").unwrap_err();
    assert!(matches!(missing, fatx::Error::NotFound(_)), "{missing:?}");
}

#[test]
fn file_read_via_cursor_and_via_tempfile() {
    let payload: Vec<u8> = (0..0x2500u32).map(|i| (i * 7 + 3) as u8).collect(); // spans 3 clusters
    let vol = FatxImageBuilder::new()
        .cluster_size(0x1000)
        .file(save_path(&profile_a(), "SPANS_3"), payload.clone())
        .build_volume();

    // In memory.
    let mut cur = Cursor::new(vol.bytes.clone());
    let mut volume = FatxVolume::open(&mut cur, 0, vol.bytes.len() as u64).unwrap();
    let path = format!("/Content/{}/45410822/00000001/SPANS_3", profile_a());
    assert_eq!(volume.read_file(&path).unwrap(), payload);

    // File-backed.
    let mut tmp = tempfile::tempfile().unwrap();
    std::io::Write::write_all(&mut tmp, &vol.bytes).unwrap();
    tmp.seek(SeekFrom::Start(0)).unwrap();
    let mut volume2 = FatxVolume::open(tmp, 0, vol.bytes.len() as u64).unwrap();
    assert_eq!(volume2.read_file(&path).unwrap(), payload);
}

#[test]
fn fragmented_chain_reads_byte_for_byte() {
    let payload = fatx::test_util::oracle_career_latest(); // 823,296 bytes
    let vol = FatxImageBuilder::new()
        .cluster_size(0x1000)
        .fragment_stride(5)
        .file(save_path(&profile_a(), "CAREER_01_360"), payload.clone())
        .build_volume();

    // The stride must actually have produced a non-contiguous chain.
    let first = vol.first_clusters[&save_path(&profile_a(), "CAREER_01_360")];
    assert!(first >= 2, "root is cluster 1");

    let mut cur = Cursor::new(vol.bytes.clone());
    let mut volume = FatxVolume::open(&mut cur, 0, vol.bytes.len() as u64).unwrap();
    let chain = volume.cluster_chain(first).unwrap();
    assert!(chain.len() > 10, "career save spans many clusters");
    let contiguous = chain.windows(2).all(|w| w[1] == w[0] + 1);
    assert!(!contiguous, "chain is fragmented with stride 5: {chain:?}");

    let readback = volume
        .read_file(&format!(
            "/Content/{}/45410822/00000001/CAREER_01_360",
            profile_a()
        ))
        .unwrap();
    assert_eq!(readback.len(), payload.len());
    assert_eq!(readback, payload, "byte-for-byte round trip");
}

#[test]
fn fat32_width_on_large_volume() {
    // Cluster count >= 0xFFF0 forces 4-byte FAT entries. With the smallest
    // cluster size that is a ~268 MiB volume — one deliberately large
    // synthetic image (the size real USB data partitions have).
    let payload = b"tiny".to_vec();
    let vol = FatxImageBuilder::new()
        .cluster_size(0x1000)
        .min_clusters(0xFFF0)
        .file(save_path(&profile_a(), "TINY"), payload.clone())
        .build_volume();
    assert_eq!(vol.fat_width, 4, "large volume -> 32-bit FAT");
    assert_eq!(vol.max_cluster, 0xFFF0 + 8, "sized to the span exactly");

    let len = vol.bytes.len() as u64;
    let mut cur = Cursor::new(vol.bytes); // ~268 MiB; move, don't clone
    let mut volume = FatxVolume::open(&mut cur, 0, len).unwrap();
    assert_eq!(volume.superblock().fat_entry_width, 4);
    assert_eq!(volume.fat_entry(0), Some(MEDIA_32));
    let got = volume
        .read_file(&format!("/Content/{}/45410822/00000001/TINY", profile_a()))
        .unwrap();
    assert_eq!(got, payload);
}

#[test]
fn deleted_entries_are_flagged_and_excluded() {
    let live = b"live".to_vec();
    let dead = b"dead".to_vec();
    let vol = FatxImageBuilder::new()
        .file(save_path(&profile_a(), "ALIVE_1"), live.clone())
        .deleted_file(save_path(&profile_a(), "GONE_1"), dead.clone())
        .build_volume();

    let mut cur = Cursor::new(vol.bytes.clone());
    let mut volume = FatxVolume::open(&mut cur, 0, vol.bytes.len() as u64).unwrap();
    let dir = format!("/Content/{}/45410822/00000001", profile_a());

    let visible = volume.list_dir(&dir).unwrap();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].name, "ALIVE_1");

    let with_deleted = volume.list_dir_with_deleted(&dir).unwrap();
    assert_eq!(with_deleted.len(), 2);
    let gone = with_deleted.iter().find(|e| e.name == "GONE_1").unwrap();
    assert!(gone.deleted);

    // The bytes are still on disk (chain freed, dirent intact) but the file
    // cannot be read through the normal path anymore.
    assert!(volume.read_file(&format!("{dir}/GONE_1")).is_err());
}

#[test]
fn long_42_char_name_round_trips() {
    let name = format!("CAREER_{}", "X".repeat(35));
    assert_eq!(name.len(), 42);
    let payload = b"long-name-payload".to_vec();
    let vol = FatxImageBuilder::new()
        .file(save_path(&profile_a(), &name), payload.clone())
        .build_volume();

    let mut cur = Cursor::new(vol.bytes.clone());
    let mut volume = FatxVolume::open(&mut cur, 0, vol.bytes.len() as u64).unwrap();
    let dir = format!("/Content/{}/45410822/00000001", profile_a());
    let entries = volume.list_dir(&dir).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, name);
    assert_eq!(volume.read_file(&format!("{dir}/{name}")).unwrap(), payload);
}

#[test]
fn empty_file_reads_as_empty() {
    let vol = FatxImageBuilder::new()
        .file(save_path(&profile_a(), "EMPTY"), Vec::new())
        .build_volume();
    let mut cur = Cursor::new(vol.bytes.clone());
    let mut volume = FatxVolume::open(&mut cur, 0, vol.bytes.len() as u64).unwrap();
    assert_eq!(
        volume
            .read_file(&format!("/Content/{}/45410822/00000001/EMPTY", profile_a()))
            .unwrap(),
        Vec::<u8>::new()
    );
}

#[test]
fn corrupt_chains_are_detected() {
    let payload = vec![0xABu8; 0x3000]; // 3 clusters
    let vol = FatxImageBuilder::new()
        .cluster_size(0x1000)
        .file(save_path(&profile_a(), "CORRUPT_ME"), payload)
        .build_volume();

    // Sabotage: point the first FAT entry of the chain at an absurd cluster.
    let first = *vol.first_clusters.values().next().unwrap();
    let mut bytes = vol.bytes.clone();
    let fat = 0x1000usize + first as usize * 2;
    bytes[fat..fat + 2].copy_from_slice(&0xF00Du16.to_be_bytes());

    let mut cur = Cursor::new(bytes);
    let mut volume = FatxVolume::open(&mut cur, 0, vol.bytes.len() as u64).unwrap();
    let err = volume
        .read_file(&format!(
            "/Content/{}/45410822/00000001/CORRUPT_ME",
            profile_a()
        ))
        .unwrap_err();
    assert!(matches!(err, fatx::Error::CorruptChain { .. }), "{err:?}");

    // Sabotage 2: a self-loop.
    let mut bytes = vol.bytes.clone();
    bytes[fat..fat + 2].copy_from_slice(&(first as u16).to_be_bytes());
    let mut cur = Cursor::new(bytes);
    let mut volume = FatxVolume::open(&mut cur, 0, vol.bytes.len() as u64).unwrap();
    let err = volume
        .read_file(&format!(
            "/Content/{}/45410822/00000001/CORRUPT_ME",
            profile_a()
        ))
        .unwrap_err();
    assert!(matches!(err, fatx::Error::CorruptChain { .. }), "{err:?}");
}

#[test]
fn invalid_superblocks_are_rejected() {
    let err = match FatxVolume::open(Cursor::new(vec![0u8; 0x2000]), 0, 0x2000) {
        Err(e) => e,
        Ok(_) => panic!("zero superblock must be rejected"),
    };
    assert!(matches!(err, fatx::Error::BadSuperblock { .. }), "{err:?}");

    // 'FATX' magic (original Xbox, little-endian variant) must be rejected:
    // this crate only supports the 360 'XTAF' variant.
    let mut bytes = vec![0u8; 0x4000];
    bytes[..4].copy_from_slice(b"FATX");
    let err = match FatxVolume::open(Cursor::new(bytes), 0, 0x4000) {
        Err(e) => e,
        Ok(_) => panic!("FATX (original Xbox) magic must be rejected"),
    };
    assert!(matches!(err, fatx::Error::BadSuperblock { .. }), "{err:?}");
}
