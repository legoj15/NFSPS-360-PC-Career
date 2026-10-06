//! Command-line parsing (moved out of main.rs so it is testable; the bin
//! target has `test = false`).

use std::path::PathBuf;

use nfspc_converter::app::cli::{Cli, SCAN_FATX_FLAG, parse_args};

fn args(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

#[test]
fn no_args_is_plain_gui() {
    assert_eq!(parse_args(&[]).unwrap(), Cli::Gui { scan_fatx: false });
}

#[test]
fn scan_fatx_flag_starts_gui_in_fatx_mode() {
    assert_eq!(SCAN_FATX_FLAG, "--scan-fatx");
    assert_eq!(
        parse_args(&args(&["--scan-fatx"])).unwrap(),
        Cli::Gui { scan_fatx: true }
    );
}

#[test]
fn convert_mode_still_parses() {
    assert_eq!(
        parse_args(&args(&["--convert", "F:\\", "--out", "o"])).unwrap(),
        Cli::Convert {
            src: PathBuf::from("F:\\"),
            out: PathBuf::from("o"),
        }
    );
}

#[test]
fn scan_fatx_is_gui_only() {
    assert!(parse_args(&args(&["--scan-fatx", "--convert", "a", "--out", "b"])).is_err());
}

#[test]
fn errors_are_kept() {
    assert!(parse_args(&args(&["--out", "o"])).is_err());
    assert!(parse_args(&args(&["--convert", "a"])).is_err());
    assert!(parse_args(&args(&["--bogus"])).is_err());
    assert!(parse_args(&args(&["--convert"])).is_err());
}

#[test]
fn help_is_a_variant_not_an_exit() {
    assert_eq!(parse_args(&args(&["--help"])).unwrap(), Cli::Help);
}

#[test]
fn fatx_button_scans_in_place_only_when_elevated() {
    use nfspc_converter::app::elevation::{FatxAction, fatx_action};
    assert_eq!(fatx_action(true), FatxAction::ScanHere);
    assert_eq!(fatx_action(false), FatxAction::Relaunch);
}
