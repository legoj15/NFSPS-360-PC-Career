//! End-to-end discovery round trip on a fully synthetic USB image built
//! from the real tracked oracle bytes (feature test-util).

#![cfg(feature = "test-util")]

use std::io::{Cursor, Seek};

use fatx::discovery::discover_prostreet_saves;
use fatx::partition::XboxDriveImage;
use fatx::stfs::TITLE_ID_NFS_PROSTREET;
use fatx::test_util::{FatxImageBuilder, content_save_path};

const PROFILE_A: &str = "E0001A2B3C4D5E6F";
const PROFILE_B: &str = "E0000FEEDFACEC0D";
const PROFILE_C: &str = "FF00112233445566";
const TITLE: &str = "45410822";
const OTHER_TITLE_DIR: &str = "4D530926"; // some other game's title folder

fn save(profile: &str, save_type: &str, file: &str) -> String {
    content_save_path(profile, TITLE, save_type, file)
}

/// The full scenario image: multiple profiles and save types, a 42-char
/// name, a fragmented chain, a foreign-title save caught by its name,
/// a foreign-title save that must be dropped, and a deleted save.
fn scenario() -> (fatx::test_util::SyntheticUsbImage, Vec<String>) {
    let career = fatx::test_util::oracle_career_latest();
    let career_fresh = fatx::test_util::oracle_career_fresh();
    let other_title = fatx::test_util::with_title_id(
        fatx::test_util::oracle_career_latest(),
        [0x4D, 0x53, 0x09, 0x26],
    );
    let long_name = format!("CAREER_{}", "L".repeat(35));
    assert_eq!(long_name.len(), 42);

    let builder = FatxImageBuilder::new()
        .cluster_size(0x1000)
        .fragment_stride(3)
        // Profile A: the canonical latest save plus a long-name variant.
        .file(save(PROFILE_A, "00000001", "CAREER_01_360"), career.clone())
        .file(save(PROFILE_A, "00000001", &long_name), career.clone())
        // Profile B: second oracle save and an ALIAS save under 00000002.
        .file(
            save(PROFILE_B, "00000001", "CAREER_02_360"),
            career_fresh.clone(),
        )
        .file(save(PROFILE_B, "00000002", "ALIAS_01_360"), career.clone())
        // Foreign title, but the file name forces inclusion.
        .file(
            content_save_path(PROFILE_A, OTHER_TITLE_DIR, "00000001", "CAREER_OTHER"),
            other_title.clone(),
        )
        // Foreign title and foreign name: must be dropped.
        .file(
            content_save_path(PROFILE_C, OTHER_TITLE_DIR, "00000001", "SETTINGS_DAT"),
            other_title.clone(),
        )
        // ProStreet package that is not a save (seen on real media as
        // SHADOW_74GR1): the title ID alone must not pull it in.
        .file(save(PROFILE_C, "00000001", "SHADOW_74GR1"), career.clone())
        // Deleted ProStreet save: on disk but must not be discovered.
        .deleted_file(save(PROFILE_C, "00000001", "CAREER_DELETED"), career);

    let expected = vec![
        save(PROFILE_A, "00000001", "CAREER_01_360"),
        save(PROFILE_A, "00000001", &long_name),
        content_save_path(PROFILE_A, OTHER_TITLE_DIR, "00000001", "CAREER_OTHER"),
        save(PROFILE_B, "00000001", "CAREER_02_360"),
        save(PROFILE_B, "00000002", "ALIAS_01_360"),
    ];

    (builder.build_usb_image(), expected)
}

fn scan(usb: &fatx::test_util::SyntheticUsbImage) -> Vec<fatx::DiscoveredSave> {
    let mut cur = Cursor::new(usb.image.clone());
    let drive = XboxDriveImage::probe(&mut cur, usb.image.len() as u64)
        .expect("synthetic image must probe");
    let mut vol = fatx::FatxVolume::open(
        &mut cur,
        drive.data_partition.offset,
        drive.data_partition.length,
    )
    .expect("synthetic volume must mount");
    discover_prostreet_saves(&mut vol).expect("discovery must succeed")
}

#[test]
fn discovery_finds_exactly_the_embedded_saves() {
    let (usb, expected) = scenario();
    let found = scan(&usb);

    let mut paths: Vec<&str> = found.iter().map(|s| s.source_path.as_str()).collect();
    paths.sort();
    let mut want: Vec<&str> = expected.iter().map(|s| s.as_str()).collect();
    want.sort();
    assert_eq!(
        paths, want,
        "discovered set must match the embedded set exactly"
    );
}

#[test]
fn discovered_bytes_match_oracles_byte_for_byte() {
    let (usb, _) = scenario();
    let found = scan(&usb);
    let career = fatx::test_util::oracle_career_latest();
    let fresh = fatx::test_util::oracle_career_fresh();

    for s in &found {
        let name = s.source_path.rsplit('/').next().unwrap();
        let expected: Vec<u8> = if name == "CAREER_02_360" {
            fresh.clone()
        } else if name == "CAREER_OTHER" {
            // The foreign-title save is the career oracle with the title ID
            // swapped; only that 4-byte field differs.
            fatx::test_util::with_title_id(career.clone(), [0x4D, 0x53, 0x09, 0x26])
        } else {
            career.clone()
        };
        assert_eq!(
            s.bytes, expected,
            "byte-for-byte mismatch for {}",
            s.source_path
        );
        assert_eq!(s.bytes.len(), 823_296, "oracle size for {}", s.source_path);
    }
}

#[test]
fn friendly_names_are_per_save() {
    let (usb, _) = scenario();
    let found = scan(&usb);
    assert!(!found.is_empty());
    for s in &found {
        // The STFS file-table name identifies the save ("CAREER_01"); the
        // CON title name would read "NFS ProStreet" on every row.
        let file = s.source_path.rsplit('/').next().unwrap();
        let want = if file == "CAREER_02_360" {
            "CAREER_02"
        } else {
            "CAREER_01"
        };
        assert_eq!(s.friendly_name, want, "for {}", s.source_path);
    }
}

/// A package too damaged to name falls back to its FATX file name, never
/// the game title name.
#[test]
fn friendly_name_falls_back_to_fatx_file_name() {
    let mut broken = vec![0u8; 0x1800];
    broken[..4].copy_from_slice(b"CON ");
    let usb = FatxImageBuilder::new()
        .cluster_size(0x1000)
        .file(
            save(PROFILE_A, "00000001", "CAREER_01_360"),
            fatx::test_util::oracle_career_latest(),
        )
        .file(save(PROFILE_A, "00000001", "CAREER_BAD"), broken)
        .build_usb_image();
    let found = scan(&usb);
    let by_file = |f: &str| {
        found
            .iter()
            .find(|s| s.source_path.ends_with(f))
            .unwrap_or_else(|| panic!("{f} missing"))
    };
    assert_eq!(by_file("CAREER_01_360").friendly_name, "CAREER_01");
    assert_eq!(by_file("CAREER_BAD").friendly_name, "CAREER_BAD");
}

#[test]
fn deleted_and_foreign_saves_are_not_discovered() {
    let (usb, _) = scenario();
    let found = scan(&usb);
    for s in &found {
        assert!(
            !s.source_path.contains("CAREER_DELETED"),
            "deleted save leaked"
        );
        assert!(
            !s.source_path.contains("SETTINGS_DAT"),
            "foreign save leaked"
        );
    }
}

#[test]
fn round_trip_via_tempfile_backed_file() {
    let (usb, _) = scenario();
    let mut tmp = tempfile::tempfile().unwrap();
    std::io::Write::write_all(&mut tmp, &usb.image).unwrap();
    tmp.seek(std::io::SeekFrom::Start(0)).unwrap();

    let drive = XboxDriveImage::probe(&mut tmp, usb.image.len() as u64).unwrap();
    let mut vol = fatx::FatxVolume::open(
        &mut tmp,
        drive.data_partition.offset,
        drive.data_partition.length,
    )
    .unwrap();
    let found = discover_prostreet_saves(&mut vol).unwrap();
    assert_eq!(found.len(), 5, "same result through a file-backed source");
}

#[test]
fn empty_content_partition_is_not_an_error() {
    let usb = FatxImageBuilder::new().build_usb_image(); // no files at all
    let mut cur = Cursor::new(usb.image.clone());
    let drive = XboxDriveImage::probe(&mut cur, usb.image.len() as u64).unwrap();
    let mut vol = fatx::FatxVolume::open(
        &mut cur,
        drive.data_partition.offset,
        drive.data_partition.length,
    )
    .unwrap();
    assert_eq!(discover_prostreet_saves(&mut vol).unwrap(), Vec::new());
}

#[test]
fn title_id_constant_is_prostreet_45410822() {
    assert_eq!(TITLE_ID_NFS_PROSTREET, [0x45, 0x41, 0x08, 0x22]);
}

/// One corrupt directory entry must not hide the good saves on the same
/// media: the broken file is skipped with a note, the scan keeps going.
#[test]
fn corrupt_file_is_skipped_without_hiding_good_saves() {
    let career = fatx::test_util::oracle_career_latest();
    let good = save(PROFILE_A, "00000001", "CAREER_01_360");
    let broken = save(PROFILE_C, "00000001", "CAREER_BRKN");
    let usb = FatxImageBuilder::new()
        .cluster_size(0x1000)
        .file(good.clone(), career.clone())
        .file(broken.clone(), career)
        .build_usb_image();

    // Sabotage the CAREER_BRKN dirent: declared size far beyond its chain.
    let mut image = usb.image.clone();
    let name_off = image
        .windows(11)
        .position(|w| w == b"CAREER_BRKN".as_slice())
        .expect("dirent name present in the image");
    let dirent = name_off - 2; // the name starts at +2 inside the dirent
    image[dirent + 0x30..dirent + 0x34].copy_from_slice(&0x7F_FF_F0u32.to_be_bytes());

    let mut cur = Cursor::new(image);
    let drive = XboxDriveImage::probe(&mut cur, usb.image.len() as u64).unwrap();
    let mut vol = fatx::FatxVolume::open(
        &mut cur,
        drive.data_partition.offset,
        drive.data_partition.length,
    )
    .unwrap();

    let report = fatx::discovery::discover_prostreet_saves_noted(&mut vol).unwrap();
    assert_eq!(report.saves.len(), 1, "the good save must survive");
    assert_eq!(report.saves[0].source_path, good);
    assert!(
        report.notes.iter().any(|n| n.contains("CAREER_BRKN")),
        "the skipped file must be noted: {:?}",
        report.notes
    );
}

/// An UNREADABLE save-type directory (corrupt cluster chain) is noted at
/// its own level and the scan keeps going — "no saves found" on otherwise
/// healthy media must always carry a diagnostic. A merely absent 00000001/
/// 00000002 folder stays silent (normal for titles with one save type).
#[test]
fn unreadable_save_type_dir_is_noted_and_scan_continues() {
    let usb = FatxImageBuilder::new()
        .cluster_size(0x1000)
        .file(
            save(PROFILE_A, "00000001", "CAREER_DEAD"),
            fatx::test_util::oracle_career_latest(),
        )
        .file(
            save(PROFILE_A, "00000002", "CAREER_02_360"),
            fatx::test_util::oracle_career_fresh(),
        )
        .build_usb_image();

    // Loop the 00000001 directory cluster's FAT entry onto itself. Directory
    // clusters are 0xFF-padded behind the name, so match the raw 8-byte name
    // plus the directory attribute byte in front of it.
    let mut image = usb.image.clone();
    let name_off = image
        .windows(8)
        .enumerate()
        .position(|(i, w)| w == b"00000001".as_slice() && image[i - 1] == 0x10)
        .expect("save-type dirent present");
    let dirent = name_off - 2;
    let cluster = u32::from_be_bytes(
        image[dirent + 0x2C..dirent + 0x30]
            .try_into()
            .expect("4 bytes"),
    );
    // 16-bit FAT on this small volume; offsets are volume-relative, so add
    // the Data-partition base inside the raw image.
    let fat = usb.data_offset as usize + 0x1000 + cluster as usize * 2;
    image[fat..fat + 2].copy_from_slice(&(cluster as u16).to_be_bytes());

    let mut cur = Cursor::new(image);
    let drive = XboxDriveImage::probe(&mut cur, usb.image.len() as u64).unwrap();
    let mut vol = fatx::FatxVolume::open(
        &mut cur,
        drive.data_partition.offset,
        drive.data_partition.length,
    )
    .unwrap();

    let report = fatx::discovery::discover_prostreet_saves_noted(&mut vol).unwrap();
    assert_eq!(
        report.saves.len(),
        1,
        "the good save in the sibling save-type dir must survive"
    );
    assert_eq!(
        report.saves[0].source_path,
        save(PROFILE_A, "00000002", "CAREER_02_360")
    );
    assert!(
        report
            .notes
            .iter()
            .any(|n| n.contains("00000001") && n.contains("skipped")),
        "the unreadable save-type dir must be noted: {:?}",
        report.notes
    );
}

#[test]
fn save_names_are_recognised_by_prefix_case_insensitively() {
    use fatx::discovery::is_save_name;
    for yes in [
        "CAREER_01",
        "ALIAS_JOSHUA S 10",
        "career_01",
        "Alias_x",
        "CAREER_",
    ] {
        assert!(is_save_name(yes), "{yes}");
    }
    // ghost racers, near misses, too short, non-ASCII must not panic
    for no in [
        "SHADOW_74GR1",
        "CAREER",
        "CAREER01",
        "",
        "ÄLIAS_01",
        "日本語のファイル名",
    ] {
        assert!(!is_save_name(no), "{no}");
    }
}

#[test]
fn oversized_save_entry_is_noted_not_read() {
    let career = fatx::test_util::oracle_career_latest();
    let good = save(PROFILE_A, "00000001", "CAREER_01_360");
    let usb = FatxImageBuilder::new()
        .cluster_size(0x1000)
        .file(good.clone(), career.clone())
        .file(save(PROFILE_C, "00000001", "CAREER_HUGE"), career)
        .build_usb_image();

    // Declare a size far above any real save (and above MAX_SAVE_BYTES).
    let mut image = usb.image.clone();
    let name_off = image
        .windows(11)
        .position(|w| w == b"CAREER_HUGE".as_slice())
        .expect("dirent name present in the image");
    let dirent = name_off - 2;
    image[dirent + 0x30..dirent + 0x34].copy_from_slice(&0x7FFF_FFF0u32.to_be_bytes());

    let mut cur = Cursor::new(image);
    let drive = XboxDriveImage::probe(&mut cur, usb.image.len() as u64).unwrap();
    let mut vol = fatx::FatxVolume::open(
        &mut cur,
        drive.data_partition.offset,
        drive.data_partition.length,
    )
    .unwrap();

    let report = fatx::discovery::discover_prostreet_saves_noted(&mut vol).unwrap();
    assert_eq!(report.saves.len(), 1);
    assert!(
        report
            .notes
            .iter()
            .any(|n| n.contains("CAREER_HUGE") && n.contains("too large")),
        "{:?}",
        report.notes
    );
}
