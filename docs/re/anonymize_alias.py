"""Anonymize a 360 alias save (STFS CON + MC02) into a shareable test fixture.

    python docs/re/anonymize_alias.py SRC DST NEW_NAME

Rewrites the player name (same length as the original, so every fixed-width
field and the STFS name-length bits stay valid) in:
  - the STFS file-table entry ('ALIAS_<name>')
  - the CON display name (UTF-16BE, every locale slot that holds it)
  - the MC02 payload: extra blob name field and the UserProfile chunk
and blanks console/profile identity in the CON header: the certificate body
(console id, part number, date, public key, signatures; 0x06..0x22C),
console id 0x36C, profile id 0x371, device id 0x3FD.

The MC02 header/extra/tree CRCs are recomputed so the payload passes
MC02.check(). Left stale on purpose: the STFS hash tables, the header SHA-1
(0x32C) and the 360 tree hash at tree[0:0x10] (the 360 build does not use the
PC treehash scheme) - nfssave's reader verifies none of them. The output is a
converter fixture, not a console-loadable package.

Fails loudly if the name is found anywhere it is not expected, or if any
identity bytes survive.
"""

import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts" / "python"))

from nfssave.container360 import BLOCK, parse_container, stfs_block_offset  # noqa: E402
from nfssave.crc import crc32_ea  # noqa: E402
from nfssave.mc02 import HEADER_SIZE, MC02  # noqa: E402

ALIAS_PREFIX = "ALIAS_"
DISPLAY_NAME = 0x411          # 18 locales x 0x80 B, UTF-16BE
LOCALES = 18
CERT_BODY = (0x06, 0x22C)
CONSOLE_ID = (0x36C, 5)
PROFILE_ID = (0x371, 8)
DEVICE_ID = (0x3FD, 0x14)


def _payload_blocks(data: bytes) -> tuple[int, list[int]]:
    """(file-table entry offset, file offsets of the payload's data blocks)."""
    header_size = struct.unpack_from(">I", data, 0x340)[0]
    first_table = (header_size + BLOCK - 1) & ~(BLOCK - 1)
    shift = 0 if data[0x37B] & 1 else 1
    table_block = int.from_bytes(data[0x37E:0x381], "little")
    entry_off = stfs_block_offset(table_block, first_table, shift)
    entry = data[entry_off:entry_off + 0x40]
    n_blocks = int.from_bytes(entry[0x29:0x2C], "little")
    start = int.from_bytes(entry[0x2F:0x32], "little")
    return entry_off, [stfs_block_offset(start + i, first_table, shift)
                       for i in range(n_blocks)]


def _recrc(payload: bytes) -> bytes:
    """Recompute the three big-endian MC02 CRCs in place."""
    p = bytearray(payload)
    _, total, extra_size, _ = struct.unpack_from(">IIII", p, 0)
    extra = p[HEADER_SIZE:HEADER_SIZE + extra_size]
    tree = p[HEADER_SIZE + extra_size:total]
    struct.pack_into(">II", p, 0x10, crc32_ea(bytes(extra)), crc32_ea(bytes(tree)))
    struct.pack_into(">I", p, 0x18, crc32_ea(bytes(p[:0x18])))
    return bytes(p)


def anonymize(data: bytes, new_name: str) -> bytes:
    c = parse_container(data, "source")
    if not c.name.startswith(ALIAS_PREFIX):
        raise ValueError(f"not an alias save: {c.name!r}")
    old = c.name[len(ALIAS_PREFIX):].encode("ascii")
    new = new_name.encode("ascii")
    if len(new) != len(old):
        raise ValueError(f"new name must be {len(old)} chars like {old!r}, got {len(new)}")
    out = bytearray(data)

    # identity fields recorded before blanking, to prove none survive
    secrets = [bytes(data[o:o + n]) for o, n in (CONSOLE_ID, PROFILE_ID, DEVICE_ID)]
    secrets = [s for s in secrets if s.strip(b"\0 ")]

    # STFS file table entry name (length unchanged -> flags bits unchanged)
    entry_off, blocks = _payload_blocks(data)
    assert out[entry_off:entry_off + len(c.name)] == c.name.encode("ascii")
    out[entry_off + len(ALIAS_PREFIX):entry_off + len(c.name)] = new

    # CON display name slots
    old16, new16 = old.decode().encode("utf-16-be"), new.decode().encode("utf-16-be")
    for i in range(LOCALES):
        off = DISPLAY_NAME + i * 0x80
        if out[off:off + len(old16)] == old16:
            out[off:off + len(new16)] = new16

    # identity blanking
    lo, hi = CERT_BODY
    out[lo:hi] = bytes(hi - lo)
    for off, n in (CONSOLE_ID, PROFILE_ID, DEVICE_ID):
        out[off:off + n] = bytes(n)

    # MC02 payload: name occurrences (extra blob + UserProfile), then CRCs
    payload = c.payload
    hits = [i for i in range(len(payload)) if payload.startswith(old, i)]
    if len(hits) != 2:
        raise ValueError(f"expected the name twice in the payload (extra, UserProfile), "
                         f"found at {[hex(h) for h in hits]}")
    p = bytearray(payload)
    for h in hits:
        p[h:h + len(old)] = new
    p = _recrc(bytes(p))
    for i, off in enumerate(blocks):
        chunk = p[i * BLOCK:(i + 1) * BLOCK]
        out[off:off + len(chunk)] = chunk

    # verification
    out = bytes(out)
    check = parse_container(out, "anonymized")
    if check.name != ALIAS_PREFIX + new_name or check.payload != p:
        raise AssertionError("re-parse mismatch")
    probs = MC02.parse(check.payload).check()
    if probs:
        raise AssertionError(f"MC02 check failed: {probs}")
    for needle in (old, old16, old.decode().encode("utf-16-le"), *secrets):
        if needle in out:
            raise AssertionError(f"identity bytes survive at {out.find(needle):#x}: {needle!r}")
    return out


def main(argv: list[str]) -> int:
    if len(argv) != 4:
        print(__doc__)
        return 2
    src, dst, name = Path(argv[1]), Path(argv[2]), argv[3]
    out = anonymize(src.read_bytes(), name)
    dst.parent.mkdir(parents=True, exist_ok=True)
    dst.write_bytes(out)
    print(f"wrote {dst} ({len(out)} B), container name {ALIAS_PREFIX + name!r}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
