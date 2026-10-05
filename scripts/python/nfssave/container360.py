"""Xbox 360 save container reader (STFS 'CON ' package).

The saves are standard STFS packages (console-signed CON, unsigned content
works the same way):
  0x0000  'CON ' header + certificate, metadata, PNG icons
  0x0340  BE u32 header size -> first hash table at (size + 0xFFF) & ~0xFFF
          (0xA000 for these saves)
  0x0379  volume descriptor; byte 0x37B bit 0 = block separation. Bit clear
          -> two copies of every hash table per level (table shift 1)
  data block 0 = file table (one 0x40-byte entry per file):
          +0x00 name (0x28, NUL-padded), +0x28 flags (0x40 = contiguous,
          low 6 bits = name length), +0x29 block count (LE24),
          +0x2F starting block (LE24), +0x34 BE u32 file size,
          +0x38/+0x3C update/access timestamps
Data blocks are interleaved with hash tables: a level-0 table group precedes
every 170 data blocks (and a level-1 group every 170*170), so a payload
larger than ~0xA9000 bytes is NOT one contiguous slice of the file. Reading
it as one (as earlier versions did) pulls hash tables into the save and
truncates its tail.
"""

import struct
from dataclasses import dataclass
from pathlib import Path

BLOCK = 0x1000
HASHES_PER_TABLE = 0xAA      # 170 data blocks per level-0 hash table
L1_SPAN = 0x70E4             # 170 * 170 data blocks per level-1 hash table
FLAG_CONTIGUOUS = 0x40


def stfs_block_offset(block: int, first_table: int, shift: int) -> int:
    """File offset of data block `block` (Free60/Velocity backing-block math).

    shift is 1 when every hash table is stored twice, else 0.
    """
    backing = (((block + HASHES_PER_TABLE) // HASHES_PER_TABLE) << shift) + block
    if block >= HASHES_PER_TABLE:
        backing += ((block + L1_SPAN) // L1_SPAN) << shift
        if block >= L1_SPAN:
            backing += 1 << shift
    return first_table + backing * BLOCK


@dataclass
class Container360:
    data: bytes
    name: str
    payload: bytes


def parse_container(data: bytes, label: str = "container") -> Container360:
    if data[:4] != b"CON ":
        raise ValueError(f"{label}: not a CON container (magic {data[:4]!r})")
    if len(data) < 0x381:
        raise ValueError(f"{label}: truncated CON header ({len(data):#x} B)")
    header_size = struct.unpack_from(">I", data, 0x340)[0]
    first_table = (header_size + BLOCK - 1) & ~(BLOCK - 1)
    shift = 0 if data[0x37B] & 1 else 1

    def block_at(n: int) -> bytes:
        off = stfs_block_offset(n, first_table, shift)
        if off >= len(data):
            raise ValueError(f"{label}: data block {n} at {off:#x} beyond file end")
        return data[off:off + BLOCK]

    table_block = int.from_bytes(data[0x37E:0x381], "little")
    entry = block_at(table_block)[:0x40]
    nul = entry.find(b"\0", 0, 0x28)
    name = entry[:nul if nul != -1 else 0x28].decode("ascii", errors="replace")
    if not name:
        raise ValueError(f"{label}: empty STFS file table")
    flags = entry[0x28]
    n_blocks = int.from_bytes(entry[0x29:0x2C], "little")
    start = int.from_bytes(entry[0x2F:0x32], "little")
    size = struct.unpack_from(">I", entry, 0x34)[0]
    if not flags & FLAG_CONTIGUOUS:
        raise ValueError(f"{label}: non-contiguous STFS file '{name}' is not supported")
    if n_blocks * BLOCK < size:
        raise ValueError(f"{label}: '{name}' size {size:#x} exceeds its {n_blocks} blocks")
    payload = b"".join(block_at(start + i) for i in range(n_blocks))[:size]
    if len(payload) != size:
        raise ValueError(f"{label}: '{name}' truncated ({len(payload):#x} of {size:#x} B)")
    return Container360(data, name, payload)


def read_container(path: str | Path) -> Container360:
    p = Path(path)
    return parse_container(p.read_bytes(), str(p))
