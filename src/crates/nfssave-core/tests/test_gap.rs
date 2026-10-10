//! Gapped-career regression: a career whose console record tail is damaged
//! (tree parses with `gap != 0`) converts without any recovery source,
//! warns about the gap, and the output is byte-identical to the Python's.
//! Digest produced by `scripts/python/nfssave` on the same fixture
//! (tests/gapfix.py), 2026-10-09.

mod common;

use md5::{Digest, Md5};

use nfssave_core::MC02;
use nfssave_core::convert::{ConversionReport, convert_payload};

fn md5(b: &[u8]) -> String {
    let d = Md5::digest(b);
    d.iter().map(|x| format!("{x:02x}")).collect()
}

#[test]
fn gapped_career_converts_with_gap_warning_matching_python() {
    let (gapped, _) = common::build_gapped(false, None);
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
