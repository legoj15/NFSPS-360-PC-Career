//! Raw physical drives reject unaligned reads with os error 87
//! (ERROR_INVALID_PARAMETER) — measured on a real 360-formatted USB stick
//! (SPEC.md §8). These tests pin the [`SectorReader`] contract and prove the
//! whole probe → FATX → discovery stack is sector-clean by running it over a
//! device mock that reproduces exactly that rejection.

use std::io::{self, Read, Seek, SeekFrom};

use fatx::SectorReader;

/// Read + Seek mock mimicking a physical device: any read whose position or
/// buffer size is not a multiple of 512 fails with os error 87.
struct PickyDevice {
    data: Vec<u8>,
    pos: u64,
}

impl PickyDevice {
    fn new(data: Vec<u8>) -> Self {
        Self { data, pos: 0 }
    }
}

impl Read for PickyDevice {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if !self.pos.is_multiple_of(512) || !buf.len().is_multiple_of(512) {
            return Err(io::Error::from_raw_os_error(87));
        }
        let start = self.pos as usize;
        let n = buf.len().min(self.data.len().saturating_sub(start));
        buf[..n].copy_from_slice(&self.data[start..start + n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for PickyDevice {
    fn seek(&mut self, target: SeekFrom) -> io::Result<u64> {
        self.pos = match target {
            SeekFrom::Start(n) => n,
            SeekFrom::Current(d) => (self.pos as i64 + d).max(0) as u64,
            SeekFrom::End(d) => (self.data.len() as i64 + d).max(0) as u64,
        };
        Ok(self.pos)
    }
}

fn sample_image() -> Vec<u8> {
    // Distinct bytes so any cross-wired slice is obvious.
    (0..4096usize).map(|i| (i % 251) as u8).collect()
}

fn read_via_reader(data: &[u8], offset: u64, size: usize) -> Vec<u8> {
    let mut reader = SectorReader::new(PickyDevice::new(data.to_vec()), data.len() as u64, 512);
    reader.seek(SeekFrom::Start(offset)).expect("seek");
    let mut out = vec![0u8; size];
    reader.read_exact(&mut out).expect("read_exact");
    out
}

#[test]
fn unaligned_reads_match_source_bytes() {
    let data = sample_image();
    for offset in [0u64, 1, 5, 500, 511, 512, 513, 4090] {
        let got = read_via_reader(&data, offset, 1);
        assert_eq!(got[0], data[offset as usize], "single byte at {offset}");
    }
}

#[test]
fn cross_boundary_reads_match_source_bytes() {
    let data = sample_image();
    for (offset, size) in [(500usize, 100usize), (100, 1500), (0, 4096), (12, 13)] {
        let got = read_via_reader(&data, offset as u64, size);
        assert_eq!(&got[..], &data[offset..offset + size], "{size} at {offset}");
    }
}

#[test]
fn sequential_reads_stream_the_image() {
    let data = sample_image();
    let mut reader =
        SectorReader::new(PickyDevice::new(data.clone()), data.len() as u64, 512);
    let mut streamed = Vec::new();
    loop {
        let mut chunk = [0u8; 100]; // unaligned chunk sizes on purpose
        let n = reader.read(&mut chunk).expect("read");
        if n == 0 {
            break;
        }
        streamed.extend_from_slice(&chunk[..n]);
    }
    assert_eq!(streamed, data);
}

#[test]
fn seek_end_and_relative_positions_work() {
    let data = sample_image();
    let mut reader =
        SectorReader::new(PickyDevice::new(data.clone()), data.len() as u64, 512);
    let end = reader.seek(SeekFrom::End(0)).expect("seek end");
    assert_eq!(end, data.len() as u64);
    reader.seek(SeekFrom::End(-3)).expect("seek end -3");
    let mut tail = [0u8; 3];
    reader.read_exact(&mut tail).expect("tail read");
    assert_eq!(&tail, &data[data.len() - 3..]);
    let cur = reader.seek(SeekFrom::Current(-10)).expect("relative");
    assert_eq!(cur, (data.len() - 10) as u64);
    assert_eq!(reader.seek(SeekFrom::Start(0)).expect("start"), 0);
}

#[test]
fn read_at_or_past_end_returns_zero() {
    let data = sample_image();
    let mut reader =
        SectorReader::new(PickyDevice::new(data.clone()), data.len() as u64, 512);
    reader.seek(SeekFrom::End(0)).expect("seek end");
    let mut out = [0u8; 512];
    assert_eq!(reader.read(&mut out).expect("read at end"), 0);
    reader.seek(SeekFrom::Start(data.len() as u64 + 4096)).expect("past end");
    assert_eq!(reader.read(&mut out).expect("read past end"), 0);
}

// ---- full stack over the picky device (needs the synthetic builder) ----

#[cfg(feature = "test-util")]
mod full_stack {
    use super::*;

    #[test]
    fn raw_unaligned_probe_reproduces_os_error_87() {
        // Without SectorReader, the very first XTAF magic probe is a 4-byte
        // unaligned-size read: the real-stick failure mode, reproduced.
        let image = scenario_image();
        let mut raw = PickyDevice::new(image.clone());
        let err = fatx::XboxDriveImage::probe(&mut raw, image.len() as u64)
            .expect_err("unaligned probe must fail");
        assert!(
            err.to_string().contains("87"),
            "expected os error 87, got: {err}"
        );
    }

    #[test]
    fn sector_reader_full_stack_discovers_saves() {
        let (image, expected_bytes) = scenario_image_with_save();
        let mut reader =
            SectorReader::new(PickyDevice::new(image.clone()), image.len() as u64, 512);
        let drive = fatx::XboxDriveImage::probe(&mut reader, image.len() as u64)
            .expect("probe through aligned reader");
        let mut vol = fatx::FatxVolume::open(
            &mut reader,
            drive.data_partition.offset,
            drive.data_partition.length,
        )
        .expect("mount through aligned reader");
        let saves = fatx::discover_prostreet_saves(&mut vol).expect("discovery");
        assert_eq!(saves.len(), 1, "exactly one embedded save expected");
        assert_eq!(saves[0].bytes, expected_bytes, "extracted bytes must match");
    }

    fn scenario_image() -> Vec<u8> {
        scenario_image_with_save().0
    }

    fn scenario_image_with_save() -> (Vec<u8>, Vec<u8>) {
        let save = fatx::test_util::oracle_career_latest();
        let image = fatx::test_util::FatxImageBuilder::new()
            .file(
                fatx::test_util::content_save_path(
                    "E000ALIGNEDTEST",
                    "45410822",
                    "00000001",
                    "CAREER_01_360",
                ),
                save.clone(),
            )
            .build_usb_image();
        (image.image, save)
    }
}
