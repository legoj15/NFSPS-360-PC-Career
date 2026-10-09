//! Re-save twin ("--twin") regression tests.
//!
//! Fixtures are built from the tracked oracle by 0xAA-ing the LAST record's
//! whole span (classic trailing console damage: the record chain stops early,
//! `gap != 0`, the twin path activates). Every digest below was produced by
//! the Python reference converter (`scripts/python/nfssave`) on the same
//! fixtures, run on 2026-10-05 (digests refreshed 2026-10-09 for the race-day progress fix).

mod common;

use std::path::PathBuf;

use md5::{Digest, Md5};

use nfssave_core::convert::{ConversionReport, convert_payload};
use nfssave_core::mc02::Endian;
use nfssave_core::tree::Tree;
use nfssave_core::{MC02, read_container};

const SRC: &str = "docs/re/c1_latest/CAREER_01_360";

fn src_path() -> PathBuf {
    common::repo_root().join(SRC)
}

fn md5(b: &[u8]) -> String {
    let d = Md5::digest(b);
    d.iter().map(|x| format!("{x:02x}")).collect()
}

/// `(crafted, twin)`: the twin is a pristine rebuild of the (optionally
/// duplicated) record list; the crafted source has the last record's span
/// replaced by 0xAA noise so its tree parses with a trailing gap.
fn build_pair(duplicate_last: bool) -> (Vec<u8>, Vec<u8>) {
    let mc02 = MC02::parse(&read_container(src_path()).unwrap().payload).unwrap();
    let mut tree = Tree::parse(&mc02.tree, true).unwrap();
    if duplicate_last {
        // a duplicate chunk id INSIDE the intact record prefix: exercises the
        // last-wins dedup of the merge's by-id map
        let dup = tree.records[tree.records.len() - 1].clone();
        tree.records.insert(tree.records.len() - 1, dup);
    }
    let twin = MC02::new(
        Endian::Big,
        mc02.extra.clone(),
        tree.build(true, mc02.tree_size as usize).unwrap(),
        mc02.tree_size,
    )
    .to_bytes()
    .unwrap();

    let mut crafted_tree = tree.build(true, mc02.tree_size as usize).unwrap();
    let mut off = 0x48usize;
    for r in &tree.records[..tree.records.len() - 1] {
        off += 12 + r.payload.len();
    }
    let span = 12 + tree.records.last().unwrap().payload.len();
    crafted_tree[off..off + span].fill(0xAA);
    let crafted = MC02::new(
        Endian::Big,
        mc02.extra.clone(),
        crafted_tree,
        mc02.tree_size,
    )
    .to_bytes()
    .unwrap();

    // sanity: the crafted tree really carries trailing damage
    let gapped = Tree::parse(&MC02::parse(&crafted).unwrap().tree, true).unwrap();
    assert_eq!(gapped.gap, span, "fixture must parse with a trailing gap");
    (crafted, twin)
}

/// A damaged tail recovers from a matching twin: the twin's last record is
/// converted and merged in (`recovered from re-save twin` warning), and the
/// output is byte-identical to the Python's.
#[test]
fn twin_merge_recovers_damaged_tail_matching_python() {
    let (crafted, twin) = build_pair(false);
    assert_eq!(md5(&crafted), "c57cfaefded23cd1d2b3ed9c01b296aa", "fixture drift");
    assert_eq!(md5(&twin), "746363afed1ca5af6c1fabf3130271af", "fixture drift");
    let mut report = ConversionReport::default();
    let pc = convert_payload(
        &MC02::parse(&crafted).unwrap(),
        Some(&mut report),
        Some(&twin),
    )
    .unwrap();
    assert_eq!(md5(&pc.to_bytes().unwrap()), "f60c9680ff3354a008fb74e2c3a6f2a2");
    assert_eq!(report.records, 9);
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("UnlockSystem recovered from re-save twin")),
        "warnings: {:?}",
        report.warnings
    );
}

/// An EMPTY twin slice means "no twin" (Python: falsy bytes skip the twin
/// path): the damaged source still converts, with the no-twin warning.
#[test]
fn empty_twin_slice_is_treated_as_no_twin() {
    let (crafted, _) = build_pair(false);
    let mut report = ConversionReport::default();
    let pc = convert_payload(
        &MC02::parse(&crafted).unwrap(),
        Some(&mut report),
        Some(&[]),
    )
    .unwrap_or_else(|e| panic!("empty twin must be ignored, not fail: {e}"));
    assert_eq!(md5(&pc.to_bytes().unwrap()), "a3720d7babc27661780a2da8ae833a73");
    assert_eq!(report.records, 8);
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("no twin available")),
        "warnings: {:?}",
        report.warnings
    );
}

/// Duplicate chunk ids collapse last-wins in the merge's by-id map exactly
/// like the Python's dict comprehension (output digest pinned from it).
#[test]
fn twin_merge_duplicate_ids_last_wins_like_python() {
    let (crafted, twin) = build_pair(true);
    assert_eq!(md5(&crafted), "12bd862cf16176b54157fdc2bda8b56f", "fixture drift");
    assert_eq!(md5(&twin), "4552504782a964d5d010a2ff0b981b92", "fixture drift");
    let mut report = ConversionReport::default();
    let pc = convert_payload(
        &MC02::parse(&crafted).unwrap(),
        Some(&mut report),
        Some(&twin),
    )
    .unwrap();
    assert_eq!(md5(&pc.to_bytes().unwrap()), "aa02bcb1058250b9402ea074d952f368");
    assert_eq!(report.records, 10);
    assert_eq!(
        report.chunk_list.iter().filter(|c| **c == "UnlockSystem").count(),
        2,
        "duplicate id keeps its twin-order slots: {:?}",
        report.chunk_list
    );
}
