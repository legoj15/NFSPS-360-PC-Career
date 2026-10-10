//! Destination-resolution matrix (the 'Select location to export saves' rules).
//!
//! P = the folder the user picked:
//!  1. P named exactly 'NFS ProStreet' (case-insensitive) -> P itself.
//!  2. P contains an 'NFS ProStreet' folder one or two levels deep -> that
//!     folder; several matches prefer the one running through a 'SAVE' folder.
//!  3. Otherwise create P/NFS ProStreet and export there.

use std::fs;
use std::path::{Path, PathBuf};

use nfspc_converter::app::destination::{Destination, GAME_FOLDER_NAME, resolve, resolve_dry};
use tempfile::TempDir;

fn mkdirs(base: &Path, rel: &str) -> PathBuf {
    let p = base.join(rel);
    fs::create_dir_all(&p).unwrap();
    p
}

fn dest(root: PathBuf, created: bool) -> Destination {
    Destination { root, created }
}

#[test]
fn rule1_exact_folder_name_is_used_directly() {
    let tmp = TempDir::new().unwrap();
    let picked = mkdirs(tmp.path(), "NFS ProStreet");
    assert_eq!(resolve_dry(&picked), dest(picked.clone(), false));
    // resolve() must not fail or change anything
    assert_eq!(resolve(&picked).unwrap(), dest(picked, false));
}

#[test]
fn rule1_is_case_insensitive() {
    let tmp = TempDir::new().unwrap();
    let picked = mkdirs(tmp.path(), "nfs proStreet");
    assert_eq!(resolve_dry(&picked), dest(picked, false));
}

#[test]
fn rule2_finds_nfs_prostreet_one_level_deep() {
    let tmp = TempDir::new().unwrap();
    let picked = mkdirs(tmp.path(), "picked");
    let game = mkdirs(&picked, GAME_FOLDER_NAME);
    assert_eq!(resolve_dry(&picked), dest(game, false));
}

#[test]
fn rule2_finds_nfs_prostreet_two_levels_deep() {
    let tmp = TempDir::new().unwrap();
    let picked = mkdirs(tmp.path(), "picked");
    let game = mkdirs(&picked, "GAMES/NFS ProStreet");
    assert_eq!(resolve_dry(&picked), dest(game, false));
}

#[test]
fn rule2_community_repack_save_layout() {
    // P/SAVE/NFS ProStreet - the community-repack layout
    let tmp = TempDir::new().unwrap();
    let picked = mkdirs(tmp.path(), "picked");
    let game = mkdirs(&picked, "SAVE/NFS ProStreet");
    assert_eq!(resolve_dry(&picked), dest(game, false));
}

#[test]
fn rule2_prefers_the_path_through_a_save_folder() {
    let tmp = TempDir::new().unwrap();
    let picked = mkdirs(tmp.path(), "picked");
    let plain = mkdirs(&picked, "NFS ProStreet");
    let via_save = mkdirs(&picked, "SAVE/NFS ProStreet");
    assert_eq!(resolve_dry(&picked), dest(via_save, false));
    assert_ne!(resolve_dry(&picked).root, plain);
}

#[test]
fn rule2_save_preference_is_case_insensitive() {
    let tmp = TempDir::new().unwrap();
    let picked = mkdirs(tmp.path(), "picked");
    let via_save = mkdirs(&picked, "save/nfs prostreet");
    mkdirs(&picked, "NFS ProStreet");
    assert_eq!(resolve_dry(&picked), dest(via_save, false));
}

#[test]
fn rule2_without_save_prefers_the_shallowest_match() {
    let tmp = TempDir::new().unwrap();
    let picked = mkdirs(tmp.path(), "picked");
    let shallow = mkdirs(&picked, "NFS ProStreet");
    mkdirs(&picked, "GAMES/NFS ProStreet");
    assert_eq!(resolve_dry(&picked), dest(shallow, false));
}

#[test]
fn rule3_creates_p_nfs_prostreet_when_nothing_matches() {
    let tmp = TempDir::new().unwrap();
    let picked = mkdirs(tmp.path(), "picked");
    let expected = picked.join(GAME_FOLDER_NAME);
    assert_eq!(resolve_dry(&picked), dest(expected.clone(), true));
    assert!(
        !expected.exists(),
        "dry resolution must not create anything"
    );

    let got = resolve(&picked).unwrap();
    assert_eq!(got, dest(expected.clone(), true));
    assert!(expected.is_dir(), "resolve() creates the missing folder");
}

#[test]
fn rule3_applies_when_match_is_too_deep() {
    // three levels below P is beyond the one-or-two-level search
    let tmp = TempDir::new().unwrap();
    let picked = mkdirs(tmp.path(), "picked");
    mkdirs(&picked, "a/b/NFS ProStreet");
    assert_eq!(
        resolve_dry(&picked),
        dest(picked.join(GAME_FOLDER_NAME), true)
    );
}

#[test]
fn resolved_root_is_absolute_even_for_relative_input() {
    let tmp = TempDir::new().unwrap();
    let picked = mkdirs(tmp.path(), "NFS ProStreet");
    // resolve via a relative path (cwd changed for the duration of the test)
    let cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(tmp.path()).unwrap();
    let got = resolve_dry(Path::new("."));
    std::env::set_current_dir(cwd).unwrap();
    assert!(got.root.is_absolute());
    assert_eq!(got.root, picked);
}

/// Headless `--out`: an empty path means the current directory (scripts'
/// default), and the save folder comes back absolute with `..` resolved.
#[test]
fn resolve_out_root_is_absolute_and_empty_means_cwd() {
    use nfspc_converter::app::destination::resolve_out_root;
    let cwd = std::env::current_dir().unwrap();
    let (dir, _) = resolve_out_root(std::path::Path::new(""));
    assert_eq!(dir, cwd);
    let tmp = tempfile::TempDir::new().unwrap();
    let game = tmp.path().join("NFS ProStreet");
    std::fs::create_dir_all(game.join("x")).unwrap();
    let (dir, is_game) = resolve_out_root(&game.join("x").join(".."));
    assert!(is_game);
    assert_eq!(dir, game);
}
