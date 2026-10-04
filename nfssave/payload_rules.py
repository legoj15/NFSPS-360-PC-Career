"""Payload conversion engines: 360 (BE) -> PC (LE) per chunk.

All record and nested-record sizes are multiples of 4, so payloads are u32-
tiled end to end and headers convert with the same pass as data.

Rule sources, in priority order:
  1. Fieldmap slot rules (research/fieldmaps_parsed.json) — derived from the
     matched 360/PC sample pair; positionally valid for the exact source
     save being converted. Class semantics (from research/match.py):
       NUM  wpc == swap(w360)  -> swap
       SAME wpc == w360 (raw)  -> copy bytes
       STR  printable both     -> copy bytes
       STR360 360 string, PC leaves fill/small-int -> copy 360 bytes
       DIFF any other          -> swap (player values differ)
       FILL garbage both sides -> swap (value-preserving)
  2. Auto mode for unmapped chunks: detect EA-encoded strings
     ([0x40|len][len-1 printable chars]) and keep those runs natural;
     everything else swaps per-u32.

The JSON is regenerated from research/fieldmaps/*.txt by `regenerate_rules()`
(handle both per-slot rows and run-length rows).
"""

import json
import re
from pathlib import Path

RESEARCH = Path(__file__).parent.parent / "research"
RULES_PATH = RESEARCH / "fieldmaps_parsed.json"
COPY_CLASSES = ("STR", "STR360", "SAME")

_SLOT_RE = re.compile(
    r"^\s*(\d+)\s+(0x[0-9A-Fa-f]+)\s+(0x[0-9A-Fa-f]+)\s+(\w+)\s+([0-9a-f]+)\s+([0-9a-f]+)")
_RUN_RE = re.compile(
    r"run: slots \d+\.\.\d+\s+off360 (0x[0-9A-Fa-f]+)\.\.(0x[0-9A-Fa-f]+)\s+"
    r"offPC (0x[0-9A-Fa-f]+)\.\.(0x[0-9A-Fa-f]+)\s+class=(\w+)\s+len=(\d+)")
_NAME_RE = re.compile(r"^(alias|career)_(?:.*?)([0-9A-Fa-f]{8})(?:_big)?$")


def regenerate_rules() -> dict:
    """Parse research/fieldmaps/*.txt into fieldmaps_parsed.json."""
    rules: dict = {"alias": {}, "career": {}}
    for f in sorted((RESEARCH / "fieldmaps").glob("*.txt")):
        m = _NAME_RE.search(f.stem)
        if not m:
            continue
        kind, cid = m.group(1), int(m.group(2), 16)
        acts: dict[int, str] = {}
        ref_size = None
        for line in f.read_text().splitlines():
            mm = _NAME_RE.search(line)
            if line.startswith("360:") and ref_size is None:
                sm = re.search(r"size=0x([0-9A-Fa-f]+)", line)
                if sm:
                    ref_size = int(sm.group(1), 16)
            rm = _RUN_RE.search(line)
            if rm:
                off = int(rm.group(1), 16)
                end = int(rm.group(2), 16)
                cls = rm.group(5)
                for o in range(off, end, 4):
                    acts[o] = cls
                continue
            sm = _SLOT_RE.match(line)
            if sm and sm.group(4) not in ("off360",):
                acts[int(sm.group(2), 16)] = sm.group(4)
        if acts:
            rules[kind][cid] = {"acts": acts, "ref_size": ref_size}
    RULES_PATH.write_text(json.dumps(rules))
    return rules


def _load_rules() -> tuple[dict, bool]:
    if not (RESEARCH / "fieldmaps").is_dir():
        return {"alias": {}, "career": {}}, False
    # regenerate from source .txt unless a current v2 JSON is present
    if RULES_PATH.exists():
        try:
            data = json.loads(RULES_PATH.read_text())
            if "alias" in data and "career" in data:
                out = {}
                for kind in ("alias", "career"):
                    out[kind] = {}
                    for cid, v in data[kind].items():
                        v["acts"] = {int(o): cls for o, cls in v["acts"].items()}
                        out[kind][int(cid)] = v
                return out, True
        except Exception:
            pass
    try:
        return regenerate_rules(), True
    except Exception:
        return {"alias": {}, "career": {}}, False


def find_string_runs(payload: bytes) -> list[tuple[int, int]]:
    """Byte ranges to keep in natural order.

    Two detectors:
      a) EA-encoded strings: [0x40|len][len-1 printable chars], len >= 4.
      b) NUL-terminated C strings / fixed-width name fields: a printable run
         of >= 4 chars extended through NUL/0xAA/0x00 fill (zero and 0xAA
         words are palindromic under u32 swap, so extending through them is
         lossless; observed e.g. 'MV09_Nitrocide\\0\\0ion\\0' + 0xAA fill).
    """
    runs = []
    i = 0
    n = len(payload)
    while i < n:
        b = payload[i]
        if 0x44 <= b <= 0x7F:
            ln = b & 0x3F
            if ln >= 4 and i + ln <= n:
                cand = payload[i + 1:i + ln]
                if all(0x20 <= c < 0x7F for c in cand):
                    runs.append((i, i + ln))
                    i += ln
                    continue
        i += 1
    # field scan
    i = 0
    while i < n:
        # start: >=4 printable
        j = i
        while j < n and 0x20 <= payload[j] < 0x7F:
            j += 1
        if j - i >= 4:
            # extend through NUL/0xAA/0x00 fill and further printable runs
            k = j
            while k < n:
                c = payload[k]
                if c in (0x00, 0xAA):
                    k += 1
                elif 0x20 <= c < 0x7F:
                    m2 = k
                    while m2 < n and 0x20 <= payload[m2] < 0x7F:
                        m2 += 1
                    if m2 - k >= 4:
                        k = m2
                    else:
                        break
                else:
                    break
            end = (k + 3) & ~3
            runs.append((i, min(end, n)))
            i = end
            continue
        i = j + 1 if j > i else i + 1
    merged = []
    for s, e in sorted(set(runs)):
        if merged and s <= merged[-1][1]:
            merged[-1] = (merged[-1][0], max(e, merged[-1][1]))
        else:
            merged.append((s, e))
    return merged


def _swap_tiled(payload: bytes, natural_ranges: list[tuple[int, int]]) -> bytes:
    out = bytearray(payload)
    for i in range(0, len(payload) - 3, 4):
        out[i:i + 4] = payload[i:i + 4][::-1]
    for s, e in natural_ranges:
        out[s:e] = payload[s:e]
    return bytes(out)


def _subword_garbage(word: bytes) -> bool:
    """True for [value byte][FF FF FF] words: a sub-word (u8) field with
    uninitialized tail bytes, serialized in natural byte order on both
    platforms (observed: 360 '01 ff ff ff' <-> PC '01 00 5d 00'). The
    meaningful byte stays at offset 0; swapping would move it to the end.
    """
    return word[0] != 0xFF and word[1:] == b"\xff\xff\xff"


def convert_payload_auto(payload: bytes) -> bytes:
    subwords = [(i, i + 4) for i in range(0, len(payload) - 3, 4)
                if _subword_garbage(payload[i:i + 4])]
    return _swap_tiled(payload, subwords + find_string_runs(payload))


def convert_payload_mapped(payload: bytes, acts: dict) -> bytes:
    out = bytearray(payload)
    for off in range(0, len(payload) - 3, 4):
        cls = acts.get(off)
        if cls in COPY_CLASSES:
            continue
        word = payload[off:off + 4]
        if cls == "DIFF" and _subword_garbage(word):
            continue  # keep the value byte in place; tail is garbage anyway
        out[off:off + 4] = word[::-1]
    return bytes(out)


def convert_record(kind: str, rec, warnings: list | None = None) -> str:
    """Convert a Record's payload in place (BE -> LE). Returns mode used."""
    entry = RULES.get(kind, {}).get(rec.id)
    if entry:
        acts = entry["acts"]
        ref = entry.get("ref_size")
        if ref is not None and ref != len(rec.payload):
            # positional rules only apply to same-shape payloads; a size
            # mismatch means variable content (different player) — auto mode
            if warnings is not None:
                warnings.append(
                    f"chunk {rec.id:#x} size {len(rec.payload):#x} != map reference {ref:#x}; "
                    "converted in auto mode (positional rules unsafe)")
            rec.payload = convert_payload_auto(rec.payload)
            rec.children = []
            return "auto"
        rec.payload = convert_payload_mapped(rec.payload, acts)
        mode = "mapped"
    else:
        rec.payload = convert_payload_auto(rec.payload)
        mode = "auto"
    rec.children = []
    return mode


RULES, RULES_LOADED = _load_rules()
