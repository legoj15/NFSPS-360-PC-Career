import struct
tb = open("tree_alias.bin","rb").read()
def u32(o): return struct.unpack_from('<I', tb, o)[0]

def scan_phase(start, end, phase, stride):
    """list (slot, name, size, flagsbyte, value) for slots at start+k*stride"""
    out = []
    s = start
    while s + 0x10 <= end:
        out.append((s, u32(s), u32(s+4), tb[s+8], u32(s+0xc)))
        s += stride
    return out

# GStats payload 0x1dc..0x774; try phase 0x1e4 stride 0x10
print("=== GStats stream, slots 0x1e4+0x10k ===")
for row in scan_phase(0x1e4, 0x2a4, 0x10, 0x10):
    print(f"  {row[0]:#06x}: name={row[1]:#010x} size={row[2]:#010x} flags={row[3]:#04x} val={row[4]:#010x}")

print("=== same but slots 0x1e0+0x10k ===")
for row in scan_phase(0x1e0, 0x2a0, 0x10, 0x10):
    print(f"  {row[0]:#06x}: name={row[1]:#010x} size={row[2]:#010x} flags={row[3]:#04x} val={row[4]:#010x}")
