"""360 -> PC save conversion for NFS ProStreet.

Pipeline:
  1. Read the 360 CON container, extract the big-endian MC02 payload.
  2. Parse the chunk tree; normalize platform buffer differences (the 360
     GameplayData chunk carries a 0x4000 zero tail the PC buffer omits);
     convert every record payload BE->LE (fieldmap rules or auto
     string-detect + u32 swap).
  3. Re-encode as a PC tree: same records; PC head region is allocator
     garbage that the loader never reads (zeros + root record).
  4. Rebuild the little-endian MC02 with fresh CRCs, patch the tree-hash
     placeholder and the used-tree-size word in the extra blob, and write
     to the PC save layout: <root>/<NAME>/<NAME>.

Chunk ids are djb2(name) hashes (h=-1; h=h*33+c), identical across
platforms — see CHUNK_NAMES.
"""

import hashlib
import shutil
import struct
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path

from .container360 import read_container
from .mc02 import MC02, Endian
from .payload_rules import convert_record, convert_payload_auto, find_string_runs, RULES_LOADED
from .tree import Tree, TREE_MAGIC, REC_START_PC

PC_HEAD_STRUCT_SIZE = 0x1AC  # allocator-garbage region PC keeps between count and root

GAMEPLAY_ID = 0x3B309E09
GAMEPLAY_PC_SIZE = 0x10014       # PC buffer for the gameplay chunk
GAMEPLAY_360_SIZE = 0x10014 + 0x4000  # 360 buffer: same content + 0x4000 zero pad

CHUNK_NAMES = {
    0x59F2D89B: "MEMCARD_ROOT",
    0x8FFBE3E8: "GStatsImpl::SavableStats",
    0x4E8AA143: "AchievementManager",
    0x9F72F194: "OnlineUserProfile",
    0xDC6B027F: "ProfileStats",
    0x6CC89C57: "Jukebox",
    0xC3EC4947: "VideoSettings",
    0xB74C1044: "ForceFeedbackSettings",
    0x8C4A2DC0: "GameplaySettings",
    0x9CB326C2: "AudioSettings",
    0x8B7D0AAD: "PlayerSettings0",
    0x8B7D0AAE: "PlayerSettings1",
    0x8B7D0AAF: "PlayerSettings2",
    0x8B7D0AB0: "PlayerSettings3",
    0x322ED42F: "UserProfile",
    0x328C6431: "SPEECH DATA",
    0xB67F6CC6: "MarkerSystem",
    0x3B309E09: "GameplayData",
    0x1FB48CF2: "GameplayBinarySavableHelper",
    0x51A41B14: "RaceData",
    0x47A07113: "FEPlayerCarDB",
    0x34B74942: "VehicleBinarySavableHelper",
    0x885B4DDC: "FECareer",
    0x39156567: "PCControllerSettings",      # PC-only
    0xD548266C: "CustomRaceDayMemcard",      # PC-only
    0xCA269650: "UnlockSystem",              # PC-only
}


@dataclass
class ConversionReport:
    source: str = ""
    kind: str = "career"
    records: int = 0
    chunk_list: list = field(default_factory=list)
    warnings: list = field(default_factory=list)

    def __str__(self) -> str:
        lines = [f"{self.kind}: {self.records} chunks -> {', '.join(self.chunk_list)}"]
        for w in self.warnings:
            lines.append(f"  ! {w}")
        return "\n".join(lines)


def swap_u32s(data: bytes) -> bytes:
    assert len(data) % 4 == 0
    return b"".join(data[i:i + 4][::-1] for i in range(0, len(data), 4))


def normalize_gameplay(rec, warnings: list) -> None:
    """Trim the 360-only 0x4000 zero tail from GameplayData (in place).

    The console allocates a 16 KB larger buffer for the gameplay chunk than
    the PC build; the extra region is always zero padding (verified on a
    fresh and a day-7 career: real content ends before 0xA1C in both). The
    nested-helper size word at payload offset 0x0C is rewritten to match
    the trimmed length, in big-endian (the payload-wide swap happens later).
    """
    p = rec.payload
    if rec.id != GAMEPLAY_ID or len(p) <= GAMEPLAY_PC_SIZE:
        return
    tail = p[GAMEPLAY_PC_SIZE:]
    if any(tail):
        warnings.append(
            f"GameplayData content ({len(p):#x} B) exceeds the PC buffer "
            f"({GAMEPLAY_PC_SIZE:#x} B); converting untrimmed - the game may "
            "reject the chunk")
        return
    rec.payload = bytearray(p[:GAMEPLAY_PC_SIZE])
    struct.pack_into(">I", rec.payload, 0x0C, GAMEPLAY_PC_SIZE - 0x10)
    rec.payload = bytes(rec.payload)


def convert_extra(extra_be: bytes) -> bytes:
    """MC02 'extra' preamble, 360 -> PC.

    career (28 B): all numeric -> swap.
    alias (64 B): [id][used][1][day][0] swap; NUL-terminated player name
    natural (any byte values - gamertags may be non-ASCII); 0xAA pad words
    natural; tail floats/hash swap.
    """
    n = len(extra_be)
    if n == 28:
        return swap_u32s(extra_be)
    if n != 64:
        raise ValueError(f"unexpected extra size {n}")
    out = bytearray(extra_be)
    for i in range(0, 0x14, 4):
        out[i:i + 4] = extra_be[i:i + 4][::-1]
    # name: everything up to the first NUL is the name, kept natural
    nul = extra_be.find(b"\x00", 0x14, n)
    if nul != -1:
        end = nul + 1
    else:
        # no terminator: printable run then 0x00/0xAA pad (defensive)
        end = 0x14
        while end < n and extra_be[end] not in (0x00, 0xAA) and 0x20 <= extra_be[end] < 0x7F:
            end += 1
        if end < n and extra_be[end] == 0x00:
            end += 1
    while end + 4 <= n and extra_be[end:end + 4] == b"\xaa\xaa\xaa\xaa":
        end += 4
    # align to 4 and swap the numeric tail
    end = (end + 3) & ~3
    for i in range(end, n, 4):
        out[i:i + 4] = extra_be[i:i + 4][::-1]
    return bytes(out)


RAW_BLOB_IDS = (GAMEPLAY_ID, 0x47A07113)  # memcpy structs, not property nodes
# node streams of u32/float values only (no strings): swap every word, then
# fix_node_flags. RaceData (race results: track keys, times) had a fieldmap
# from fresh careers whose empty slots fell back to the string heuristic,
# leaving times like 0x42724630 (60.57 s, "BrF0") big-endian - the PC race
# HUD then lost its speedometer/leaderboard and the camera reset (in-game).
RACEDATA_ID = 0x51A41B14
NUMERIC_IDS = (RACEDATA_ID,)
CARDB_ID = 0x47A07113
CARDB_RECORDS = (0x2680, 0x1870, 80)     # PC payload offset, stride, count
CARDB_PART_SLOTS = (0x3C, 0x186)         # u16 installed-part arrays per car
CARDB_PART_FLAGS = (0x186, 0x190)        # u8 fields right after the array
CARDB_BLUEPRINT_SETS = (0x0, 0x7B4, 0xF68)  # 3 customization sets per car
CARDB_TABLE = (0x14, 24, 410)            # car table: offset, entry size, count
CARDB_TABLE_SLOT = 20                    # entry word [u8][u8][u8][pad]


def is_node_flag(word: bytes) -> bool:
    """360 node flag word: [u8 flag][FF FF FF] (or zeroed)."""
    return word[1:4] in (b"\xff\xff\xff", b"\0\0\0")


def fix_node_flags(src: bytes, out: bytearray) -> None:
    """Keep property-node flag words in natural byte order.

    Node stream (both platforms): [flag u8 + 3 uninit][data][u32 0][u32 len]
    ... The flag word follows each [0][len] pair; its first byte is the
    flag, the other three are heap junk. A blanket u32 swap moves the flag
    byte to the end of the word.

    The [0][len] header itself must always swap. The string heuristic can
    swallow it: a NUL-padded name node runs into the next header and the
    word grid rounds it in, leaving len big-endian (0x04000000 on PC). The
    PC then drops that node and everything after it - custom race days
    lost their event lists and the Race Day menu crashed (nfs.exe 0x7F6480).
    """
    # data offsets of the nodes on the property chain (node_spans walk):
    # their [0][len] headers are real, unlike matches inside numeric data
    chain = {d for d, ln in node_spans(src) if ln == 1}
    for o in range(8, len(src) - 3, 4):
        zero, ln = struct.unpack_from(">II", src, o - 8)
        if o - 4 in chain:
            continue      # [flag 0][u8 node data 00 00 00 08] is not a header
        if zero == 0 and 1 <= ln <= 0x400:
            out[o - 4:o] = src[o - 4:o][::-1]
            out[o:o + 4] = src[o:o + 4]
            # one-byte node: the value is the first data byte on both
            # platforms ([u8][3 pad]); a u32 swap reads back as 0 on PC
            # (every alias on/off option). The 360 pad (and flag) bytes can
            # hold heap junk (01 00 13 10), so a node on the chain keeps its
            # first byte whatever the pad. Off the chain a [0][1] match may be
            # numeric data ([0][1][flag][00 00 00 04] in SPEECH DATA): there a
            # nonzero pad, or a flag word that is not [u8][FF FF FF | 00 00 00],
            # = not a u8 node.
            if ln == 1 and o + 4 in chain and o + 8 <= len(src):
                out[o + 4:o + 8] = src[o + 4:o + 8]
            elif ln == 1 and src[o + 5:o + 8] == b"\0\0\0" and is_node_flag(src[o:o + 4]):
                out[o + 4:o + 8] = src[o + 4:o + 8]


CUSTOM_RACEDAY_ID = 0xD548266C


def node_spans(src: bytes, start: int = 4) -> list[tuple[int, int]]:
    """(offset, length) of each node's data in a 360 property-node stream.

    The first value sits at `start` (after the 360 marker word); every later
    node is [u32 0][u32 len][flag word][data], its header on the u32 grid
    after the previous data (junk bytes may sit in between).
    """
    spans = [(start, 4)]
    o = start + 4
    while o + 12 <= len(src):
        h = (o + 3) & ~3
        while h + 8 <= len(src):
            zero, ln = struct.unpack_from(">II", src, h)
            if zero == 0 and 0 < ln <= 0x400:
                break
            h += 4
        else:
            break
        d = h + 12
        if d + ln > len(src):
            break
        spans.append((d, ln))
        o = d + ln
    return spans


def fix_custom_raceday_strings(src: bytes, out: bytearray) -> None:
    """CustomRaceDayMemcard nodes are u32 values (swapped by the generic
    pass) or strings: GUID (25 B) and name (36 B), each [4 junk][chars, NUL].
    String nodes copy byte for byte; the generic string heuristic misses
    GUIDs followed by a junk byte and half-swapped them."""
    for d, ln in node_spans(src):
        if ln > 4:
            out[d:d + ln] = src[d:d + ln]


FECAREER_ID = 0x885B4DDC
CAREER_NAME_LEN = 0x24


def fix_career_name(src: bytes, out: bytearray) -> None:
    """FECareer's one 36-byte node is the career-slot name: [4 junk][32
    chars, NUL]. The PC names the save file after it (CAREER_<name>). The
    360 pads "01\\0" with 0xAA heap fill, which defeats the string heuristic;
    the u32 swap then saved every converted career as CAREER_<0xAA>. Copy
    the chars naturally and zero after the NUL, like a native PC career."""
    for d, ln in node_spans(src):
        if ln == CAREER_NAME_LEN:
            out[d:d + 4] = src[d:d + 4]
            chars = src[d + 4:d + ln].split(b"\0", 1)[0]
            out[d + 4:d + ln] = chars + bytes(ln - 4 - len(chars))


CARDB_PACKED = (0x7C980, 0x90660)   # region holding 8-byte packed entries
PACKED_FILL = bytes((0x2A, 0xAA))     # 360 uninitialized 14/16-bit field (0xAAAA masked)


def convert_packed_entry(e: bytes) -> bytes:
    """One 8-byte packed entry, 360 BE -> PC LE.

    Layout (both platforms, as u32 pairs): w1 = [u16 index | 14-bit link
    << 16 | 2 bits], w2 = [u16 low | flag bit << 16 | value << 17].
    The PC writes 'none' into the link (0x3FFF) and low (0xFFFF) fields and
    keeps the flag bit clear; the 360 leaves link/low as heap fill (0x2AAA)
    and the flag bit set on some entries. A walked link of 0x2AAA points
    past the ~9400-entry table (garage hang). Verified on the matched pairs:
    2aaafffe ffff2aaa -> feffff3f fffffeff; value 0x5C85 -> 0x5C84.
    """
    w1, w2 = struct.unpack(">II", e)
    w1 = (w1 & 0xC000FFFF) | (0x3FFF << 16)
    w2 = (w2 & 0xFFFE0000) | 0xFFFF
    return struct.pack("<II", w1, w2)


def fix_cardb_packed(src: bytes, out: bytearray) -> None:
    lo, hi = CARDB_PACKED
    for o in range(lo, hi, 8):
        e = src[o + 4:o + 12]
        if e[:2] == PACKED_FILL and e[6:8] == PACKED_FILL:
            out[o + 4:o + 12] = convert_packed_entry(e)


# per blueprint set (offsets relative to the set; verified vs native PC saves)
BP_PAINT = (0x194, 12, 12)       # 12 x [paint: u16,u16][f32][f32]
BP_DECALS = (0x240, 26, 20)      # 20 x 26 B decal slots
BP_VINYLS = (0x450, 14, 20)      # 20 x 14 B vinyl slots
BP_COLOURS = (0x574, 0x628)      # u8[20][9] vinyl colour bytes, natural


def _swap16s(b: bytes) -> bytes:
    return b"".join(b[i:i + 2][::-1] for i in range(0, len(b), 2))


def convert_decal_entry(e: bytes) -> bytes:
    """Decal/vinyl entry: [s16 x][s16 y][u16][u8 x4][u16 id][u16]...(u16s).
    All u16 fields swap per u16; the four bytes at +6..+9 stay natural
    (native PC 'bf 12 12 00' <-> 360 'c0 1b 1b 00' pattern)."""
    return _swap16s(e[:6]) + e[6:10] + _swap16s(e[10:])


def fix_blueprint_set(src: bytes, out: bytearray, s: int) -> None:
    """One customization set at 360-payload offset s (PC offset + 4).

    Decal/vinyl entries are 26/14 bytes - not u32 aligned - so a blanket
    u32 swap tears fields across entries (garage hang on customized cars)."""
    off, step, n = BP_PAINT
    for k in range(n):
        o = s + off + k * step
        out[o:o + 4] = _swap16s(src[o:o + 4])
    for off, step, n in (BP_DECALS, BP_VINYLS):
        for k in range(n):
            o = s + off + k * step
            out[o:o + step] = convert_decal_entry(src[o:o + step])
    lo, hi = BP_COLOURS
    out[s + lo:s + hi] = src[s + lo:s + hi]


def fix_cardb_parts(src: bytes, out: bytearray) -> None:
    """Customization part slots are u16 arrays: swap each u16 in place.

    A u32 swap exchanges neighbouring slots, so every installed part lands
    in the wrong slot and the game falls back to a stock car (verified
    against the matched-state PC save: starter car slots byte-exact).
    The u8 fields after the array stay natural. Offsets are PC payload
    offsets; the 360 payload (still unframed here) has one extra leading
    word.
    """
    base, stride, count = CARDB_RECORDS
    lo, hi = CARDB_PART_SLOTS
    flo, fhi = CARDB_PART_FLAGS
    for r in range(count):
        rec = base + r * stride + 4
        out[rec:rec + 4] = src[rec:rec + 4]                  # u8 x3 + pad
        out[rec + 4:rec + 8] = src[rec + 4:rec + 6][::-1] + src[rec + 6:rec + 8][::-1]
        for bp in CARDB_BLUEPRINT_SETS:
            rec0 = rec + bp
            for s in range(rec0 + lo, rec0 + hi, 2):
                out[s:s + 2] = src[s:s + 2][::-1]
            out[rec0 + flo:rec0 + fhi] = src[rec0 + flo:rec0 + fhi]
            fix_blueprint_set(src, out, rec0)
    # car table: last word of every entry is [u8 a][u8 b][u8 slot][pad] -
    # garage slot/index bytes; a u32 swap scrambles which record a car uses
    t0, size, n = CARDB_TABLE
    for k in range(n):
        o = t0 + k * size + CARDB_TABLE_SLOT + 4
        out[o:o + 4] = src[o:o + 4]


GAMEPLAY_U8_FIELDS = (0x1F4, 0x2D0)  # [u8][pad x3] words, natural order
RACEDAY_STATE = 0x2D4       # GameplayData: race-day state (0 = none active)
RACEDAY_START = 0x2E0       # variable-length race-day block starts here
RACEDAY_PAD = 0x314         # 360-only 4-byte pad inside the race-day block
RACEDAY_NAME = (0x300, 0x310)          # car/name C-string, natural order
POST_BLOCK_COUNT = 0x11     # the list after the block starts [u32 0][u32 0x11]


def _u32be(src: bytes, o: int) -> int:
    return struct.unpack_from(">I", src, o + 4)[0]   # PC offset -> 360 payload


def raceday_block_end(src: bytes) -> int | None:
    """PC offset where the race-day block ends (= start of the list that
    follows it: [u32 0][u32 0x11][hash]...). Verified: 0x3E70 on the
    Battle Machine save, 0xB5B0 on the Willow Springs save (the same list
    sits at 0x2E0 when no race day is active)."""
    for o in range(RACEDAY_START + 0x40, len(src) - 16, 16):
        if _u32be(src, o) == 0 and _u32be(src, o + 4) == POST_BLOCK_COUNT                 and _u32be(src, o + 8) != 0:
            return o
    return None


def _is_record_kind(w: int) -> bool:
    """Race-day record header kind word, e.g. 0x00001310, 0x00051110,
    0x00081B10: low byte 0x10, next byte 0x11..0x1B (odd), top byte 0."""
    return (w & 0xFF) == 0x10 and ((w >> 8) & 0xFF) in (0x11, 0x13, 0x15, 0x17, 0x19, 0x1B)         and w >> 24 == 0


def fix_raceday_block(src: bytes, out: bytearray, warnings: list) -> None:
    """GameplayData in-progress race-day block (0x2E0..end, variable length).

    Verified on the mid-race-day pair (Battle Machine, Nevada):
      * the 360 block has a 4-byte pad at 0x314 the PC lacks: every later
        field moves back 4 bytes; the freed word lands at the block end;
      * record headers are [u32 kind][u16][u16] - the second word swaps
        per u16;
      * per-event words [u8 flags][u8 0][pad pad] (0xAAAA fill) and the name
        string at 0x300 stay natural.
    PC offsets; the unframed 360 payload has one extra leading word.
    """
    for o in GAMEPLAY_U8_FIELDS:
        out[o + 4:o + 8] = src[o + 4:o + 8]
    active = _u32be(src, RACEDAY_STATE) != 0
    end = raceday_block_end(src) if active else RACEDAY_START
    # race-day name strings ('MV03_BattleMachine') live after the block;
    # string detection is restricted there because float pairs inside the
    # block can look printable
    if end is not None:
        for a, b in find_string_runs(src[end + 4:]):
            out[end + 4 + a:end + 4 + b] = src[end + 4 + a:end + 4 + b]
    if not active:
        return
    if end is None:
        warnings.append("GameplayData: active race day but block end not found - "
                        "race day will not resume")
        return
    lo, hi = RACEDAY_NAME
    out[lo + 4:hi + 4] = src[lo + 4:hi + 4]
    prev_kind = False
    for o in range(RACEDAY_PAD + 4, end, 4):        # 360 word positions (PC coords)
        w = src[o + 4:o + 8]
        if prev_kind:
            out[o + 4:o + 8] = w[1::-1] + w[:1:-1]   # two u16s
        elif w[1] == 0 and w[2:] == b"\xaa\xaa":
            out[o + 4:o + 8] = w                     # [u8][u8][pad pad]
        prev_kind = _is_record_kind(int.from_bytes(w, "big"))
    a, e = RACEDAY_PAD + 4, end + 4
    out[a:e - 4] = out[a + 4:e]
    out[e - 4:e] = bytes(4)


# Race-day progress table in GameplayData: 90 x [u32 key][u32 state][u32 score],
# after the race-day block (position varies). Located by its first and last key.
PROGRESS_FIRST, PROGRESS_LAST, PROGRESS_LEN = 0xA70EA9B0, 0xFA5D360A, 90

# Race days the 360 marks with console-only state (bits 0x04/0x08/0x10 and a
# score, even in a fresh career with no custom race days) that the PC never
# writes: every PC save has them at state 0 or 2, score 0 - fresh, mid-career
# and 100%, and a PC save made after creating a custom race day left them
# untouched. Five (state 0 here: 8F7CCCE0 46AE8E2F C8A0888E 0A6C2097
# AF51A403) are the custom race-day SLOTS (career [0xAB9DC8]+0xB0); they are
# not in gameplay.bin. Values = native PC side of the matched pairs;
# 0x8DA1975B from the PC 100% save. (Not the Race Day crash cause - that was
# the CustomRaceDayMemcard node framing, see fix_node_flags.)
CONSOLE_ONLY_RACEDAYS = {
    0xB48C11C4: 2, 0x0A6C2097: 0, 0xF841FB9F: 2, 0x8DA1975B: 0,
    0x8F7CCCE0: 0, 0x92407122: 2, 0x5C838C1A: 0, 0xAF51A403: 0,
    0xDDCEF290: 2, 0x46AE8E2F: 0, 0xB3F02D70: 2, 0x8FEB3CC6: 2,
    0xC8A0888E: 0, 0x66705CF6: 2, 0x150B07D4: 2, 0xD663D2A8: 2,
    0x21471712: 2,
}


def progress_table_offset(p: bytes) -> int | None:
    """Offset of the race-day progress table in a little-endian payload."""
    first = struct.pack("<I", PROGRESS_FIRST)
    o = p.find(first)
    while o >= 0:
        last = o + 12 * (PROGRESS_LEN - 1)
        if last + 4 <= len(p) and struct.unpack_from("<I", p, last)[0] == PROGRESS_LAST:
            return o
        o = p.find(first, o + 1)
    return None


def fix_raceday_progress(out: bytearray, warnings: list) -> None:
    """Reset console-only race days to their PC-native state (see
    CONSOLE_ONLY_RACEDAYS). Operates on the already-swapped payload."""
    o = progress_table_offset(out)
    if o is None:
        warnings.append("GameplayData: race-day progress table not found - "
                        "the PC Race Day menu may crash")
        return
    for i in range(PROGRESS_LEN):
        e = o + 12 * i
        key = struct.unpack_from("<I", out, e)[0]
        if key in CONSOLE_ONLY_RACEDAYS:
            struct.pack_into("<II", out, e + 4, CONSOLE_ONLY_RACEDAYS[key], 0)


def apply_struct_fixes(rec, src: bytes, warnings: list) -> None:
    out = bytearray(rec.payload)
    if rec.id == GAMEPLAY_ID:
        fix_raceday_block(src, out, warnings)
        fix_raceday_progress(out, warnings)
    elif rec.id == CARDB_ID:
        fix_cardb_parts(src, out)
        fix_cardb_packed(src, out)
    elif rec.id not in RAW_BLOB_IDS:
        fix_node_flags(src, out)
        if rec.id == CUSTOM_RACEDAY_ID:
            fix_custom_raceday_strings(src, out)
        elif rec.id == FECAREER_ID:
            fix_career_name(src, out)
    rec.payload = bytes(out)


GAMEPLAY_BLOB = (0x14, 0x10000)  # PC payload offset/size of the gameplay blob


def rehash_gameplay(rec) -> None:
    """GameplayData blob = [MD5(rest)][rest]; recompute after conversion.

    The PC deserializer (nfs.exe 0x59E550 -> [0xAB9D88] vtbl+0x70) rejects
    a blob whose MD5 does not match and the career starts from scratch
    (intro movie). Verified on native PC saves: blob[0:16] ==
    md5(blob[16:0x10000]). Operates on the PC-framed payload.
    """
    if rec.id != GAMEPLAY_ID:
        return
    off, size = GAMEPLAY_BLOB
    p = bytearray(rec.payload)
    p[off:off + 16] = hashlib.md5(bytes(p[off + 16:off + size])).digest()
    rec.payload = bytes(p)


def tail_word(rec_id: int, src: bytes, spill: bytes) -> bytes:
    """PC byte order for a record's final word, taken from the 360 word at
    the next record's header slot (`spill`, big-endian as stored).

    Verified: CAREER_01 CustomRaceDayMemcard spills 0x00000001 (its last
    event flag; the PC-written race day ends 01 00 00 00 too) and FECareer
    spills 0x2848 in every 360 sample. Node streams ending in a u32 node
    ([0][4][flag] before the spill) swap it; raw memcpy blobs swap like the
    rest of the blob; anything else (a string node running into the spill)
    stays natural. GameplayData keeps zero: its 360 buffer is trimmed, so
    the spilled word is 360-only padding. Other trailing scalar nodes (u8
    options, 8-byte nodes) convert as in scalar_tail.
    """
    if rec_id == GAMEPLAY_ID or len(spill) != 4:
        return bytes(4)
    if rec_id in RAW_BLOB_IDS or src[-12:-4] == bytes(4) + struct.pack(">I", 4):
        return spill[::-1]
    w = scalar_tail(src, spill)
    return w if any(w) else spill


def scalar_tail(src: bytes, tail: bytes) -> bytes:
    """PC last payload word when the payload ends in a scalar node, else
    zeros (tail_word then keeps the word natural).

    Both platforms store [flag word][nodes...][last data word]; the 360
    writes the last word after the record (tree.Record.tail / the spill).
    The word is that node's value: [0][len 1..4][flag] (u8 node -> natural,
    else u32 swap) or [0][len 5..8][flag][d1] (tail = d2, u32 swap). The
    personal alias's values match a fresh PC alias (AudioSettings 3,
    PlayerSettings0 2).
    """
    if len(tail) != 4:
        return bytes(4)
    for k, lo, hi in ((0, 1, 4), (4, 5, 8)):
        h = len(src) - 12 - k
        if h < 0:
            continue
        zero, ln = struct.unpack_from(">II", src, h)
        if zero == 0 and lo <= ln <= hi and is_node_flag(src[h + 8:h + 12]):
            if ln == 1 and tail[1:] == b"\0\0\0":
                return tail
            return tail[::-1]
    return bytes(4)


def _to_pc_record(rec, last: bytes = bytes(4)) -> None:
    """Re-frame a converted record for PC-native emission.

    360 records: [prev record's last word][id][size][payload = marker word
    + content]. PC records:  [id][size][flags=1][payload = content + last
    word] with the same total size. The 360 marker word maps onto the PC
    flags slot; we emit the native flags pattern 0x00000001 instead.
    `last` is the converted last word (tail_word; zeros otherwise).
    """
    rec.type = 0x00000001
    if rec.payload:
        rec.payload = rec.payload[4:] + last


# VideoSettings keeps the 360's two extra trailing 8-byte nodes (0.5, 1.0):
# the PC loads them fine (in-game 2026-10-09). Trimming to the native 0x74
# was tried and only ever appeared in failing runs, so it was dropped.

PC_CONTROLLER_ID = 0x39156567
# native PC default bindings (game-created default profile, keyboard: arrows,
# LCtrl, Space, ...); the 360 has no such chunk. A size-0 filler loaded, but
# the PC dropped the profile mid-session for a default 'Player' (in-game).
PC_CONTROLLER_DEFAULT = (Path(__file__).parent / "pc_controller_default.bin").read_bytes()


def record_spills(tree: Tree) -> list:
    """Each 360 record's final word: it sits in the next record's header
    slot; the last record's in the word after the record area. Noise breaks
    the chain: the record right before a damaged region has no spill (the next
    header is a re-anchored one, not its tail), and a trailing gap leaves the
    last record with none too. After a successful re-anchor everything,
    the last record included, is as in an undamaged tree."""
    spills = [struct.pack(">I", r.type) for r in tree.records[1:]]
    if tree.gap_at:
        spills[tree.gap_at - 1] = b""
    spills.append(tree.post[:4] if not tree.gap or tree.gap_at is not None else b"")
    return spills


def convert_to_pc_record(rec, spill: bytes, report: ConversionReport) -> None:
    """Convert one 360 record to its PC-framed form (in place). Used by
    the main loop."""
    normalize_gameplay(rec, report.warnings)
    src = rec.payload
    if rec.id == GAMEPLAY_ID:
        # variable layout (race-day block) - positional maps do not apply;
        # fields are u32/float except the fixes in fix_raceday_block
        rec.payload = swap_u32s(rec.payload)
        mode = "gameplay"
    elif rec.id in NUMERIC_IDS:
        rec.payload = swap_u32s(rec.payload)
        mode = "numeric"
    else:
        mode = convert_record(report.kind, rec, report.warnings)
    apply_struct_fixes(rec, src, report.warnings)
    if mode == "auto" and len(rec.payload) > 0x1000 and rec.id != GAMEPLAY_ID:
        report.warnings.append(
            f"chunk {CHUNK_NAMES.get(rec.id, hex(rec.id))} ({len(rec.payload):#x} B) "
            "converted in auto mode (no fieldmap)")
    _to_pc_record(rec, tail_word(rec.id, src, spill))
    rehash_gameplay(rec)


def convert_tree(tree360: Tree, report: ConversionReport) -> Tree:
    if tree360.gap:
        report.warnings.append(
            f"{tree360.gap:#x} bytes of damaged noise inside the console record "
            "region (known console writing bug)")
    if not RULES_LOADED:
        report.warnings.append("fieldmap rules unavailable - all chunks converted in auto mode")

    pc = Tree(
        noise=tree360.noise,
        count=0,
        pre_records=b"",
        records=[],
        post=convert_payload_auto(tree360.post) if tree360.post else b"",
        used=0,
    )
    for rec, spill in zip(tree360.records, record_spills(tree360)):
        convert_to_pc_record(rec, spill, report)
        pc.records.append(rec)
    if tree360.gap and len(pc.records) < tree360.count:
        report.warnings.append(
            "console record region damaged - missing chunks "
            "convert as absent and the game fills defaults")
    pc.count = len(pc.records)
    # PC tree head: [allocator garbage (never read)][root record: id/used/flags=1]
    # followed by records at tree+0x1CC in native [id][size][flags][payload] framing
    pc.pre_records = (b"\0" * PC_HEAD_STRUCT_SIZE
                      + struct.pack("<III", TREE_MAGIC, 0, 0x00000001))
    # positional pairing: the PC loader walks savable[i] against record[i];
    # a chunk the 360 never writes must hold its slot or every later pairing
    # desyncs (PCControllerSettings sits between AudioSettings and
    # PlayerSettings0 in the PC registration order). It gets the native
    # default bindings: an empty one made the PC drop the profile mid-session.
    if report.kind == "alias" and not any(r.id == PC_CONTROLLER_ID for r in pc.records):
        ps0 = next((k for k, r in enumerate(pc.records) if r.id == 0x8B7D0AAD),
                   len(pc.records))
        from .tree import Record
        pc.records.insert(ps0, Record(0x00000001, PC_CONTROLLER_ID,
                                      len(PC_CONTROLLER_DEFAULT), PC_CONTROLLER_DEFAULT))
        pc.count = len(pc.records)
    report.chunk_list = [CHUNK_NAMES.get(r.id, hex(r.id)) for r in pc.records]
    return pc


def convert_payload(mc02_be: MC02, report: ConversionReport | None = None) -> MC02:
    from .treehash import tree_hash

    report = report or ConversionReport()
    tree360 = Tree.parse(mc02_be.tree, big=True)
    report.kind = "alias" if len(mc02_be.extra) == 64 else "career"
    pc_tree = convert_tree(tree360, report)
    report.records = len(pc_tree.records)
    tree_bytes = pc_tree.build(big=False, tree_size=mc02_be.tree_size)
    used = sum(12 + len(r.payload) for r in pc_tree.records)
    # native PC saves carry the built tree's used size in the extra blob
    # (word 1), careers and aliases alike; copy it through so the loader sees
    # a consistent pair. Aliases need it most: the inserted
    # PCControllerSettings record (12 + 0x684 B) makes the 360 value stale,
    # and a stale value made the PC skip the alias for a default 'Player'.
    extra = bytearray(convert_extra(mc02_be.extra))
    struct.pack_into("<I", extra, 4, used)
    tree_bytes = bytearray(tree_bytes)
    tree_bytes[0:16] = tree_hash(bytes(tree_bytes))
    pc = MC02(Endian.LITTLE, extra=extra, tree=bytes(tree_bytes), tree_size=mc02_be.tree_size)
    return pc


def check_save_name(name: str) -> None:
    """Refuse a save name that cannot be a plain folder name. Shared by the
    real write and the dry run, so both report the same refusal."""
    # Windows drops trailing dots/spaces, so "..." or "  " would collapse onto
    # the output root itself.
    if (not name or any(c in name for c in "\\/:") or name in (".", "..")
            or not name.rstrip(". ")):
        raise ValueError(f"unsafe save name '{name}'")


def write_pc_save(mc02_pc: MC02, name: str, save_root: str) -> Path:
    check_save_name(name)
    root = Path(save_root)
    folder = root / name
    folder.mkdir(parents=True, exist_ok=True)
    target = folder / name
    target.write_bytes(mc02_pc.to_bytes())
    return target


# Folder (next to the export folder) that receives replaced saves; same
# convention as the Windows app (nfspc-converter app/batch.rs).
BACKUP_DIR = "SaveConverter backups"


def utc_stamp(t: datetime | None = None) -> str:
    """YYYY-MM-DD_HH-MM-SS (UTC) for backup folder names."""
    t = t or datetime.now(timezone.utc)
    return t.astimezone(timezone.utc).strftime("%Y-%m-%d_%H-%M-%S")


def back_up_existing(save_root, name: str, stamp: str, base=None) -> Path | None:
    """Copy <save_root>/<name>/<name> to
    <base>/SaveConverter backups/<stamp>[-N]/<name>/<name>; base defaults to
    the parent of save_root. Returns the backup path, or None when there was
    nothing to keep."""
    existing = Path(save_root) / name / name
    if not existing.is_file():
        return None
    if base is None:
        base = Path(save_root).absolute().parent  # not resolve(): batch.rs parity
    base = Path(base)
    # runs inside the same second share a stamp: never overwrite an earlier
    # backup, fall through to <stamp>-2, <stamp>-3, ...
    n = 1
    while True:
        dest = base / BACKUP_DIR / (stamp if n == 1 else f"{stamp}-{n}") / name / name
        if not dest.exists():
            break
        n += 1
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(existing, dest)
    return dest
