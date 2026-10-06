//! Port of tests/test_golden.py: golden regression tests — source containers
//! -> byte-exact verified output.
//!
//! Regression pins for the converter (2026-10-04 evening: STFS block map,
//! node flag words, u16 car part slots). Any diff here means the converter
//! changed behavior. Golden md5 pins are kept EXACTLY as written.

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
            "7b7e1893047b01b00f7037ef54ceca44",
        ),
        (
            r.join("docs/re/pair/CAREER_02_360_fresh"),
            "8dd15c6cb5736cf14aa2694289d8480d",
        ),
        (
            r.join("Extracted/Career/CAREER_03"),
            "0dcfed80eab3dfcc246499b07ae54c37",
        ),
        (
            r.join("Extracted/Alias/ALIAS_360"),
            "578a10cb583785bb6cb00fa64bc69439",
        ),
        (
            r.join("docs/re/c1_latest/CAREER_01_360"),
            "718b6b6b8494decde59eb6b1defcc01d",
        ),
        (
            r.join("docs/re/pair_raceday/CAREER_02_360"),
            "2bb7d00963509e71d6eaeccbed496b65",
        ),
        // anonymized copy of the personal alias save (docs/re/alias_anon/README.md)
        (
            r.join("docs/re/alias_anon/ALIAS_360"),
            "8ae3d82a3c9cb1c9500d6fcce8c01b9d",
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
