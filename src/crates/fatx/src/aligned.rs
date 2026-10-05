//! Sector-aligned buffering for raw Windows devices.
//!
//! Physical-drive reads fail with `ERROR_INVALID_PARAMETER` (os error 87)
//! unless every read is sector-aligned in both offset and size. Measured on
//! a real 360-formatted USB stick (2026-10-05): the 4-byte `XTAF` magic
//! probe at the Data-partition offset aborted the scan, while the same code
//! over files and in-memory images worked — which is why synthetic-image
//! round-trips cannot catch unaligned access by construction.
//!
//! [`SectorReader`] wraps the raw device handle so any Read + Seek pattern
//! above it becomes sector-clean: unaligned requests are served from one
//! cached sector, aligned bulk requests pass straight through.

use std::io::{self, Read, Seek, SeekFrom};

const MIN_SECTOR: usize = 512;

/// Wraps a raw device so every underlying read is `sector_size`-aligned in
/// offset and size, while exposing ordinary byte-granular Read/Seek.
pub struct SectorReader<R> {
    inner: R,
    sector_size: usize,
    len: u64,
    pos: u64,
    buf: Box<[u8]>,
    buf_len: usize,
    buf_start: u64,
}

impl<R: Read + Seek> SectorReader<R> {
    /// Wraps `inner`, a read-only device of `len` bytes whose sector size is
    /// `sector_size`. Values that are not a power of two of at least 512
    /// fall back to 512.
    pub fn new(inner: R, len: u64, sector_size: usize) -> Self {
        let sector_size = if sector_size.is_power_of_two() && sector_size >= MIN_SECTOR {
            sector_size
        } else {
            MIN_SECTOR
        };
        Self {
            inner,
            sector_size,
            len,
            pos: 0,
            buf: vec![0u8; sector_size].into_boxed_slice(),
            buf_len: 0,
            buf_start: u64::MAX,
        }
    }

    /// Loads the sector containing `self.pos` into the cache. Synchronous
    /// disk reads fill the buffer or hit end-of-device; a short read means
    /// the device ends inside this sector and is kept as a partial tail.
    fn load_current_sector(&mut self) -> io::Result<()> {
        let start = (self.pos / self.sector_size as u64) * self.sector_size as u64;
        if self.buf_len != 0 && self.buf_start == start {
            return Ok(());
        }
        self.inner.seek(SeekFrom::Start(start))?;
        let n = self.inner.read(&mut self.buf)?;
        self.buf_len = n;
        self.buf_start = start;
        Ok(())
    }
}

impl<R: Read + Seek> Read for SectorReader<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() || self.pos >= self.len {
            return Ok(0);
        }
        let mut done = 0;

        // Fast path: aligned position with a multi-sector request reads whole
        // sectors straight into the caller's buffer.
        let ss = self.sector_size;
        if self.pos.is_multiple_of(ss as u64) && out.len() >= ss {
            let want = (out.len() / ss) * ss;
            self.inner.seek(SeekFrom::Start(self.pos))?;
            let mut filled = 0;
            while filled < want {
                match self.inner.read(&mut out[filled..want])? {
                    0 => break,
                    n => filled += n,
                }
            }
            self.pos += filled as u64;
            done = filled;
        }

        // Slow path: serve the (remaining) unaligned bytes from one sector.
        while done < out.len() && self.pos < self.len {
            self.load_current_sector()?;
            let in_buf = (self.pos - self.buf_start) as usize;
            if in_buf >= self.buf_len {
                break; // device ends inside the cached sector
            }
            let n = (self.buf_len - in_buf).min(out.len() - done);
            out[done..done + n]
                .copy_from_slice(&self.buf[in_buf..in_buf + n]);
            self.pos += n as u64;
            done += n;
        }
        Ok(done)
    }
}

impl<R: Read + Seek> Seek for SectorReader<R> {
    fn seek(&mut self, target: SeekFrom) -> io::Result<u64> {
        let bad = || io::Error::new(io::ErrorKind::InvalidInput, "seek before start");
        let new = match target {
            SeekFrom::Start(n) => n,
            SeekFrom::Current(d) => self.pos.checked_add_signed(d).ok_or_else(bad)?,
            SeekFrom::End(d) => self.len.checked_add_signed(d).ok_or_else(bad)?,
        };
        self.pos = new;
        Ok(new)
    }
}
