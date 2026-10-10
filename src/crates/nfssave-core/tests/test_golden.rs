//! Port of tests/test_golden.py: golden regression tests — source containers
//! -> byte-exact verified output.
//!
//! Regression pins for the converter (last moved 2026-10-09: race-day
//! progress table, node [0][len] headers, CustomRaceDayMemcard strings,
//! record tail words, alias extra used size). Any diff here means the
//! converter changed behavior. Golden md5 pins are kept EXACTLY as written.

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use nfssave_core::convert::{ConversionReport, convert_payload, write_pc_save};
use nfssave_core::tree::Tree;
use nfssave_core::{MC02, read_container};

fn root() -> PathBuf {
    common::repo_root()
}

/// (source path, golden md5)
fn cases() -> Vec<(PathBuf, &'static str)> {
    let r = root();
    vec![
        (
            r.join("Extracted/Career/CAREER_01"),
            "1074f3f138d265e4a41f885badd8bb87",
        ),
        (
            r.join("docs/re/pair/CAREER_02_360_fresh"),
            "3da9f4c0a5a2b7d5c55863d49de4852c",
        ),
        (
            r.join("Extracted/Career/CAREER_03"),
            "7e9cc08492c4971153cab398e854d5c6",
        ),
        (
            r.join("Extracted/Alias/ALIAS_360"),
            "5d8ab470357fc03e6f7ccc2571d95fe1",
        ),
        (
            r.join("docs/re/c1_latest/CAREER_01_360"),
            "3629c8559ee06e5e0b13fba02400ddd3",
        ),
        (
            r.join("docs/re/pair_raceday/CAREER_02_360"),
            "ec77c9309356db48faeae8e66f840176",
        ),
        // anonymized copy of the personal alias save (docs/re/alias_anon/README.md)
        (
            r.join("docs/re/alias_anon/ALIAS_360"),
            "e2b29e6eb34771b96a57b1ee394f1f74",
        ),
    ]
}

/// Personal saves (gitignored, under Extracted/) may be absent and skip;
/// a missing tracked fixture is a failure.
fn present(src: &Path) -> bool {
    if src.is_file() {
        return true;
    }
    assert!(
        src.starts_with(root().join("Extracted")),
        "tracked fixture missing: {}",
        src.display()
    );
    eprintln!("skipped (source not present): {}", src.display());
    false
}

fn convert(src: &Path, out_root: &Path) -> PathBuf {
    let mc02 = MC02::parse(&read_container(src).unwrap().payload).unwrap();
    let mut report = ConversionReport {
        source: src.display().to_string(),
        ..Default::default()
    };
    let pc = convert_payload(&mc02, Some(&mut report), None).unwrap();
    let name = src.file_name().unwrap().to_str().unwrap();
    write_pc_save(&pc, name, out_root).unwrap()
}

fn md5_hex(data: &[u8]) -> String {
    use md5::{Digest, Md5};
    let digest = Md5::digest(data);
    common::hex(&digest)
}

#[test]
fn golden_outputs() {
    let tmp = common::TempDir::new("golden");
    for (src, golden) in cases() {
        if !present(&src) {
            continue;
        }
        let target = convert(&src, tmp.path());
        let digest = md5_hex(&fs::read(&target).unwrap());
        assert_eq!(digest, golden, "golden md5 mismatch for {}", src.display());
    }
}

/// Every golden output re-parses with clean CRCs and PC framing.
#[test]
fn output_self_validates() {
    let tmp = common::TempDir::new("selfval");
    for (src, _) in cases() {
        if !present(&src) {
            continue;
        }
        let target = convert(&src, tmp.path());
        let m = MC02::parse(&fs::read(&target).unwrap()).unwrap();
        assert_eq!(
            m.check(),
            Vec::<String>::new(),
            "CRC failures in output for {}",
            src.display()
        );
        let t = Tree::parse(&m.tree, false).unwrap();
        assert!(!t.records.is_empty());
        for r in &t.records {
            assert_eq!(r.flags & 0xFF, 1, "record {:#x} lost PC flags byte", r.id);
        }
    }
}
