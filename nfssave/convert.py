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

import struct
from dataclasses import dataclass, field
from pathlib import Path

from .container360 import read_container
from .mc02 import MC02, Endian
from .payload_rules import convert_record, convert_payload_auto, RULES_LOADED
from .tree import Tree, TREE_MAGIC, REC_START_PC

PC_SAVE_ROOT = r"E:\legoj\Documents\Need for Speed ProStreet\SAVE\NFS ProStreet"
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
    # car table: last word of every entry is [u8 a][u8 b][u8 slot][pad] -
    # garage slot/index bytes; a u32 swap scrambles which record a car uses
    t0, size, n = CARDB_TABLE
    for k in range(n):
        o = t0 + k * size + CARDB_TABLE_SLOT + 4
        out[o:o + 4] = src[o:o + 4]


def apply_struct_fixes(rec, src: bytes) -> None:
    out = bytearray(rec.payload)
    if rec.id == CARDB_ID:
        fix_cardb_parts(src, out)
    elif rec.id not in RAW_BLOB_IDS:
        fix_node_flags(src, out)
    rec.payload = bytes(out)


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
        mode = convert_record(report.kind, rec, report.warnings)
        apply_struct_fixes(rec, src)
        if mode == "auto" and len(rec.payload) > 0x1000:
            report.warnings.append(
                f"chunk {CHUNK_NAMES.get(rec.id, hex(rec.id))} ({len(rec.payload):#x} B) "
                "converted in auto mode (no fieldmap)")
        _to_pc_record(rec)
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


def write_pc_save(mc02_pc: MC02, name: str, save_root: str = PC_SAVE_ROOT) -> Path:
    if not name or any(c in name for c in "\\/:") or name in (".", ".."):
        raise ValueError(f"unsafe save name {name!r}")
    root = Path(save_root)
    folder = root / name
    folder.mkdir(parents=True, exist_ok=True)
    target = folder / name
    target.write_bytes(mc02_pc.to_bytes())
    return target
