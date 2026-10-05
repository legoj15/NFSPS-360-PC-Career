//! Partition-table / drive-image detection tests (feature test-util).

#![cfg(feature = "test-util")]

use std::io::Cursor;

use fatx::partition::{
    DriveLayout, XboxDriveImage, RETAIL_USB_DATA_OFFSET, USB_SIGNATURE,
};
use fatx::test_util::{content_save_path, FatxImageBuilder};

fn profile() -> String {
    "E0001A2B3C4D5E6F".to_string()
}

fn path(file: &str) -> String {
    content_save_path(&profile(), "45410822", "00000001", file)
}

#[test]
fn retail_usb_image_probes_at_fixed_offset() {
    let usb = FatxImageBuilder::new()
        .file(path("CAREER_01_360"), b"payload".to_vec())
        .build_usb_image();

    // The synthetic image really is a raw drive image: signature present,
    // FATX Data partition at the retail USB offset.
    assert_eq!(&usb.image[0x1FF..0x1FF + USB_SIGNATURE.len()], USB_SIGNATURE);
    assert_eq!(&usb.image[RETAIL_USB_DATA_OFFSET as usize..][..4], b"XTAF");
    assert!(usb.data_offset == RETAIL_USB_DATA_OFFSET);

    let mut cur = Cursor::new(usb.image.clone());
    let drive = XboxDriveImage::probe(&mut cur, usb.image.len() as u64).unwrap();
    assert!(drive.signature_found, "signature at 0x1FF is detected");
    assert_eq!(drive.layout, DriveLayout::RetailUsb);
    assert_eq!(drive.data_partition.offset, RETAIL_USB_DATA_OFFSET);
    assert_eq!(drive.data_partition.length, usb.data_length);

    // The volume opens straight from the probed region.
    let mut vol =
        fatx::FatxVolume::open(&mut cur, drive.data_partition.offset, drive.data_partition.length)
            .unwrap();
    assert_eq!(
        vol.read_file(&format!("/Content/{}/45410822/00000001/CAREER_01_360", profile()))
            .unwrap(),
        b"payload".to_vec()
    );
}

#[test]
fn usb_image_without_signature_still_detected() {
    let usb = FatxImageBuilder::new()
        .no_signature()
        .file(path("X"), b"y".to_vec())
        .build_usb_image();
    assert!(usb.image[..0x400].iter().all(|&b| b == 0), "sector 0 clean");

    let mut cur = Cursor::new(usb.image.clone());
    let drive = XboxDriveImage::probe(&mut cur, usb.image.len() as u64).unwrap();
    assert!(!drive.signature_found, "signature is optional, not required");
    assert_eq!(drive.layout, DriveLayout::RetailUsb);
    assert_eq!(drive.data_partition.offset, RETAIL_USB_DATA_OFFSET);
}

#[test]
fn devkit_table_image_probes_with_entries() {
    let data_lba: u32 = 0x800; // 1 MiB in
    let dk = FatxImageBuilder::new()
        .file(path("CAREER_01_360"), b"devkit payload".to_vec())
        .build_devkit_image(data_lba);

    let mut cur = Cursor::new(dk.image.clone());
    let drive = XboxDriveImage::probe(&mut cur, dk.image.len() as u64).unwrap();
    let DriveLayout::DevkitTable(entries) = &drive.layout else {
        panic!("expected devkit table, got {:?}", drive.layout);
    };
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].name, "Content");
    assert_eq!(entries[0].start_lba, data_lba);
    assert_eq!(entries[0].offset(), data_lba as u64 * 0x200);
    assert_eq!(entries[0].length(), dk.data_length);
    assert_eq!(entries[1].name, "Dashboard");
    assert_eq!(drive.data_partition.offset, dk.data_offset);
    assert_eq!(drive.data_partition.length, dk.data_length);
    assert!(!drive.signature_found, "devkit images carry no USB signature");

    let mut vol =
        fatx::FatxVolume::open(&mut cur, drive.data_partition.offset, drive.data_partition.length)
            .unwrap();
    assert_eq!(
        vol.read_file(&format!("/Content/{}/45410822/00000001/CAREER_01_360", profile()))
            .unwrap(),
        b"devkit payload".to_vec()
    );
}

#[test]
fn non_xbox_media_is_rejected() {
    let mut cur = Cursor::new(vec![0u8; 0x1000_0000]);
    let err = XboxDriveImage::probe(&mut cur, 0x1000_0000).unwrap_err();
    assert!(matches!(err, fatx::Error::NotXboxImage(_)), "{err:?}");

    // An all-zero image the size of the signature area alone.
    let mut cur = Cursor::new(vec![0u8; 0x400]);
    assert!(matches!(
        XboxDriveImage::probe(&mut cur, 0x400).unwrap_err(),
        fatx::Error::NotXboxImage(_)
    ));
}

#[test]
fn truncated_media_is_rejected_not_panicked() {
    let mut cur = Cursor::new(vec![0u8; 0x10]);
    let err = XboxDriveImage::probe(&mut cur, 0x10).unwrap_err();
    assert!(matches!(err, fatx::Error::NotXboxImage(_)), "{err:?}");
}
