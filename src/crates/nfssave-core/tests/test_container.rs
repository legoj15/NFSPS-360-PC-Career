//! Port of tests/test_container.py: STFS container reader — payloads must be
//! read through the block map.
//!
//! The CON files are standard STFS packages (two hash-table copies per level).
//! A hash-table group sits after every 170 data blocks, so a career payload
//! (183 blocks) straddles one. Reading the payload as one contiguous slice
//! pulls hash tables into the save and loses FECareer's tail plus the
//! CustomRaceDayMemcard/UnlockSystem chunks — the "damaged console tail".

mod common;

use std::path::PathBuf;

use nfssave_core::container360::stfs_block_offset;
use nfssave_core::tree::Tree;
use nfssave_core::{MC02, read_container};

fn root() -> PathBuf {
    common::repo_root()
}

fn careers() -> [PathBuf; 3] {
    [
        root().join("Extracted/Career/CAREER_01"),
        root().join("docs/re/pair/CAREER_02_360_fresh"),
        root().join("Extracted/Career/CAREER_03"),
    ]
}

/// FECareer, CustomRaceDay, Unlock
const CAREER_TAIL: [u32; 3] = [0x885B4DDC, 0xD548266C, 0xCA269650];

#[test]
fn block_offsets_two_table_package() {
    // first hash table at 0xA000; data block 0 (file table) at 0xC000
    assert_eq!(stfs_block_offset(0, 0xA000, 1), 0xC000);
    assert_eq!(stfs_block_offset(169, 0xA000, 1), 0xB5000);
    // level-0 + level-1 tables (two copies each) precede block 170
    assert_eq!(stfs_block_offset(170, 0xA000, 1), 0xBA000);
    assert_eq!(stfs_block_offset(340, 0xA000, 1), 0x166000);
}

#[test]
fn career_payloads_complete() {
    let mut ran = 0;
    for src in careers() {
        if !src.is_file() {
            eprintln!("skipped (source not present): {}", src.display());
            continue;
        }
        ran += 1;
        let cont = read_container(&src).expect("read container");
        let mc02 = MC02::parse(&cont.payload).expect("parse MC02");
        assert_eq!(
            mc02.check(),
            Vec::<String>::new(),
            "MC02 CRCs must validate for {}",
            src.display()
        );
        let tree = Tree::parse(&mc02.tree, true).expect("parse tree");
        assert_eq!(tree.gap, 0, "unexpected damaged tail for {}", src.display());
        let ids: Vec<u32> = tree.records.iter().map(|r| r.id).collect();
        assert!(
            ids.len() >= 3,
            "expected at least the tail chunks for {}",
            src.display()
        );
        assert_eq!(
            &ids[ids.len() - 3..],
            &CAREER_TAIL,
            "career tail chunks for {}",
            src.display()
        );
    }
    // docs/re/pair/CAREER_02_360_fresh is tracked and always present
    assert!(
        ran >= 1,
        "at least the tracked career oracle must be present"
    );
}

/// The pair CON cut `len` bytes into its file-table block, with the entry's
/// block count and size zeroed (an empty file). Same fixture as
/// tests/test_container.py and Run-Tests.ps1.
fn short_table(len: usize) -> Vec<u8> {
    let mut data = std::fs::read(root().join("docs/re/pair/CAREER_02_360_fresh")).unwrap();
    let header = u32::from_be_bytes(data[0x340..0x344].try_into().unwrap()) as usize;
    let first_table = (header + 0xFFF) & !0xFFF;
    let block = data[0x37E] as usize | (data[0x37F] as usize) << 8 | (data[0x380] as usize) << 16;
    let shift = if data[0x37B] & 1 != 0 { 0 } else { 1 };
    let off = stfs_block_offset(block, first_table, shift);
    data[off + 0x29..off + 0x2C].fill(0);
    data[off + 0x34..off + 0x38].fill(0);
    data.truncate(off + len);
    data
}

#[test]
fn table_block_of_0x3c_bytes_parses() {
    let c = nfssave_core::container360::parse_container(&short_table(0x3C), "t").unwrap();
    assert_eq!(c.name, "CAREER_02");
    assert!(c.payload.is_empty());
}

#[test]
fn table_block_of_0x30_bytes_is_refused() {
    let e = nfssave_core::container360::parse_container(&short_table(0x30), "t").unwrap_err();
    assert!(
        e.to_string().contains("STFS file table block truncated"),
        "{e}"
    );
}
