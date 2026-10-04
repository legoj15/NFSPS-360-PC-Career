"""Emulate nfs.exe tree-hash fn (VA 0x6D9CE0) with Unicorn - ground-truth oracle.
Patches (in emulated memory only): fs:[0] SEH ops -> nops, CRT thunks -> stubs,
SecuROM thunk [0x19e787c] -> 0x6d3f35."""
import struct
import exetools as E
from unicorn import *
from unicorn.x86_const import *

EXE = E.data
BASE = 0x400000
STACK, STACK_SIZE = 0x00100000, 0x200000
HEAP = 0x70000000
HOOK_RET = 0x50000000

STUB = 0x51000000  # code stubs page

def make_emu():
    mu = Uc(UC_ARCH_X86, UC_MODE_32)
    text = [s for s in E.sections if s[0] == '.text'][0]
    name, sva, vsize, roff, rsize = text
    tsize = (max(vsize, rsize) + 0xFFF) & ~0xFFF
    mu.mem_map(BASE + sva, tsize)
    img = bytearray(EXE[roff:roff+rsize])
    # pad to mapped size
    img += bytes(tsize - len(img))

    # ---- patch fs:[0] ops in .text ----
    def patch_all(needle, repl, desc):
        n = 0
        i = img.find(needle)
        while i >= 0:
            img[i:i+len(needle)] = repl
            n += 1
            i = img.find(needle, i+1)
        return n
    n1 = patch_all(b'\x64\xa1\x00\x00\x00\x00', b'\x31\xc0\x90\x90\x90\x90', 'mov eax,fs:[0]')
    n2 = patch_all(b'\x64\xa3\x00\x00\x00\x00', b'\x90'*6, 'mov fs:[0],eax')
    # mov fs:[0], r32  ->  64 89 2d/0d/... 00000000 (7 bytes)
    n3 = 0
    for m in (0x05,0x0d,0x15,0x1d,0x25,0x2d,0x35,0x3d):
        n3 += patch_all(bytes([0x64,0x89,m,0,0,0,0]), b'\x90'*7, 'mov fs:[0],r')
        n3 += patch_all(bytes([0x64,0x8b,m,0,0,0,0]), b'\x31\xc0' + b'\x90'*5, 'mov r,fs:[0]')

    mu.mem_write(BASE + sva, bytes(img))

    # map other sections
    for name, sva, vsize, roff, rsize in E.sections:
        if name in ('.text', '.securom'):
            continue
        size = (max(vsize, rsize) + 0xFFF) & ~0xFFF
        mu.mem_map(BASE + sva, size)
        mu.mem_write(BASE + sva, EXE[roff:roff+rsize])

    mu.mem_map(STACK, STACK_SIZE)
    mu.mem_map(HEAP, 0x100000)
    mu.mem_map(HOOK_RET & ~0xFFF, 0x1000)
    mu.mem_map(STUB, 0x1000)
    mu.mem_map(0x19e7000, 0x2000)
    mu.mem_write(0x19e787c, struct.pack('<I', 0x6d3f36))

    # ---- stubs ----
    # memcpy(dst,src,n) - preserves callee-saved regs per x86 ABI
    mc = bytes.fromhex(
        '56'                          # push esi
        '57'                          # push edi
        '8b7c240c' '8b742410' '8b4c2414'  # edi=[esp+0xc] dst, esi=[esp+0x10] src, ecx=[esp+0x14] n
        'f3a4'                        # rep movsb
        '8b44240c'                    # eax=[esp+0xc] dst
        '5f' '5e'                     # pop edi, esi
        'c3')                         # ret (cdecl)
    mu.mem_write(STUB + 0x00, mc)
    ms = bytes.fromhex(
        '57'                          # push edi
        '8b7c2408' '8a44240c' '8b4c2410'  # edi=[esp+8] dst, al=[esp+0xc], ecx=[esp+0x10]
        'f3aa'                        # rep stosb
        '8b442408'
        '5f'
        'c3')
    mu.mem_write(STUB + 0x40, ms)
    # ret
    mu.mem_write(STUB + 0x80, b'\xc3')
    # redirect CRT thunks: write jmp STUB at thunk addresses (in .text, already mapped)
    def jmp_stub(addr, stub_off):
        rel = (STUB + stub_off) - (addr + 5)
        mu.mem_write(addr, b'\xe9' + struct.pack('<i', rel))
    jmp_stub(0x8287CE, 0x00)   # memcpy
    jmp_stub(0x8287C8, 0x40)   # memset
    jmp_stub(0x828460, 0x80)   # __security_check_cookie -> ret
    return mu, (n1, n2, n3)

def run_hash(data: bytes) -> bytes:
    mu, counts = make_emu()
    data_addr = HEAP + 0x1000
    out_addr  = HEAP + 0x2000
    mu.mem_write(data_addr, data)
    esp = STACK + STACK_SIZE - 0x10000
    mu.mem_write(esp, struct.pack('<III', HOOK_RET, data_addr, len(data)))
    mu.reg_write(UC_X86_REG_ESP, esp)
    mu.reg_write(UC_X86_REG_ECX, out_addr)
    mu.emu_start(0x6d9ce0, HOOK_RET, timeout=600*1000000)
    return bytes(mu.mem_read(out_addr, 16))

if __name__ == '__main__':
    base = "E:/legoj/Documents/Need for Speed ProStreet/100% Gamesave (OPTIONAL) - Place This in SAVE folder below/NFS Prostreet/"
    for tag, path in [("alias","ALIAS_PEIROKUNMANWSP/ALIAS_PEIROKUNMANWSP"),
                      ("career","CAREER_01/CAREER_01")]:
        d = open(base+path,'rb').read()
        extra = struct.unpack_from('<I', d, 8)[0]
        tree = d[0x1c+extra:]
        stored = tree[0:16]
        got = run_hash(tree[0x10:])
        print(f"{tag}: stored={stored.hex()} emu={got.hex()} {'MATCH' if got==stored else 'MISMATCH'}")
