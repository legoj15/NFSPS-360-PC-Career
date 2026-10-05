//! Port of tests/test_raceday.py: mid-race-day pair (Battle Machine, Nevada):
//! GameplayData carries an active race-day block at 0x2E0 whose 360 layout has
//! a 4-byte pad at 0x314 that the PC layout lacks. Converted output must line
//! up with native PC.

mod common;

use std::fs;
use std::path::PathBuf;

use nfssave_core::convert::{
    ConversionReport, convert_decal_entry, convert_payload, raceday_block_end,
};
use nfssave_core::tree::Tree;
use nfssave_core::{MC02, read_container};

const GAMEPLAY: u32 = 0x3B309E09;
const CARDB: u32 = 0x47A07113;

fn root() -> PathBuf {
    common::repo_root()
}

fn r360() -> PathBuf {
    root().join("docs/re/pair_raceday/CAREER_02_360")
}

fn rpc() -> PathBuf {
    root().join("docs/re/pair_raceday/CAREER_02_pc_native")
}

fn gp(tree: &Tree, id: u32) -> Vec<u8> {
    tree.records
        .iter()
        .find(|r| r.id == id)
        .unwrap()
        .payload
        .clone()
}

fn conv_tree(src: &std::path::Path) -> Tree {
    let mc02 = MC02::parse(&read_container(src).unwrap().payload).unwrap();
    let mut report = ConversionReport::default();
    let pc = convert_payload(&mc02, Some(&mut report), None).unwrap();
    Tree::parse(&pc.tree, false).unwrap()
}

fn native_tree() -> Tree {
    let raw = fs::read(rpc()).unwrap();
    Tree::parse(&MC02::parse(&raw).unwrap().tree, false).unwrap()
}

#[test]
fn raceday_block_aligned() {
    if !r360().is_file() || !rpc().is_file() {
        eprintln!("skipped: raceday pair absent");
        return;
    }
    let conv = gp(&conv_tree(&r360()), GAMEPLAY);
    let nat = gp(&native_tree(), GAMEPLAY);
    // event flag entries after the 360 pad, the name string before it,
    // and a u32 near the block end (physics floats differ in noise bits)
    for o in [0x300usize, 0x434, 0x444, 0x4D4, 0x77C, 0x34D4] {
        assert_eq!(
            common::hex(&conv[o..o + 4]),
            common::hex(&nat[o..o + 4]),
            "{o:#x}"
        );
    }
}

/// Starter car blueprint sets in the race-day pair: paint words are two u16s;
/// vinyl colour bytes (+0x574..0x628) stay natural.
#[test]
fn paint_and_colour_bytes() {
    if !r360().is_file() || !rpc().is_file() {
        eprintln!("skipped: raceday pair absent");
        return;
    }
    let conv = gp(&conv_tree(&r360()), CARDB);
    let nat = gp(&native_tree(), CARDB);
    for bs in [0x0usize, 0x7B4, 0xF68] {
        let r = 0x2680 + bs;
        assert_eq!(
            conv[r + 0x194..r + 0x198],
            nat[r + 0x194..r + 0x198],
            "{bs:#x}"
        );
    }
    let r = 0x2680;
    for o in [0x1A0usize, 0x1AC] {
        assert_eq!(
            common::hex(&conv[r + o..r + o + 4]),
            common::hex(&nat[r + o..r + o + 4])
        );
    }
    let nz = |b: &[u8]| -> Vec<usize> {
        b.iter()
            .enumerate()
            .filter(|(_, x)| **x != 0)
            .map(|(i, _)| i)
            .collect()
    };
    for o in [0x5A0usize, 0x5AC, 0x600, 0x604] {
        // values differ, layout must not
        assert_eq!(
            nz(&conv[r + o..r + o + 4]),
            nz(&nat[r + o..r + o + 4]),
            "{o:#x}"
        );
    }
}

#[test]
fn decal_entry_layout() {
    // 360 Camaro vinyl entry -> u16 fields swapped, bytes 6..9 natural
    assert_eq!(
        common::hex(&convert_decal_entry(&common::unhex(
            "04f6006c0002c01b1b0006590000"
        ))),
        "f6046c000200c01b1b0059060000"
    );
}

#[test]
fn block_end_detection() {
    let cases = [
        (r360(), 0x3E70usize),
        (root().join("docs/re/c1_latest/CAREER_01_360"), 0xB5B0),
    ];
    for (src, want) in cases {
        if !src.is_file() {
            continue;
        }
        let mc02 = MC02::parse(&read_container(&src).unwrap().payload).unwrap();
        let tree = Tree::parse(&mc02.tree, true).unwrap();
        let p = gp(&tree, GAMEPLAY);
        assert_eq!(raceday_block_end(&p), Some(want), "{}", src.display());
    }
}

/// GameplayData blob (PC payload 0x14, 0x10000 B) starts with
/// MD5(blob[0x10:]); the PC deserializer rejects the blob otherwise.
#[test]
fn converted_blob_md5() {
    use md5::{Digest, Md5};
    let sources = [r360(), root().join("docs/re/c1_latest/CAREER_01_360")];
    for src in sources {
        if !src.is_file() {
            continue;
        }
        let p = gp(&conv_tree(&src), GAMEPLAY);
        let blob = &p[0x14..0x14 + 0x10000];
        assert_eq!(
            &blob[..16],
            &Md5::digest(&blob[16..])[..],
            "{}",
            src.display()
        );
    }
}
