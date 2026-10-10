"""Gapped-career fixture builder shared by the tests.

build_gapped() damages the tracked oracle the way the console's record-tail
bug does: every record from `first_damaged_id` to the end of the record
region is overwritten with 0xAA, so Tree.parse reports a trailing `gap`.
"""
import sys
from pathlib import Path

ROOT = Path(__file__).parent.parent
sys.path.insert(0, str(ROOT / "scripts/python"))

from nfssave import MC02, read_container
from nfssave.tree import Tree

SRC = ROOT / "docs/re/c1_latest/CAREER_01_360"
REC_START_360 = 0x48


def build_gapped(first_damaged_id: int | None = None):
    """(gapped, pristine, lost_ids): MC02 payload bytes. `pristine` is a clean
    rebuild of the oracle tree; `gapped` has every record from
    `first_damaged_id` (default: the last record) damaged. `lost_ids` are the
    ids of the damaged records."""
    mc = MC02.parse(read_container(SRC).payload)
    tree = Tree.parse(mc.tree, big=True)
    pristine = MC02(mc.endian, mc.extra, tree.build(True, mc.tree_size), mc.tree_size).to_bytes()
    k = (len(tree.records) - 1 if first_damaged_id is None
         else next(i for i, r in enumerate(tree.records) if r.id == first_damaged_id))
    off = REC_START_360 + sum(12 + len(r.payload) for r in tree.records[:k])
    span = sum(12 + len(r.payload) for r in tree.records[k:])
    raw = bytearray(tree.build(True, mc.tree_size))
    raw[off:off + span] = b"\xAA" * span
    gapped = MC02(mc.endian, mc.extra, bytes(raw), mc.tree_size).to_bytes()
    assert Tree.parse(MC02.parse(gapped).tree, big=True).gap == span
    return gapped, pristine, [r.id for r in tree.records[k:]]
