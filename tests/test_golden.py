"""Golden regression tests: source containers -> byte-exact verified output.

Regression pins for the current converter (last moved 2026-10-09: race-day
progress table, node [0][len] headers, CustomRaceDayMemcard strings, record
tail words, alias extra used size). Verification state lives in
docs/HANDOFF.md. Any diff here means the converter changed behavior.

Run:  python -m unittest discover tests
"""

import hashlib
import tempfile
import unittest
from pathlib import Path

import sys
sys.path.insert(0, str(Path(__file__).parent.parent / "scripts" / "python"))

from nfssave import MC02, read_container
from nfssave.convert import ConversionReport, convert_payload, write_pc_save

ROOT = Path(__file__).parent.parent
PAIR_360 = ROOT / "docs/re/pair/CAREER_02_360_fresh"

# (source path, golden md5)
CASES = [
    (ROOT / "Extracted/Career/CAREER_01", "4afedb367a0fe2c8dee178e4ed3daca1"),
    (PAIR_360, "3da9f4c0a5a2b7d5c55863d49de4852c"),
    (ROOT / "Extracted/Career/CAREER_03", "77e5b3e95f3734b5460a986a9b74a92b"),
    (ROOT / "Extracted/Alias/ALIAS_360", "a5a24e0f79571d2b5f819a76704a6b89"),
    (ROOT / "docs/re/c1_latest/CAREER_01_360", "5b7d3fcb229ba2ec135d68de121bb0ff"),
    (ROOT / "docs/re/pair_raceday/CAREER_02_360", "ec77c9309356db48faeae8e66f840176"),
    # anonymized copy of the personal alias save (docs/re/alias_anon/README.md)
    (ROOT / "docs/re/alias_anon/ALIAS_360", "377651916f0e1bd488561b7481a66da1"),
]

PERSONAL = ROOT / "Extracted"


def _skip_if_personal_missing(case: unittest.TestCase, src: Path) -> None:
    """Personal saves (gitignored) may be absent; tracked fixtures may not."""
    if src.is_file():
        return
    if src.is_relative_to(PERSONAL):
        case.skipTest(f"source not present: {src}")
    case.fail(f"tracked fixture missing: {src}")


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
                    _skip_if_personal_missing(self, src)
                    target = _convert(src, out_root)
                    digest = hashlib.md5(target.read_bytes()).hexdigest()
                    self.assertEqual(digest, golden)

    def test_output_self_validates(self):
        """Every golden output re-parses with clean CRCs and PC framing."""
        with tempfile.TemporaryDirectory() as tmp:
            out_root = Path(tmp)
            for src, _ in CASES:
                with self.subTest(source=src):
                    _skip_if_personal_missing(self, src)
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
