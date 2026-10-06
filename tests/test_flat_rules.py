"""The flat rules file used by the PowerShell port must match the JSON rules.

Regenerate with:  python -c "from nfssave.payload_rules import write_flat_rules; write_flat_rules()"
(run from scripts/python).
"""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent / "scripts" / "python"))

from nfssave import payload_rules as pr


def _parse(text: str) -> dict:
    """Expand the flat file back to {kind: {id: (ref, {off: tag})}}."""
    out: dict = {"alias": {}, "career": {}}
    cur = None
    for line in text.splitlines():
        if not line or line.startswith("#"):
            continue
        parts = line.split()
        if parts[0] == "chunk":
            ref = None if parts[3] == "-" else int(parts[3], 16)
            cur = {}
            out[parts[1]][int(parts[2], 16)] = (ref, cur)
        else:
            s, e, tag = int(parts[0], 16), int(parts[1], 16), parts[2]
            for o in range(s, e, 4):
                cur[o] = tag
    return out


class FlatRulesTests(unittest.TestCase):
    def test_committed_file_is_fresh(self):
        self.assertTrue(pr.FLAT_RULES_PATH.is_file(), "fieldmaps.rules missing")
        self.assertEqual(pr.FLAT_RULES_PATH.read_text(), pr.flat_rules_text())

    def test_round_trip_matches_json_semantics(self):
        flat = _parse(pr.flat_rules_text())
        for kind in ("alias", "career"):
            self.assertEqual(set(flat[kind]), set(pr.RULES[kind]))
            for cid, entry in pr.RULES[kind].items():
                ref, tags = flat[kind][cid]
                self.assertEqual(ref, entry.get("ref_size"))
                for off, cls in entry["acts"].items():
                    want = ("C" if cls in pr.COPY_CLASSES
                            else "D" if cls in ("DIFF", "ZERO") else None)
                    self.assertEqual(tags.get(off), want, f"{kind} {cid:#x} +{off:#x}")
                self.assertTrue(set(tags) <= set(entry["acts"]))


if __name__ == "__main__":
    unittest.main()
