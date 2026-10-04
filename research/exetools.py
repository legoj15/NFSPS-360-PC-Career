"""Helper: load nfs.exe, provide VA<->file, xref scanning, capstone disasm."""
import struct, bisect
from capstone import Cs, CS_ARCH_X86, CS_MODE_32

EXE = r"E:/legoj/Documents/Need for Speed ProStreet/nfs.exe"
data = open(EXE, 'rb').read()

# Parse PE headers
e_lfanew = struct.unpack_from('<I', data, 0x3C)[0]
machine, nsec = struct.unpack_from('<HH', data, e_lfanew+4)
opt_size = struct.unpack_from('<H', data, e_lfanew+20)[0]
image_base = struct.unpack_from('<I', data, e_lfanew+24+28)[0]
sec_off = e_lfanew+24+opt_size
sections = []
for i in range(nsec):
    o = sec_off + i*40
    name = data[o:o+8].rstrip(b'\0').decode()
    vsize, va, rsize, roff = struct.unpack_from('<IIII', data, o+8)
    sections.append((name, va, vsize, roff, rsize))
IMG_BASE = image_base

def va2off(va):
    rva = va - IMG_BASE
    for name, sva, vsize, roff, rsize in sections:
        if sva <= rva < sva + max(vsize, rsize):
            if rva - sva < rsize:
                return roff + (rva - sva)
            return None  # in bss / uninitialized
    return None

def off2va(off):
    for name, sva, vsize, roff, rsize in sections:
        if roff <= off < roff + rsize:
            return IMG_BASE + sva + (off - roff)
    return None

def sec_of(va):
    rva = va - IMG_BASE
    for name, sva, vsize, roff, rsize in sections:
        if sva <= rva < sva + max(vsize, rsize):
            return name
    return None

def read(va, n):
    off = va2off(va)
    if off is None: return None
    return data[off:off+n]

def u32(va):
    b = read(va, 4)
    return struct.unpack('<I', b)[0] if b else None

def cstr(va, maxlen=200):
    off = va2off(va)
    if off is None: return None
    end = data.find(b'\0', off, off+maxlen)
    if end < 0: return None
    try: return data[off:end].decode('ascii')
    except: return None

md = Cs(CS_ARCH_X86, CS_MODE_32)
md.detail = True

def disasm(va, n=40, stop_at_ret=False):
    """Disassemble n instructions at va. Returns list of (addr, mnem, opstr, bytes)."""
    b = read(va, n*16 + 32)
    out = []
    for ins in md.disasm(b, va):
        out.append(ins)
        if len(out) >= n: break
        if stop_at_ret and ins.mnemonic in ('ret','retn','retf','jmp'): break
    return out

def find_dword_xrefs(target_va, sections_only=None):
    """Find all occurrences of the 4-byte LE value target_va in file; return VAs."""
    needle = struct.pack('<I', target_va)
    res = []
    i = data.find(needle)
    while i >= 0:
        va = off2va(i)
        if va: res.append(va)
        i = data.find(needle, i+1)
    return res

def imm_xrefs(target_va, code_sec=('.text',)):
    """Scan code for instructions referencing target_va as immediate (push/mov)."""
    res = []
    needle = struct.pack('<I', target_va)
    for name, sva, vsize, roff, rsize in sections:
        if code_sec and name not in code_sec: continue
        blob = data[roff:roff+rsize]
        base = IMG_BASE + sva
        i = blob.find(needle)
        while i >= 0:
            va = base + i
            # try to disassemble at a few preceding offsets to identify the insn
            res.append(va)
            i = blob.find(needle, i+1)
    return res

def find_ascii(s, start=0):
    """Find ASCII string in file, return VAs."""
    if isinstance(s, str): s = s.encode()
    res = []
    i = data.find(s, start)
    while i >= 0:
        va = off2va(i)
        if va: res.append(va)
        i = data.find(s, i+1)
    return res

def bytes_at(va, n):
    return read(va, n)

if __name__ == '__main__':
    print(f"image base {IMG_BASE:#x}, {nsec} sections:")
    for name, sva, vsize, roff, rsize in sections:
        print(f"  {name:8s} va {IMG_BASE+sva:#010x} vsize {vsize:#010x} raw {roff:#010x} rsize {rsize:#010x}")
