//! Chunk-tree parse/convert for NFS ProStreet MC02 saves.
//!
//! Grammar (verified empirically and cross-checked against PC-native saves):
//!
//! ```text
//! tree := noise[16] count:u32 pad:magic 0x59F2D89B used:u32 records...
//! ```
//!   - 360: records start at tree+0x48 (record = [junk][id][size][payload]);
//!     PC: tree+0x1CC (record = [id][size][flags][payload]), root record at
//!     tree+0x1C0 (360: +0x40).
//!   - id is the platform-independent chunk identity (matches 360<->PC).
//!   - the first payload word of a 360 record is a 0x01xxxxxx marker + junk;
//!     PC drops it and appends the last data word (Record.tail; same total size).
//!   - records tile exactly to records_start + used. The console writing bug
//!     can leave damaged noise in the record region: a trailing gap (records
//!     missing at the end) or, in principle, an internal gap (parse
//!     re-anchors at the first offset where a clean record chain tiles to the
//!     end again).
//!   - after the record area, the rest of the fixed-size tree buffer holds a
//!     hash directory table ([h1][h2][0xFFFFFFFF][0] cells); the loader never
//!     reads it on either platform (byte-swapped on conversion).
//!
//! Payload conversion engines live in [`crate::payload_rules`]; the platform
//! buffer normalization for GameplayData lives in
//! [`crate::convert::normalize_gameplay`].

use crate::{Result, format_err};

pub const TREE_MAGIC: u32 = 0x59F2_D89B;
pub const REC_START_360: usize = 0x48;
pub const REC_START_PC: usize = 0x1CC;
/// root record [id][size][flags]; children follow at 0x1CC
pub const PC_ROOT_OFF: usize = 0x1C0;

/// don't resync through implausibly large noise
pub const REANCHOR_MAX_GAP: usize = 0x4000;

/// One chunk record. `flags` is the Python `Record.type` field: the [junk]
/// word on 360 records / the [flags] word on PC records.
#[derive(Clone, Debug)]
pub struct Record {
    pub flags: u32,
    pub id: u32,
    pub size: u32,
    pub payload: Vec<u8>,
    /// 360 only: the word after the payload. Records tile as [id][size]
    /// [flag word + nodes][last data word], so this word (read as the next
    /// record's header "type") is the last node's value. Empty when noise
    /// follows (damaged region).
    pub tail: Vec<u8>,
}

/// `tree[off+12+s .. off+16+s]` clamped to the buffer (Python slice), 360 only.
fn tail_after(tree: &[u8], off: usize, s: usize, big: bool) -> Vec<u8> {
    if !big {
        return Vec::new();
    }
    let a = (off + 12 + s).min(tree.len());
    tree[a..(a + 4).min(tree.len())].to_vec()
}

#[derive(Clone, Debug, Default)]
pub struct Tree {
    /// 16 bytes: 128-bit tree hash (never verified at load; recomputed on write)
    pub noise: Vec<u8>,
    pub count: u32,
    /// allocator garbage + machine GUID region (never read on load)
    pub pre_records: Vec<u8>,
    pub records: Vec<Record>,
    /// everything after the record area (360-side directory)
    pub post: Vec<u8>,
    pub used: u32,
    /// bytes of damaged noise skipped inside the record region
    pub gap: usize,
}

fn rd_u32(buf: &[u8], off: usize, big: bool) -> u32 {
    let b: [u8; 4] = buf[off..off + 4].try_into().unwrap();
    if big {
        u32::from_be_bytes(b)
    } else {
        u32::from_le_bytes(b)
    }
}

impl Tree {
    pub fn parse(tree: &[u8], big: bool) -> Result<Tree> {
        let rec_start = if big { REC_START_360 } else { REC_START_PC };
        // Python reads the count word with `unpack_from`, which raises on a
        // blob shorter than 0x14; refuse instead of slicing out of range.
        if tree.len() < 0x14 {
            return Err(format_err(format!(
                "tree blob ({:#x} B) too short for the chunk-count word",
                tree.len()
            )));
        }
        let count = rd_u32(tree, 0x10, big);
        // locate magic between 0x14 and rec_start. The scan stops at the
        // buffer end (Python's unpack_from raises there; both mean "no
        // magic" for anything this short).
        let mut magic_off = None;
        for off in (0x14..rec_start.saturating_sub(4)).step_by(4) {
            let Some(word) = tree.get(off..off + 4) else {
                break;
            };
            if rd_u32(word, 0, big) == TREE_MAGIC {
                magic_off = Some(off);
                break;
            }
        }
        let magic_off = magic_off.ok_or_else(|| format_err("tree magic 0x59F2D89B not found"))?;
        let used = rd_u32(tree, magic_off + 4, big);
        if tree.len() < rec_start || used as usize > tree.len() - rec_start {
            return Err(format_err(format!(
                "corrupt used size {used:#x} exceeds tree buffer"
            )));
        }
        let mut records: Vec<Record> = Vec::new();
        let mut off = rec_start;
        let end = rec_start + used as usize;
        let limit = end.min(tree.len());
        let mut stopped = None;
        while off + 12 <= limit {
            let (t, i, s) = if big {
                // [junk][id][size]
                (
                    rd_u32(tree, off, true),
                    rd_u32(tree, off + 4, true),
                    rd_u32(tree, off + 8, true),
                )
            } else {
                // [id][size][flags]
                (
                    rd_u32(tree, off + 8, false),
                    rd_u32(tree, off, false),
                    rd_u32(tree, off + 4, false),
                )
            };
            if off + 12 + s as usize > end || (s == 0 && i == 0 && t == 0) {
                stopped = Some(off);
                break;
            }
            // size-0 records are legal (positional hole fillers); 12-byte stride
            records.push(Record {
                flags: t,
                id: i,
                size: s,
                payload: tree[off + 12..off + 12 + s as usize].to_vec(),
                tail: tail_after(tree, off, s as usize, big),
            });
            off += 12 + s as usize;
        }
        let stopped = stopped.unwrap_or(off);
        let gap = end.saturating_sub(stopped);
        if gap != 0 {
            if let Some(last) = records.last_mut() {
                last.tail.clear(); // noise, not a value
            }
            records.extend(Self::reafter_gap(tree, stopped, end, big));
        }
        Ok(Tree {
            noise: tree[0..0x10].to_vec(),
            count,
            pre_records: tree[0x14..rec_start].to_vec(),
            records,
            post: tree[end..].to_vec(),
            used,
            gap,
        })
    }

    /// Internal-gap recovery: find a 4-aligned offset after the noise
    /// where a clean record chain tiles exactly to `end`; damage beyond
    /// [`REANCHOR_MAX_GAP`] is treated as unrecoverable tail loss.
    fn reafter_gap(tree: &[u8], stopped: usize, end: usize, big: bool) -> Vec<Record> {
        if end - stopped > REANCHOR_MAX_GAP {
            return Vec::new();
        }
        let mut cand = (stopped + 4 + 3) & !3;
        while cand + 11 < end {
            let mut off = cand;
            let mut recs: Vec<Record> = Vec::new();
            let mut ok = true;
            while off + 12 <= end {
                let (t, i, s) = if big {
                    (
                        rd_u32(tree, off, true),
                        rd_u32(tree, off + 4, true),
                        rd_u32(tree, off + 8, true),
                    )
                } else {
                    (
                        rd_u32(tree, off + 8, false),
                        rd_u32(tree, off, false),
                        rd_u32(tree, off + 4, false),
                    )
                };
                if off + 12 + s as usize > end || (s == 0 && i == 0 && t == 0) {
                    ok = false;
                    break;
                }
                recs.push(Record {
                    flags: t,
                    id: i,
                    size: s,
                    payload: tree[off + 12..off + 12 + s as usize].to_vec(),
                    tail: tail_after(tree, off, s as usize, big),
                });
                off += 12 + s as usize;
            }
            if ok && off == end {
                return recs;
            }
            cand += 4;
        }
        Vec::new()
    }

    pub fn build(&self, big: bool, tree_size: usize) -> Result<Vec<u8>> {
        let mut body: Vec<u8> = Vec::new();
        for rec in &self.records {
            let len = rec.payload.len() as u32;
            if big {
                body.extend_from_slice(&rec.flags.to_be_bytes());
                body.extend_from_slice(&rec.id.to_be_bytes());
                body.extend_from_slice(&len.to_be_bytes());
            } else {
                body.extend_from_slice(&rec.id.to_le_bytes());
                body.extend_from_slice(&len.to_le_bytes());
                body.extend_from_slice(&rec.flags.to_le_bytes());
            }
            body.extend_from_slice(&rec.payload);
        }
        let used = body.len();
        // patch magic+used inside pre_records
        let mut pre = self.pre_records.clone();
        let mut magic_off = None;
        for off in (0..pre.len().saturating_sub(4)).step_by(4) {
            if rd_u32(&pre, off, big) == TREE_MAGIC {
                magic_off = Some(off);
                break;
            }
        }
        let magic_off = magic_off.ok_or_else(|| format_err("tree magic lost in pre_records"))?;
        pre[magic_off + 4..magic_off + 8].copy_from_slice(&if big {
            (used as u32).to_be_bytes()
        } else {
            (used as u32).to_le_bytes()
        });
        let mut out = Vec::with_capacity(tree_size);
        out.extend_from_slice(&self.noise);
        out.extend_from_slice(&if big {
            self.count.to_be_bytes()
        } else {
            self.count.to_le_bytes()
        });
        out.extend_from_slice(&pre);
        out.extend_from_slice(&body);
        // the post-records directory region is variable-length: keep as much
        // as fits in the fixed tree buffer
        if out.len() > tree_size {
            return Err(format_err(format!(
                "tree overflow: records end at {:#x} > {tree_size:#x}",
                out.len()
            )));
        }
        let room = tree_size - out.len();
        let keep = room.min(self.post.len());
        out.extend_from_slice(&self.post[..keep]);
        out.resize(tree_size, 0);
        Ok(out)
    }
}

/// Reverse every 4-byte word (panics if `data.len() % 4 != 0`, like the
/// Python's `assert`).
pub fn swap_u32s(data: &[u8]) -> Vec<u8> {
    assert!(data.len().is_multiple_of(4));
    let mut out = data.to_vec();
    for word in out.as_chunks_mut::<4>().0 {
        word.reverse();
    }
    out
}
