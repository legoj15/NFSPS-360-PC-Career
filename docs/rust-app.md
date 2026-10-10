# Rust Windows app (agent-facing)

Covers `src/`, the NFSPS-SaveConverter Windows app. Human-facing usage lives
in the README; save-format research lives in `docs/re/` and `docs/HANDOFF.md`.

## Architecture: three crates

Workspace root is `src/` (`src/Cargo.toml`, members listed at lines 3-7):

| Crate | Role |
|---|---|
| `nfssave-core` | Save-format library: STFS container reader, MC02 parse, chunk-tree byte-order conversion. Rust port of `scripts/python/nfssave`; the Python is the verified spec, the port is pinned byte-exact by golden-md5 tests (`src/crates/nfssave-core/src/lib.rs:1-5`, `tests/test_golden.rs`). |
| `fatx` | Xbox 360 USB/FATX scanner (opt-in, legacy layout; also owns the shared `CONTENT_ROOT`/`SAVE_TYPE_DIRS`/`is_save_name` rules the default FAT32 scan reuses): probes a raw drive image or `\\.\PhysicalDriveN`, finds the FATX Data partition, walks `Content/<profile>/<titleID>/0000000{1,2}/`, returns matching ProStreet CON saves with bytes (`src/crates/fatx/src/lib.rs:1-33`). Feature `test-util` synthesizes whole FATX USB images so tests need no hardware. |
| `nfspc-converter` | The app itself. Two strictly separated layers: `app` (destination resolution, manual-source discovery, drive scan, batch conversion — std + the two libraries only, unit-tested) and `ui` (thin eframe/egui front end); headless entry point in `main.rs` (`src/crates/nfspc-converter/src/lib.rs:1-10`). Binary name: `NFSPS-SaveConverter`. |

The bin target has `test = false` — the exe must not run a (bin) test harness
— so all converter-app tests are lib/integration tests
(`src/crates/nfspc-converter/Cargo.toml`, `[[bin]]` comment).

## Running the tests

From `src/`:

    cargo test --workspace

Verified this session (2026-10-05): exit code 0, every suite ok — the tail
includes nfssave-core unit tests (5 passed), `test_raceday` (5 passed) and
the fatx lib doctest (1 passed). No hardware needed: fatx tests use the
`test-util` synthetic images; nfssave-core tests use tracked oracles under
`docs/re/`.

Formatting is enforced: `cargo fmt --all --check` (from `src/`) must be
clean before every commit. Run `cargo fmt --all` after editing Rust; do not
mix unrelated reformatting into feature commits (the whole workspace was
formatted once in its own commit, 2026-10-09).

Release build: `cargo build --release` from `src/`. The workspace release
profile is `lto`, `strip`, `codegen-units = 1` (`src/Cargo.toml:17-20`) and
`.cargo/config.toml` statically links the MSVC CRT, so the exe ships without
the Visual C++ redistributable. The build embeds `app.manifest` via rc.exe;
if the Windows SDK is missing it warns and builds WITHOUT the manifest
(`src/crates/nfspc-converter/build.rs:31-37`).

## Headless conversion mode

    NFSPS-SaveConverter.exe --convert <file-or-folder> --out <dir>

(usage text and parsing in `src/crates/nfspc-converter/src/app/cli.rs`)

Console behaviour: release builds use the Windows GUI subsystem
(`#![windows_subsystem]` in `main.rs`, gated on `not(debug_assertions)`), so
double-clicking shows no console window. The CLI paths (`--convert`, `--help`,
argument errors) call `app::console::attach_parent_console`: redirected
stdout/stderr (pipes, files, `Command::output`, `Start-Process -Redirect*`)
work as-is and exit codes are intact; otherwise output goes to the launching
terminal's console. Caveat of any GUI-subsystem exe: an interactive `cmd.exe` prompt and a
direct call from PowerShell do not wait for it (the prompt can return before
the output, and `$LASTEXITCODE` is not set); `cmd` batch files, piped or
redirected PowerShell calls, and `Start-Process -Wait -PassThru` do wait and
get the exit code. If a console-native CLI is ever
needed, ship a second console-subsystem bin rather than reverting this. A
window-creation failure in the GUI path shows a message box
(`console::error_box`). Debug builds stay console-subsystem for env_logger.
`tests/subsystem.rs` checks the PE subsystem field; run it with
`cargo test --release` to cover the release half.

- `--convert` accepts a CON container, a raw MC02 save (either byte order),
  or a folder — folders are walked depth-bounded (`MAX_DEPTH = 5`) for
  `CAREER_*`/`ALIAS_*` files with the `CON ` magic, which also covers an
  extracted `Content` tree (`src/crates/nfspc-converter/src/app/sources.rs:1-15`).
- `--out` is the export directory; writes `<out>/<NAME>/<NAME>`
  (`nfssave-core` `write_pc_save`, via `app/batch.rs` `run_batch`). Exports are written atomically: bytes land in
  `<target>.tmp` and are renamed over the target, so an interrupted write
  never truncates a previous good export (`nfssave-core` `write_pc_save`).
- Exit code 0 only when every requested save converted; failures print to
  stderr with a nonzero exit. Per-file load failures report to stderr and
  the run continues with the remaining files, matching the GUI worker and
  the Python CLI (`app/headless.rs`).
- No arguments launches the GUI; `--help`/`-h`/?` prints usage (`main.rs`).
- Headless does NOT scan physical drives — manual file/folder input only
  (`app/headless.rs` uses `discover_manual`).

## FATX format reference

All on-disk FATX/XTAF layout constants, offsets and their sources are
documented in `src/crates/fatx/SPEC.md` — read that before touching the
`fatx` crate. Sections cover superblock/FAT/dirents (§1-4), whole-drive
layouts incl. retail USB fixed offsets and the devkit HDD table (§5), where
saves live in the Data partition (§6), CON/STFS header fields we parse (§7),
and Windows raw-drive access (§8).

## Elevation: why the manifest is `asInvoker`

Decision (2026-10-05): the exe launches unelevated on purpose
(`src/crates/nfspc-converter/app.manifest:17`), because

- the headless `--convert` mode must run in CI/automation, and
- double-clicking the GUI must not prompt UAC for plain file conversion
  (`app.manifest:6-13`, `build.rs:4-10`).

Evidence: raw `\\.\PhysicalDriveN` read-only opens DO require elevation —
measured 2026-10-05, unelevated `cargo test -p fatx --test device_probe`:
PhysicalDrive0-3 → `ERROR_ACCESS_DENIED`, 4-15 → not found
(`src/crates/fatx/SPEC.md:196-199`). Consequence and runtime handling: a drive
scan that hits access-denied surfaces one UI note; normally the FATX scan
only runs from the self-relaunched elevated instance (`--scan-fatx`,
`app/elevation.rs`). The app requests elevation (UAC) only when the user
clicks the FATX button, never at launch. Related: the manifest is linked into bins
only so cargo's test harnesses stay unelevated (`build.rs:51-54`), and
`build.rs` rejects XML comments containing `--` (WinError 14001 killer,
`build.rs:57-61`).

## Known gaps

- The raw FATX scan is opt-in and **untested on real media**: current
  consoles write plain FAT32 (`src/crates/fatx/SPEC.md` §5.1), which the
  default mounted-volume scan covers. FATX runs only after the user clicks
  "Click to scan for FATX drives" (shown when the normal scan found
  nothing), which relaunches the exe elevated with `--scan-fatx`
  (`app/elevation.rs`, `app/cli.rs`). The relaunch drops the current
  window's state (manual picks, chosen export folder). Candidate for
  deletion if no old-format media ever turns up.
- Mounted-volume scan only looks at `Content\` at a drive-letter root;
  partitions without a letter are not seen. Real-hardware check:
  `cargo run -p nfspc-converter --example volume_scan` (from `src/`).
- Headless mode has no drive scanning (files/folders only, see above).
- Drive probe is fixed to `\\.\PhysicalDrive0..=15` (`MAX_DRIVE_INDEX`,
  `drivescan.rs:32`); more than 16 physical drives are not scanned.
- Original-Xbox FATX (LE, `FATX` magic) intentionally unsupported
  (`SPEC.md` intro); the `MICROSOFT*XBOX360` sector-0 signature remains
  UNVERIFIED and is never required for detection (`SPEC.md` §5.4).
- No code signing → SmartScreen "Windows protected your PC" on first run
  (README documents the More info → Run anyway path).
- No git remote is configured on this checkout yet (verified:
  `git remote -v` is empty), so the README's "GitHub Releases" download
  wording presumes a repo/remote with published releases that does not
  exist yet.
