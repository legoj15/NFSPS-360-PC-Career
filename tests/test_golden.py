"""Golden regression tests: source containers -> byte-exact verified output.

The golden MD5s are the files verified in-game on 2026-10-04 (career day 7,
money, repair markers all loaded correctly). Any diff here means the
converter changed behavior for the known-good inputs.

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
TWIN = Path(r"E:/GitHub/NFSPS360/user_data/B13EBABEBABEBABE/"
            r"45410822/00000001/CAREER_01/CAREER_01")
FLASH = Path("F:/Content/E00001CFFAB204C4/45410822/00000001")

# (source path, twin needed, golden md5)
CASES = [
    (ROOT / "Extracted/Career/CAREER_01", True,
     "841a2ead9306c57f56431c316c20997c"),
    (FLASH / "CAREER_02", False,
     "0261dd8f79ec0333cd2cbda7bbbc690d"),
    (FLASH / "CAREER_03", False,
     "0d70bf92e44e485ad94351c4ccc7947e"),
    (ROOT / "Extracted/Alias/ALIAS_JOSHUA S 10", False,
     "f725c9bb0748e34c9a6a9553bcae94a2"),
]


def _convert(src: Path, tmp: Path, twin: bytes | None) -> Path:
    mc02 = MC02.parse(read_container(src).payload)
    report = ConversionReport(source=str(src))
    pc = convert_payload(mc02, report, twin_payload=twin)
    return write_pc_save(pc, src.name, tmp)


class GoldenTests(unittest.TestCase):
    def test_golden_outputs(self):
        with tempfile.TemporaryDirectory() as tmp:
            out_root = Path(tmp)
            for src, needs_twin, golden in CASES:
                with self.subTest(source=src):
                    if not src.is_file():
                        self.skipTest(f"source not present: {src}")
                    twin = None
                    if needs_twin:
                        if not TWIN.is_file():
                            self.skipTest(f"re-save twin not present: {TWIN}")
                        twin = TWIN.read_bytes()
                    target = _convert(src, out_root, twin)
                    digest = hashlib.md5(target.read_bytes()).hexdigest()
                    self.assertEqual(digest, golden)

    def test_output_self_validates(self):
        """Every golden output re-parses with clean CRCs and PC framing."""
        with tempfile.TemporaryDirectory() as tmp:
            out_root = Path(tmp)
            for src, needs_twin, _ in CASES:
                with self.subTest(source=src):
                    if not src.is_file():
                        self.skipTest(f"source not present: {src}")
                    twin = TWIN.read_bytes() if needs_twin and TWIN.is_file() else None
                    target = _convert(src, out_root, twin)
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
