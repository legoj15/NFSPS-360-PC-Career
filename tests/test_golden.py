"""Golden regression tests: source containers -> byte-exact verified output.

Regression pins for the current converter (2026-10-04 evening: STFS block
map, node flag words, u16 car part slots). NOT yet verified in-game - see
docs/HANDOFF.md. Any diff here means the converter changed behavior.

Run:  python -m unittest discover tests
"""

import hashlib
import tempfile
import unittest
from pathlib import Path

import sys
sys.path.insert(0, str(Path(__file__).parent.parent))

from nfssave import MC02, read_container
from nfssave.convert import ConversionReport, convert_payload, write_pc_save

ROOT = Path(__file__).parent.parent
PAIR_360 = ROOT / "research/pair/CAREER_02_360_fresh"

# (source path, golden md5)
CASES = [
    (ROOT / "Extracted/Career/CAREER_01", "7b7e1893047b01b00f7037ef54ceca44"),
    (PAIR_360, "8dd15c6cb5736cf14aa2694289d8480d"),
    (ROOT / "Extracted/Career/CAREER_03", "0dcfed80eab3dfcc246499b07ae54c37"),
    (ROOT / "Extracted/Alias/ALIAS_JOSHUA S 10", "578a10cb583785bb6cb00fa64bc69439"),
    (ROOT / "research/c1_latest/CAREER_01_360", "718b6b6b8494decde59eb6b1defcc01d"),
    (ROOT / "research/pair_raceday/CAREER_02_360", "2bb7d00963509e71d6eaeccbed496b65"),
]


def _convert(src: Path, tmp: Path) -> Path:
    mc02 = MC02.parse(read_container(src).payload)
    report = ConversionReport(source=str(src))
    pc = convert_payload(mc02, report)
    return write_pc_save(pc, src.name, tmp)


class GoldenTests(unittest.TestCase):
    def test_golden_outputs(self):
        with tempfile.TemporaryDirectory() as tmp:
            out_root = Path(tmp)
            for src, golden in CASES:
                with self.subTest(source=src):
                    if not src.is_file():
                        self.skipTest(f"source not present: {src}")
                    target = _convert(src, out_root)
                    digest = hashlib.md5(target.read_bytes()).hexdigest()
                    self.assertEqual(digest, golden)

    def test_output_self_validates(self):
        """Every golden output re-parses with clean CRCs and PC framing."""
        with tempfile.TemporaryDirectory() as tmp:
            out_root = Path(tmp)
            for src, _ in CASES:
                with self.subTest(source=src):
                    if not src.is_file():
                        self.skipTest(f"source not present: {src}")
                    target = _convert(src, out_root)
                    m = MC02.parse(target.read_bytes())
                    self.assertEqual(m.check(), [], "CRC failures in output")
                    from nfssave.tree import Tree
                    t = Tree.parse(m.tree, big=False)
                    self.assertGreater(len(t.records), 0)
                    for r in t.records:
                        self.assertEqual(r.type & 0xFF, 1,
                                         f"record {r.id:#x} lost PC flags byte")


if __name__ == "__main__":
    unittest.main()
