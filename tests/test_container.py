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

sys.path.insert(0, str(Path(__file__).parent.parent / "scripts" / "python"))

from nfssave import MC02, read_container
from nfssave.container360 import stfs_block_offset
from nfssave.tree import Tree

ROOT = Path(__file__).parent.parent
FLASH = ROOT / "Extracted/Career"

CAREERS = [
    ROOT / "Extracted/Career/CAREER_01",
    ROOT / "docs/re/pair/CAREER_02_360_fresh",
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


def short_table(length: int) -> bytes:
    """The pair CON cut `length` bytes into its file-table block, with the
    entry's block count and size zeroed (an empty file). Same fixture in
    test_container.rs and Run-Tests.ps1."""
    data =bytearray((ROOT / "docs/re/pair/CAREER_02_360_fresh").read_bytes())
    first_table = (int.from_bytes(data[0x340:0x344], "big") + 0xFFF) & ~0xFFF
    block = int.from_bytes(data[0x37E:0x381], "little")
    off = stfs_block_offset(block, first_table, 0 if data[0x37B] & 1 else 1)
    data[off + 0x29:off + 0x2C] = bytes(3)
    data[off + 0x34:off + 0x38] = bytes(4)
    return bytes(data[:off + length])


class ShortFileTableTests(unittest.TestCase):
    """Every field read sits in the entry's first 0x38 bytes: a table block
    of 0x38..0x3F bytes parses, a shorter one is refused with ValueError
    (all three ports)."""

    def test_table_block_of_0x3c_bytes_parses(self):
        from nfssave.container360 import parse_container
        c = parse_container(short_table(0x3C), "t")
        self.assertEqual((c.name, c.payload), ("CAREER_02", b""))

    def test_table_block_of_0x30_bytes_is_refused(self):
        from nfssave.container360 import parse_container
        with self.assertRaisesRegex(ValueError, "STFS file table block truncated"):
            parse_container(short_table(0x30), "t")


if __name__ == "__main__":
    unittest.main()
