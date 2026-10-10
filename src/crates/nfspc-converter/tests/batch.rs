//! Conversion orchestration: end-to-end CON conversion through the batch
//! runner, corruption refusal (extra-blob CRC mismatch) that writes nothing,
//! and continue-with-the-others behaviour.

use std::fs;
use std::path::{Path, PathBuf};

use nfspc_converter::app::batch::{SaveInput, SaveStatus, run_batch};
use nfssave_core::{MC02, parse_container};
use tempfile::TempDir;

/// The oracle-verified 360 career container (see docs/re/c1_latest).
const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../docs/re/c1_latest/CAREER_01_360"
);

fn fixture_bytes() -> Vec<u8> {
    fs::read(FIXTURE).expect("docs/re/c1_latest/CAREER_01_360 fixture present")
}

/// Raw MC02 bytes of the fixture with one flipped byte inside the extra
/// blob -> stored extra CRC no longer matches (parse-level corruption that
/// `check()` reports as "extra CRC mismatch").
fn corrupted_mc02_bytes() -> Vec<u8> {
    let bytes = fixture_bytes();
    let cont = parse_container(&bytes, "fixture").unwrap();
    let mut raw = cont.payload;
    let extra_off = 0x1C; // header is 0x1C bytes, extra blob follows
    assert!(raw.len() > extra_off + 8);
    raw[extra_off + 2] ^= 0xFF; // career extra is 28 B, well inside
    raw
}

#[test]
fn con_container_converts_end_to_end() {
    let out = TempDir::new().unwrap();
    let input = SaveInput::from_path(Path::new(FIXTURE)).unwrap();
    let save_name = input.name().to_string();

    let batch = run_batch(vec![input], out.path());

    assert_eq!(batch.results.len(), 1);
    let status = &batch.results[0].status;
    match status {
        SaveStatus::Converted {
            chunks,
            warnings,
            target,
        } => {
            assert!(*chunks > 0, "career has multiple chunks");
            let _ = warnings; // informational
            assert_eq!(target, &out.path().join(&save_name).join(&save_name));
            assert!(target.is_file(), "PC save written at <root>/<NAME>/<NAME>");
            assert!(fs::metadata(target).unwrap().len() > 0x100);
            // the written file must self-parse as a little-endian MC02
            let pc = MC02::parse(&fs::read(target).unwrap()).unwrap();
            assert!(pc.check().is_empty(), "self-check clean: {:?}", pc.check());
        }
        other => panic!("expected conversion, got {other:?}"),
    }
    let exported = batch
        .exported_to
        .clone()
        .expect("at least one save converted");
    assert!(exported.is_absolute());
    let message = batch.success_message().unwrap();
    assert!(
        message.starts_with("Converted saves exported to "),
        "finished-message wording: {message}"
    );
    assert!(message.contains(exported.to_string_lossy().as_ref()));
}

#[test]
fn corrupted_save_is_refused_and_nothing_is_written() {
    let out = TempDir::new().unwrap();
    let input = SaveInput::from_bytes("CORRUPT", "CORRUPT", corrupted_mc02_bytes());
    let batch = run_batch(vec![input], out.path());
    assert_eq!(batch.results.len(), 1);
    match &batch.results[0].status {
        SaveStatus::Refused { reason } => assert!(
            reason.contains("extra-blob CRC mismatch") && reason.contains("corrupted"),
            "clear corruption message, got: {reason}"
        ),
        other => panic!("expected refusal, got {other:?}"),
    }
    assert!(!out.path().join("CORRUPT").exists(), "nothing written");
    assert!(batch.exported_to.is_none());
    assert!(batch.success_message().is_none());
}

#[test]
fn corrupted_save_does_not_stop_the_others() {
    let out = TempDir::new().unwrap();
    let good = SaveInput::from_path(Path::new(FIXTURE)).unwrap();
    let bad = SaveInput::from_bytes("CORRUPT", "CORRUPT", corrupted_mc02_bytes());
    let batch = run_batch(vec![bad, good], out.path());
    assert_eq!(batch.results.len(), 2);
    assert!(matches!(
        batch.results[0].status,
        SaveStatus::Refused { .. }
    ));
    assert!(matches!(
        batch.results[1].status,
        SaveStatus::Converted { .. }
    ));
    assert!(!out.path().join("CORRUPT").exists());
    assert!(batch.exported_to.is_some());
}

#[test]
fn raw_mc02_input_converts_with_file_stem_name() {
    let out = TempDir::new().unwrap();
    // the raw payload as stored (to_bytes would recompute the CRCs)
    let input = SaveInput::from_bytes(
        "CAREER_99",
        "CAREER_99",
        parse_container(&fixture_bytes(), "fixture")
            .unwrap()
            .payload,
    );
    let batch = run_batch(vec![input], out.path());
    match &batch.results[0].status {
        SaveStatus::Converted { target, .. } => {
            assert_eq!(target, &out.path().join("CAREER_99").join("CAREER_99"));
            assert!(target.is_file());
        }
        other => panic!("expected conversion, got {other:?}"),
    }
}

#[test]
fn from_path_rejects_non_save_files() {
    let tmp = TempDir::new().unwrap();
    let f = tmp.path().join("junk.bin");
    fs::write(&f, b"not a save at all").unwrap();
    let err = SaveInput::from_path(&f).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
}

/// A discovered save whose CON wrapper cannot be parsed falls back to the
/// FATX file name for the export — never the CON title name, which
/// would label every failing save "NFS ProStreet" and collapse them into
/// one export folder.
#[test]
fn from_discovered_parse_failure_uses_fatx_file_name() {
    use fatx::DiscoveredSave;
    use nfspc_converter::app::batch::dirent_name_of;

    let full = fixture_bytes();
    let save = DiscoveredSave {
        friendly_name: "NFS ProStreet".into(),
        source_path: "Content/E0001A2B3C4D5E6F/45410822/00000001/CAREER_BAD_360".into(),
        bytes: full[..0x2000].to_vec(), // CON magic, truncated before the file table
    };
    let input = SaveInput::from_discovered(&save);
    assert_eq!(input.name(), "CAREER_BAD_360");
    assert_eq!(dirent_name_of(&save), "CAREER_BAD_360");
}

#[test]
fn batch_creates_missing_output_root() {
    let tmp = TempDir::new().unwrap();
    let root: PathBuf = tmp.path().join("deep/ly/missing");
    let input = SaveInput::from_path(Path::new(FIXTURE)).unwrap();
    let name = input.name().to_string();
    let batch = run_batch(vec![input], &root);
    match &batch.results[0].status {
        SaveStatus::Converted { target, .. } => {
            assert_eq!(target, &root.join(&name).join(&name));
            assert!(target.is_file(), "save written through the created root");
        }
        other => panic!("expected conversion, got {other:?}"),
    }
}

/// Two selected saves with the same export name (e.g. CAREER_01 on two
/// different sticks) must not silently overwrite each other: the first
/// converts, the second is refused with a reason naming the conflict, and
/// the written file is the first save's.
#[test]
fn duplicate_export_names_in_one_batch_are_refused() {
    let out = TempDir::new().unwrap();
    let first = SaveInput::from_path(Path::new(FIXTURE)).unwrap();
    // Different content, same name: flip a byte far from the headers.
    let mut bytes = fixture_bytes();
    *bytes.last_mut().unwrap() ^= 0xFF;
    let second = SaveInput::from_bytes("G:/Content/E000/45410822/00000001/CAREER_01", "X", bytes);
    let first_label = first.label.clone();
    let name = first.name().to_string();

    let result = run_batch(vec![first, second], out.path());
    assert_eq!(result.results.len(), 2);
    let target = match &result.results[0].status {
        SaveStatus::Converted { target, .. } => target.clone(),
        other => panic!("first save must convert: {other:?}"),
    };
    match &result.results[1].status {
        SaveStatus::Refused { reason } => {
            assert_eq!(
                reason,
                &format!(
                    "another selected save ({first_label}) is also named {name}; \
                     converting both would overwrite it - convert it separately"
                )
            );
        }
        other => panic!("second save must be refused: {other:?}"),
    }
    let written = fs::read(&target).unwrap();
    let expected = {
        let solo = TempDir::new().unwrap();
        let r = run_batch(
            vec![SaveInput::from_path(Path::new(FIXTURE)).unwrap()],
            solo.path(),
        );
        match &r.results[0].status {
            SaveStatus::Converted { target, .. } => fs::read(target).unwrap(),
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(written, expected, "the first save's output is kept");
}

fn raw_input(name: &str, bytes: Vec<u8>) -> SaveInput {
    SaveInput::from_bytes(format!("pick/{name}"), name, bytes)
}

fn raw_payload() -> Vec<u8> {
    parse_container(&fixture_bytes(), "fixture")
        .unwrap()
        .payload
}

/// Windows folds case and drops trailing dots/spaces, so these names land
/// on the same file and must count as duplicates.
#[test]
fn duplicate_guard_matches_windows_name_folding() {
    for (a, b) in [
        ("CAREER_X", "career_x"),
        ("CAREER_Y", "CAREER_Y. "),
        ("ALIAS_Z", "alias_z."),
    ] {
        let out = TempDir::new().unwrap();
        let r = run_batch(
            vec![raw_input(a, raw_payload()), raw_input(b, raw_payload())],
            out.path(),
        );
        assert!(
            matches!(r.results[0].status, SaveStatus::Converted { .. }),
            "{a}: {:?}",
            r.results[0].status
        );
        assert!(
            matches!(r.results[1].status, SaveStatus::Refused { .. }),
            "{b} must be refused as a duplicate of {a}: {:?}",
            r.results[1].status
        );
    }
}

/// A save that fails to convert does not claim its name: a later good save
/// with the same name still converts.
#[test]
fn failed_save_does_not_claim_its_name() {
    let out = TempDir::new().unwrap();
    let r = run_batch(
        vec![
            raw_input("CAREER_Q", corrupted_mc02_bytes()),
            raw_input("CAREER_Q", raw_payload()),
        ],
        out.path(),
    );
    assert!(matches!(r.results[0].status, SaveStatus::Refused { .. }));
    assert!(
        matches!(r.results[1].status, SaveStatus::Converted { .. }),
        "{:?}",
        r.results[1].status
    );
}

/// Every file under `dir`, recursively.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(files_under(&p));
            } else {
                out.push(p);
            }
        }
    }
    out
}

/// Export folder shaped like the game's: <tmp>/SAVE/NFS ProStreet.
fn game_like_out(tmp: &TempDir) -> PathBuf {
    let out = tmp.path().join("SAVE").join("NFS ProStreet");
    fs::create_dir_all(&out).unwrap();
    out
}

/// A plain output folder (not named `NFS ProStreet`, e.g. headless
/// `--out D:\out`) keeps its backups inside itself, like the scripts'
/// case 4 (docs/scripts-cli.md): never write outside the chosen folder.
#[test]
fn plain_out_folder_keeps_backups_inside_itself() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("out");
    let input = SaveInput::from_path(Path::new(FIXTURE)).unwrap();
    let existing = out.join(input.name()).join(input.name());
    fs::create_dir_all(existing.parent().unwrap()).unwrap();
    fs::write(&existing, b"earlier export").unwrap();

    let r = run_batch(vec![input], &out);
    assert!(
        matches!(r.results[0].status, SaveStatus::Converted { .. }),
        "{:?}",
        r.results[0].status
    );
    let backups = files_under(&out.join("SaveConverter backups"));
    assert_eq!(backups.len(), 1, "{backups:?}");
    assert_eq!(fs::read(&backups[0]).unwrap(), b"earlier export");
    assert!(!tmp.path().join("SaveConverter backups").exists());
}

#[test]
fn existing_export_is_backed_up_before_being_replaced() {
    let tmp = TempDir::new().unwrap();
    let out = game_like_out(&tmp);
    let input = SaveInput::from_path(Path::new(FIXTURE)).unwrap();
    let existing = out.join(input.name()).join(input.name());
    fs::create_dir_all(existing.parent().unwrap()).unwrap();
    fs::write(&existing, b"native PC career").unwrap();

    let r = run_batch(vec![input], &out);
    let warnings = match &r.results[0].status {
        SaveStatus::Converted {
            warnings, target, ..
        } => {
            assert_eq!(target, &existing);
            warnings.clone()
        }
        other => panic!("{other:?}"),
    };
    assert_ne!(fs::read(&existing).unwrap(), b"native PC career");

    let backups = files_under(&tmp.path().join("SAVE").join("SaveConverter backups"));
    assert_eq!(backups.len(), 1, "{backups:?}");
    assert_eq!(fs::read(&backups[0]).unwrap(), b"native PC career");
    assert!(backups[0].ends_with(Path::new("CAREER_01").join("CAREER_01")));
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("backed up") && w.contains(&backups[0].display().to_string())),
        "{warnings:?}"
    );
}

#[test]
fn no_backup_folder_when_nothing_is_replaced() {
    let tmp = TempDir::new().unwrap();
    let out = game_like_out(&tmp);
    let r = run_batch(
        vec![SaveInput::from_path(Path::new(FIXTURE)).unwrap()],
        &out,
    );
    assert!(matches!(r.results[0].status, SaveStatus::Converted { .. }));
    assert!(
        !tmp.path()
            .join("SAVE")
            .join("SaveConverter backups")
            .exists()
    );
}

#[test]
fn failed_conversion_leaves_the_existing_save_in_place() {
    let tmp = TempDir::new().unwrap();
    let out = game_like_out(&tmp);
    let existing = out.join("CAREER_Q").join("CAREER_Q");
    fs::create_dir_all(existing.parent().unwrap()).unwrap();
    fs::write(&existing, b"native PC career").unwrap();

    let r = run_batch(vec![raw_input("CAREER_Q", corrupted_mc02_bytes())], &out);
    assert!(matches!(r.results[0].status, SaveStatus::Refused { .. }));
    assert_eq!(fs::read(&existing).unwrap(), b"native PC career");
}

/// The fixture container with one flipped byte inside its extra blob.
fn corrupted_con_bytes() -> Vec<u8> {
    let mut bytes = fixture_bytes();
    let mc = bytes.windows(4).position(|w| w == b"MC02").unwrap();
    bytes[mc + 0x1C + 2] ^= 0xFF; // MC02 header is 0x1C bytes
    bytes
}

/// A save refused as corrupt is refused before the backup (Python and
/// PowerShell order): the existing save stays and no backup copy is made.
#[test]
fn corrupt_save_is_refused_before_any_backup() {
    let name = parse_container(&fixture_bytes(), "fixture").unwrap().name;
    for input in [
        SaveInput::from_bytes("con", name.clone(), corrupted_con_bytes()),
        raw_input(&name, corrupted_mc02_bytes()),
    ] {
        let tmp = TempDir::new().unwrap();
        let out = game_like_out(&tmp);
        let existing = out.join(&name).join(&name);
        fs::create_dir_all(existing.parent().unwrap()).unwrap();
        fs::write(&existing, b"native PC career").unwrap();

        let r = run_batch(vec![input], &out);
        match &r.results[0].status {
            SaveStatus::Refused { reason } => {
                assert!(reason.contains("CRC mismatch"), "{reason}");
                assert!(!reason.contains("backed up"), "{reason}");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(fs::read(&existing).unwrap(), b"native PC career");
        assert!(
            !tmp.path()
                .join("SAVE")
                .join("SaveConverter backups")
                .exists()
        );
    }
}

/// Guard and backup must key on the name the converter actually writes (the
/// CON file-table name), never on the caller's fallback name: the package
/// is parsed once, at construction, and its name wins.
#[test]
fn con_inputs_are_keyed_by_their_package_name() {
    let tmp = TempDir::new().unwrap();
    let out = game_like_out(&tmp);
    let real = SaveInput::from_path(Path::new(FIXTURE)).unwrap();
    let existing = out.join(real.name()).join(real.name());
    fs::create_dir_all(existing.parent().unwrap()).unwrap();
    fs::write(&existing, b"native PC career").unwrap();

    let a = SaveInput::from_bytes("first", "SOMETHING_ELSE", fixture_bytes());
    let b = SaveInput::from_bytes("second", "YET_ANOTHER", fixture_bytes());
    assert_eq!(a.name(), real.name());
    assert_eq!(b.name(), real.name());
    let r = run_batch(vec![a, b], &out);

    assert!(matches!(r.results[0].status, SaveStatus::Converted { .. }));
    assert!(
        matches!(r.results[1].status, SaveStatus::Refused { .. }),
        "same package name must be a duplicate: {:?}",
        r.results[1].status
    );
    let backups = files_under(&tmp.path().join("SAVE").join("SaveConverter backups"));
    assert_eq!(backups.len(), 1, "native save must be backed up");
    assert_eq!(fs::read(&backups[0]).unwrap(), b"native PC career");
}

#[test]
fn backup_failure_refuses_cleanly_and_keeps_the_original() {
    let tmp = TempDir::new().unwrap();
    let out = game_like_out(&tmp);
    let input = SaveInput::from_path(Path::new(FIXTURE)).unwrap();
    let existing = out.join(input.name()).join(input.name());
    fs::create_dir_all(existing.parent().unwrap()).unwrap();
    fs::write(&existing, b"native PC career").unwrap();
    // A FILE where the backup folder should go makes the backup fail.
    fs::write(tmp.path().join("SAVE").join("SaveConverter backups"), b"x").unwrap();

    let r = run_batch(vec![input], &out);
    match &r.results[0].status {
        SaveStatus::Refused { reason } => {
            assert!(reason.contains("left it untouched"), "{reason}");
            assert!(!reason.contains("  "), "stray spacing: {reason:?}");
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(fs::read(&existing).unwrap(), b"native PC career");
}

/// The fixture with its STFS file-table name rewritten to `bad` (same length,
/// so the container still parses).
fn fixture_with_name(bad: &str) -> Vec<u8> {
    let mut bytes = fixture_bytes();
    let old = parse_container(&bytes, "fixture").unwrap().name;
    assert_eq!(bad.len(), old.len());
    let mut needle = old.as_bytes().to_vec();
    needle.push(0);
    let at = bytes
        .windows(needle.len())
        .position(|w| w == needle.as_slice())
        .expect("STFS name in fixture");
    bytes[at..at + bad.len()].copy_from_slice(bad.as_bytes());
    assert_eq!(parse_container(&bytes, "bad").unwrap().name, bad);
    bytes
}

/// An unsafe container name is refused before anything touches disk: no
/// backup folder, nothing under the output root.
#[test]
fn unsafe_container_name_is_refused_without_backup_or_output() {
    let len = parse_container(&fixture_bytes(), "fixture")
        .unwrap()
        .name
        .len();
    for bad in ["/".repeat(len), ".".repeat(len)] {
        let tmp = TempDir::new().unwrap();
        let out = tmp.path().join("NFS ProStreet");
        let input = SaveInput::from_bytes("BAD", "BAD", fixture_with_name(&bad));
        let batch = run_batch(vec![input], &out);
        match &batch.results[0].status {
            SaveStatus::Refused { reason } => {
                assert!(
                    reason.contains(&format!("unsafe save name '{bad}'")),
                    "{reason}"
                );
            }
            other => panic!("expected refusal, got {other:?}"),
        }
        assert!(!tmp.path().join("SaveConverter backups").exists());
        assert!(!out.exists(), "a refused-only batch creates nothing");
    }
}

/// The anonymized 360 alias container (docs/re/alias_anon).
const ALIAS_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../docs/re/alias_anon/ALIAS_360"
);

const STRAY_ALIAS: &str = "the save folder also holds {} next to the converted alias; a \
     second alias usually means the game once fell back to a default profile - move the \
     one you do not play out of the folder";
const STRAY_CAREER: &str = "the save folder holds {}; a CAREER_ save with a non-ASCII name \
     is usually left over from the game falling back to a default profile - move it out \
     unless you made it";

/// `<out>/<name>/<name>` with dummy bytes.
fn seed(out: &Path, name: &str) {
    fs::create_dir_all(out.join(name)).unwrap();
    fs::write(out.join(name).join(name), b"x").unwrap();
}

/// Game folder holding a stray default-profile alias, a CAREER_ with a
/// non-ASCII name, an ordinary career, and an empty ALIAS_ folder (no save
/// file inside, so not a save).
fn stray_game_folder(tmp: &TempDir) -> PathBuf {
    let out = game_like_out(tmp);
    seed(&out, "ALIAS_Player");
    seed(&out, "CAREER_\u{aa}\u{aa}");
    seed(&out, "CAREER_07");
    fs::create_dir_all(out.join("ALIAS_Empty")).unwrap();
    out
}

/// Same notes as the scripts (tests/test_cli.py, Run-Tests.ps1).
#[test]
fn stray_saves_are_noted_next_to_a_converted_alias() {
    let tmp = TempDir::new().unwrap();
    let out = stray_game_folder(&tmp);
    let r = run_batch(
        vec![SaveInput::from_path(Path::new(ALIAS_FIXTURE)).unwrap()],
        &out,
    );
    assert!(
        matches!(r.results[0].status, SaveStatus::Converted { .. }),
        "{:?}",
        r.results[0].status
    );
    assert_eq!(
        r.notes,
        [
            STRAY_ALIAS.replace("{}", "ALIAS_Player"),
            STRAY_CAREER.replace("{}", "CAREER_\u{aa}\u{aa}"),
        ]
    );
}

#[test]
fn career_only_batch_notes_only_odd_careers() {
    let tmp = TempDir::new().unwrap();
    let out = stray_game_folder(&tmp);
    let r = run_batch(
        vec![SaveInput::from_path(Path::new(FIXTURE)).unwrap()],
        &out,
    );
    assert_eq!(r.notes, [STRAY_CAREER.replace("{}", "CAREER_\u{aa}\u{aa}")]);
}

/// Headless `--out D` into a plain folder: not the game's, so no notes.
#[test]
fn plain_out_folder_gets_no_stray_notes() {
    let tmp = TempDir::new().unwrap();
    let out = tmp.path().join("plain");
    seed(&out, "ALIAS_Player");
    seed(&out, "CAREER_\u{aa}");
    let r = run_batch(
        vec![SaveInput::from_path(Path::new(ALIAS_FIXTURE)).unwrap()],
        &out,
    );
    assert!(r.notes.is_empty(), "{:?}", r.notes);
}
