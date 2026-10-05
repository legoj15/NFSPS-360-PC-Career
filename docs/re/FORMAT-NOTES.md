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
  (group after every 170 data blocks), so payloads > 0xA9000 B are NOT one
  slice. See nfssave/container360.py. The earlier "not STFS" claim and the
  entry-field guesses below (hash/checksum) were wrong: +0x29 block count,
  +0x2F start block (LE24), +0x38/+0x3C timestamps.
- 0x0000 `CON ` + console serial/date ASCII + console cert (~0x1AC);
  0x01AC..0xC000 per-file metadata, PNG icons; 0xC000 entry table:
  filename NUL-padded, +0x28 hash, +0x2C block count, +0x30 flags
  `00 00 ff ff`, +0x34 BE u32 payload size, +0x38/+0x3C BE checksum ×2
  (algorithm != EA CRC, not needed); 0xD000 raw MC02 payload.
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
         PC VERIFIES it on load at 0x5AABD0 — recomputation required;
         algorithm being reversed, see tree_hash.py)
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
  CORRECTION of an earlier note: the console CAREER files' copies of these
  two tail records are DAMAGED (uninitialized noise written after the tree
  CRC was computed — proven by the recomp re-save twin at E:/GitHub/
  NFSPS360/user_data/B13EBABEBABEBABE/..., which stores valid records 8-9
  of the same session and parses 9/9 with all CRCs OK). The converter
  recovers those records from the twin when available (CAREER_01 only —
  no twins exist for CAREER_02/03, which ship without them; the game
  fills defaults). PCControllerSettings (alias 0x39156567) remains
  PC-only (absent from both 360 files AND the 360 twin).
- The first u32 of each record header ("type") is UNINITIALIZED HEAP on
  both platforms (0x01310001 etc. are stale bytes; PC garbage includes
  0x10101001/0x3F39E000). Value-preserving swap-through is safe; the
  loaders do not gate on it (PC sample has 0xBF2383CA on a top-level
  record).
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
- Container -> MC02 parse -> tree parse -> per-chunk payload conversion:
  fieldmap rules (fieldmaps, positionally valid for these exact
  source files; 156,454/156,454 verifiable slots byte-exact vs PC reference)
  or auto mode (u32 value-preserving swap + string/subword natural ranges).
- PC tree assembly: zeroed head struct (loader never reads it), records,
  post region (clamped to buffer), count = emitted top-level records.
- CRCs recomputed. Remaining blocker: 16-byte tree hash (fn 0x6D9CE0) —
  being reversed; outputs currently carry the 360's hash and WILL FAIL the
  PC's load-time verification if enforced.

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
  Extracted/Alias/ALIAS_JOSHUA S 10 (fully CRC-valid)
- PC reference: "100% Gamesave (OPTIONAL)" folder (CAREER_01 +
  ALIAS_PEIROKUNMANWSP); PC save root: E:\legoj\Documents\Need for Speed
  ProStreet\SAVE\NFS ProStreet (empty; game accepts third-party saves)

## Open items
1. 16-byte tree hash algorithm (fn 0x6D9CE0) + reject-vs-log on load
   (agent task; tree_hash.py when done).
2. 360-side loader confirmations (agent A pending).
3. Absent-chunk tolerance at PC load (PC-only chunks missing) — agent B.
4. djb2 names for the four consecutive alias chunks 0x8B7D0AAD..B0
   (runtime-generated: same base name + consecutive suffix char; controller
   configs inferred; cosmetic only).

## 2026-10-04 late: tail "damage" was a reader bug
Everything below about damaged console tails / re-save twins is SUPERSEDED:
the noise was STFS hash-table blocks read as payload. All careers parse
9/9 with valid CRCs once the block map is honoured. Further findings
(node flag words, u16 car part slots, open struct questions): ../HANDOFF.md.

## 2026-10-04 final framing correction + IN-GAME VERIFICATION (convert.py rewrite)

The empirical-grammar note above (both platforms `[type][id][size]`) was
wrong about the 360->PC emission. Actual layouts, proven by diffing a
native PC file against the 360 twins and by in-game behavior:

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
  size-0 dummy record at index 9 (between AudioSettings 0x9CB326C2 and
  PlayerSettings0 0x8B7D0AAD) to keep the pairing aligned.
- Conversion rule that fixed everything (nfssave/convert.py
  _to_pc_record): type=0x00000001, payload = 360payload[4:] + 4 zero
  bytes (drop the 360 leading marker word, re-add the PC trailing junk).
- Tree hash (16B at tree[0:0x10]): chained-MD5 x4 + RSA pow(M,E,N),
  E/N tables at VA 0x98CF88/0x98CF48 — computed on write, NEVER verified
  on load (0x5AABD0 is dead code). Implemented in nfssave/treehash.py.
- 360 career tail damage: last 2 records (CustomRaceDayMemcard
  0xD548266C, UnlockSystem 0xCA269650) overwritten with noise AFTER the
  tree CRC was computed (console-side bug; CRCs are stale too).
  Recovered for CAREER_01 from the NFSPS360 recomp twin
  (user_data/B13EBABEBABEBABE/45410822/00000001/CAREER_01): records 8-9
  valid, all CRCs OK, same session. CAREER_02/03 have no twin; they
  convert without those 2 records (loader fills defaults for absent
  trailing chunks).
- Record ids are djb2(name) h=0xFFFFFFFF,h=h*33+c (e.g. 0x59F2D89B
  MEMCARD_ROOT, 0x3B309E09 career root). See CHUNK_NAMES in convert.py.

VERIFIED IN-GAME 2026-10-04: converted JOSHUA S 10 alias + CAREER_01
load on PC — CAREER HUB day 7, $1,345,600, 4 repair markers, correct
race-day menu. Game exit save wrote no file changes (no dirty state), so
no native re-save oracle was produced; fresh-native references archived
at oracle/ instead.
