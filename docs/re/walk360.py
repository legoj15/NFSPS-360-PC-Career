#!/usr/bin/env python3
"""Empirical walker for NFS ProStreet (2007) Xbox 360 MC02 save trees.

MC02 container (360 = big-endian, PC = little-endian, otherwise identical):
  [0x00 'MC02'/'20CM'][0x04 total][0x08 extra_size][0x0C tree_size]
  [0x10 CRC(extra)][0x14 CRC(tree)][0x18 CRC(bytes 0..0x17)]
  [extra blob][tree]
  total == 0x18 + extra_size + tree_size + 4  (4 trailing bytes after the tree)

CRC (verified byte-exact on all four sample files):
  MSB-first table, poly 0x04C11DB7, seeded with the first 4 data bytes:
  crc = BE32(data[0:4]) ^ 0xFFFFFFFF
  for b in data[4:]: crc = ((crc << 8) | b) ^ TBL[crc >> 24]
  result = ~crc

Tree (360):
  [16B noise/nonce][u32 count][zero pad][u32 0x59F2D89B][u32 used_size]
  records start at tree+0x48.

Record grammar (verified: 360 alias tiles exactly, 14 records, to tree 0x48+used):
  [u32 type][u32 id][u32 size][payload of `size` bytes]
  type: 0x0131vvvv = property-node chunk, 0x00004000, or small ints/flags
        (career rec2/rec3 carry high-entropy type words - timestamps/flags)
  Records are contiguous; NO trailers between them (the "seam extras" seen
  by naive 8-byte-header walks are the type words of the next record).

Container records: payload begins [u32 1][u32 0] (count=1), followed by a
nested record [u32 type][u32 id][u32 size][payload] that ends exactly at the
container payload end.  Verified on 360 career rec3 (nested 0x1FB48CF2) and
rec5 (nested 0x34B74942).

Post-records region (alias, verified): at tree 0x48+used begins a directory
table: [u32 0] then 475 cells of [u32 h1][u32 h2][u32 0xFFFFFFFF][u32 0].
The hashes do NOT reference record ids (verified).  Career post-records
region: high-entropy hash-like blob ending in an 0x??FFFFFF tag, a zero run,
then 16-byte stat cells ([u32 a][u32 b] pairs like 0x1F841F84/0) to buffer end.

Career caveat (unresolved): count=9.  Parseable: 7 top-level + 2 nested = 9
chunk objects, ending at tree 0xAA6CC.  used_size however decodes to
tree 0x48+used = 0xAADEC, 0x720 past the last parseable record; the gap is
high-entropy with no valid [type][id][size] headers at any 4-byte alignment
(BFS-verified unreachable).  Either the last chunk(s) are stored
encrypted/obfuscated wholesale, or used_size has extra semantics on career.
"""
import struct
import sys

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
    if len(data) < 4:
        return 0
    crc = struct.unpack('>I', data[:4])[0] ^ 0xFFFFFFFF
    for b in data[4:]:
        crc = (((crc << 8) | b) & 0xFFFFFFFF) ^ TBL[(crc >> 24) & 0xFF]
    return crc ^ 0xFFFFFFFF


MAGIC_TREE = 0x59F2D89B
REC_START = 0x48


def parse_tree_header(tree):
    count = struct.unpack('>I', tree[16:20])[0]
    for i in range(0x14, min(len(tree), 0x48), 4):
        if struct.unpack('>I', tree[i:i + 4])[0] == MAGIC_TREE:
            used = struct.unpack('>I', tree[i + 4:i + 8])[0]
            return count, i + 8, used
    raise ValueError('tree magic not found')


def looks_container(payload):
    """[u32 0x01000000] prefix (1 child), then a nested record that fits exactly."""
    if len(payload) < 4 + 12:
        return False
    if struct.unpack('>I', payload[0:4])[0] != 0x01000000:
        return False
    t, i, s = struct.unpack('>III', payload[4:16])
    if s == 0 and i == 0 and t == 0:
        return False
    return 4 + 12 + s <= len(payload) <= 4 + 12 + s + 0x10


def walk_records(tree, start, end, depth=0):
    """Walk [type][id][size] records in [start,end).  Returns record list."""
    recs = []
    off = start
    while off + 12 <= end:
        t, i, s = struct.unpack('>III', tree[off:off + 12])
        if s == 0 or off + 12 + s > end:
            break
        payload = tree[off + 12:off + 12 + s]
        rec = dict(off=off, type=t, id=i, size=s, payload_off=off + 12,
                   end=off + 12 + s, depth=depth, children=[])
        if looks_container(payload):
            child = walk_records(tree, off + 12 + 4, off + 12 + s, depth + 1)  # skip 0x01000000
            if child and off + 12 + s - 0x10 <= child[-1]['end'] <= off + 12 + s:
                rec['children'] = child
                rec['container_trailer'] = off + 12 + s - child[-1]['end']
        recs.append(rec)
        off = rec['end']
    return recs


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


def dump_record(tree, rec, lines, tree_base_file, idxpath):
    ind = '  ' * rec['depth']
    fo = tree_base_file + rec['off']
    lines.append(f'{ind}[{idxpath}] tree_off=0x{rec["off"]:06X} file_off=0x{fo:06X} '
                 f'type=0x{rec["type"]:08X} id=0x{rec["id"]:08X} size=0x{rec["size"]:X} '
                 f'end=0x{rec["end"]:06X}')
    p = tree[rec['payload_off']:rec['end']]
    lines.append(f'{ind}  head: {p[:32].hex()}')
    ss = ascii_strings(p, 5)[:10]
    if ss:
        lines.append(f'{ind}  strings: ' + ' | '.join(f'@{o}:{t}' for o, t in ss))
    for k, ch in enumerate(rec['children']):
        dump_record(tree, ch, lines, tree_base_file, f'{idxpath}.{k}')


def main(path, payload_off, label, outpath):
    raw = open(path, 'rb').read()
    d = raw[payload_off:payload_off + 0x100000]
    total, extra, tsize = struct.unpack('>III', d[4:16])
    d = raw[payload_off:payload_off + total]
    crc_e, crc_t, crc_h = struct.unpack('>III', d[0x10:0x1C])
    tree_off_in_file = payload_off + 0x1C + extra
    tree = d[0x1C + extra:0x1C + extra + tsize]

    L = []
    L.append(f'=== {label} (Xbox 360, big-endian) ===')
    L.append(f'file: {path}')
    L.append(f'payload at file 0x{payload_off:X}, total=0x{total:X} extra=0x{extra:X} tree=0x{tsize:X}')
    ok_e = crc32_msb(d[0x1C:0x1C + extra]) == crc_e
    ok_t = crc32_msb(tree) == crc_t
    ok_h = crc32_msb(d[0:0x18]) == crc_h
    L.append(f'CRC hdr:   0x{crc_h:08X} {"OK" if ok_h else "FAIL"}')
    L.append(f'CRC extra: 0x{crc_e:08X} {"OK" if ok_e else "FAIL"}')
    L.append(f'CRC tree:  0x{crc_t:08X} {"OK" if ok_t else "FAIL"}')
    L.append(f'extra blob: {d[0x1C:0x1C+extra].hex()}')

    count, start, used = parse_tree_header(tree)
    recs = walk_records(tree, REC_START, len(tree))
    n_top = len(recs)
    n_nested = sum(len(r['children']) for r in recs) + \
        sum(len(r['children'][k2]['children']) for r in recs for k2 in range(len(r['children'])))
    last_end = recs[-1]['end'] if recs else 0
    used_end = REC_START + used
    L.append(f'tree count field = {count}; walked {n_top} top-level + {n_nested} nested = {n_top + n_nested} chunk objects')
    L.append(f'records start tree 0x{REC_START:X} (file 0x{tree_off_in_file + REC_START:X}); used_size=0x{used:X} '
             f'-> used end tree 0x{used_end:X}; last parsed record ends 0x{last_end:X}; buffer end 0x{len(tree):X}')
    L.append(f'TILING: {"EXACT - records end exactly at 0x48+used" if last_end == used_end else "PARTIAL - see caveat"}')
    L.append('')
    L.append('idx  tree_off  file_off  type       id         size     end')
    for n, r in enumerate(recs):
        L.append(f'{n:<3}  0x{r["off"]:06X}  0x{tree_off_in_file + r["off"]:06X}  0x{r["type"]:08X}  '
                 f'0x{r["id"]:08X}  0x{r["size"]:06X}  0x{r["end"]:06X}' + ('  [container]' if r['children'] else ''))
    L.append('')
    L.append('--- record details ---')
    for n, r in enumerate(recs):
        dump_record(tree, r, L, tree_off_in_file, str(n))

    # post-records region analysis
    L.append('')
    L.append('--- post-records region ---')
    post = tree[last_end:]
    L.append(f'length 0x{len(post):X} (tree 0x{last_end:X}..0x{len(tree):X})')
    # count 16-byte directory cells [h1][h2][FFFFFFFF][0]
    cells = 0
    off = last_end
    if off % 4 == 0:
        while off + 16 <= len(tree) and tree[off + 8:off + 12] == b'\xff\xff\xff\xff' \
                and tree[off + 12:off + 16] == b'\x00\x00\x00\x00':
            cells += 1
            off += 16
    if cells:
        L.append(f'directory table: {cells} cells of [h1][h2][0xFFFFFFFF][0] '
                 f'(tree 0x{last_end:X}..0x{last_end + cells * 16:X})')
    text = '\n'.join(L)
    open(outpath, 'w').write(text)
    print(text)
    return recs


if __name__ == '__main__':
    import os
    here = os.path.dirname(os.path.abspath(__file__))
    root = os.path.dirname(os.path.dirname(here))  # repo root
    main(os.path.join(root, 'Extracted', 'Alias', 'ALIAS_360'),
         0xD000, '360 ALIAS (author)',
         os.path.join(here, 'inventory_360_alias.txt'))
    print()
    main(os.path.join(root, 'Extracted', 'Career', 'CAREER_01'),
         0xD000, '360 CAREER (CAREER_01)',
         os.path.join(here, 'inventory_360_career.txt'))
