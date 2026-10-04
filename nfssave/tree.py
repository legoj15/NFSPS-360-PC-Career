"""Chunk-tree parse/convert for NFS ProStreet MC02 saves.

Grammar (verified empirically on 4 files; code-level confirmation pending from
the recomp/exe analyses):

tree := noise[16] count:u32 pad:magic 0x59F2D89B used:u32 records...
  - 360: records start at tree+0x48; PC: tree+0x1C8 (after a 0x1AC preamble
    chunk that PC serializes from uninitialized memory).
  - record := type:u32 id:u32 size:u32 payload[size]
    * id is the platform-independent chunk identity (matches 360<->PC).
    * type is partly volatile (uninitialized heap on PC; timestamps on 360);
      small values (0,2,3,...) are meaningful flags. We copy it through.
    * records tile exactly to records_start + used (alias verified; career
      has a 0x720 obfuscated tail region - see below).
  - after the record area, the rest of the fixed-size tree buffer holds a
    hash directory table ([h1][h2][0xFFFFFFFF][0] cells); it is byte-swapped
    as numeric data.
  - container chunks: payload = [0x01000000][nested record...]; recurse.

Payload conversion: property-node data - numeric leaves swap BE<->LE, string
data stays natural. The per-chunk engines live in payload_rules; see
research/fieldmaps for the empirically-derived classifications.
"""

import struct
from dataclasses import dataclass, field as dfield

TREE_MAGIC = 0x59F2D89B
REC_START_360 = 0x48
REC_START_PC = 0x1CC
PC_ROOT_OFF = 0x1C0  # root record [id][size][flags] ; children follow at 0x1CC


@dataclass
class Record:
    type: int
    id: int
    size: int
    payload: bytes
    children: list = dfield(default_factory=list)


@dataclass
class Tree:
    noise: bytes           # 16 bytes: 128-bit tree hash (never verified at load; recomputed on write)
    count: int
    pre_records: bytes     # allocator garbage + machine GUID region (never read on load)
    records: list
    post: bytes            # everything after the record area (360-side directory)
    used: int
    gap: int = 0           # bytes between last parseable record and used end

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
            payload = bytes(tree[off + 12:off + 12 + s])
            rec = Record(t, i, s, payload)
            _parse_nested(rec, big)
            records.append(rec)
            off += 12 + s
        if stopped is None:
            stopped = off
        gap = max(0, end - stopped)
        return Tree(
            noise=bytes(tree[0:0x10]),
            count=count,
            pre_records=bytes(tree[0x14:rec_start]),
            records=records,
            post=bytes(tree[stopped:]),
            used=used,
            gap=gap,
        )

    def build(self, big: bool, tree_size: int, rec_start: int) -> bytes:
        e = ">" if big else "<"
        body = bytearray()
        for rec in self.records:
            if big:
                body += struct.pack(">III", rec.type, rec.id, len(rec.payload))
            else:
                body += struct.pack("<III", rec.id, len(rec.payload), rec.type)
            body += rec.payload
        used = len(body)
        hdr = self.noise + struct.pack(e + "I", self.count) + self.pre_records
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


def _parse_nested(rec: Record, big: bool, depth: int = 0) -> None:
    """Container chunks: payload = [0x01000000][nested record]..."""
    if depth > 4 or len(rec.payload) < 16:
        return
    e = ">" if big else "<"
    if struct.unpack_from(e + "I", rec.payload, 0)[0] != 0x01000000:
        return
    off = 4
    while off + 12 <= len(rec.payload):
        t, i, s = struct.unpack_from(e + "III", rec.payload, off)
        if s == 0 or off + 12 + s > len(rec.payload):
            break
        child = Record(t, i, s, bytes(rec.payload[off + 12:off + 12 + s]))
        _parse_nested(child, big, depth + 1)
        rec.children.append(child)
        off += 12 + s


def swap_u32s(data: bytes) -> bytes:
    assert len(data) % 4 == 0
    return b"".join(data[i:i + 4][::-1] for i in range(0, len(data), 4))


def swap_u16_pairs(data: bytes) -> bytes:
    """Swap within 16-bit pairs (for regions of u16 arrays)."""
    assert len(data) % 2 == 0
    return b"".join(data[i:i + 2][::-1] for i in range(0, len(data), 2))
