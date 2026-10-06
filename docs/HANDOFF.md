# Handoff — NFSPS 360 -> PC converter (updated 2026-10-05, 21:30 EDT)

## 2026-10-06 — script ports: QuickBMS dropped, PowerShell pending
- User dropped QuickBMS (no bignum for the tree hash, no JSON, 4th
  byte-exact copy to maintain). Committed ee64835 (README + scripts/bms).
- PowerShell port: recommended target = Windows PowerShell 5.1 syntax/APIs
  (zero install for the no-Python, no-exe audience), CI-tested on both 5.1
  and pwsh 7. Hot loops (CRC, swaps) in Add-Type C# 5; rule tables via a
  generated neutral format, not ConvertFrom-Json on the 3 MB file. Gate on
  the existing golden-md5 corpus (nfssave-core tests/test_golden.rs).
- DONE 77bd8d9: 18/18 on 5.1 and pwsh 7.7, ~0.7 s/save; README section.
  My first test draft expected source-file names; output correctly uses the
  STFS container name (game looks saves up by it) - test fixed.
  Review fan-out ps-port-review-20261006-170056-b599 (shop26 Qwen 27B +
  glm-flash) on 4dcbc3e..77bd8d9 - collect with opencode_wait_group,
  verify findings, fix as follow-up commits.
- Port leftovers worth a look: GAMEPLAY_U8_FIELDS is a 2-tuple iterated as
  offsets but its comment reads like a range (impl agent flagged; goldens
  pin the tuple behaviour, so only change with an in-game check).
- Delegation log: impl (Sonnet medium) -> ok first try, 0 escalations;
  correctly stopped on my wrong test expectation instead of editing it.
- History of the in-progress state:
  - `payload_rules.flat_rules_text/write_flat_rules` -> generated
    `scripts/powershell/fieldmaps.rules` (36 KB, C/D runs; JSON 3 MB);
    freshness + round-trip test `tests/test_flat_rules.py` (green).
  - `scripts/powershell/tests/Run-Tests.ps1`: black-box runner, child process
    of the same host (5.1 or pwsh); golden md5s, multi-source, dry run,
    backup convention, -Flash walk, error exits. Failed before the port.
  - Port `scripts/powershell/Convert-NfsSave.ps1` delegated to `impl`
    (C# 5 Add-Type for hot loops, no --twin).
- Python debt found (not fixed yet): convert.py PC_SAVE_ROOT hardcodes
  `E:\legoj\Documents\...` (published repo!); `Path("F:")/"Content"` is
  drive-relative (`F:Content`); write_pc_save overwrites with no backup
  (exe backs up).

## 2026-10-06 — no console window behind the GUI
- Release exe is now GUI-subsystem; CLI paths attach to the parent console
  (app/console.rs, details + cmd/PowerShell no-wait caveat in
  docs/rust-app.md "Headless conversion mode"). Debug stays console.
- Verified: `cargo test --release --test subsystem` failed before (CUI=3),
  passes after; redirected --help/--bogus/--convert give output + exit codes
  0/2/1; unredirected run in a fresh conhost prints into that console.
  NOT verified by hand: double-click shows no console (expected by PE field).
- dist/NFSPS-SaveConverter.exe is the OLD console build until rebuilt/copied.
- Review (shop26 Qwen3.8-27B + glm-flash, 13:26, 7e8b21c..5484387; also
  closes the pending 5a0402c review): no defects in 5a0402c. Rejected: Qwen
  high "PE32+ Subsystem is at 64" (wrong: PE32+ drops BaseOfData but widens
  ImageBase, so 68 holds for both; GLM agreed, test read 3/2 empirically);
  GLM low dead `!c.name.is_empty()` in export_name (kept as a guard).
  Fixed: shell-wait caveat wording (batch files DO wait), console.rs doc
  invariant, export-layout doc pointer.
- Delegation log: no Anthropic agents; opencode fan-out shop26+glm-flash ->
  ok, 3 lows fixed, 1 high + 1 low rejected.

## 2026-10-05 night — USB sticks are FAT32, not FATX (read first)
- User formatted a fresh 32 GB stick on the console and copied a save:
  result is plain FAT32, `F:\Content\E00001CFFAB204C4\45410822\00000001\CAREER_01`
  (standard `CON ` STFS) + `name.txt`. No `Xbox360\Data000N`, no XTAF.
  The GLM-built `fatx` crate assumed FATX; that assumption is wrong for
  current dashboards. SPEC.md §5.1 and docs/rust-app.md corrected.
- Verified: `NFSPS-SaveConverter.exe --convert F:\ --out <dir>` converts
  CAREER_01 (exit 0) via the existing manual folder walk; covered by
  `crates/nfspc-converter/tests/sources.rs:133`. Warnings printed:
  chunk 0xb67f6cc6 size 0x2b4 != map ref 0x104 (auto mode); chunk
  0xd548266c 13 unaligned string runs padded.
- DONE same night (user asked): default GUI scan = mounted drive letters
  with `Content\` at root (`app/drivescan.rs` scan_volume_roots, no
  elevation); verified on the user's two real sticks F: and G:. When it finds
  nothing, a "Click to scan for FATX drives" button relaunches the exe
  elevated via UAC with `--scan-fatx` (`app/elevation.rs`, `app/cli.rs`),
  which adds the raw FATX scan. UAC relaunch VERIFIED by the user on real
  hardware 2026-10-05 (no sticks plugged in, button -> UAC -> elevated app).
- Save recognition is now by NAME only (`fatx::discovery::is_save_name`,
  CAREER_/ALIAS_) in both scanners: the old "or ProStreet title ID" rule
  picked up ghost-racer packages (`SHADOW_74GR1` on G:), which are out of
  scope (user, 2026-10-05). `extra_title_ids` API removed (no callers).
- Batch export now refuses a second selected save with the same name
  (CAREER_01 on F: and G: silently overwrote each other before).
- User-confirmed: the console blocks copying another profile's save while
  signed in as a different profile, so cross-profile mixing is not a case.
- Review round 1 (shop26 Qwen3.8-27B + glm-flash, 21:36-21:47): both flagged
  the FATX button being clickable mid-conversion (fixed d7dc180), unbounded
  reads of save-named files (fixed: <=16 MiB + CON magic), unsorted FATX-mode
  report (fixed). Rejected: UAC freeze on UI thread (secure desktop anyway),
  signed HINSTANCE check, GetLogicalDrives==0. GLM saw a pre-fix snapshot
  (its dialog/case findings were already fixed).
- Review round 2 (same lanes, 21:49-21:58): fixed duplicate guard now folds
  names like Windows (case + trailing dots/spaces), FATX path capped at
  16 MiB like the volume scan, stale manual-pick error cleared, tests for
  claim-on-success and help aliases.
- DECIDED (user, 2026-10-05): a same-named save already in the export folder
  is COPIED to `<parent>/SaveConverter backups/<UTC stamp>/<NAME>/<NAME>`,
  then replaced (app/batch.rs back_up_existing). Copy, not move: a failed
  conversion leaves the game's save in place. Backup failure refuses that save.
  Same-second runs fall through to `<stamp>-2`, ... (17396b5). Verified end
  to end with the debug exe (two runs into a fake SAVE folder).
- Review round 3 (22:11-22:21): same-second backup collision (already fixed
  in 17396b5); GLM high CONFIRMED + fixed: guard/backup keyed on
  SaveInput.name while convert_one writes the CON file-table name, so a
  mismatched input replaced a save with no backup -> run_batch now keys on
  export_name(). Also fixed: stray spaces in the backup-failure message; a
  post-write failure now still reports where the backup went.
  5a0402c (the export-name fix) has NOT had its own outside review yet;
  next session: one shop26 + glm-flash pass on `git diff 7e8b21c..5a0402c`.
- Deferred lows (round 3): backup copy is left behind when a conversion is
  refused (reason says where); drive-root out_root puts backups at
  <drive>\SaveConverter backups; nfssave-core write_pc_save removes the
  target before rename (std rename already replaces on Windows) - widens a
  crash window; a directory at <out>/<NAME>/<NAME> is skipped by the backup.
- Deferred lows: manual single-file pick reads the whole file on the UI
  thread and again at convert; write_pc_save accepts names safe_name would
  clean; no test for the FATX-mode combined sort (needs scan_drives to take
  roots).
- Debt: the workspace is not rustfmt-clean (cargo fmt touches 9 untouched
  files); deliberately not mixed into this change.
- Delegation log: opencode review triad shop26+glm-flash (round 1) ->
  ok, 1 medium shared finding confirmed + fixed; round 2 -> 1 shared medium
  (cross-run overwrite -> backup-then-replace, user decision) + lows fixed;
  round 3 -> 1 confirmed high (export-name key) fixed; no Anthropic
  agents spawned.

## CURRENT STATE (read first)
VERIFIED IN-GAME: CAREER_01/02/03 resume at their saved point (incl. race
days in progress); garage loads all 10 CAREER_01 cars incl. DLC Veyrons with
heavy decals; CAREER_03 Camaro fixed. Tests: `python -m pytest -q tests`.
Fix chain (all in scripts/python/nfssave/convert.py unless noted): STFS block map
(container360.py); node flag words; car-table slot bytes; u16 part slots in
3 blueprint sets; packed-table 'none' links; paint/decal/vinyl/colour layout
per set; GameplayData plain u32 + race-day block (variable length, 360 pad
at 0x314, record headers u16 pairs) + MD5 recompute.
Remaining (none blocking):
1. Unverified-but-harmless: per-set ints +0x224..0x240, set words
   +0x448/+0x568, float block +0x738.., record tail +0x171C..0x1870,
   per-car 0x40 entries at 0x7C980 (byte0 00 vs ff on PC).
2. Debt: twin code path (validate_twin, Tree._reafter_gap, load_twin,
   --twin); positional node-chunk fieldmaps (superseded by node grammar +
   flag fix); unused scripts/python/nfssave/typemaps.json + docs/re/typemap.py; duplicate
   sample copies; stale out/; tree head/post not 0xAA like native (ignored
   by the loader).
3. Opencode third-party review not run (waived by the user this session).
Delegation log: no agents spawned this session (all orchestrator work).

---
# History (rounds, oldest first)

# Handoff — 2026-10-04 21:00 EDT (Opus session, picked up from ZCode/GLM)

## State
- Tests: `python -m pytest -q tests` -> all green (container, pair, golden pins).
- Installed in the PC save folder (backup of the previous folder:
  `docs/re/backups/save_20261004_2050/`): new CAREER_01 and CAREER_03.
  CAREER_02 there is still the user's NATIVE PC fresh career (the pair
  oracle; also archived at docs/re/pair/CAREER_02_pc_native). Alias untouched.
- NOT yet verified in-game. First action next session: ask the user how
  CAREER_01 loads (starting level? starter/blueprint car? garage crash?).

## Root causes found this session
1. **Container reader bug (the "damaged console tail").** CON files are
   standard STFS (two hash-table copies per level, first table 0xA000).
   A career payload is 183 blocks; data block 170 sits after a 0x4000
   hash-table group at 0xB6000. Reading the payload as one slice pulled
   hash tables into the save: FECareer from +0x980 on, plus
   CustomRaceDayMemcard and UnlockSystem, were garbage/missing in every
   career. Fixed in scripts/python/nfssave/container360.py (block map,
   `stfs_block_offset`). All 3 careers now parse 9/9 chunks with valid
   CRCs. The NFSPS360 re-save "twin" is no longer needed (auto-detect
   removed; `--twin` kept as a manual last resort — candidate for deletion).
   This alone plausibly explains "career starts at the first level".
2. **Property-node flag words were byte-swapped.** Node stream on both
   platforms: `[flag u8 + 3 junk][data][u32 0][u32 len]`; the flag byte must
   stay first. Fixed by `fix_node_flags` (nfssave/convert.py).
3. **Car part slots are u16 arrays.** FEPlayerCarDB (raw memcpy) holds 80
   car records at PC payload 0x2680, stride 0x1870. Per record, offsets
   0x3C..0x186 are u16 installed-part slots (0xFFFF = empty), 0x186..0x190
   u8 fields. The blanket u32 swap moved every part into the neighbouring
   slot -> stock car. Fixed by `fix_cardb_parts`; starter car is byte-exact
   vs the native PC pair.

## Open items (priority order)
1. In-game verification of the three fixes (user).
2. Rest of FEPlayerCarDB is still converted by the fresh-pair fieldmap /
   u32 default. Known non-u32 areas: u8-triple words (record +0x00 etc.),
   paint/vinyl entries at record +0x194..+0x1B4 (pair is self-contradictory:
   record 0 behaves as u16 pairs, records 1-10 as u32), and the 8-byte
   packed table at 0x7D000..end (bitfields; 360 empty = 2aaafffe ffff2aaa,
   PC empty = feffff3f fffffeff; PC fresh has entries the 360 fresh lacks —
   may be a platform-built cache). Garage crash likely lives here.
   docs/re/typemap.py is a first-cut automatic inference tool; its pooled
   distribution scoring is NOT reliable for small-vocabulary u16 data
   (see its notes) — do not ship scripts/python/nfssave/typemaps.json until it is fixed
   (it is currently unused by the converter).
3. GameplayData raw blob: pair shows only content diffs, but the 100%
   career fills areas that are empty in the fresh pair; needs the same
   field-type audit.
4. Best source of truth for struct layouts: the 360 recomp
   (E:/GitHub/NFSPS360, field widths from lhz/lbz/lwz in the loaders) or
   the PC exe. Start at impl-opus per the ladder (RE class).
5. Third-party (opencode) review of this change was NOT run (session hit
   the 9PM wrap-up) — run opencode-fanout review on commit HEAD.
6. Cleanup debt: twin code path (convert.validate_twin, Tree._reafter_gap,
   load_twin), positional fieldmaps for node chunks (superseded by the
   node grammar), duplicate sample copies (Extracted/Career/CAREER_02_pair
   == docs/re/pair/CAREER_02_360_fresh; docs/re/oracle/native_fresh_CAREER_02_pc
   == docs/re/pair/CAREER_02_pc_native).

## Delegation log
- (none this session — all analysis done by the orchestrator; no agents spawned)

## Round 3 notes (21:20) — PC install environment
- The game allows 3 careers per alias: the CAREER_04 diagnostic triggered a
  warning on every screen; moved to docs/re/backups/diag_CAREER_04/.
- The PC install is a ChemicalFlood repack, not a clean v1.1: mods =
  FusionFix (FramerateUncap=1, SimRate=-1 = monitor refresh), NFS_XtendedInput
  (input remap), d3d9-wrapper (FPSLimit=60), Ultimate ASI loader (dinput8.dll).
  The repack also ships "car fixes" (modified CARS data) and its own FAQ admits
  garage crashes with DLC cars + stage-4 kits -> the garage crash may be
  repack-side, not converter-side. Part IDs in modified car data could also
  differ from the 360 TU.
- User reports missing speedometer HUD and phantom left/right menu input
  (blocked selecting "Yes" to load). Suspects: XtendedInput / a drifting or
  virtual controller axis; SimRate (-1) != FPS cap (60). Not changed - user's
  call.

## Round 4 (21:15) — user test result
- CAREER_01 now loads the CORRECT car (car-table slot fix confirmed in-game).
- Still wrong: it plays the "new game" intro cutscene; on the 360 the save
  resumes an in-progress Willow Springs race day. So a "career started /
  current race day" state is still lost.
- Checked and ruled out tonight: GameplayData shows no width-error
  signature vs the PC 100% save (2 mirror hits, both content); FECareer
  node values look like plausible swapped u32s (8-byte nodes are 16-byte
  aligned with padding, so a naive node walker breaks — alignment is
  preserved by the converter because PC/360 record offsets stay congruent).
- Next: get an oracle for the in-progress state. Ask the user to save BOTH
  platforms' CAREER_02 mid-race-day (start the next race day, finish one
  event, save/quit) -> new matched pair. Diff its chunks against the current
  pair to locate the race-day-in-progress fields (likely RaceData 0x51A41B14,
  FECareer, or GameplayData), then check how the converter handles them.
  Also worth checking: alias-side career state (game loaded alias "Player",
  not the author's alias).

## Round 5 (21:30) — mid-race-day pair
- New oracle: docs/re/pair_raceday/ (CAREER_02 at the "Battle Machine"
  race day in Nevada, saved on both platforms).
- GameplayData is NOT a fixed struct: while a race day is in progress
  (u32 at PC 0x2D4 == 1) a race-day block occupies 0x2E0..0x3E70 and the
  event list follows. The 360 block has a 4-byte pad at 0x314 the PC lacks;
  fixed (fix_raceday_block) + u8 per-event flag words (0x434..0x784 step 16)
  and the name string at 0x300 kept natural. Physics floats differ only in
  noise bits. tests/test_raceday.py.
- Installed: converted raceday 360 save as SAVE/.../CAREER_02 (native PC
  copy archived byte-exact at docs/re/pair_raceday/CAREER_02_pc_native —
  copy it back to restore). User to test: does CAREER_02 resume the Battle
  Machine race day like native did?
- IMPORTANT: the user's 360 CAREER_01 has NO active race-day block
  (0x2D0 looks like the fresh career) - so its "resume Willow Springs"
  state is not in GameplayData's block; this fix does not change CAREER_01.
  Next: once CAREER_02 is confirmed, diff the remaining chunks of CAREER_01
  vs the resume behaviour (FECareer/RaceData/alias-side state).

## Round 6 (late night) — GameplayData MD5 = the "new career" cause
- User: converted CAREER_02 (raceday) started a NEW career (intro movie).
- Root cause: GameplayData blob (PC payload 0x14, 0x10000 B) is
  [MD5(blob[16:])][rest]; loader 0x59E550 -> deserializer [0xAB9D88]
  vtbl+0x70 rejects a stale hash -> defaults. Every converted file carried
  the 360's MD5. Fixed (rehash_gameplay). Extra-blob word0 is a vtable
  pointer (0x974BB8 career / 0x974BAC alias) the loader ignores.
- Race-day block is variable length (CAREER_02 0x3B90 B state 1, latest
  CAREER_01 0xB2D0 B state 3); end found via the following [0][0x11] list.
  Block records: [u32 kind 0x000?1x10][u16][u16] + float matrices.
  Only the CAREER_02 layout is oracle-verified; CAREER_01's longer block
  uses the same rules unverified.
- GameplayData now plain u32 swap (+fixes); the positional fresh-pair map
  for it is no longer used.
- Installed: CAREER_01 = latest 360 copy (docs/re/c1_latest), CAREER_02 =
  converted raceday, CAREER_03. Awaiting in-game test.
