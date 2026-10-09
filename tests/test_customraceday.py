"""CustomRaceDayMemcard (0xD548266C): custom race days are a property-node
stream [u32 0][u32 len][flag word][data] ... The string heuristic used to
keep the [0][len] header after a race-day NAME in 360 byte order (the name's
zero padding ran into it), so the PC read a length of 0x04000000, dropped the
event list and the Race Day menu crashed on the empty slot (nfs.exe
0x7F6480). Verified live: the PC-side record had name/GUID/settings but no
events."""

import struct
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent / "scripts" / "python"))

from nfssave import MC02, read_container
from nfssave.convert import ConversionReport, convert_payload
from nfssave.tree import Tree

ROOT = Path(__file__).parent.parent
C1 = ROOT / "docs/re/c1_latest/CAREER_01_360"          # 4 custom race days
PC_CUSTOM = ROOT / "docs/re/pair_customrd/CAREER_aa_after_pc"  # PC-written, 1 race day
CRD = 0xD548266C


def walk(p: bytes, big: bool, start: int) -> list[bytes]:
    """Node data in order. The first value sits at `start` (4 bytes); every
    later node is [u32 0][u32 len][flag word][data len], headers on the u32
    grid after the previous data."""
    e = ">" if big else "<"
    nodes = [p[start:start + 4]]
    o = start + 4
    while o + 12 <= len(p):
        h = (o + 3) & ~3
        while h + 8 <= len(p):
            z, ln = struct.unpack_from(e + "II", p, h)
            if z == 0 and 0 < ln <= 0x400:
                break
            h += 4
        else:
            break
        z, ln = struct.unpack_from(e + "II", p, h)
        d = h + 12
        if d + ln > len(p):
            break
        nodes.append(p[d:d + ln])
        o = d + ln
    return nodes


def _crd(tree: Tree) -> bytes:
    return [r.payload for r in tree.records if r.id == CRD][0]


def _norm(nodes: list[bytes], big: bool) -> list:
    """u32 nodes as ints, string nodes as their text (first 4 bytes are junk)."""
    out = []
    for n in nodes:
        if len(n) == 4:
            out.append(struct.unpack(">I" if big else "<I", n)[0])
        else:
            out.append(n[4:].split(b"\0")[0])
    return out


class CustomRaceDayTests(unittest.TestCase):
    def test_pc_written_stream_walks(self):
        # guards the walker itself against the PC's own output
        nodes = _norm(walk(_crd(Tree.parse(MC02.parse(PC_CUSTOM.read_bytes()).tree,
                                           big=False)), False, 0), False)
        self.assertIn(b"My Race Day 1", nodes)
        name = nodes.index(b"My Race Day 1")
        self.assertEqual(nodes[name + 1], 3)                 # NumEvents

    def test_converted_nodes_match_console(self):
        t360 = Tree.parse(MC02.parse(read_container(C1).payload).tree, big=True)
        i = [r.id for r in t360.records].index(CRD)
        # a 360 record's final value lives in the NEXT record's header word
        src = t360.records[i].payload + struct.pack(">I", t360.records[i + 1].type)
        want = _norm(walk(src, True, 4), True)               # skip the 360 marker word
        conv = _crd(Tree.parse(convert_payload(
            MC02.parse(read_container(C1).payload), ConversionReport()).tree, big=False))
        got = _norm(walk(conv, False, 0), False)
        self.assertEqual(want[1:], got[1:])                  # [0] is an uninit word
        for name in (b"My Race Day 12", b"My Race Day 13", b"My Race Day 14",
                     b"My Race Day 15"):
            self.assertIn(name, got)
        self.assertEqual(got[got.index(b"My Race Day 13") + 1], 4)   # NumEvents
        self.assertEqual(got[-1], 1)          # last event flag, from the next 360 header


class RecordTailTests(unittest.TestCase):
    """PC payload = 360 payload[4:] + the 360 word at the next record's
    header slot (verified: CAREER_01 CustomRaceDayMemcard -> 0x00000001 =
    last event flag; the PC-written race day ends 01 00 00 00 too)."""

    def test_tail_words_carried(self):
        t360 = Tree.parse(MC02.parse(read_container(C1).payload).tree, big=True)
        conv = Tree.parse(convert_payload(MC02.parse(read_container(C1).payload),
                                          ConversionReport()).tree, big=False)
        pc = {r.id: r.payload for r in conv.records}
        for a, b in zip(t360.records, t360.records[1:]):
            if a.id == 0x3B309E09:                 # GameplayData: trimmed 360 pad
                continue
            with self.subTest(chunk=hex(a.id)):
                tail = pc[a.id][-4:]
                nat = struct.pack(">I", b.type)
                self.assertIn(tail, (nat, nat[::-1]))
                if a.payload[-12:-4] == bytes(4) + struct.pack(">I", 4):
                    self.assertEqual(tail, nat[::-1])   # u32 value node: swapped


if __name__ == "__main__":
    unittest.main()
