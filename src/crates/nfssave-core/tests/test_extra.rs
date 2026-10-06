//! convert_extra on a 64-byte alias extra blob whose name has no NUL.
//! Port of tests/test_extra.py (same vector): the golden alias fixtures all
//! NUL-terminate the name, so this pins the defensive branch.

use nfssave_core::convert::convert_extra;

fn vector() -> Vec<u8> {
    let mut v: Vec<u8> = (1u8..=0x14).collect(); // five BE u32s
    v.extend_from_slice(b"ANONYMOUS 1"); // 0x14..0x1F, no terminator
    v.resize(0x38, 0xAA); // pad to 0x38
    v.extend_from_slice(&[0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88]);
    v
}

#[test]
fn vector_has_no_nul() {
    let v = vector();
    assert_eq!(v.len(), 64);
    assert!(!v.contains(&0));
}

#[test]
fn no_nul_name() {
    let v = vector();
    let mut want = v.clone();
    for i in (0..0x14).step_by(4) {
        want[i..i + 4].reverse();
    }
    want[0x38..0x3C].reverse();
    want[0x3C..0x40].reverse();
    assert_eq!(convert_extra(&v).unwrap(), want);
}
