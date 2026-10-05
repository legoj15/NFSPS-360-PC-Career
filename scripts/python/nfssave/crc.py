"""EA's CRC-32 variant used by NFS ProStreet saves (both PC and Xbox 360).

Algorithm (reversed from nfs.exe @ 0x8A18F6 / 360 xex sub_827C75B8):
  - MSB-first table-driven CRC over RAW bytes (no endianness of the data involved)
  - init: first 4 bytes preloaded into the register (b0<<24|b1<<16|b2<<8|b3), then NOT
  - step: crc = ((crc << 8) | byte) ^ table[crc >> 24]
  - final: NOT
  - lengths < 4 return 0
The table is the standard MSB-first CRC-32 table (poly 0x04C11DB7).
"""

from functools import lru_cache
from typing import Iterable


@lru_cache(maxsize=1)
def _table() -> tuple[int, ...]:
    tbl = []
    for i in range(256):
        c = i << 24
        for _ in range(8):
            c = ((c << 1) ^ 0x04C11DB7) & 0xFFFFFFFF if c & 0x80000000 else (c << 1) & 0xFFFFFFFF
        tbl.append(c)
    return tuple(tbl)


def crc32_ea(data: bytes) -> int:
    tbl = _table()
    n = len(data)
    if n < 4:
        return 0
    crc = (data[0] << 24) | (data[1] << 16) | (data[2] << 8) | data[3]
    crc ^= 0xFFFFFFFF
    for b in data[4:]:
        crc = (((crc << 8) & 0xFFFFFFFF) | b) ^ tbl[crc >> 24]
    return crc ^ 0xFFFFFFFF


def crc_table_from_blob(blob: bytes) -> tuple[int, ...]:
    """Load a 1024-entry LE u32 table (e.g. lifted from an exe)."""
    import struct
    return struct.unpack("<1024I", blob[:4096])
