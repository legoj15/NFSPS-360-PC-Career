//! Embeds the application manifest into the exe's resource section via the
//! Windows SDK resource compiler.
//!
//! The manifest is `asInvoker`: the exe must stay launchable unelevated so
//! the headless `--convert` mode works in CI/automation. Raw-drive scanning
//! needs elevation (fatx SPEC.md §8), which the app handles at runtime by
//! telling the user to relaunch elevated - not by demanding UAC at launch.
//!
//! If rc.exe cannot be located the build continues without the manifest and
//! emits a loud warning - tests must never require the SDK.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=app.manifest");
    println!("cargo:rerun-if-changed=app.rc");

    let crate_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    // A malformed manifest (e.g. `--` inside an XML comment) compiles fine
    // with rc.exe but makes the exe unlaunchable (WinError 14001, SxS). The
    // loader parses it at runtime, so catch it here instead - even when the
    // SDK is missing and embedding is skipped.
    validate_manifest_comments(&crate_dir.join("app.manifest"))
        .expect("app.manifest comment check failed");

    let Some(rc) = find_rc_exe() else {
        println!(
            "cargo:warning=rc.exe (Windows SDK) not found: \
             building WITHOUT the embedded manifest"
        );
        return;
    };

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let res = out_dir.join("app.res");

    // rc resolves the manifest path relative to its working directory.
    let status = Command::new(&rc)
        .arg(format!("/fo{}", res.display()))
        .arg("app.rc")
        .current_dir(&crate_dir)
        .status()
        .unwrap_or_else(|e| panic!("failed to run {}: {e}", rc.display()));
    assert!(status.success(), "rc.exe failed with {status}");

    // link.exe accepts .res files as direct inputs; the RT_MANIFEST resource
    // with id 1 becomes the exe's embedded manifest. Scoped to bin targets
    // only: test harnesses must stay launchable unelevated (cargo test).
    println!("cargo:rustc-link-arg-bins={}", res.display());
}

/// XML comments must not contain `--` (XML spec); the Windows loader rejects
/// the whole manifest (WinError 14001) when they do. Walk the file segment by
/// segment: comment inner texts and the non-comment content must both be free
/// of `--` (the only legal `--` sequences are the delimiters themselves).
/// Anything deeper (structure, encoding) is left to the loader.
fn validate_manifest_comments(path: &std::path::Path) -> Result<(), String> {
    let text = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut cursor = 0usize;
    while let Some(rel) = text[cursor..].find("<!--") {
        let start = cursor + rel;
        if text[cursor..start].contains("--") {
            return Err(format!(
                "{}: `--` outside a comment at byte {start} is not valid \
                 manifest XML",
                path.display()
            ));
        }
        let Some(len) = text[start + 4..].find("-->") else {
            return Err(format!(
                "{}: unterminated comment at byte {start}",
                path.display()
            ));
        };
        if text[start + 4..start + 4 + len].contains("--") {
            return Err(format!(
                "{}: comment at byte {start} contains `--`, which is illegal \
                 in XML comments and makes Windows reject the manifest",
                path.display()
            ));
        }
        cursor = start + 4 + len + 3;
    }
    if text[cursor..].contains("--") {
        return Err(format!(
            "{}: `--` outside a comment after byte {cursor} is not valid \
             manifest XML",
            path.display()
        ));
    }
    Ok(())
}

/// rc.exe architecture subdirectories to probe, in preference order (x64
/// matches the toolchain; x86/arm64 hosts must not silently lose the
/// manifest just because their SDK lacks the x64 directory).
const RC_ARCHES: [&str; 3] = ["x64", "x86", "arm64"];

/// Newest `Windows Kits/*/bin/10.0.*/<arch>/rc.exe` across the usual roots.
fn find_rc_exe() -> Option<PathBuf> {
    let roots = [
        r"C:\Program Files (x86)\Windows Kits\10\bin".to_string(),
        r"C:\Program Files\Windows Kits\10\bin".to_string(),
    ];
    let mut best: Option<(String, PathBuf)> = None;
    for root in roots {
        let Ok(rd) = fs::read_dir(&root) else {
            continue;
        };
        for entry in rd.filter_map(|e| e.ok()) {
            let ver = entry.file_name().to_string_lossy().into_owned();
            if !ver.starts_with("10.") {
                continue;
            }
            // first arch subdirectory (in preference order) that has rc.exe
            let cand = RC_ARCHES
                .iter()
                .map(|arch| entry.path().join(arch).join("rc.exe"))
                .find(|cand| cand.is_file());
            let Some(cand) = cand else {
                continue;
            };
            if best.as_ref().is_none_or(|(bv, _)| ver > *bv) {
                best = Some((ver, cand));
            }
        }
    }
    best.map(|(_, p)| p)
}
