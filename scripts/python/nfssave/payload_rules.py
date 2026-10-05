"""Payload conversion engines: 360 (BE) -> PC (LE) per chunk.

All record and nested-record sizes are multiples of 4, so payloads are u32-
tiled end to end and headers convert with the same pass as data.

Rule sources, in priority order:
  1. Fieldmap slot rules (fieldmaps_parsed.json, packaged next to this
     module) — derived from the matched 360/PC sample pair; positionally
     valid only where the payload layout is rigid. Class semantics (from
     docs/re/match.py in the repo):
       NUM  wpc == swap(w360)  -> swap
       SAME wpc == w360 (raw)  -> copy bytes
       STR  printable both     -> copy bytes
       STR360 360 string, PC leaves fill/small-int -> copy 360 bytes
       DIFF any other          -> swap (player values differ)
       FILL garbage both sides -> swap (value-preserving)
     A payload whose size differs from the map reference, or whose EA-string
     positions disagree with the map's copy/swap slots, falls back to auto
     mode (the map was derived from a different session's content).
  2. Auto mode for unmapped chunks: detect string regions and keep them in
     natural byte order; swap every other u32; keep [value][FF FF FF]
     sub-word fields natural. String regions are quantized to the u32 grid
     so no word ends up half-swapped/half-natural.

The JSON is regenerated from the fieldmap sources (docs/re/fieldmaps/*.txt)
by `regenerate_rules(fieldmaps_dir)`; regeneration is explicit, never done
at import time.
"""

import json
import re
from pathlib import Path

RULES_PATH = Path(__file__).parent / "fieldmaps_parsed.json"
COPY_CLASSES = ("STR", "STR360", "SAME")

_SLOT_RE = re.compile(
    r"^\s*(\d+)\s+(0x[0-9A-Fa-f]+)\s+(0x[0-9A-Fa-f]+)\s+(\w+)\s+([0-9a-f]+)\s+([0-9a-f]+)")
_RUN_RE = re.compile(
    r"run: slots \d+\.\.\d+\s+off360 (0x[0-9A-Fa-f]+)\.\.(0x[0-9A-Fa-f]+)\s+"
    r"offPC (0x[0-9A-Fa-f]+)\.\.(0x[0-9A-Fa-f]+)\s+class=(\w+)\s+len=(\d+)")
_NAME_RE = re.compile(r"^(alias|career)_(?:.*?)([0-9A-Fa-f]{8})(?:_big)?$")


def regenerate_rules(fieldmaps_dir: Path) -> dict:
    """Parse <fieldmaps_dir>/*.txt into the packaged fieldmaps_parsed.json."""
    rules: dict = {"alias": {}, "career": {}}
    for f in sorted(fieldmaps_dir.glob("*.txt")):
        m = _NAME_RE.search(f.stem)
        if not m:
            continue
        kind, cid = m.group(1), int(m.group(2), 16)
        acts: dict[int, str] = {}
        ref_size = None
        for line in f.read_text().splitlines():
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
    if not RULES_PATH.exists():
        return {"alias": {}, "career": {}}, False
    try:
        data = json.loads(RULES_PATH.read_text())
        out = {}
        for kind in ("alias", "career"):
            out[kind] = {}
            for cid, v in data[kind].items():
                v["acts"] = {int(o): cls for o, cls in v["acts"].items()}
                out[kind][int(cid)] = v
        return out, True
    except Exception:
        return {"alias": {}, "career": {}}, False


def _printable(b: int) -> bool:
    return 0x20 <= b < 0x7F


def _ea_string_ranges(payload: bytes, min_len: int = 5) -> list[tuple[int, int]]:
    """EA-encoded strings: [0x40|len][len-1 printable chars].

    min_len trades recall for precision: a 4-byte detection (3 printable
    chars after a header byte) is statistically indistinguishable from a
    numeric word whose bytes happen to be printable (verified on the
    matched pair: 'D#7f' <-> 'f7#D' is a plain BE/LE u32), so the default
    only accepts >= 5. Callers that need high confidence (map
    cross-checks) pass min_len=8.
    """
    runs = []
    i = 0
    n = len(payload)
    while i < n:
        b = payload[i]
        if 0x40 + min_len <= b <= 0x7F:
            ln = b & 0x3F
            if ln >= min_len and i + ln <= n:
                if all(_printable(c) for c in payload[i + 1:i + ln]):
                    runs.append((i, i + ln))
                    i += ln
                    continue
        i += 1
    return runs


def _fixed_string_ranges(payload: bytes) -> list[tuple[int, int]]:
    """Fixed-width C-string fields: printable text in a NUL/0xAA-padded slot.

    Accepts a printable run of >= 5 chars that is followed by at least two
    fill bytes (0x00/0xAA) or the end of the payload; runs may bridge
    through fill into a further printable segment of >= 4 chars (observed:
    car blueprint fields like 'MV09_Nitrocide\\0\\0ion\\0' + 0xAA fill).
    The trailing-fill requirement rejects lone NUL bytes inside numeric
    data, which caused most false positives.
    """
    runs = []
    i = 0
    n = len(payload)
    while i < n:
        j = i
        while j < n and _printable(payload[j]):
            j += 1
        if j - i >= 5:
            k = j
            last_text_end = j
            while k < n:
                c = payload[k]
                if c in (0x00, 0xAA):
                    k += 1
                elif _printable(c):
                    m2 = k
                    while m2 < n and _printable(payload[m2]):
                        m2 += 1
                    if m2 - k >= 4:
                        k = m2
                        last_text_end = m2
                    else:
                        break
                else:
                    break
            fill_after = k - last_text_end
            if k >= n or fill_after >= 2:
                runs.append((i, k))
                i = k
                continue
        i = j + 1 if j > i else i + 1
    return runs


def _merge(ranges: list[tuple[int, int]]) -> list[tuple[int, int]]:
    merged: list[list[int]] = []
    for s, e in sorted(set(ranges)):
        if merged and s <= merged[-1][1]:
            merged[-1][1] = max(merged[-1][1], e)
        else:
            merged.append([s, e])
    return [(s, e) for s, e in merged]


def _quantize(ranges: list[tuple[int, int]]) -> list[tuple[int, int]]:
    """Snap range boundaries to the u32 grid and re-merge.

    Conversion decisions are per-word (a word is copied whole or swapped
    whole); without this, a string starting mid-word leaves that word
    half-swapped/half-natural.
    """
    return _merge([(s & ~3, (e + 3) & ~3) for s, e in ranges])


def find_string_runs(payload: bytes) -> list[tuple[int, int]]:
    """Word-aligned byte ranges of payload to keep in natural order."""
    return _quantize(_ea_string_ranges(payload) + _fixed_string_ranges(payload))


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


def convert_payload_auto(payload: bytes, warnings: list | None = None,
                         label: str = "chunk") -> bytes:
    raw = _ea_string_ranges(payload) + _fixed_string_ranges(payload)
    if warnings is not None:
        n_unaligned = sum(1 for s, e in raw if s % 4 or e % 4)
        if n_unaligned:
            warnings.append(
                f"{label}: {n_unaligned} string run(s) not word-aligned; "
                "padded to word grid (leading bytes are format padding)")
    subwords = [(i, i + 4) for i in range(0, len(payload) - 3, 4)
                if _subword_garbage(payload[i:i + 4])]
    return _swap_tiled(payload, subwords + _quantize(raw))


def string_word_set(payload: bytes) -> set[int]:
    """Word offsets covered by a detected string region (word-quantized)."""
    words = set()
    for s, e in find_string_runs(payload):
        words.update(range(s, e, 4))
    return words


def convert_payload_mapped(payload: bytes, acts: dict) -> bytes:
    """Apply per-word fieldmap classes.

    COPY classes and natural-order SAME words (equal and non-palindrome on
    the reference pair) copy. NUM words swap. ZERO and DIFF words were
    empty or volatile in the reference pair - they say nothing about byte
    order and a richer save may hold real content there - so they convert
    with the auto grammar: swap unless the word sits in a detected string
    region or is a sub-word field.
    """
    str_words = string_word_set(payload)
    out = bytearray(payload)
    for off in range(0, len(payload) - 3, 4):
        cls = acts.get(off)
        if cls in COPY_CLASSES:
            continue
        word = payload[off:off + 4]
        if cls in ("DIFF", "ZERO"):
            if off in str_words or _subword_garbage(word):
                continue
            out[off:off + 4] = word[::-1]
            continue
        out[off:off + 4] = word[::-1]
    return bytes(out)


def convert_record(kind: str, rec, warnings: list | None = None) -> str:
    """Convert a Record's payload in place (BE -> LE). Returns mode used."""
    label = f"chunk {rec.id:#x}"
    entry = RULES.get(kind, {}).get(rec.id)
    if entry:
        ref = entry.get("ref_size")
        if ref is not None and ref != len(rec.payload):
            if warnings is not None:
                warnings.append(
                    f"{label} size {len(rec.payload):#x} != map reference {ref:#x}; "
                    "converted in auto mode (positional rules unsafe)")
            rec.payload = convert_payload_auto(rec.payload, warnings, label)
            return "auto"
        rec.payload = convert_payload_mapped(rec.payload, entry["acts"])
        return "mapped"
    rec.payload = convert_payload_auto(rec.payload, warnings, label)
    return "auto"


RULES, RULES_LOADED = _load_rules()
