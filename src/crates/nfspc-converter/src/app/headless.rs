//! Headless `--convert <file-or-folder> --out <dir>` pipeline.
//!
//! Mirrors the GUI's and the Python CLI's per-file error handling: one file
//! failing to LOAD reports to stderr and the run continues with the rest —
//! a single unreadable file must not abort the batch before anything
//! converts. Exit code: SUCCESS only when every requested save converted.

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use super::batch::{SaveInput, SaveStatus, run_batch};
use super::destination::resolve_out_root;
use super::sources::discover_manual;

/// Runs the headless conversion. Blocking; prints progress to stdout and
/// problems to stderr.
pub fn run(src: &Path, out: &Path) -> ExitCode {
    // like the scripts' --out-root: an existing non-folder is a usage error,
    // reported before any source problem
    if out.exists() && !out.is_dir() {
        eprintln!("error: --out {} exists and is not a folder", out.display());
        return ExitCode::from(2);
    }
    let manual = match discover_manual(src) {
        Ok(found) if !found.is_empty() => found,
        Ok(_) => {
            eprintln!("no ProStreet saves found under {}", src.display());
            return ExitCode::FAILURE;
        }
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Per-file load errors degrade to stderr lines; the remaining files
    // still convert (the GUI worker records these the same way).
    let mut inputs: Vec<SaveInput> = Vec::new();
    let mut load_failures = 0usize;
    for m in &manual {
        match SaveInput::from_path(&m.path) {
            Ok(input) => inputs.push(input),
            Err(e) => {
                load_failures += 1;
                eprintln!("error: {e}");
            }
        }
    }

    // the scripts' output rule: R/SAVE/NFS ProStreet, R/NFS ProStreet, else
    // R. A missing R is created by the first write, so a run where every
    // save fails leaves nothing behind.
    let (save_dir, is_game) = resolve_out_root(out);
    if is_game {
        println!("[+] game save folder: {}", save_dir.display());
    } else {
        println!("[+] output folder: {}", save_dir.display());
        println!(
            "    (copy the converted folders into the game's SAVE\\NFS ProStreet \
             folder, or rerun with --out <game folder>)"
        );
    }

    let batch = run_batch(inputs, &save_dir);
    let mut failures = load_failures;
    for result in &batch.results {
        match &result.status {
            SaveStatus::Converted {
                chunks,
                warnings,
                target,
            } => {
                println!("[+] {}: {} chunks", result.label, chunks);
                for w in warnings {
                    println!("      ! {w}");
                }
                let _ = writeln!(std::io::stdout(), "      wrote {}", target.display());
            }
            SaveStatus::Refused { reason } => {
                failures += 1;
                eprintln!("[!] FAILED {}: {}", result.label, reason);
            }
        }
    }
    for note in &batch.notes {
        println!("[!] {note}");
    }

    match batch.success_message() {
        Some(msg) => {
            println!("{msg}");
            if failures == 0 {
                ExitCode::SUCCESS
            } else {
                eprintln!("error: {failures} save(s) could not be converted");
                ExitCode::FAILURE
            }
        }
        None => {
            eprintln!("error: no saves were converted");
            ExitCode::FAILURE
        }
    }
}
