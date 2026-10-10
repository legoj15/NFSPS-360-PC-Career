//! Regression tests for corrupt/short record sizes and truncated containers.
//!
//! Every expected byte string or digest below was produced by the Python
//! reference converter (`scripts/python/nfssave`) on the same input, run on
//! 2026-10-05. Where the Python itself fails (its `assert`/`unpack_from`),
//! the expected Rust behavior is a refusal error, never a panic.

mod common;

use std::path::PathBuf;

use md5::{Digest, Md5};

use nfssave_core::container360::parse_container;
use nfssave_core::convert::{
    CARDB_ID, GAMEPLAY_ID, apply_struct_fixes, convert_payload, fix_cardb_packed, fix_cardb_parts,
    normalize_gameplay, rehash_gameplay, to_pc_record,
};
use nfssave_core::mc02::Endian;
use nfssave_core::payload_rules::convert_payload_auto;
use nfssave_core::tree::{Record, Tree, swap_u32s};
use nfssave_core::{MC02, read_container};

const SRC: &str = "docs/re/c1_latest/CAREER_01_360";
/// File-table block offset of the tracked save (header size 0x971a -> first
/// table 0xA000; volume descriptor has table-shift 1 -> block 0 at 0xC000).
const FILE_TABLE_OFF: usize = 0xC000;

fn src_path() -> PathBuf {
    common::repo_root().join(SRC)
}

fn md5(b: &[u8]) -> String {
    let d = Md5::digest(b);
    d.iter().map(|x| format!("{x:02x}")).collect()
}

/// Rebuild the tracked save with one record's payload replaced; mirrors the
/// fixture construction run against the Python converter.
fn craft(rec_id: u32, payload: &[u8]) -> Vec<u8> {
    let mc02 = MC02::parse(&read_container(src_path()).unwrap().payload).unwrap();
    let mut tree = Tree::parse(&mc02.tree, true).unwrap();
    for r in tree.records.iter_mut() {
        if r.id == rec_id {
            r.payload = payload.to_vec();
        }
    }
    let tree_bytes = tree.build(true, mc02.tree_size as usize).unwrap();
    MC02::new(Endian::Big, mc02.extra.clone(), tree_bytes, mc02.tree_size)
        .to_bytes()
        .unwrap()
}

fn convert_crafted(data: &[u8]) -> nfssave_core::Result<MC02> {
    convert_payload(&MC02::parse(data).unwrap(), None, None)
}

/// F1: a word-quantized string run whose rounded-up end exceeds a
/// non-word-multiple payload must clamp like the Python, not panic.
/// Python: convert_payload_auto(b"ABCDE\0\0\0\0\0") == 41424344450000000000.
#[test]
fn auto_mode_clamps_past_payload_tail() {
    let out = convert_payload_auto(b"ABCDE\x00\x00\x00\x00\x00", None, "chunk");
    assert_eq!(common::hex(&out), "41424344450000000000");
}

/// F2: the CARDB struct fixes on a 0x100-byte payload (far shorter than the
/// offsets they touch) must complete. Over an unmodified clone the Python
/// writes are all no-ops; over the swapped payload only the in-range car
/// table words are restored. Both vectors are the Python's exact output.
#[test]
fn cardb_fixes_clamp_on_short_payload() {
    let src: Vec<u8> = (0..=255u8).collect();

    // identity case: fixes applied with out == src
    let mut out = src.clone();
    fix_cardb_parts(&src, &mut out);
    fix_cardb_packed(&src, &mut out);
    assert_eq!(out, src, "natural-copy fixes over a clone must be identity");

    // pipeline-shaped case: fixes over the word-swapped payload
    let mut out = swap_u32s(&src);
    fix_cardb_parts(&src, &mut out);
    fix_cardb_packed(&src, &mut out);
    assert_eq!(
        common::hex(&out),
        "03020100070605040b0a09080f0e0d0c13121110171615141b1a19181f1e1d1c23222120272625242b2a29282c2d2e2f33323130373635343b3a39383f3e3d3c43424140444546474b4a49484f4e4d4c53525150575655545b5a59585c5d5e5f63626160676665646b6a69686f6e6d6c73727170747576777b7a79787f7e7d7c83828180878685848b8a89888c8d8e8f93929190979695949b9a99989f9e9d9ca3a2a1a0a4a5a6a7abaaa9a8afaeadacb3b2b1b0b7b6b5b4bbbab9b8bcbdbebfc3c2c1c0c7c6c5c4cbcac9c8cfcecdccd3d2d1d0d4d5d6d7dbdad9d8dfdedddce3e2e1e0e7e6e5e4ebeae9e8ecedeeeff3f2f1f0f7f6f5f4fbfaf9f8fffefdfc"
    );
}

/// F3: a 0x1000-byte GameplayData record runs the whole per-record pipeline
/// with clamped rehash/raceday handling. Digest and payload md5 are the
/// Python's (its output length stays 4096; it warns about the missing
/// race-day block end and progress table).
#[test]
fn gameplay_short_pipeline_matches_python() {
    let mut payload = vec![0x11u8; 0x1000];
    payload[0x0C..0x10].copy_from_slice(&(0x1000u32 - 0x10).to_be_bytes());
    let mut rec = Record {
        flags: 0x0100_0000,
        id: GAMEPLAY_ID,
        size: payload.len() as u32,
        payload,
        tail: Vec::new(),
    };
    let mut warnings = Vec::new();
    normalize_gameplay(&mut rec, &mut warnings);
    let src = rec.payload.clone();
    rec.payload = swap_u32s(&rec.payload);
    apply_struct_fixes(&mut rec, &src, &mut warnings).unwrap();
    to_pc_record(&mut rec, [0; 4]);
    rehash_gameplay(&mut rec);

    assert_eq!(rec.payload.len(), 0x1000);
    assert_eq!(
        warnings,
        vec![
            "GameplayData: active race day but block end not found - \
             race day will not resume"
                .to_string(),
            "GameplayData: race-day progress table not found - \
             the PC Race Day menu may crash"
                .to_string()
        ]
    );
    // digest clamps to the payload that exists
    assert_eq!(
        &rec.payload[0x14..0x24],
        &Md5::digest(&rec.payload[0x24..])[..]
    );
    assert_eq!(md5(&rec.payload), "7b121fa65da19598d889b86bb504d9f4");
}

/// A gameplay payload shorter than the race-day state word (< 0x2DC) is
/// refused: the Python reads that word with unpack_from and fails there too.
#[test]
fn gameplay_below_raceday_state_is_refused() {
    let data = craft(GAMEPLAY_ID, &[0x11; 0x2D8]);
    assert_eq!(
        md5(&data),
        "acd56e6ada2458d1efc25a22e6876ae7",
        "fixture drift"
    );
    let err = convert_crafted(&data).unwrap_err();
    assert!(
        err.to_string().contains("too short"),
        "unexpected error: {err}"
    );
}

/// A gameplay record size that is not a multiple of 4 is refused (the
/// Python's swap_u32s asserts there).
#[test]
fn gameplay_unaligned_size_is_refused() {
    let data = craft(GAMEPLAY_ID, &[0x11; 0x2E3]);
    assert_eq!(
        md5(&data),
        "e7084c4aaa3291502a34f35714975bf8",
        "fixture drift"
    );
    let err = convert_crafted(&data).unwrap_err();
    assert!(
        err.to_string().contains("word-aligned"),
        "unexpected error: {err}"
    );
}

/// A short-but-legal gameplay tail converts; digest md5 pinned from Python.
#[test]
fn gameplay_0x2e0_tail_matches_python() {
    let data = craft(GAMEPLAY_ID, &[0x11; 0x2E0]);
    assert_eq!(
        md5(&data),
        "3be0c08e4681cd323396ca2109ae18d9",
        "fixture drift"
    );
    let pc = convert_crafted(&data).unwrap();
    assert_eq!(
        md5(&pc.to_bytes().unwrap()),
        "bb30e797dc0b6fd926c6ccac55510bba"
    );
}

/// Every payload length in [0x2DC, 0x2E4) with a ZERO race-day state word
/// must refuse-with-error or convert like the Python — which clamps the
/// post-block tail `src[0x2E4:]` to empty — never panic on the tail slice.
/// Word-aligned lengths convert (digests pinned from the Python); unaligned
/// lengths are refused at the alignment check (Python asserts there too).
#[test]
fn gameplay_zero_state_window_matches_python() {
    // len -> (crafted fixture md5, converted output md5)
    let pinned = [
        (0x2DCusize, "d90b6f6931545fba07971c4fdb4908d7", "44138608e8501861891b389bca18daa9"),
        (0x2E0, "e4882a46c79271139bb47dd4dcd59a9c", "48dab5bcf0227d68679ccaf79e8c0ade"),
    ];
    for len in 0x2DC..0x2E4 {
        let mut payload = vec![0x11u8; len];
        payload[0x2D8..0x2DC].fill(0); // race-day state word (360-side offset)
        let data = craft(GAMEPLAY_ID, &payload);
        if let Some(&(_, fixture, output)) = pinned.iter().find(|p| p.0 == len) {
            assert_eq!(md5(&data), fixture, "fixture drift at {len:#x}");
            let pc = convert_crafted(&data)
                .unwrap_or_else(|e| panic!("zero-state payload {len:#x} must convert: {e}"));
            assert_eq!(
                md5(&pc.to_bytes().unwrap()),
                output,
                "converted output at {len:#x}"
            );
        } else {
            let err = convert_crafted(&data)
                .expect_err("unaligned gameplay payload {len:#x} must be refused");
            assert!(
                err.to_string().contains("word-aligned"),
                "{len:#x}: unexpected error: {err}"
            );
        }
    }
}

/// Tree blobs shorter than the count word / magic region error instead of
/// slicing out of range (the Python's unpack_from raises there too).
#[test]
fn short_tree_blobs_error_not_panic() {
    for blob in [&[0u8; 10][..], &[0u8; 20][..]] {
        let err =
            Tree::parse(blob, true).expect_err("short tree blob must be refused, not a panic");
        let msg = err.to_string();
        assert!(
            msg.contains("too short") || msg.contains("magic"),
            "unexpected error: {msg}"
        );
    }
}

/// convert_payload on an MC02 whose file ends mid-tree (total == file length,
/// so the tree slice is 10 bytes) must refuse, not abort.
#[test]
fn convert_payload_on_short_tree_mc02_errors() {
    let mc02 = MC02::parse(&read_container(src_path()).unwrap().payload).unwrap();
    let mut data = mc02.to_bytes().unwrap();
    data.truncate(0x1C + mc02.extra.len() + 10);
    let total = data.len() as u32;
    data[4..8].copy_from_slice(&total.to_be_bytes());
    let short = MC02::parse(&data).unwrap();
    assert_eq!(short.tree.len(), 10);
    let err = convert_crafted(&data).expect_err("short-tree MC02 must be refused");
    assert!(
        err.to_string().contains("too short") || err.to_string().contains("magic"),
        "unexpected error: {err}"
    );
}

/// A short CARDB record inside the real tree converts byte-exactly like the
/// Python (clamped struct fixes, auto mode).
#[test]
fn short_cardb_record_matches_python() {
    let data = craft(CARDB_ID, &(0..=255u8).collect::<Vec<u8>>());
    assert_eq!(
        md5(&data),
        "5fe66a67dffda37ec3e632f52c67b7b0",
        "fixture drift"
    );
    let pc = convert_crafted(&data).unwrap();
    assert_eq!(
        md5(&pc.to_bytes().unwrap()),
        "bdb741bf7b157b60c6f7d327bde5dee1"
    );
}

/// to_pc_record on a payload shorter than the marker word: Python yields
/// four zero bytes.
#[test]
fn to_pc_record_short_payload() {
    let mut rec = Record {
        flags: 0x1234,
        id: 0xABCD,
        size: 2,
        payload: vec![0xAA, 0xBB],
        tail: Vec::new(),
    };
    to_pc_record(&mut rec, [0; 4]);
    assert_eq!(rec.payload, vec![0, 0, 0, 0]);
    assert_eq!(rec.flags, 1);
}

/// rehash_gameplay on a payload shorter than the digest slot: Python's
/// bytearray slice assignment grows the buffer to 0x24 (digest of empty).
#[test]
fn rehash_grows_tiny_gameplay_payload() {
    let mut rec = Record {
        flags: 1,
        id: GAMEPLAY_ID,
        size: 0x20,
        payload: (0..0x20u8).collect(),
        tail: Vec::new(),
    };
    rehash_gameplay(&mut rec);
    assert_eq!(
        common::hex(&rec.payload),
        "000102030405060708090a0b0c0d0e0f10111213d41d8cd98f00b204e9800998ecf8427e"
    );
}

/// A download-truncated CON whose file-table block lands near EOF is
/// refused, not a slice panic.
#[test]
fn truncated_file_table_block_is_refused() {
    let full = std::fs::read(src_path()).unwrap();
    // leave only 0x30 bytes of the file-table block
    let cut = full[..FILE_TABLE_OFF + 0x30].to_vec();
    let err = parse_container(&cut, "cut").unwrap_err();
    assert!(
        err.to_string().contains("file table block truncated"),
        "unexpected error: {err}"
    );
}

/// A crafted 24-bit block count (0xFFFFFF) must be refused, not abort the
/// process on a ~64 GiB allocation.
#[test]
fn huge_block_count_is_refused_not_aborted() {
    let mut data = std::fs::read(src_path()).unwrap();
    assert!(data[FILE_TABLE_OFF..].starts_with(b"CAREER_01"));
    // file-table entry +0x29: LE24 block count
    data[FILE_TABLE_OFF + 0x29..FILE_TABLE_OFF + 0x2C].copy_from_slice(&[0xFF, 0xFF, 0xFF]);
    assert!(parse_container(&data, "crafted").is_err());
}
