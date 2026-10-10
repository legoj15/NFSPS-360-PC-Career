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
from nfssave.convert import ConversionReport, convert_payload, convert_to_pc_record
from nfssave.tree import Tree


POST_WORD = bytes([0, 0, 0, 7])  # non-zero so the last record's post spill shows
INTERNAL_GAP_MD5 = "743068fdd49685367041faa4df7c3903"


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

    def test_internal_gap_only_the_record_before_the_damage_loses_its_spill(self):
        # damage one mid-tree record: the records after it re-anchor, so every
        # one of them (the last included) converts exactly as in a clean tree
        # and the last record before the noise gets no spill (b"")
        gapped, pristine, lost = build_gapped(first_damaged_id=0xD548266C, damaged_count=1,
                                             post_word=POST_WORD)
        self.assertEqual(lost, [0xD548266C])
        self.assertEqual(hashlib.md5(gapped).hexdigest(), "bacda5eeb061896b540221dc26fb8aa8")
        gt = Tree.parse(MC02.parse(gapped).tree, big=True)
        self.assertNotEqual(gt.gap, 0)
        k = gt.gap_at
        self.assertIsNotNone(k)
        self.assertLess(k, len(gt.records))
        self.assertEqual(gt.records[k - 1].id, 0x885B4DDC)

        report = ConversionReport()
        pc = convert_payload(MC02.parse(gapped), report)

        clean = ConversionReport()
        want = convert_payload(MC02.parse(pristine), clean)
        got_recs = {r.id: r for r in Tree.parse(pc.tree, big=False).records}
        want_recs = {r.id: r for r in Tree.parse(want.tree, big=False).records}
        self.assertNotIn(0xD548266C, got_recs)
        self.assertEqual(gt.post[:4], POST_WORD)
        for rec in gt.records[k:]:
            self.assertEqual(got_recs[rec.id], want_recs[rec.id], hex(rec.id))
        # the record before the damage: converted as if its spill were b""
        pt = Tree.parse(MC02.parse(pristine).tree, big=True)
        before = next(r for r in pt.records if r.id == gt.records[k - 1].id)
        convert_to_pc_record(before, b"", ConversionReport())
        self.assertEqual((got_recs[before.id].type, got_recs[before.id].payload),
                         (before.type, before.payload))
        # and every record further back is untouched by the damage
        for rec in gt.records[:k - 1]:
            self.assertEqual(got_recs[rec.id], want_recs[rec.id], hex(rec.id))
        self.assertTrue(any("console record region damaged - missing chunks" in w
                            for w in report.warnings), report.warnings)
        self.assertEqual(hashlib.md5(pc.to_bytes()).hexdigest(), INTERNAL_GAP_MD5)


if __name__ == "__main__":
    unittest.main()
