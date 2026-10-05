//! EA's CRC-32 variant used by NFS ProStreet saves (both PC and Xbox 360).
//!
//! Algorithm (reversed from nfs.exe @ 0x8A18F6 / 360 xex sub_827C75B8):
//!   - MSB-first table-driven CRC over RAW bytes (no endianness of the data involved)
//!   - init: first 4 bytes preloaded into the register (b0<<24|b1<<16|b2<<8|b3), then NOT
//!   - step: crc = ((crc << 8) | byte) ^ table[crc >> 24]
//!   - final: NOT
//!   - lengths < 4 return 0
//!
//! The table is the standard MSB-first CRC-32 table (poly 0x04C11DB7).

/// Standard MSB-first CRC-32 table (poly 0x04C11DB7), computed at compile time.
static TABLE: [u32; 256] = build_table();

const fn build_table() -> [u32; 256] {
    let mut tbl = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut c = (i as u32) << 24;
        let mut k = 0;
        while k < 8 {
            // u32 shifts already discard the carry bit (the Python masks
            // with 0xFFFFFFFF because its ints are unbounded)
            c = if c & 0x8000_0000 != 0 {
                (c << 1) ^ 0x04C1_1DB7
            } else {
                c << 1
            };
            k += 1;
        }
        tbl[i] = c;
        i += 1;
    }
    tbl
}

/// EA-variant CRC-32 over raw bytes; 0 for inputs shorter than 4 bytes.
pub fn crc32_ea(data: &[u8]) -> u32 {
    let n = data.len();
    if n < 4 {
        return 0;
    }
    let mut crc = ((data[0] as u32) << 24)
        | ((data[1] as u32) << 16)
        | ((data[2] as u32) << 8)
        | (data[3] as u32);
    crc ^= 0xFFFF_FFFF;
    for &b in &data[4..] {
        crc = ((crc << 8) | b as u32) ^ TABLE[(crc >> 24) as usize];
    }
    crc ^ 0xFFFF_FFFF
}

/// Load a 1024-entry LE u32 table (e.g. lifted from an exe).
///
/// Returns `None` when the blob has fewer than 4096 bytes (the Python
/// `struct.unpack` raises in that case).
pub fn crc_table_from_blob(blob: &[u8]) -> Option<[u32; 1024]> {
    if blob.len() < 4096 {
        return None;
    }
    let mut out = [0u32; 1024];
    for (i, word) in out.iter_mut().enumerate() {
        *word = u32::from_le_bytes(blob[i * 4..i * 4 + 4].try_into().unwrap());
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_inputs_return_zero() {
        assert_eq!(crc32_ea(b""), 0);
        assert_eq!(crc32_ea(b"abc"), 0);
    }

    #[test]
    fn matches_python_reference() {
        // pinned against scripts/python/nfssave/crc.py (2026-10-05 run)
        assert_eq!(crc32_ea(&[0x4D, 0x43, 0x30, 0x32, 0, 0, 0, 0]), 0x299A_D3EB);
        assert_eq!(crc32_ea(&(0u8..=255).collect::<Vec<u8>>()), 0xB756_E3A8);
    }
}
