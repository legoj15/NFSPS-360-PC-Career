//! Alias option chunks keep their values on PC. Port of
//! tests/test_alias_settings.py (2026-10-09 in-game bug: every on/off option
//! read as 0, VideoSettings larger than the PC savable, last node values
//! zeroed).

mod common;

use std::collections::HashMap;
use std::fs;

use common::repo_root;
use nfssave_core::convert::{ConversionReport, alias_tail, convert_payload, fix_node_flags};
use nfssave_core::tree::Tree;
use nfssave_core::{MC02, read_container};

const PC_CONTROLLER: u32 = 0x3915_6567;

fn be(p: &[u8], o: usize) -> u32 {
    u32::from_be_bytes(p[o..o + 4].try_into().unwrap())
}

/// (data offset, value) of every one-byte node [0][len=1][flag][u8 + 3 pad]
fn u8_nodes(p: &[u8]) -> Vec<(usize, u8)> {
    let mut out = Vec::new();
    let mut o = 8;
    while o + 8 <= p.len() {
        if be(p, o - 8) == 0 && be(p, o - 4) == 1 && p[o] <= 1 && p[o + 5..o + 8] == [0, 0, 0] {
            out.push((o + 4, p[o + 4]));
        }
        o += 4;
    }
    out
}

fn convert_alias() -> (Tree, Tree) {
    let src_path = repo_root().join("docs/re/alias_anon/ALIAS_360");
    let mc02 = MC02::parse(&read_container(&src_path).unwrap().payload).unwrap();
    let src = Tree::parse(&mc02.tree, true).unwrap();
    let mut report = ConversionReport::default();
    let pc = convert_payload(&mc02, Some(&mut report), None).unwrap();
    (src, Tree::parse(&pc.tree, false).unwrap())
}

#[test]
fn one_byte_options_keep_their_value() {
    let (src, pc) = convert_alias();
    let by_id: HashMap<u32, &Vec<u8>> = pc.records.iter().map(|r| (r.id, &r.payload)).collect();
    let mut checked = 0;
    for r in &src.records {
        for (off, value) in u8_nodes(&r.payload[4..]) {
            assert_eq!(
                by_id[&r.id][off], value,
                "chunk {:#x} node at {off:#x}",
                r.id
            );
            checked += 1;
        }
    }
    assert!(checked > 50, "only {checked} u8 nodes checked");
}

#[test]
fn last_node_value_comes_from_the_word_after_the_record() {
    let (src, pc) = convert_alias();
    let tails: HashMap<u32, &Vec<u8>> = src.records.iter().map(|r| (r.id, &r.tail)).collect();
    let by_id: HashMap<u32, &Vec<u8>> = pc.records.iter().map(|r| (r.id, &r.payload)).collect();
    assert_eq!(tails[&0x9CB3_26C2].as_slice(), &[0, 0, 0, 3]);
    let last = |id: u32| by_id[&id][by_id[&id].len() - 4..].to_vec();
    assert_eq!(last(0x9CB3_26C2), vec![3, 0, 0, 0]);
    assert_eq!(last(0x8B7D_0AAD), vec![2, 0, 0, 0]);
    assert_eq!(last(0x9F72_F194), vec![1, 0, 0, 0]);
}

#[test]
fn chunk_sizes_match_native_pc() {
    let (_, pc) = convert_alias();
    let native = fs::read(repo_root().join("docs/re/oracle/native_ALIAS_Player")).unwrap();
    let native = Tree::parse(&MC02::parse(&native).unwrap().tree, false).unwrap();
    let want: HashMap<u32, usize> = native
        .records
        .iter()
        .map(|r| (r.id, r.payload.len()))
        .collect();
    let got: std::collections::HashSet<u32> = pc.records.iter().map(|r| r.id).collect();
    let native_ids: std::collections::HashSet<u32> = want.keys().copied().collect();
    assert_eq!(got, native_ids);
    for r in pc.records.iter().filter(|r| r.id != PC_CONTROLLER) {
        assert_eq!(r.payload.len(), want[&r.id], "chunk {:#x}", r.id);
    }
}

fn swapped(src: &[u8]) -> Vec<u8> {
    src.chunks(4)
        .flat_map(|w| w.iter().rev().copied())
        .collect()
}

fn node(value: u32) -> Vec<u8> {
    [0u32, 1, 0x00FF_FFFF, value]
        .iter()
        .flat_map(|w| w.to_be_bytes())
        .collect()
}

#[test]
fn u32_node_after_zero_is_not_mistaken_for_u8() {
    let src = node(4);
    let mut out = swapped(&src);
    fix_node_flags(&src, &mut out);
    assert_eq!(&out[12..16], &[4, 0, 0, 0]);
}

#[test]
fn alias_tail_rules() {
    let hdr = |ln: u32| -> Vec<u8> {
        [0u32, ln, 0x00FF_FFFF]
            .iter()
            .flat_map(|w| w.to_be_bytes())
            .collect()
    };
    assert_eq!(alias_tail(&hdr(4), &[0, 0, 0, 3]), [3, 0, 0, 0]); // u32 swap
    assert_eq!(alias_tail(&hdr(1), &[1, 0, 0, 0]), [1, 0, 0, 0]); // u8 natural
    assert_eq!(alias_tail(&hdr(1), &[0, 0, 0, 4]), [4, 0, 0, 0]); // pad set: swap
    assert_eq!(alias_tail(&hdr(5), &[0, 0, 0, 3]), [0; 4]); // not a scalar node
    let len0 = [vec![0u8; 8], vec![0xFF; 4]].concat();
    assert_eq!(alias_tail(&len0, &[0, 0, 0, 3]), [0; 4]); // len 0
    assert_eq!(alias_tail(&hdr(4)[4..], &[0, 0, 0, 3]), [0; 4]); // payload < 12
    assert_eq!(alias_tail(&hdr(4), &[0, 3]), [0; 4]); // truncated tail
    // 8-byte node: payload ends [0][8][flag][d1]; the tail is d2
    let eight = [hdr(8), vec![0; 4]].concat();
    assert_eq!(alias_tail(&eight, &[0x3F, 0x80, 0, 0]), [0, 0, 0x80, 0x3F]);
    // header whose flag word is not [u8][FF FF FF | 00 00 00]: not a node
    let junk_flag: Vec<u8> = [0u32, 4, 0x0123_4567]
        .iter()
        .flat_map(|w| w.to_be_bytes())
        .collect();
    assert_eq!(alias_tail(&junk_flag, &[0, 0, 0, 3]), [0; 4]);
}

#[test]
fn u8_rule_needs_a_node_flag_word() {
    let src: Vec<u8> = [0u32, 1, 0x0123_4567, 0x0100_0000]
        .iter()
        .flat_map(|w| w.to_be_bytes())
        .collect();
    let mut out = swapped(&src);
    fix_node_flags(&src, &mut out);
    assert_eq!(&out[12..16], &[0, 0, 0, 1]); // left swapped
}

#[test]
fn u8_node_stays_natural() {
    let src = node(0x0100_0000);
    let mut out = swapped(&src);
    fix_node_flags(&src, &mut out);
    assert_eq!(&out[12..16], &[1, 0, 0, 0]);
}
