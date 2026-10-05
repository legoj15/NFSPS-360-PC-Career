#!/usr/bin/env python3
"""Empirical walker for NFS ProStreet (2007) PC MC02 saves (little-endian).

The PC save is the SAME MC02 container as Xbox 360, byte-swapped:
  [0x00 '20CM'][total][extra][tree][CRC extra][CRC tree][CRC hdr][extra][tree]
  All u32s little-endian, CRCs computed over the stored (LE) bytes with the
  same MSB-first seeded algorithm as the 360 files (verified on both files).

Tree layout (verified on both PC files):
  [16B noise][u32 count][preamble chunk: 0x1AC bytes, NOT counted][u32 0x59F2D89B][u32 used]
  [count records of [u32 type][u32 id][u32 size][payload]] ...
  used == (end of last record) - (start of records)  -- exact on both files.
  Records start at tree+0x1C8 (magic at tree+0x1C0, same offset in both files).

Chunk ids are PLATFORM-INDEPENDENT: they match the 360 record ids exactly
(e.g. alias chunk 0x8FFBE3E8 size 0x598 on both platforms).  Sizes are
structural (fixed) for most chunks and content-dependent for a few.

The type slot is garbage on PC (uninitialized heap: 0x10101001, 0xFFFFFF01,
float bits, etc.) -- the PC writer is a memory-image serializer and leaves
type words uninitialized.  0x10101010 fill also replaces 360 string-name
fragments inside property chunks.

PC-only chunks: alias has an extra record id 0x39156567 (type 3, after
0x9CB326C2) -> count 15 vs 360's 14.  Career reveals the 360 career's two
missing tail chunks: 0xD548266C (size 0x24) and 0xCA269650 (size 0x44) --
on 360 these live in the high-entropy gap 0xAA6CC..0xAADEC (0x720 bytes,
headers obfuscated/encrypted; sizes differ because payloads are player data).
"""
import struct

POLY = 0x04C11DB7


def crc_table():
    tbl = []
    for i in range(256):
        c = i << 24
        for _ in range(8):
            c = ((c << 1) ^ POLY) & 0xFFFFFFFF if c & 0x80000000 else (c << 1) & 0xFFFFFFFF
        tbl.append(c)
    return tbl


TBL = crc_table()


def crc32_msb(data: bytes) -> int:
    """Same seeded MSB-first CRC as 360 (seed = BE32 of first 4 stored bytes)."""
    if len(data) < 4:
        return 0
    crc = struct.unpack('>I', data[:4])[0] ^ 0xFFFFFFFF
    for b in data[4:]:
        crc = (((crc << 8) | b) & 0xFFFFFFFF) ^ TBL[(crc >> 24) & 0xFF]
    return crc ^ 0xFFFFFFFF


MAGIC_LE = b'\x9b\xd8\xf2\x59'


def ascii_strings(data, minlen=4):
    out, cur, st = [], bytearray(), 0
    for idx, b in enumerate(data):
        if 32 <= b < 127:
            if not cur:
                st = idx
            cur.append(b)
        else:
            if len(cur) >= minlen:
                out.append((st, cur.decode()))
            cur = bytearray()
    if len(cur) >= minlen:
        out.append((st, cur.decode()))
    return out


def walk(tree):
    """Return (count, preamble, magic_off, used, records)."""
    count = struct.unpack('<I', tree[0x10:0x14])[0]
    m = tree.find(MAGIC_LE, 0x14)
    if m < 0:
        raise ValueError('magic not found')
    used = struct.unpack('<I', tree[m + 4:m + 8])[0]
    recs = []
    off = m + 8
    while off + 12 <= len(tree):
        t, i, s = struct.unpack('<III', tree[off:off + 12])
        if s == 0 or off + 12 + s > len(tree):
            break
        recs.append(dict(off=off, type=t, id=i, size=s,
                         payload_off=off + 12, end=off + 12 + s))
        off += 12 + s
    return count, tree[0x14:m], m, used, recs


def main(path, label, outpath):
    d = open(path, 'rb').read()
    total, extra, tsize = struct.unpack('<III', d[4:16])
    crc_e, crc_t, crc_h = struct.unpack('<III', d[0x10:0x1C])
    tree = d[0x1C + extra:0x1C + extra + tsize]
    L = []
    L.append(f'=== {label} (PC, little-endian) ===')
    L.append(f'file: {path}')
    L.append(f'total=0x{total:X} extra=0x{extra:X} tree=0x{tsize:X}')
    L.append(f'CRC hdr:   0x{crc_h:08X} {"OK" if crc_h == crc32_msb(d[0:0x18]) else "FAIL"}')
    L.append(f'CRC extra: 0x{crc_e:08X} {"OK" if crc_e == crc32_msb(d[0x1C:0x1C + extra]) else "FAIL"}')
    L.append(f'CRC tree:  0x{crc_t:08X} {"OK" if crc_t == crc32_msb(tree) else "FAIL"}')
    L.append(f'extra blob: {d[0x1C:0x1C + extra].hex()}')

    count, pre, m, used, recs = walk(tree)
    last_end = recs[-1]['end'] if recs else m + 8
    L.append(f'count field = {count}; walked {len(recs)} records')
    L.append(f'preamble (uncounted) tree 0x14..0x{m:X} ({m - 0x14:#x} bytes)')
    L.append(f'magic @tree 0x{m:X}; used=0x{used:X}; records start 0x{m + 8:X}; '
             f'used-end 0x{m + 8 + used:X}; last record ends 0x{last_end:X}; tree len 0x{len(tree):X}')
    L.append(f'TILING: {"EXACT - last record ends at records_start+used" if last_end == m + 8 + used else "MISMATCH"}')
    L.append('')
    L.append('idx  tree_off  type       id         size     end       notes')
    for n, r in enumerate(recs):
        p = tree[r['payload_off']:r['end']]
        notes = []
        if r['type'] & 0x01010101 == 0x01010101 or r['type'] in (0x10101010,):
            notes.append('type=0x10-fill garbage')
        ss = ascii_strings(p, 5)
        if ss:
            notes.append('strings: ' + ' | '.join(f'@{o}:{t}' for o, t in ss[:6]))
        L.append(f'{n:<3}  0x{r["off"]:06X}  0x{r["type"]:08X}  0x{r["id"]:08X}  0x{r["size"]:06X}  0x{r["end"]:06X}  ' + '; '.join(notes))
        L.append(f'     head: {p[:32].hex()}')
    # trailing region
    L.append('')
    L.append(f'--- trailing region after records: tree 0x{last_end:X}..0x{len(tree):X} (0x{len(tree) - last_end:X} bytes)')
    tr = tree[last_end:]
    z = len(tr) - len(tr.rstrip(b'\x00'))
    t10 = sum(1 for i in range(0, len(tr) - 3, 4) if tr[i:i + 4] == b'\x10\x10\x10\x10')
    L.append(f'      trailing zero bytes: {z}, 0x10101010 words: {t10}')
    text = '\n'.join(L)
    open(outpath, 'w').write(text)
    print(text)
    return recs


if __name__ == '__main__':
    import os
    here = os.path.dirname(os.path.abspath(__file__))
    main('E:/legoj/Documents/Need for Speed ProStreet/100% Gamesave (OPTIONAL) - Place This in SAVE folder below/NFS Prostreet/ALIAS_PEIROKUNMANWSP/ALIAS_PEIROKUNMANWSP',
         'PC ALIAS (PEIROKUNMANWSP)',
         os.path.join(here, 'inventory_pc_alias.txt'))
    print()
    main('E:/legoj/Documents/Need for Speed ProStreet/100% Gamesave (OPTIONAL) - Place This in SAVE folder below/NFS Prostreet/CAREER_01/CAREER_01',
         'PC CAREER (CAREER_01)',
         os.path.join(here, 'inventory_pc_career.txt'))
