//! Embeds the application manifest (UAC `requireAdministrator`) into the
//! exe's resource section via the Windows SDK resource compiler.
//!
//! The fatx crate measured (SPEC.md §8, 2026-10-05) that a read-only open
//! of `\\.\PhysicalDriveN` fails with ERROR_ACCESS_DENIED unelevated, so the
//! shipped exe must request elevation.
//!
//! Set `NFSPC_NO_MANIFEST=1` to build without embedding (used to run the
//! identical binary for headless smoke tests in unelevated automation).
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
    println!("cargo:rerun-if-env-changed=NFSPC_NO_MANIFEST");

    if env::var_os("NFSPC_NO_MANIFEST").is_some() {
        println!("cargo:warning=NFSPC_NO_MANIFEST set: skipping UAC manifest embedding");
        return;
    }

    let Some(rc) = find_rc_exe() else {
        println!(
            "cargo:warning=rc.exe (Windows SDK) not found: \
             building WITHOUT the requireAdministrator manifest"
        );
        return;
    };

    let crate_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
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

/// Newest `Windows Kits/*/bin/10.0.*/x64/rc.exe` across the usual roots.
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
            let cand = entry.path().join("x64").join("rc.exe");
            if !cand.is_file() {
                continue;
            }
            if best
                .as_ref()
                .is_none_or(|(bv, _)| ver > *bv)
            {
                best = Some((ver, cand));
            }
        }
    }
    best.map(|(_, p)| p)
}
