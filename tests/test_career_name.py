"""FECareer career-slot name node: [0][len 0x24][flag][4 junk][32 chars].

The PC names the career file after it (CAREER_<name>). The 360 writes
"01\\0" + 0xAA heap fill; the generic u32 pass swapped it to
"\\xaa\\0" "10", so the PC saved every converted career as CAREER_<0xAA>
("CAREER_ª"). A native PC career (fresh, user's TEST2 2026-10-09) holds
"01" followed by zeros.
"""
import struct
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent / "scripts/python"))

from nfssave import MC02, read_container
from nfssave.convert import ConversionReport, convert_payload, node_spans
from nfssave.tree import Tree

ROOT = Path(__file__).parent.parent
FECAREER_ID = 0x885B4DDC
SAMPLES = [
    (ROOT / "docs/re/c1_latest/CAREER_01_360", b"01"),
    (ROOT / "docs/re/pair_raceday/CAREER_02_360", b"02"),
    (ROOT / "docs/re/pair/CAREER_02_360_fresh", b"02"),
]


def fecareer(tree: bytes, big: bool) -> bytes:
    return next(r.payload for r in Tree.parse(tree, big=big).records if r.id == FECAREER_ID)


class CareerNameTests(unittest.TestCase):
    def test_name_node_is_text(self):
        for path, name in SAMPLES:
            with self.subTest(sample=path.name):
                mc = MC02.parse(read_container(path).payload)
                src = fecareer(mc.tree, big=True)
                (d, ln), = [(d, ln) for d, ln in node_spans(src) if ln == 0x24]
                pc = fecareer(convert_payload(mc, ConversionReport()).tree, big=False)
                o = d - 4                      # PC payload drops the 360 marker word
                self.assertEqual(pc[o:o + 4], src[d:d + 4])           # junk word natural
                self.assertEqual(pc[o + 4:o + 0x24], name + bytes(32 - len(name)))


if __name__ == "__main__":
    unittest.main()
