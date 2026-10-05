#!/usr/bin/env python3
r"""
parse360.py -- Need for Speed ProStreet (Xbox 360) save "chunk tree" parser.

Derives the on-disk grammar of the MC02 save file (career / alias) from the
static PPC recompilation (E:/GitHub/NFSPS360) plus byte-level comparison
against a recomp-written twin of the same save session.  Everything below was
verified against four real files:

    console : Extracted/Career/CAREER_01            (career, tree 0xB6800)
    console : Extracted/Alias/ALIAS_JOSHUA S 10     (alias,   tree 0x5000)
    recomp  : NFSPS360/user_data/.../CAREER_01      (same session re-saved)
    recomp  : NFSPS360/user_data/.../ALIAS_JOSHUA S 10

===========================================================================
 GRAMMAR (all big-endian)
===========================================================================
Container
    bytes 0..0xD000   Xbox 360 console container header (STFS) -- strip it.
    The MC02 file follows at 0xD000.

MC02 file
    +0x00 u32  magic 0x4D433032 ('MC02')           \ built by sub_827BCDB8
    +0x04 u32  total_size = 28 + extra_size + tree_size
    +0x08 u32  extra_size (28 career / 64 alias)
    +0x0C u32  tree_size
    +0x10 u32  crc(extra)      } EA-CRC (sub_827C75B8): MSB-first,
    +0x14 u32  crc(tree)       } poly 0x04C11DB7, seed = BE32 of first
    +0x18 u32  crc(header[0:0x18])  } 4 bytes, final NOT -- see crc32ea()
    then extra blob (extra_size bytes), then the TREE (tree_size bytes).
    NOTE: crc(tree) covers the FULL tree buffer (tree_size), slack included.

extra blob (observed; fields beyond +0x10 partly unresolved)
    +0x00 u32  0x8209E9xx  (per-file, meaning unknown)
    +0x04 u32  == tree.used (bytes of records region)
    +0x08 u32  1
    +0x0C u32  7
    +0x10 u32  0
    +0x14 ..   alias only: profile name, NUL-terminated, 0xAA padded
    tail       career [.. 0x0064420D C9 D6], alias [0x4248][0x41C8] (unknown)

TREE buffer (this is the "chunk tree")
    +0x00 16B  junk (uninitialized heap; not parsed, but CRC-covered)
    +0x10 u32  count  = number of record slots that follow
    +0x14 44B  junk/pad (zeros on console, 0xAA on recomp)
    +0x40 u32  0x59F2D89B  root record id (constant across every file; not a
                literal in code nor in .data -- provenance unresolved)
    +0x44 u32  used  = bytes of the records region (0x48 .. 0x48+used)
    +0x48      count x record slots, back to back:
        +0x00 u32  junk (uninitialized 4 bytes; NOT a type/version field --
                    console files carry stale bytes here, recomp files 0xAA)
        +0x04 u32  id    (per-record id, stable per logical chunk type)
        +0x08 u32  size
        +0x0C       payload (size bytes)
    The record walk must therefore advance by 12 + size per slot.  The last
    payload ends exactly at 0x48 + used.  Bytes beyond (up to tree_size) are
    buffer slack (still covered by crc(tree)).

record payload
    +0x00 u8   0x01 (payload kind marker, always 1 in these files)
    +0x01 7-11B junk (uninitialized; the entry stream starts at +8 or +12,
             resolved per record by the tiler below)
    then a stream of entries (boundaries recovered by exact tiling):
      prop : [u32 0][u32 type 0..0x18][u32 tag24] + schema-sized data
             (type 4 = u32; type 8 carries a float + optional inline name,
              e.g. "ICON_SPEED"; tag = 24-bit attribute-dictionary handle)
      str  : [u32 capacity >= 0x19][u32 tag24][u32 0] + padded string slot
             (NUL-terminated ASCII right-padded to the schema slot size)
      blob : [u32 id][u32 size >= 0x19] + size bytes of opaque struct
             (single-blob records carry size == payload_size - 0x10)
    The game's attribute schema (not present in the file) drives per-entry
    data sizes; the tiler recovers boundaries by requiring an exact fill.

===========================================================================
 VERIFICATION STATUS (see inventory_360.txt)
===========================================================================
 alias  : 14/14 records parse, nodes tile 100% of every payload, all three
          CRCs verify.
 career : 6/9 record slots parse 100%.  Slot 7 (id 0x885b4ddc) tiles only its
          first 0x1898/0x1ED4 payload bytes and slots 8-9 are unreadable:
          from tree offset 0xAA6CC to 0xB67FB the buffer is uninitialized
          noise.  Corroborating damage evidence: crc(tree) over the full
          buffer does NOT verify on the console file (while header/extra
          CRCs do), but the recomp-written twin of the same session stores
          valid records there (ids 0xD548266C, 0xCA269650), verifies all
          CRCs, and parses 9/9 -- the grammar holds; the console file's
          tail region was modified after its CRC was computed.
"""

import struct
import sys
import os

# --------------------------------------------------------------------------
# EA CRC-32 (sub_827C75B8 / incremental variant sub_827C7640)
# --------------------------------------------------------------------------

def _make_table():
    tbl = []
    for i in range(256):
        c = i << 24
        for _ in range(8):
            c = ((c << 1) ^ 0x04C11DB7) & 0xFFFFFFFF if c & 0x80000000 else (c << 1) & 0xFFFFFFFF
        tbl.append(c)
    return tbl

_CRC_TABLE = _make_table()

def crc32ea(data, seed=None):
    """MSB-first table CRC.  seed=None reproduces sub_827C75B8 exactly:
    crc = ~BE32(first 4 bytes) as the running value, per byte
    crc = ((crc<<8)|b) ^ table[crc>>24], return ~crc."""
    if seed is None:
        if len(data) < 4:
            return 0
        seed = struct.unpack('>I', data[:4])[0]
        data = data[4:]
    c = (~seed) & 0xFFFFFFFF
    for b in data:
        c = (((c << 8) & 0xFFFFFFFF) | b) ^ _CRC_TABLE[(c >> 24) & 0xFF]
    return (~c) & 0xFFFFFFFF

# --------------------------------------------------------------------------
# Container / MC02 file
# --------------------------------------------------------------------------

CONTAINER_SIZE = 0xD000
ROOT_ID = 0x59F2D89B

class SaveFile:
    def __init__(self, path):
        self.path = path
        raw = open(path, 'rb').read()
        off = raw.find(b'MC02', 0)
        if off < 0:
            raise ValueError('no MC02 magic found in %s' % path)
        self.container = raw[:off]
        m = raw[off:]
        (self.magic, self.total_size, self.extra_size, self.tree_size,
         self.crc_extra, self.crc_tree, self.crc_header) = struct.unpack('>7I', m[:28])
        self.extra = m[28:28 + self.extra_size]
        self.tree = m[28 + self.extra_size:28 + self.extra_size + self.tree_size]
        self.header_crc_ok = crc32ea(m[:24]) == self.crc_header
        self.extra_crc_ok = crc32ea(self.extra) == self.crc_extra
        self.tree_crc_ok = crc32ea(self.tree) == self.crc_tree
        # tree prefix
        self.prefix_junk = self.tree[0:0x10]
        self.count = struct.unpack('>I', self.tree[0x10:0x14])[0]
        self.pad = self.tree[0x14:0x40]
        self.root_id, self.used = struct.unpack('>II', self.tree[0x40:0x48])

# --------------------------------------------------------------------------
# Record walk
# --------------------------------------------------------------------------

class Record:
    __slots__ = ('slot_off', 'junk', 'id', 'size', 'payload')

def walk_records(tree, count, used):
    """Records start at 0x48; each slot is 4 junk bytes + id + size + payload.
    Returns (records, notes).  Stops early (with a note) if a slot header is
    garbage so the caller can report unreadable bytes."""
    recs = []
    notes = []
    pos = 0x48
    end = 0x48 + used
    n = 0
    while n < count and pos + 12 <= end:
        junk, rid, size = struct.unpack('>III', tree[pos:pos + 12])
        if pos + 12 + size > end or size > used:
            notes.append('slot %d @0x%x has garbage header (id=%08x size=%08x); '
                         '%d unreadable bytes remain inside used region'
                         % (n + 1, pos, rid, size, end - pos))
            break
        r = Record()
        r.slot_off = pos
        r.junk = junk
        r.id = rid
        r.size = size
        r.payload = tree[pos + 12:pos + 12 + size]
        recs.append(r)
        pos += 12 + size
        n += 1
    if pos < end and n == count:
        notes.append('walk ended at 0x%x but used region ends at 0x%x' % (pos, end))
    return recs, notes, pos, end

# --------------------------------------------------------------------------
# Payload tiler (DP): recovers entry boundaries so every byte is accounted.
#
# Entry shapes (empirically derived; boundaries proven by exact tiling of all
# four reference files, and by written-byte analysis of the recomp twins
# where 0xAA marks bytes the game never writes):
#   prop : [u32 A==0][u32 B 0..0x18][u32 tag] + D bytes      (12-byte header)
#          B is a small type code (4 = int32 ...), tag a 24-bit dictionary
#          handle, D driven by the attribute schema (observed 4..20).
#          B==0 entries are "null" slots (D==0).  The FINAL entry of a
#          payload may be a bare 12-byte header with no data (terminator).
#   str  : [u32 A >= 0x19][u32 tag][u32 0] + D bytes          (12-byte header)
#          A is the string slot capacity; D is the padded slot size
#          (observed 20 for 21-char GUIDs, 36 for race-day names).
#   blob : [u32 id][u32 B >= 0x19] + B bytes                   (8-byte header)
#          opaque serialized struct; the big career records 0x3b309e09 /
#          0x47a07113 are single blobs whose B equals payload_size - 0x10.
# --------------------------------------------------------------------------

PROP_REST = (4, 8, 12, 16, 20, 24, 28, 32, 36, 40, 44, 48)
STR_D = (12, 16, 20, 24, 28, 32, 36, 40, 44, 48)

def _prop_order(B):
    # type 8 entries overwhelmingly carry 20 (or 16) bytes of data in these
    # files; trying those first reproduces the intended segmentation.
    if B == 8:
        return (20, 16, 8, 12, 4) + PROP_REST
    return PROP_REST

class Entry:
    __slots__ = ('off', 'A', 'B', 'tag', 'dsize', 'kind')

def _tile(buf, start, end):
    """Exact tiling of buf[start:end] with the entry grammar.  Returns
    (entries, good) where entries is None if no tiling exists."""
    good = [False] * (end + 1)
    good[end] = True
    reach = [None] * (end + 1)
    for k in range(end - 1, start - 1, -1):
        if k + 8 > end:
            continue
        A, B = struct.unpack('>II', buf[k:k + 8])
        left = end - k
        # 1. prop (strongest: A==0, small type code)
        if A == 0 and 0 <= B <= 0x18 and k % 4 == 0:
            if k + 12 == end:                      # bare terminator header
                good[k] = True
                reach[k] = ('prop', 0, end)
                continue
            if B == 0 and k + 12 < end and good[k + 12]:
                good[k] = True
                reach[k] = ('prop', 0, k + 12)
                continue
            if k + 12 <= end:
                for d in _prop_order(B):
                    nxt = k + 12 + d
                    if nxt <= end and good[nxt]:
                        good[k] = True
                        reach[k] = ('prop', d, nxt)
                        break
                if good[k]:
                    continue
        # 2. blob (strong: B is an exact byte size)
        if B >= 0x19 and B + 8 <= left and good[k + 8 + B]:
            good[k] = True
            reach[k] = ('blob', B, k + 8 + B)
            continue
        # 3. string slot (weaker: keyed on the capacity word; third word 0)
        if A >= 0x19 and k % 4 == 0 and k + 12 <= end:
            Z = struct.unpack('>I', buf[k + 8:k + 12])[0]
            if Z == 0:
                for d in STR_D:
                    nxt = k + 12 + d
                    if nxt <= end and good[nxt]:
                        good[k] = True
                        reach[k] = ('str', d, nxt)
                        break
                if good[k]:
                    continue
        # 4. short trailing footer (<12 B) at the very end of the payload
        if 0 < end - k <= 12 and k > start:
            good[k] = True
            reach[k] = ('tail', end - k, end)
    if not good[start]:
        return None, good
    out = []
    k = start
    while k != end and reach[k]:
        kind, d, nxt = reach[k]
        A, B = struct.unpack('>II', buf[k:k + 8])
        e = Entry()
        e.off = k
        e.A = A
        e.B = B
        e.tag = struct.unpack('>I', buf[k + 8:k + 12])[0] if kind not in ('blob', 'tail') else None
        e.dsize = d
        e.kind = kind
        out.append(e)
        k = nxt
    if k != end:
        return None, good
    return out, good

def tile_payload(rec):
    """Try node-stream prefixes of 12 then 8 (both observed), then other
    small offsets.  Returns (entries, prefix, good) or (None, None, good)."""
    best_good = None
    for prefix in (12, 8, 4, 16, 0, 20):
        entries, good = _tile(rec.payload, prefix, len(rec.payload))
        if entries is not None:
            return entries, prefix, good
        if good is not None:
            best_good = good
    return None, None, best_good

def deepest_prefix(rec):
    """For partially-readable payloads: longest span [prefix..x) that tiles."""
    best = (0, None)
    for prefix in (8, 12):
        _, good = _tile(rec.payload, prefix, len(rec.payload))
        if good is None:
            continue
        reach = max((k for k in range(prefix, len(rec.payload) + 1) if good[k]),
                    default=prefix)
        if reach - prefix > best[0]:
            best = (reach - prefix, prefix)
    return best  # (span_len, prefix)

# --------------------------------------------------------------------------
# Pretty helpers
# --------------------------------------------------------------------------

def printable(bs):
    return ''.join(chr(c) if 32 <= c < 127 else '.' for c in bs)

def entry_summary(rec, e):
    data = rec.payload[e.off:e.off + (8 if e.kind == 'blob' else 12) + e.dsize]
    if e.kind == 'prop' and e.B == 4 and e.dsize == 4:
        val = struct.unpack('>I', rec.payload[e.off + 12:e.off + 16])[0]
        return 'int %u (0x%x)' % (val, val)
    if e.kind == 'prop' and e.B == 8 and e.dsize >= 16:
        f = struct.unpack('>f', rec.payload[e.off + 16:e.off + 20])[0] \
            if e.dsize >= 20 else 0.0
        s = rec.payload[e.off + 20:e.off + 12 + e.dsize]
        txt = s.split(b'\0')[0]
        if txt and all(32 <= c < 127 for c in txt):
            return 'float %g, name %r' % (f, txt.decode('ascii', 'replace'))
        return 'float %g' % f
    if e.kind in ('str', 'blob'):
        s = data[12:]
        txt = s.split(b'\0')[0]
        if len(txt) >= 3 and all(32 <= c < 127 for c in txt):
            return 'string %d B (%r...)' % (e.dsize, txt[:32].decode('ascii', 'replace'))
        return '%d B' % e.dsize
    return '%d B' % e.dsize

# --------------------------------------------------------------------------
# Inventory dump
# --------------------------------------------------------------------------

def dump_save(sf, out):
    w = out.write
    w('=' * 78 + '\n')
    w('SAVE: %s\n' % sf.path)
    w('=' * 78 + '\n')
    w('container header : %d bytes (stripped)\n' % len(sf.container))
    w('MC02 header      : magic=%08x total=0x%x extra=0x%x tree=0x%x\n'
      % (sf.magic, sf.total_size, sf.extra_size, sf.tree_size))
    w('CRC header[0:24] : %08x %s\n' % (sf.crc_header, 'OK' if sf.header_crc_ok else 'FAIL'))
    w('CRC extra        : %08x %s\n' % (sf.crc_extra, 'OK' if sf.extra_crc_ok else 'FAIL'))
    w('CRC tree (full)  : %08x %s\n' % (sf.crc_tree, 'OK' if sf.tree_crc_ok else 'FAIL'))
    w('extra blob       : %s\n' % sf.extra.hex())
    w('\n')
    w('tree prefix      : junk16=%s\n' % sf.prefix_junk.hex())
    w('                   count=%d  pad44=%s\n' % (sf.count, sf.pad.hex()))
    w('                   root id=%08x used=0x%x (records end at 0x%x)\n'
      % (sf.root_id, sf.used, 0x48 + sf.used))
    w('tree slack       : 0x%x bytes beyond used (CRC-covered)\n'
      % (len(sf.tree) - 0x48 - sf.used))
    w('\n')

    recs, notes, pos, end = walk_records(sf.tree, sf.count, sf.used)
    total_nodes = 0
    tiled_records = 0
    for n, r in enumerate(recs):
        entries, prefix, _ = tile_payload(r)
        w('-' * 78 + '\n')
        w('record %d  slot@0x%06x  junk=%08x  id=%08x  size=0x%x (%d B)\n'
          % (n + 1, r.slot_off, r.junk, r.id, r.size, r.size))
        if entries is None:
            span, pfx = deepest_prefix(r)
            w('  payload: NOT fully tileable (damaged/opaque beyond +0x%x of 0x%x)\n'
              % (span, len(r.payload)))
            continue
        tiled_records += 1
        total_nodes += len(entries)
        w('  payload: marker=0x%02x prefix=%d junk bytes, then %d entries:\n'
          % (r.payload[0], prefix, len(entries)))
        # summarize by kind
        kinds = {}
        for e in entries:
            kinds[e.kind] = kinds.get(e.kind, 0) + 1
        w('    kinds: %s\n' % ', '.join('%s x%d' % kv for kv in sorted(kinds.items())))
        shown = 0
        for e in entries:
            if shown >= 12 and len(entries) > 16:
                w('    ... %d more entries ...\n' % (len(entries) - shown))
                break
            if e.kind == 'blob':
                w('    +0x%04x blob    id=%08x size=%6d %s\n'
                  % (e.off, e.A, e.B, entry_summary(r, e)))
            elif e.kind == 'tail':
                w('    +0x%04x tail    %d B footer\n' % (e.off, e.dsize))
            elif e.kind == 'str':
                w('    +0x%04x strslot cap=%-6d tag=%06x %s\n'
                  % (e.off, e.A, e.B & 0xFFFFFF, entry_summary(r, e)))
            else:
                w('    +0x%04x prop    type=%-2d         tag=%06x %s\n'
                  % (e.off, e.B, e.tag & 0xFFFFFF, entry_summary(r, e)))
            shown += 1
    w('-' * 78 + '\n')
    for note in notes:
        w('NOTE: %s\n' % note)
    if pos < end:
        w('unreadable slot region: tree[0x%x:0x%x] (%d B) -- contents:\n'
          % (pos, end, end - pos))
        w('  %s...\n' % sf.tree[pos:pos + 48].hex())
    w('\nSUMMARY: %d/%d record slots parsed, %d entries total%s\n\n'
      % (tiled_records, sf.count, total_nodes,
         '' if not notes else ' (see NOTEs)'))
    return tiled_records, sf.count

def main():
    here = os.path.dirname(os.path.abspath(__file__))
    base = os.path.dirname(os.path.dirname(here))  # repo root
    targets = [
        os.path.join(base, 'Extracted', 'Career', 'CAREER_01'),
        os.path.join(base, 'Extracted', 'Alias', 'ALIAS_JOSHUA S 10'),
    ]
    # corroboration: the recomp-written twins of the same save sessions
    for extra in ('E:/GitHub/NFSPS360/user_data/B13EBABEBABEBABE/45410822/00000001/CAREER_01/CAREER_01',
                  'E:/GitHub/NFSPS360/user_data/B13EBABEBABEBABE/45410822/00000001/ALIAS_JOSHUA S 10/ALIAS_JOSHUA S 10'):
        if os.path.exists(extra):
            targets.append(extra)
    out_path = os.path.join(here, 'inventory_360.txt')
    with open(out_path, 'w', encoding='utf-8') as out:
        for t in targets:
            sf = SaveFile(t)
            dump_save(sf, out)
    print('wrote %s' % out_path)
    # concise console echo
    for t in targets:
        sf = SaveFile(t)
        recs, notes, pos, end = walk_records(sf.tree, sf.count, sf.used)
        ok = 0
        for r in recs:
            entries, _, _ = tile_payload(r)
            if entries is not None:
                ok += 1
        print('%-40s CRC %s/%s/%s  count=%d parsed=%d%s'
              % (os.path.basename(t),
                 'OK' if sf.header_crc_ok else 'FAIL',
                 'OK' if sf.extra_crc_ok else 'FAIL',
                 'OK' if sf.tree_crc_ok else 'FAIL',
                 sf.count, ok,
                 '' if not notes else '  (tail damaged)'))

if __name__ == '__main__':
    main()
