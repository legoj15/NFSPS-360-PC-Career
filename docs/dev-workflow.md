# Development workflow: tests and release (agent-facing)

There is no CI. Everything below is run by hand before a change is called
done. Write the test first, watch it fail, then implement.

## The three ports and their suites
The Python converter (`scripts/python/nfssave/`) is the reference. The Rust
core (`src/crates/nfssave-core`, used by the app) and the PowerShell script
(`scripts/powershell/Convert-NfsSave.ps1`) must stay byte-exact with it.
Any change that alters converted output lands in all three, with the golden
md5 pins moved identically in all three suites and the reason recorded in the
commit; anything save-visible then needs an in-game check by the user.

| Port | Command | Notes |
|---|---|---|
| Python | `python -m pytest -q tests` from the repo root | `pyproject.toml` only sets `testpaths` and `pythonpath`. Cases that need the gitignored `Extracted/` saves skip. |
| Rust | from `src/`: `cargo test --workspace`, `cargo fmt --all --check` | fmt is required clean (docs/rust-app.md). Also run `cargo test --release` after touching subsystem or console code. Clippy is not part of any gate and its current state was not checked. |
| PowerShell | `scripts/powershell/tests/Run-Tests.ps1` with the PowerShell tool, on BOTH Windows PowerShell 5.1 (`powershell.exe -NoProfile -ExecutionPolicy Bypass -File ...`) and pwsh 7 (`pwsh -NoProfile -File ...`) | Black-box: runs the script in a child process of the same host. Exit 0 = all passed. |

Fixture policy: tracked fixtures under `docs/re/` must exist. The golden
suites FAIL (not skip) on a missing tracked fixture; only their sources under
the gitignored `Extracted/` skip when absent. Other tests (for example much
of `tests/test_cli.py`) skip when `docs/re/pair/CAREER_02_360_fresh` is
missing, so a green run in a checkout without the tracked fixtures proves
less than it looks. Golden md5 pins live in `tests/test_golden.py`,
`src/crates/nfssave-core/tests/test_golden.rs` and `Run-Tests.ps1`; keep all
three identical.

Test notes: the PowerShell `RehashGameplay` branch for a GameplayData payload
under 0x24 bytes is unreachable through the conversion pipeline (the race-day
fix refuses anything under 0x2DC first); only the unit vectors in
`Run-Tests.ps1` and `test_short_records.rs` exercise it.

Windows notes: run `.ps1` files with the PowerShell tool; in Bash use forward
slashes. Do not edit Rust or PowerShell sources with Python replace scripts
(backslash escapes get mangled); use the Edit tool.

## Release
1. All three suites green; the user has verified save-visible changes in-game.
2. From `src/`: `cargo build --release`. Profile is `lto`, `strip`,
   `codegen-units = 1`; `src/.cargo/config.toml` statically links the MSVC
   CRT, so the exe needs no Visual C++ runtime (about 11 MB). The build
   embeds `app.manifest` via rc.exe; without the Windows SDK it warns and
   builds without it (check the warning).
3. Copy `src/target/release/NFSPS-SaveConverter.exe` to
   `dist/NFSPS-SaveConverter.exe`. `dist/` is gitignored, so the GitHub
   release asset is the only public copy.
4. Tag `vX.Y.Z` and attach the exe to a GitHub release (v1.0.0, v1.1.0, v1.1.1
   exist). Command: `gh release create vX.Y.Z dist/NFSPS-SaveConverter.exe
   --title X.Y.Z --notes-file <notes.md>`. The exe is unsigned: SmartScreen shows "Windows protected your
   PC" on first run (the README explains More info, then Run anyway).
5. Smoke test the dist exe: double-click shows no console window (verified on
   the v1.1.0 build); `NFSPS-SaveConverter.exe --convert <save> --out <dir>`
   exits 0.

Never exercised yet: the raw FATX scan on real old-format media (parked until
a user reports a problem).
