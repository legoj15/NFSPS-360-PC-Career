//! Headless `--convert` pipeline tests: per-file load failures must not
//! abort the run — the remaining saves convert and the exit code reports
//! the failure.

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use nfspc_converter::app::headless;
use tempfile::TempDir;

/// The oracle-verified 360 career container (see docs/re/c1_latest).
const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../docs/re/c1_latest/CAREER_01_360"
);

/// A CON-magic file truncated well before its STFS file table: the folder
/// walk keeps it (CON magic + CAREER_ prefix) but `SaveInput::from_path`
/// refuses it.
fn write_truncated(folder: &Path) {
    let full = fs::read(FIXTURE).unwrap();
    fs::write(folder.join("CAREER_TRUNC"), &full[..0x2000]).unwrap();
}

#[test]
fn one_bad_file_does_not_abort_the_folder_run() {
    let src = TempDir::new().unwrap();
    let out = TempDir::new().unwrap();
    fs::copy(FIXTURE, src.path().join("CAREER_01_360")).unwrap();
    write_truncated(src.path());

    let code = headless::run(src.path(), out.path());
    assert_eq!(
        code,
        ExitCode::FAILURE,
        "one file failed to load -> failure exit"
    );
    // the good save converted anyway
    assert!(
        out.path().join("CAREER_01").join("CAREER_01").is_file(),
        "the loadable save must still convert"
    );
    assert!(
        !out.path().join("CAREER_TRUNC").exists(),
        "nothing written for the refused file"
    );
}

#[test]
fn clean_folder_run_exits_success() {
    let src = TempDir::new().unwrap();
    let out = TempDir::new().unwrap();
    fs::copy(FIXTURE, src.path().join("CAREER_01_360")).unwrap();

    let code = headless::run(src.path(), out.path());
    assert_eq!(code, ExitCode::SUCCESS);
    assert!(out.path().join("CAREER_01").join("CAREER_01").is_file());
}

#[test]
fn single_unreadable_file_fails_without_writing() {
    let src = TempDir::new().unwrap();
    let out = TempDir::new().unwrap();
    let full = fs::read(FIXTURE).unwrap();
    fs::write(src.path().join("CAREER_TRUNC"), &full[..0x2000]).unwrap();

    let code = headless::run(src.path(), out.path());
    assert_eq!(code, ExitCode::FAILURE);
    assert!(!out.path().join("CAREER_TRUNC").exists());
}

#[test]
fn folder_without_saves_fails() {
    let src = TempDir::new().unwrap();
    let out = TempDir::new().unwrap();
    assert_eq!(headless::run(src.path(), out.path()), ExitCode::FAILURE);
}
