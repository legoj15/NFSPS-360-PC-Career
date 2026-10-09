"""Alias option chunks must keep their values on PC (2026-10-09 in-game bug).

Converted aliases loaded with every on/off option read as 0 (driving aids
off, HUD gauge hidden): one-byte property nodes were u32-swapped, moving the
value byte to the end of its word. VideoSettings also kept two 360-only
nodes, leaving the chunk larger than the PC savable.

Run:  python -m unittest discover tests
"""

import struct
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent / "scripts" / "python"))

from nfssave import MC02, read_container
from nfssave.convert import ConversionReport, convert_payload, fix_node_flags
from nfssave.tree import Tree

ROOT = Path(__file__).parent.parent
ALIAS_360 = ROOT / "docs/re/alias_anon/ALIAS_360"
NATIVE_PC = ROOT / "docs/re/oracle/native_ALIAS_Player"

VIDEO_SETTINGS = 0xC3EC4947
PC_CONTROLLER = 0x39156567


def u8_nodes(payload: bytes, big: bool) -> list[tuple[int, int]]:
    """(data offset, value) of every one-byte property node:
    [u32 0][u32 len=1][flag word][u8 value + 3 pad]."""
    e = ">" if big else "<"
    out = []
    for o in range(8, len(payload) - 7, 4):
        z, ln = struct.unpack_from(e + "II", payload, o - 8)
        if z == 0 and ln == 1 and payload[o] in (0, 1) and payload[o + 5:o + 8] == b"\0\0\0":
            out.append((o + 4, payload[o + 4]))
    return out


def convert_alias():
    src = Tree.parse(MC02.parse(read_container(ALIAS_360).payload).tree, big=True)
    pc_m = convert_payload(MC02.parse(read_container(ALIAS_360).payload), ConversionReport())
    pc = Tree.parse(pc_m.tree, big=False)
    return src, pc


class AliasSettingsTests(unittest.TestCase):
    def test_one_byte_options_keep_their_value(self):
        src, pc = convert_alias()
        by_id = {r.id: r for r in pc.records}
        checked = 0
        for r in src.records:
            # 360 payload = marker word + content; PC payload = content + junk
            for off, value in u8_nodes(r.payload[4:], big=True):
                got = by_id[r.id].payload[off]
                self.assertEqual(got, value, f"chunk {r.id:#x} node at {off:#x}")
                checked += 1
        self.assertGreater(checked, 50)   # Gameplay/Video/PlayerSettings0-3/OnlineUserProfile

    def test_last_node_value_comes_from_the_word_after_the_record(self):
        # 360 records tile as [id][size][flag word + nodes][last data word]:
        # the last node's value sits in the word Tree.parse reads as the next
        # record's header "type". Personal alias: AudioSettings ends 3,
        # PlayerSettings0 ends 2, OnlineUserProfile ends with u8 1.
        src, pc = convert_alias()
        tails = {r.id: r.tail for r in src.records}
        by_id = {r.id: r for r in pc.records}
        self.assertEqual(tails[0x9CB326C2], b"\0\0\0\x03")
        self.assertEqual(by_id[0x9CB326C2].payload[-4:], b"\x03\0\0\0")
        self.assertEqual(by_id[0x8B7D0AAD].payload[-4:], b"\x02\0\0\0")
        self.assertEqual(by_id[0x9F72F194].payload[-4:], b"\x01\0\0\0")

    def test_chunk_sizes_match_native_pc(self):
        _, pc = convert_alias()
        native = Tree.parse(MC02.parse(NATIVE_PC.read_bytes()).tree, big=False)
        want = {r.id: len(r.payload) for r in native.records}
        for r in pc.records:
            if r.id == PC_CONTROLLER:
                continue                  # PC-only, size-0 positional filler
            with self.subTest(chunk=hex(r.id)):
                self.assertEqual(len(r.payload), want[r.id])

    def test_u32_node_after_zero_is_not_mistaken_for_u8(self):
        # [0][len=1][flag][00 00 00 04] - a word whose pad bytes are not zero
        # is not a one-byte node (seen in SPEECH DATA / race-day chunks)
        src = struct.pack(">IIII", 0, 1, 0x00FFFFFF, 4)
        out = bytearray(b"".join(src[i:i + 4][::-1] for i in range(0, 16, 4)))
        fix_node_flags(src, out)
        self.assertEqual(out[12:16], b"\x04\0\0\0")

    def test_u8_node_stays_natural(self):
        src = struct.pack(">IIII", 0, 1, 0x00FFFFFF, 0x01000000)
        out = bytearray(b"".join(src[i:i + 4][::-1] for i in range(0, 16, 4)))
        fix_node_flags(src, out)
        self.assertEqual(out[12:16], b"\x01\0\0\0")


if __name__ == "__main__":
    unittest.main()
