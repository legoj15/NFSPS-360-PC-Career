# Handoff — 2026-10-04 21:00 EDT (Opus session, picked up from ZCode/GLM)

## State
- Tests: `python -m pytest -q tests` -> all green (container, pair, golden pins).
- Installed in the PC save folder (backup of the previous folder:
  `research/backups/save_20261004_2050/`): new CAREER_01 and CAREER_03.
  CAREER_02 there is still the user's NATIVE PC fresh career (the pair
  oracle; also archived at research/pair/CAREER_02_pc_native). Alias untouched.
- NOT yet verified in-game. First action next session: ask the user how
  CAREER_01 loads (starting level? starter/blueprint car? garage crash?).

## Root causes found this session
1. **Container reader bug (the "damaged console tail").** CON files are
   standard STFS (two hash-table copies per level, first table 0xA000).
   A career payload is 183 blocks; data block 170 sits after a 0x4000
   hash-table group at 0xB6000. Reading the payload as one slice pulled
   hash tables into the save: FECareer from +0x980 on, plus
   CustomRaceDayMemcard and UnlockSystem, were garbage/missing in every
   career. Fixed in nfssave/container360.py (block map,
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
   research/typemap.py is a first-cut automatic inference tool; its pooled
   distribution scoring is NOT reliable for small-vocabulary u16 data
   (see its notes) — do not ship nfssave/typemaps.json until it is fixed
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
   == research/pair/CAREER_02_360_fresh; research/oracle/native_fresh_CAREER_02_pc
   == research/pair/CAREER_02_pc_native).

## Delegation log
- (none this session — all analysis done by the orchestrator; no agents spawned)

## Round 3 notes (21:20) — PC install environment
- The game allows 3 careers per alias: the CAREER_04 diagnostic triggered a
  warning on every screen; moved to research/backups/diag_CAREER_04/.
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
  not JOSHUA S 10).

## Round 5 (21:30) — mid-race-day pair
- New oracle: research/pair_raceday/ (CAREER_02 at the "Battle Machine"
  race day in Nevada, saved on both platforms).
- GameplayData is NOT a fixed struct: while a race day is in progress
  (u32 at PC 0x2D4 == 1) a race-day block occupies 0x2E0..0x3E70 and the
  event list follows. The 360 block has a 4-byte pad at 0x314 the PC lacks;
  fixed (fix_raceday_block) + u8 per-event flag words (0x434..0x784 step 16)
  and the name string at 0x300 kept natural. Physics floats differ only in
  noise bits. tests/test_raceday.py.
- Installed: converted raceday 360 save as SAVE/.../CAREER_02 (native PC
  copy archived byte-exact at research/pair_raceday/CAREER_02_pc_native —
  copy it back to restore). User to test: does CAREER_02 resume the Battle
  Machine race day like native did?
- IMPORTANT: the user's 360 CAREER_01 has NO active race-day block
  (0x2D0 looks like the fresh career) - so its "resume Willow Springs"
  state is not in GameplayData's block; this fix does not change CAREER_01.
  Next: once CAREER_02 is confirmed, diff the remaining chunks of CAREER_01
  vs the resume behaviour (FECareer/RaceData/alias-side state).
