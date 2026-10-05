//! ProStreet save discovery inside a FATX Data partition.
//!
//! Walks `Content/<profile>/<titleID>/0000000{1,2}/*`, reads each file's CON
//! header, and keeps saves whose title ID matches Need for Speed: ProStreet
//! or whose file name starts with `CAREER_` / `ALIAS_`.

use std::io::{Read, Seek};

use crate::error::Result;
use crate::fatx::{DirEntry, FatxVolume};
use crate::stfs::{ConHeader, TITLE_ID_NFS_PROSTREET};

/// Root folder scanned inside the Data partition.
pub const CONTENT_ROOT: &str = "Content";
/// Per-title sub-folders that hold saves.
pub const SAVE_TYPE_DIRS: [&str; 2] = ["00000001", "00000002"];
/// File-name prefixes that force inclusion regardless of title ID.
pub const NAME_PREFIXES: [&str; 2] = ["CAREER_", "ALIAS_"];
/// Title IDs accepted by default (oracle-verified ProStreet id 45410822).
pub const PROSTREET_TITLE_IDS: &[[u8; 4]] = &[TITLE_ID_NFS_PROSTREET];

/// One save found on the volume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredSave {
    /// Human-friendly name: the CON display name when available, otherwise
    /// the file name.
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

/// Scans `volume` for ProStreet saves using the default title-ID set.
pub fn discover_prostreet_saves<R: Read + Seek>(
    volume: &mut FatxVolume<R>,
) -> Result<Vec<DiscoveredSave>> {
    Ok(discover_prostreet_saves_noted(volume, &[])?.saves)
}

/// Scans `volume` for saves, additionally accepting `extra_title_ids`
/// (e.g. regional ProStreet variants such as 45418827).
pub fn discover_prostreet_saves_with<R: Read + Seek>(
    volume: &mut FatxVolume<R>,
    extra_title_ids: &[[u8; 4]],
) -> Result<Vec<DiscoveredSave>> {
    Ok(discover_prostreet_saves_noted(volume, extra_title_ids)?.saves)
}

/// Like [`discover_prostreet_saves_with`], also reporting skipped problems.
///
/// One unreadable file, directory or cluster chain degrades to a note and
/// the scan keeps going: a single corrupt entry on the media must not hide
/// every other save. Only failure of the `Content` root itself aborts
/// (nothing is enumerable then).
pub fn discover_prostreet_saves_noted<R: Read + Seek>(
    volume: &mut FatxVolume<R>,
    extra_title_ids: &[[u8; 4]],
) -> Result<DiscoveryReport> {
    let mut accepted: Vec<[u8; 4]> = PROSTREET_TITLE_IDS.to_vec();
    accepted.extend_from_slice(extra_title_ids);

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
                let Ok(files) = volume.list_dir(&type_path) else {
                    continue; // most titles only have one of the two
                };
                for file in files {
                    if file.is_directory() || file.deleted {
                        continue;
                    }
                    let label = format!("{type_path}/{}", file.name);
                    let keep = match should_keep(volume, &file, &accepted) {
                        Ok(k) => k,
                        Err(e) => {
                            report.notes.push(format!("{label}: skipped ({e})"));
                            continue;
                        }
                    };
                    if !keep {
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
                    let con = ConHeader::parse(&bytes).ok();
                    let friendly_name = con
                        .as_ref()
                        .map(|c| c.display_name.trim().to_string())
                        .filter(|name| !name.is_empty())
                        .unwrap_or_else(|| file.name.clone());
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

/// Name-prefix filter and CON-title filter, evaluated without reading the
/// whole file (the CON header fits in the first 0x1711 bytes).
fn should_keep<R: Read + Seek>(
    volume: &mut FatxVolume<R>,
    file: &DirEntry,
    accepted: &[[u8; 4]],
) -> Result<bool> {
    if NAME_PREFIXES.iter().any(|p| file.name.starts_with(p)) {
        return Ok(true);
    }
    let header_len = crate::stfs::DISPLAY_NAME_OFFSET + crate::stfs::DISPLAY_NAME_LEN;
    if (file.size as usize) < header_len {
        return Ok(false); // too small to carry a CON header
    }
    let header = volume.read_entry_partial(file, header_len)?;
    match ConHeader::parse(&header) {
        Ok(con) => Ok(con.title_id_matches(accepted)),
        Err(_) => Ok(false),
    }
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
