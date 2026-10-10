//! Port of tests/test_customraceday.py: CustomRaceDayMemcard (0xD548266C) is a
//! property-node stream [u32 0][u32 len][flag word][data] ... The string
//! heuristic used to keep the [0][len] header after a race-day NAME in 360
//! byte order, so the PC read a length of 0x04000000, dropped the event list
//! and the Race Day menu crashed on the empty slot (nfs.exe 0x7F6480). Also
//! covers the per-record tail words (the 360 stores a record's final value in
//! the next record's header slot).

mod common;

use std::path::PathBuf;

use nfssave_core::convert::{ConversionReport, convert_payload};
use nfssave_core::tree::Tree;
use nfssave_core::{MC02, read_container};

const CRD: u32 = 0xD548266C;
const GAMEPLAY: u32 = 0x3B309E09;

fn c1() -> PathBuf {
    common::repo_root().join("docs/re/c1_latest/CAREER_01_360")
}

fn pc_custom() -> PathBuf {
    common::repo_root().join("docs/re/pair_customrd/CAREER_aa_after_pc")
}

fn rd(p: &[u8], o: usize, big: bool) -> u32 {
    let w: [u8; 4] = p[o..o + 4].try_into().unwrap();
    if big {
        u32::from_be_bytes(w)
    } else {
        u32::from_le_bytes(w)
    }
}

/// Node data in order. The first value sits at `start` (4 bytes); every later
/// node is [u32 0][u32 len][flag word][data len], headers on the u32 grid
/// after the previous data.
fn walk(p: &[u8], big: bool, start: usize) -> Vec<Vec<u8>> {
    let mut nodes = vec![p[start..start + 4].to_vec()];
    let mut o = start + 4;
    while o + 12 <= p.len() {
        let mut h = (o + 3) & !3;
        let mut found = false;
        while h + 8 <= p.len() {
            let (z, ln) = (rd(p, h, big), rd(p, h + 4, big));
            if z == 0 && 0 < ln && ln <= 0x400 {
                found = true;
                break;
            }
            h += 4;
        }
        if !found {
            break;
        }
        let ln = rd(p, h + 4, big) as usize;
        let d = h + 12;
        if d + ln > p.len() {
            break;
        }
        nodes.push(p[d..d + ln].to_vec());
        o = d + ln;
    }
    nodes
}

#[derive(Debug, PartialEq)]
enum Node {
    U32(u32),
    Text(Vec<u8>),
}

/// u32 nodes as ints, string nodes as their text (first 4 bytes are junk).
fn norm(nodes: &[Vec<u8>], big: bool) -> Vec<Node> {
    nodes
        .iter()
        .map(|n| {
            if n.len() == 4 {
                Node::U32(rd(n, 0, big))
            } else {
                Node::Text(n[4..].split(|&b| b == 0).next().unwrap().to_vec())
            }
        })
        .collect()
}

fn text(s: &str) -> Node {
    Node::Text(s.as_bytes().to_vec())
}

fn crd(tree: &Tree) -> Vec<u8> {
    tree.records
        .iter()
        .find(|r| r.id == CRD)
        .unwrap()
        .payload
        .clone()
}

fn tree360() -> Tree {
    Tree::parse(
        &MC02::parse(&read_container(c1()).unwrap().payload)
            .unwrap()
            .tree,
        true,
    )
    .unwrap()
}

fn converted() -> Tree {
    let mc02 = MC02::parse(&read_container(c1()).unwrap().payload).unwrap();
    let mut report = ConversionReport::default();
    let pc = convert_payload(&mc02, Some(&mut report)).unwrap();
    Tree::parse(&pc.tree, false).unwrap()
}

#[test]
fn pc_written_stream_walks() {
    // guards the walker itself against the PC's own output
    if !pc_custom().is_file() {
        eprintln!("skipped: {} absent", pc_custom().display());
        return;
    }
    let raw = std::fs::read(pc_custom()).unwrap();
    let t = Tree::parse(&MC02::parse(&raw).unwrap().tree, false).unwrap();
    let nodes = norm(&walk(&crd(&t), false, 0), false);
    let name = nodes
        .iter()
        .position(|n| *n == text("My Race Day 1"))
        .unwrap();
    assert_eq!(nodes[name + 1], Node::U32(3)); // NumEvents
}

#[test]
fn converted_nodes_match_console() {
    if !c1().is_file() {
        eprintln!("skipped: {} absent", c1().display());
        return;
    }
    let t360 = tree360();
    let i = t360.records.iter().position(|r| r.id == CRD).unwrap();
    // a 360 record's final value lives in the NEXT record's header word
    let mut src = t360.records[i].payload.clone();
    src.extend_from_slice(&t360.records[i + 1].flags.to_be_bytes());
    let want = norm(&walk(&src, true, 4), true); // skip the 360 marker word
    let got = norm(&walk(&crd(&converted()), false, 0), false);
    assert_eq!(want[1..], got[1..]); // [0] is an uninit word
    for name in [
        "My Race Day 12",
        "My Race Day 13",
        "My Race Day 14",
        "My Race Day 15",
    ] {
        assert!(got.contains(&text(name)), "{name}");
    }
    let p = got
        .iter()
        .position(|n| *n == text("My Race Day 13"))
        .unwrap();
    assert_eq!(got[p + 1], Node::U32(4)); // NumEvents
    assert_eq!(*got.last().unwrap(), Node::U32(1)); // last event flag, from the next 360 header
}

/// PC payload = 360 payload[4:] + the 360 word at the next record's header
/// slot (verified: CAREER_01 CustomRaceDayMemcard -> 0x00000001 = last event
/// flag; the PC-written race day ends 01 00 00 00 too).
#[test]
fn tail_words_carried() {
    if !c1().is_file() {
        eprintln!("skipped: {} absent", c1().display());
        return;
    }
    let t360 = tree360();
    let conv = converted();
    for w in t360.records.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        if a.id == GAMEPLAY {
            continue; // GameplayData: trimmed 360 pad
        }
        let pc = &conv.records.iter().find(|r| r.id == a.id).unwrap().payload;
        let tail = &pc[pc.len() - 4..];
        let nat = b.flags.to_be_bytes();
        let rev = [nat[3], nat[2], nat[1], nat[0]];
        assert!(tail == nat || tail == rev, "{:#x}", a.id);
        let n = a.payload.len();
        if n >= 12 && a.payload[n - 12..n - 4] == [0, 0, 0, 0, 0, 0, 0, 4] {
            assert_eq!(tail, rev, "{:#x}: u32 value node swapped", a.id);
        }
    }
}
