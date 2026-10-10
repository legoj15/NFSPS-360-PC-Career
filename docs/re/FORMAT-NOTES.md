# NFS ProStreet save format — verified findings

Agent-facing technical notes (working document). Marked VERIFIED = checked
against real files and/or disassembly. Sources: direct RE of nfs.exe (PC,
v1.1-era build), the 360 static recomp (E:/GitHub/NFSPS360, TU 11.0.2.0),
and empirical cross-analysis of 4 sample saves. Supersedes the earlier
empirical-only notes (kept in git-less history below where relevant).

Probe scripts in this folder assume the author's machine: game install at
E:/legoj/Documents/..., 360 dumps under Extracted/ (gitignored), and a
360-formatted flash drive at F:. They are research records, not portable
tools.

## Xbox 360 side

### Console container (`CON ` files in `Content/<profile>/<title>/00000001/`)
- CORRECTED 2026-10-04 late: these ARE standard STFS packages (header
  size @0x340 = 0x971A -> first hash table 0xA000; block separation 0 ->
  two copies per hash table; file table = data block 0 @0xC000, entry
  flags 0x40 = contiguous). Data blocks interleave with hash tables
  (group after every 170 data blocks), so payloads > 0xAA000 B (170 blocks)
  are NOT one slice. See nfssave/container360.py. The earlier "not STFS" claim and the
  entry-field guesses below (hash/checksum) were wrong: +0x29 block count,
  +0x2F start block (LE24), +0x38/+0x3C timestamps.
- 0x0000 `CON ` + console serial/date ASCII + console cert (~0x1AC);
  0x01AC..0xC000 per-file metadata, PNG icons; 0xC000 entry table:
  filename NUL-padded, +0x28 hash, +0x2C block count, +0x30 flags
  `00 00 ff ff`, +0x34 BE u32 payload size, +0x38/+0x3C BE checksum ×2
  (algorithm != EA CRC, not needed); 0xD000 raw MC02 payload.
  (SUPERSEDED for the entry fields: the bullet above is right: +0x28 flags,
  +0x29 block count, +0x2F start block, +0x34 BE size, +0x38/+0x3C
  timestamps. Only the size at +0x34 and the 0xD000 payload start hold.)
- Reader: nfssave/container360.py.

### MC02 file (360 = big-endian, PC = little-endian, identical layout)
```
0x00 u32 magic 0x4D433032   0x04 u32 total = 0x1C+extra+tree
0x08 u32 extra size (career 28 / alias 64)
0x0C u32 tree size (career 0xB6800 / alias 0x5000) — fixed buffer sizes
0x10 u32 CRC(extra)  0x14 u32 CRC(tree)  0x18 u32 CRC(hdr[0:0x18])
0x1C extra blob | tree blob
```
- CRC (VERIFIED byte-exact, both platforms): MSB-first table CRC, poly
  0x04C11DB7, seed = BE32(first 4 bytes)^~ , step crc=((crc<<8)|b)^tbl[crc>>24],
  final ~; len<4 -> 0. Runs over raw bytes. PC impl nfs.exe 0x8A18F6
  (table 0xA8DC00); 360 sub_827C75B8.
- 360 career files carry stale tree CRCs (console writer doesn't maintain;
  loader apparently doesn't verify). Converter recomputes everything.

### extra blob
career 28 B: [identity][used tree size][1 (lib version — PC gates ==1 at
0x59D0F0)][career-day count (7)][0][100][f32 playtime].
alias 64 B: same 5 u32s, then NUL-terminated player name, 0xAA pad, then
[50.0f][25.0f]; the PC sample (older alias layout, day=1) carries a 16-byte
hash + extra u32 between name and floats — carried through value-preserving.

## Chunk tree (both platforms)

Record ids are djb2(name) with h=-1, h=h*33+c (VERIFIED:
djb2("MEMCARD_ROOT")=0x59F2D89B — resolves the earlier "magic"; the
name->id hash question is CLOSED).

```
tree+0x000 16B hash of tree[0x10:tree_size] (128-bit fn nfs.exe 0x6D9CE0;
         computed on write, NEVER verified on load: 0x5AABD0 is dead code
         (see the 2026-10-04 final section). The converter still writes a
         valid one: nfssave/treehash.py, probe tool tree_hash.py)
tree+0x010 u32 count (informational: top-level savables on PC; on 360
         counts nested helpers too)
tree+0x014 .. root record: allocator garbage (never read on load) on PC
         0x1AC bytes incl. machine GUID @+0x40; on 360 zeros, root at +0x40
root record [id=0x59F2D89B][size=used][flags 1 + 3B uninit] at PC 0x1C0 /
         360 0x40; children records follow (PC 0x1C8 / 360 0x48)
record = [flags u8 + 3B uninit ("type")][id u32 = djb2(name)][size u32][payload]
         — chains next = cur + 12 + size to root end; nested container
         chunks payload = [u32 1][nested record] (GameplayData ->
         GameplayBinarySavableHelper; FEPlayerCarDB -> VehicleBinarySavableHelper,
         a raw memcpy blob, size-validated 0x90684 by loader 0x59E444)
post region: directory/hash table (360 alias: 475 cells [h1][h2][FFFFFFFF][0]);
         variable length, fills remainder of fixed tree buffer
```
- PC load dispatcher 0x5B3DF0 advances by [slot+4]+0xC; value nodes align
  to 16 bytes (SSE) — pads are palindromic under u32 swap (0x10101010/0).
- Chunk names recovered via djb2 brute force: see nfssave/convert.py
  CHUNK_NAMES. Cross-platform chunks incl. CustomRaceDayMemcard
  (0xD548266C) and UnlockSystem (0xCA269650) — both 360 and PC write them.
  (An earlier note called these two tail records damaged on the console;
  that was a reader bug, see the 2026-10-04 late section.)
  PCControllerSettings (alias 0x39156567) remains PC-only (absent from
  the 360 files).
- CORRECTED 2026-10-09: the 360 word read as a record's header "type" is
  NOT heap junk. Both platforms tile records as [id][size][body], body =
  [flag word (u8 + 3 junk)][nodes...][last data word]; the 360 writes
  [id][size] one word later, so the word before each [id] is the PREVIOUS
  record's last data word (the first one is the root record's flag word;
  the last record's is post[0:4]). Parsers keep it as Record.tail.
  Alias evidence: structural (each payload ends in a [0][len 4][flag]
  header with no data); personal 360 alias tails AudioSettings=3,
  PlayerSettings0=2 equal a fresh PC alias's last values (user's
  ALIAS_TEST, not in the repo - may be shared defaults), while the native
  oracle alias holds AudioSettings=1 there, so the PC word is real
  per-profile data, not a platform constant.
  Converter carries it for every record (tail_word): GameplayData zero,
  raw blobs and trailing scalar nodes swapped (u8 nodes natural), other
  endings (strings, e.g. SavableStats 0x4000) natural. Career FECareer's
  tail is a constant 0x2848 on 360 vs 0 in both native PC pairs; carrying
  it is the in-game-verified career path (2026-10-09), so it stays.
  The PC [flags] slot is the same flag word (native 'aaaaaa01').
- Property node grammar: [u32 0][u32 len][flag word][data, padded to 4].
  len 1 = u8 at the first data byte, [u8][3 pad]; must stay natural (a
  u32 swap made every alias on/off option read 0 on PC). The 360 pad, and
  sometimes the flag word, can hold heap junk (01 00 13 10, flag 00001b10;
  docs/re/alias_anon_junkpad): nodes on the node_spans chain keep the first
  byte whatever the pad. Off-chain [0][1] matches can be numeric data.
- VideoSettings (alias 0xC3EC4947): 360 payload has two extra trailing
  8-byte nodes (0.5, 1.0) vs PC; native PC payload is 0x74 B. The
  converter keeps the 0xB4 payload (PC loads it fine; the game's own
  re-save trims it to 0x74). A trim was tried and dropped (2026-10-09).
- PlayerSettings0: node 31 (PC 0x1F0) = leaderboard, node 32 (0x200) =
  turn indicators, 1 = On, same on both platforms.
- 360 alias post-records directory table: 475 cells [h1][h2][FFFFFFFF][0];
  hashes do not reference record ids.
- PC stores award names as plain strings inside 0x4E8AA143 where 360
  stores compact property nodes (content differs, framing identical).

### Payload node types (empirical)
- 16-byte property groups [u32 0][u32 type=4/8][u32 tag][u32 counter]
- EA strings [0x40|len][len-1 chars] (0x4F 'OADINGTEXTURES'='LOADINGTEXTURES')
- fixed-width C-string name fields ("MV09_Nitrocide\0" + stale tail + 0xAA fill)
- [u8 value][FF FF FF] sub-word fields with uninit tails, natural byte order
  on both platforms (360 '01 ff ff ff' <-> PC '01 00 5d 00')
- floats (BE/LE), e.g. car units `00 0f 00 XX | ff ff 00 aa` + float pairs

## Conversion approach (nfssave/convert.py)
(Early description, partly superseded: the node grammar, per-chunk fixes and
tail words in convert.py are authoritative; the later sections of this file
and the last section ("In-game findings") record why.)
- Container -> MC02 parse -> tree parse -> per-chunk payload conversion:
  fieldmap rules (fieldmaps, positionally valid for these exact
  source files; 156,454/156,454 verifiable slots byte-exact vs PC reference,
  a 2026-10-04 measurement that is not reproducible from the repo today)
  or auto mode (u32 value-preserving swap + string/subword natural ranges).
- PC tree assembly: zeroed head struct (loader never reads it), records,
  post region (clamped to buffer), count = emitted top-level records.
- CRCs and the 16-byte tree hash are recomputed (treehash.py). The PC never
  verifies the tree hash on load, so it was never a blocker.

## Key code addresses
PC nfs.exe: loader state machine 0x89F812 (career) / 0x89FB88 (alias);
CRC 0x8A18F6 (tbl 0xA8DC00), wrapper 0x89F205; header ctor 0x89FFF1;
version gate 0x59D0F0 ([arg+8]==1); serializer 0x5AACC0 (root write
0x5AACF5, record loop 0x5AAD63); load dispatcher 0x5B3DF0/0x5B3E14;
tree hash fn 0x6D9CE0, verify 0x5AABD0; djb2 0x436680; node writers
0x59CD30/CD80/CDD0/CCE0; VehicleBinarySavableHelper size check 0x59E444.
360 (recomp): CRC sub_827C75B8; header builder sub_827BCDB8; writer
sub_827BD2F8 (caller recomp.15:25346); validator sub_827BECE8; loaders
recomp.121/128/129 (magic lis 19779/ori 12338); collector sub_827BF770
(const 1539).

## Sample files
- 360: Extracted/Career/CAREER_01 + F:/.../CAREER_02, CAREER_03 (all ~93%);
  Extracted/Alias/ALIAS_360 (fully CRC-valid)
- PC reference: "100% Gamesave (OPTIONAL)" folder (CAREER_01 +
  ALIAS_PEIROKUNMANWSP); PC save root: E:\legoj\Documents\Need for Speed
  ProStreet\SAVE\NFS ProStreet (empty; game accepts third-party saves)

## Open items
Only one format question is left in this list (the others were closed: tree
hash implemented and never verified on load; absent trailing chunks
tolerated, absent middle chunks must be size-0 fillers, see the 2026-10-04
final section). Unproven fields and other open research are in
docs/backlog-research.md.
- djb2 names for the four consecutive alias chunks 0x8B7D0AAD..B0
  (runtime-generated: same base name + consecutive suffix char; controller
  configs inferred; cosmetic only).

## 2026-10-04 late: tail "damage" was a reader bug
Everything below about damaged console tails is SUPERSEDED:
the noise was STFS hash-table blocks read as payload. All careers parse
9/9 with valid CRCs once the block map is honoured. Further findings
(node flag words, u16 car part slots, GameplayData MD5, race-day block): the
sections below and "In-game findings" at the end of this file; the car-slot
and part layouts themselves are in the docstrings of nfssave/convert.py
(fix_cardb_parts and the blueprint/decal fixes).

## 2026-10-04 final framing correction + IN-GAME VERIFICATION (convert.py rewrite)

The empirical-grammar note above (both platforms `[type][id][size]`) was
wrong about the 360->PC emission. Actual layouts, proven by diffing a
native PC file against the 360 files and by in-game behavior:

- 360 record: `[junk/marker u32][id][size][payload = u32 0x01 marker +
  7-11B junk + entries]`, records from tree+0x48, root record at +0x40.
- PC record: `[id][size][flags byte 0x01 + junk][payload = content + 4
  trailing junk bytes]`, records from tree+0x1CC, root at +0x1C0.
  -> The 360 marker word sits where PC has [id]; a naive BE->LE
  reframing shifts everything 4 bytes early and puts the 360 marker into
  the PC flags slot, which desyncs the loader's positional pairing.
  Symptom: career slots listed, but loading yields a completely fresh
  career (loader skips every chunk, keeps defaults).
- PC tree pre-record region: `[16B hash][u32 count][0x1A0-ish garbage +
  machine GUID][u32 0x59F2D89B][u32 used][u32 0x00000001]` then records
  at 0x1CC. The 0x1AC "preamble chunk" is part of this garbage region.
- Loader (dispatcher nfs.exe 0x5B3DF0) pairs registered savables[i] vs
  record[i] positionally; `slot += [slot+4]+0xC` advance; id mismatch
  skips BOTH; missing middle chunks desync (must be size-0 fillers);
  missing trailing chunks tolerated; count field informational.
- PC-only chunk PCControllerSettings 0x39156567: alias must carry a
  record at index 9 (between AudioSettings 0x9CB326C2 and PlayerSettings0
  0x8B7D0AAD) to keep the pairing aligned. A size-0 one loads, but the PC
  then drops the profile mid-session for a default 'Player'; the converter
  writes the native default bindings (nfssave/pc_controller_default.bin,
  0x684 B) instead (in-game 2026-10-09).
- RaceData 0x51A41B14: u32/float table (track keys, race times), no
  strings: converted as a pure word swap + fix_node_flags. The fieldmap's
  string fallback left times like 0x42724630 (60.57 s, "BrF0") big-endian,
  which killed the race HUD (no speedometer/leaderboard, camera reset).
- FECareer 36-byte node = career-slot name ([4 junk][32 chars]); the PC
  names the file CAREER_<name>. Copied as text (was swapped -> CAREER_ª).
- Conversion rule that fixed everything (nfssave/convert.py
  _to_pc_record): type=0x00000001, payload = 360payload[4:] + the record's
  last data word (drop the 360 leading marker word, re-add the PC trailing
  word). At the time of this note the trailing word was 4 zero bytes; since
  2026-10-09 it is the carried tail word (the "CORRECTED 2026-10-09" bullet
  in the chunk tree section).
- Tree hash (16B at tree[0:0x10]): chained-MD5 x4 + RSA pow(M,E,N),
  E/N tables at VA 0x98CF88/0x98CF48 — computed on write, NEVER verified
  on load (0x5AABD0 is dead code). Implemented in nfssave/treehash.py.
- 360 career tail "damage" (last 2 records, CustomRaceDayMemcard
  0xD548266C and UnlockSystem 0xCA269650) was the STFS hash-block reader
  bug (see above). A genuinely gapped tree converts without the missing
  records (the loader fills defaults for absent trailing chunks) and the
  converter warns.
- Record ids are djb2(name) h=0xFFFFFFFF,h=h*33+c (e.g. 0x59F2D89B
  MEMCARD_ROOT, 0x3B309E09 GameplayData). See CHUNK_NAMES in convert.py.

VERIFIED IN-GAME 2026-10-04: converted the author's alias + CAREER_01
load on PC — CAREER HUB day 7, $1,345,600, 4 repair markers, correct
race-day menu. Game exit save wrote no file changes (no dirty state), so
no native re-save oracle was produced; fresh-native references archived
at oracle/ instead.

## In-game findings 2026-10-04 .. 2026-10-10
Moved here from the retired session log. The fixes were verified in-game by
the user; bullets that are offline or unverified say so. Fixes are in nfssave/convert.py
(Python reference), the Rust core and the PowerShell script; tests pin them.

### GameplayData
- The blob at PC payload 0x14 (0x10000 B) is [MD5(blob[16:])][rest]. The
  loader (0x59E550 -> deserializer [0xAB9D88] vtbl+0x70) rejects a stale
  hash and falls back to defaults: the career starts from scratch (intro
  movie). Every early converted file carried the 360's MD5. Fixed by
  rehash_gameplay.
- While a race day is in progress (u32 at PC 0x2D4 == 1) a variable-length
  race-day block occupies 0x2E0.. and the event list follows. Lengths seen:
  0x3B90 (state 1, pair_raceday), 0xB2D0 (state 3, c1_latest). The 360 block
  has a 4-byte pad at 0x314 the PC lacks; per-event u8 flag words
  ([u8][0][AA AA] pattern; in the pair_raceday block at 0x434..0x784 step
  16, pinned by tests/test_raceday.py) and the name string at 0x300 are kept
  natural. The end is found via the following [0][0x11] list. Block records:
  [u32 kind 0x000?1x10][u16][u16] + float matrices. Oracle: pair_raceday.
- Race-day progress table: 90 x [key][state][score], first key 0xA70EA9B0,
  last 0xFA5D360A. CONSOLE_ONLY_RACEDAYS resets 17 entries that the 360
  always marks (even on a fresh career) and the PC never writes (every PC
  save: state 0/2, score 0); 5 of them are the custom race-day slots. Not
  the cause of the Race Day crash. Meaning of the other 12: open, see
  docs/backlog-research.md.
- A width check of the GameplayData blob against the PC 100% save found no
  width-error signature (two mirror hits, both content). A full field-type
  audit was never done.

### CustomRaceDayMemcard
- Layout (the PC writes the same as the 360; oracle pair_customrd): header
  [u32][count <= 5]; per race day [slot][13 u32: mode, 3 x (car key, flag),
  ...][GUID 25 B = 4 junk + 21][name 36 B = 4 junk + 32][NumEvents]
  [(event key, u32) x N]. The last event flag is the record's tail word.
- Crash fixed: main-menu Race Day with a converted CAREER_01 dereferenced
  null at nfs.exe 0x7F6480 on an FEMapHub with 0 events. Hubs 0x10..0x14 are
  the 5 custom slots (career [0xAB9DC8]+0xB0 = 8F7CCCE0 46AE8E2F C8A0888E
  0A6C2097 AF51A403; built for all slots by 0x56BA80). Live breakpoints on
  the memcard loader (vtable 0x96F1D4 slot 3 = 0x5473E0) showed slot 0
  parsed with name/GUID/settings but ZERO events. Cause: the string
  heuristic left the [0][len] header after a NUL-padded name big-endian
  (len 0x04000000), dropping the event list. Fix: fix_node_flags swaps every
  [0][len] header and the string nodes are copied structurally (node_spans +
  fix_custom_raceday_strings; GUIDs had been half-swapped).
- RE traps: 0x5322F0 / 0x5321D0 (CRD record read/write) are SecuROM-VM
  bytecode (push/pushfd/ret into 0x1166690): do not read them, put live
  breakpoints on the unprotected callers instead. 0x532820 is the shared
  race-day TEXT parser (GUID:, RaceName:, NumEvents:), not the memcard path.

### Alias and extra blob
- Extra-blob word 0 is a vtable pointer. Native PC values: 0x974BB8 (career),
  0x974BAC (alias). The 360 alias holds 0x8209E9A8, and converted careers and
  aliases keep their 360 value (carried across with the other extra words,
  value-preserving swap). The PC loader ignores it, so it is not rewritten.
- The 360 UserProfile chunk holds a constant 0x2848 in the word after the
  player name (native PC: 0). The converter carries it through value-
  preserving and the game's own re-save keeps it, so it is not what makes the
  PC drop a profile (that was the size-0 PCControllerSettings).
- Extra word 1 (used tree size) used to be patched for careers only. The
  converter inserts the PCControllerSettings record (12 + 0x684 B), so the
  360 value was stale; the PC silently refused the alias, ran on a default
  'Player' profile and its first save wrote ALIAS_Player. Now patched for
  every MC02 file.
- Before the fixes a converted alias showed: no speed/RPM gauge in a race,
  camera on bumper, ABS/TCS/ESC off, assists on casual, every launch, while
  a fresh PC alias was fine. The causes were separate (which symptom came
  from which was not isolated in every case): len-1 property nodes
  u32-swapped (30+ on/off options read 0), RaceData string heuristic (HUD
  gauge, leaderboard, camera) and the size-0 PCControllerSettings (profile
  dropped mid-session). A game-side clue: the alias list then shows only
  "Player" plus a "too many aliases" popup.
- AudioSettings node 8 carries 0xAAAAAAAA (360 heap fill) as its f32; the PC
  default is 0. No visible effect so far (docs/backlog-research.md #2).
- Expected warnings on the author's real CAREER_01 (the converted save loads
  in-game with them present): chunk 0xb67f6cc6 size 0x2b4 != map ref 0x104
  (auto mode) and chunk 0xd548266c with 13 unaligned string runs padded.

### Console container header (anonymizing a 360 save)
- The profile id at 0x371 is the offline XUID and the device id at 0x3FD is
  the USB serial; both are identity values (blanked by anonymize_alias.py).
  The OnlineUserProfile chunk carries no name strings or XUIDs.

### Game behaviour observed on the PC
- The game looks saves up by their STFS container name, not the file name
  (so the converter writes <container name>/<container name>).
- The game allows 3 careers per alias: a CAREER_04 raises a warning on
  every screen; keep diagnostic saves out of the SAVE folder.
- Creating a new alias in the game WIPES the other files in the save
  folder (old alias and CAREER_02/03 gone, CAREER_01 overwritten by the new
  alias's career).
- Stray ALIAS_Player / CAREER_<non-ASCII> files usually mean the game fell
  back to the default profile; remove them before the next test. Caveat: a
  CAREER_<0xAA> was also written once under a pure native profile (likely a
  quick race day without a career), so it alone is not proof of a converter
  fault.
- The console refuses to copy another profile's save while signed in as a
  different profile, so cross-profile mixing of saves is not a case.
- The PC tree head and post region are not 0xAA allocator fill like native
  files (an observation from the retired session log, undated and not
  re-verified here); the loader ignores them and the converter does not
  pursue it.
- Method note for any future automatic type inference over saves: the
  deleted docs/re/typemap.py scored pooled value distributions, which is
  NOT reliable for small-vocabulary u16 data (git 338afa9^ has the tool).
