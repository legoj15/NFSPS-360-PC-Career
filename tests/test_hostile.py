"""Hostile input refused cleanly (ValueError), never a crash or a huge
allocation. Same vectors in src/crates/nfssave-core/tests/test_hostile.rs and
scripts/powershell/tests/Run-Tests.ps1."""

import hashlib
import struct
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent / "scripts" / "python"))

from nfssave import MC02, read_container
from nfssave.convert import GAMEPLAY_ID, RACEDATA_ID, convert_payload
from nfssave.mc02 import Endian
from nfssave.tree import Tree


def magic_at_end() -> bytes:
    """0x44-byte 360 tree blob whose magic is its last word: no room for the
    used-size word after it."""
    blob = bytearray(0x44)
    blob[0x40:0x44] = struct.pack(">I", 0x59F2D89B)
    return bytes(blob)


def huge_tree_size() -> bytes:
    """Bare 0x1C-byte MC02 header declaring a 2 GiB tree buffer."""
    return struct.pack("<7I", 0x4D433032, 0x1C, 0, 0x80000000, 0, 0, 0)


class HostileInputTests(unittest.TestCase):
    def test_magic_without_used_word_is_refused(self):
        with self.assertRaisesRegex(ValueError, "too short for the used-size word"):
            Tree.parse(magic_at_end(), big=True)

    def test_blob_without_count_word_is_refused(self):
        with self.assertRaisesRegex(ValueError, r"tree blob \(0x10 B\) too short for the chunk-count word"):
            Tree.parse(bytes(0x10), big=True)

    def test_short_blob_without_magic_is_refused(self):
        with self.assertRaisesRegex(ValueError, "tree magic 0x59F2D89B not found"):
            Tree.parse(bytes(0x20), big=True)

    def test_huge_tree_size_is_refused(self):
        with self.assertRaisesRegex(ValueError, "tree size 0x80000000 exceeds"):
            MC02.parse(huge_tree_size())


C1 = Path(__file__).parent.parent / "docs/re/c1_latest/CAREER_01_360"


def craft(rec_id: int, payload: bytes) -> bytes:
    """The tracked save with one record's payload replaced; same fixture (and
    md5) as `craft` in src/crates/nfssave-core/tests/test_short_records.rs."""
    mc02 = MC02.parse(read_container(C1).payload)
    tree = Tree.parse(mc02.tree, big=True)
    for r in tree.records:
        if r.id == rec_id:
            r.payload = payload
    built = tree.build(big=True, tree_size=mc02.tree_size)
    return MC02(Endian.BIG, mc02.extra, built, mc02.tree_size).to_bytes()


class ShortRecordTests(unittest.TestCase):
    """Refusal messages shared with the Rust core and the PowerShell port
    (test_short_records.rs, Run-Tests.ps1), not a bare struct.error or
    AssertionError."""

    def convert(self, data: bytes, md5: str):
        self.assertEqual(hashlib.md5(data).hexdigest(), md5, "fixture drift")
        convert_payload(MC02.parse(data))

    def test_gameplay_below_raceday_state_is_refused(self):
        with self.assertRaisesRegex(
                ValueError, r"^GameplayData chunk too short \(0x2d8 B\) to hold the "
                            r"race-day state - the source file is corrupted$"):
            self.convert(craft(GAMEPLAY_ID, b"\x11" * 0x2D8), "acd56e6ada2458d1efc25a22e6876ae7")

    def test_gameplay_unaligned_size_is_refused(self):
        with self.assertRaisesRegex(
                ValueError, r"^GameplayData chunk size 0x2e3 is not word-aligned - "
                            r"the source file is corrupted$"):
            self.convert(craft(GAMEPLAY_ID, b"\x11" * 0x2E3), "e7084c4aaa3291502a34f35714975bf8")

    def test_numeric_unaligned_size_is_refused(self):
        data = craft(RACEDATA_ID, b"\x11" * 0x13)
        with self.assertRaisesRegex(
                ValueError, r"^RaceData chunk size 0x13 is not word-aligned - "
                            r"the source file is corrupted$"):
            convert_payload(MC02.parse(data))


if __name__ == "__main__":
    unittest.main()
