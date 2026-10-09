"""Side-by-side dump of the GameplayData race-day progress table
(90 x [key][state][score]) for any mix of 360 / PC saves:
    python docs/re/rdtable.py SAVE [SAVE ...]
"""
import struct,sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts" / "python"))
from nfssave.mc02 import MC02
from nfssave.tree import Tree
from nfssave.container360 import read_container
def load(path):
    try:
        c=read_container(path); m=MC02.parse(c.payload); big=True
    except Exception:
        m=MC02.parse(open(path,'rb').read()); big=False
    t=Tree.parse(m.tree,big=big)
    return {x.id:x.payload for x in t.records}[0x3B309E09], big
ANCHOR=0x25acc392
def table(path):
    p,big=load(path); e='>' if big else '<'
    a=p.find(struct.pack(e+'I',ANCHOR))
    # walk back/forward in 12-byte strides while key looks like a hash (non-small)
    s=a
    while s-12>=0 and struct.unpack_from(e+'I',p,s-12)[0]>0xffff: s-=12
    out={}; o=s; order=[]
    while o+12<=len(p):
        k,v1,v2=struct.unpack_from(e+'III',p,o)
        if k<=0xffff: break
        out[k]=(v1,v2); order.append(k); o+=12
    return out,order,s,o
if __name__=='__main__':
    res=[table(f) for f in sys.argv[1:]]
    for f,(t,order,s,e) in zip(sys.argv[1:],res): print(f, hex(s), hex(e), len(t))
    keys=res[0][1]
    for k in keys:
        print(hex(k).ljust(11), '  '.join(f'{r[0].get(k,("-","-"))[0]:>6x} {r[0].get(k,("-","-"))[1]:>6x}' if k in r[0] else '     -      -' for r in res))
