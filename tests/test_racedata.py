"""RaceData is a u32/float table (race results: track keys, times) with no
strings. Its fieldmap came from fresh careers, so every slot empty there
(ZERO/DIFF) fell back to the string heuristic, which left real race times
big-endian whenever their bytes looked like ASCII (0x42724630 = 60.57 s
reads "BrF0"). On the PC that broke the race HUD (no speedometer or
leaderboard, wrong camera); a native RaceData fixed it in-game
(2026-10-09, test 6).
"""
import struct
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent / "scripts/python"))

from nfssave import MC02, read_container
from nfssave.convert import ConversionReport, convert_payload, fix_node_flags
from nfssave.tree import Tree, swap_u32s

ROOT = Path(__file__).parent.parent
RACEDATA_ID = 0x51A41B14
SAMPLES = [
    ROOT / "docs/re/c1_latest/CAREER_01_360",
    ROOT / "docs/re/pair_raceday/CAREER_02_360",
    ROOT / "docs/re/pair/CAREER_02_360_fresh",
]


def racedata(tree: bytes, big: bool):
    return next(r for r in Tree.parse(tree, big=big).records if r.id == RACEDATA_ID)


class RaceDataTests(unittest.TestCase):
    def test_race_time_swapped(self):
        mc = MC02.parse(read_container(SAMPLES[0]).payload)
        pc = racedata(convert_payload(mc, ConversionReport()).tree, big=False).payload
        self.assertAlmostEqual(struct.unpack_from("<f", pc, 0x30)[0], 60.569, places=3)
        self.assertAlmostEqual(struct.unpack_from("<f", pc, 0x12AC)[0], 60.069, places=3)

    def test_every_word_numeric(self):
        for path in SAMPLES:
            with self.subTest(sample=path.name):
                mc = MC02.parse(read_container(path).payload)
                src = racedata(mc.tree, big=True).payload
                want = bytearray(swap_u32s(src))
                fix_node_flags(src, want)
                pc = racedata(convert_payload(mc, ConversionReport()).tree, big=False).payload
                self.assertEqual(pc[:-4], bytes(want[4:]))


if __name__ == "__main__":
    unittest.main()
