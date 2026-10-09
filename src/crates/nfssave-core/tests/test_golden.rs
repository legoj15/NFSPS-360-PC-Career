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
            "0cce08c3c9a502b9275657d62166e932",
        ),
        (
            r.join("docs/re/pair/CAREER_02_360_fresh"),
            "00f9d4427e18486eef546be07a5b2744",
        ),
        (
            r.join("Extracted/Career/CAREER_03"),
            "1c71bb3f1425a7c6a60d02f96798e7ff",
        ),
        (
            r.join("Extracted/Alias/ALIAS_360"),
            "02efaff0f7f60b73e0d93fbbe62ed4f3",
        ),
        (
            r.join("docs/re/c1_latest/CAREER_01_360"),
            "e5ddeef1cf4314ff9929743f69062e9d",
        ),
        (
            r.join("docs/re/pair_raceday/CAREER_02_360"),
            "a7b6ae96d15222948b31fdb27a7b8fda",
        ),
        // anonymized copy of the personal alias save (docs/re/alias_anon/README.md)
        (
            r.join("docs/re/alias_anon/ALIAS_360"),
            "42b389645cc9d7e3db296de6ee67e8fa",
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
