//! Windows converter app for NFS ProStreet 360 -> PC career saves.
//!
//! Two layers, strictly separated:
//!
//! * [`app`] - the application logic (destination resolution, manual-source
//!   discovery, drive scanning, conversion orchestration). It uses only
//!   `std` + the two library crates and is unit-tested.
//! * [`ui`] - the thin eframe/egui front end over [`app`], plus the
//!   headless `--convert` entry point lives in `main.rs`.

pub mod app;
pub mod ui;
