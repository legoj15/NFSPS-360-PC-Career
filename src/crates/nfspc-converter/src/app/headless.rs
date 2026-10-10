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
use super::sources::discover_manual;

/// Runs the headless conversion. Blocking; prints progress to stdout and
/// problems to stderr.
pub fn run(src: &Path, out: &Path) -> ExitCode {
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

    if let Err(e) = std::fs::create_dir_all(out) {
        eprintln!("error: cannot create {}: {e}", out.display());
        return ExitCode::FAILURE;
    }

    let batch = run_batch(inputs, out);
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
