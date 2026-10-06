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
CARDB_ID = 0x47A07113
CARDB_RECORDS = (0x2680, 0x1870, 80)     # PC payload offset, stride, count
CARDB_PART_SLOTS = (0x3C, 0x186)         # u16 installed-part arrays per car
CARDB_PART_FLAGS = (0x186, 0x190)        # u8 fields right after the array
CARDB_BLUEPRINT_SETS = (0x0, 0x7B4, 0xF68)  # 3 customization sets per car
CARDB_TABLE = (0x14, 24, 410)            # car table: offset, entry size, count
CARDB_TABLE_SLOT = 20                    # entry word [u8][u8][u8][pad]


def fix_node_flags(src: bytes, out: bytearray) -> None:
    """Keep property-node flag words in natural byte order.

    Node stream (both platforms): [flag u8 + 3 uninit][data][u32 0][u32 len]
    ... The flag word follows each [0][len] pair; its first byte is the
    flag, the other three are heap junk. A blanket u32 swap moves the flag
    byte to the end of the word.
    """
    for o in range(8, len(src) - 3, 4):
        zero, ln = struct.unpack_from(">II", src, o - 8)
        if zero == 0 and 1 <= ln <= 0x400:
            out[o:o + 4] = src[o:o + 4]


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


def apply_struct_fixes(rec, src: bytes, warnings: list) -> None:
    out = bytearray(rec.payload)
    if rec.id == GAMEPLAY_ID:
        fix_raceday_block(src, out, warnings)
    elif rec.id == CARDB_ID:
        fix_cardb_parts(src, out)
        fix_cardb_packed(src, out)
    elif rec.id not in RAW_BLOB_IDS:
        fix_node_flags(src, out)
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


def _to_pc_record(rec) -> None:
    """Re-frame a converted record for PC-native emission.

    360 records: [junk word][id][size][payload = marker word + content].
    PC records:  [id][size][flags=1][payload = content + 4 junk bytes] with
    the same total size (native saves keep size == 360 size; their payload
    carries a trailing junk word). The 360 marker word maps onto the PC
    flags slot; we emit the native flags pattern 0x00000001 instead.
    """
    rec.type = 0x00000001
    if rec.payload:
        rec.payload = rec.payload[4:] + b"\x00\x00\x00\x00"


def validate_twin(src: Tree, twin: Tree, warnings: list) -> None:
    """Reject a re-save twin that is not the same career session.

    Verified on the real pair: the correct twin shares the source's record
    id sequence and every overlapping record's payload size (payload bytes
    themselves differ - volatile junk words/timestamps). A different day's
    save diverges in sizes and/or sequence.
    """
    if twin.gap:
        raise ValueError(
            f"twin file has a damaged record tail ({twin.gap:#x} B) - it cannot "
            "be used for recovery")
    src_ids = [(r.id, len(r.payload)) for r in src.records]
    twin_ids = [(r.id, len(r.payload)) for r in twin.records[:len(src.records)]]
    if src_ids != twin_ids:
        raise ValueError(
            "twin does not match the source career (record ids/sizes differ) - "
            "it is a different session; pass the correct --twin or drop it")


def convert_tree(tree360: Tree, report: ConversionReport, twin: Tree | None = None) -> Tree:
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
    for rec in tree360.records:
        normalize_gameplay(rec, report.warnings)
        src = rec.payload
        if rec.id == GAMEPLAY_ID:
            # variable layout (race-day block) - positional maps do not apply;
            # fields are u32/float except the fixes in fix_raceday_block
            rec.payload = swap_u32s(rec.payload)
            mode = "gameplay"
        else:
            mode = convert_record(report.kind, rec, report.warnings)
        apply_struct_fixes(rec, src, report.warnings)
        if mode == "auto" and len(rec.payload) > 0x1000 and rec.id != GAMEPLAY_ID:
            report.warnings.append(
                f"chunk {CHUNK_NAMES.get(rec.id, hex(rec.id))} ({len(rec.payload):#x} B) "
                "converted in auto mode (no fieldmap)")
        _to_pc_record(rec)
        rehash_gameplay(rec)
        pc.records.append(rec)
    if twin is not None:
        # console tail damaged: rebuild the sequence in the twin's order,
        # substituting the console records wherever the ids match, so the
        # positional pairing keeps the loader's registration order
        for trec in twin.records:
            normalize_gameplay(trec, report.warnings)
        validate_twin(tree360, twin, report.warnings)
        by_id = {r.id: r for r in pc.records}
        merged = []
        for trec in twin.records:
            if trec.id in by_id:
                merged.append(by_id.pop(trec.id))
            else:
                convert_record(report.kind, trec, report.warnings)
                _to_pc_record(trec)
                merged.append(trec)
                report.warnings.append(
                    f"record {CHUNK_NAMES.get(trec.id, hex(trec.id))} recovered from "
                    "re-save twin (console copy damaged)")
        if by_id:
            left = ", ".join(hex(i) for i in by_id)
            report.warnings.append(f"records absent from twin kept at end: {left}")
            merged.extend(by_id.values())
        pc.records = merged
    elif tree360.gap and len(pc.records) < tree360.count:
        report.warnings.append(
            "console record region damaged with no twin available - missing chunks "
            "convert as absent and the game fills defaults")
    pc.count = len(pc.records)
    # PC tree head: [allocator garbage (never read)][root record: id/used/flags=1]
    # followed by records at tree+0x1CC in native [id][size][flags][payload] framing
    pc.pre_records = (b"\0" * PC_HEAD_STRUCT_SIZE
                      + struct.pack("<III", TREE_MAGIC, 0, 0x00000001))
    # positional pairing: the PC loader walks savable[i] against record[i];
    # a chunk the 360 never writes must hold its slot with a size-0 filler
    # or every later pairing desyncs (PCControllerSettings sits between
    # AudioSettings and PlayerSettings0 in the PC registration order)
    if report.kind == "alias" and not any(r.id == 0x39156567 for r in pc.records):
        ps0 = next((k for k, r in enumerate(pc.records) if r.id == 0x8B7D0AAD),
                   len(pc.records))
        from .tree import Record
        pc.records.insert(ps0, Record(0x00000001, 0x39156567, 0, b""))
        pc.count = len(pc.records)
    report.chunk_list = [CHUNK_NAMES.get(r.id, hex(r.id)) for r in pc.records]
    return pc


def convert_payload(mc02_be: MC02, report: ConversionReport | None = None,
                    twin_payload: bytes | None = None) -> MC02:
    from .treehash import tree_hash

    report = report or ConversionReport()
    tree360 = Tree.parse(mc02_be.tree, big=True)
    report.kind = "alias" if len(mc02_be.extra) == 64 else "career"
    twin = None
    if twin_payload and report.kind == "career" and tree360.gap:
        twin = Tree.parse(MC02.parse(twin_payload).tree, big=True)
    pc_tree = convert_tree(tree360, report, twin)
    report.records = len(pc_tree.records)
    tree_bytes = pc_tree.build(big=False, tree_size=mc02_be.tree_size)
    used = sum(12 + len(r.payload) for r in pc_tree.records)
    # native PC saves carry the built tree's used size in the extra blob
    # (word 1); copy it through so the loader sees a consistent pair
    extra = bytearray(convert_extra(mc02_be.extra))
    if len(extra) == 28:
        struct.pack_into("<I", extra, 4, used)
    tree_bytes = bytearray(tree_bytes)
    tree_bytes[0:16] = tree_hash(bytes(tree_bytes))
    pc = MC02(Endian.LITTLE, extra=extra, tree=bytes(tree_bytes), tree_size=mc02_be.tree_size)
    return pc


def write_pc_save(mc02_pc: MC02, name: str, save_root: str) -> Path:
    if not name or any(c in name for c in "\\/:") or name in (".", ".."):
        raise ValueError(f"unsafe save name {name!r}")
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


def back_up_existing(save_root, name: str, stamp: str) -> Path | None:
    """Copy <save_root>/<name>/<name> to
    <parent of save_root>/SaveConverter backups/<stamp>[-N]/<name>/<name>.
    Returns the backup path, or None when there was nothing to keep."""
    existing = Path(save_root) / name / name
    if not existing.is_file():
        return None
    base = Path(save_root).resolve().parent
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
