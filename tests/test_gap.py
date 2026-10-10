"""A career whose console record tail is damaged (Tree.gap != 0) converts
without any recovery source, warns about the gap, and the output is pinned
(same digest as the Rust test_gap.rs)."""
import hashlib
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from gapfix import build_gapped
from nfssave import MC02
from nfssave.convert import ConversionReport, convert_payload


class GappedCareerTests(unittest.TestCase):
    def test_gapped_career_converts_with_gap_warning(self):
        gapped, _, _ = build_gapped()
        self.assertEqual(hashlib.md5(gapped).hexdigest(), "c57cfaefded23cd1d2b3ed9c01b296aa")
        report = ConversionReport()
        pc = convert_payload(MC02.parse(gapped), report)
        self.assertEqual(hashlib.md5(pc.to_bytes()).hexdigest(),
                         "d00a8fdce99bea2068efdfa961451f6d")
        self.assertEqual(report.records, 8)
        self.assertTrue(any("console record region damaged - missing chunks" in w
                            for w in report.warnings), report.warnings)


if __name__ == "__main__":
    unittest.main()
