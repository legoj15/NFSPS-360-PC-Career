//! One save-name rule across the Python, Rust and PowerShell ports
//! (docs/scripts-cli.md): same vectors, same message. The vectors live in
//! the shared `tests/save_name_vectors.txt` (see [`shared_vectors`]).

mod common;

use nfssave_core::convert::check_save_name;

/// The shared save-name rule vectors: `tests/save_name_vectors.txt`, one
/// `<bad|ok>\t<name>` per line. '#' comments and blank lines are ignored,
/// the name is everything after the first tab (possibly empty), and a
/// trailing '\r' is trimmed so a CRLF checkout reads the same.
fn shared_vectors() -> (Vec<String>, Vec<String>) {
    let text = std::fs::read_to_string(common::repo_root().join("tests/save_name_vectors.txt"))
        .expect("shared save-name vectors present");
    let (mut bad, mut ok) = (Vec::new(), Vec::new());
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (verdict, name) = line
            .split_once('\t')
            .unwrap_or_else(|| panic!("save-name vector line {line:?} has no tab"));
        match verdict {
            "bad" => bad.push(name.to_string()),
            "ok" => ok.push(name.to_string()),
            v => panic!("unknown verdict {v:?} in save_name_vectors.txt"),
        }
    }
    (bad, ok)
}

#[test]
fn unsafe_names_are_refused_with_the_shared_message() {
    for bad in shared_vectors().0 {
        let e = check_save_name(&bad).unwrap_err().to_string();
        assert!(
            e.contains(&format!("unsafe save name '{bad}'")),
            "{bad:?} -> {e}"
        );
    }
}

#[test]
fn plain_names_are_accepted() {
    for good in shared_vectors().1 {
        check_save_name(&good).unwrap();
    }
}

/// The whole bad list (path separators, control characters, the backup
/// folder's own name, Windows device names) is refused with exactly the
/// shared message. Same vectors as tests/test_cli.py and Run-Tests.ps1 via
/// tests/save_name_vectors.txt.
#[test]
fn non_folder_names_are_refused() {
    for bad in shared_vectors().0 {
        let e = check_save_name(&bad).unwrap_err().to_string();
        assert_eq!(e, format!("unsafe save name '{bad}'"), "{bad:?}");
    }
}

#[test]
fn other_names_are_accepted() {
    for good in shared_vectors().1 {
        check_save_name(&good).unwrap_or_else(|e| panic!("{good:?}: {e}"));
    }
}

/// `convert_one` checks the name before the corruption check (Python
/// convert_one order): an unsafe-named, extra-CRC-corrupt container reports
/// the name.
#[test]
fn convert_one_reports_an_unsafe_name_before_corruption() {
    use nfssave_core::convert::convert_one;
    use nfssave_core::parse_container;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let mut bytes = std::fs::read(root.join("docs/re/c1_latest/CAREER_01_360")).unwrap();
    let old = parse_container(&bytes, "fixture").unwrap().name;
    let bad = "/".repeat(old.len());
    let mut needle = old.into_bytes();
    needle.push(0);
    let at = bytes
        .windows(needle.len())
        .position(|w| w == needle)
        .unwrap();
    bytes[at..at + bad.len()].copy_from_slice(bad.as_bytes());
    // flip a byte inside the extra blob (MC02 header is 0x1C bytes)
    let mc = bytes.windows(4).position(|w| w == b"MC02").unwrap();
    bytes[mc + 0x1C + 2] ^= 0xFF;
    let tmp = std::env::temp_dir().join(format!("nfssave-name-before-crc-{}", std::process::id()));
    let Err(e) = convert_one(&bytes, "bad", &tmp) else {
        panic!("converted an unsafe-named, corrupt save")
    };
    let e = e.to_string();
    assert!(e.contains(&format!("unsafe save name '{bad}'")), "{e}");
    assert!(!tmp.exists());
}
