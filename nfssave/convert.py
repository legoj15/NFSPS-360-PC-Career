"""360 -> PC save conversion for NFS ProStreet.

Pipeline:
  1. Read the 360 CON container, extract the big-endian MC02 payload.
  2. Parse the chunk tree; convert every record payload BE->LE (fieldmap
     rules or auto string-detect + u32 swap).
  3. Re-encode as a PC tree: same records; PC head region is allocator
     garbage that the loader never reads (zeros + root record); the tree's
     16-byte hash placeholder is patched by finalize_tree() once the
     hash algorithm is available.
  4. Rebuild the little-endian MC02 with fresh CRCs and write to the PC
     save layout: <root>/<NAME>/<NAME>.

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

    def __str__(self):
        lines = [f"{self.kind}: {self.records} chunks -> {', '.join(self.chunk_list)}"]
        for w in self.warnings:
            lines.append(f"  ! {w}")
        return "\n".join(lines)


def swap_u32s(data: bytes) -> bytes:
    assert len(data) % 4 == 0
    return b"".join(data[i:i + 4][::-1] for i in range(0, len(data), 4))


def convert_extra(extra_be: bytes) -> bytes:
    """MC02 'extra' preamble, 360 -> PC.

    career (28 B): all numeric -> swap.
    alias (64 B): [id][used][1][day][0] swap; NUL-terminated player name
    natural; 0xAA pad words natural; tail floats/hash swap.
    """
    n = len(extra_be)
    if n == 28:
        return swap_u32s(extra_be)
    if n != 64:
        raise ValueError(f"unexpected extra size {n}")
    out = bytearray(extra_be)
    for i in range(0, 0x14, 4):
        out[i:i + 4] = extra_be[i:i + 4][::-1]
    # name: printable run terminated by NUL, then 0xAA pad words only
    end = 0x14
    while end < n and extra_be[end] not in (0x00, 0xAA) and 0x20 <= extra_be[end] < 0x7F:
        out[end] = extra_be[end]
        end += 1
    if end < n and extra_be[end] == 0x00:
        out[end] = 0
        end += 1
    while end + 4 <= n and extra_be[end:end + 4] == b"\xaa\xaa\xaa\xaa":
        end += 4
    # align to 4 and swap the numeric tail
    end = (end + 3) & ~3
    for i in range(end, n, 4):
        out[i:i + 4] = extra_be[i:i + 4][::-1]
    return bytes(out)


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


def convert_tree(tree360: Tree, report: ConversionReport, twin: Tree | None = None) -> Tree:
    if tree360.gap:
        report.warnings.append(
            f"{tree360.gap:#x} bytes between last parseable record and used-size "
            "anchor (console file's record tail is damaged noise)")
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
        mode = convert_record(report.kind, rec, report.warnings)
        if mode == "auto" and len(rec.payload) > 0x1000:
            report.warnings.append(
                f"chunk {CHUNK_NAMES.get(rec.id, hex(rec.id))} ({len(rec.payload):#x} B) "
                "converted in auto mode (no fieldmap)")
        _to_pc_record(rec)
        pc.records.append(rec)
    if twin is not None and len(twin.records) > len(tree360.records):
        # console tail damaged: recover the trailing records from a re-save of
        # the same session (recomp twin) — records before the tail keep the
        # authentic console bytes
        have = {r.id for r in pc.records}
        for rec in twin.records[len(tree360.records):]:
            if rec.id in have:
                continue
            convert_record(report.kind, rec, report.warnings)
            _to_pc_record(rec)
            pc.records.append(rec)
            report.warnings.append(
                f"record {CHUNK_NAMES.get(rec.id, hex(rec.id))} recovered from re-save twin "
                "(console copy damaged)")
    elif tree360.gap and len(pc.records) < tree360.count:
        missing = tree360.count - len(pc.records)
        report.warnings.append(
            f"{missing} record(s) missing: console tail damaged and no twin available; "
            "the game will use defaults for those chunks (CustomRaceDayMemcard/"
            "UnlockSystem carry race-day and unlock state)")
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
    tree_bytes = pc_tree.build(big=False, tree_size=mc02_be.tree_size, rec_start=REC_START_PC)
    # 128-bit tree hash (loader never verifies it, but native saves carry it)
    tree_bytes = bytearray(tree_bytes)
    tree_bytes[0:16] = tree_hash(bytes(tree_bytes))
    pc = MC02(Endian.LITTLE, extra=bytearray(convert_extra(mc02_be.extra)),
              tree=bytes(tree_bytes), tree_size=mc02_be.tree_size)
    return pc


def write_pc_save(mc02_pc: MC02, name: str, save_root: str = PC_SAVE_ROOT) -> Path:
    root = Path(save_root)
    folder = root / name
    folder.mkdir(parents=True, exist_ok=True)
    target = folder / name
    target.write_bytes(mc02_pc.to_bytes())
    return target
