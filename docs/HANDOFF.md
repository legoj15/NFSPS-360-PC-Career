# Handoff — NFSPS 360 -> PC converter (updated 2026-10-11)

State and open work only. History is `git log` (conventional-commit subjects
name every fix); knowledge lives in the docs below. The 787-line session log
this file used to be was retired on 2026-10-10 (still in git history:
`git show 6658f3b:docs/HANDOFF.md`).

Read order for a new session: this file, then `docs/decisions.md`, then
`docs/dev-workflow.md`.

## Where things live
| Need | File |
|---|---|
| Save-format findings, what is verified in-game | `docs/re/FORMAT-NOTES.md` (last section first) |
| Fixtures, RE/debug tools, the author's PC test environment | `docs/re/README.md` |
| User decisions not derivable from code (do not reverse) | `docs/decisions.md` |
| Run the tests, cut a release | `docs/dev-workflow.md` |
| Review findings already probed and rejected | `docs/review-false-positives.md` |
| Script CLI contract / Windows app internals | `docs/scripts-cli.md` / `docs/rust-app.md` |
| Unproven fields, parked research | `docs/backlog-research.md` |
| Who was delegated what, ladder observations | `docs/delegation-log.md` |

## State
- Release: v1.1.0 published on GitHub (exe built to `dist/`, gitignored). All
  work is on `main`.
- Verified in-game by the user (October 2026): careers resume at their saved
  point including a race day in progress; the garage loads every car (DLC
  Veyrons with heavy decals included); main-menu Race Day and custom race
  days; the converted alias loads with its options, speedometer, leaderboard
  and camera, and no stray `ALIAS_Player` / `CAREER_<junk>` files appear;
  double-click shows no console window; the UAC relaunch for the FATX scan.
- USB sticks formatted by the console are plain FAT32 (docs/rust-app.md). The
  raw FATX scanner has never run on real media (parked).
- Three ports (Python reference, Rust core used by the app, PowerShell
  script) are byte-exact; golden md5 pins are in each suite. Last full run,
  2026-10-10 (open-work batch, converted output unchanged): pytest 147
  passed; `cargo test --workspace` green and `cargo fmt --all --check`
  clean; Run-Tests.ps1 67/67 on Windows PowerShell 5.1 and on pwsh 7
  (nothing skipped: `Extracted/` is present, junctioned into worktrees from
  the main checkout with `mklink /J`; without it a few golden cases skip.
  Remove the junction with `rmdir` before the worktree is cleaned up, so a
  recursive delete cannot reach the personal saves).
- 2026-10-11 fix, NOT yet verified in-game: converted careers could never
  gain a car (challenge reward cars and dealer purchases silently dropped)
  because every FEPlayerCarDB record handle read 0xAA, so the game's
  free-slot scans failed (docs/re/FORMAT-NOTES.md, FEPlayerCarDB struct).
  `fix_cardb_bytes` in all three ports; career golden pins moved (alias pins
  unchanged); pytest 155 passed, `cargo test --workspace --no-fail-fast` green,
  Run-Tests.ps1 67/67 on 5.1 and pwsh 7. The author's played CAREER_01 was
  repaired in place with docs/re/rebase_fix.py (backup:
  `SaveConverter backups/2026-10-11_07-34-39_before-cardb-repair`).
  In-game check wanted: buy any car at the dealer, or win a challenge race
  day, and see it in the garage. The three reward cars already won are
  not in the save and were not restored.
- Not yet seen by the user: the app's new orange stray-save notes
  (`[!] the save folder ...`) under the results list. Tests cover the text,
  not the GUI rendering.

## Open work
Debt: the save-name rule, device list, backup-folder name and stray-note
wording are hand-copied across the three ports and their suites; the
app's `stray_save_notes` could move next to `check_save_name` in
`nfssave-core`, and one shared vector file (like
`pc_controller_default.bin`) would replace the three copied test lists.

Test gaps: PowerShell post-write self-check failure (unreachable without a
seam; Python and Rust pin the message); backup at a drive-root output folder; old-format USB scan order;
the shared backup stamp across one `main()` run; PowerShell "a failed save
does not claim its name" (Python pins it); combined mounted + FATX sort
(`scan_drives` takes no roots); any alias gap fixture (the "aliases never take
the gap path" rationale is an unverified guess, see
`docs/review-false-positives.md`). The internal-gap fixture is built three
times (`tests/gapfix.py`, `nfssave-core/tests/common`, and
`New-InternalGapMc02` in `Run-Tests.ps1`); keep them in sync.

NEXT (user decision 2026-10-11): once the user confirms in-game that a new
car reaches the garage, release this fix as 1.2.0. Steps: merge branch
`claude/challenge-reward-cars-missing-1b196f` (commit 17568c5 and after) into
`main`; bump `src/Cargo.toml` (the four `version = ` lines incl.
`Cargo.lock`), the README download link, and the release lists in
docs/dev-workflow.md and docs/rust-app.md, as in 37a0d13; build and
release per docs/dev-workflow.md with notes `docs/release-notes-1.2.0.md`;
then the post-merge review (dgx / Qwen Flash + glm-5.3). If the test fails,
do not release: re-check the allocators (FORMAT-NOTES FEPlayerCarDB).

Loose ends:
- The release upload command was never written down (docs/dev-workflow.md).

Research and parked (details in `docs/backlog-research.md`): unproven
car/blueprint fields; AudioSettings node 8 holds 360 heap fill; meaning of
12 race-day progress keys; the `scalar_tail` zero-pad rule (no sample);
whether the positional `docs/re/fieldmaps/` can go; FATX raw scanner on real
media (wait for a user report).
