"""Matched-state pair: converting the fresh 360 CAREER_02 must reproduce the
PC-native CAREER_02 (same career state, saved on both platforms) wherever
the data is not volatile.

Checks:
  * property-node flag words keep the flag byte first (natural order);
  * the starter car's customization part arrays (u16 slots) match byte-exact.
"""

import struct
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent))

from nfssave import MC02, read_container
from nfssave.convert import ConversionReport, convert_payload
from nfssave.tree import Tree

ROOT = Path(__file__).parent.parent
PAIR_360 = ROOT / "research/pair/CAREER_02_360_fresh"
PAIR_PC = ROOT / "research/pair/CAREER_02_pc_native"
NODE_CHUNKS = [0x328C6431, 0xDC6B027F, 0xB67F6CC6, 0x51A41B14, 0x885B4DDC,
               0xD548266C, 0xCA269650]
CARDB = 0x47A07113
CAR0_PARTS = (0x2680 + 0x3C, 0x2680 + 0x190)


def _records(tree: Tree) -> dict:
    return {r.id: r.payload for r in tree.records}


@unittest.skipUnless(PAIR_360.is_file() and PAIR_PC.is_file(), "pair samples absent")
class PairTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        mc = MC02.parse(read_container(PAIR_360).payload)
        conv = convert_payload(mc, ConversionReport())
        cls.conv = _records(Tree.parse(conv.tree, big=False))
        cls.native = _records(Tree.parse(MC02.parse(PAIR_PC.read_bytes()).tree, big=False))

    def test_node_flag_bytes(self):
        for cid in NODE_CHUNKS:
            with self.subTest(chunk=hex(cid)):
                a, b = self.conv[cid], self.native[cid]
                bad = []
                # flag word follows each [u32 0][u32 len] node header
                for o in range(8, len(b) - 4, 4):
                    zero, ln = struct.unpack_from("<II", b, o - 8)
                    if zero == 0 and ln == 4 and b[o] in (0x00, 0x01, 0xFF):
                        if a[o] != b[o]:
                            bad.append(hex(o))
                self.assertEqual(bad[:10], [], f"{len(bad)} flag bytes misplaced")

    def test_car_table_slot_bytes(self):
        """Car-table entries end in [u8][u8][u8][pad] (garage slot/index);
        owned-car entries must keep them natural."""
        a, b = self.conv[CARDB], self.native[CARDB]
        for k in (114, 150, 190):  # catalog entries identical on both platforms
            o = 0x14 + 24 * k + 20
            self.assertEqual(a[o:o + 3].hex(), b[o:o + 3].hex(), f"entry {k}")

    def test_all_blueprint_part_sets(self):
        """Each car record holds three customization sets 0x7B4 apart."""
        a, b = self.conv[CARDB], self.native[CARDB]
        for setoff in (0x7B4, 0xF68):
            s, e = 0x2680 + setoff + 0x3C, 0x2680 + setoff + 0x186
            self.assertEqual(a[s:e].hex(), b[s:e].hex(), f"set +{setoff:#x}")

    def test_starter_car_parts(self):
        s, e = CAR0_PARTS
        self.assertEqual(self.conv[CARDB][s:e].hex(), self.native[CARDB][s:e].hex())


if __name__ == "__main__":
    unittest.main()
