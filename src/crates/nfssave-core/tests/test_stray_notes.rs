//! `stray_save_notes` in the core library, beside `check_save_name`
//! (mirroring the Python library, nfssave/convert.py). The wording below is
//! written out here on purpose: importing the production strings would make
//! the pin tautological (same policy as the literals in
//! nfspc-converter/tests/batch.rs and Run-Tests.ps1).

mod common;

use std::fs;
use std::path::{Path, PathBuf};

use common::TempDir;
use nfssave_core::convert::stray_save_notes;

const STRAY_ALIAS: &str = "the save folder also holds {} next to the converted alias; a second alias usually means the game once fell back to a default profile - move the one you do not play out of the folder";
const STRAY_CAREER: &str = "the save folder holds {}; a CAREER_ save with a non-ASCII name is usually left over from the game falling back to a default profile - move it out unless you made it";

/// `<dir>/<name>/<name>` with dummy bytes.
fn seed(out: &Path, name: &str) {
    fs::create_dir_all(out.join(name)).unwrap();
    fs::write(out.join(name).join(name), b"x").unwrap();
}

/// Game save folder holding a stray default-profile alias, a CAREER_ with a
/// non-ASCII name, an ordinary career, and an empty ALIAS_ folder (no save
/// file inside, so not a save).
fn game_folder(tmp: &TempDir) -> PathBuf {
    let out = tmp.path().join("NFS ProStreet");
    seed(&out, "ALIAS_Player");
    seed(&out, "CAREER_\u{aa}\u{aa}");
    seed(&out, "CAREER_07");
    fs::create_dir_all(out.join("ALIAS_Empty")).unwrap();
    out
}

/// A converted alias (a key starting `alias_`) next to a stray alias and an
/// odd-named career reports both notes; the converted key itself is not a
/// stray.
#[test]
fn converted_alias_notes_both_strays() {
    let tmp = TempDir::new("stray-notes");
    let out = game_folder(&tmp);
    assert_eq!(
        stray_save_notes(&out, &["alias_x"]),
        [
            STRAY_ALIAS.replace("{}", "ALIAS_Player"),
            STRAY_CAREER.replace("{}", "CAREER_\u{aa}\u{aa}"),
        ]
    );
}

/// Without a converted alias there is no alias note, only the odd career.
#[test]
fn no_converted_alias_notes_only_the_odd_career() {
    let tmp = TempDir::new("stray-notes");
    let out = game_folder(&tmp);
    assert_eq!(
        stray_save_notes(&out, &[]),
        [STRAY_CAREER.replace("{}", "CAREER_\u{aa}\u{aa}")]
    );
}

/// A folder that cannot be read (here: missing) has no notes.
#[test]
fn a_missing_folder_has_no_notes() {
    let tmp = TempDir::new("stray-notes");
    assert_eq!(
        stray_save_notes(&tmp.path().join("no such dir"), &["alias_x"]),
        Vec::<String>::new()
    );
}
