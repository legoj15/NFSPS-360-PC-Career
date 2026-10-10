//! One save-name rule across the Python, Rust and PowerShell ports
//! (docs/scripts-cli.md): same vectors, same message.

use nfssave_core::convert::check_save_name;

#[test]
fn unsafe_names_are_refused_with_the_shared_message() {
    for bad in ["", "...", "  ", ". .", ".", "..", "a/b", "a\\b", "C:x"] {
        let e = check_save_name(bad).unwrap_err().to_string();
        assert!(
            e.contains(&format!("unsafe save name '{bad}'")),
            "{bad:?} -> {e}"
        );
    }
}

#[test]
fn plain_names_are_accepted() {
    for good in ["CAREER_01", "A.", "a b", ".x"] {
        check_save_name(good).unwrap();
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
