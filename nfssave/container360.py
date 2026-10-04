"""Xbox 360 save container reader (the 'CON ' wrapper EA 360 saves sit in).

Empirically mapped layout (files from a RGH console's Content directory):
  0x0000: 'CON ' + 8B console info + console serial ASCII (e.g. '852197-001')
          + date ASCII (e.g. '12-10-10') + console certificate (~to 0x1AC)
  0x01AC: per-file metadata (title id, content ids, PNG icons, ...)
  0xC000: entry: filename (NUL-padded), then at +0x28: u32 hash, +0x2C block count,
          +0x30 flags (00 00 ff ff), +0x34 BE u32 payload size,
          +0x38/+0x3C BE u32 payload checksum (stored twice)
  0xD000: raw payload (the MC02 save), padded to a 0x1000 block boundary
This is NOT a standard retail STFS package (no 0x114-byte RSA signature block);
it is the simplified container the homebrew stack on the source console wrote.
"""

from dataclasses import dataclass
from pathlib import Path

ENTRY_OFFSET = 0xC000
PAYLOAD_OFFSET = 0xD000


@dataclass
class Container360:
    data: bytes
    name: str
    payload_size: int
    checksum: int

    @property
    def payload(self) -> bytes:
        return self.data[PAYLOAD_OFFSET:PAYLOAD_OFFSET + self.payload_size]

    def payload_checksum_ok(self, crc_fn) -> bool:
        return crc_fn(self.payload) == self.checksum


def read_container(path: str | Path) -> Container360:
    data = Path(path).read_bytes()
    if data[:4] != b"CON ":
        raise ValueError(f"{path}: not a CON container (magic {data[:4]!r})")
    name_end = data.index(b"\0", ENTRY_OFFSET)
    name = data[ENTRY_OFFSET:name_end].decode("ascii", errors="replace")
    import struct
    payload_size = struct.unpack_from(">I", data, ENTRY_OFFSET + 0x34)[0]
    checksum = struct.unpack_from(">I", data, ENTRY_OFFSET + 0x38)[0]
    if PAYLOAD_OFFSET + payload_size > len(data):
        raise ValueError(f"{path}: payload {payload_size:#x} exceeds file size {len(data):#x}")
    return Container360(data, name, payload_size, checksum)
