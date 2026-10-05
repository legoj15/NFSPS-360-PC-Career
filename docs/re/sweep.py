"""Linear sweep of .text building an xref database (calls, leas, data refs)."""
import pickle
import exetools as E
from capstone import Cs, CS_ARCH_X86, CS_MODE_32
from capstone.x86 import X86_OP_IMM, X86_OP_MEM, X86_REG_EIP

md = Cs(CS_ARCH_X86, CS_MODE_32)
md.detail = True

name, sva, vsize, roff, rsize = [s for s in E.sections if s[0]=='.text'][0]
base = E.IMG_BASE + sva
blob = E.data[roff:roff+rsize]

xrefs = {}   # target -> set of insn addrs
insn_at = {} # addr -> (mnemonic, op_str)

def feed(ins):
    insn_at[ins.address] = (ins.mnemonic, ins.op_str)
    for op in ins.operands:
        if op.type == X86_OP_IMM:
            t = op.imm & 0xFFFFFFFF
            if 0x400000 <= t < 0x2000000:
                xrefs.setdefault(t, set()).add(ins.address)
        elif op.type == X86_OP_MEM:
            m = op.mem
            if m.base == X86_REG_EIP or (m.base == 0 and m.index == 0):
                t = (m.disp & 0xFFFFFFFF) if m.base == 0 else (ins.address + ins.size + m.disp) & 0xFFFFFFFF
                if 0x400000 <= t < 0x2000000:
                    xrefs.setdefault(t, set()).add(ins.address)

count = 0
pos = 0
n = len(blob)
while pos < n:
    got = False
    for ins in md.disasm(blob[pos:], base + pos):
        got = True
        count += 1
        feed(ins)
        pos = (ins.address - base) + ins.size
        if pos >= n: break
    if not got:
        pos += 1  # skip undecodable byte

print(f"swept {count} instructions")
with open("text_sweep.pkl","wb") as f:
    pickle.dump({"xrefs": {k: sorted(v) for k,v in xrefs.items()},
                 "insn_at": insn_at}, f)
print("saved text_sweep.pkl")
print("xrefs to 0x99CF94 region (stat blob):")
for k in sorted(xrefs):
    if 0x99CF00 <= k <= 0x99E000:
        print(f"  {k:#x} <- {[hex(a) for a in xrefs[k]]}")
