//! 360 -> PC save conversion for NFS ProStreet.
//!
//! Pipeline:
//!   1. Read the 360 CON container, extract the big-endian MC02 payload.
//!   2. Parse the chunk tree; normalize platform buffer differences (the 360
//!      GameplayData chunk carries a 0x4000 zero tail the PC buffer omits);
//!      convert every record payload BE->LE (fieldmap rules or auto
//!      string-detect + u32 swap).
//!   3. Re-encode as a PC tree: same records; PC head region is allocator
//!      garbage that the loader never reads (zeros + root record).
//!   4. Rebuild the little-endian MC02 with fresh CRCs, patch the tree-hash
//!      placeholder and the used-tree-size word in the extra blob, and write
//!      to the PC save layout: `<root>/<NAME>/<NAME>`.
//!
//! Chunk ids are djb2(name) hashes (h=-1; h=h*33+c), identical across
//! platforms — see [`chunk_name`].

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use md5::{Digest, Md5};

use crate::container360::parse_container;
use crate::mc02::{Endian, MC02};
use crate::payload_rules::{convert_payload_auto, convert_record, find_string_runs, rules_loaded};
use crate::tree::{Record, TREE_MAGIC, Tree, swap_u32s};
use crate::treehash::tree_hash;
use crate::{Error, Result, format_err};

/// allocator-garbage region PC keeps between count and root
pub const PC_HEAD_STRUCT_SIZE: usize = 0x1AC;

pub const GAMEPLAY_ID: u32 = 0x3B30_9E09;
/// PC buffer for the gameplay chunk
pub const GAMEPLAY_PC_SIZE: usize = 0x10014;
/// 360 buffer: same content + 0x4000 zero pad
pub const GAMEPLAY_360_SIZE: usize = 0x10014 + 0x4000;

/// djb2(name) chunk-id -> chunk name (see docs/re/hashscan.py).
pub fn chunk_name(id: u32) -> String {
    let name = match id {
        0x59F2D89B => "MEMCARD_ROOT",
        0x8FFBE3E8 => "GStatsImpl::SavableStats",
        0x4E8AA143 => "AchievementManager",
        0x9F72F194 => "OnlineUserProfile",
        0xDC6B027F => "ProfileStats",
        0x6CC89C57 => "Jukebox",
        0xC3EC4947 => "VideoSettings",
        0xB74C1044 => "ForceFeedbackSettings",
        0x8C4A2DC0 => "GameplaySettings",
        0x9CB326C2 => "AudioSettings",
        0x8B7D0AAD => "PlayerSettings0",
        0x8B7D0AAE => "PlayerSettings1",
        0x8B7D0AAF => "PlayerSettings2",
        0x8B7D0AB0 => "PlayerSettings3",
        0x322ED42F => "UserProfile",
        0x328C6431 => "SPEECH DATA",
        0xB67F6CC6 => "MarkerSystem",
        0x3B309E09 => "GameplayData",
        0x1FB48CF2 => "GameplayBinarySavableHelper",
        0x51A41B14 => "RaceData",
        0x47A07113 => "FEPlayerCarDB",
        0x34B74942 => "VehicleBinarySavableHelper",
        0x885B4DDC => "FECareer",
        0x39156567 => return "PCControllerSettings".into(), // PC-only
        0xD548266C => return "CustomRaceDayMemcard".into(), // PC-only
        0xCA269650 => return "UnlockSystem".into(),         // PC-only
        _ => return format!("{id:#x}"),
    };
    name.into()
}

#[derive(Clone, Debug, Default)]
pub struct ConversionReport {
    pub source: String,
    pub kind: String,
    pub records: usize,
    pub chunk_list: Vec<String>,
    pub warnings: Vec<String>,
}

impl fmt::Display for ConversionReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "{}: {} chunks -> {}",
            self.kind,
            self.records,
            self.chunk_list.join(", ")
        )?;
        for w in &self.warnings {
            writeln!(f, "  ! {w}")?;
        }
        Ok(())
    }
}

/// Trim the 360-only 0x4000 zero tail from GameplayData (in place).
///
/// The console allocates a 16 KB larger buffer for the gameplay chunk than
/// the PC build; the extra region is always zero padding (verified on a
/// fresh and a day-7 career: real content ends before 0xA1C in both). The
/// nested-helper size word at payload offset 0x0C is rewritten to match
/// the trimmed length, in big-endian (the payload-wide swap happens later).
pub fn normalize_gameplay(rec: &mut Record, warnings: &mut Vec<String>) {
    let p = &rec.payload;
    if rec.id != GAMEPLAY_ID || p.len() <= GAMEPLAY_PC_SIZE {
        return;
    }
    let tail = &p[GAMEPLAY_PC_SIZE..];
    if tail.iter().any(|&t| t != 0) {
        warnings.push(format!(
            "GameplayData content ({:#x} B) exceeds the PC buffer ({GAMEPLAY_PC_SIZE:#x} B); \
             converting untrimmed - the game may reject the chunk",
            p.len()
        ));
        return;
    }
    let mut trimmed = p[..GAMEPLAY_PC_SIZE].to_vec();
    trimmed[0x0C..0x10].copy_from_slice(&((GAMEPLAY_PC_SIZE - 0x10) as u32).to_be_bytes());
    rec.payload = trimmed;
}

/// MC02 'extra' preamble, 360 -> PC.
///
/// career (28 B): all numeric -> swap.
/// alias (64 B): [id][used][1][day][0] swap; NUL-terminated player name
/// natural (any byte values - gamertags may be non-ASCII); 0xAA pad words
/// natural; tail floats/hash swap.
pub fn convert_extra(extra_be: &[u8]) -> Result<Vec<u8>> {
    let n = extra_be.len();
    if n == 28 {
        return Ok(swap_u32s(extra_be));
    }
    if n != 64 {
        return Err(format_err(format!("unexpected extra size {n}")));
    }
    let mut out = extra_be.to_vec();
    let swap_word = |out: &mut Vec<u8>, i: usize| {
        out[i..i + 4].copy_from_slice(&[
            extra_be[i + 3],
            extra_be[i + 2],
            extra_be[i + 1],
            extra_be[i],
        ]);
    };
    for i in (0..0x14).step_by(4) {
        swap_word(&mut out, i);
    }
    // name: everything up to the first NUL is the name, kept natural
    let nul = extra_be[0x14..n]
        .iter()
        .position(|&b| b == 0x00)
        .map(|p| p + 0x14);
    let mut end = match nul {
        Some(p) => p + 1,
        None => {
            // no terminator: printable run then 0x00/0xAA pad (defensive)
            let mut e = 0x14;
            while e < n
                && extra_be[e] != 0x00
                && extra_be[e] != 0xAA
                && (0x20..0x7F).contains(&extra_be[e])
            {
                e += 1;
            }
            if e < n && extra_be[e] == 0x00 {
                e += 1;
            }
            e
        }
    };
    while end + 4 <= n && extra_be[end..end + 4] == [0xAA; 4] {
        end += 4;
    }
    // align to 4 and swap the numeric tail
    let end = (end + 3) & !3;
    for i in (end..n).step_by(4) {
        swap_word(&mut out, i);
    }
    Ok(out)
}

/// memcpy structs, not property nodes
pub const RAW_BLOB_IDS: [u32; 2] = [GAMEPLAY_ID, 0x47A0_7113];
pub const CARDB_ID: u32 = 0x47A0_7113;
/// PC payload offset, stride, count
pub const CARDB_RECORDS: (usize, usize, usize) = (0x2680, 0x1870, 80);
/// u16 installed-part arrays per car
pub const CARDB_PART_SLOTS: (usize, usize) = (0x3C, 0x186);
/// u8 fields right after the array
pub const CARDB_PART_FLAGS: (usize, usize) = (0x186, 0x190);
/// 3 customization sets per car
pub const CARDB_BLUEPRINT_SETS: [usize; 3] = [0x0, 0x7B4, 0xF68];
/// car table: offset, entry size, count
pub const CARDB_TABLE: (usize, usize, usize) = (0x14, 24, 410);
/// entry word [u8][u8][u8][pad]
pub const CARDB_TABLE_SLOT: usize = 20;

/// Keep property-node flag words in natural byte order.
///
/// Node stream (both platforms): [flag u8 + 3 uninit][data][u32 0][u32 len]
/// ... The flag word follows each [0][len] pair; its first byte is the
/// flag, the other three are heap junk. A blanket u32 swap moves the flag
/// byte to the end of the word.
///
/// The [0][len] header itself must always swap. The string heuristic can
/// swallow it: a NUL-padded name node runs into the next header and the
/// word grid rounds it in, leaving len big-endian (0x04000000 on PC). The
/// PC then drops that node and everything after it - custom race days
/// lost their event lists and the Race Day menu crashed (nfs.exe 0x7F6480).
pub fn fix_node_flags(src: &[u8], out: &mut [u8]) {
    for o in (8..src.len().saturating_sub(3)).step_by(4) {
        let zero = u32::from_be_bytes(src[o - 8..o - 4].try_into().unwrap());
        let ln = u32::from_be_bytes(src[o - 4..o].try_into().unwrap());
        if zero == 0 && (1..=0x400).contains(&ln) {
            for i in 0..4 {
                out[o - 4 + i] = src[o - 1 - i];
            }
            out[o..o + 4].copy_from_slice(&src[o..o + 4]);
        }
    }
}

pub const CUSTOM_RACEDAY_ID: u32 = 0xD548_266C;

/// (offset, length) of each node's data in a 360 property-node stream.
///
/// The first value sits at `start` (after the 360 marker word); every later
/// node is [u32 0][u32 len][flag word][data], its header on the u32 grid
/// after the previous data (junk bytes may sit in between).
pub fn node_spans(src: &[u8], start: usize) -> Vec<(usize, usize)> {
    let be = |o: usize| u32::from_be_bytes(src[o..o + 4].try_into().unwrap());
    let mut spans = vec![(start, 4)];
    let mut o = start + 4;
    while o + 12 <= src.len() {
        let mut h = (o + 3) & !3;
        let mut found = false;
        while h + 8 <= src.len() {
            let ln = be(h + 4);
            if be(h) == 0 && 0 < ln && ln <= 0x400 {
                found = true;
                break;
            }
            h += 4;
        }
        if !found {
            break;
        }
        let ln = be(h + 4) as usize;
        let d = h + 12;
        if d + ln > src.len() {
            break;
        }
        spans.push((d, ln));
        o = d + ln;
    }
    spans
}

/// CustomRaceDayMemcard nodes are u32 values (swapped by the generic
/// pass) or strings: GUID (25 B) and name (36 B), each [4 junk][chars, NUL].
/// String nodes copy byte for byte; the generic string heuristic misses
/// GUIDs followed by a junk byte and half-swapped them.
pub fn fix_custom_raceday_strings(src: &[u8], out: &mut [u8]) {
    for (d, ln) in node_spans(src, 4) {
        if ln > 4 {
            copy_nat(src, out, d, d + ln);
        }
    }
}

/// region holding 8-byte packed entries
pub const CARDB_PACKED: (usize, usize) = (0x7C980, 0x90660);
/// 360 uninitialized 14/16-bit field (0xAAAA masked)
pub const PACKED_FILL: [u8; 2] = [0x2A, 0xAA];

/// `src[a:b]` with Python slice semantics: clamps to the buffer end instead
/// of failing, so a fixed offset beyond a short (corrupt-size) payload is an
/// empty or partial slice, never a panic.
fn py_slice(src: &[u8], a: usize, b: usize) -> &[u8] {
    let hi = b.min(src.len());
    if a >= hi { &[] } else { &src[a..hi] }
}

/// `out[a:b] = src[a:b]` with Python clamping (`src` and `out` always have
/// the same length in the struct fixes: `out` is a clone of the payload).
fn copy_nat(src: &[u8], out: &mut [u8], a: usize, b: usize) {
    let hi = b.min(src.len());
    if a >= hi {
        return;
    }
    out[a..hi].copy_from_slice(&src[a..hi]);
}

/// `out[a:b] = f(src[a:b])` with Python clamping: `f` sees the clamped
/// slice exactly like the Python right-hand side, and its (length-preserving)
/// result lands in the same clamped left-hand slice.
fn put_mapped(src: &[u8], out: &mut [u8], a: usize, b: usize, f: impl FnOnce(&[u8]) -> Vec<u8>) {
    let hi = b.min(src.len());
    if a >= hi {
        return;
    }
    let x = f(&src[a..hi]);
    out[a..hi].copy_from_slice(&x);
}

/// One 8-byte packed entry, 360 BE -> PC LE.
///
/// Layout (both platforms, as u32 pairs): w1 = [u16 index | 14-bit link
/// << 16 | 2 bits], w2 = [u16 low | flag bit << 16 | value << 17].
/// The PC writes 'none' into the link (0x3FFF) and low (0xFFFF) fields and
/// keeps the flag bit clear; the 360 leaves link/low as heap fill (0x2AAA)
/// and the flag bit set on some entries. A walked link of 0x2AAA points
/// past the ~9400-entry table (garage hang). Verified on the matched pairs:
/// 2aaafffe ffff2aaa -> feffff3f fffffeff; value 0x5C85 -> 0x5C84.
pub fn convert_packed_entry(e: &[u8; 8]) -> [u8; 8] {
    let w1 = u32::from_be_bytes(e[0..4].try_into().unwrap());
    let w2 = u32::from_be_bytes(e[4..8].try_into().unwrap());
    let w1 = (w1 & 0xC000_FFFF) | (0x3FFF << 16);
    let w2 = (w2 & 0xFFFE_0000) | 0xFFFF;
    let mut out = [0u8; 8];
    out[0..4].copy_from_slice(&w1.to_le_bytes());
    out[4..8].copy_from_slice(&w2.to_le_bytes());
    out
}

pub fn fix_cardb_packed(src: &[u8], out: &mut [u8]) {
    let (lo, hi) = CARDB_PACKED;
    for o in (lo..hi).step_by(8) {
        // Python reads `src[o + 4:o + 12]` (clamped): a short entry fails
        // the fill check and is skipped.
        let Some(e) = src.get(o + 4..o + 12) else {
            continue;
        };
        if e[0..2] == PACKED_FILL && e[6..8] == PACKED_FILL {
            out[o + 4..o + 12].copy_from_slice(&convert_packed_entry(e.try_into().unwrap()));
        }
    }
}

// per blueprint set (offsets relative to the set; verified vs native PC saves)
/// 12 x [paint: u16,u16][f32][f32]
pub const BP_PAINT: (usize, usize, usize) = (0x194, 12, 12);
/// 20 x 26 B decal slots
pub const BP_DECALS: (usize, usize, usize) = (0x240, 26, 20);
/// 20 x 14 B vinyl slots
pub const BP_VINYLS: (usize, usize, usize) = (0x450, 14, 20);
/// u8[20][9] vinyl colour bytes, natural
pub const BP_COLOURS: (usize, usize) = (0x574, 0x628);

fn swap16s(b: &[u8]) -> Vec<u8> {
    let mut out = b.to_vec();
    for pair in out.as_chunks_mut::<2>().0 {
        pair.reverse();
    }
    // a trailing odd byte copies through unchanged (Python b[i:i+2][::-1])
    out
}

/// Decal/vinyl entry: [s16 x][s16 y][u16][u8 x4][u16 id][u16]...(u16s).
/// All u16 fields swap per u16; the four bytes at +6..+9 stay natural
/// (native PC 'bf 12 12 00' <-> 360 'c0 1b 1b 00' pattern). Sub-slices
/// clamp like Python's for truncated entries.
pub fn convert_decal_entry(e: &[u8]) -> Vec<u8> {
    let mut out = swap16s(py_slice(e, 0, 6));
    out.extend_from_slice(py_slice(e, 6, 10));
    out.extend(swap16s(py_slice(e, 10, usize::MAX)));
    out
}

/// One customization set at 360-payload offset `s` (PC offset + 4).
///
/// Decal/vinyl entries are 26/14 bytes - not u32 aligned - so a blanket
/// u32 swap tears fields across entries (garage hang on customized cars).
pub fn fix_blueprint_set(src: &[u8], out: &mut [u8], s: usize) {
    let (off, step, n) = BP_PAINT;
    for k in 0..n {
        let o = s + off + k * step;
        put_mapped(src, out, o, o + 4, swap16s);
    }
    for &(off, step, n) in &[BP_DECALS, BP_VINYLS] {
        for k in 0..n {
            let o = s + off + k * step;
            put_mapped(src, out, o, o + step, convert_decal_entry);
        }
    }
    let (lo, hi) = BP_COLOURS;
    copy_nat(src, out, s + lo, s + hi);
}

/// Customization part slots are u16 arrays: swap each u16 in place.
///
/// A u32 swap exchanges neighbouring slots, so every installed part lands
/// in the wrong slot and the game falls back to a stock car (verified
/// against the matched-state PC save: starter car slots byte-exact).
/// The u8 fields after the array stay natural. Offsets are PC payload
/// offsets; the 360 payload (still unframed here) has one extra leading
/// word.
pub fn fix_cardb_parts(src: &[u8], out: &mut [u8]) {
    let (base, stride, count) = CARDB_RECORDS;
    let (lo, hi) = CARDB_PART_SLOTS;
    let (flo, fhi) = CARDB_PART_FLAGS;
    for r in 0..count {
        let rec = base + r * stride + 4;
        copy_nat(src, out, rec, rec + 4); // u8 x3 + pad
        put_mapped(src, out, rec + 4, rec + 8, |w| {
            // two u16s, each reversed (Python: s[a:b][::-1] + s[b:d][::-1])
            let mut x = w.to_vec();
            let mid = x.len().min(2);
            let (a, b) = x.split_at_mut(mid);
            a.reverse();
            b.reverse();
            x
        });
        for &bp in &CARDB_BLUEPRINT_SETS {
            let rec0 = rec + bp;
            let mut s = rec0 + lo;
            while s < rec0 + hi {
                put_mapped(src, out, s, s + 2, |w| {
                    let mut x = w.to_vec();
                    x.reverse();
                    x
                });
                s += 2;
            }
            copy_nat(src, out, rec0 + flo, rec0 + fhi);
            fix_blueprint_set(src, out, rec0);
        }
    }
    // car table: last word of every entry is [u8 a][u8 b][u8 slot][pad] -
    // garage slot/index bytes; a u32 swap scrambles which record a car uses
    let (t0, size, n) = CARDB_TABLE;
    for k in 0..n {
        let o = t0 + k * size + CARDB_TABLE_SLOT + 4;
        copy_nat(src, out, o, o + 4);
    }
}

/// [u8][pad x3] words, natural order
pub const GAMEPLAY_U8_FIELDS: [usize; 2] = [0x1F4, 0x2D0];
/// GameplayData: race-day state (0 = none active)
pub const RACEDAY_STATE: usize = 0x2D4;
/// variable-length race-day block starts here
pub const RACEDAY_START: usize = 0x2E0;
/// 360-only 4-byte pad inside the race-day block
pub const RACEDAY_PAD: usize = 0x314;
/// car/name C-string, natural order
pub const RACEDAY_NAME: (usize, usize) = (0x300, 0x310);
/// the list after the block starts [u32 0][u32 0x11]
pub const POST_BLOCK_COUNT: u32 = 0x11;

/// PC offset -> BE u32 in the 360 (unframed) payload.
fn u32be(src: &[u8], o: usize) -> u32 {
    u32::from_be_bytes(src[o + 4..o + 8].try_into().unwrap())
}

/// PC offset where the race-day block ends (= start of the list that
/// follows it: [u32 0][u32 0x11][hash]...). Verified: 0x3E70 on the
/// Battle Machine save, 0xB5B0 on the Willow Springs save (the same list
/// sits at 0x2E0 when no race day is active).
pub fn raceday_block_end(src: &[u8]) -> Option<usize> {
    (RACEDAY_START + 0x40..src.len().saturating_sub(16))
        .step_by(16)
        .find(|&o| {
            u32be(src, o) == 0 && u32be(src, o + 4) == POST_BLOCK_COUNT && u32be(src, o + 8) != 0
        })
}

/// Race-day record header kind word, e.g. 0x00001310, 0x00051110,
/// 0x00081B10: low byte 0x10, next byte 0x11..0x1B (odd), top byte 0.
fn is_record_kind(w: u32) -> bool {
    (w & 0xFF) == 0x10
        && matches!((w >> 8) & 0xFF, 0x11 | 0x13 | 0x15 | 0x17 | 0x19 | 0x1B)
        && (w >> 24) == 0
}

/// GameplayData in-progress race-day block (0x2E0..end, variable length).
///
/// Verified on the mid-race-day pair (Battle Machine, Nevada):
///   * the 360 block has a 4-byte pad at 0x314 the PC lacks: every later
///     field moves back 4 bytes; the freed word lands at the block end;
///   * record headers are [u32 kind][u16][u16] - the second word swaps
///     per u16;
///   * per-event words [u8 flags][u8 0][pad pad] (0xAAAA fill) and the name
///     string at 0x300 stay natural.
///
/// PC offsets; the unframed 360 payload has one extra leading word.
/// Writes clamp like the Python's slice assignments; a payload too short
/// to hold the race-day state word (< 0x2DC) is refused - the Python reads
/// that word with `struct.unpack_from`, which fails there too.
pub fn fix_raceday_block(src: &[u8], out: &mut [u8], warnings: &mut Vec<String>) -> Result<()> {
    for &o in &GAMEPLAY_U8_FIELDS {
        copy_nat(src, out, o + 4, o + 8);
    }
    let Some(state) = src
        .get(RACEDAY_STATE + 4..RACEDAY_STATE + 8)
        .map(|w| u32::from_be_bytes(w.try_into().unwrap()))
    else {
        return Err(format_err(format!(
            "GameplayData chunk too short ({:#x} B) to hold the race-day state - \
             the source file is corrupted",
            src.len()
        )));
    };
    let active = state != 0;
    let end = if active {
        raceday_block_end(src)
    } else {
        Some(RACEDAY_START)
    };
    // race-day name strings ('MV03_BattleMachine') live after the block;
    // string detection is restricted there because float pairs inside the
    // block can look printable
    if let Some(end) = end {
        // Python `src[end + 4:]` clamps to an empty slice when the payload
        // ends before the (zero-state) block start: a payload in
        // [0x2DC, 0x2E4) converts with the tail empty, never panics.
        let tail = src.get(end + 4..).unwrap_or(&[]);
        for (a, b) in find_string_runs(tail) {
            copy_nat(src, out, end + 4 + a, end + 4 + b);
        }
    }
    if !active {
        return Ok(());
    }
    let Some(end) = end else {
        warnings.push(
            "GameplayData: active race day but block end not found - \
             race day will not resume"
                .to_string(),
        );
        return Ok(());
    };
    let (lo, hi) = RACEDAY_NAME;
    copy_nat(src, out, lo + 4, hi + 4);
    let mut prev_kind = false;
    for o in (RACEDAY_PAD + 4..end).step_by(4) {
        // 360 word positions (PC coords)
        let w: [u8; 4] = src[o + 4..o + 8].try_into().unwrap();
        if prev_kind {
            out[o + 4..o + 8].copy_from_slice(&[w[1], w[0], w[3], w[2]]); // two u16s
        } else if w[1] == 0 && w[2..] == [0xAA, 0xAA] {
            out[o + 4..o + 8].copy_from_slice(&w); // [u8][u8][pad pad]
        }
        prev_kind = is_record_kind(u32::from_be_bytes(w));
    }
    let (a, e) = (RACEDAY_PAD + 4, end + 4);
    out.copy_within(a + 4..e, a);
    out[e - 4..e].fill(0);
    Ok(())
}

/// Race-day progress table in GameplayData: 90 x [u32 key][u32 state][u32 score],
/// after the race-day block (position varies). Located by its first and last key.
pub const PROGRESS_FIRST: u32 = 0xA70EA9B0;
pub const PROGRESS_LAST: u32 = 0xFA5D360A;
pub const PROGRESS_LEN: usize = 90;

/// Race days the 360 marks with console-only state (bits 0x04/0x08/0x10 and a
/// score, even in a fresh career with no custom race days) that the PC never
/// writes: every PC save has them at state 0 or 2, score 0 - fresh, mid-career
/// and 100%, and a PC save made after creating a custom race day left them
/// untouched. Five (state 0 here: 8F7CCCE0 46AE8E2F C8A0888E 0A6C2097
/// AF51A403) are the custom race-day SLOTS (career [0xAB9DC8]+0xB0); they are
/// not in gameplay.bin. Values = native PC side of the matched pairs;
/// 0x8DA1975B from the PC 100% save. (Not the Race Day crash cause - that was
/// the CustomRaceDayMemcard node framing, see fix_node_flags.)
pub const CONSOLE_ONLY_RACEDAYS: [(u32, u32); 17] = [
    (0xB48C11C4, 2), (0x0A6C2097, 0), (0xF841FB9F, 2), (0x8DA1975B, 0),
    (0x8F7CCCE0, 0), (0x92407122, 2), (0x5C838C1A, 0), (0xAF51A403, 0),
    (0xDDCEF290, 2), (0x46AE8E2F, 0), (0xB3F02D70, 2), (0x8FEB3CC6, 2),
    (0xC8A0888E, 0), (0x66705CF6, 2), (0x150B07D4, 2), (0xD663D2A8, 2),
    (0x21471712, 2),
];

/// Offset of the race-day progress table in a little-endian payload.
pub fn progress_table_offset(p: &[u8]) -> Option<usize> {
    let first = PROGRESS_FIRST.to_le_bytes();
    let word = |at: usize| u32::from_le_bytes(p[at..at + 4].try_into().unwrap());
    let mut from = 0;
    while let Some(i) = p[from..].windows(4).position(|w| w == first) {
        let o = from + i;
        let last = o + 12 * (PROGRESS_LEN - 1);
        if last + 4 <= p.len() && word(last) == PROGRESS_LAST {
            return Some(o);
        }
        from = o + 1;
    }
    None
}

/// Reset console-only race days to their PC-native state (see
/// CONSOLE_ONLY_RACEDAYS). Operates on the already-swapped payload.
pub fn fix_raceday_progress(out: &mut [u8], warnings: &mut Vec<String>) {
    let Some(o) = progress_table_offset(out) else {
        warnings.push(
            "GameplayData: race-day progress table not found - \
             the PC Race Day menu may crash"
                .to_string(),
        );
        return;
    };
    for i in 0..PROGRESS_LEN {
        let e = o + 12 * i;
        let key = u32::from_le_bytes(out[e..e + 4].try_into().unwrap());
        if let Some(&(_, state)) = CONSOLE_ONLY_RACEDAYS.iter().find(|(k, _)| *k == key) {
            out[e + 4..e + 8].copy_from_slice(&state.to_le_bytes());
            out[e + 8..e + 12].fill(0);
        }
    }
}

pub fn apply_struct_fixes(rec: &mut Record, src: &[u8], warnings: &mut Vec<String>) -> Result<()> {
    let mut out = rec.payload.clone();
    if rec.id == GAMEPLAY_ID {
        fix_raceday_block(src, &mut out, warnings)?;
        fix_raceday_progress(&mut out, warnings);
    } else if rec.id == CARDB_ID {
        fix_cardb_parts(src, &mut out);
        fix_cardb_packed(src, &mut out);
    } else if !RAW_BLOB_IDS.contains(&rec.id) {
        fix_node_flags(src, &mut out);
        if rec.id == CUSTOM_RACEDAY_ID {
            fix_custom_raceday_strings(src, &mut out);
        }
    }
    rec.payload = out;
    Ok(())
}

/// PC payload offset/size of the gameplay blob
pub const GAMEPLAY_BLOB: (usize, usize) = (0x14, 0x10000);

/// GameplayData blob = [MD5(rest)][rest]; recompute after conversion.
///
/// The PC deserializer (nfs.exe 0x59E550 -> [0xAB9D88] vtbl+0x70) rejects
/// a blob whose MD5 does not match and the career starts from scratch
/// (intro movie). Verified on native PC saves: blob[0:16] ==
/// md5(blob[16:0x10000]). Operates on the PC-framed payload.
pub fn rehash_gameplay(rec: &mut Record) {
    if rec.id != GAMEPLAY_ID {
        return;
    }
    let (off, size) = GAMEPLAY_BLOB;
    let mut p = rec.payload.clone();
    let mut h = Md5::new();
    // Python clamps the MD5 input to the payload: a short (corrupt-size)
    // gameplay record hashes the tail that exists instead of panicking.
    h.update(py_slice(&p, off + 16, off + size));
    let d = h.finalize();
    if p.len() < off + 16 {
        // Python bytearray slice assignment grows the buffer: the digest
        // replaces the tail and the payload becomes off+16 bytes.
        p.truncate(off);
        p.extend_from_slice(&d);
    } else {
        p[off..off + 16].copy_from_slice(&d);
    }
    rec.payload = p;
}

/// Re-frame a converted record for PC-native emission.
///
/// 360 records: [prev tail word][id][size][payload = marker word + content].
/// PC records:  [id][size][flags=1][payload = content + tail word] with
/// the same total size. The 360 marker word maps onto the PC flags slot;
/// we emit the native flags pattern 0x00000001 instead. The tail word is
/// the record's final value, which the 360 stores in the NEXT record's
/// header slot (see tail_word).
pub fn to_pc_record_with(rec: &mut Record, tail: [u8; 4]) {
    rec.flags = 0x0000_0001;
    if !rec.payload.is_empty() {
        // Python `payload[4:]` is empty for payloads shorter than 4 bytes;
        // the tail word still lands, so the result is just the tail.
        let mut p = if rec.payload.len() < 4 {
            Vec::new()
        } else {
            rec.payload[4..].to_vec()
        };
        p.extend_from_slice(&tail);
        rec.payload = p;
    }
}

/// `to_pc_record_with` and a zero tail word (the twin path).
pub fn to_pc_record(rec: &mut Record) {
    to_pc_record_with(rec, [0; 4]);
}

/// PC byte order for a record's final word, taken from the 360 word at
/// the next record's header slot (`spill`, big-endian as stored).
///
/// Verified: CAREER_01 CustomRaceDayMemcard spills 0x00000001 (its last
/// event flag; the PC-written race day ends 01 00 00 00 too) and FECareer
/// spills 0x2848 in every 360 sample. Node streams ending in a u32 node
/// ([0][4][flag] before the spill) swap it; raw memcpy blobs swap like the
/// rest of the blob; anything else (a string node running into the spill)
/// stays natural. GameplayData keeps zero: its 360 buffer is trimmed, so
/// the spilled word is 360-only padding.
pub fn tail_word(rec_id: u32, src: &[u8], spill: &[u8]) -> [u8; 4] {
    let Ok(sp) = <[u8; 4]>::try_from(spill) else {
        return [0; 4];
    };
    if rec_id == GAMEPLAY_ID {
        return [0; 4];
    }
    let n = src.len();
    let u32_node_end = n >= 12 && src[n - 12..n - 4] == [0, 0, 0, 0, 0, 0, 0, 4];
    if RAW_BLOB_IDS.contains(&rec_id) || u32_node_end {
        [sp[3], sp[2], sp[1], sp[0]]
    } else {
        sp
    }
}

/// Reject a re-save twin that is not the same career session.
///
/// Verified on the real pair: the correct twin shares the source's record
/// id sequence and every overlapping record's payload size (payload bytes
/// themselves differ - volatile junk words/timestamps). A different day's
/// save diverges in sizes and/or sequence.
pub fn validate_twin(src: &Tree, twin: &Tree) -> Result<()> {
    if twin.gap != 0 {
        return Err(format_err(format!(
            "twin file has a damaged record tail ({:#x} B) - it cannot be used for recovery",
            twin.gap
        )));
    }
    let src_ids: Vec<(u32, usize)> = src
        .records
        .iter()
        .map(|r| (r.id, r.payload.len()))
        .collect();
    let twin_ids: Vec<(u32, usize)> = twin
        .records
        .iter()
        .take(src.records.len())
        .map(|r| (r.id, r.payload.len()))
        .collect();
    if src_ids != twin_ids {
        return Err(format_err(
            "twin does not match the source career (record ids/sizes differ) - \
             it is a different session; pass the correct --twin or drop it",
        ));
    }
    Ok(())
}

pub fn convert_tree(
    tree360: &mut Tree,
    report: &mut ConversionReport,
    mut twin: Option<Tree>,
) -> Result<Tree> {
    if tree360.gap != 0 {
        report.warnings.push(format!(
            "{:#x} bytes of damaged noise inside the console record region \
             (known console writing bug)",
            tree360.gap
        ));
    }
    if !rules_loaded() {
        report
            .warnings
            .push("fieldmap rules unavailable - all chunks converted in auto mode".into());
    }

    let mut pc = Tree {
        noise: tree360.noise.clone(),
        count: 0,
        pre_records: Vec::new(),
        records: Vec::new(),
        post: if tree360.post.is_empty() {
            Vec::new()
        } else {
            convert_payload_auto(&tree360.post, None, "chunk")
        },
        used: 0,
        gap: 0,
    };
    let kind = report.kind.clone();
    // a 360 record's final word sits in the next record's header slot; the
    // last record's in the word after the record area (unless that is noise)
    let mut spills: Vec<Vec<u8>> = tree360
        .records
        .iter()
        .skip(1)
        .map(|r| r.flags.to_be_bytes().to_vec())
        .collect();
    spills.push(if tree360.gap == 0 {
        tree360.post.iter().take(4).copied().collect()
    } else {
        Vec::new()
    });
    for (rec, spill) in tree360.records.iter_mut().zip(spills) {
        normalize_gameplay(rec, &mut report.warnings);
        let src = rec.payload.clone();
        let mode: &str = if rec.id == GAMEPLAY_ID {
            // variable layout (race-day block) - positional maps do not apply;
            // fields are u32/float except the fixes in fix_raceday_block.
            // A record size that is not a multiple of 4 cannot come from a
            // real save; the Python asserts there - refuse instead.
            if rec.payload.len() % 4 != 0 {
                return Err(format_err(format!(
                    "GameplayData chunk size {:#x} is not word-aligned - \
                     the source file is corrupted",
                    rec.payload.len()
                )));
            }
            rec.payload = swap_u32s(&rec.payload);
            "gameplay"
        } else {
            convert_record(&kind, rec, Some(&mut report.warnings))
        };
        apply_struct_fixes(rec, &src, &mut report.warnings)?;
        if mode == "auto" && rec.payload.len() > 0x1000 && rec.id != GAMEPLAY_ID {
            report.warnings.push(format!(
                "chunk {} ({:#x} B) converted in auto mode (no fieldmap)",
                chunk_name(rec.id),
                rec.payload.len()
            ));
        }
        to_pc_record_with(rec, tail_word(rec.id, &src, &spill));
        rehash_gameplay(rec);
        pc.records.push(rec.clone());
    }
    if let Some(ref mut twin) = twin {
        // console tail damaged: rebuild the sequence in the twin's order,
        // substituting the console records wherever the ids match, so the
        // positional pairing keeps the loader's registration order
        for trec in twin.records.iter_mut() {
            normalize_gameplay(trec, &mut report.warnings);
        }
        validate_twin(tree360, twin)?;
        // Python: by_id = {r.id: r for r in pc.records} — duplicate ids
        // collapse last-wins, and `order` keeps the dict's first-occurrence
        // iteration order for the leftovers appended below.
        let mut by_id: HashMap<u32, Record> = HashMap::new();
        let mut order: Vec<u32> = Vec::new();
        for r in std::mem::take(&mut pc.records) {
            let id = r.id;
            if by_id.insert(id, r).is_none() {
                order.push(id);
            }
        }
        let mut merged: Vec<Record> = Vec::new();
        for mut trec in std::mem::take(&mut twin.records) {
            if let Some(pcrec) = by_id.remove(&trec.id) {
                merged.push(pcrec);
            } else {
                let id = trec.id;
                convert_record(&kind, &mut trec, Some(&mut report.warnings));
                to_pc_record(&mut trec);
                report.warnings.push(format!(
                    "record {} recovered from re-save twin (console copy damaged)",
                    chunk_name(id)
                ));
                merged.push(trec);
            }
        }
        if !by_id.is_empty() {
            // remaining console records (ids absent from the twin) keep their
            // first-occurrence order at the end, one entry per id
            let remaining: Vec<Record> = order
                .iter()
                .filter_map(|id| by_id.remove(id))
                .collect();
            let left = remaining
                .iter()
                .map(|r| format!("{:#x}", r.id))
                .collect::<Vec<_>>()
                .join(", ");
            report
                .warnings
                .push(format!("records absent from twin kept at end: {left}"));
            merged.extend(remaining);
        }
        pc.records = merged;
    } else if tree360.gap != 0 && pc.records.len() < tree360.count as usize {
        report.warnings.push(
            "console record region damaged with no twin available - missing chunks \
             convert as absent and the game fills defaults"
                .to_string(),
        );
    }
    pc.count = pc.records.len() as u32;
    // PC tree head: [allocator garbage (never read)][root record: id/used/flags=1]
    // followed by records at tree+0x1CC in native [id][size][flags][payload] framing
    pc.pre_records = {
        let mut pre = vec![0u8; PC_HEAD_STRUCT_SIZE];
        pre.extend_from_slice(&TREE_MAGIC.to_le_bytes());
        pre.extend_from_slice(&0u32.to_le_bytes());
        pre.extend_from_slice(&0x0000_0001u32.to_le_bytes());
        pre
    };
    // positional pairing: the PC loader walks savable[i] against record[i];
    // a chunk the 360 never writes must hold its slot with a size-0 filler
    // or every later pairing desyncs (PCControllerSettings sits between
    // AudioSettings and PlayerSettings0 in the PC registration order)
    if report.kind == "alias" && !pc.records.iter().any(|r| r.id == 0x3915_6567) {
        let ps0 = pc
            .records
            .iter()
            .position(|r| r.id == 0x8B7D_0AAD)
            .unwrap_or(pc.records.len());
        pc.records.insert(
            ps0,
            Record {
                flags: 0x0000_0001,
                id: 0x3915_6567,
                size: 0,
                payload: Vec::new(),
            },
        );
        pc.count = pc.records.len() as u32;
    }
    report.chunk_list = pc.records.iter().map(|r| chunk_name(r.id)).collect();
    Ok(pc)
}

/// Full payload conversion: BE MC02 -> LE MC02 (see [`convert_tree`]).
///
/// `twin_payload` is a re-save MC02 (or CON-wrapped, see [`load_twin`])
/// used for tail recovery when the console record region is damaged.
pub fn convert_payload(
    mc02_be: &MC02,
    report: Option<&mut ConversionReport>,
    twin_payload: Option<&[u8]>,
) -> Result<MC02> {
    let mut scratch = ConversionReport {
        kind: "career".into(),
        ..Default::default()
    };
    let report: &mut ConversionReport = match report {
        Some(r) => r,
        None => &mut scratch,
    };
    let mut tree360 = Tree::parse(&mc02_be.tree, true)?;
    report.kind = if mc02_be.extra.len() == 64 {
        "alias".into()
    } else {
        "career".into()
    };
    let mut twin = None;
    // Python: `if twin_payload and ...` — an EMPTY twin slice is falsy and
    // means "no twin"; it must not fail the conversion with a parse error.
    if let Some(tp) = twin_payload.filter(|tp| !tp.is_empty())
        && report.kind == "career"
        && tree360.gap != 0
    {
        twin = Some(Tree::parse(&MC02::parse(tp)?.tree, true)?);
    }
    let pc_tree = convert_tree(&mut tree360, report, twin)?;
    report.records = pc_tree.records.len();
    let tree_size = mc02_be.tree_size as usize;
    let mut tree_bytes = pc_tree.build(false, tree_size)?;
    // Python sums in arbitrary precision and raises in struct.pack when the
    // used size leaves u32 range; overflow-check instead of wrapping.
    let used = pc_tree.records.iter().try_fold(0u32, |acc, r| {
        acc.checked_add(12)
            .and_then(|v| v.checked_add(r.payload.len() as u32))
            .ok_or_else(|| {
                format_err("converted tree used size exceeds 32 bits - the source file is corrupted")
            })
    })?;
    // native PC saves carry the built tree's used size in the extra blob
    // (word 1), careers and aliases alike; copy it through so the loader sees
    // a consistent pair. Aliases need it most: the inserted size-0
    // PCControllerSettings record adds 12 bytes, and a stale value made the
    // PC skip the alias and run on a default 'Player' profile.
    let mut extra = convert_extra(&mc02_be.extra)?;
    extra[4..8].copy_from_slice(&used.to_le_bytes());
    let th = tree_hash(&tree_bytes);
    tree_bytes[0..16].copy_from_slice(&th);
    Ok(MC02::new(
        Endian::Little,
        extra,
        tree_bytes,
        mc02_be.tree_size,
    ))
}

/// Accept either a raw MC02 re-save or one still inside its CON wrapper.
pub fn load_twin(data: &[u8]) -> Result<Vec<u8>> {
    if data.starts_with(b"CON ") {
        Ok(parse_container(data, "twin")?.payload)
    } else {
        Ok(data.to_vec())
    }
}

/// Write a converted save to the PC save layout `<save_root>/<name>/<name>`.
///
/// The bytes land in `<target>.tmp` first and are renamed over the target,
/// so an interrupted write (window close, full disk, unplugged destination)
/// can never leave a truncated file silently replacing a good export; the
/// previous export survives and the leftover `.tmp` is removed on failure.
/// `std::fs::rename` refuses to replace an existing file on Windows, so an
/// existing target is removed first (a brief non-atomic gap).
pub fn write_pc_save(mc02_pc: &MC02, name: &str, save_root: &Path) -> Result<PathBuf> {
    if name.is_empty() || name.contains(['\\', '/', ':']) || name == "." || name == ".." {
        return Err(format_err(format!("unsafe save name {name:?}")));
    }
    let folder = save_root.join(name);
    fs::create_dir_all(&folder)?;
    let target = folder.join(name);
    let tmp = folder.join(format!("{name}.tmp"));
    let bytes = mc02_pc.to_bytes()?;
    let write = || -> std::io::Result<()> {
        use std::io::Write;
        let mut f = fs::File::create(&tmp)?;
        f.write_all(&bytes)?;
        f.sync_all()?;
        drop(f);
        if target.exists() {
            fs::remove_file(&target)?;
        }
        fs::rename(&tmp, &target)
    };
    if let Err(e) = write() {
        let _ = fs::remove_file(&tmp); // never leave a stray .tmp behind
        return Err(e.into());
    }
    Ok(target)
}

/// Outcome of [`convert_one`]: where the save was written, the conversion
/// report, and the post-write self-check problems (empty == OK).
pub struct ConvertOutcome {
    pub target: PathBuf,
    pub report: ConversionReport,
    pub self_check: Vec<String>,
}

/// The app-facing pipeline (mirrors `convert_one` in scripts/python/convert.py):
/// 360 CON container bytes -> payload -> MC02 parse -> validate (refuse on
/// extra-blob CRC mismatch) -> convert to PC payload -> write
/// `<out_root>/<NAME>/<NAME>` -> re-parse the written file and self-check.
pub fn convert_one(
    src_bytes: &[u8],
    label: &str,
    out_root: &Path,
    twin: Option<&[u8]>,
) -> Result<ConvertOutcome> {
    let cont = parse_container(src_bytes, label)?;
    let mc02 = MC02::parse(&cont.payload)?;
    let bad = mc02.check();
    if bad.iter().any(|p| p == "extra CRC mismatch") {
        return Err(Error::Format(format!(
            "{label}: extra-blob CRC mismatch - the source file is corrupted; \
             refusing to convert"
        )));
    }
    let mut report = ConversionReport {
        source: label.to_string(),
        ..Default::default()
    };
    for prob in &bad {
        report
            .warnings
            .push(format!("{prob} (CRCs are recomputed on write)"));
    }
    let pc = convert_payload(&mc02, Some(&mut report), twin)?;
    let target = write_pc_save(&pc, &cont.name, out_root)?;
    let self_check = MC02::parse(&fs::read(&target)?)?.check();
    Ok(ConvertOutcome {
        target,
        report,
        self_check,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mc02_with(extra_word: u32) -> MC02 {
        let mut extra = vec![0u8; 28];
        extra[0..4].copy_from_slice(&extra_word.to_le_bytes());
        MC02::new(Endian::Little, extra, vec![0xA5; 0x100], 0x100)
    }

    /// A pre-existing target is fully replaced and no `.tmp` remains: the
    /// write goes through `<target>.tmp` + rename, so an interrupted write
    /// can never leave a truncated export silently in place of a good one.
    #[test]
    fn write_pc_save_replaces_existing_target_without_tmp_leftover() {
        let dir = std::env::temp_dir().join(format!("nfssave-write-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        // seed a stale export with different content
        let save = mc02_with(1);
        let stale = mc02_with(2);
        let folder = dir.join("CAREER_XX");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("CAREER_XX"), stale.to_bytes().unwrap()).unwrap();

        let target = write_pc_save(&save, "CAREER_XX", &dir).unwrap();
        assert_eq!(target, folder.join("CAREER_XX"));
        assert_eq!(fs::read(&target).unwrap(), save.to_bytes().unwrap());
        assert!(
            !folder.join("CAREER_XX.tmp").exists(),
            "no .tmp may survive a successful write"
        );
        // a second write replaces again, still atomically
        write_pc_save(&mc02_with(3), "CAREER_XX", &dir).unwrap();
        assert_eq!(
            fs::read(&target).unwrap(),
            mc02_with(3).to_bytes().unwrap(),
            "pre-existing target must be fully replaced"
        );
        assert!(!folder.join("CAREER_XX.tmp").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
