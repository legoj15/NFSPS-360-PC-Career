"""FEPlayerCarDB single-byte fields must keep their natural byte order.

The car DB is a raw memcpy struct; the blanket u32 swap moves every u8 field
to the far end of its word and drags the 360's 0xAA pad fill into its place.
Fatal case: the record handles. nfs.exe finds a free customization record
(0x564910) and a free career record (0x547A30) by scanning for handle 0xFF;
after the swap every handle read 0xAA, both scans failed, and the PC could
never add a car (challenge reward cars and dealer purchases silently lost,
reported 2026-10-11). Layouts are from the PC initializers 0x548F60 (record),
0x4CD920 (blueprint set), 0x547A30 (career record), 0x560A08 (DB default).

Offsets are PC payload offsets of the FEPlayerCarDB record (struct base
`this` = payload 0x14).
"""

import struct
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent.parent / "scripts" / "python"))

from nfssave import MC02, read_container
from nfssave.convert import ConversionReport, convert_payload
from nfssave.tree import Tree

ROOT = Path(__file__).parent.parent
PAIR_360 = ROOT / "docs/re/pair/CAREER_02_360_fresh"
PAIR_PC = ROOT / "docs/re/pair/CAREER_02_pc_native"
C1_360 = ROOT / "docs/re/c1_latest/CAREER_01_360"
CARDB = 0x47A07113

RECS, REC_SIZE, NRECS = 0x2684, 0x1870, 80      # customization records
SETS = (4, 4 + 0x7B4, 4 + 0xF68)                 # blueprint sets in a record
CAREERS, CAREER_SIZE = 0x7C984, 0x40             # career records (80)
DB_FLAG = 0x90694                                # this+0x90680, u8


def _cardb(path: Path, native_pc: bool = False) -> bytes:
    if native_pc:
        tree = Tree.parse(MC02.parse(path.read_bytes()).tree, big=False)
    else:
        mc = MC02.parse(read_container(path).payload)
        tree = Tree.parse(convert_payload(mc, ConversionReport()).tree, big=False)
    return next(r.payload for r in tree.records if r.id == CARDB)


def _source(path: Path) -> bytes:
    """360 FEPlayerCarDB payload re-based to PC offsets (drop the marker word)."""
    tree = Tree.parse(MC02.parse(read_container(path).payload).tree, big=True)
    return next(r.payload for r in tree.records if r.id == CARDB)[4:]


def _free_custom_record(p: bytes):
    """nfs.exe 0x564910: first record whose handle byte is 0xFF, else None."""
    for r in range(NRECS):
        if p[RECS + r * REC_SIZE + 0x17F8] == 0xFF:
            return r
    return None


def _free_career_record(p: bytes):
    """nfs.exe 0x547A30: first career record whose handle byte is 0xFF."""
    for i in range(NRECS):
        if p[CAREERS + i * CAREER_SIZE] == 0xFF:
            return i
    return None


def _u16s(b: bytes) -> bytes:
    return b"".join(b[i:i + 2][::-1] for i in range(0, len(b), 2))


@unittest.skipUnless(C1_360.is_file() and PAIR_360.is_file() and PAIR_PC.is_file(),
                     "car DB fixtures absent")
class CarDbByteFields(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.c1, cls.c1_src = _cardb(C1_360), _source(C1_360)
        cls.pair, cls.pair_src = _cardb(PAIR_360), _source(PAIR_360)
        cls.native = _cardb(PAIR_PC, native_pc=True)

    def test_game_can_add_a_car(self):
        """Acceptance: the PC allocators find room (they returned NULL)."""
        for name, p in (("c1_latest", self.c1), ("pair", self.pair)):
            with self.subTest(save=name):
                self.assertEqual(_free_custom_record(p), 10 if name == "c1_latest" else 1)
                self.assertEqual(_free_career_record(p), 10 if name == "c1_latest" else 1)

    def test_record_handles_match_native_pc(self):
        got = bytes(self.pair[RECS + r * REC_SIZE + 0x17F8] for r in range(NRECS))
        want = bytes(self.native[RECS + r * REC_SIZE + 0x17F8] for r in range(NRECS))
        self.assertEqual(got.hex(), want.hex())

    def test_career_handles_match_native_pc(self):
        got = bytes(self.pair[CAREERS + i * CAREER_SIZE] for i in range(NRECS))
        want = bytes(self.native[CAREERS + i * CAREER_SIZE] for i in range(NRECS))
        self.assertEqual(got.hex(), want.hex())

    def test_record_byte_words_natural(self):
        """Per record: handle (+0x17F8), u8 fields +0x1854..+0x185F, per-set
        u8[3] at +0x186C (fix_cardb_parts already kept records 0-78's; record
        79's word sat outside its loop)."""
        for p, src in ((self.c1, self.c1_src), (self.pair, self.pair_src)):
            for r in range(NRECS):
                b = RECS + r * REC_SIZE
                for o in (0x17F8, 0x1854, 0x185C, 0x186C):
                    with self.subTest(record=r, off=hex(o)):
                        self.assertEqual(p[b + o:b + o + 4].hex(), src[b + o:b + o + 4].hex())

    def test_blueprint_set_byte_fields(self):
        """Blueprint set: u16 + pad at +8, u8 x2 + pad at +0x188, u8[12] at
        +0x21C (initializer 0x4CD920)."""
        for p, src in ((self.c1, self.c1_src), (self.pair, self.pair_src)):
            for r in range(NRECS):
                for s in SETS:
                    b = RECS + r * REC_SIZE + s
                    with self.subTest(record=r, set=hex(s)):
                        self.assertEqual(p[b + 8:b + 12].hex(),
                                         (_u16s(src[b + 8:b + 10]) + src[b + 10:b + 12]).hex())
                        self.assertEqual(p[b + 0x188:b + 0x18C].hex(),
                                         src[b + 0x188:b + 0x18C].hex())
                        self.assertEqual(p[b + 0x21C:b + 0x228].hex(),
                                         src[b + 0x21C:b + 0x228].hex())

    def test_blueprint_set_origin_anchored_on_native_pc(self):
        """The set offsets above are only as good as the set origin. Anchor
        it independently: the set initializer 0x4CD920 writes u32 1 at
        set+0x22C, and every set of the native PC pair holds exactly that
        there. Also ties the origin to the in-game-verified part slots
        (CARDB_RECORDS/CARDB_PART_SLOTS use converter framing 4 bytes
        before the struct record; review claims of a mismatch were wrong,
        docs/review-false-positives.md)."""
        from nfssave.convert import CARDB_RECORDS, CARDB_PART_SLOTS
        for r in range(NRECS):
            for s in SETS:
                b = RECS + r * REC_SIZE + s
                with self.subTest(record=r, set=hex(s)):
                    self.assertEqual(self.native[b + 0x22C:b + 0x230].hex(), "01000000")
        # part array = set+0x34 in the struct, +0x3C in converter framing
        self.assertEqual(RECS + SETS[0] + 0x34, CARDB_RECORDS[0] + CARDB_PART_SLOTS[0])

    def test_career_record_layout(self):
        """[u8 handle][pad][u8][u8] [u16][pad2] [f32] [u8][pad3] [u32]
        [u16][u16] [f32] [u32] [u16 x16]."""
        for p, src in ((self.c1, self.c1_src), (self.pair, self.pair_src)):
            for i in range(NRECS):
                b = CAREERS + i * CAREER_SIZE
                s = src[b:b + CAREER_SIZE]
                want = (s[0:4] + _u16s(s[4:6]) + s[6:8] + s[8:12][::-1] + s[12:16]
                        + s[16:20][::-1] + _u16s(s[20:24]) + s[24:28][::-1]
                        + s[28:32][::-1] + _u16s(s[32:64]))
                with self.subTest(career=i):
                    self.assertEqual(p[b:b + CAREER_SIZE].hex(), want.hex())

    def test_db_flag_byte_natural(self):
        for p, src in ((self.c1, self.c1_src), (self.pair, self.pair_src)):
            self.assertEqual(p[DB_FLAG:DB_FLAG + 4].hex(), src[DB_FLAG:DB_FLAG + 4].hex())


class CarDbSyntheticWords(unittest.TestCase):
    """Distinct bytes in every position, so pad handling is visible (the
    fixtures' 0xAA AA pads are symmetric under any swap)."""

    def test_u16_plus_pad_words_keep_the_pad_natural(self):
        from nfssave.convert import fix_cardb_bytes, swap_u32s
        src = bytearray(0x906A4 + 4)
        car = CAREERS + 4 + 4                 # career record 0 word +4, 360 coords
        bp = RECS + 4 + SETS[0] + 8           # record 0, set 0, word +8
        for o in (car, bp):
            src[o:o + 4] = bytes.fromhex("123400aa")
        out = bytearray(swap_u32s(bytes(src)))
        fix_cardb_bytes(bytes(src), out)
        self.assertEqual(out[car:car + 4].hex(), "341200aa")
        self.assertEqual(out[bp:bp + 4].hex(), "341200aa")

    def test_wrong_length_out_is_refused(self):
        """A bytearray slice-assign past out's end appends (resizes) instead
        of failing, so the equal-length contract is refused up front (same
        guard in nfssave-core convert.rs and Convert-NfsSave.ps1)."""
        from nfssave.convert import fix_cardb_parts, fix_cardb_packed
        src = bytes(range(0x100))
        for out in (bytearray(0x80), bytearray(0x200)):
            for fix in (fix_cardb_parts, fix_cardb_packed):
                with self.subTest(fix=fix.__name__, out=f"0x{len(out):x}"):
                    with self.assertRaisesRegex(ValueError, "equal length"):
                        fix(src, out)


if __name__ == "__main__":
    unittest.main()
