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
