//! Conversion orchestration: end-to-end CON conversion through the batch
//! runner, corruption refusal (extra-blob CRC mismatch) that writes nothing,
//! and continue-with-the-others behaviour.

use std::fs;
use std::path::{Path, PathBuf};

use nfspc_converter::app::batch::{run_batch, SaveInput, SaveStatus};
use nfssave_core::{parse_container, MC02};
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
    let save_name = input.name.clone();

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
    let input = SaveInput {
        label: "CORRUPT".into(),
        name: "CORRUPT".into(),
        bytes: corrupted_mc02_bytes(),
    };
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
    let bad = SaveInput {
        label: "CORRUPT".into(),
        name: "CORRUPT".into(),
        bytes: corrupted_mc02_bytes(),
    };
    let batch = run_batch(vec![bad, good], out.path());
    assert_eq!(batch.results.len(), 2);
    assert!(matches!(batch.results[0].status, SaveStatus::Refused { .. }));
    assert!(matches!(batch.results[1].status, SaveStatus::Converted { .. }));
    assert!(!out.path().join("CORRUPT").exists());
    assert!(batch.exported_to.is_some());
}

#[test]
fn raw_mc02_input_converts_with_file_stem_name() {
    let out = TempDir::new().unwrap();
    let input = SaveInput {
        label: "CAREER_99".into(),
        name: "CAREER_99".into(),
        // fixture MC02 re-serialized: parse+to_bytes keeps the stored CRCs?
        // to_bytes recomputes them, so use the raw payload directly.
        bytes: {
            let bytes = fixture_bytes();
            parse_container(&bytes, "fixture").unwrap().payload
        },
    };
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

#[test]
fn batch_creates_missing_output_root() {
    let tmp = TempDir::new().unwrap();
    let root: PathBuf = tmp.path().join("deep/ly/missing");
    let input = SaveInput::from_path(Path::new(FIXTURE)).unwrap();
    let name = input.name.clone();
    let batch = run_batch(vec![input], &root);
    match &batch.results[0].status {
        SaveStatus::Converted { target, .. } => {
            assert_eq!(target, &root.join(&name).join(&name));
            assert!(target.is_file(), "save written through the created root");
        }
        other => panic!("expected conversion, got {other:?}"),
    }
}
