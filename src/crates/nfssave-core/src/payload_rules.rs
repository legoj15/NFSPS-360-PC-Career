//! Payload conversion engines: 360 (BE) -> PC (LE) per chunk.
//!
//! All record and nested-record sizes are multiples of 4, so payloads are u32-
//! tiled end to end and headers convert with the same pass as data.
//!
//! Rule sources, in priority order:
//! 1. Fieldmap slot rules ([`fieldmaps_parsed.json`], embedded from
//!    `scripts/python/nfssave/fieldmaps_parsed.json`) — derived from the
//!    matched 360/PC sample pair; positionally valid only where the payload
//!    layout is rigid. Class semantics (from docs/re/match.py in the repo):
//!    NUM   wpc == swap(w360)  -> swap
//!    SAME  wpc == w360 (raw)  -> copy bytes
//!    STR   printable both     -> copy bytes
//!    STR360 360 string, PC leaves fill/small-int -> copy 360 bytes
//!    DIFF  any other          -> swap (player values differ)
//!    FILL  garbage both sides -> swap (value-preserving)
//!    A payload whose size differs from the map reference, or whose EA-string
//!    positions disagree with the map's copy/swap slots, falls back to auto
//!    mode (the map was derived from a different session's content).
//! 2. Auto mode for unmapped chunks: detect string regions and keep them in
//!    natural byte order; swap every other u32; keep [value][FF FF FF]
//!    sub-word fields natural. String regions are quantized to the u32 grid
//!    so no word ends up half-swapped/half-natural.
//!
//! The JSON is regenerated from the fieldmap sources (docs/re/fieldmaps/*.txt)
//! by the Python `regenerate_rules` tool; that maintenance utility is not
//! ported (the packaged JSON is embedded at compile time).

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde::Deserialize;

use crate::tree::Record;

pub const COPY_CLASSES: [&str; 3] = ["STR", "STR360", "SAME"];

const RULES_JSON: &str = include_str!("../../../../scripts/python/nfssave/fieldmaps_parsed.json");

#[derive(Deserialize)]
struct FileRules {
    alias: HashMap<String, FileChunk>,
    career: HashMap<String, FileChunk>,
}

#[derive(Deserialize)]
struct FileChunk {
    acts: HashMap<String, String>,
    #[serde(default)]
    ref_size: Option<u64>,
}

/// Per-chunk fieldmap: word offset -> class name.
#[derive(Debug, Clone)]
pub struct ChunkRules {
    pub acts: HashMap<u32, String>,
    pub ref_size: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct Rules {
    pub alias: HashMap<u32, ChunkRules>,
    pub career: HashMap<u32, ChunkRules>,
}

static RULES: OnceLock<Option<Rules>> = OnceLock::new();

/// Loaded fieldmap rules (`None` mirrors the Python's `RULES_LOADED == False`
/// fallback to pure auto mode).
pub fn rules() -> Option<&'static Rules> {
    RULES.get_or_init(load_rules).as_ref()
}

pub fn rules_loaded() -> bool {
    rules().is_some()
}

fn load_rules() -> Option<Rules> {
    let raw: FileRules = serde_json::from_str(RULES_JSON).ok()?;
    let convert = |kind: HashMap<String, FileChunk>| -> Option<HashMap<u32, ChunkRules>> {
        let mut out = HashMap::with_capacity(kind.len());
        for (cid, chunk) in kind {
            let id = cid.parse::<u32>().ok()?;
            let mut acts = HashMap::with_capacity(chunk.acts.len());
            for (off, class) in chunk.acts {
                acts.insert(off.parse::<u32>().ok()?, class);
            }
            out.insert(id, ChunkRules {
                acts,
                ref_size: chunk.ref_size,
            });
        }
        Some(out)
    };
    Some(Rules {
        alias: convert(raw.alias)?,
        career: convert(raw.career)?,
    })
}

fn printable(b: u8) -> bool {
    (0x20..0x7F).contains(&b)
}

/// EA-encoded strings: [0x40|len][len-1 printable chars].
///
/// `min_len` trades recall for precision: a 4-byte detection (3 printable
/// chars after a header byte) is statistically indistinguishable from a
/// numeric word whose bytes happen to be printable (verified on the
/// matched pair: 'D#7f' <-> 'f7#D' is a plain BE/LE u32), so the default
/// of 5 (see [`find_string_runs`]) only accepts >= 5. Callers that need
/// high confidence (map cross-checks) pass 8.
pub fn ea_string_ranges(payload: &[u8], min_len: usize) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut i = 0;
    let n = payload.len();
    while i < n {
        let b = payload[i] as usize;
        let ln = (payload[i] & 0x3F) as usize;
        if (0x40 + min_len..=0x7F).contains(&b)
            && ln >= min_len
            && i + ln <= n
            && payload[i + 1..i + ln].iter().all(|&c| printable(c))
        {
            runs.push((i, i + ln));
            i += ln;
            continue;
        }
        i += 1;
    }
    runs
}

/// Fixed-width C-string fields: printable text in a NUL/0xAA-padded slot.
///
/// Accepts a printable run of >= 5 chars that is followed by at least two
/// fill bytes (0x00/0xAA) or the end of the payload; runs may bridge
/// through fill into a further printable segment of >= 4 chars (observed:
/// car blueprint fields like 'MV09_Nitrocide\0\0ion\0' + 0xAA fill).
/// The trailing-fill requirement rejects lone NUL bytes inside numeric
/// data, which caused most false positives.
fn fixed_string_ranges(payload: &[u8]) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let mut i = 0;
    let n = payload.len();
    while i < n {
        let mut j = i;
        while j < n && printable(payload[j]) {
            j += 1;
        }
        if j - i >= 5 {
            let mut k = j;
            let mut last_text_end = j;
            while k < n {
                let c = payload[k];
                if c == 0x00 || c == 0xAA {
                    k += 1;
                } else if printable(c) {
                    let mut m2 = k;
                    while m2 < n && printable(payload[m2]) {
                        m2 += 1;
                    }
                    if m2 - k >= 4 {
                        k = m2;
                        last_text_end = m2;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            let fill_after = k - last_text_end;
            if k >= n || fill_after >= 2 {
                runs.push((i, k));
                i = k;
                continue;
            }
        }
        i = if j > i { j + 1 } else { i + 1 };
    }
    runs
}

fn merge(mut ranges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    ranges.sort_unstable();
    ranges.dedup();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (s, e) in ranges {
        if let Some(last) = merged.last_mut()
            && s <= last.1
        {
            if e > last.1 {
                last.1 = e;
            }
            continue;
        }
        merged.push((s, e));
    }
    merged
}

/// Snap range boundaries to the u32 grid and re-merge.
///
/// Conversion decisions are per-word (a word is copied whole or swapped
/// whole); without this, a string starting mid-word leaves that word
/// half-swapped/half-natural.
fn quantize(ranges: Vec<(usize, usize)>) -> Vec<(usize, usize)> {
    merge(
        ranges
            .into_iter()
            .map(|(s, e)| (s & !3, (e + 3) & !3))
            .collect(),
    )
}

/// Word-aligned byte ranges of payload to keep in natural order.
pub fn find_string_runs(payload: &[u8]) -> Vec<(usize, usize)> {
    let mut raw = ea_string_ranges(payload, 5);
    raw.extend(fixed_string_ranges(payload));
    quantize(raw)
}

fn swap_tiled(payload: &[u8], natural_ranges: &[(usize, usize)]) -> Vec<u8> {
    let mut out = payload.to_vec();
    for i in (0..payload.len().saturating_sub(3)).step_by(4) {
        out[i..i + 4].reverse();
    }
    for &(s, e) in natural_ranges {
        out[s..e].copy_from_slice(&payload[s..e]);
    }
    out
}

/// True for [value byte][FF FF FF] words: a sub-word (u8) field with
/// uninitialized tail bytes, serialized in natural byte order on both
/// platforms (observed: 360 '01 ff ff ff' <-> PC '01 00 5d 00'). The
/// meaningful byte stays at offset 0; swapping would move it to the end.
fn subword_garbage(word: &[u8]) -> bool {
    word[0] != 0xFF && word[1..] == [0xFF, 0xFF, 0xFF]
}

/// Auto-mode conversion; `warnings` mirrors the Python's optional list.
pub fn convert_payload_auto(
    payload: &[u8],
    mut warnings: Option<&mut Vec<String>>,
    label: &str,
) -> Vec<u8> {
    let mut raw = ea_string_ranges(payload, 5);
    raw.extend(fixed_string_ranges(payload));
    if let Some(w) = warnings.as_mut() {
        let n_unaligned = raw
            .iter()
            .filter(|&&(s, e)| s % 4 != 0 || e % 4 != 0)
            .count();
        if n_unaligned != 0 {
            w.push(format!(
                "{label}: {n_unaligned} string run(s) not word-aligned; \
                 padded to word grid (leading bytes are format padding)"
            ));
        }
    }
    let mut natural: Vec<(usize, usize)> = Vec::new();
    for i in (0..payload.len().saturating_sub(3)).step_by(4) {
        if subword_garbage(&payload[i..i + 4]) {
            natural.push((i, i + 4));
        }
    }
    natural.extend(quantize(raw));
    swap_tiled(payload, &natural)
}

/// Word offsets covered by a detected string region (word-quantized).
fn string_word_set(payload: &[u8]) -> HashSet<usize> {
    let mut words = HashSet::new();
    for (s, e) in find_string_runs(payload) {
        let mut o = s;
        while o < e {
            words.insert(o);
            o += 4;
        }
    }
    words
}

/// Apply per-word fieldmap classes.
///
/// COPY classes and natural-order SAME words (equal and non-palindrome on
/// the reference pair) copy. NUM words swap. ZERO and DIFF words were
/// empty or volatile in the reference pair - they say nothing about byte
/// order and a richer save may hold real content there - so they convert
/// with the auto grammar: swap unless the word sits in a detected string
/// region or is a sub-word field.
pub fn convert_payload_mapped(payload: &[u8], acts: &HashMap<u32, String>) -> Vec<u8> {
    let str_words = string_word_set(payload);
    let mut out = payload.to_vec();
    for off in (0..payload.len().saturating_sub(3)).step_by(4) {
        let off32 = off as u32;
        let class = acts.get(&off32).map(String::as_str);
        if let Some(c) = class
            && COPY_CLASSES.contains(&c)
        {
            continue;
        }
        let word: [u8; 4] = payload[off..off + 4].try_into().unwrap();
        let swapped = [word[3], word[2], word[1], word[0]];
        if class == Some("DIFF") || class == Some("ZERO") {
            if str_words.contains(&off) || subword_garbage(&word) {
                continue;
            }
            out[off..off + 4].copy_from_slice(&swapped);
            continue;
        }
        out[off..off + 4].copy_from_slice(&swapped);
    }
    out
}

/// Convert a Record's payload in place (BE -> LE). Returns the mode used.
pub fn convert_record(
    kind: &str,
    rec: &mut Record,
    mut warnings: Option<&mut Vec<String>>,
) -> &'static str {
    let label = format!("chunk {:#x}", rec.id);
    let entry = rules().and_then(|r| match kind {
        "alias" => r.alias.get(&rec.id),
        "career" => r.career.get(&rec.id),
        _ => None,
    });
    let Some(entry) = entry else {
        rec.payload = convert_payload_auto(&rec.payload, warnings, &label);
        return "auto";
    };
    if let Some(ref_size) = entry.ref_size
        && ref_size as usize != rec.payload.len()
    {
        if let Some(w) = warnings.as_mut() {
            w.push(format!(
                "{label} size {:#x} != map reference {ref_size:#x}; \
                 converted in auto mode (positional rules unsafe)",
                rec.payload.len()
            ));
        }
        rec.payload = convert_payload_auto(&rec.payload, warnings, &label);
        return "auto";
    }
    rec.payload = convert_payload_mapped(&rec.payload, &entry.acts);
    "mapped"
}
