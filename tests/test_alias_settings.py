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
from nfssave.convert import (ConversionReport, scalar_tail, convert_payload, fix_node_flags,
                             node_spans)
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
            # 360 payload = marker word + content; PC payload = content + last word
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
        self.assertEqual({r.id for r in pc.records}, set(want))
        for r in pc.records:
            with self.subTest(chunk=hex(r.id)):
                if r.id == VIDEO_SETTINGS:
                    # keeps the 360's two extra trailing nodes (0xB4): the PC
                    # loads them fine (in-game 2026-10-09); the trimmed 0x74
                    # was only ever part of failing test runs
                    self.assertEqual(len(r.payload), 0xB4)
                else:
                    self.assertEqual(len(r.payload), want[r.id])

    def test_pc_controller_gets_native_defaults(self):
        # A size-0 PCControllerSettings loaded, then the PC dropped the
        # profile mid-session for a default 'Player' (ALIAS_Player +
        # "too many aliases"); native default bindings fixed it in-game.
        _, pc = convert_alias()
        got = next(r.payload for r in pc.records if r.id == PC_CONTROLLER)
        default = (ROOT / "scripts/python/nfssave/pc_controller_default.bin").read_bytes()
        self.assertEqual(got, default)
        native = Tree.parse(MC02.parse(NATIVE_PC.read_bytes()).tree, big=False)
        oracle = next(r.payload for r in native.records if r.id == PC_CONTROLLER)
        # same bindings as the repo oracle; only the flag words' junk bytes differ
        flags = {o + 12 for o in range(0, len(oracle), 16)}
        for o in range(0, len(oracle), 4):
            if o not in flags:
                self.assertEqual(got[o:o + 4], oracle[o:o + 4], f"word {o:#x}")

    def test_u32_node_after_zero_is_not_mistaken_for_u8(self):
        # [0][len=1][flag][00 00 00 04] - a word whose pad bytes are not zero
        # is not a one-byte node (seen in SPEECH DATA / race-day chunks)
        src = struct.pack(">IIII", 0, 1, 0x00FFFFFF, 4)
        out = bytearray(b"".join(src[i:i + 4][::-1] for i in range(0, 16, 4)))
        fix_node_flags(src, out)
        self.assertEqual(out[12:16], b"\x04\0\0\0")

    def test_scalar_tail_rules(self):
        hdr = lambda ln: struct.pack(">III", 0, ln, 0x00FFFFFF)
        self.assertEqual(scalar_tail(hdr(4), b"\0\0\0\x03"), b"\x03\0\0\0")     # u32 swap
        self.assertEqual(scalar_tail(hdr(1), b"\x01\0\0\0"), b"\x01\0\0\0")     # u8 natural
        self.assertEqual(scalar_tail(hdr(1), b"\0\0\0\x04"), b"\x04\0\0\0")     # pad set: swap
        self.assertEqual(scalar_tail(hdr(5), b"\0\0\0\x03"), bytes(4))          # not a scalar node
        self.assertEqual(scalar_tail(b"\0" * 8 + b"\xff" * 4, b"\0\0\0\x03"), bytes(4))  # len 0
        self.assertEqual(scalar_tail(hdr(4)[4:], b"\0\0\0\x03"), bytes(4))      # payload < 12
        self.assertEqual(scalar_tail(hdr(4), b"\0\x03"), bytes(4))              # truncated tail
        # 8-byte node: payload ends [0][8][flag][d1]; the tail is d2
        self.assertEqual(scalar_tail(hdr(8) + b"\0" * 4, b"\x3f\x80\0\0"), b"\0\0\x80\x3f")
        # header whose flag word is not [u8][FF FF FF | 00 00 00]: not a node
        junk_flag = struct.pack(">III", 0, 4, 0x01234567)
        self.assertEqual(scalar_tail(junk_flag, b"\0\0\0\x03"), bytes(4))

    def test_u8_rule_needs_a_node_flag_word(self):
        src = struct.pack(">IIII", 0, 1, 0x01234567, 0x01000000)
        out = bytearray(b"".join(src[i:i + 4][::-1] for i in range(0, 16, 4)))
        fix_node_flags(src, out)
        self.assertEqual(out[12:16], b"\0\0\0\x01")   # left swapped

    def test_u8_node_stays_natural(self):
        src = struct.pack(">IIII", 0, 1, 0x00FFFFFF, 0x01000000)
        out = bytearray(b"".join(src[i:i + 4][::-1] for i in range(0, 16, 4)))
        fix_node_flags(src, out)
        self.assertEqual(out[12:16], b"\x01\0\0\0")

    def test_on_chain_u8_with_junk_pad_keeps_first_byte(self):
        # [marker][first value][0][len 1][flag 00001b10][01 00 00 5d]: a real
        # node on the property chain; the 360 left heap junk in its pad (and
        # flag) bytes. The value is the first byte (PC read 0x5d before).
        src = struct.pack(">IIIIII", 0x01000000, 7, 0, 1, 0x00001B10, 0x0100005D)
        out = bytearray(b"".join(src[i:i + 4][::-1] for i in range(0, len(src), 4)))
        fix_node_flags(src, out)
        self.assertEqual(out[20], 1)

    def test_junk_pad_fixture_keeps_every_u8_value(self):
        # docs/re/alias_anon_junkpad: the only sample with junk-padded nodes
        path = ROOT / "docs/re/alias_anon_junkpad/ALIAS_360"
        mc = MC02.parse(read_container(path).payload)
        src = Tree.parse(mc.tree, big=True)
        pc = {r.id: r.payload for r in Tree.parse(convert_payload(mc, ConversionReport()).tree,
                                                   big=False).records}
        checked = 0
        for r in src.records:
            for d, ln in node_spans(r.payload):
                if ln == 1:
                    self.assertEqual(pc[r.id][d - 4], r.payload[d], f"chunk {r.id:#x} at {d - 4:#x}")
                    checked += 1
        self.assertGreater(checked, 50)


if __name__ == "__main__":
    unittest.main()
