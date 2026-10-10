//! Destination resolution for 'Select location to export saves'.
//!
//! Given the folder `P` the user picked:
//!
//! 1. If `P`'s folder name is exactly `NFS ProStreet` (case-insensitive),
//!    export directly into `P`.
//! 2. Else, if `P` contains an `NFS ProStreet` folder one or two levels deep
//!    (e.g. `P/SAVE/NFS ProStreet`, the community-repack layout, or
//!    `P/NFS ProStreet`), export into that existing folder; several matches
//!    prefer a path that runs through a `SAVE` folder.
//! 3. Else create `P/NFS ProStreet` and export there.
//!
//! The game's real save folder
//! (`Documents/Need for Speed ProStreet/SAVE/NFS ProStreet`, resolved via the
//! shell known-folder API, never an env var) is offered as the default
//! suggestion when it exists.

use std::fs;
use std::io;
use std::path::{Path, PathBuf, absolute};

/// The folder name the PC game reads saves from under its SAVE root.
pub const GAME_FOLDER_NAME: &str = "NFS ProStreet";
/// Intermediate folder name preferred when several candidates match.
pub const SAVE_FOLDER_NAME: &str = "SAVE";

/// A resolved export destination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    /// Absolute folder the saves must be written into.
    pub root: PathBuf,
    /// True when the folder did not exist and has to be created (rule 3).
    pub created: bool,
}

fn folder_name_is_game(p: &Path) -> bool {
    p.file_name()
        .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(GAME_FOLDER_NAME))
}

/// True when at least one path component strictly between `picked` and
/// `cand` is named `SAVE` (case-insensitive).
fn runs_through_save(picked: &Path, cand: &Path) -> bool {
    cand.ancestors()
        .skip(1) // the candidate itself
        .take_while(|a| *a != picked)
        .any(|a| {
            a.file_name()
                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(SAVE_FOLDER_NAME))
        })
}

/// Subdirectories of `dir` (unreadable -> empty).
fn child_dirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    out.sort();
    out
}

/// All `NFS ProStreet` folders one or two levels below `picked`.
fn find_candidates(picked: &Path) -> Vec<(usize, PathBuf)> {
    let mut cands = Vec::new();
    for d in child_dirs(picked) {
        if folder_name_is_game(&d) {
            cands.push((1, d.clone()));
        }
    }
    // two levels: P/<anything>/NFS ProStreet (a game folder nested inside a
    // game folder is not searched further)
    for d in child_dirs(picked) {
        if folder_name_is_game(&d) {
            continue;
        }
        for d2 in child_dirs(&d) {
            if folder_name_is_game(&d2) {
                cands.push((2, d2));
            }
        }
    }
    cands
}

/// Pick the best candidate: prefer a path through a SAVE folder, then the
/// shallowest, then the lexicographically smallest (deterministic).
fn pick_candidate(picked: &Path, cands: Vec<(usize, PathBuf)>) -> Option<PathBuf> {
    let via_save: Vec<&(usize, PathBuf)> = cands
        .iter()
        .filter(|(_, p)| runs_through_save(picked, p))
        .collect();
    let pool: Vec<&(usize, PathBuf)> = if via_save.is_empty() {
        cands.iter().collect()
    } else {
        via_save
    };
    pool.into_iter()
        .min_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)))
        .map(|(_, p)| p.clone())
}

/// Compute the destination without creating anything (for UI previews).
pub fn resolve_dry(picked: &Path) -> Destination {
    let abs = absolute(picked).unwrap_or_else(|_| picked.to_path_buf());
    if folder_name_is_game(&abs) {
        return Destination {
            root: abs,
            created: false,
        };
    }
    if let Some(root) = pick_candidate(&abs, find_candidates(&abs)) {
        return Destination {
            root,
            created: false,
        };
    }
    Destination {
        root: abs.join(GAME_FOLDER_NAME),
        created: true,
    }
}

/// `parent/names...`, each level matched case-insensitively, when it exists.
fn child_dir_ci(parent: &Path, names: &[&str]) -> Option<PathBuf> {
    let mut cur = parent.to_path_buf();
    for name in names {
        cur = child_dirs(&cur).into_iter().find(|d| {
            d.file_name()
                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(name))
        })?;
    }
    Some(cur)
}

/// Headless `--out R`, the scripts' rule (docs/scripts-cli.md "Output
/// folder", Python `resolve_save_folder`): `R/SAVE/NFS ProStreet` if it
/// exists, else `R/NFS ProStreet`, else R itself. Returns the absolute save
/// folder (`..` resolved; an empty R is the current directory, the scripts'
/// default) and whether it is the game's: true also when R itself is named
/// `NFS ProStreet`, even before it exists. Nothing is created.
pub fn resolve_out_root(root: &Path) -> (PathBuf, bool) {
    let root = if root.as_os_str().is_empty() {
        Path::new(".")
    } else {
        root
    };
    let abs = absolute(root).unwrap_or_else(|_| root.to_path_buf());
    // absolute() keeps `..` on Unix; on Windows it is GetFullPathName,
    // which resolves it like the PowerShell script's GetFullPath
    if let Some(s) = child_dir_ci(&abs, &[SAVE_FOLDER_NAME, GAME_FOLDER_NAME])
        .or_else(|| child_dir_ci(&abs, &[GAME_FOLDER_NAME]))
    {
        return (s, true);
    }
    let is_game = folder_name_is_game(&abs);
    (abs, is_game)
}

/// Resolve and, when rule 3 applies, create the missing folder.
pub fn resolve(picked: &Path) -> io::Result<Destination> {
    let d = resolve_dry(picked);
    if d.created {
        fs::create_dir_all(&d.root)?;
    }
    Ok(d)
}

/// The game's real save folder, when it exists:
/// `<Documents>/Need for Speed ProStreet/SAVE/NFS ProStreet`.
///
/// Documents is resolved through the shell known-folder API
/// (`FOLDERID_Documents`), which honours redirected Documents folders -
/// never through an environment variable.
#[cfg(windows)]
pub fn documents_save_folder() -> Option<PathBuf> {
    let docs = known_folder_documents()?;
    let cand = docs
        .join("Need for Speed ProStreet")
        .join(SAVE_FOLDER_NAME)
        .join(GAME_FOLDER_NAME);
    cand.is_dir().then_some(cand)
}

#[cfg(not(windows))]
pub fn documents_save_folder() -> Option<PathBuf> {
    None
}

#[cfg(windows)]
fn known_folder_documents() -> Option<PathBuf> {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_Documents, KF_FLAG_DEFAULT, SHGetKnownFolderPath};

    // SAFETY: standard known-folder call with a constant GUID.
    let pwsz = unsafe { SHGetKnownFolderPath(&FOLDERID_Documents, KF_FLAG_DEFAULT, None) }.ok()?;
    if pwsz.is_null() {
        return None;
    }
    // SAFETY: known-folder strings are NUL-terminated.
    let wide = unsafe { pwsz.as_wide() }.to_vec();
    // SAFETY: freeing our own CoTaskMem-allocated string exactly once.
    unsafe { CoTaskMemFree(Some(pwsz.as_ptr().cast())) };
    Some(PathBuf::from(String::from_utf16_lossy(&wide)))
}
