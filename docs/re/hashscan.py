import re, struct

trees = {}
for n in ("alias","career"):
    trees[n] = open(f"tree_{n}.bin","rb").read()

# all aligned u32 words in both trees
words = set()
for n,tb in trees.items():
    for off in range(0, len(tb)-3, 4):
        words.add(struct.unpack_from('<I', tb, off)[0])
print(f"{len(words)} distinct aligned words in trees")

def djb2(s):
    h = 0xFFFFFFFF
    for c in s.encode():
        h = (h*33 + c) & 0xFFFFFFFF
    return h

blob = open(r"E:/legoj/Documents/Need for Speed ProStreet/nfs.exe",'rb').read()
hits = []
seen = set()
for m in re.finditer(rb'[\x20-\x7e]{4,80}', blob):
    s = m.group().decode('ascii')
    if s in seen: continue
    seen.add(s)
    for cand in (s, s.upper(), s.lower()):
        h = djb2(cand)
        if h in words:
            hits.append((h, cand))
print(f"{len(hits)} hash hits:")
for h, s in sorted(set(hits)):
    where = [n for n,tb in trees.items() if struct.pack('<I',h) in tb]
    print(f"  {h:#010x}  {s!r}  in {where}")
