//! Conversion orchestration ('Convert').
//!
//! Runs each selected save through the nfssave-core pipeline and produces
//! per-save status lines. A corrupted source (extra-blob CRC mismatch) is
//! refused without writing anything and never stops the remaining saves.
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf, absolute};

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
/// file name from the source path. Never the game-title display name
/// ("NFS ProStreet"), which would collapse every failing save into one
/// export folder.
pub fn dirent_name_of(save: &DiscoveredSave) -> String {
    let dirent = save.source_path.rsplit('/').next().unwrap_or("");
    safe_name(dirent)
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
    for input in inputs {
        if let Some(owner) = claimed.get(&windows_name_key(&input.name)) {
            results.push(SaveResult {
                status: SaveStatus::Refused {
                    reason: format!(
                        "another selected save ({owner}) is also named {}; \
                         converting both would overwrite it - deselect one \
                         and convert it separately",
                        input.name
                    ),
                },
                label: input.label,
            });
            continue;
        }
        let status = match convert_input(&input, out_root) {
            Ok((chunks, warnings, target)) => {
                any_converted = true;
                claimed.insert(windows_name_key(&input.name), input.label.clone());
                SaveStatus::Converted {
                    chunks,
                    warnings,
                    target,
                }
            }
            Err(e) => SaveStatus::Refused {
                reason: e.to_string(),
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

fn convert_input(
    input: &SaveInput,
    out_root: &Path,
) -> Result<(usize, Vec<String>, PathBuf), Error> {
    if is_con_bytes(&input.bytes) {
        let outcome = convert_one(&input.bytes, &input.label, out_root, None)?;
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
    let pc = convert_payload(&mc02, Some(&mut report), None)?;
    let target = write_pc_save(&pc, &input.name, out_root)?;
    let self_check = MC02::parse(&fs::read(&target)?)?.check();
    for prob in &self_check {
        report
            .warnings
            .push(format!("post-write self-check: {prob}"));
    }
    Ok((report.records, report.warnings, target))
}
