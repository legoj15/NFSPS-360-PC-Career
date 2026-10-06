//! ProStreet save discovery inside a FATX Data partition.
//!
//! Walks `Content/<profile>/<titleID>/0000000{1,2}/*` and keeps files whose
//! name starts with `CAREER_` / `ALIAS_` (see [`is_save_name`]).

use std::io::{Read, Seek};

use crate::error::Result;
use crate::fatx::{DirEntry, FatxVolume};

/// Root folder scanned inside the Data partition.
pub const CONTENT_ROOT: &str = "Content";
/// Per-title sub-folders that hold saves.
pub const SAVE_TYPE_DIRS: [&str; 2] = ["00000001", "00000002"];
/// Upper bound for a save read whole. Real careers are 823,296 bytes and
/// aliases 81,920; anything near this cap is not a save.
pub const MAX_SAVE_BYTES: u64 = 16 * 1024 * 1024;
/// File-name prefixes that mark a ProStreet save.
pub const NAME_PREFIXES: [&str; 2] = ["CAREER_", "ALIAS_"];

/// One save found on the volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredSave {
    /// Human-friendly per-save name: the STFS file-table name ("CAREER_01")
    /// when the package can be read, otherwise the FATX file name. Never the
    /// CON display name — that is the game title ("NFS ProStreet") on every
    /// save and would label every row identically.
    pub friendly_name: String,
    /// Volume-relative source path, e.g.
    /// `Content/E000.../45410822/00000001/CAREER_01_360`.
    pub source_path: String,
    /// Full file contents.
    pub bytes: Vec<u8>,
}

/// Discovery outcome: the saves found plus the non-fatal problems skipped
/// along the way, so one corrupt file or directory never hides the rest.
#[derive(Debug, Clone, Default)]
pub struct DiscoveryReport {
    /// ProStreet saves found (sorted by source path).
    pub saves: Vec<DiscoveredSave>,
    /// Per-file/per-folder problems that were skipped while scanning.
    pub notes: Vec<String>,
}

/// Scans `volume` for ProStreet saves.
pub fn discover_prostreet_saves<R: Read + Seek>(
    volume: &mut FatxVolume<R>,
) -> Result<Vec<DiscoveredSave>> {
    Ok(discover_prostreet_saves_noted(volume)?.saves)
}

/// Like [`discover_prostreet_saves`], also reporting skipped problems.
///
/// One unreadable file, directory or cluster chain degrades to a note and
/// the scan keeps going: a single corrupt entry on the media must not hide
/// every other save. Only failure of the `Content` root itself aborts
/// (nothing is enumerable then).
pub fn discover_prostreet_saves_noted<R: Read + Seek>(
    volume: &mut FatxVolume<R>,
) -> Result<DiscoveryReport> {
    let mut report = DiscoveryReport::default();
    let content_root = match find_entry(volume, "/", CONTENT_ROOT) {
        Some(entry) => entry,
        None => return Ok(report), // no Content folder: nothing to find
    };
    if !content_root.is_directory() {
        return Ok(report);
    }

    let profiles = volume.list_dir(CONTENT_ROOT)?;
    for profile in profiles {
        if !profile.is_directory() {
            continue;
        }
        let profile_path = format!("{CONTENT_ROOT}/{}", profile.name);
        let titles = match volume.list_dir(&profile_path) {
            Ok(t) => t,
            Err(e) => {
                report.notes.push(format!("{profile_path}: skipped ({e})"));
                continue;
            }
        };
        for title in titles {
            if !title.is_directory() {
                continue;
            }
            let title_path = format!("{profile_path}/{}", title.name);
            for save_type in SAVE_TYPE_DIRS {
                let type_path = format!("{title_path}/{save_type}");
                let files = match volume.list_dir(&type_path) {
                    Ok(files) => files,
                    Err(crate::error::Error::NotFound(_)) => {
                        continue; // absent save-type dir is normal, not a problem
                    }
                    Err(e) => {
                        // unreadable but present: surface it so "no saves
                        // found" always carries a diagnostic
                        report.notes.push(format!("{type_path}: skipped ({e})"));
                        continue;
                    }
                };
                for file in files {
                    if file.is_directory() || file.deleted {
                        continue;
                    }
                    let label = format!("{type_path}/{}", file.name);
                    if !is_save_name(&file.name) {
                        continue;
                    }
                    if u64::from(file.size) > MAX_SAVE_BYTES {
                        report.notes.push(format!(
                            "{label}: skipped ({} bytes is too large for a save)",
                            file.size
                        ));
                        continue;
                    }
                    log::debug!("keeping {label}");
                    let bytes = match volume.read_entry(&file) {
                        Ok(b) => b,
                        Err(e) => {
                            report.notes.push(format!("{label}: skipped ({e})"));
                            continue;
                        }
                    };
                    // per-save name from the STFS file table ("CAREER_01"),
                    // NOT the CON display name — that is the game title
                    // ("NFS ProStreet") on every save. Fall back to the FATX
                    // file name when the package is too damaged to name.
                    let friendly_name =
                        crate::stfs::file_table_name(&bytes).unwrap_or_else(|_| file.name.clone());
                    report.saves.push(DiscoveredSave {
                        friendly_name,
                        source_path: label,
                        bytes,
                    });
                }
            }
        }
    }

    report
        .saves
        .sort_by(|a, b| a.source_path.cmp(&b.source_path));
    Ok(report)
}

/// Saves are recognised by file name alone. The title ID is not enough:
/// ProStreet also stores non-save packages (e.g. `SHADOW_74GR1`, seen on
/// real media 2026-10-05) that fail conversion, and regional ProStreet
/// releases carry other title IDs but the same save names.
pub fn is_save_name(name: &str) -> bool {
    NAME_PREFIXES.iter().any(|p| {
        name.len() >= p.len() && name.as_bytes()[..p.len()].eq_ignore_ascii_case(p.as_bytes())
    })
}

/// Finds a live entry by name inside `dir` (`"/"` for the root).
fn find_entry<R: Read + Seek>(
    volume: &mut FatxVolume<R>,
    dir: &str,
    name: &str,
) -> Option<DirEntry> {
    volume
        .list_dir(dir)
        .ok()?
        .into_iter()
        .find(|e| e.name.eq_ignore_ascii_case(name))
}
