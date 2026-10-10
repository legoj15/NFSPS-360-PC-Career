# Handoff — NFSPS 360 -> PC converter (updated 2026-10-09)

## 2026-10-09 — twin-recovered records convert like the main loop
- convert_tree's per-record work (normalize, GameplayData/NUMERIC_IDS swap,
  convert_record, apply_struct_fixes, auto-mode warning, tail_word,
  _to_pc_record, rehash_gameplay) is now one helper, convert_to_pc_record,
  plus record_spills for the spill words; used by the main loop and the
  --twin merge (Python convert.py, Rust convert.rs). Twin-recovered records
  previously got only convert_record + zero tail, so a recovered
  GameplayData/RaceData/FECareer/CRD came out wrong (no swap, no MD5,
  career name and race-day fixes missing). Recovered records now take their
  spill from the twin's own chain. PS still has no twin path (documented).
- Non-twin outputs unchanged (goldens green). Twin digests moved:
  test_twin.rs last-record 7efcaece -> d9a44137, duplicate 6a477442 ->
  9dd796f4 (UnlockSystem now gets its real tail word), recomputed with the
  Python converter (HEAD Python reproduced the old pins first).
- Tests (written first, failed before the fix): tests/test_twin.py damages
  every record from GameplayData on and checks each recovered record equals
  the plain conversion of the twin; test_twin.rs
  twin_recovered_gameplay_racedata_fecareer_match_python pins the Python
  digest e517abc0. pytest 81 + 9 skipped, cargo workspace green, PS 44/44.
- Note: HANDOFF lists the whole twin path as deletion-candidate debt; this
  fix keeps it correct while it exists, it does not argue for keeping it.
- Review (opencode shop26 Qwen 27B, 11 min): approve. Acted on: the
  pre-validate twin normalize now discards its warnings (an untrimmable
  GameplayData warned twice); Rust test asserts all six recoveries.
  Open (pre-existing, main loop only): on an INTERNAL gap re-anchored by
  Tree._reafter_gap, record_spills gives the last pre-gap record the first
  re-anchored record's header word as spill; it should get b"" like a
  trailing gap.
- Delegation log: none (orchestrator; serial, tightly scoped).

## 2026-10-09 — PS RehashGameplay short-payload parity (closes the DEFERRED low below)
- RehashGameplay no longer throws on a GameplayData payload < 0x24 B: it
  hashes the clamped tail and grows the buffer to min(len,0x14)+16 like
  Python's bytearray slice assignment / Rust rehash_gameplay. Unreachable
  via the pipeline (FixRacedayBlock refuses < 0x2DC), unit-level parity only.
- Tests (written first): Run-Tests.ps1 reflection units with the
  test_short_records.rs vectors - 'tiny GameplayData rehash grows like
  Python' (failed before the fix) and 'CARDB fixes clamp on a 0x100-byte
  payload like Python' (already passed; d5ccf51 closed that gap). Goldens
  unchanged; 5.1 and 7 green.
- Delegation log: none (orchestrator; serial, tightly scoped).
- Review (opencode shop26 Qwen 27B, 6 min): confirmed byte parity for all
  lengths; 1 high REJECTED ("GetField can't see a C# const, test aborts the
  suite") - consts are literal fields GetField returns, and the test failed
  pre-fix with the RehashGameplay throw, which needs Id == GameplayId.
- Landing: main checkout was mid-merge (another session) at first try;
  merged main into the worktree branch afterwards, then fast-forwarded main.

## 2026-10-09 — merged alias-options branch into main
- Branch claude/pc-speed-hud-wheel-animation-1ae827 and main (dc305ef) fixed
  the same "last word sits in the next record's header" bug independently:
  main's tail_word (all records, in-game verified on careers/race day) vs
  the branch's alias_tail (aliases only, careers zero). Resolution: main's
  tail_word stays the rule; the branch's scalar detection is folded in as
  scalar_tail (u8 trailing node natural, len 1..8 scalar nodes swapped),
  used when main's rule would keep the word natural. Branch's u8 node rule
  and VideoSettings trim kept as-is.
- Result: career goldens identical to main (in-game verified); alias output
  identical to the branch except SavableStats (0x8FFBE3E8) last word,
  which keeps main's natural 0x00004000 (branch zeroed it). Alias goldens
  moved (Extracted 5d8ab470..., anon e2b29e6e...). py 76, cargo all, PS 44.
- Still NOT verified in-game: HUD speed gauge / options on the merged alias.
- Delegation log: none (orchestrator merge, serial).
- Follow-up (same evening): user saw "too many aliases" + a CAREER_ª. The
  strays (ALIAS_Player, CAREER_ª) were written 20:45, BEFORE the merged
  build: the game ran on the branch-only alias (no used-size fix from
  dc305ef) and fell back to a default profile. The 20:51 merged outputs
  hash to the goldens; alias extra used = tree used (0x31D0), record
  layout matches the native ALIAS_Player except PCControllerSettings
  (size 0, as in main). Strays moved to SAVE/SaveConverter backups/
  2026-10-09_strays2/. Idea: converter could warn when the target folder
  already holds another ALIAS_* or a CAREER_ with a non-ASCII name.
- CORRECTION: strays came back at 20:55 with a clean folder -> the MERGED
  alias (5d8ab470) is REJECTED by the PC (falls back to 'Player'); the
  branch's fixes were never actually loaded in-game. vs dc305ef (loads):
  only (a) ~40 u8 node words 00000001 -> 01000000 and (b) VideoSettings
  0xB4 -> 0x74 (used 0x3210 -> 0x31D0). Post-record area same shape.
  Bisect in progress: installed v1 = merged minus the VideoSettings trim
  (md5 4f5cb16f..., PC_PAYLOAD_SIZES cleared). Merged alias + strays in
  SAVE/SaveConverter backups/2026-10-09_strays3/. If v1 loads -> drop
  the trim; else the u8 rule is what the PC rejects.
- UPDATE 21:4x: earlier "rejected" reads were wrong. User: v1 alias LOADS
  (alias screen JOSHUA S 10, no popup), but in a quick race day the camera
  prefs are corrupt + no HUD, and back in the menu the alias list shows only
  PLAYER + "too many aliases" -> the game DROPS the loaded profile mid-session
  and falls back to default 'Player' (that is what writes ALIAS_Player +
  CAREER_<0xAA>). Same-session evidence: ALIAS_JOSHUA never rewritten.
  UserProfile holds 360 constant 0x2848 after the name (native 0) - suspect.
  Record bisect with docs/re/graft_alias.py (base alias + donor records,
  rebuilds used/hash; self-test reproduces a native file byte-exact).
  v2 installed = v1 with all 9 settings records from native ALIAS_Player
  (strays/, 16:25) - md5 d2e66c22. If v2 is clean -> culprit in settings;
  else in Stats/Achievements/OnlineUserProfile/ProfileStats/Jukebox/UserProfile.
  Strays of each test in SAVE/SaveConverter backups/2026-10-09_strays{2..5}.
- v2 RESULT (21:50): profile KEPT (game re-saved ALIAS_JOSHUA itself, no
  ALIAS_Player) -> the profile drop is caused by the converted SETTINGS
  records. Camera, car tech, assists ("King") matched console = they live
  outside those 9 records. From native settings: autosave on, EA Trax on,
  leaderboard shows x/n with no names. Speedometer STILL missing -> not in
  the 9 settings records. CAREER_<0xAA> still created (also happened under a
  pure native Player at 16:25: likely quick race day without a career, not
  ours). Game re-save kept UserProfile 0x2848. Game-saved copy:
  strays6/ALIAS_JOSHUA S 10_pcsaved_2150.
- v3 installed (md5 b4675582): base = game-saved v2, plus SavableStats,
  AchievementManager, OnlineUserProfile, ProfileStats, Jukebox from native
  Player; only UserProfile (+extra) remain converted. Speedometer back ->
  culprit in those 5; still missing -> UserProfile/extra or the career.
  Open thread 2: which settings record/node makes the PC drop the profile.
- v3 RESULT: identical to v2 (speedometer missing, camera/assists = console).
  User then made a fresh PC alias TEST2 -> speedometer + leaderboard fine
  (creating it WIPED the folder: J + CAREER_02/03 gone, CAREER_01 overwritten
  by TEST2's career; all regenerable; TEST2 pair in backups/2026-10-09_test2).
  KEY FIND: the game-written CAREER_<0xAA> is NOT fresh - it is the converted
  career re-saved (custom race days, stats). Cause: FECareer's 36-byte node
  = career-slot name [4 junk][32 chars]; 360 "01\0"+0xAA fill was u32-swapped
  to "\xaa\0""10" -> PC names the career CAREER_<0xAA>. Present in every
  converted career ever (pair_customrd README blamed the alias). FIXED in
  d58ec2b (fix_career_name, all 3 ports, tests/test_career_name.py).
  Camera/assists/speedometer follow the CAREER, not the alias.
- Installed for test 4 (22:2x): fixed careers from c1_latest/CAREER_01_360
  (3629c855), pair_raceday/CAREER_02_360 (ec77c930), Extracted CAREER_03
  (7e9cc084) - the same sources the user's installed set hashed to - plus
  v1 alias (full converted settings, no VideoSettings trim, 4f5cb16f).
  Watch: CAREER_<0xAA> gone? profile kept? speedometer?
- Test 4 RESULT: career-name fix CONFIRMED (game re-saved CAREER_01, no
  CAREER_<0xAA>). Profile still dropped after the race day (ALIAS_Player) ->
  thread A: converted settings records. No speedometer/leaderboard, camera
  corrupt -> thread B: career-side (also seen with native alias settings).
  Results in backups/2026-10-09_test4 (game-saved CAREER_01 = 42d761cd).
- Test 5 installed (combined split): alias = v1 with Video/FF/Gameplay/Audio/
  PCController from native Player, PlayerSettings0-3 converted (43e78773);
  CAREER_01 = fixed career with GameplayData+RaceData from TEST2's fresh
  career (6ff9e5b1); CAREER_02/03 fixed. Drop -> PlayerSettings; no drop ->
  the other 5. Speedometer back -> GameplayData/RaceData; else FECareer/
  CarDB/CRD/Speech/ProfileStats/Marker/Unlock.
- Test 5 RESULT: speedometer PRESENT, leaderboard works (after toggling on),
  profile KEPT, game re-saved both normally. -> thread A culprit in Video/
  FF/Gameplay/Audio/PCController (PlayerSettings0-3 converted are fine);
  thread B culprit in GameplayData or RaceData. Game-saved files in
  backups/2026-10-09_test5 and scratchpad t5saved/.
- Test 6 installed: alias = v1 + PCControllerSettings only from native
  (a5a24e0f; converter writes it size 0); CAREER_01 = fixed + RaceData only
  from TEST2 (00172c13). No drop -> PCController filler is thread A.
  Speedometer back -> RaceData; missing -> GameplayData (race-day state).
- Test 6 RESULT: everything works (camera, HUD, audio, jukebox, assists,
  network, units all match the 360 per the user's side-by-side check). ->
  A = size-0 PCControllerSettings, B = RaceData. FIXED:
  546a8e5 RaceData = NUMERIC_IDS (swap all + fix_node_flags): its fieldmap's
    ZERO/DIFF slots (from fresh careers) hit the string heuristic, leaving
    race times like 0x42724630 (60.57 s, "BrF0") big-endian.
  c7c784d PCControllerSettings = native defaults (pc_controller_default.bin,
    game default profile; Rust include_bytes, PS base64 + test); VideoSettings
    trim dropped (verified layout keeps 0xB4). Personal alias output now ==
    the in-game-verified test-6 alias (a5a24e0f).
- OPEN: turn indicators shows On (360: Off). Converted values all equal the
  360's. In the game re-save (scratchpad t6saved / backups test6) PlayerSettings0
  node 31 (0x1F0) went 0->1 (leaderboard the user turned on) and node 32
  (0x200) 1->0 - asked the user whether they turned turn indicators off; if
  so node 32 has inverted meaning or a different option order on 360.
- TURN INDICATORS (22:56): fresh conversion loads with turn indicators On.
  User toggled only that option Off in-game; game re-save diff (data words,
  flag junk ignored) = PlayerSettings0 PC offset 0x200 (node 32) 01 -> 00.
  So PC node 32 = turn indicators, 1 = On. The 360 holds 1 there but shows
  Off; neighbours (nodes 29-33 = 1,1,0,1,1; 360 UI minimap On, best line
  On, turn Off, leaderboard Off; PC node 31 = leaderboard) fit no simple
  order. Pending 360-side experiment: user flips turn indicators On on the
  360, re-copies the alias to USB -> diff 360 aliases: node 32 changes ->
  invert on convert; another node -> option order differs, remap. Also
  observed: the game's own re-save trims VideoSettings to 0x74 (so the PC
  accepts both lengths). Game-saved copy: scratchpad ti_saved.
- 360 RESULT (23:00): user flipped turn indicators On on the 360 and
  re-saved -> no value changed (that save did not carry the change).
  CORRECTED 23:20: after a few more 360 saves (+ leaderboard On) the alias
  shows PlayerSettings0 node 31 (0x1F0) 0->1 and node 32 (0x200) 1->0, and
  the converted save shows leaderboard On / turn indicators Off in-game.
  So both platforms store them identically (node 31 = leaderboard, node 32
  = turn indicators, 1 = On); the original 360 save really held turn
  indicators On. NOT a converter bug. CLOSED.
  Side find, FIXED: on-chain one-byte nodes can carry 360 heap junk in their
  pad (01 00 13 10) and flag (00001b10) bytes; the old rule (pad must be 0)
  u32-swapped them (PC read 0x10 / 0x5d). fix_node_flags now keeps the first
  byte for len-1 nodes on the node_spans chain and skips false [0][len]
  matches whose len word is such a node's data. Off-chain matches keep the
  old guard. All prior goldens unchanged; new fixture
  docs/re/alias_anon_junkpad (anonymized 360 save, golden 43a94087). Not
  changed (no sample): scalar_tail's u8 rule still requires a zero pad.
- Final check installed (22:4x): pure converter output for all four files
  (alias a5a24e0f, C01 5b7d3fcb with the real RaceData fix, C02 ec77c930,
  C03 77e5b3e9). Test-6 files in backups/2026-10-09_test6.
- Merge review (opencode: shop26 Qwen 27B approve, glm-flash): no parity
  bugs. Acted on: PS scalar_tail 'len 0' vector (46/46). PENDING after the
  bisect: docs/re/alias_anon/README.md still cites the branch's pre-merge
  md5s (4056c0e2 / 5f04f3ff). Rest pre-existing (PS no --twin, documented)
  or already fixed (RehashGameplay, ec9b709).

## 2026-10-09 — PS short-record clamping (GLM review follow-up)
- Convert-NfsSave.ps1 C#: FixCarDbParts / FixBlueprintSet / ConvertDecal now
  write through CopyNat / Swap16Nat (Python slice clamping, like Rust
  py_slice/copy_nat/put_mapped), so a CARDB record shorter than the fixed
  offsets (base 0x2684...) converts instead of throwing ArgumentException.
  FixRacedayBlock refuses a GameplayData payload < 0x2DC with the Rust
  message ("GameplayData chunk too short (0x.. B) to hold the race-day
  state - the source file is corrupted"); its fixed copies clamp too.
- Tests (written first, both failed): Run-Tests.ps1 builds the same crafted
  fixtures as test_short_records.rs in-process (New-ShortRecordMc02; tree
  hash functions lifted from the converter's AST) - short CARDB -> md5
  bdb741bf..., 0x2D8 GameplayData -> refusal message. 39/39 on 5.1 and 7
  (42/42 with Extracted/ present), cargo test --workspace green, pytest green.
- Also fixed: tests/test_extra.py test_used_matches_tree failed in any
  checkout without the gitignored Extracted/ (now skips that subtest).
- Delegation log: none (orchestrator did it; serial, tightly scoped).
- Review (opencode triad 17:38, shop26 Qwen 27B + glm-flash): shop26 timed
  out (45 min) with no findings; GLM confirmed parity for all lengths, 1 low
  DEFERRED: PS RehashGameplay throws on < 0x24 B where py/rs grow the buffer
  - unreachable (FixRacedayBlock refuses < 0x2DC first).

## 2026-10-09 — Race Day crash + alias never loading (all VERIFIED IN-GAME)
Final state (committed together):
- RACE DAY CRASH (main-menu Race Day, converted CAREER_01): null deref at
  nfs.exe 0x7F6480 on an FEMapHub with 0 events. Hubs 0x10..0x14 = the 5
  CUSTOM race-day slots (career [0xAB9DC8]+0xB0 = 8F7CCCE0 46AE8E2F C8A0888E
  0A6C2097 AF51A403; built for all slots by 0x56BA80). Live breakpoints on
  the CustomRaceDayMemcard loader (vtable 0x96F1D4 slot 3 = 0x5473E0) showed
  slot 0's record parsed with name/GUID/settings but ZERO events.
  Cause: the string heuristic left the [0][len] header after a NUL-padded
  race-day name big-endian (len 0x04000000) -> event list dropped.
  Fixes: fix_node_flags swaps every [0][len] header; CustomRaceDayMemcard
  string nodes copy structurally (node_spans + fix_custom_raceday_strings;
  GUIDs were half-swapped). VERIFIED: Race Day opens, custom race day loads.
- RECORD TAIL WORDS (general): a 360 record's final value sits in the NEXT
  record's header slot (last record: first word of post). Was zeroed; now
  carried (tail_word: swap after a u32 node / raw blob, natural otherwise,
  GameplayData zero). E.g. CRD last event flag 1, FECareer 0x2848, alias
  AudioSettings 3 / PlayerSettings 2 (native PC tails are small LE values).
- RACE-DAY PROGRESS TABLE (GameplayData, 90 x [key][state][score], first key
  0xA70EA9B0, last 0xFA5D360A): CONSOLE_ONLY_RACEDAYS resets 17 entries the
  360 always marks (even fresh, no custom race days) and the PC never writes
  (every PC save 0/2, score 0; a PC save after creating a race day left them
  untouched). 5 of them are the custom slots. NOT the crash cause (first
  in-game test still crashed). Open: meaning of the other 11 keys' 360 state.
- ALIAS NEVER LOADED ON PC (pre-existing): extra word 1 (used tree size) was
  patched for careers only; aliases kept 0x3204 vs tree 0x3210 (+12 from the
  inserted size-0 PCControllerSettings) -> PC silently used a default
  'Player' profile; its first in-game save wrote ALIAS_Player + a career
  named CAREER_<0xAA> (= PC uninit fill). FIXED for all MC02 files.
  VERIFIED: JOSHUA S 10 loads, no popup. Untouched alias header diffs:
  word 0 = 360 vtable 0x8209E9A8 (native 0x974BAC; careers load with it),
  0xAA fill after the name.
- User's SAVE folder: installed CAREER_01 + alias from this code; strays in
  SAVE/SaveConverter backups/2026-10-09_strays/ (2x ALIAS_Player, CAREER_ª).
- Oracle: docs/re/pair_customrd/ (PC-written custom race day; MAC in GUID
  anonymized, see its README). The PC writes the same CRD layout as the 360:
  header [u32][count<=5]; per race day [slot][13 u32: mode, 3 x (car key,
  flag), ...][GUID 25 B = 4 junk + 21][name 36 B = 4 junk + 32][NumEvents]
  [(event key, u32) x N].
- RE facts: 0x5322F0/0x5321D0 (CRD record read/write) are SecuROM-VM bytecode
  (push/pushfd/ret into 0x1166690) - don't try to read them; use live
  breakpoints on the unprotected callers instead. 0x532820 is the shared
  race-day TEXT parser (GUID:, RaceName:, NumEvents:...), not the memcard path.
- Tools added (docs/re): vsdbg.ps1 (live VS state via EnvDTE, PS 5.1),
  vsgo.ps1 (resume to next breakpoint + eval), vsmem.ps1 / vsstack.ps1 (dump
  memory/stack via VS), procmem.ps1 / procscan.ps1 (ReadProcessMemory read /
  string scan, no debugger), vscode.ps1 (memory save via VS), calls.py
  (direct call sites), rdtable.py (progress table side by side).
  Breakpoints: Debugger.Breakpoints.Add("0x005473F9") binds by address.
  When attached via Attach-to-Process, expression eval after Break() fails
  (0x89711006); use breakpoints or procmem instead.
- Review (opencode triad, 16:31, shop26 Qwen 27B + glm-flash): shop26 timed
  out with no findings; GLM 10 lows/1 medium. REJECTED: "[0][len] scan
  reverses values to BE" (it writes the swapped form, a no-op for u32 data).
  FIXED: stale golden docstrings (py + rs), PS TailWord dead condition, test
  helper bounds. DEFERRED (low): last record's tail is zeroed whenever the
  tree has a gap, even a recovered internal one. Spun off (pre-existing,
  medium): PS FEPlayerCarDB fixes throw on short records where py/rs clamp.
  The alias used-size fix landed after the review (one line x3, goldens).
- Delegation log: impl (Sonnet medium) x2 -> Rust + PowerShell ports of the
  progress table, node framing, CRD strings and tail words; ok first try
  both rounds, 0 escalations; used Python replace scripts on Rust sources
  once (aborted, redone with Edit) and once on non-literal Rust text.
  Orchestrator: all RE/debugging, Python reference, alias fix port.

## 2026-10-09 — converted alias: options read as off, HUD gauge hidden
- User report: with a converted alias, the race speed/RPM gauge never
  shows, camera = bumper, ABS/TCS/ESC off, assists casual on every launch;
  a fresh PC alias is fine. Ruled out in the game install first (FusionFix
  aspect/SimRate, FE_ATTRIB.BIN, HUD .bun) by the game-folder session.
- Byte-level cause (personal alias vs fresh PC `ALIAS_TEST`), 3 bugs:
  1. len-1 property nodes were u32-swapped: 30+ on/off options (Gameplay,
     Video, PlayerSettings0-3, OnlineUserProfile) read 0 on PC.
  2. VideoSettings kept two 360-only trailing nodes (180 vs native 116 B).
  3. Every record's last data word was zeroed: the 360 stores it in the
     word the parser called the next record's "type" (FORMAT-NOTES).
- Fix in all three converters (Python, Rust core, PowerShell), byte-exact:
  fix_node_flags keeps [u8][000] node data natural; Record.tail +
  alias_tail (aliases only); VideoSettings trimmed to 0x74. Career outputs
  unchanged (all career goldens identical); alias goldens updated.
  Tests: tests/test_alias_settings.py, nfssave-core
  tests/test_alias_settings.rs (failed before, pass after), Run-Tests.ps1
  goldens. NOT yet verified in-game: which option hides the HUD gauge is
  unknown; the fix restores every option, the user must confirm.
- Not fixed, worth a look: AudioSettings node 8 carries 360 heap fill
  0xAAAAAAAA as its f32 (~ -3e-13, PC default 0); PCControllerSettings is
  still a size-0 filler (PC fills defaults); careers' FECareer last word
  (360 constant 0x2848) left zero.
- Delegation log: implementation by orchestrator (serial scoped work).
  reviewer (Haiku) on 3ee4858 -> pass with nits, 0 parity bugs; acted on:
  alias_tail edge-case unit tests (py + rs), softened the "verified" tail
  claim (fresh PC alias is not in the repo). Rejected: "len-1 node may hold
  a u32 0x3F000000" (len is the byte count). Kept by design: tail cleared
  before a damaged gap (aliases never take the gap/twin path in practice).
  opencode triad (shop26 Qwen 27B + glm-flash high) on 3ee4858 -> no parity
  bugs in new code; acted on: u8 rule and alias_tail now also require a
  node flag word [u8][FFFFFF|000000]; alias_tail also carries the tail of
  a trailing 8-byte node (d2); chunk-set equality in the size test; stale
  "junk bytes" docstrings; PS unit tests for AliasTail/FixNodeFlags. All
  goldens unchanged. Not done (latent): validate_twin vs PC_PAYLOAD_SIZES
  trim (careers carry no VideoSettings); PS throws on corrupt short
  CarDB/GameplayData records where Py/Rust clamp (pre-existing, spun off).

## 2026-10-06 — script CLI redesign (user request)
- Contract: docs/scripts-cli.md. Inputs = files or folders (recursive,
  CAREER_/ALIAS_ + "CON " magic, skips backups) and --usb/-Usb (old
  --flash/-Flash kept as alias). Default output = current directory (the
  Documents lookup is gone); game folder auto-detected (R/SAVE/NFS ProStreet,
  R/NFS ProStreet, or R named NFS ProStreet); plain-mode backups stay inside R.
- DONE: Python (tests/test_cli.py, 53 green) and PowerShell (Run-Tests.ps1
  36/36 on 5.1 + pwsh 7). README "Using the scripts" section rewritten.
  R is created lazily by the first write (a run where every source fails
  leaves nothing); an empty --usb/-Usb is one failure, other inputs still run.
- Delegation log: impl (Sonnet medium) -> PowerShell CLI port, ok first try,
  0 escalations; orchestrator aligned eager-mkdir and -Usb early-exit with
  Python afterwards.
- Review scripts-cli-redesign (ab6c6de..bb0ed27): shop26 Qwen 27B + glm-flash
  (high). REJECTED (probed): Qwen high "Find-SaveFiles `, $hits` breaks
  multi-save folders / no-saves branch dead" - unary comma is the
  no-unroll idiom; new 'folder with several saves' test passes on the old
  code. FIXED in both scripts + tests: overlapping inputs de-duplicated;
  claim only on success (exe parity); Python key folds trailing dots/spaces;
  Python --out-root file -> exit 2; "" input rejected; no banner when every
  input failed; tests for any-name file, --all no-op, named save folder E2E.
  PowerShell "failed save does not claim" has no black-box test (no fixture
  fails after the name check); Python pins it. Not fixed (low, pre-existing):
  dry run with an unsafe STFS name exits 1 in PS, 0 in Python.
  Python 63 green, Run-Tests.ps1 40/40 on 5.1 + pwsh 7.
- Decision: the Windows app keeps its Documents auto-detection (user).

## 2026-10-06 — fatx: 0x1691 relabelled TITLE name
- `stfs::TITLE_NAME_OFFSET/LEN` (0x1691) + `ConHeader::title_name`;
  `DISPLAY_NAME_OFFSET/LEN` now mean 0x411 (locale-0 slot, unparsed).
  Oracles hold "Career 01"/"Career 02"/"ANONYMOUS 1" at 0x411, pinned in
  tests/stfs_oracle.rs. SPEC.md §7 corrected. No behaviour change.
- Pre-existing `cargo fmt --check` drift in fatx/nfssave-core files
  (real_scan.rs, aligned.rs, device.rs, ...) - left untouched.
- Delegation log: shop26 Qwen 27B review -> approve, 1 low (SPEC §7
  "identical bytes" preamble), fixed.

## 2026-10-06 — anonymized alias golden fixture
- `docs/re/alias_anon/ALIAS_360` (container `ALIAS_ANONYMOUS 1`), built by
  the re-runnable `docs/re/anonymize_alias.py` from the personal alias;
  provenance + what is stale in `docs/re/alias_anon/README.md`. Seventh
  golden case in all three suites, md5 8ae3d82a3c9cb1c9500d6fcce8c01b9d;
  converted output differs from the personal golden only in MC02 CRCs, the
  extra name, the PC tree hash and the UserProfile name (checked bytewise).
- Name sites found: STFS file table, CON display name (locale 0), MC02 extra,
  UserProfile chunk. OnlineUserProfile carries no strings/XUIDs. Blanked:
  cert body 0x06..0x22C, console id, profile id (offline XUID), device id
  (USB serial). The 360 tree[0:0x10] does NOT match the PC treehash scheme.
- Golden suites now FAIL (not skip) on a missing tracked fixture; only
  `Extracted/` sources skip. New unit tests pin convert_extra's no-NUL
  branch (tests/test_extra.py, nfssave-core tests/test_extra.rs,
  Run-Tests.ps1) since every golden alias NUL-terminates its name.
- Delegation log: review fan-out alias-anon-fixture (013197b..e37702d):
  shop26 Qwen 27B -> 1 medium, REJECTED: "display name is at 0x1691, not
  0x411" - the source save holds the player name UTF-16BE at 0x411 (STFS
  display name, locale 0); 0x1691 is the STFS TITLE name. The fatx crate
  (stfs.rs DISPLAY_NAME_OFFSET, SPEC.md §7) mislabels it - spun off as a
  separate task. glm-flash -> partial (tool-calls; sandbox auto-reject),
  its notes report pin, container name, identity scan and all three suites
  green, no findings.

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
  Review ps-port-review (shop26 Qwen 27B + glm-flash, 4dcbc3e..77bd8d9),
  verified + fixed in the follow-up commit, 22/22 on 5.1 and 7.7:
  bare `-Flash F` (no colon) -> drive root; -Flash names case-insensitive
  (exe parity); duplicate container names in one run refused (exe
  batch.rs); 1-3 byte record payload framed like Python (GLM repro'd a
  crash); tests for backup `<stamp>-2` and short records; .gitattributes
  `*.rules eol=lf` + EOL-agnostic freshness test. REJECTED (probed):
  "Join-Path 'F:' 'Content' is drive-relative" and "drive-root OutRoot
  backs up drive-relative" - PowerShell Join-Path yields `F:\Content` /
  `D:\SaveConverter backups`. Alias coverage gap CLOSED: see next section.
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
- Python CLI debt FIXED (tests/test_cli.py): PC_SAVE_ROOT removed,
  write_pc_save needs an explicit root, CLI defaults to known-folder
  Documents; bare drive letters -> drive root; CLI backs up replaced saves
  (nfssave.convert back_up_existing, same layout as app/batch.rs). The
  known-folder lookup had a wrong FOLDERID_Documents GUID and always fell
  back to %USERPROFILE%\Documents - the hardcoded path hid it; fixed + test.
  Remaining nit: --dry-run still requires the output folder to exist.
  Review py-cli-fixes (shop26 Qwen 27B + glm-flash low, 1cee3e4..853dc0d):
  11 low; fixed as follow-up: --flash --all duplicate-name refusal,
  backup failure -> clean "left it untouched" exit 1, absolute() parity,
  "F:\" on non-Windows. Rejected: "F:\ not normalized" (already a root).
  Not done: test gaps for main()-level shared stamp and ctypes failure paths.

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
