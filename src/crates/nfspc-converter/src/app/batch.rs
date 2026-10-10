//! Conversion orchestration ('Convert').
//!
//! Runs each selected save through the nfssave-core pipeline and produces
//! per-save status lines. A corrupted source (extra-blob CRC mismatch) is
//! refused without writing anything and never stops the remaining saves.
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf, absolute};
use std::time::{SystemTime, UNIX_EPOCH};

use fatx::DiscoveredSave;
use nfssave_core::convert::{PreparedSave, check_save_name, prepare_mc02};
use nfssave_core::{Error, parse_container};

use super::destination::GAME_FOLDER_NAME;
use super::sources::{is_con_bytes, is_mc02_bytes};

/// One save selected for conversion. A CON package is parsed once, here:
/// its file-table name is the export name and only its MC02 payload is kept.
#[derive(Debug, Clone)]
pub struct SaveInput {
    /// Display/source label used in status lines.
    pub label: String,
    /// Name written as `<root>/<name>/<name>` (the CON file-table name, else
    /// the caller's fallback).
    name: String,
    source: Source,
}

#[derive(Debug, Clone)]
enum Source {
    /// MC02 payload of a parsed CON package.
    Con(Vec<u8>),
    /// A CON package that did not parse: refused with this message.
    BadCon(String),
    /// Anything else, converted as a raw MC02.
    Mc02(Vec<u8>),
}

impl SaveInput {
    /// Input from file bytes: a CON package exports under its STFS
    /// file-table name, anything else (a raw MC02, or a package that does
    /// not parse and is refused at conversion) under `fallback_name`.
    pub fn from_bytes(
        label: impl Into<String>,
        fallback_name: impl Into<String>,
        bytes: Vec<u8>,
    ) -> SaveInput {
        let label = label.into();
        let mut name = fallback_name.into();
        let source = if is_con_bytes(&bytes) {
            match parse_container(&bytes, &label) {
                Ok(c) => {
                    name = c.name;
                    Source::Con(c.payload)
                }
                Err(e) => Source::BadCon(e.to_string()),
            }
        } else {
            Source::Mc02(bytes)
        };
        SaveInput {
            label,
            name,
            source,
        }
    }

    /// Build an input from a file on disk: CON containers take their export
    /// name from the STFS file table, raw MC02 files from the file stem. A
    /// package that does not parse is an error here (nothing to select).
    pub fn from_path(path: &Path) -> io::Result<SaveInput> {
        let bytes = fs::read(path)?;
        let label = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        if !is_con_bytes(&bytes) && !is_mc02_bytes(&bytes) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{label}: not a recognized save file \
                     (expected a CON container or a raw MC02)"
                ),
            ));
        }
        let stem = path
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| label.clone());
        let input = SaveInput::from_bytes(label, stem, bytes);
        if let Source::BadCon(e) = &input.source {
            return Err(io::Error::new(io::ErrorKind::InvalidData, e.clone()));
        }
        Ok(input)
    }

    /// Build an input from a save discovered on a FATX drive scan; a package
    /// that does not parse falls back to the FATX file name (and is refused
    /// with the parse error at conversion).
    pub fn from_discovered(save: &DiscoveredSave) -> SaveInput {
        SaveInput::from_bytes(
            save.source_path.clone(),
            dirent_name_of(save),
            save.bytes.clone(),
        )
    }

    /// The export name: `<root>/<name>/<name>`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Convert in memory; nothing touches disk.
    fn prepare(&self) -> Result<PreparedSave, Error> {
        match &self.source {
            Source::Con(payload) | Source::Mc02(payload) => {
                prepare_mc02(payload, &self.label, self.name.clone())
            }
            Source::BadCon(e) => Err(Error::Format(e.clone())),
        }
    }
}

/// Export-name fallback when the CON wrapper cannot be parsed: the FATX
/// file name from the source path. Never the CON title name
/// ("NFS ProStreet"), which would collapse every failing save into one
/// export folder.
pub fn dirent_name_of(save: &DiscoveredSave) -> String {
    let dirent = save.source_path.rsplit('/').next().unwrap_or("");
    safe_name(dirent)
}

/// Folder that receives replaced saves: beside a game save folder (named
/// `NFS ProStreet`), inside any other export folder (see back_up_existing).
pub use nfssave_core::convert::BACKUP_DIR;

/// A game save folder (named `NFS ProStreet`, every GUI destination), as
/// opposed to a plain export folder (headless `--out D`).
fn is_game_folder(root: &Path) -> bool {
    root.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case(GAME_FOLDER_NAME))
}

/// Copies `<out_root>/<name>/<name>` to
/// `<B>/SaveConverter backups/<stamp>[-N]/<name>/<name>` when it exists, B =
/// the parent of a game save folder (named `NFS ProStreet`), else out_root
/// itself (never write outside a plain folder the user chose; scripts' case
/// 4 in docs/scripts-cli.md). Returns the backup path, or `None` when there
/// was nothing to keep.
fn back_up_existing(out_root: &Path, name: &str, stamp: &str) -> io::Result<Option<PathBuf>> {
    let existing = out_root.join(name).join(name);
    if !existing.is_file() {
        return Ok(None);
    }
    let root = absolute(out_root)?;
    let base = match root.parent() {
        Some(p) if is_game_folder(&root) => p,
        _ => &root,
    };
    // Never overwrite an earlier backup: runs inside the same second share a
    // stamp, so fall through to `<stamp>-2`, `<stamp>-3`, ...
    let dest = (1u32..)
        .map(|n| {
            let dir = if n == 1 {
                stamp.to_string()
            } else {
                format!("{stamp}-{n}")
            };
            base.join(BACKUP_DIR).join(dir).join(name).join(name)
        })
        .find(|p| !p.exists())
        .expect("unbounded range always yields a free path");
    fs::create_dir_all(dest.parent().expect("dest has a parent"))?;
    fs::copy(&existing, &dest)?;
    Ok(Some(dest))
}

/// `YYYY-MM-DD_HH-MM-SS` (UTC) for backup folder names.
fn utc_stamp(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (days, rem) = ((secs / 86_400) as i64, secs % 86_400);
    // civil-from-days (H. Hinnant), proleptic Gregorian
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}_{:02}-{:02}-{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// The name Windows actually creates for `name`: case-insensitive, with
/// trailing dots and spaces dropped. Two inputs with the same key write the
/// same file.
fn windows_name_key(name: &str) -> String {
    name.trim_end_matches(['.', ' ']).to_lowercase()
}

/// Keep a friendly name usable as a folder/file name.
fn safe_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c < ' ' => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_end_matches('.');
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        "SAVE".into()
    } else {
        trimmed.into()
    }
}

/// Outcome for one save.
#[derive(Debug, Clone)]
pub enum SaveStatus {
    /// Converted and written; carries the chunk count and warnings.
    Converted {
        chunks: usize,
        warnings: Vec<String>,
        /// The file that was written.
        target: PathBuf,
    },
    /// Refused (corruption, format violation, write failure). Nothing was
    /// written for this save, except when the written file then failed its
    /// self-check: the reason says so and names the backup, if any.
    Refused { reason: String },
}

/// Status line for one save in the batch.
#[derive(Debug, Clone)]
pub struct SaveResult {
    pub label: String,
    pub status: SaveStatus,
}

/// Whole-batch outcome.
#[derive(Debug, Clone, Default)]
pub struct BatchResult {
    pub results: Vec<SaveResult>,
    /// Absolute export root actually used, when at least one save converted.
    pub exported_to: Option<PathBuf>,
    /// Batch-level warnings about the game folder (see [`stray_save_notes`]).
    pub notes: Vec<String>,
}

impl BatchResult {
    /// The finished-message text, or `None` when nothing was converted.
    pub fn success_message(&self) -> Option<String> {
        self.exported_to
            .as_ref()
            .map(|p| format!("Converted saves exported to {}", p.display()))
    }
}

/// Convert every input into `out_root`, one result line per save. A missing
/// `out_root` is created by the first write (like the scripts), so a batch
/// where every save is refused leaves nothing behind. Failures never abort
/// the batch.
pub fn run_batch(inputs: Vec<SaveInput>, out_root: &Path) -> BatchResult {
    let mut results = Vec::with_capacity(inputs.len());
    let mut any_converted = false;
    if out_root.exists() && !out_root.is_dir() {
        let reason = format!(
            "cannot use output folder {}: it exists and is not a folder",
            out_root.display()
        );
        return BatchResult {
            results: inputs
                .into_iter()
                .map(|i| SaveResult {
                    label: i.label,
                    status: SaveStatus::Refused {
                        reason: reason.clone(),
                    },
                })
                .collect(),
            exported_to: None,
            notes: Vec::new(),
        };
    }
    // Export name (case-insensitive, like the filesystem) -> label of the
    // save that claimed it. A second save with the same name (CAREER_01 on
    // two sticks) would silently overwrite the first.
    let mut claimed: HashMap<String, String> = HashMap::new();
    let stamp = utc_stamp(SystemTime::now());
    for input in inputs {
        // Guard and backup key on the name the converter will write.
        let name = input.name().to_string();
        // Name first (before the duplicate check and any backup), like the
        // Python and PowerShell ports.
        if let Err(e) = check_save_name(&name) {
            results.push(SaveResult {
                status: SaveStatus::Refused {
                    reason: e.to_string(),
                },
                label: input.label,
            });
            continue;
        }
        if let Some(owner) = claimed.get(&windows_name_key(&name)) {
            results.push(SaveResult {
                status: SaveStatus::Refused {
                    reason: format!(
                        "another selected save ({owner}) is also named {name}; \
                         converting both would overwrite it - convert it separately"
                    ),
                },
                label: input.label,
            });
            continue;
        }
        // Everything that can refuse the source (corruption, conversion)
        // runs before the backup, so a refused save leaves no backup copy.
        let prepared = match input.prepare() {
            Ok(p) => p,
            Err(e) => {
                results.push(SaveResult {
                    status: SaveStatus::Refused {
                        reason: e.to_string(),
                    },
                    label: input.label,
                });
                continue;
            }
        };
        // A same-named save already in the folder (from an earlier run, or
        // the user's native PC career) is copied aside before the write; the
        // copy is left in place, so a failed write leaves the game untouched.
        // Back up exactly the file the write will replace (prepared.name ==
        // name).
        let backup = match back_up_existing(out_root, &name, &stamp) {
            Ok(b) => b,
            Err(e) => {
                results.push(SaveResult {
                    status: SaveStatus::Refused {
                        reason: format!(
                            "an existing {name} could not be backed up ({e}); \
                             left it untouched"
                        ),
                    },
                    label: input.label,
                });
                continue;
            }
        };
        let backup_note = backup
            .as_ref()
            .map(|b| format!("previous {name} backed up to {}", b.display()));
        let status = match prepared.write(out_root) {
            Ok(outcome) => {
                let target = outcome.target;
                let chunks = outcome.report.records;
                let mut warnings = outcome.report.warnings;
                any_converted = true;
                claimed.insert(windows_name_key(&name), input.label.clone());
                if let Some(note) = backup_note {
                    warnings.push(format!("replaced an existing save; {note}"));
                }
                SaveStatus::Converted {
                    chunks,
                    warnings,
                    target,
                }
            }
            // A failure after the write (post-write self-check) can leave
            // the target replaced, so never hide where the backup went.
            Err(e) => SaveStatus::Refused {
                reason: match backup_note {
                    Some(note) => format!("{e} ({note})"),
                    None => e.to_string(),
                },
            },
        };
        results.push(SaveResult {
            label: input.label,
            status,
        });
    }
    let exported_to =
        any_converted.then(|| absolute(out_root).unwrap_or_else(|_| out_root.to_path_buf()));
    let notes = if is_game_folder(&absolute(out_root).unwrap_or_else(|_| out_root.to_path_buf())) {
        stray_save_notes(out_root, &claimed)
    } else {
        Vec::new()
    };
    BatchResult {
        results,
        exported_to,
        notes,
    }
}

const STRAY_ALIAS: &str = "the save folder also holds {} next to the converted alias; a \
     second alias usually means the game once fell back to a default profile - move the \
     one you do not play out of the folder";
const STRAY_CAREER: &str = "the save folder holds {}; a CAREER_ save with a non-ASCII name \
     is usually left over from the game falling back to a default profile - move it out \
     unless you made it";

/// Warnings for saves in the game's save folder that usually mean the PC
/// once fell back to a default profile (docs/re/FORMAT-NOTES.md): another
/// `ALIAS_*` next to an alias this batch converted, and `CAREER_` saves with a
/// non-ASCII name. `converted` is keyed by [`windows_name_key`]. A save is a
/// `<NAME>/<NAME>` file. Same rule and wording as the Python and PowerShell
/// ports (`stray_save_notes`, `Get-StraySaveNotes`).
fn stray_save_notes(save_dir: &Path, converted: &HashMap<String, String>) -> Vec<String> {
    let Ok(rd) = fs::read_dir(save_dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = rd
        .flatten()
        .filter(|e| e.path().join(e.file_name()).is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let has_prefix = |n: &str, p: &str| n.get(..p.len()).is_some_and(|h| h.eq_ignore_ascii_case(p));
    let mut notes = Vec::new();
    if converted.keys().any(|k| k.starts_with("alias_")) {
        let aliases: Vec<&str> = names
            .iter()
            .map(String::as_str)
            .filter(|n| has_prefix(n, "alias_") && !converted.contains_key(&windows_name_key(n)))
            .collect();
        if !aliases.is_empty() {
            notes.push(STRAY_ALIAS.replace("{}", &aliases.join(", ")));
        }
    }
    let odd: Vec<&str> = names
        .iter()
        .map(String::as_str)
        .filter(|n| has_prefix(n, "career_") && !n.is_ascii())
        .collect();
    if !odd.is_empty() {
        notes.push(STRAY_CAREER.replace("{}", &odd.join(", ")));
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn utc_stamp_matches_reference_dates() {
        for (secs, want) in [
            (0, "1970-01-01_00-00-00"),
            (951_868_799, "2000-02-29_23-59-59"),
            (1_791_248_400, "2026-10-06_01-00-00"),
            (4_107_587_696, "2100-03-01_12-34-56"),
        ] {
            assert_eq!(utc_stamp(UNIX_EPOCH + Duration::from_secs(secs)), want);
        }
    }

    /// Two runs inside the same second share a stamp; the second backup
    /// must not overwrite the first (which may be the native PC save).
    #[test]
    fn same_stamp_backups_never_overwrite_each_other() {
        let tmp = tempfile::TempDir::new().unwrap();
        let out = tmp.path().join("SAVE").join("NFS ProStreet");
        let target = out.join("CAREER_01").join("CAREER_01");
        fs::create_dir_all(target.parent().unwrap()).unwrap();

        fs::write(&target, b"native").unwrap();
        let first = back_up_existing(&out, "CAREER_01", "S").unwrap().unwrap();
        fs::write(&target, b"converted").unwrap();
        let second = back_up_existing(&out, "CAREER_01", "S").unwrap().unwrap();

        assert_ne!(first, second);
        assert_eq!(fs::read(&first).unwrap(), b"native");
        assert_eq!(fs::read(&second).unwrap(), b"converted");
    }

    #[test]
    fn windows_name_key_folds_case_and_trailing_dots_spaces() {
        assert_eq!(windows_name_key("CAREER_01. ."), "career_01");
        assert_eq!(windows_name_key("Alias_X"), "alias_x");
    }
}
