"""Chunk-tree parse/convert for NFS ProStreet MC02 saves.

Grammar (verified empirically and cross-checked against PC-native saves):

tree := noise[16] count:u32 pad:magic 0x59F2D89B used:u32 records...
  - 360: records start at tree+0x48 (record = [junk][id][size][payload]);
    PC: tree+0x1CC (record = [id][size][flags][payload]), root record at
    tree+0x1C0 (360: +0x40).
  - id is the platform-independent chunk identity (matches 360<->PC).
  - the first payload word of a 360 record is a 0x01xxxxxx marker + junk;
    PC drops it and appends 4 trailing junk bytes (same total size).
  - records tile exactly to records_start + used. The console writing bug
    can leave damaged noise in the record region: a trailing gap (records
    missing at the end) or, in principle, an internal gap (parse
    re-anchors at the first offset where a clean record chain tiles to the
    end again).
  - after the record area, the rest of the fixed-size tree buffer holds a
    hash directory table ([h1][h2][0xFFFFFFFF][0] cells); the loader never
    reads it on either platform (byte-swapped on conversion).

Payload conversion engines live in payload_rules; the platform buffer
normalization for GameplayData lives in convert.normalize_gameplay.
"""

import struct
from dataclasses import dataclass

TREE_MAGIC = 0x59F2D89B
REC_START_360 = 0x48
REC_START_PC = 0x1CC
PC_ROOT_OFF = 0x1C0  # root record [id][size][flags] ; children follow at 0x1CC

REANCHOR_MAX_GAP = 0x4000  # don't resync through implausibly large noise


@dataclass
class Record:
    type: int
    id: int
    size: int
    payload: bytes


@dataclass
class Tree:
    noise: bytes           # 16 bytes: 128-bit tree hash (never verified at load; recomputed on write)
    count: int
    pre_records: bytes     # allocator garbage + machine GUID region (never read on load)
    records: list
    post: bytes            # everything after the record area (360-side directory)
    used: int
    gap: int = 0           # bytes of damaged noise skipped inside the record region

    @staticmethod
    def parse(tree: bytes, big: bool) -> "Tree":
        e = ">" if big else "<"
        count = struct.unpack_from(e + "I", tree, 0x10)[0]
        rec_start = REC_START_360 if big else REC_START_PC
        # locate magic between 0x14 and rec_start
        magic_off = None
        for off in range(0x14, rec_start - 4, 4):
            if struct.unpack_from(e + "I", tree, off)[0] == TREE_MAGIC:
                magic_off = off
                break
        if magic_off is None:
            raise ValueError("tree magic 0x59F2D89B not found")
        used = struct.unpack_from(e + "I", tree, magic_off + 4)[0]
        if used > len(tree) - rec_start:
            raise ValueError(f"corrupt used size {used:#x} exceeds tree buffer")
        records = []
        off = rec_start
        end = rec_start + used
        stopped = None
        while off + 12 <= min(end, len(tree)):
            if big:
                t, i, s = struct.unpack_from(">III", tree, off)          # [junk][id][size]
            else:
                i, s, t = struct.unpack_from("<III", tree, off)          # [id][size][flags]
            if off + 12 + s > end or (s == 0 and i == 0 and t == 0):
                stopped = off
                break
            # size-0 records are legal (positional hole fillers); 12-byte stride
            records.append(Record(t, i, s, bytes(tree[off + 12:off + 12 + s])))
            off += 12 + s
        if stopped is None:
            stopped = off
        gap = max(0, end - stopped)
        if gap:
            records = records + Tree._reafter_gap(tree, stopped, end, big)
        return Tree(
            noise=bytes(tree[0:0x10]),
            count=count,
            pre_records=bytes(tree[0x14:rec_start]),
            records=records,
            post=bytes(tree[end:]),
            used=used,
            gap=gap,
        )

    @staticmethod
    def _reafter_gap(tree: bytes, stopped: int, end: int, big: bool) -> list:
        """Internal-gap recovery: find a 4-aligned offset after the noise
        where a clean record chain tiles exactly to `end`; damage beyond
        REANCHOR_MAX_GAP is treated as unrecoverable tail loss."""
        if end - stopped > REANCHOR_MAX_GAP:
            return []
        e = ">" if big else "<"
        for cand in range((stopped + 4 + 3) & ~3, end - 11, 4):
            off = cand
            recs = []
            ok = True
            while off + 12 <= end:
                if big:
                    t, i, s = struct.unpack_from(">III", tree, off)
                else:
                    i, s, t = struct.unpack_from("<III", tree, off)
                if (off + 12 + s > end) or (s == 0 and i == 0 and t == 0):
                    ok = False
                    break
                recs.append(Record(t, i, s, bytes(tree[off + 12:off + 12 + s])))
                off += 12 + s
            if ok and off == end:
                return recs
        return []

    def build(self, big: bool, tree_size: int) -> bytes:
        e = ">" if big else "<"
        body = bytearray()
        for rec in self.records:
            if big:
                body += struct.pack(">III", rec.type, rec.id, len(rec.payload))
            else:
                body += struct.pack("<III", rec.id, len(rec.payload), rec.type)
            body += rec.payload
        used = len(body)
        # patch magic+used inside pre_records
        pre = bytearray(self.pre_records)
        magic_off = None
        for off in range(0, len(pre) - 4, 4):
            if struct.unpack_from(e + "I", pre, off)[0] == TREE_MAGIC:
                magic_off = off
                break
        if magic_off is None:
            raise ValueError("tree magic lost in pre_records")
        struct.pack_into(e + "I", pre, magic_off + 4, used)
        out = self.noise + struct.pack(e + "I", self.count) + bytes(pre) + bytes(body)
        # the post-records directory region is variable-length: keep as much
        # as fits in the fixed tree buffer
        room = tree_size - len(out)
        if room < 0:
            raise ValueError(f"tree overflow: records end at {len(out):#x} > {tree_size:#x}")
        out += self.post[:room]
        out += b"\0" * (tree_size - len(out))
        return out


def swap_u32s(data: bytes) -> bytes:
    assert len(data) % 4 == 0
    return b"".join(data[i:i + 4][::-1] for i in range(0, len(data), 4))
