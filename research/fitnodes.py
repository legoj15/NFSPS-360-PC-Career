import struct

tb = open("tree_alias.bin","rb").read()

def u32(off): return struct.unpack_from('<I', tb, off)[0]

# Region 1: GStats payload area 0x1dc..0x774 (44 float stats + 1 count node)
# Region 2: 0x1e4..0x3e8 striated (ProfileStats? no - GStats is 0x1dc..0x774)
# Try: start 0x1e4, rule 'raw' next = slot+0xc+size; check flags byte==0 and sane sizes
def walk(start, end, next_rule, payload_rule, maxn=100):
    s = start
    nodes = []
    for _ in range(maxn):
        if s >= end: break
        name, size, flags = u32(s), u32(s+4), tb[s+8]
        if flags != 0:
            return None, f"flags={flags} at {s:#x}"
        p = payload_rule(s)
        if p is None or p + max(0,(s+0xc+size)-p) > end:
            return nodes, f"oob at {s:#x}"
        nodes.append((s, name, size, p, s+0xc+size))
        s = next_rule(s, size)
    return nodes, "max"

def raw_next(s, size): return s + 0xc + size
def align_next(s, size): return (s + 0xc + size + 0xF) & ~0xF
def p_raw(s): return s + 0xc
def p_align(s): return (s + 0xc + 0xF) & ~0xF

for label, nxt, pay in [("raw/raw", raw_next, p_raw), ("align/raw", align_next, p_raw),
                        ("raw/align", raw_next, p_align), ("align/align", align_next, p_align)]:
    nodes, why = walk(0x1e4, 0x774, nxt, pay)
    if nodes is None:
        print(f"{label}: FAIL ({why})")
    else:
        print(f"{label}: {len(nodes)} nodes, stop: {why}")
        for n in nodes[:8]:
            print(f"    slot {n[0]:#x} name {n[1]:#010x} size {n[2]:#x} payload {n[3]:#x} val {u32(n[3]):#010x} float {struct.unpack('<f', tb[n[3]:n[3]+4])[0]:.3f}")
