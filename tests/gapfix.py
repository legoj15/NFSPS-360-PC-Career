"""Gapped-career fixture builder shared by the tests.

build_gapped() damages the tracked oracle the way the console's record-tail
bug does: every record from `first_damaged_id` to the end of the record
region is overwritten with 0xAA, so Tree.parse reports a trailing `gap`.
With `damaged_count` only that many records are overwritten, leaving clean
records after the noise: Tree.parse then re-anchors (internal gap).
"""
import sys
from pathlib import Path

ROOT = Path(__file__).parent.parent
sys.path.insert(0, str(ROOT / "scripts/python"))

from nfssave import MC02, read_container
from nfssave.tree import Tree

SRC = ROOT / "docs/re/c1_latest/CAREER_01_360"
REC_START_360 = 0x48


def build_gapped(first_damaged_id: int | None = None, damaged_count: int | None = None,
                 post_word: bytes | None = None):
    """(gapped, pristine, lost_ids): MC02 payload bytes. `pristine` is a clean
    rebuild of the oracle tree; `gapped` has `damaged_count` records from
    `first_damaged_id` (default: the last record) overwritten. With the default
    count (all the way to the end) the gap is trailing; with fewer, the clean
    records after the noise are re-anchored (internal gap). `lost_ids` are the
    ids of the damaged records. `post_word` replaces the (all-zero) word after
    the record area in both outputs, so the last record's spill is observable."""
    mc = MC02.parse(read_container(SRC).payload)
    tree = Tree.parse(mc.tree, big=True)
    if post_word is not None:
        tree.post = post_word + tree.post[4:]
    pristine = MC02(mc.endian, mc.extra, tree.build(True, mc.tree_size), mc.tree_size).to_bytes()
    k = (len(tree.records) - 1 if first_damaged_id is None
         else next(i for i, r in enumerate(tree.records) if r.id == first_damaged_id))
    stop = len(tree.records) if damaged_count is None else k + damaged_count
    off = REC_START_360 + sum(12 + len(r.payload) for r in tree.records[:k])
    hit = sum(12 + len(r.payload) for r in tree.records[k:stop])
    span = sum(12 + len(r.payload) for r in tree.records[k:])  # parser's gap: noise to the end
    raw = bytearray(tree.build(True, mc.tree_size))
    raw[off:off + hit] = bytes([0xAA]) * hit
    gapped = MC02(mc.endian, mc.extra, bytes(raw), mc.tree_size).to_bytes()
    parsed = Tree.parse(MC02.parse(gapped).tree, big=True)
    assert parsed.gap == span
    if stop < len(tree.records):
        # internal gap: the clean records after the noise were re-anchored
        assert parsed.gap_at == k and len(parsed.records) == len(tree.records) - (stop - k)
    return gapped, pristine, [r.id for r in tree.records[k:stop]]
