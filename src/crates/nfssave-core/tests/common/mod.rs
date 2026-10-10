//! Shared helpers for the ported Python test suite.
//!
//! Oracles resolve relative to this crate via `../../..` (the repo root),
//! mirroring `ROOT = Path(__file__).parent.parent` in the Python tests.
// each test binary uses a different subset of these helpers
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("repo root reachable")
}

pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

pub fn unhex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2));
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

static TMP_COUNTER: AtomicU32 = AtomicU32::new(0);

pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> TempDir {
        let n = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("nfssave-core-{tag}-{}-{n}", std::process::id()));
        fs::create_dir_all(&p).expect("create temp dir");
        TempDir(p)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Oracle career the gap fixtures are built from (repo-relative).
pub const GAP_SRC: &str = "docs/re/c1_latest/CAREER_01_360";

/// `(gapped, pristine)` MC02 payloads built from the tracked oracle: the
/// pristine one is a clean rebuild of the (optionally last-record
/// duplicated) tree; the gapped one has every record from index
/// `first_damaged` (default: the last) overwritten with 0xAA noise, so its
/// tree parses with a trailing `gap` (the console's record-tail damage).
pub fn build_gapped(duplicate_last: bool, first_damaged: Option<usize>) -> (Vec<u8>, Vec<u8>) {
    use nfssave_core::mc02::Endian;
    use nfssave_core::tree::Tree;
    use nfssave_core::{MC02, read_container};

    let mc02 = MC02::parse(&read_container(repo_root().join(GAP_SRC)).unwrap().payload).unwrap();
    let mut tree = Tree::parse(&mc02.tree, true).unwrap();
    if duplicate_last {
        let dup = tree.records[tree.records.len() - 1].clone();
        tree.records.insert(tree.records.len() - 1, dup);
    }
    let pristine = MC02::new(
        Endian::Big,
        mc02.extra.clone(),
        tree.build(true, mc02.tree_size as usize).unwrap(),
        mc02.tree_size,
    )
    .to_bytes()
    .unwrap();

    let mut gapped_tree = tree.build(true, mc02.tree_size as usize).unwrap();
    let k = first_damaged.unwrap_or(tree.records.len() - 1);
    let off: usize = 0x48
        + tree.records[..k]
            .iter()
            .map(|r| 12 + r.payload.len())
            .sum::<usize>();
    let span: usize = tree.records[k..].iter().map(|r| 12 + r.payload.len()).sum();
    gapped_tree[off..off + span].fill(0xAA);
    let gapped = MC02::new(Endian::Big, mc02.extra.clone(), gapped_tree, mc02.tree_size)
        .to_bytes()
        .unwrap();

    // sanity: the gapped tree really carries trailing damage
    let parsed = Tree::parse(&MC02::parse(&gapped).unwrap().tree, true).unwrap();
    assert_eq!(parsed.gap, span, "fixture must parse with a trailing gap");
    (gapped, pristine)
}
