"""graft.py <base alias> <donor alias> <out> <name,name,...>: base alias with
the named records' payloads taken from donor (PC LE files); rebuilds used
size and tree hash."""
import sys, struct, hashlib
sys.path.insert(0, 'E:/GitHub/NFSPS-360-PC-Career/scripts/python')
from nfssave.mc02 import MC02, Endian
from nfssave.tree import Tree
from nfssave.convert import CHUNK_NAMES
from nfssave.treehash import tree_hash
base, donor, out, names = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4].split(',')
inv = {v: k for k, v in CHUNK_NAMES.items()}
mb = MC02.parse(open(base, 'rb').read()); md = MC02.parse(open(donor, 'rb').read())
tb = Tree.parse(mb.tree, big=False); td = {r.id: r.payload for r in Tree.parse(md.tree, big=False).records}
for n in names:
    k = inv[n]
    for r in tb.records:
        if r.id == k:
            r.payload = td[k]; r.size = len(td[k])
used = sum(12 + len(r.payload) for r in tb.records)
tree = bytearray(tb.build(big=False, tree_size=mb.tree_size))
tree[0:16] = tree_hash(bytes(tree))
extra = bytearray(mb.extra); struct.pack_into('<I', extra, 4, used)
pc = MC02(Endian.LITTLE, extra=bytes(extra), tree=bytes(tree), tree_size=mb.tree_size)
open(out, 'wb').write(pc.to_bytes())
chk = Tree.parse(MC02.parse(open(out, 'rb').read()).tree, big=False)
print(out, hashlib.md5(open(out, 'rb').read()).hexdigest(), 'used', hex(used), hex(chk.used), 'recs', len(chk.records))
