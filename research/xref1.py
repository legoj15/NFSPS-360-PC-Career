import exetools as E

print("=== xrefs to 0x59D0F0 (memcard lib version check) ===")
for va in E.find_dword_xrefs(0x59D0F0):
    print(f"  data xref at {va:#x} ({E.sec_of(va)})")

print("=== imm xrefs in .text to 0x59D0F0 ===")
for va in E.imm_xrefs(0x59D0F0):
    print(f"  {va:#x} ({E.sec_of(va)})")

for s, base in [("ALIAS_",0x974DE8), ("CAREER_",0x974DF0), ("Memcard SaveBuffer",0x974E08), ("SHADOW_",0x974E30)]:
    print(f"=== string '{s}' ===")
    vas = E.find_ascii(s)
    print("  occurrences:", [hex(v) for v in vas])
    for sv in vas:
        dx = E.find_dword_xrefs(sv)
        ix = E.imm_xrefs(sv)
        print(f"  at {sv:#x}: data xrefs {[hex(v) for v in dx]}, code imm xrefs {[hex(v) for v in ix]}")

print("=== stat string blob at 0x99CF94 — sample strings ===")
va = 0x99CF94
for i in range(12):
    s = E.cstr(va)
    print(f"  {va:#x}: {s!r}")
    va += len(s)+1 if s else 4
