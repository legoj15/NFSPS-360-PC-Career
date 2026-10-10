# Decisions (the user's, unless marked otherwise; not derivable from code)

Do not reverse these without asking. Newest last within each group.

## Scope
- Drive and USB discovery (both scanners) recognises saves by name only
  (`CAREER_*`, `ALIAS_*`, case-insensitive); folder walks in the scripts and
  the app additionally require the `CON ` magic. Ghost-racer packages
  (`SHADOW_<id>`) are out of scope. (2026-10-05; also
  `src/crates/fatx/SPEC.md`.)
- Reverse conversion (PC to 360) and shadow racers are not looked into.
- Cross-profile mixing is not a case: the console refuses to copy another
  profile's save while signed in as a different profile. (2026-10-05)
- The FATX raw scanner is parked until a user reports a problem with
  old-format media. (2026-10-10 triage.) Original-Xbox FATX is unsupported by
  design (`src/crates/fatx/SPEC.md`), not a recorded user decision.

## Backups and overwrites
- A same-named save already in the export folder is COPIED (not moved) to
  `<parent>/SaveConverter backups/<UTC stamp>/<NAME>/<NAME>`, then replaced.
  Copy, not move, so a failed conversion leaves the game's own save in place;
  a backup failure refuses that save; the backup is kept if the later write
  fails and the message names where it is. Same-second runs fall through to
  `<stamp>-2`, ... (2026-10-05)
- The backup base is the parent of the output folder only when that folder is
  named `NFS ProStreet` (every GUI destination); otherwise the folder itself,
  so headless `--out D` keeps backups in D. "The app should match the
  script." (2026-10-10; rule text in docs/scripts-cli.md)
- A dry run refuses unsafe save names exactly like a real run.

## Scripts and ports
- QuickBMS port dropped: no bignum for the tree hash, no JSON, and a fourth
  byte-exact copy to maintain. (2026-10-06, ee64835)
- PowerShell port targets Windows PowerShell 5.1 syntax and APIs (zero
  install for the no-Python, no-exe audience) and must run unchanged on
  pwsh 7. Hot loops are `Add-Type` C# 5; rule tables come from the generated
  flat `fieldmaps.rules`, not `ConvertFrom-Json` on the 3 MB file. (The
  orchestrator's recommendation of 2026-10-06, built that way and never
  objected to; not an explicit user sign-off.)
- The scripts' default output is the current directory. The Windows app keeps
  its own Documents-folder auto-detection on purpose (docs/scripts-cli.md,
  2026-10-06).

## Removed or settled on purpose
- The `--twin` recovery path (second-source repair of damaged tails) was a
  flawed proof of concept and is deleted (3f9331d, 2026-10-10). A genuinely
  gapped career converts without the missing records and the converter warns.
  The gap fixture builders live in `tests/gapfix.py`,
  `nfssave-core/tests/common` and (PowerShell) `New-InternalGapMc02` in
  `Run-Tests.ps1`. `Tree._reafter_gap` (internal-gap re-anchoring) is NOT
  twin debt: it is on the main parse path in all ports and stays. Do not
  reintroduce the twin without new evidence.
- Unused `typemaps.json` / `typemap.py`, unreferenced RE blobs and a
  duplicate sample were removed in 338afa9.
- The converter's output matches the PC's native default `PCControllerSettings`
  record (`pc_controller_default.bin`) rather than a size-0 filler, and keeps
  the 360 `VideoSettings` length (0xB4); both were verified in-game
  (2026-10-09).

## Policy
- `cargo fmt --all --check` must be clean (docs/rust-app.md).
- Golden suites fail, not skip, on a missing tracked fixture.
