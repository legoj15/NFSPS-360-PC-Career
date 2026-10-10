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
use nfssave_core::convert::{ConversionReport, convert_one, convert_payload, write_pc_save};
use nfssave_core::{Error, MC02, parse_container};

use super::sources::{is_con_bytes, is_mc02_bytes};

/// One save selected for conversion.
#[derive(Debug, Clone)]
pub struct SaveInput {
    /// Display/source label used in status lines.
    pub label: String,
    /// Name written as `<root>/<name>/<name>` (from the CON file table, or
    /// the file stem for a raw MC02).
    pub name: String,
    /// Full file bytes (CON container or raw MC02).
    pub bytes: Vec<u8>,
}

impl SaveInput {
    /// Build an input from a file on disk: CON containers take their export
    /// name from the STFS file table, raw MC02 files from the file stem.
    pub fn from_path(path: &Path) -> io::Result<SaveInput> {
        let bytes = fs::read(path)?;
        let label = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let name = if is_con_bytes(&bytes) {
            parse_container(&bytes, &label)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?
                .name
        } else if is_mc02_bytes(&bytes) {
            path.file_stem()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| label.clone())
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "{label}: not a recognized save file \
                     (expected a CON container or a raw MC02)"
                ),
            ));
        };
        Ok(SaveInput { label, name, bytes })
    }

    /// Build an input from a save discovered on a FATX drive scan.
    pub fn from_discovered(save: &DiscoveredSave) -> SaveInput {
        let name = parse_container(&save.bytes, &save.source_path)
            .ok()
            .map(|c| c.name)
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| dirent_name_of(save));
        SaveInput {
            label: save.source_path.clone(),
            name,
            bytes: save.bytes.clone(),
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

/// Folder (next to the export folder) that receives replaced saves.
pub const BACKUP_DIR: &str = "SaveConverter backups";

/// Copies `<out_root>/<name>/<name>` to
/// `<parent of out_root>/SaveConverter backups/<stamp>[-N]/<name>/<name>` when
/// it exists. Returns the backup path, or `None` when there was nothing to keep.
fn back_up_existing(out_root: &Path, name: &str, stamp: &str) -> io::Result<Option<PathBuf>> {
    let existing = out_root.join(name).join(name);
    if !existing.is_file() {
        return Ok(None);
    }
    let root = absolute(out_root)?;
    let base = root.parent().unwrap_or(&root);
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
    /// written for this save.
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
}

impl BatchResult {
    /// The finished-message text, or `None` when nothing was converted.
    pub fn success_message(&self) -> Option<String> {
        self.exported_to
            .as_ref()
            .map(|p| format!("Converted saves exported to {}", p.display()))
    }
}

/// Convert every input into `out_root` (creating it when missing), one
/// result line per save. Failures never abort the batch.
pub fn run_batch(inputs: Vec<SaveInput>, out_root: &Path) -> BatchResult {
    let mut results = Vec::with_capacity(inputs.len());
    let mut any_converted = false;
    if let Err(e) = fs::create_dir_all(out_root) {
        let reason = format!("cannot create output folder {}: {e}", out_root.display());
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
        };
    }
    // Export name (case-insensitive, like the filesystem) -> label of the
    // save that claimed it. A second save with the same name (CAREER_01 on
    // two sticks) would silently overwrite the first.
    let mut claimed: HashMap<String, String> = HashMap::new();
    let stamp = utc_stamp(SystemTime::now());
    for input in inputs {
        // Guard and backup key on the name the converter will write.
        let name = export_name(&input);
        if let Some(owner) = claimed.get(&windows_name_key(&name)) {
            results.push(SaveResult {
                status: SaveStatus::Refused {
                    reason: format!(
                        "another selected save ({owner}) is also named {name}; \
                         converting both would overwrite it - deselect one \
                         and convert it separately"
                    ),
                },
                label: input.label,
            });
            continue;
        }
        // A same-named save already in the folder (from an earlier run, or
        // the user's native PC career) is copied aside first; the copy is
        // left in place, so a failed conversion leaves the game untouched.
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
        let status = match convert_input(&input, out_root) {
            Ok((chunks, mut warnings, target)) => {
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
    BatchResult {
        results,
        exported_to,
    }
}

/// The name the converter writes `<root>/<name>/<name>` under: the CON
/// file-table name for packages (what `convert_one` uses), else
/// `input.name`.
fn export_name(input: &SaveInput) -> String {
    if is_con_bytes(&input.bytes)
        && let Ok(c) = parse_container(&input.bytes, &input.label)
        && !c.name.is_empty()
    {
        return c.name;
    }
    input.name.clone()
}

fn convert_input(
    input: &SaveInput,
    out_root: &Path,
) -> Result<(usize, Vec<String>, PathBuf), Error> {
    if is_con_bytes(&input.bytes) {
        let outcome = convert_one(&input.bytes, &input.label, out_root)?;
        let mut warnings = outcome.report.warnings;
        for prob in &outcome.self_check {
            warnings.push(format!("post-write self-check: {prob}"));
        }
        Ok((outcome.report.records, warnings, outcome.target))
    } else {
        convert_raw_mc02(input, out_root)
    }
}

/// Same contract as `convert_one` for a bare MC02 payload (no CON wrapper,
/// so the export name comes from the input).
fn convert_raw_mc02(
    input: &SaveInput,
    out_root: &Path,
) -> Result<(usize, Vec<String>, PathBuf), Error> {
    let label = &input.label;
    let mc02 = MC02::parse(&input.bytes)?;
    let bad = mc02.check();
    if bad.iter().any(|p| p == "extra CRC mismatch") {
        return Err(Error::Format(format!(
            "{label}: extra-blob CRC mismatch - the source file is corrupted; \
             refusing to convert"
        )));
    }
    let mut report = ConversionReport {
        source: label.clone(),
        ..Default::default()
    };
    for prob in &bad {
        report
            .warnings
            .push(format!("{prob} (CRCs are recomputed on write)"));
    }
    let pc = convert_payload(&mc02, Some(&mut report))?;
    let target = write_pc_save(&pc, &input.name, out_root)?;
    let self_check = MC02::parse(&fs::read(&target)?)?.check();
    for prob in &self_check {
        report
            .warnings
            .push(format!("post-write self-check: {prob}"));
    }
    Ok((report.records, report.warnings, target))
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
