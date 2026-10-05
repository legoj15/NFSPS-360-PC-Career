"""MC02 save file format (shared container by PC and Xbox 360, different endianness).

Layout (0x1C-byte header, then two blobs):
  0x00  u32  magic 0x4D433032 ('MC02')
  0x04  u32  total file size (== extra_size + tree_size + 0x1C)
  0x08  u32  extra blob size (career 28, alias 64)
  0x0C  u32  tree blob size (career 0xB6800, alias 0x5000)
  0x10  u32  CRC(extra blob)
  0x14  u32  CRC(tree blob)
  0x18  u32  CRC(header bytes 0x00..0x18)
  0x1C  extra blob  (fixed preamble: identity hash, used-tree size, version
        ints 1/7/0/100, floats; alias additionally the player name string)
  0x1C+extra_size  tree blob (chunk records; big-endian on 360, little-endian on PC)

All multi-byte fields native to the platform: big-endian on 360, little-endian on PC.
The CRC (see nfssave.crc) runs over raw bytes, so the same code validates both.
"""

import struct
from dataclasses import dataclass, field
from enum import Enum

from .crc import crc32_ea

MAGIC = 0x4D433032
HEADER_SIZE = 0x1C


class Endian(str, Enum):
    BIG = ">"
    LITTLE = "<"


@dataclass
class MC02:
    endian: Endian
    extra: bytes = b""
    tree: bytes = b""
    tree_size: int = 0  # declared (buffer) size; tree may be shorter

    @staticmethod
    def parse(data: bytes) -> "MC02":
        if len(data) < HEADER_SIZE:
            raise ValueError("MC02: truncated header")
        magic = struct.unpack_from("<I", data, 0)[0]
        if magic == MAGIC:
            end = Endian.LITTLE
        elif struct.unpack_from(">I", data, 0)[0] == MAGIC:
            end = Endian.BIG
        else:
            raise ValueError(f"MC02: bad magic {magic:#x}")
        e = end.value
        magic, total, extra_size, tree_size = struct.unpack_from(e + "IIII", data, 0)
        crc_extra, crc_tree, crc_hdr = struct.unpack_from(e + "III", data, 0x10)
        if total != len(data):
            raise ValueError(f"MC02: size field {total:#x} != file size {len(data):#x}")
        extra = data[HEADER_SIZE:HEADER_SIZE + extra_size]
        tree = data[HEADER_SIZE + extra_size:]
        m = MC02(end, extra, tree, tree_size)
        m._stored = (crc_extra, crc_tree, crc_hdr)
        m.total = total
        return m

    def check(self) -> list[str]:
        """Return a list of validation problems (empty == valid)."""
        probs = []
        e = self.endian.value
        crc_extra, crc_tree, crc_hdr = getattr(self, "_stored", (None,)*3)
        if crc_hdr is not None and crc_hdr != crc32_ea(self.header_bytes()[:0x18]):
            probs.append("header CRC mismatch")
        if crc_extra is not None and crc_extra != crc32_ea(self.extra):
            probs.append("extra CRC mismatch")
        if crc_tree is not None and crc_tree != crc32_ea(self.tree):
            probs.append("tree CRC mismatch")
        return probs

    def header_bytes(self) -> bytes:
        e = self.endian.value
        tree = self.tree
        if len(tree) < self.tree_size:
            tree = tree + b"\0" * (self.tree_size - len(tree))
        hdr = struct.pack(
            e + "IIIIIII",
            MAGIC,
            HEADER_SIZE + len(self.extra) + len(tree),
            len(self.extra),
            len(tree),
            crc32_ea(self.extra),
            crc32_ea(tree),
            0,  # header CRC patched below
        )
        return hdr[:0x18] + struct.pack(e + "I", crc32_ea(hdr[:0x18]))

    def to_bytes(self) -> bytes:
        tree = self.tree
        if len(tree) > self.tree_size:
            raise ValueError(
                f"tree data ({len(tree):#x} B) exceeds declared buffer "
                f"{self.tree_size:#x} - refusing to truncate")
        if len(tree) < self.tree_size:
            tree = tree + b"\0" * (self.tree_size - len(tree))
        return self.header_bytes() + self.extra + tree

    # -- extra blob accessors ------------------------------------------------
    def extra_field(self, index: int) -> int:
        e = self.endian.value
        return struct.unpack_from(e + "I", self.extra, index * 4)[0]

    def set_extra_field(self, index: int, value: int) -> None:
        e = self.endian.value
        struct.pack_into(e + "I", self.extra, index * 4, value)

    @property
    def used_tree_size(self) -> int:
        return self.extra_field(1)
