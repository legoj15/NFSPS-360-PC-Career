"""
parsepc.py - Need for Speed ProStreet (2007) PC save chunk-tree parser.
Grammar derived from static analysis of nfs.exe. Python 3.

==================================================================== MC02 FILE
  +0x00 u32  magic 0x4D433032 (bytes "20CM" LE)
  +0x04 u32  total file size
  +0x08 u32  extra_size (28 career / 64 alias)
  +0x0C u32  tree_size  (0x5000 alias / 0xB6800 career; fixed device buffer)
  +0x10 u32  CRC32(extra)     } MSB-first CRC-32 (poly 0x04C11DB7, table at
  +0x14 u32  CRC32(tree)      } VA 0xA8DC00): init = ~BE32(buf[0:4]), feed
  +0x18 u32  CRC32(hdr[0:18]) } bytes buf[4:], final ~.  fn VA 0x8A18F6,
                               } wrapper VA 0x89F205
  +0x1C      extra blob (game defined)
  then       tree buffer (tree_size bytes)

==================================================================== TREE BUFFER
  +0x000 16B  hash of tree[0x10:tree_size] (128-bit hash fn VA 0x6D9CE0;
              verified on load by fn VA 0x5AABD0)
  +0x010 u32  count of registered top-level savables
  +0x014..0x3F  stale allocator bytes (never read)
  +0x040 21B   machine GUID (runtime global 0xACDC6C <- HKLM RegQueryValueEx)
  +0x055..0x1BF stale allocator bytes
  +0x1C0 ROOT RECORD id=djb2("MEMCARD_ROOT")=0x59F2D89B;
              root.end (0x3A00 alias / 0xA69A0 career) = used size of tree;
              beyond it, untouched allocator fill.

==================================================================== ELEMENT HEADER (records & value nodes share it)
  S+0x00 u32  id    = djb2(chunk name), hash fn VA 0x436680 (h=-1; h=h*33+c)
  S+0x04 u32  size  = bytes from S+0x0C to element end (payload incl. pads)
  S+0x08 u8   flags = 1 record (child chunks) / 0 value node
  S+0x09..0x0B   uninitialized (allocator fill bleeds through)
  end = S + 0x0C + size

  TOP-LEVEL records under MEMCARD_ROOT chain RAW from ROOT+0x0C:
      next = slot + 0x0C + size     (serializer loop VA 0x5AACC0/0x5AAD35)
  NESTED element chains (inside a record) start at align16up(S+0x0C) and
  advance with align16up(end) (writer cursor rule, e.g. VA 0x59CD30/CD80/
  CDD0; read-side advance VA 0x5AABA0). Value nodes: ints land at S+0x0C,
  SSE data (floats/buffers) at align16up(S+0x0C). Some writers leave size
  words unwritten (word shows the zone fill, e.g. 0x10101000).

Chunk names recovered by djb2 brute-force over every ASCII string in nfs.exe.
"""
import struct, re, os

MAGIC = 0x4D433032
ROOT_SLOT = 0x1C0

def djb2(s):
    h = 0xFFFFFFFF
    for c in s.encode():
        h = (h * 33 + c) & 0xFFFFFFFF
    return h

def load_name_table(exe_path):
    table = {djb2('MEMCARD_ROOT'): 'MEMCARD_ROOT'}
    if exe_path and os.path.exists(exe_path):
        blob = open(exe_path, 'rb').read()
        for m in re.finditer(rb'[\x20-\x7e]{4,80}', blob):
            s = m.group().decode('ascii')
            for cand in (s, s.upper(), s.lower()):
                table.setdefault(djb2(cand), cand)
    return table

_TBL = None
def crc_msb(buf):
    global _TBL
    if _TBL is None:
        t = []
        for i in range(256):
            c = i << 24
            for _ in range(8):
                c = ((c << 1) ^ 0x04C11DB7) & 0xFFFFFFFF if c & 0x80000000 else (c << 1) & 0xFFFFFFFF
            t.append(c)
        _TBL = t
    c = (buf[0] << 24) | (buf[1] << 16) | (buf[2] << 8) | buf[3]
    c ^= 0xFFFFFFFF
    for b in buf[4:]:
        c = (((c << 8) & 0xFFFFFFFF) | b) ^ _TBL[(c >> 24) & 0xFF]
    return c ^ 0xFFFFFFFF

def align16(x): return (x + 0xF) & ~0xF

class El:
    __slots__ = ('slot','id','size','flags','name','kids','kind','ival','fval','note')
    def __init__(self, slot, eid, size, flags):
        self.slot, self.id, self.size, self.flags = slot, eid, size, flags
        self.name = None; self.kids = None; self.kind = '?'
        self.ival = None; self.fval = None; self.note = ''
    @property
    def end(self): return self.slot + 0xC + self.size

def read_el(tree, slot):
    eid, size = struct.unpack_from('<II', tree, slot)
    return El(slot, eid, size, tree[slot+8])

def _fill_like(w):  # unwritten size word (high bytes are zone fill)
    return (w & 0xFFFFFF00) in (0x10101000, 0xEEEEEE00, 0xBEBEBE00, 0xFEFEFE00)

def walk_top(tree, start, end, names):
    """RAW record chain (top level under MEMCARD_ROOT)."""
    out = []
    s = start
    while s + 0xC <= end:
        el = read_el(tree, s)
        if el.flags != 1 or el.size > end - s - 0xC or el.size == 0:
            break
        el.name = names.get(el.id)
        el.kind = 'REC'
        el.kids, ok = walk_nested(tree, el, names, depth=1)
        if not ok: el.note = 'opaque payload'
        out.append(el)
        s = el.end
    return out, s

def walk_nested(tree, rec, names, depth=0):
    """Parse a record's payload as an aligned element chain (records+nodes)."""
    if depth > 24:
        return None, False
    start = align16(rec.slot + 0xC)
    end = rec.end
    kids = []
    s = start
    while s + 0xC <= end:
        el = read_el(tree, s)
        if el.size > end - s - 0xC:
            return kids, False
        if _fill_like(el.size):
            return kids, False          # unwritten header -> not a stream
        el.name = names.get(el.id)
        if el.flags == 1 or (el.name is not None and el.size > 0x40):
            # flags==1 set by top-level serializer; nested child chunks often
            # keep flags=0, so also trust a known chunk-name id with real size
            el.kind = 'REC'
            el.kids, ok = walk_nested(tree, el, names, depth+1)
            if not ok: el.note = 'opaque payload'
        else:
            el.kind = 'NODE'
            p = s + 0xC
            el.ival = struct.unpack_from('<I', tree, p)[0] if p+4 <= end else None
            ap = align16(p)
            el.fval = struct.unpack_from('<f', tree, ap)[0] if ap+4 <= end else None
        kids.append(el)
        s = align16(el.end)
    return kids, (s >= end)

def parse_file(path):
    d = open(path, 'rb').read()
    magic, total, extra_size, tree_size, ce, ct, ch = struct.unpack_from('<7I', d, 0)
    assert magic == MAGIC, f'bad magic {magic:#x}'
    extra = d[0x1C:0x1C+extra_size]
    tree = d[0x1C+extra_size:0x1C+extra_size+tree_size]
    return dict(data=d, total=total, extra=extra, tree=tree, ce=ce, ct=ct, ch=ch)

def inventory(path, exe_path, out):
    names = load_name_table(exe_path)
    f = parse_file(path)
    tree = f['tree']
    root = read_el(tree, ROOT_SLOT)
    root.name = 'MEMCARD_ROOT'
    out.write(f"FILE: {path}\n")
    out.write(f"  total={f['total']} extra={len(f['extra'])} tree={len(tree):#x}\n")
    out.write(f"  CRC ok: hdr={crc_msb(f['data'][:0x18])==f['ch']} "
              f"extra={crc_msb(f['extra'])==f['ce']} tree={crc_msb(tree)==f['ct']}\n")
    out.write(f"  tree hash[0:16]={tree[0:0x10].hex()}\n")
    guid = tree[0x40:0x55]
    out.write(f"  count@0x10={struct.unpack_from('<I',tree,0x10)[0]} guid@0x40={guid.hex()} "
              f"('{''.join(chr(c) if 32<=c<127 else '.' for c in guid)}')\n")
    out.write(f"  extra={f['extra'].hex()}\n")
    out.write(f"  ROOT @{ROOT_SLOT:#x} size={root.size:#x} end={root.end:#x} "
              f"({root.end*100//len(tree)}% of buffer)\n")

    tops, consumed = walk_top(tree, ROOT_SLOT + 0xC, root.end, names)
    stats = {'rec':0,'node':0,'unknown_rec':0}
    def dump(el, depth):
        pad = '  ' * (depth + 1)
        if el.kind == 'REC':
            stats['rec'] += 1
            if el.name is None: stats['unknown_rec'] += 1
            out.write(f"{pad}REC  {el.name or '#'+format(el.id,'08x'):36s} sz={el.size:#08x} "
                      f"slot={el.slot:#07x} end={el.end:#07x} "
                      f"kids={len(el.kids) if el.kids is not None else '-'}"
                      f"{' '+el.note if el.note else ''}\n")
            if el.kids:
                for k in el.kids: dump(k, depth+1)
        else:
            stats['node'] += 1
            nm = el.name if el.name else ('(unnamed)' if el.id == 0 else '#'+format(el.id,'08x'))
            fs = f"{el.fval:.4g}" if el.fval is not None else '-'
            out.write(f"{pad}node {nm:36s} sz={el.size:#08x} slot={el.slot:#07x} "
                      f"i={el.ival if el.ival is not None else '-'} f={fs}\n")
    for t in tops:
        dump(t, 0)
    out.write(f"\n  SUMMARY: top-level={len(tops)} (hdr count={struct.unpack_from('<I',tree,0x10)[0]}) "
              f"records={stats['rec']} nodes={stats['node']} unknown-record-ids={stats['unknown_rec']} "
              f"tiled-to={consumed:#x} root-end={root.end:#x} "
              f"{'EXACT TILE' if consumed==root.end else 'MISMATCH'}\n\n")
    return stats, root, tops

if __name__ == '__main__':
    import os
    here = os.path.dirname(os.path.abspath(__file__))
    base = "E:/legoj/Documents/Need for Speed ProStreet/100% Gamesave (OPTIONAL) - Place This in SAVE folder below/NFS Prostreet/"
    exe = "E:/legoj/Documents/Need for Speed ProStreet/nfs.exe"
    out_path = os.path.join(here, 'inventory_pc.txt')
    with open(out_path, 'w', encoding='utf-8') as out:
        for name in ("ALIAS_PEIROKUNMANWSP/ALIAS_PEIROKUNMANWSP",
                     "CAREER_01/CAREER_01"):
            stats, root, tops = inventory(base + name, exe, out)
            print(f"{name.split('/')[0]}: tops={len(tops)} recs={stats['rec']} "
                  f"nodes={stats['node']} unknown-rec-ids={stats['unknown_rec']}")
    print("wrote", out_path)
