# Handoff — NFSPS 360 -> PC converter (updated 2026-10-10)

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
  script) are byte-exact; golden md5 pins are in each suite. Last full run on
  `main`, 2026-10-10: pytest 110 passed; `cargo test --workspace` green and
  `cargo fmt --all --check` clean; Run-Tests.ps1 59/59 on Windows PowerShell
  5.1 and on pwsh 7 (nothing skipped: `Extracted/` is present in this
  checkout, so worktrees without it skip a few golden cases).

## Open work (each confirmed still open against the code on 2026-10-10)
Correctness and parity, small:
1. Python `GameplayData` payload < 0x2DC bytes dies with a bare
   `struct.error`; Rust and PowerShell refuse with "GameplayData chunk too
   short". Make the Python message match (test first).
2. PowerShell converts each record inline in its embedded C# loop and
   hard-codes the GameplayData / RaceData ids where Python uses
   `NUMERIC_IDS`. Equivalent today only because that set is just RaceData;
   parity rests on the Run-Tests pins.
3. A save literally named `SaveConverter backups` passes every name check
   (all ports) and exports inside the backup folder. Refuse it or accept it
   explicitly.
4. `check_save_name` / `Test-SaveName` (all three ports) are deliberately not
   full Windows-name validators and let `* ? " < > |` through; `safe_name`
   only cleans the fallback dirent name, so such a name fails at write time
   with an OS error instead of the clean refusal. Real save names are
   `CAREER_nn` / `ALIAS_*`, so this is cosmetic unless a hostile name matters.
5. A directory at `<out>/<NAME>/<NAME>` is skipped by the backup
   (`is_file`) and the rename then fails. Rust `write_pc_save` pins the clean
   refusal; Python has no equivalent test.
6. A read-only or game-locked target fails with a bare OS error that does not
   say the old save survived (Rust and Python `write_pc_save`).
7. Post-write self-check failure is exit 1 in PowerShell but a warning in
   Python and Rust (unreachable in practice).
8. A CON input is parsed three times per save in the app flow
   (`from_path`/`from_discovered`, `export_name`, `prepare_one`); carry the
   parsed name in `SaveInput`.
9. `GAMEPLAY_U8_FIELDS` is a 2-tuple iterated as offsets, but its comment
   reads like a range (`convert.py:359`, `convert.rs:494`). Goldens pin it;
   change only with an in-game check.
10. Idea: warn when the target folder already holds another `ALIAS_*` or a
    `CAREER_` with a non-ASCII name (stray saves like that usually mean the
    PC fell back to a default profile; see FORMAT-NOTES for the caveat).

Test gaps: backup at a drive-root output folder; old-format USB scan order;
the shared backup stamp across one `main()` run; PowerShell "a failed save
does not claim its name" (Python pins it); combined mounted + FATX sort
(`scan_drives` takes no roots); any alias gap fixture (the "aliases never take
the gap path" rationale is an unverified guess, see
`docs/review-false-positives.md`). The internal-gap fixture is built three
times (`tests/gapfix.py`, `nfssave-core/tests/common`, and
`New-InternalGapMc02` in `Run-Tests.ps1`); keep them in sync.

Loose ends:
- The release upload command was never written down (docs/dev-workflow.md).

Research and parked (details in `docs/backlog-research.md`): unproven
car/blueprint fields; AudioSettings node 8 holds 360 heap fill; meaning of
12 race-day progress keys; the `scalar_tail` zero-pad rule (no sample);
whether the positional `docs/re/fieldmaps/` can go; FATX raw scanner on real
media (wait for a user report).
