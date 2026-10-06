//! Mounted-volume scan: console-formatted USB sticks are plain FAT32 with a
//! `Content\<profile>\<titleID>\<type>\<file>` tree (measured on real
//! hardware 2026-10-05, fatx SPEC.md §5.1). Drive letters are modelled as
//! temp-dir roots.

use std::fs;
use std::path::{Path, PathBuf};

use fatx::test_util::{oracle_career_latest, with_title_id};
use nfspc_converter::app::drivescan::scan_volume_roots;
use tempfile::TempDir;

const PROFILE: &str = "E00001CFFAB204C4";
const PROSTREET: &str = "45410822";

fn put(root: &Path, rel: &str, bytes: &[u8]) -> PathBuf {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, bytes).unwrap();
    p
}

fn career() -> Vec<u8> {
    oracle_career_latest()
}

fn other_game() -> Vec<u8> {
    with_title_id(career(), [0x4D, 0x53, 0x07, 0xD1])
}

fn rel(file: &str) -> String {
    format!("Content/{PROFILE}/{PROSTREET}/00000001/{file}")
}

#[test]
fn finds_the_real_stick_layout() {
    let stick = TempDir::new().unwrap();
    let bytes = career();
    put(stick.path(), &rel("CAREER_01"), &bytes);
    put(stick.path(), "name.txt", b"\xfe\xff\0U\0S\0B");

    let report = scan_volume_roots(&[stick.path().to_path_buf()]);
    assert!(report.notes.is_empty(), "{:?}", report.notes);
    assert_eq!(report.saves.len(), 1);
    let save = &report.saves[0];
    assert_eq!(save.bytes, bytes, "full file bytes are carried");
    assert!(
        save.source_path.ends_with(&rel("CAREER_01")),
        "{}",
        save.source_path
    );
    assert!(
        save.source_path
            .starts_with(&stick.path().display().to_string().replace('\\', "/")),
        "source path names the drive: {}",
        save.source_path
    );
    assert!(!save.friendly_name.is_empty());
}

#[test]
fn filter_matches_the_fatx_discovery_rules() {
    let stick = TempDir::new().unwrap();
    // kept: name prefix
    put(stick.path(), &rel("ALIAS_01"), &career());
    // dropped: ProStreet packages that are not saves (real G: drive had a
    // "Shadow 74GR1" item next to the careers; it fails conversion)
    put(stick.path(), &rel("SHADOW_74GR1"), &career());
    put(stick.path(), &rel("SAVEGAME"), &career());
    // kept: save name under another title ID (regional ProStreet releases)
    put(
        stick.path(),
        &format!("Content/{PROFILE}/4D5307D1/00000001/CAREER_09"),
        &other_game(),
    );
    // kept: 00000002 is the other save-type folder
    put(
        stick.path(),
        &format!("Content/{PROFILE}/{PROSTREET}/00000002/CAREER_02"),
        &career(),
    );
    // dropped: another game's save without a ProStreet prefix
    put(
        stick.path(),
        &format!("Content/{PROFILE}/4D5307D1/00000001/SAVE"),
        &other_game(),
    );
    // dropped: non-save content type folder (DLC/marketplace)
    put(
        stick.path(),
        "Content/0000000000000000/FFFE07DF/00040000/ContentCache.pkg",
        &career(),
    );
    // dropped: tiny junk with no CON header
    put(stick.path(), &rel("README"), b"hi");

    let report = scan_volume_roots(&[stick.path().to_path_buf()]);
    let mut names: Vec<String> = report
        .saves
        .iter()
        .map(|s| s.source_path.rsplit('/').next().unwrap().to_string())
        .collect();
    names.sort();
    assert_eq!(names, vec!["ALIAS_01", "CAREER_02", "CAREER_09"]);
}

#[test]
fn scans_every_root_and_sorts_results() {
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    put(a.path(), &rel("CAREER_02"), &career());
    put(
        a.path(),
        &format!("Content/E000000000000002/{PROSTREET}/00000001/CAREER_01"),
        &career(),
    );
    put(b.path(), &rel("CAREER_03"), &career());

    let report = scan_volume_roots(&[a.path().to_path_buf(), b.path().to_path_buf()]);
    assert_eq!(report.saves.len(), 3);
    let paths: Vec<&String> = report.saves.iter().map(|s| &s.source_path).collect();
    let mut sorted = paths.clone();
    sorted.sort();
    assert_eq!(paths, sorted);
}

#[test]
fn roots_without_content_or_unreadable_are_silent() {
    let empty = TempDir::new().unwrap();
    let missing = empty.path().join("no-such-drive");
    let report = scan_volume_roots(&[empty.path().to_path_buf(), missing]);
    assert!(report.saves.is_empty());
    assert!(report.notes.is_empty(), "{:?}", report.notes);
}

#[test]
fn old_container_format_is_reported_not_ignored() {
    let stick = TempDir::new().unwrap();
    put(stick.path(), "Xbox360/Data0000", &[0u8; 16]);
    put(stick.path(), "Xbox360/Data0001", &[0u8; 16]);

    let report = scan_volume_roots(&[stick.path().to_path_buf()]);
    assert!(report.saves.is_empty());
    assert_eq!(report.notes.len(), 1, "{:?}", report.notes);
    assert!(report.notes[0].contains("Xbox360"), "{}", report.notes[0]);
}

#[test]
fn save_named_files_that_are_not_save_packages_are_noted_not_loaded() {
    let stick = TempDir::new().unwrap();
    put(stick.path(), &rel("CAREER_01"), &career());
    // right name, wrong content: no CON magic
    put(stick.path(), &rel("CAREER_02"), &[0x42u8; 0x2000]);
    // right name and magic, absurd size: never read whole
    let mut huge = b"CON ".to_vec();
    huge.resize(
        nfspc_converter::app::drivescan::MAX_SAVE_BYTES as usize + 1,
        0,
    );
    put(stick.path(), &rel("CAREER_03"), &huge);

    let report = scan_volume_roots(&[stick.path().to_path_buf()]);
    assert_eq!(report.saves.len(), 1, "{:?}", report.notes);
    assert!(report.saves[0].source_path.ends_with("CAREER_01"));
    assert_eq!(report.notes.len(), 2, "{:?}", report.notes);
    assert!(report.notes.iter().any(|n| n.contains("CAREER_02")));
    assert!(report.notes.iter().any(|n| n.contains("CAREER_03")));
}
