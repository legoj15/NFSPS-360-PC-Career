//! Manual save selection ('Choose saves manually').
//!
//! * A single picked file must be a CON container (`CON ` magic) or a raw
//!   MC02 save (either endianness).
//! * A picked folder is walked recursively, depth-bounded to [`MAX_DEPTH`],
//!   collecting files named `CAREER_*` / `ALIAS_*` that carry the CON
//!   magic - which also covers an extracted `Content` tree.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

/// Maximum folder depth walked below the picked folder. An extracted dump
/// root needs all five levels: `Content/<profile>/<titleID>/<type>/<file>`.
pub const MAX_DEPTH: u32 = 5;
/// File-name prefixes that mark a save file.
pub const NAME_PREFIXES: [&str; 2] = ["CAREER_", "ALIAS_"];

/// One manually selected save file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualSave {
    /// Absolute path of the file.
    pub path: PathBuf,
    /// Display label (the file name).
    pub label: String,
}

pub fn is_con_bytes(data: &[u8]) -> bool {
    data.starts_with(b"CON ")
}

/// Raw MC02 saves exist in both byte orders (360 big-endian, PC
/// little-endian); the ASCII magic reads reversed in one of them.
pub fn is_mc02_bytes(data: &[u8]) -> bool {
    const MAGIC_BE: [u8; 4] = *b"MC02";
    const MAGIC_LE: [u8; 4] = [0x32, 0x30, 0x43, 0x4D];
    data.get(..4) == Some(&MAGIC_BE) || data.get(..4) == Some(&MAGIC_LE)
}

pub fn is_save_bytes(data: &[u8]) -> bool {
    is_con_bytes(data) || is_mc02_bytes(data)
}

fn file_label(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.display().to_string())
}

fn has_save_prefix(name: &str) -> bool {
    NAME_PREFIXES
        .iter()
        .any(|p| name.len() >= p.len() && name[..p.len()].eq_ignore_ascii_case(p))
}

/// Accepts a single file or a folder and returns the saves found in it.
///
/// A file that is neither a CON container nor a raw MC02 is an error; an
/// empty folder is fine and returns an empty list.
pub fn discover_manual(path: &Path) -> io::Result<Vec<ManualSave>> {
    let meta = fs::metadata(path).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("{}: cannot read ({e})", path.display()),
        )
    })?;
    if meta.is_file() {
        let bytes = fs::read(path)?;
        if !is_save_bytes(&bytes) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{}: not a recognized save file \
                     (expected a CON container or a raw MC02)",
                    path.display()
                ),
            ));
        }
        return Ok(vec![ManualSave {
            path: path.to_path_buf(),
            label: file_label(path),
        }]);
    }
    if meta.is_dir() {
        let mut out = Vec::new();
        walk(path, 1, &mut out);
        out.sort_by(|a, b| a.path.cmp(&b.path));
        return Ok(out);
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{} is neither a file nor a folder", path.display()),
    ))
}

/// Recursive walk; `depth` is the depth of files sitting directly in `dir`.
fn walk(dir: &Path, depth: u32, out: &mut Vec<ManualSave>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for entry in rd.filter_map(|e| e.ok()) {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_symlink() {
            continue; // never follow links: no loops, no escapes
        }
        let path = entry.path();
        if ft.is_dir() {
            walk(&path, depth + 1, out);
        } else if ft.is_file() {
            let name = file_label(&path);
            if !has_save_prefix(&name) {
                continue;
            }
            if has_con_magic(&path) {
                out.push(ManualSave {
                    path,
                    label: name,
                });
            }
        }
    }
}

/// Read the first four bytes and compare against the CON magic.
fn has_con_magic(path: &Path) -> bool {
    let Ok(mut f) = File::open(path) else {
        return false;
    };
    let mut magic = [0u8; 4];
    f.read_exact(&mut magic).is_ok() && magic == *b"CON "
}
