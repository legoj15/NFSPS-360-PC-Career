# Rust Windows app (agent-facing)

Covers `src/`, the NFSPS-SaveConverter Windows app. Human-facing usage lives
in the README; save-format research lives in `docs/re/` and `docs/HANDOFF.md`.

## Architecture: three crates

Workspace root is `src/` (`src/Cargo.toml`, members listed at lines 3-7):

| Crate | Role |
|---|---|
| `nfssave-core` | Save-format library: STFS container reader, MC02 parse, chunk-tree byte-order conversion. Rust port of `scripts/python/nfssave`; the Python is the verified spec, the port is pinned byte-exact by golden-md5 tests (`src/crates/nfssave-core/src/lib.rs:1-5`, `tests/test_golden.rs`). |
| `fatx` | Xbox 360 USB/FATX scanner: probes a raw drive image or `\\.\PhysicalDriveN`, finds the FATX Data partition, walks `Content/<profile>/<titleID>/0000000{1,2}/`, returns matching ProStreet CON saves with bytes (`src/crates/fatx/src/lib.rs:1-33`). Feature `test-util` synthesizes whole FATX USB images so tests need no hardware. |
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
`docs/re/`. Note one stale comment: `nfspc-converter/Cargo.toml`'s `[[bin]]`
block still says the manifest is `requireAdministrator`; the manifest is
`asInvoker` (see below) — trust `app.manifest` and `build.rs`.

Release build: `cargo build --release` from `src/`. The workspace release
profile is `lto`, `strip`, `codegen-units = 1` (`src/Cargo.toml:17-20`) and
`.cargo/config.toml` statically links the MSVC CRT, so the exe ships without
the Visual C++ redistributable. The build embeds `app.manifest` via rc.exe;
if the Windows SDK is missing it warns and builds WITHOUT the manifest
(`src/crates/nfspc-converter/build.rs:31-37`).

## Headless conversion mode

    NFSPS-SaveConverter.exe --convert <file-or-folder> --out <dir>

(usage text at `src/crates/nfspc-converter/src/main.rs:12-27`)

- `--convert` accepts a CON container, a raw MC02 save (either byte order),
  or a folder — folders are walked depth-bounded (`MAX_DEPTH = 5`) for
  `CAREER_*`/`ALIAS_*` files with the `CON ` magic, which also covers an
  extracted `Content` tree (`src/crates/nfspc-converter/src/app/sources.rs:1-15`).
- `--out` is the export directory; writes `<out>/<NAME>/<NAME>`
  (`main.rs:23-24`).
- Exit code 0 only when every requested save converted; failures print to
  stderr with a nonzero exit (`main.rs:26-27`, `run_headless` at 93-159).
- No arguments launches the GUI; `--help`/`-h`/?` prints usage (`main.rs:53-56`).
- Headless does NOT scan physical drives — manual file/folder input only
  (`run_headless` uses `discover_manual`, `main.rs:94`).

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
scan that hits access-denied surfaces one UI note asking the user to relaunch
elevated (`src/crates/nfspc-converter/src/app/drivescan.rs:75-86`); the app
never demands elevation itself. Related: the manifest is linked into bins
only so cargo's test harnesses stay unelevated (`build.rs:51-54`), and
`build.rs` rejects XML comments containing `--` (WinError 14001 killer,
`build.rs:57-61`).

## Known gaps

- Unelevated GUI cannot scan raw drives: USB FATX scanning needs an elevated
  relaunch (user-facing note is the only remedy; no self-elevation).
- Headless mode has no drive scanning (files/folders only, see above).
- Drive probe is fixed to `\\.\PhysicalDrive0..=15` (`MAX_DRIVE_INDEX`,
  `drivescan.rs:32`); more than 16 physical drives are not scanned.
- Original-Xbox FATX (LE, `FATX` magic) intentionally unsupported
  (`SPEC.md` intro); the `MICROSOFT*XBOX360` sector-0 signature remains
  UNVERIFIED and is never required for detection (`SPEC.md` §5.4).
- No code signing → SmartScreen "Windows protected your PC" on first run
  (README documents the More info → Run anyway path).
- Stale `requireAdministrator` comment in `nfspc-converter/Cargo.toml`
  `[[bin]]` (manifest is `asInvoker`).
- No git remote is configured on this checkout yet (verified:
  `git remote -v` is empty), so the README's "GitHub Releases" download
  wording presumes a repo/remote with published releases that does not
  exist yet.
