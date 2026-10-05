"""STFS container reader: payloads must be read through the block map.

The CON files are standard STFS packages (two hash-table copies per level).
A hash-table group sits after every 170 data blocks, so a career payload
(183 blocks) straddles one. Reading the payload as one contiguous slice
pulls hash tables into the save and loses FECareer's tail plus the
CustomRaceDayMemcard/UnlockSystem chunks — the "damaged console tail".
"""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from nfssave import MC02, read_container
from nfssave.container360 import stfs_block_offset
from nfssave.tree import Tree

ROOT = Path(__file__).parent.parent
FLASH = Path("F:/Content/E00001CFFAB204C4/45410822/00000001")

CAREERS = [
    ROOT / "Extracted/Career/CAREER_01",
    ROOT / "research/pair/CAREER_02_360_fresh",
    FLASH / "CAREER_03",
]
CAREER_TAIL = [0x885B4DDC, 0xD548266C, 0xCA269650]  # FECareer, CustomRaceDay, Unlock


class BlockMapTests(unittest.TestCase):
    def test_block_offsets_two_table_package(self):
        # first hash table at 0xA000; data block 0 (file table) at 0xC000
        self.assertEqual(stfs_block_offset(0, 0xA000, 1), 0xC000)
        self.assertEqual(stfs_block_offset(169, 0xA000, 1), 0xB5000)
        # level-0 + level-1 tables (two copies each) precede block 170
        self.assertEqual(stfs_block_offset(170, 0xA000, 1), 0xBA000)
        self.assertEqual(stfs_block_offset(340, 0xA000, 1), 0x166000)


class CareerPayloadTests(unittest.TestCase):
    def test_career_payloads_complete(self):
        for src in CAREERS:
            with self.subTest(source=src.name):
                if not src.is_file():
                    self.skipTest(f"source not present: {src}")
                mc02 = MC02.parse(read_container(src).payload)
                self.assertEqual(mc02.check(), [], "MC02 CRCs must validate")
                tree = Tree.parse(mc02.tree, big=True)
                self.assertEqual(tree.gap, 0)
                self.assertEqual([r.id for r in tree.records][-3:], CAREER_TAIL)


if __name__ == "__main__":
    unittest.main()
