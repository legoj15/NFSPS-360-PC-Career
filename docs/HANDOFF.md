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
