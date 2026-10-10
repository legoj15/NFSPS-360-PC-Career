"""Hostile input refused cleanly (ValueError), never a crash or a huge
allocation. Same vectors in src/crates/nfssave-core/tests/test_hostile.rs and
scripts/powershell/tests/Run-Tests.ps1."""

import struct
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent / "scripts" / "python"))

from nfssave import MC02
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


if __name__ == "__main__":
    unittest.main()
