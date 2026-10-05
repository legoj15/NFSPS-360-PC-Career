//! 128-bit tree hash stored at `tree[0:0x10]` (reversed from nfs.exe 0x6D9CE0).
//!
//! chained-MD5 (4 rounds over the raw digest) + RSA-style modpow with
//! exe-embedded exponent/modulus; input span = `tree[0x10:tree_size]` (the
//! FULL fixed device buffer). The verify path (0x5AABD0) is dead code in the
//! PC build — the loader never checks this hash — but we compute it anyway so
//! converted saves are indistinguishable from native ones.

use md5::{Digest, Md5};
use num_bigint::BigUint;

const E_HEX: &str = concat!(
    "41712698348b53b247ac4b0cfe32162265c1bdeb66590ed156707de646d307b4",
    "18e67f5a51c987be4b0bc80369920669e02bdcebfcdc40dba7169e1c7b22a62e"
);

const N_HEX: &str = concat!(
    "57a11e76c0fea0c76e43ac00cf073334444da3b91b462aa4bdfe3c389b383bb5",
    "a081c6d8d0b5ee6d1a2fcdf965a14743de47cc4f7eaf309e22cb6be37dcadaaf"
);

fn md5_once(data: &[u8]) -> [u8; 16] {
    let mut h = Md5::new();
    h.update(data);
    let d = h.finalize();
    let mut out = [0u8; 16];
    out.copy_from_slice(&d);
    out
}

/// Decode a hex string into bytes (the Python `bytes.fromhex`).
fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("valid hex constant"))
        .collect()
}

/// 16-byte value the game stores at `tree[0:0x10]`.
pub fn tree_hash(tree: &[u8]) -> [u8; 16] {
    let mut cur = md5_once(&tree[(0x10).min(tree.len())..]);
    let mut parts = Vec::with_capacity(64);
    for _ in 0..4 {
        parts.extend_from_slice(&cur);
        cur = md5_once(&cur);
    }
    let m = BigUint::from_bytes_le(&parts);
    // the Python loads the exe-embedded constants little-endian
    let e = BigUint::from_bytes_le(&unhex(E_HEX));
    let n = BigUint::from_bytes_le(&unhex(N_HEX));
    let r = m.modpow(&e, &n);
    // Python: r.to_bytes(0x82, "little")[0:0x10] — the low 16 bytes, zero-padded.
    let rb = r.to_bytes_le();
    let mut out = [0u8; 16];
    let keep = rb.len().min(16);
    out[..keep].copy_from_slice(&rb[..keep]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_python_reference() {
        // Pinned against scripts/python/nfssave/treehash.py (2026-10-05 run)
        let mut tree = vec![0u8; 0x10];
        tree.extend((0u8..=255).cycle().take(256 * 8));
        let hex = |h: &[u8; 16]| h.iter().map(|b| format!("{b:02x}")).collect::<String>();
        assert_eq!(hex(&tree_hash(&tree)), "eee09373ba533c81c0e9f878ea5ec28a");
        assert_eq!(
            hex(&tree_hash(&(0u8..0x50).collect::<Vec<u8>>())),
            "73d70bbdb9a38ffe14fc9b85119502f6"
        );
    }
}
