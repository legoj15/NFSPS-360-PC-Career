//! NFS ProStreet save format library (Rust port of `scripts/python/nfssave`):
//! 360 STFS container reader, MC02 save files, chunk-tree parse/convert.
//!
//! The Python implementation under `scripts/python` is the verified
//! specification; this port is byte-exact by construction and pinned by the
//! golden md5 regression tests (see `tests/test_golden.rs`).

pub mod container360;
pub mod convert;
pub mod crc;
pub mod mc02;
pub mod payload_rules;
pub mod tree;
pub mod treehash;
pub mod typemap;

pub use container360::{Container360, parse_container, read_container};
pub use crc::crc32_ea;
pub use mc02::{Endian, MC02};

use std::io;

/// Error type for the library: I/O failures plus format violations
/// (the Python raises `ValueError` for those).
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("{0}")]
    Format(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Python `ValueError` equivalent.
pub fn format_err(msg: impl Into<String>) -> Error {
    Error::Format(msg.into())
}
