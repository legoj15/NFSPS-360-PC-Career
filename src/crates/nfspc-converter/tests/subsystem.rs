//! The exe's PE subsystem: release builds are windowed (no console flashes
//! up behind the GUI); debug builds keep the console for env_logger output.
//! Run `cargo test --release` to exercise the release half.

const IMAGE_SUBSYSTEM_WINDOWS_GUI: u16 = 2;
const IMAGE_SUBSYSTEM_WINDOWS_CUI: u16 = 3;

fn pe_subsystem(bytes: &[u8]) -> u16 {
    let u32_at = |o: usize| u32::from_le_bytes(bytes[o..o + 4].try_into().unwrap());
    let pe = u32_at(0x3C) as usize;
    assert_eq!(&bytes[pe..pe + 4], b"PE\0\0", "not a PE image");
    // Optional header follows the 4-byte signature and the 20-byte COFF
    // header; Subsystem sits at offset 68 in both PE32 and PE32+.
    let off = pe + 4 + 20 + 68;
    u16::from_le_bytes(bytes[off..off + 2].try_into().unwrap())
}

#[test]
fn exe_subsystem_matches_build_profile() {
    let exe = std::fs::read(env!("CARGO_BIN_EXE_NFSPS-SaveConverter")).unwrap();
    let expected = if cfg!(debug_assertions) {
        IMAGE_SUBSYSTEM_WINDOWS_CUI
    } else {
        IMAGE_SUBSYSTEM_WINDOWS_GUI
    };
    assert_eq!(pe_subsystem(&exe), expected);
}
