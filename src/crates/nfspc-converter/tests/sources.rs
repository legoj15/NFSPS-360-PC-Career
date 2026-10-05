//! Manual-source discovery: single-file picks (CON container or raw MC02)
//! and depth-bounded folder scans collecting CAREER_*/ALIAS_* CON files.

use std::fs;
use std::path::{Path, PathBuf};

use nfspc_converter::app::sources::{discover_manual, MAX_DEPTH, NAME_PREFIXES};
use tempfile::TempDir;

/// Minimal file with CON magic (discovery checks magic, not full structure).
fn write_con(dir: &Path, name: &str) -> PathBuf {
    let mut bytes = vec![0u8; 0x400];
    bytes[..4].copy_from_slice(b"CON ");
    let p = dir.join(name);
    fs::write(&p, bytes).unwrap();
    p
}

fn write_raw_mc02(dir: &Path, name: &str) -> PathBuf {
    let mut bytes = vec![0u8; 0x40];
    bytes[..4].copy_from_slice(b"MC02");
    let p = dir.join(name);
    fs::write(&p, bytes).unwrap();
    p
}

fn write_text(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, b"hello world, definitely not a save file").unwrap();
    p
}

fn mkdirs(base: &Path, rel: &str) -> PathBuf {
    let p = base.join(rel);
    fs::create_dir_all(&p).unwrap();
    p
}

fn found_paths(root: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = discover_manual(root)
        .unwrap()
        .iter()
        .map(|m| m.path.clone())
        .collect();
    v.sort();
    v
}

#[test]
fn constants_are_the_specified_contract() {
    assert_eq!(NAME_PREFIXES, ["CAREER_", "ALIAS_"]);
    // depth-bounded (~4): 5 covers Content/<profile>/<title>/<type>/<file>
    // from an extracted dump root; deeper structure is not an Xbox layout.
    assert_eq!(MAX_DEPTH, 5);
}

#[test]
fn single_file_con_container_is_accepted() {
    let tmp = TempDir::new().unwrap();
    let f = write_con(tmp.path(), "CAREER_01_360");
    let found = discover_manual(&f).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].path, f);
    assert_eq!(found[0].label, "CAREER_01_360");
}

#[test]
fn single_file_raw_mc02_is_accepted() {
    let tmp = TempDir::new().unwrap();
    let f = write_raw_mc02(tmp.path(), "CAREER_02");
    let found = discover_manual(&f).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].path, f);
}

#[test]
fn single_file_garbage_is_rejected_with_clear_error() {
    let tmp = TempDir::new().unwrap();
    let f = write_text(tmp.path(), "notes.txt");
    let err = discover_manual(&f).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    let msg = err.to_string();
    assert!(
        msg.contains("CON") || msg.contains("MC02"),
        "error should name what was expected: {msg}"
    );
}

#[test]
fn folder_scan_collects_career_and_alias_con_files() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let d1 = mkdirs(root, "dir1");
    let d2 = mkdirs(&d1, "dir2");
    let d3 = mkdirs(&d2, "dir3");

    let depth1 = write_con(root, "CAREER_01");
    let depth2 = write_con(&d1, "ALIAS_JOSHUA S 10");
    let depth3 = write_con(&d2, "CAREER_03");
    let depth4 = write_con(&d3, "CAREER_04"); // exactly MAX_DEPTH: kept

    let mut expected = vec![depth1, depth2, depth3, depth4];
    expected.sort();
    assert_eq!(found_paths(root), expected);
}

#[test]
fn folder_scan_is_depth_bounded() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let deep = mkdirs(root, "a/b/c/d/e"); // file at depth 6 = one past MAX_DEPTH
    let too_deep = write_con(&deep, "CAREER_06");
    let within = write_con(root, "CAREER_01");
    let paths = found_paths(root);
    assert!(paths.contains(&within));
    assert!(!paths.contains(&too_deep), "depth-6 file must be skipped");
}

#[test]
fn folder_scan_requires_con_magic_and_save_name() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    write_text(root, "CAREER_NOT_A_CON"); // right name, no CON magic
    write_con(root, "SOMETHING_ELSE"); // CON magic, wrong name
    assert!(
        discover_manual(root).unwrap().is_empty(),
        "neither qualifies"
    );
}

#[test]
fn folder_scan_covers_extracted_content_tree_shape() {
    // an extracted Content tree: Content/<profile>/<title>/00000001/CAREER_01
    let tmp = TempDir::new().unwrap();
    let root = mkdirs(tmp.path(), "Content/E0000000/45410822/00000001");
    let save = write_con(&root, "CAREER_01");
    let found = discover_manual(tmp.path()).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].path, save);
}

#[test]
fn empty_folder_is_ok_but_empty() {
    let tmp = TempDir::new().unwrap();
    assert!(discover_manual(tmp.path()).unwrap().is_empty());
}
