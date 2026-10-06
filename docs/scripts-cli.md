# Script CLI contract (Python + PowerShell)

Both scripts (`scripts/python/convert.py`, `scripts/powershell/Convert-NfsSave.ps1`)
implement the same command line. Behaviour changes go in both, with tests in
`tests/test_cli.py` and `scripts/powershell/tests/Run-Tests.ps1`.

## Inputs

- Positional: one or more files and/or folders.
  - A file is converted as given (any name; it must be a 360 `CON ` container).
  - A folder is walked recursively; a file is picked when its name starts with
    `CAREER_` or `ALIAS_` (case-insensitive) AND its first 4 bytes are `CON `.
    Anything under a `SaveConverter backups` folder is skipped. A folder that
    yields nothing is an error for that folder (exit 1, other inputs still run).
- `--usb <drive-or-folder>` / `-Usb`: the stick's `<root>\Content\*\*\0000000[12]\*`
  saves (CAREER_/ALIAS_, case-insensitive). Bare `F` / `F:` = drive root.
  Old spellings `--flash` / `-Flash` stay as aliases; Python's `--all` is a
  hidden no-op.
- No input at all -> usage error, exit 2. An empty-string input is an error
  for that input (it never means the current directory).
- The collected list is de-duplicated by full path (case-insensitive), so
  overlapping inputs (a folder plus a file inside it) convert each file once.
- If every input failed to yield a save, exit 1 before printing the banner.
- `--out-root` / `-OutRoot` that exists as a file -> usage error, exit 2.
- Duplicate STFS names in one run: key = name with trailing dots/spaces
  dropped, case-insensitive (exe `windows_name_key`); only a save that
  converted (or passed a dry run) claims its name, so a failed save never
  blocks a later good one with the same name.

## Output folder

`R` = `--out-root` / `-OutRoot` if given, else the current directory. `R` is
created if missing (not on a dry run). The save folder `S` is:

1. `R\SAVE\NFS ProStreet` if that folder exists (R = game folder);
2. else `R\NFS ProStreet` if that exists (R = SAVE folder);
3. else `R` itself if its name is `NFS ProStreet` (R = the save folder);
4. else `R` (plain output; nothing detected).

Name matches are case-insensitive. Cases 1-3 print
`[+] game save folder: <S>`; case 4 prints `[+] output folder: <S>` plus a
one-line hint to copy the converted folders into the game's
`SAVE\NFS ProStreet` folder.

Each save is written to `S\<NAME>\<NAME>`, NAME = the STFS file-table name.

## Backups

An existing `S\<NAME>\<NAME>` is always copied before it is replaced, to
`<B>\SaveConverter backups\<UTC stamp>[-N]\<NAME>\<NAME>`, where `B` is
the parent of `S` in cases 1-3 (exe convention, app/batch.rs) and `R` itself
in case 4 (never write outside the folder the user chose).

## Not the app's behaviour (deliberate)

The Windows app keeps its own Documents-folder auto-detection: it is the
"auto" path, the scripts are the explicit one. Do not align them (user
decision, 2026-10-06).

## Unchanged

`--dry-run` / `-DryRun`, duplicate-name refusal within one run, exit codes
(0 ok, 1 any source failed, 2 usage), Python-only `--twin`.
