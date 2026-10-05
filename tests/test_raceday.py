"""Mid-race-day pair (Battle Machine, Nevada): GameplayData carries an
active race-day block at 0x2E0 whose 360 layout has a 4-byte pad at 0x314
that the PC layout lacks. Converted output must line up with native PC."""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from nfssave import MC02, read_container
from nfssave.convert import ConversionReport, convert_payload
from nfssave.tree import Tree

ROOT = Path(__file__).parent.parent
R360 = ROOT / "research/pair_raceday/CAREER_02_360"
RPC = ROOT / "research/pair_raceday/CAREER_02_pc_native"
GAMEPLAY = 0x3B309E09


def _gp(tree: Tree) -> bytes:
    return [r.payload for r in tree.records if r.id == GAMEPLAY][0]


@unittest.skipUnless(R360.is_file() and RPC.is_file(), "raceday pair absent")
class RaceDayTests(unittest.TestCase):
    def test_raceday_block_aligned(self):
        conv = _gp(Tree.parse(convert_payload(
            MC02.parse(read_container(R360).payload), ConversionReport()).tree, big=False))
        nat = _gp(Tree.parse(MC02.parse(RPC.read_bytes()).tree, big=False))
        # event flag entries after the 360 pad, the name string before it,
        # and a u32 near the block end (physics floats differ in noise bits)
        for o in (0x300, 0x434, 0x444, 0x4D4, 0x77C, 0x34D4):
            self.assertEqual(conv[o:o + 4].hex(), nat[o:o + 4].hex(), hex(o))


if __name__ == "__main__":
    unittest.main()
