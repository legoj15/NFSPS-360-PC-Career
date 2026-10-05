"""Build career fieldmaps from the matched-state pair (2026-10-04).

Pair: the user's own career saved at the same progress point on both
platforms (console CAREER_02 via flash drive vs PC-native CAREER_02).
Every shared chunk is classified word-by-word and written as a fieldmap
txt (run-length rows) for payload_rules.regenerate_rules. A simulation of
the conversion is compared against the native PC bytes per word: NUM/SAME/
STR360 words must reproduce exactly; DIFF words are volatile (GUIDs,
timestamps) and expected to differ.

Alignment (verified on the pair): 360 payload word k (k>=1) corresponds to
PC word k-1; the 360 marker word 0 maps to the PC flags slot (dropped by
_to_pc_record) and the PC's final word is trailing junk with no partner.
"""

import sys
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from nfssave.mc02 import MC02
from nfssave.tree import Tree, Record
from nfssave.container360 import read_container
from nfssave.convert import normalize_gameplay
from nfssave.payload_rules import convert_payload_mapped, string_word_set

PAIR = Path(__file__).parent / "pair"
OUT = Path(__file__).parent / "fieldmaps"


def classify(w360: bytes, wpc: bytes, k: int, str_words: set[int]) -> str:
    if wpc == w360[::-1] and w360 != wpc:
        return "NUM"
    if wpc == w360:
        # only a NON-palindrome equality proves a natural-order field; a
        # palindrome (zeros / fill) says nothing about byte order, and a
        # richer save may hold real content at that position
        return "ZERO" if w360 == w360[::-1] else "SAME"
    if 4 * k in str_words:
        return "STR360"
    return "DIFF"


def main() -> None:
    t360 = Tree.parse(MC02.parse(read_container(PAIR / "CAREER_02_360_fresh").payload).tree, big=True)
    tpc = Tree.parse(MC02.parse((PAIR / "CAREER_02_pc_native").read_bytes()).tree, big=False)
    pc_by_id = {r.id: r for r in tpc.records}

    for rec in t360.records:
        pcr = pc_by_id.get(rec.id)
        if pcr is None:
            print(f"!! chunk {rec.id:#x} absent from PC file - skipped")
            continue
        normalize_gameplay(rec, [])
        n = len(rec.payload)
        if n != len(pcr.payload):
            print(f"!! chunk {rec.id:#x}: sizes differ "
                  f"({n:#x} vs {len(pcr.payload):#x}) - skipped")
            continue
        nw = n // 4
        str_words = string_word_set(rec.payload)
        classes = {0: "MARK"}
        for k in range(1, nw):
            classes[k] = classify(rec.payload[4 * k:4 * k + 4],
                                  pcr.payload[4 * (k - 1):4 * (k - 1) + 4],
                                  k, str_words)

        # emit run-length fieldmap rows
        lines = [
            f"field map: career_pair2_{rec.id:08X}",
            f"360: id=0x{rec.id:08X} size=0x{n:X} type=0x{rec.type:08X}",
            f"PC : id=0x{pcr.id:08X} size=0x{n:X}",
            "alignment: 360 word k <-> PC word k-1 (marker/junk slots at both ends)",
        ]
        k = 1
        while k < nw:
            j = k
            while j + 1 < nw and classes[j + 1] == classes[k]:
                j += 1
            lines.append(
                f"run: slots {k}..{j} off360 0x{4*k:X}..0x{4*(j+1):X} "
                f"offPC 0x{4*(k-1):X}..0x{4*j:X} class={classes[k]} len={j-k+1}")
            k = j + 1
        (OUT / f"career_pair2_{rec.id:08X}.txt").write_text("\n".join(lines) + "\n")

        # simulate the real pipeline: mapped swap on the raw payload, then
        # the PC re-frame (drop marker word, append trailing junk word)
        acts = {off: classes[off // 4] for off in range(4, 4 * nw, 4)}
        conv = convert_payload_mapped(rec.payload, acts)[4:] + b"\x00\x00\x00\x00"
        exact = volatile = 0
        mism = Counter()
        for k in range(nw):
            got, want = conv[4 * k:4 * k + 4], pcr.payload[4 * k:4 * k + 4]
            if got == want:
                exact += 1
            else:
                volatile += 1
                mism[classes[k + 1] if k + 1 in classes else "TRAILING"] += 1
        hist = Counter(classes.values())
        print(f"{rec.id:08X} ({n:7X} B): {dict(hist)}")
        print(f"    simulate: {exact}/{nw} words exact; differing: {dict(mism)}")
        # where do the differing words sit? junk blocks cluster; misalignment
        # would scatter mismatches through data regions
        diff_words = [k for k, c in classes.items() if c == "DIFF"]
        if diff_words:
            runs = []
            s = p = diff_words[0]
            for k in diff_words[1:]:
                if k == p + 1:
                    p = k
                else:
                    runs.append((s, p))
                    s = p = k
            runs.append((s, p))
            big = [(a, b) for a, b in runs if b - a >= 8]
            print(f"    DIFF clusters >=8 words: {len(big)} "
                  f"(largest {max(b-a+1 for a,b in runs)} words); "
                  f"first/last: {big[:3]}{'...' if len(big)>3 else ''}")


if __name__ == "__main__":
    main()
