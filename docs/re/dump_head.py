import os, struct, sys

def load(p):
    d = open(p,'rb').read()
    magic, total, extra, tree, crc_extra, crc_tree, crc_hdr = struct.unpack_from('<IIIIIII', d, 0)
    print(f"{p.split('/')[-1]}: len={len(d)} magic={magic:08x} total={total} extra={extra} tree={tree} crcE={crc_extra:08x} crcT={crc_tree:08x} crcH={crc_hdr:08x}")
    assert magic == 0x4D433032
    tb = d[0x18+extra : 0x18+extra+tree]
    assert len(tb) == tree
    eb = d[0x18:0x18+extra]
    return d, eb, tb

here = os.path.dirname(os.path.abspath(__file__))
base = "E:/legoj/Documents/Need for Speed ProStreet/100% Gamesave (OPTIONAL) - Place This in SAVE folder below/NFS Prostreet/"
for name in ["ALIAS_PEIROKUNMANWSP/ALIAS_PEIROKUNMANWSP", "CAREER_01/CAREER_01"]:
    d, eb, tb = load(base + name)
    open(os.path.join(here, "tree_") + ("alias" if "ALIAS" in name else "career") + ".bin","wb").write(tb)
    open(os.path.join(here, "extra_") + ("alias" if "ALIAS" in name else "career") + ".bin","wb").write(eb)
    print("  extra[:64]:", eb[:64].hex())
    print("  tree[:0x80]:", tb[:0x80].hex())
    # dump words of first 0x100 bytes
    for off in range(0, min(0x100, len(tb)), 16):
        ws = struct.unpack_from('<4I', tb, off)
        print(f"   +{off:04x}: " + " ".join(f"{w:08x}" for w in ws))
    print()
