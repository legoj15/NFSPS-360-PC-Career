# docs/re — reverse-engineering workspace (agent-facing)

Format findings: `FORMAT-NOTES.md` (read its last section first for what is
verified in-game). Everything here is a research record; the probe scripts
assume the author's machine (game install under `E:/legoj/Documents/...`,
360 dumps under the gitignored `Extracted/`). Converter behaviour is
specified by `scripts/python/nfssave/convert.py`, not by these scripts.

## Fixtures (tracked unless noted)
| Folder | What it is |
|---|---|
| `pair/` | Matched fresh CAREER_02: `CAREER_02_360_fresh` and `CAREER_02_pc_native`. Golden and STFS oracle. |
| `pair_raceday/` | CAREER_02 saved mid race day ("Battle Machine", Nevada) on both platforms. `CAREER_02_pc_native` was archived unmodified from the game (no checksum recorded); copy it back to restore the user's native save. Race-day block oracle (state 1, 0x3B90 B). |
| `pair_customrd/` | PC-written custom race day (`CAREER_aa_after_pc`); CustomRaceDayMemcard oracle. MAC in the GUID anonymized. |
| `c1_latest/` | Latest 360 CAREER_01 with the long race-day block (state 3, 0xB2D0 B). |
| `oracle/` | Native fresh PC saves: `native_ALIAS_Player` (used by the alias-settings tests) and `native_fresh_CAREER_01` (unreferenced, unique). The fresh CAREER_02 pair is in `pair/`. |
| `alias_anon/`, `alias_anon_junkpad/` | Anonymized 360 aliases (golden case 7; junk-padded one-byte nodes). READMEs say how they were built. Regenerate with `anonymize_alias.py`. |
| `fieldmaps/` | Positional field maps from the first fresh pairs. Superseded by the node grammar but still embedded by `payload_rules`; see docs/backlog-research.md before deleting. |
| `backups/` | Old 2026-10-04 PC save-folder backup and a CAREER_04 diagnostic. Historical; not referenced by tests. |

## Live-debugging tools (Windows, game running)
- `vsdbg.ps1` reads a live Visual Studio debug session of nfs.exe through
  EnvDTE. `vsgo.ps1` resumes to the next breakpoint and evaluates;
  `vsmem.ps1` and `vsstack.ps1` dump memory and stack through VS;
  `vscode.ps1` pauses, saves memory, resumes. **All five are Windows
  PowerShell 5.1 only** (stated in each header).
- `procmem.ps1` / `procscan.ps1` ReadProcessMemory read and string scan with
  no debugger attached.
- Technique notes: set breakpoints by address (`Debugger.Breakpoints.Add`
  with a string like "0x005473F9"); after `Break()` on an Attach-to-Process
  session, expression evaluation fails (0x89711006), so use breakpoints or
  `procmem.ps1` instead. SecuROM-VM bytecode must not be disassembled, see
  FORMAT-NOTES (CustomRaceDayMemcard, RE traps).

## Offline tools (no game or debugger needed)
- `calls.py` direct call sites in nfs.exe (reads the exe from disk);
  `rdtable.py` race-day progress table side by side; `graft_alias.py` builds
  an alias from a base alias plus donor records (rebuilds used size and tree
  hash) for record bisects, e.g. "which converted record makes the PC drop
  the profile"; `anonymize_alias.py` (see `alias_anon/README.md`).
- Older probes (`parse360.py`, `parsepc.py`, `walk360.py`, `walkpc.py`,
  `match.py`, `pairmap.py`, `sweep.py`, `tree_hash.py`, `emu_hash.py`,
  `disat.py`, `xref1.py`, ...) mostly carry no usage text; read the code.

## The author's test environment
- PC install is a ChemicalFlood repack of v1.1, not a clean install:
  FusionFix (FramerateUncap=1, SimRate=-1 = monitor refresh), NFS_XtendedInput
  (input remap), d3d9-wrapper (FPSLimit=60), Ultimate ASI loader
  (dinput8.dll). The repack also ships modified car data ("car fixes") and its
  own FAQ admits garage crashes with DLC cars plus stage-4 kits, so a garage
  crash can be repack-side, not converter-side. Part IDs in the modified car
  data could differ from the 360 title update.
- FusionFix aspect/SimRate, FE_ATTRIB.BIN and the HUD .bun were ruled out as
  the cause of the converted-alias option symptoms.
- The game's real save folder is the Documents `Need for Speed ProStreet/SAVE/
  NFS ProStreet`; the game accepts third-party saves there.
