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
