"""Re-save twin (--twin) recovery must convert a recovered record exactly
like the main loop does. The twin path used to run only convert_record on
it, skipping the GameplayData/RaceData u32 swap, the struct fixes (node
flags, career name, race-day block), the tail word and the GameplayData
MD5, so a twin-recovered GameplayData/RaceData/FECareer came out wrong.

Fixture: the tracked oracle with every record from GameplayData on
overwritten by 0xAA (trailing console damage too large to re-anchor), and
a pristine twin. Each recovered record must equal the same record from a
plain conversion of the twin itself.
"""
import struct
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent / "scripts/python"))

from nfssave import MC02, read_container
from nfssave.convert import ConversionReport, convert_payload
from nfssave.tree import Tree

ROOT = Path(__file__).parent.parent
SRC = ROOT / "docs/re/c1_latest/CAREER_01_360"
GAMEPLAY_ID = 0x3B309E09
REC_START_360 = 0x48


def build_pair(first_damaged_id: int):
    """(crafted, twin): the crafted source has every record from
    `first_damaged_id` to the end of the record region 0xAA-filled."""
    mc = MC02.parse(read_container(SRC).payload)
    tree = Tree.parse(mc.tree, big=True)
    twin = MC02(mc.endian, mc.extra, tree.build(True, mc.tree_size), mc.tree_size).to_bytes()
    k = next(i for i, r in enumerate(tree.records) if r.id == first_damaged_id)
    off = REC_START_360 + sum(12 + len(r.payload) for r in tree.records[:k])
    span = sum(12 + len(r.payload) for r in tree.records[k:])
    raw = bytearray(tree.build(True, mc.tree_size))
    raw[off:off + span] = b"\xAA" * span
    crafted = MC02(mc.endian, mc.extra, bytes(raw), mc.tree_size).to_bytes()
    assert Tree.parse(MC02.parse(crafted).tree, big=True).gap == span
    return crafted, twin, [r.id for r in tree.records[k:]]


class TwinRecoveryTests(unittest.TestCase):
    def test_recovered_records_convert_like_main_loop(self):
        crafted, twin, lost = build_pair(GAMEPLAY_ID)
        report = ConversionReport()
        merged = convert_payload(MC02.parse(crafted), report, twin_payload=twin)
        plain = convert_payload(MC02.parse(twin), ConversionReport())
        got = {r.id: r.payload for r in Tree.parse(merged.tree, big=False).records}
        want = {r.id: r.payload for r in Tree.parse(plain.tree, big=False).records}
        self.assertEqual(len(got), len(want))
        for rid in lost:
            with self.subTest(record=hex(rid)):
                self.assertEqual(got[rid], want[rid])
        self.assertTrue(any("GameplayData recovered from re-save twin" in w
                            for w in report.warnings), report.warnings)


if __name__ == "__main__":
    unittest.main()
