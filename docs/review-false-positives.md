# Review findings already probed and rejected

Third-party and Haiku reviews keep raising these. Each was checked against
the code and rejected; do not re-raise or "fix" them unless the stated
premise changes. Add new entries as they are rejected (claim, why wrong,
where it is proven).

| Claim | Why it is wrong | Proof |
|---|---|---|
| Duplicate-name key folds differ across ports (Rust `to_lowercase`, Python `casefold`, PowerShell `ToLowerInvariant`) | For CON inputs, STFS names decode to ASCII plus one U+FFFD per non-ASCII byte, so only ASCII letters reach the key and all three folds agree. Holds only while that decode holds. Not covered: a bare MC02 input falls back to its file name (`export_name`), which can be non-ASCII; the folds could differ there (unverified, not a real save shape). | `nfssave-core` `container360.rs` `decode_ascii_replace` + its test; fold sites `app/batch.rs`, `scripts/python/convert.py`, `Convert-NfsSave.ps1` |
| "Level-1 STFS term over-counts" | Rejected 2026-10-10: the term is the level-2 table count, and the numbers match Velocity's formula and the tests. Note the repo itself names the constant `L1_SPAN` ("data blocks per level-1 hash table, 170 * 170"), so the labels disagree; the arithmetic is what was verified. | `src/crates/fatx/src/stfs.rs` (`L1_SPAN`) + fatx tests |
| Backup uses an unvalidated CON name before the name check; duplicate guard runs before the unsafe-name check | `batch.rs` runs `check_save_name` first, then the duplicate guard, then (after prepare) the backup. | `app/batch.rs` (name first), ad24aa1 |
| Race-day pad-shift comment is wrong | It is right: the 360 block has a 4-byte pad at 0x314 and the freed word lands at the zeroed block end. | `nfssave-core` `convert.rs` fix_raceday_block comment |
| `convert_to_pc_record` is missing or misplaced (Haiku) | It exists and is in place in both ports. | `convert.py` `convert_to_pc_record`, `convert.rs` |
| PowerShell `GetField` cannot see a C# `const`, so the reflection test aborts the suite | C# consts compile to literal static fields that `GetField` returns. The test failed pre-fix only because `RehashGameplay` threw, which requires `Id == GameplayId`. | `Convert-NfsSave.ps1` `const uint GameplayId`; Run-Tests.ps1 reflection units |
| The `[0][len]` scan reverses values to big-endian | It writes the swapped (little-endian) form of the len word, as the docstring requires, and copies the data word unchanged. | `fix_node_flags` docstring |
| A len-1 node may hold a u32 such as 0x3F000000 | `len` is the byte count; a one-byte node is a u8 at its first data byte. | FORMAT-NOTES property node grammar |
| Last record's tail is cleared before a damaged gap | By design: the spill is cleared before any damaged gap for every record kind. Caveat: no alias gap test exists, so "aliases never take the gap path" is an unverified guess. | `record_spills` docstring/code; tests `test_gap.py` are career-only |
| The unary comma in PowerShell `Find-SaveFiles` (`, $hits`) breaks multi-save folders | It is the deliberate no-unroll idiom. The reviewing session reported that the new "folder with several saves" test passed against the pre-review code (not re-verified). | Run-Tests.ps1 multi-save case, `test_cli.py` |
| `Join-Path 'F:' 'Content'` is drive-relative; a drive-root OutRoot backs up drive-relative | PowerShell yields `F:\Content` and `D:\SaveConverter backups`. | `Convert-NfsSave.ps1` path helpers |
| Python `F:\` is not normalized | It is already a root. | `test_cli.py` drive-letter cases |
| The `!c.name.is_empty()` check in `export_name` is dead | Kept as a guard. | `app/batch.rs` `export_name` |
| UAC relaunch freezes the UI thread; missing signed-HINSTANCE check; missing `GetLogicalDrives()==0` check | Accepted 2026-10-05: the relaunch replaces the window anyway (state is dropped, docs/rust-app.md) and the prompt is shown on the secure desktop; whether the UI thread blocks while it is up was not measured. The other two checks were judged unnecessary. | `ui.rs` `request_fatx_scan`, `app/drivescan.rs` |
| PE32+ `Subsystem` is at offset 64 | Offset 68 in both PE32 and PE32+ (PE32+ drops BaseOfData but widens ImageBase). | `nfspc-converter/tests/subsystem.rs` |
| The STFS display name is at 0x1691, not 0x411 | The player name sits UTF-16BE at 0x411 (locale-0 display name); 0x1691 is the STFS TITLE name. | `stfs.rs` constants, `tests/stfs_oracle.rs` |
| The test counts recorded in HANDOFF are stale (reviewer added commit deltas to an old count) | The counts are measured by running the suites on `main` the same day; reviewers that cannot run them infer wrongly (a 2026-10-10 review predicted 112 / 61 from commit deltas, the run gave 110 / 59 and the collected count is 110). | `python -m pytest -q tests`, `Run-Tests.ps1` on 5.1 and 7 |
| Refusing a save hides its backup (Haiku); renaming onto a folder moves the file into it (shop26) | Neither: a refused save never reaches the backup step, and the rename onto a folder refuses cleanly. | `write_pc_save_failed_swap_keeps_target_and_removes_tmp` |
