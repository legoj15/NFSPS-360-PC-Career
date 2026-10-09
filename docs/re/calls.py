"""List direct call sites (E8 rel32) of a target VA in nfs.exe: python calls.py 0x7e3410"""
import sys, struct
from exetools import data, sections, IMG_BASE
def callers(t):
    out = []
    for name, sva, vsize, roff, rsize in sections:
        if name != '.text': continue
        blob = data[roff:roff + rsize]; base = IMG_BASE + sva
        i = blob.find(b'\xe8')
        while i >= 0:
            if i + 5 <= len(blob):
                rel = struct.unpack_from('<i', blob, i + 1)[0]
                if base + i + 5 + rel == t: out.append(base + i)
            i = blob.find(b'\xe8', i + 1)
    return out
if __name__ == '__main__':
    for a in sys.argv[1:]:
        print(a, [hex(x) for x in callers(int(a, 16))])
