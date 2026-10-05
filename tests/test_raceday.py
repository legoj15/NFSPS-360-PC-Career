"""Mid-race-day pair (Battle Machine, Nevada): GameplayData carries an
active race-day block at 0x2E0 whose 360 layout has a 4-byte pad at 0x314
that the PC layout lacks. Converted output must line up with native PC."""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent / "scripts" / "python"))

from nfssave import MC02, read_container
from nfssave.convert import ConversionReport, convert_payload
from nfssave.tree import Tree

ROOT = Path(__file__).parent.parent
R360 = ROOT / "docs/re/pair_raceday/CAREER_02_360"
RPC = ROOT / "docs/re/pair_raceday/CAREER_02_pc_native"
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


class BlueprintSetTests(unittest.TestCase):
    """Starter car blueprint sets in the race-day pair: paint words are two
    u16s; vinyl colour bytes (+0x574..0x628) stay natural."""

    def test_paint_and_colour_bytes(self):
        conv = [r.payload for r in Tree.parse(convert_payload(
            MC02.parse(read_container(R360).payload), ConversionReport()).tree,
            big=False).records if r.id == 0x47A07113][0]
        nat = [r.payload for r in Tree.parse(MC02.parse(RPC.read_bytes()).tree,
                                             big=False).records if r.id == 0x47A07113][0]
        for bs in (0x0, 0x7B4, 0xF68):
            R = 0x2680 + bs
            self.assertEqual(conv[R + 0x194:R + 0x198], nat[R + 0x194:R + 0x198], hex(bs))
        R = 0x2680
        for o in (0x1A0, 0x1AC):
            self.assertEqual(conv[R + o:R + o + 4].hex(), nat[R + o:R + o + 4].hex())
        nz = lambda b: [i for i, x in enumerate(b) if x]
        for o in (0x5A0, 0x5AC, 0x600, 0x604):   # values differ, layout must not
            self.assertEqual(nz(conv[R + o:R + o + 4]), nz(nat[R + o:R + o + 4]), hex(o))

    def test_decal_entry_layout(self):
        from nfssave.convert import convert_decal_entry
        # 360 Camaro vinyl entry -> u16 fields swapped, bytes 6..9 natural
        self.assertEqual(convert_decal_entry(bytes.fromhex("04f6006c0002c01b1b0006590000")).hex(),
                         "f6046c000200c01b1b0059060000")


class BlockEndTests(unittest.TestCase):
    def test_block_end_detection(self):
        from nfssave.convert import raceday_block_end
        cases = [(R360, 0x3E70), (ROOT / "docs/re/c1_latest/CAREER_01_360", 0xB5B0)]
        for src, want in cases:
            if not src.is_file():
                continue
            with self.subTest(source=src.name):
                p = [r.payload for r in Tree.parse(MC02.parse(
                    read_container(src).payload).tree, big=True).records
                     if r.id == GAMEPLAY][0]
                self.assertEqual(raceday_block_end(p), want)


class GameplayHashTests(unittest.TestCase):
    """GameplayData blob (PC payload 0x14, 0x10000 B) starts with
    MD5(blob[0x10:]); the PC deserializer rejects the blob otherwise."""

    def test_converted_blob_md5(self):
        import hashlib
        for src in (R360, ROOT / "docs/re/c1_latest/CAREER_01_360"):
            if not src.is_file():
                continue
            with self.subTest(source=src.name):
                p = _gp(Tree.parse(convert_payload(
                    MC02.parse(read_container(src).payload), ConversionReport()).tree,
                    big=False))
                blob = p[0x14:0x14 + 0x10000]
                self.assertEqual(blob[:16], hashlib.md5(blob[16:]).digest())


if __name__ == "__main__":
    unittest.main()
