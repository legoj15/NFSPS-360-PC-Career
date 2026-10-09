//! Port of tests/test_raceday.py: mid-race-day pair (Battle Machine, Nevada):
//! GameplayData carries an active race-day block at 0x2E0 whose 360 layout has
//! a 4-byte pad at 0x314 that the PC layout lacks. Converted output must line
//! up with native PC.

mod common;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use nfssave_core::convert::{
    CONSOLE_ONLY_RACEDAYS, ConversionReport, PROGRESS_LEN, convert_decal_entry, convert_payload,
    progress_table_offset, raceday_block_end,
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

/// Race-day progress table of a PC GameplayData payload:
/// 90 x [u32 key][u32 state][u32 score], keyed by race-day key.
fn progress(p: &[u8]) -> HashMap<u32, (u32, u32)> {
    let o = progress_table_offset(p).expect("progress table not found");
    let word = |at: usize| u32::from_le_bytes(p[at..at + 4].try_into().unwrap());
    (0..PROGRESS_LEN)
        .map(|i| {
            let e = o + 12 * i;
            (word(e), (word(e + 4), word(e + 8)))
        })
        .collect()
}

/// Some race days carry 360-only state (no events on PC; five do not exist
/// in the PC gameplay database at all). The PC Race Day map builds a hub for
/// each and crashes reading event 0 (nfs.exe 0x7F6480). Converted tables must
/// match the native PC side of both matched pairs.
#[test]
fn progress_matches_native_pairs() {
    let pairs = [
        (r360(), rpc()),
        (
            root().join("docs/re/pair/CAREER_02_360_fresh"),
            root().join("docs/re/pair/CAREER_02_pc_native"),
        ),
    ];
    for (src, nat) in pairs {
        if !src.is_file() || !nat.is_file() {
            eprintln!("skipped: pair absent ({})", src.display());
            continue;
        }
        let conv = progress(&gp(&conv_tree(&src), GAMEPLAY));
        let raw = fs::read(&nat).unwrap();
        let want = progress(&gp(
            &Tree::parse(&MC02::parse(&raw).unwrap().tree, false).unwrap(),
            GAMEPLAY,
        ));
        for (k, v) in &want {
            assert_eq!(conv[k], *v, "{:#x} in {}", k, src.display());
        }
    }
}

#[test]
fn console_only_race_days_cleared() {
    let src = root().join("docs/re/c1_latest/CAREER_01_360"); // deep career, Race Day crash
    if !src.is_file() {
        eprintln!("skipped: {} absent", src.display());
        return;
    }
    let conv = progress(&gp(&conv_tree(&src), GAMEPLAY));
    for &(k, state) in &CONSOLE_ONLY_RACEDAYS {
        assert_eq!(conv[&k], (state, 0), "{k:#x}");
    }
    assert!(CONSOLE_ONLY_RACEDAYS.iter().any(|&(k, _)| k == 0x8F7CCCE0)); // hub in the crash dump
}
