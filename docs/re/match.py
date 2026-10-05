#!/usr/bin/env python3
"""Match NFS ProStreet save chunks between Xbox 360 and PC, and produce
byte-classification field maps for the best-matched pairs.

Chunks are identified by their platform-independent id word (verified:
identical ids across 360/PC for the same chunk).  For each matched pair the
payloads are aligned 4 bytes at a time (exact-index when sizes are equal,
Needleman-Wunsch alignment when they differ) and every slot is classified:

  NUM     360 BE u32 == PC LE u32 (converter: byte-swap the word)
  SAME    raw bytes identical on both sides (converter: copy)
  STR360  printable run on 360, 0x0101../0x1010 fill on PC (name fragments
          the PC writer never serializes; converter: emit 0x10 fill)
  STR     printable on both sides (string data; copy bytes)
  DIFF    values differ (player-specific numbers; swap as NUM anyway)
  GAPx    alignment gap on one side (PC-only or 360-only words)
"""
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import walk360
import walkpc

RESEARCH = os.path.dirname(os.path.abspath(__file__))
FIELDMAPS = os.path.join(RESEARCH, 'fieldmaps')
REPO = os.path.dirname(os.path.dirname(RESEARCH))  # repo root
os.makedirs(FIELDMAPS, exist_ok=True)

P360_ALIAS = os.path.join(REPO, 'Extracted', 'Alias', 'ALIAS_360')
P360_CAREER = os.path.join(REPO, 'Extracted', 'Career', 'CAREER_01')
PPC_ALIAS = 'E:/legoj/Documents/Need for Speed ProStreet/100% Gamesave (OPTIONAL) - Place This in SAVE folder below/NFS Prostreet/ALIAS_PEIROKUNMANWSP/ALIAS_PEIROKUNMANWSP'
PPC_CAREER = 'E:/legoj/Documents/Need for Speed ProStreet/100% Gamesave (OPTIONAL) - Place This in SAVE folder below/NFS Prostreet/CAREER_01/CAREER_01'


def load360(path):
    raw = open(path, 'rb').read()
    total = struct.unpack('>I', raw[0xD004:0xD008])[0]
    d = raw[0xD000:0xD000 + total]
    extra = struct.unpack('>I', d[8:12])[0]
    tree = d[0x1C + extra:]
    recs = walk360.walk_records(tree, 0x48, len(tree))
    for r in recs:
        r['payload'] = tree[r['payload_off']:r['end']]
        for ch in r['children']:
            ch['payload'] = tree[ch['payload_off']:ch['end']]
    return recs


def loadpc(path):
    d = open(path, 'rb').read()
    extra = struct.unpack('<I', d[8:12])[0]
    tree = d[0x1C + extra:]
    _c, _p, m, _u, recs = walkpc.walk(tree)
    for r in recs:
        r['payload'] = tree[r['payload_off']:r['end']]
    return recs, tree[m + 8:m + 8 + _u], tree[0x14:m]


def printable_run(b):
    return all(32 <= x < 127 for x in b) and any(32 <= x < 127 for x in b)


def mostly_printable(b):
    return sum(1 for x in b if 32 <= x < 127) >= 3


def is_fill(b):
    """PC uninitialized-heap garbage: 0x10 fill or 0x01/0x00/0x10 mixes."""
    return len(set(b)) == 1 and b[0] in (0x10, 0x01)


def is_garbageish(b):
    return all(x in (0x00, 0x01, 0x10) for x in b)


def classify(w360, wpc):
    if w360 is None:
        return 'GAP3'
    if wpc is None:
        return 'GAPC'
    if wpc == w360[::-1]:
        return 'NUM'
    if wpc == w360:
        return 'SAME'
    p360 = mostly_printable(w360)
    ppc = mostly_printable(wpc)
    if p360 and (is_fill(wpc) or is_garbageish(wpc)):
        return 'STR360'          # 360 name fragment; PC leaves slot unserialized
    if p360 and ppc:
        return 'STR'
    if is_garbageish(w360) and is_garbageish(wpc):
        return 'FILL'
    if p360 and not ppc and struct.unpack('<I', wpc)[0] <= 0xFFFF:
        return 'STR360'          # 360 string vs PC small int in the same slot
    return 'DIFF'


def nw_align_simple(a, b):
    """Simpler correct NW with full score matrix kept."""
    n, m = len(a) // 4, len(b) // 4
    GAP, MATCH, FILLSC, MISS = -2, 3, 2, 0

    def sc(wa, wb):
        c = classify(wa, wb)
        if c in ('NUM', 'SAME', 'STR', 'STR360'):
            return MATCH
        if c == 'FILL':
            return FILLSC
        return MISS

    F = [[0] * (m + 1) for _ in range(n + 1)]
    for i in range(1, n + 1):
        F[i][0] = i * GAP
    for j in range(1, m + 1):
        F[0][j] = j * GAP
    for i in range(1, n + 1):
        wa = a[(i - 1) * 4:i * 4]
        for j in range(1, m + 1):
            wb = b[(j - 1) * 4:j * 4]
            F[i][j] = max(F[i - 1][j - 1] + sc(wa, wb),
                          F[i - 1][j] + GAP,
                          F[i][j - 1] + GAP)
    i, j = n, m
    out = []
    while i > 0 or j > 0:
        wa = a[(i - 1) * 4:i * 4] if i > 0 else None
        wb = b[(j - 1) * 4:j * 4] if j > 0 else None
        if i > 0 and j > 0 and F[i][j] == F[i - 1][j - 1] + sc(wa, wb):
            out.append((wa, wb)); i -= 1; j -= 1
        elif i > 0 and F[i][j] == F[i - 1][j] + GAP:
            out.append((wa, None)); i -= 1
        else:
            out.append((None, wb)); j -= 1
    out.reverse()
    return out


def field_map(idx_path, r360, rpc, out):
    a, b = r360['payload'], rpc['payload']
    L = []
    L.append(f'field map: {idx_path}')
    L.append(f'360: id=0x{r360["id"]:08X} size=0x{r360["size"]:X} type=0x{r360["type"]:08X}')
    L.append(f'PC : id=0x{rpc["id"]:08X} size=0x{rpc["size"]:X} type=0x{rpc["type"]:08X}')
    if len(a) == len(b):
        pairs = [(a[i:i + 4], b[i:i + 4]) for i in range(0, len(a), 4)]
        mode = 'equal size, direct index'
    elif len(a) // 4 <= 4096 and len(b) // 4 <= 4096:
        pairs = nw_align_simple(a, b)
        mode = f'size differs (0x{len(a):X} vs 0x{len(b):X}), NW alignment'
    else:
        n = min(len(a), len(b)) // 4 * 4
        pairs = [(a[i:i + 4], b[i:i + 4]) for i in range(0, n, 4)]
        pairs.append((a[n:], b[n:]))
        mode = f'size differs (0x{len(a):X} vs 0x{len(b):X}), too large for NW: direct index over common prefix + tail'
    L.append(f'alignment: {mode}; slots={len(pairs)}')
    L.append('')
    L.append('slot   off360  offPC   class    360-bytes  pc-bytes')
    stats = {}
    compact = len(pairs) > 5000
    oa = ob = 0
    run_class = None
    run_start = 0
    run_a0 = run_b0 = 0
    for k, (wa, wb) in enumerate(pairs):
        c = classify(wa, wb)
        stats[c] = stats.get(c, 0) + 1
        if not compact:
            L.append(f'{k:<5}  0x{oa:04X}   0x{ob:04X}   {c:<7}  '
                     f'{wa.hex() if wa else "--------"}  {wb.hex() if wb else "--------"}'
                     + (f'   [360={struct.unpack(">I", wa)[0]:10d} pc={struct.unpack("<I", wb)[0]:10d}]'
                        if wa and wb and c in ('NUM', 'DIFF') else ''))
        else:
            if c != run_class:
                if run_class is not None:
                    L.append(f'  run: slots {run_start}..{k - 1}  off360 0x{run_a0:04X}..0x{oa:04X}  '
                             f'offPC 0x{run_b0:04X}..0x{ob:04X}  class={run_class}  len={k - run_start}')
                run_class, run_start, run_a0, run_b0 = c, k, oa, ob
        if wa:
            oa += 4
        if wb:
            ob += 4
    if compact and run_class is not None:
        L.append(f'  run: slots {run_start}..{len(pairs) - 1}  off360 0x{run_a0:04X}..0x{oa:04X}  '
                 f'offPC 0x{run_b0:04X}..0x{ob:04X}  class={run_class}  len={len(pairs) - run_start}')
    if compact:
        L.insert(0, '(compact run-length form: >5000 slots)')
    L.append('')
    tot = len(pairs)
    L.append('coverage summary:')
    for c in sorted(stats, key=lambda x: -stats[x]):
        L.append(f'  {c:<7} {stats[c]:5d}  {100.0 * stats[c] / tot:5.1f}%')
    open(out, 'w').write('\n'.join(L))
    return stats, tot


def main():
    a360 = load360(P360_ALIAS)
    c360 = load360(P360_CAREER)
    apc, apc_used, apc_pre = loadpc(PPC_ALIAS)
    cpc, cpc_used, cpc_pre = loadpc(PPC_CAREER)

    out = []
    out.append('=== NFS ProStreet 360<->PC chunk matching (by platform-independent id) ===')
    out.append('')
    for label, r360s, rpcs in (('ALIAS', a360, apc), ('CAREER', c360, cpc)):
        out.append(f'--- {label}: {len(r360s)} 360 records, {len(rpcs)} PC records')
        pc_by_id = {r['id']: (n, r) for n, r in enumerate(rpcs)}
        used = set()
        for n, r in enumerate(r360s):
            if r['id'] in pc_by_id:
                m, pr = pc_by_id[r['id']]
                used.add(m)
                sznote = 'size-equal' if pr['size'] == r['size'] else f'sizes differ ({r["size"]:#X} vs {pr["size"]:#X})'
                tynote = 'type-equal' if pr['type'] == r['type'] else f'type 0x{r["type"]:08X} vs 0x{pr["type"]:08X}'
                out.append(f'  360[{n:2}] id=0x{r["id"]:08X} <-> PC[{m:2}]  {sznote}, {tynote}'
                           + (' [container]' if r['children'] else ''))
            else:
                out.append(f'  360[{n:2}] id=0x{r["id"]:08X} <-> NO PC MATCH (hidden/encrypted region)' if label == 'CAREER' and r['id'] in () else
                           f'  360[{n:2}] id=0x{r["id"]:08X} <-> NO PC MATCH')
        for m, pr in enumerate(rpcs):
            if m not in used:
                out.append(f'  PC-only chunk: PC[{m:2}] id=0x{pr["id"]:08X} size=0x{pr["size"]:X}')
        out.append('')
    out.append('360 career tail chunks (stored in obfuscated gap 0xAA6CC..0xAADEC):')
    out.append('  inferred from PC career order: 0xD548266C (PC size 0x24), 0xCA269650 (PC size 0x44)')
    out.append('')
    out.append('type-slot note: small types (0,2,3) match across platforms; other slots hold')
    out.append('high-entropy values on their respective platform (timestamps or uninitialised')
    out.append('heap on PC, e.g. 0x10101001 / 0xFFFFFF01 / 0x3F39E000). Converter should')
    out.append('treat the type word as per-save volatile, not structural identity.')
    out.append('')
    # alias name anchors
    out.append('string anchors:')
    out.append('  360 alias name chunk 322ED42F: "<alias>"     <-> PC: "PEIROKUNMANWSP" (same slot)')
    out.append('  360 alias stats chunk 4E8AA143: "3;-1"           <-> PC: "3;-42885" (same slot, player values differ)')
    out.append('  PC preambles (0x1AC bytes, uncounted) carry the 20-char codes: alias "LNA2PSNW8FZQ58FETRLD", career "Y4K3VZXH2ZE2J2ZC6RLD"')
    txt = '\n'.join(out)
    open(os.path.join(RESEARCH, 'matched_pairs.txt'), 'w').write(txt)
    print(txt)

    # field maps for the interesting pairs
    pairs = [
        ('alias_name_322ED42F', next(r for r in a360 if r['id'] == 0x322ED42F), next(r for r in apc if r['id'] == 0x322ED42F)),
        ('alias_stats_4E8AA143', next(r for r in a360 if r['id'] == 0x4E8AA143), next(r for r in apc if r['id'] == 0x4E8AA143)),
        ('alias_props_8FFBE3E8', next(r for r in a360 if r['id'] == 0x8FFBE3E8), next(r for r in apc if r['id'] == 0x8FFBE3E8)),
        ('alias_awards_DC6B027F', next(r for r in a360 if r['id'] == 0xDC6B027F), next(r for r in apc if r['id'] == 0xDC6B027F)),
        ('alias_6CC89C57', next(r for r in a360 if r['id'] == 0x6CC89C57), next(r for r in apc if r['id'] == 0x6CC89C57)),
        ('alias_8B7D0AAD', next(r for r in a360 if r['id'] == 0x8B7D0AAD), next(r for r in apc if r['id'] == 0x8B7D0AAD)),
        ('career_props_328C6431', next(r for r in c360 if r['id'] == 0x328C6431), next(r for r in cpc if r['id'] == 0x328C6431)),
        ('career_DC6B027F', next(r for r in c360 if r['id'] == 0xDC6B027F), next(r for r in cpc if r['id'] == 0xDC6B027F)),
        ('career_B67F6CC6', next(r for r in c360 if r['id'] == 0xB67F6CC6), next(r for r in cpc if r['id'] == 0xB67F6CC6)),
        ('career_51A41B14', next(r for r in c360 if r['id'] == 0x51A41B14), next(r for r in cpc if r['id'] == 0x51A41B14)),
        ('career_47A07113_big', next(r for r in c360 if r['id'] == 0x47A07113), next(r for r in cpc if r['id'] == 0x47A07113)),
        ('career_885B4DDC', next(r for r in c360 if r['id'] == 0x885B4DDC), next(r for r in cpc if r['id'] == 0x885B4DDC)),
    ]
    print()
    print('field maps:')
    for name, r3, rp in pairs:
        stats, tot = field_map(name, r3, rp, os.path.join(FIELDMAPS, name + '.txt'))
        summ = ' '.join(f'{k}={v}' for k, v in sorted(stats.items(), key=lambda x: -x[1]))
        print(f'  {name}: {tot} slots -> {summ}')


if __name__ == '__main__':
    main()
