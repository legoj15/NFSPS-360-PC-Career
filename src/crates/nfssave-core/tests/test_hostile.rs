//! Port of tests/test_hostile.py: hostile input is refused with an error,
//! never a panic or a huge allocation (the app would abort mid-batch).

use nfssave_core::MC02;
use nfssave_core::tree::Tree;

/// 0x44-byte 360 tree blob whose magic is its last word.
fn magic_at_end() -> Vec<u8> {
    let mut blob = vec![0u8; 0x44];
    blob[0x40..0x44].copy_from_slice(&0x59F2_D89Bu32.to_be_bytes());
    blob
}

/// Bare 0x1C-byte MC02 header declaring a 2 GiB tree buffer.
fn huge_tree_size() -> Vec<u8> {
    [0x4D43_3032u32, 0x1C, 0, 0x8000_0000, 0, 0, 0]
        .iter()
        .flat_map(|w| w.to_le_bytes())
        .collect()
}

#[test]
fn magic_without_used_word_is_refused() {
    let e = Tree::parse(&magic_at_end(), true)
        .err()
        .expect("must refuse");
    assert!(
        e.to_string().contains("too short for the used-size word"),
        "{e}"
    );
}

#[test]
fn huge_tree_size_is_refused() {
    let e = MC02::parse(&huge_tree_size()).err().expect("must refuse");
    assert!(
        e.to_string().contains("tree size 0x80000000 exceeds"),
        "{e}"
    );
}
