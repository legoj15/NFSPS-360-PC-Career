//! Port of tests/test_pair.py: matched-state pair — converting the fresh 360
//! CAREER_02 must reproduce the PC-native CAREER_02 (same career state, saved
//! on both platforms) wherever the data is not volatile.
//!
//! Checks:
//!  * property-node flag words keep the flag byte first (natural order);
//!  * the starter car's customization part arrays (u16 slots) match byte-exact.

mod common;

use std::collections::HashMap;
use std::fs;
use std::sync::OnceLock;

use nfssave_core::convert::{ConversionReport, convert_payload};
use nfssave_core::tree::Tree;
use nfssave_core::{MC02, read_container};

type RecMap = HashMap<u32, Vec<u8>>;

fn records(tree: &Tree) -> RecMap {
    tree.records
        .iter()
        .map(|r| (r.id, r.payload.clone()))
        .collect()
}

fn pair() -> Option<&'static (RecMap, RecMap)> {
    static PAIR: OnceLock<Option<(RecMap, RecMap)>> = OnceLock::new();
    PAIR.get_or_init(|| {
        let root = common::repo_root();
        let p360 = root.join("docs/re/pair/CAREER_02_360_fresh");
        let ppc = root.join("docs/re/pair/CAREER_02_pc_native");
        if !p360.is_file() || !ppc.is_file() {
            return None;
        }
        let mc = MC02::parse(&read_container(&p360).unwrap().payload).unwrap();
        let mut report = ConversionReport::default();
        let conv = convert_payload(&mc, Some(&mut report)).unwrap();
        let conv = records(&Tree::parse(&conv.tree, false).unwrap());
        let native = records(
            &Tree::parse(&MC02::parse(&fs::read(&ppc).unwrap()).unwrap().tree, false).unwrap(),
        );
        Some((conv, native))
    })
    .as_ref()
}

/// Mirrors @unittest.skipUnless(PAIR_360.is_file() and PAIR_PC.is_file(), ...)
fn guarded() -> Option<&'static (RecMap, RecMap)> {
    pair().or_else(|| {
        eprintln!("skipped: pair samples absent");
        None
    })
}

const NODE_CHUNKS: [u32; 7] = [
    0x328C6431, 0xDC6B027F, 0xB67F6CC6, 0x51A41B14, 0x885B4DDC, 0xD548266C, 0xCA269650,
];
const CARDB: u32 = 0x47A07113;

fn le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

#[test]
fn node_flag_bytes() {
    let Some((conv, native)) = guarded() else {
        return;
    };
    for &cid in &NODE_CHUNKS {
        let a = &conv[&cid];
        let b = &native[&cid];
        let mut bad: Vec<usize> = Vec::new();
        // flag word follows each [u32 0][u32 len] node header
        for o in (8..b.len().saturating_sub(4)).step_by(4) {
            let (zero, ln) = (le32(b, o - 8), le32(b, o - 4));
            if zero == 0 && ln == 4 && matches!(b[o], 0x00 | 0x01 | 0xFF) && a[o] != b[o] {
                bad.push(o);
            }
        }
        assert!(
            bad.is_empty(),
            "chunk {cid:#x}: {} flag bytes misplaced (first 10: {:?})",
            bad.len(),
            &bad[..bad.len().min(10)],
        );
    }
}

/// Car-table entries end in [u8][u8][u8][pad] (garage slot/index);
/// owned-car entries must keep them natural.
#[test]
fn car_table_slot_bytes() {
    let Some((conv, native)) = guarded() else {
        return;
    };
    let (a, b) = (&conv[&CARDB], &native[&CARDB]);
    for k in [114usize, 150, 190] {
        // catalog entries identical on both platforms
        let o = 0x14 + 24 * k + 20;
        assert_eq!(
            common::hex(&a[o..o + 3]),
            common::hex(&b[o..o + 3]),
            "entry {k}"
        );
    }
}

/// Each car record holds three customization sets 0x7B4 apart.
#[test]
fn all_blueprint_part_sets() {
    let Some((conv, native)) = guarded() else {
        return;
    };
    let (a, b) = (&conv[&CARDB], &native[&CARDB]);
    for setoff in [0x7B4usize, 0xF68] {
        let (s, e) = (0x2680 + setoff + 0x3C, 0x2680 + setoff + 0x186);
        assert_eq!(
            common::hex(&a[s..e]),
            common::hex(&b[s..e]),
            "set +{setoff:#x}"
        );
    }
}

/// 8-byte packed entries after the car records: the 360 leaves the link/low
/// fields uninitialized (0x2AAA); PC writes 'none'. Empty 360 entry
/// 2aaafffe ffff2aaa must become feffff3f fffffeff.
#[test]
fn packed_table_entries() {
    use nfssave_core::convert::convert_packed_entry;
    let case = |hexstr: &str| -> [u8; 8] { common::unhex(hexstr).try_into().unwrap() };
    assert_eq!(
        common::hex(&convert_packed_entry(&case("2aaafffeffff2aaa"))),
        "feffff3ffffffeff"
    );
    assert_eq!(
        common::hex(&convert_packed_entry(&case("2aaa01aa5c852aaa"))),
        "aa01ff3fffff845c"
    );
    assert_eq!(
        common::hex(&convert_packed_entry(&case("2aaafffe31182aaa"))),
        "feffff3fffff1831"
    );
    let Some((conv, native)) = guarded() else {
        return;
    };
    let (a, b) = (&conv[&CARDB], &native[&CARDB]);
    let empty = common::unhex("feffff3ffffffeff");
    for o in (0x7DF50..0x90658).step_by(8) {
        // every empty native entry
        if b[o..o + 8] == empty[..] && a[o..o + 8] != b[o..o + 8] {
            panic!("entry {o:#x}: {}", common::hex(&a[o..o + 8]));
        }
    }
}

#[test]
fn starter_car_parts() {
    let Some((conv, native)) = guarded() else {
        return;
    };
    let (s, e) = (0x2680 + 0x3C, 0x2680 + 0x190);
    assert_eq!(
        common::hex(&conv[&CARDB][s..e]),
        common::hex(&native[&CARDB][s..e])
    );
}
