"""convert_extra on a 64-byte alias extra blob whose name has no NUL.

The golden alias fixtures all NUL-terminate the name; this pins the
defensive branch (printable run, then 0xAA pad words, align, swap tail).
Same vector in src/crates/nfssave-core/tests/test_extra.rs and
scripts/powershell/tests/Run-Tests.ps1.
"""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent / "scripts" / "python"))

from nfssave.convert import convert_extra

HEAD = bytes(range(1, 0x15))                 # five BE u32s
NAME = b"ANONYMOUS 1"                         # 0x14..0x1F, no terminator
PAD = b"\xaa" * (0x38 - 0x14 - len(NAME))     # 0x1F..0x38
TAIL = bytes.fromhex("1122334455667788")      # no zero bytes anywhere
EXTRA = HEAD + NAME + PAD + TAIL


class ConvertExtraNoNul(unittest.TestCase):
    def test_vector_has_no_nul(self):
        self.assertEqual(len(EXTRA), 64)
        self.assertNotIn(0, EXTRA)

    def test_no_nul_name(self):
        out = convert_extra(EXTRA)
        head = b"".join(HEAD[i:i + 4][::-1] for i in range(0, 0x14, 4))
        tail = TAIL[:4][::-1] + TAIL[4:][::-1]
        self.assertEqual(out, head + NAME + PAD + tail)


if __name__ == "__main__":
    unittest.main()


class ExtraUsedSize(unittest.TestCase):
    """Extra word 1 = the tree's used size, on every native PC save (career
    and alias). The converter inserts a size-0 PCControllerSettings record
    into aliases (+12 bytes), so the 360 value no longer matches; the PC
    then silently refused the alias and ran on a default 'Player' profile
    (whose first in-game save wrote ALIAS_Player + a career named
    CAREER_<0xAA>)."""

    def test_used_matches_tree(self):
        import struct
        from nfssave import MC02, read_container
        from nfssave.convert import ConversionReport, convert_payload
        from nfssave.tree import Tree
        root = Path(__file__).parent.parent
        for src in ("Extracted/Alias/ALIAS_360", "docs/re/alias_anon/ALIAS_360",
                    "docs/re/c1_latest/CAREER_01_360"):
            p = root / src
            with self.subTest(source=src):
                # personal saves are gitignored (absent in worktrees/CI)
                if src.startswith("Extracted/") and not p.exists():
                    self.skipTest(f"source not present: {src}")
                pc = convert_payload(MC02.parse(read_container(p).payload), ConversionReport())
                t = Tree.parse(pc.tree, big=False)
                self.assertEqual(struct.unpack_from("<I", pc.extra, 4)[0], t.used)
