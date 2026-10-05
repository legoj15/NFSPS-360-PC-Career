//! Locks the CON/STFS header offsets against the tracked oracle files.
//! These paths are tracked in git; a missing file is a broken checkout.

use fatx::stfs::{
    CON_MAGIC, ConHeader, DISPLAY_NAME_OFFSET, HEADER_SIZE_OFFSET, TITLE_ID_NFS_PROSTREET,
    TITLE_ID_OFFSET,
};

fn oracle(rel: &str) -> Vec<u8> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../docs/re")
        .join(rel);
    std::fs::read(&p).unwrap_or_else(|e| panic!("oracle {p:?} must be readable: {e}"))
}

#[test]
fn both_oracles_carry_prostreet_title_id_at_0x360() {
    for rel in ["c1_latest/CAREER_01_360", "pair/CAREER_02_360_fresh"] {
        let bytes = oracle(rel);
        assert_eq!(&bytes[..4], &*CON_MAGIC, "{rel}: CON magic");
        assert_eq!(
            &bytes[TITLE_ID_OFFSET..TITLE_ID_OFFSET + 4],
            &TITLE_ID_NFS_PROSTREET,
            "{rel}: title id bytes at 0x360"
        );
        assert_eq!(
            u32::from_be_bytes(
                bytes[HEADER_SIZE_OFFSET..HEADER_SIZE_OFFSET + 4]
                    .try_into()
                    .unwrap()
            ),
            0x971A,
            "{rel}: header size BE u32 at 0x340"
        );
    }
}

#[test]
fn parses_display_name_from_both_oracles() {
    for rel in ["c1_latest/CAREER_01_360", "pair/CAREER_02_360_fresh"] {
        let bytes = oracle(rel);
        let header = ConHeader::parse(&bytes).unwrap();
        assert_eq!(header.title_id, TITLE_ID_NFS_PROSTREET, "{rel}");
        assert_eq!(header.display_name, "NFS ProStreet", "{rel}");
        assert_eq!(header.header_size, 0x971A, "{rel}");
        assert!(header.title_id_matches(&[TITLE_ID_NFS_PROSTREET]), "{rel}");
        assert!(
            !header.title_id_matches(&[[0xDE, 0xAD, 0xBE, 0xEF]]),
            "{rel}"
        );
    }
}

#[test]
fn rejects_non_con_input() {
    let bytes = oracle("c1_latest/CAREER_01_360");
    assert!(ConHeader::parse(&bytes[4..]).is_err(), "wrong magic");
    assert!(
        ConHeader::parse(&bytes[..0x1710]).is_err(),
        "truncated header"
    );
    assert!(ConHeader::parse(&[0u8; 0x2000]).is_err(), "zeros");
}

#[test]
fn display_name_field_is_utf16be_nfs_prostreet_in_raw_bytes() {
    // Independent of the parser: the region at 0x1691 must decode as
    // UTF-16BE "NFS ProStreet" followed by NULs (verified by hexdump in
    // SPEC.md §7; this pins the byte layout, not just the pretty output).
    let bytes = oracle("c1_latest/CAREER_01_360");
    let field = &bytes[DISPLAY_NAME_OFFSET..DISPLAY_NAME_OFFSET + 0x1A]; // 13 UTF-16BE chars
    let expected: Vec<u8> = "NFS ProStreet"
        .chars()
        .flat_map(|c| [(c as u16 >> 8) as u8, (c as u16 & 0xFF) as u8])
        .collect();
    assert_eq!(field, &expected[..]);
    assert!(
        bytes[DISPLAY_NAME_OFFSET + 0x1A..DISPLAY_NAME_OFFSET + 0x80]
            .iter()
            .all(|&b| b == 0),
        "rest of the name field is NUL padding"
    );
}
