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

/// `--out R` resolves the save folder like the scripts (docs/scripts-cli.md
/// "Output folder"): `R/SAVE/NFS ProStreet`, else `R/NFS ProStreet`, else R
/// itself; folder names match case-insensitively.
#[test]
fn out_resolves_the_save_folder_like_the_scripts() {
    for (layout, expect) in [
        (Some("SAVE/NFS ProStreet"), "SAVE/NFS ProStreet"),
        (Some("save/nfs prostreet"), "save/nfs prostreet"),
        (Some("NFS ProStreet"), "NFS ProStreet"),
        (None, ""),
    ] {
        let src = TempDir::new().unwrap();
        let tmp = TempDir::new().unwrap();
        let out = tmp.path().join("R");
        fs::copy(FIXTURE, src.path().join("CAREER_01_360")).unwrap();
        if let Some(l) = layout {
            fs::create_dir_all(out.join(l)).unwrap();
        }
        assert_eq!(
            headless::run(src.path(), &out),
            ExitCode::SUCCESS,
            "{layout:?}"
        );
        let save_dir = if expect.is_empty() {
            out.clone()
        } else {
            out.join(expect)
        };
        assert!(
            save_dir.join("CAREER_01").join("CAREER_01").is_file(),
            "{layout:?}: save not in {}",
            save_dir.display()
        );
    }
}

/// SAVE/NFS ProStreet wins over a bare NFS ProStreet, like the scripts.
#[test]
fn out_prefers_save_layout() {
    let src = TempDir::new().unwrap();
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("R");
    fs::copy(FIXTURE, src.path().join("CAREER_01_360")).unwrap();
    fs::create_dir_all(out.join("SAVE").join("NFS ProStreet")).unwrap();
    fs::create_dir_all(out.join("NFS ProStreet")).unwrap();
    assert_eq!(headless::run(src.path(), &out), ExitCode::SUCCESS);
    assert!(out.join("SAVE/NFS ProStreet/CAREER_01/CAREER_01").is_file());
    assert!(!out.join("NFS ProStreet/CAREER_01").exists());
}

/// In the game layout a replaced save is backed up beside the save folder
/// (`R/SAVE/SaveConverter backups`), as the scripts do.
#[test]
fn game_layout_backs_up_beside_the_save_folder() {
    let src = TempDir::new().unwrap();
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("R");
    fs::copy(FIXTURE, src.path().join("CAREER_01_360")).unwrap();
    let existing = out.join("SAVE/NFS ProStreet/CAREER_01/CAREER_01");
    fs::create_dir_all(existing.parent().unwrap()).unwrap();
    fs::write(&existing, b"native").unwrap();
    assert_eq!(headless::run(src.path(), &out), ExitCode::SUCCESS);
    let backups = out.join("SAVE").join("SaveConverter backups");
    let stamp = fs::read_dir(&backups)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(
        fs::read(stamp.join("CAREER_01").join("CAREER_01")).unwrap(),
        b"native"
    );
}

/// R itself named `NFS ProStreet` (scripts' case 3): written into R, a
/// replaced save backed up beside it.
#[test]
fn out_named_like_the_save_folder_is_used_as_is() {
    let src = TempDir::new().unwrap();
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("nfs prostreet");
    fs::copy(FIXTURE, src.path().join("CAREER_01_360")).unwrap();
    let existing = out.join("CAREER_01").join("CAREER_01");
    fs::create_dir_all(existing.parent().unwrap()).unwrap();
    fs::write(&existing, b"native").unwrap();
    assert_eq!(headless::run(src.path(), &out), ExitCode::SUCCESS);
    assert_ne!(fs::read(&existing).unwrap(), b"native");
    assert!(tmp.path().join("SaveConverter backups").is_dir());
    assert!(!out.join("SaveConverter backups").exists());
}

/// Like the scripts: a run where every source fails leaves a missing R
/// uncreated.
#[test]
fn failed_run_does_not_create_the_output_folder() {
    let src = TempDir::new().unwrap();
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("R");
    write_truncated(src.path());
    assert_eq!(headless::run(src.path(), &out), ExitCode::FAILURE);
    assert!(!out.exists());
}

/// `--out` naming an existing file is a usage error (exit 2), like the
/// scripts' `--out-root`, and nothing is written.
#[test]
fn out_that_is_a_file_is_a_usage_error() {
    let src = TempDir::new().unwrap();
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("R");
    fs::write(&out, b"x").unwrap();
    fs::copy(FIXTURE, src.path().join("CAREER_01_360")).unwrap();
    assert_eq!(headless::run(src.path(), &out), ExitCode::from(2));
    assert_eq!(fs::read(&out).unwrap(), b"x");
}

/// The `--out` check runs first, like the scripts (which queue a missing
/// source as a per-save error): a missing source plus a file `--out` is
/// still the usage error.
#[test]
fn out_file_beats_a_missing_source() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("R");
    fs::write(&out, b"x").unwrap();
    let gone = tmp.path().join("gone").join("CAREER_01");
    assert_eq!(headless::run(&gone, &out), ExitCode::from(2));
}

/// `--out <game>\SAVE` (scripts' case 2): written to `R/NFS ProStreet`, a
/// replaced save backed up in R (the save folder's parent).
#[test]
fn save_folder_parent_backs_up_into_r() {
    let src = TempDir::new().unwrap();
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("SAVE");
    fs::copy(FIXTURE, src.path().join("CAREER_01_360")).unwrap();
    let existing = out
        .join("NFS ProStreet")
        .join("CAREER_01")
        .join("CAREER_01");
    fs::create_dir_all(existing.parent().unwrap()).unwrap();
    fs::write(&existing, b"native").unwrap();
    assert_eq!(headless::run(src.path(), &out), ExitCode::SUCCESS);
    assert!(out.join("SaveConverter backups").is_dir());
}
