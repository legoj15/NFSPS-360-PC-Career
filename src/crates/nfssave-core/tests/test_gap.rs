//! Gapped-career regression: a career whose console record tail is damaged
//! (tree parses with `gap != 0`) converts without any recovery source,
//! warns about the gap, and the output is byte-identical to the Python's.
//! Digest produced by `scripts/python/nfssave` on the same fixture
//! (tests/gapfix.py), 2026-10-09.

mod common;

use md5::{Digest, Md5};

use nfssave_core::MC02;
use nfssave_core::convert::{ConversionReport, convert_payload};
use nfssave_core::tree::{Record, Tree};

fn md5(b: &[u8]) -> String {
    let d = Md5::digest(b);
    d.iter().map(|x| format!("{x:02x}")).collect()
}

#[test]
fn gapped_career_converts_with_gap_warning_matching_python() {
    let (gapped, _) = common::build_gapped(None);
    assert_eq!(
        md5(&gapped),
        "c57cfaefded23cd1d2b3ed9c01b296aa",
        "fixture drift"
    );
    let mut report = ConversionReport::default();
    let pc = convert_payload(&MC02::parse(&gapped).unwrap(), Some(&mut report)).unwrap();
    assert_eq!(
        md5(&pc.to_bytes().unwrap()),
        "d00a8fdce99bea2068efdfa961451f6d"
    );
    assert_eq!(report.records, 8);
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("console record region damaged - missing chunks")),
        "warnings: {:?}",
        report.warnings
    );
}

/// Internal gap: one mid-tree record (0xD548266C, index 7) is noise, the
/// record after it re-anchors. Every record after the damage converts exactly
/// as in a clean tree (the last one included - it keeps its post spill); the
/// record before the damage converts with no spill. The digest pins both and
/// equals the Python's (tests/test_gap.py).
#[test]
fn internal_gap_keeps_spills_after_the_damage_matching_python() {
    let post_word = [0, 0, 0, 7];
    let (gapped, pristine) = common::build_gapped_ex(Some(7), Some(1), Some(post_word));
    assert_eq!(
        md5(&gapped),
        "bacda5eeb061896b540221dc26fb8aa8",
        "fixture drift"
    );
    let gt = Tree::parse(&MC02::parse(&gapped).unwrap().tree, true).unwrap();
    assert_ne!(gt.gap, 0);
    let k = gt.gap_at.expect("internal gap must record where it is");
    assert_eq!((k, gt.records.len()), (7, 8));
    assert_eq!(gt.records[k - 1].id, 0x885B_4DDC);
    assert_eq!(gt.post[..4], post_word);

    let mut report = ConversionReport::default();
    let pc = convert_payload(&MC02::parse(&gapped).unwrap(), Some(&mut report)).unwrap();
    let want = convert_payload(&MC02::parse(&pristine).unwrap(), None).unwrap();
    let got_recs = Tree::parse(&pc.tree, false).unwrap().records;
    let want_recs = Tree::parse(&want.tree, false).unwrap().records;
    let find = |recs: &[Record], id: u32| recs.iter().find(|r| r.id == id).cloned();
    assert!(find(&got_recs, 0xD548_266C).is_none());
    for rec in gt.records.iter().filter(|r| r.id != 0x885B_4DDC) {
        let (g, w) = (
            find(&got_recs, rec.id).unwrap(),
            find(&want_recs, rec.id).unwrap(),
        );
        assert_eq!((g.flags, g.payload), (w.flags, w.payload), "{:#x}", rec.id);
    }
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("console record region damaged - missing chunks")),
        "warnings: {:?}",
        report.warnings
    );
    assert_eq!(
        md5(&pc.to_bytes().unwrap()),
        "743068fdd49685367041faa4df7c3903"
    );
}
