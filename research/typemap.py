"""Infer per-word byte-order transforms for the raw-memcpy career blobs.

GameplayData (GameplayBinarySavableHelper) and FEPlayerCarDB
(VehicleBinarySavableHelper) are raw C-struct memcpys: u32, u16 pairs, u8
runs and mixed words sit side by side, so one blanket u32 swap corrupts
every non-u32 field (e.g. the u16 installed-part arrays of each car's
customization record -> the game sees a stock car).

For each 4-byte word (PC payload coordinates) every candidate transform
is scored against the evidence:
  * exact pair: the matched-state 360/PC pair (same career state) at the
    same offset - weight 4;
  * distribution: every 360 sample converted with the transform is looked
    up in the set of PC-native values at the same offset - weight 1;
  * both of the above pooled over every offset in the same array-stride
    class (records of the same struct), so a field populated in any one
    car record informs all 80.
Only words whose candidate outputs differ count as evidence; 360 bytes of
0xAA (uninitialized fill) are wildcards. Words with no evidence default to
the u32 swap. Output: nfssave/typemaps.json (run-length per chunk).

Run: python research/typemap.py   (needs the sample saves listed below)
"""

import json
import sys
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).parent.parent
sys.path.insert(0, str(ROOT))

from nfssave.container360 import read_container
from nfssave.mc02 import MC02
from nfssave.tree import Tree

FLASH = "F:/Content/E00001CFFAB204C4/45410822/00000001/"
PC100 = ("E:/legoj/Documents/Need for Speed ProStreet/100% Gamesave (OPTIONAL) - "
         "Place This in SAVE folder below/NFS Prostreet/CAREER_01/CAREER_01")
PAIR_360 = ROOT / "research/pair/CAREER_02_360_fresh"
PAIR_PC = ROOT / "research/pair/CAREER_02_pc_native"
SAMPLES_360 = [PAIR_360, ROOT / "Extracted/Career/CAREER_01", Path(FLASH + "CAREER_03")]
SAMPLES_PC = [PAIR_PC, Path(PC100), ROOT / "research/oracle/native_fresh_CAREER_01"]

# transform name -> source byte index for each output byte
TRANSFORMS = {
    "S32": (3, 2, 1, 0),
    "S16": (1, 0, 3, 2),
    "NAT": (0, 1, 2, 3),
    "S16H": (1, 0, 2, 3),   # [u16][u8][u8]
    "S16L": (0, 1, 3, 2),   # [u8][u8][u16]
}

GAMEPLAY = 0x3B309E09
CARDB = 0x47A07113

# pooling regions (PC payload offsets): (start, end, stride)
REGIONS = {
    CARDB: [
        (0x14, 0x2680, 0x18),        # car table, 24-byte entries
        (0x2680, 0x7C980, 0x1870),   # 80 per-car records
        (0x7C980, 0x7D000, 0x40),
        (0x7D000, 0x906A4, 0x8),     # packed 8-byte entries
    ],
    GAMEPLAY: [],
}


def apply(word: bytes, t: str) -> bytes:
    idx = TRANSFORMS[t]
    return bytes(word[i] for i in idx)


def _payloads(path: Path, big: bool) -> dict:
    data = path.read_bytes()
    if big:
        data = read_container(path).payload
    tree = Tree.parse(MC02.parse(data).tree, big=big)
    out = {}
    for r in tree.records:
        p = r.payload
        if big:
            p = p[4:]  # 360 leading flag word: PC carries it in the header
        out[r.id] = p
    return out


# output units per transform: (start, length) in the PC word
UNITS = {
    "S32": ((0, 4),),
    "S16": ((0, 2), (2, 2)),
    "NAT": ((0, 1), (1, 1), (2, 1), (3, 1)),
    "S16H": ((0, 2), (2, 1), (3, 1)),
    "S16L": ((0, 1), (1, 1), (2, 2)),
}
TRIVIAL = (0x00, 0xFF, 0xAA)


def _units(word: bytes, t: str):
    """Yield (unit position, source bytes, output bytes) for transform t."""
    idx = TRANSFORMS[t]
    cv = apply(word, t)
    for start, ln in UNITS[t]:
        yield (start, ln), bytes(word[idx[k]] for k in range(start, start + ln)),             cv[start:start + ln]


def _trivial(b: bytes) -> bool:
    return all(x in TRIVIAL for x in b)


def infer(cid: int, s360: list, spc: list, pair: tuple) -> list:
    """Score transforms per stride class at field-unit granularity.

    Distribution evidence: a converted unit (u16 half, byte, whole u32)
    counts as matched when that value occurs at the same unit position in
    any PC-native word of the same class (pooled over every record and PC
    sample). Pair evidence: the unit equals the matched-state PC word at
    the same offset (weighted x5). Fill/uninit-only units carry no
    evidence.
    """
    n = min(len(p) for p in s360 + spc) & ~3
    regions = REGIONS.get(cid, [])

    def cls(o):
        for a, b, s in regions:
            if a <= o < b:
                return (a, (o - a) % s)
        return ("abs", o)

    pcsets = defaultdict(set)   # (class, unitpos) -> values
    for o in range(0, n - 3, 4):
        c = cls(o)
        for p in spc:
            w = p[o:o + 4]
            for t in TRANSFORMS:
                for start, ln in UNITS[t]:
                    pcsets[(c, (start, ln))].add(w[start:start + ln])
    score = defaultdict(lambda: defaultdict(lambda: [0, 0]))
    for o in range(0, n - 3, 4):
        c = cls(o)
        for si, src in enumerate(s360):
            w = src[o:o + 4]
            if len({apply(w, t) for t in TRANSFORMS}) == 1:
                continue
            pw = pair[1][o:o + 4] if si == 0 else None
            for t in TRANSFORMS:
                sc = score[c][t]
                for pos, sb, ub in _units(w, t):
                    if _trivial(sb):
                        continue
                    ln = pos[1]
                    sc[1] += ln
                    if ub in pcsets[(c, pos)]:
                        sc[0] += ln
                    if pw is not None:
                        sc[1] += 5 * ln
                        if ub == pw[pos[0]:pos[0] + ln]:
                            sc[0] += 5 * ln
    acts = []
    for o in range(0, n - 3, 4):
        sc = score.get(cls(o))
        if not sc:
            acts.append("S32")
            continue
        rate = {t: (sc[t][0] / sc[t][1] if sc[t][1] else 0.0) for t in TRANSFORMS}
        best = max(rate.values())
        if best == 0.0:
            acts.append("S32")
            continue
        winners = [t for t in TRANSFORMS if rate[t] >= best - 1e-9]
        acts.append(winners[0])
    return acts


def runs(acts: list) -> list:
    out = []
    for i, a in enumerate(acts):
        if out and out[-1][2] == a and out[-1][1] == 4 * i:
            out[-1][1] = 4 * i + 4
        else:
            out.append([4 * i, 4 * i + 4, a])
    return out


def main() -> None:
    s360 = [_payloads(p, True) for p in SAMPLES_360]
    spc = [_payloads(p, False) for p in SAMPLES_PC]
    result = {}
    for cid in (GAMEPLAY, CARDB):
        a = [s[cid] for s in s360]
        b = [s[cid] for s in spc]
        if cid == GAMEPLAY:
            a = [p[:len(b[0])] for p in a]  # 360 carries a 0x4000 zero tail
        acts = infer(cid, a, b, (a[0], b[0]))
        result[f"{cid:08X}"] = {"size": len(b[0]), "runs": runs(acts)}
        hist = Counter(acts)
        print(f"{cid:08X}: {dict(hist)}; {len(result[f'{cid:08X}']['runs'])} runs")
    out = ROOT / "nfssave/typemaps.json"
    out.write_text(json.dumps(result, separators=(",", ":")))
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
