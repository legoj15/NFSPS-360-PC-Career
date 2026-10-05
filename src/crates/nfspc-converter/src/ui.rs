//! The eframe/egui front end. Thin on purpose: every decision lives in
//! [`crate::app`]; this file only renders state and forwards clicks.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use eframe::egui;
use eframe::egui::Color32;

use crate::app::batch::{BatchResult, SaveInput, SaveResult, SaveStatus, run_batch};
use crate::app::destination;
use crate::app::drivescan::{DriveScanReport, scan_physical_drives};
use crate::app::sources::{ManualSave, discover_manual};
use crate::app::worker::{Guarded, run_guarded};
use fatx::DiscoveredSave;

/// Messages from the background threads to the UI thread.
enum Msg {
    ScanDone(DriveScanReport),
    BatchDone(BatchResult),
    /// A worker thread panicked; the message carries the panic text. Both
    /// progress flags clear so the UI never latches in a spinner state.
    Failed(String),
}

/// One discovered drive save with its checkbox state.
struct DriveRow {
    save: DiscoveredSave,
    checked: bool,
}

struct ConverterApp {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    scanning: bool,
    rows: Vec<DriveRow>,
    scan_notes: Vec<String>,
    manual: Vec<ManualSave>,
    manual_error: Option<String>,
    worker_error: Option<String>,
    picked: Option<PathBuf>,
    suggested: Option<PathBuf>,
    converting: bool,
    batch: Option<BatchResult>,
}

impl ConverterApp {
    fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        let mut app = ConverterApp {
            tx,
            rx,
            scanning: false,
            rows: Vec::new(),
            scan_notes: Vec::new(),
            manual: Vec::new(),
            manual_error: None,
            worker_error: None,
            picked: None,
            suggested: destination::documents_save_folder(),
            converting: false,
            batch: None,
        };
        app.start_scan();
        app
    }

    fn start_scan(&mut self) {
        self.scanning = true;
        self.worker_error = None;
        self.rows.clear();
        self.scan_notes.clear();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let msg = match run_guarded(scan_physical_drives) {
                Guarded::Done(report) => Msg::ScanDone(report),
                Guarded::Panicked(e) => Msg::Failed(format!("drive scan crashed: {e}")),
            };
            let _ = tx.send(msg);
        });
    }

    fn start_convert(&mut self) {
        let Some(picked) = self.picked.clone() else {
            return;
        };
        let mut inputs: Vec<SaveInput> = Vec::new();
        for row in &self.rows {
            if row.checked {
                inputs.push(SaveInput::from_discovered(&row.save));
            }
        }
        for m in &self.manual {
            match SaveInput::from_path(&m.path) {
                Ok(i) => inputs.push(i),
                Err(e) => self.manual_error = Some(e.to_string()),
            }
        }
        if inputs.is_empty() {
            return;
        }
        self.converting = true;
        self.worker_error = None;
        self.batch = None;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let msg = match run_guarded(|| match destination::resolve(&picked) {
                Ok(dest) => run_batch(inputs, &dest.root),
                Err(e) => BatchResult {
                    results: vec![SaveResult {
                        label: picked.display().to_string(),
                        status: SaveStatus::Refused {
                            reason: format!("cannot prepare the export folder: {e}"),
                        },
                    }],
                    exported_to: None,
                },
            }) {
                Guarded::Done(result) => Msg::BatchDone(result),
                Guarded::Panicked(e) => Msg::Failed(format!("conversion crashed: {e}")),
            };
            let _ = tx.send(msg);
        });
    }

    fn drain_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::ScanDone(report) => {
                    self.scanning = false;
                    self.scan_notes = report.notes;
                    self.rows = report
                        .saves
                        .into_iter()
                        .map(|save| DriveRow {
                            save,
                            checked: true, // all on by default
                        })
                        .collect();
                }
                Msg::BatchDone(batch) => {
                    self.converting = false;
                    self.batch = Some(batch);
                }
                Msg::Failed(err) => {
                    // A worker died: release both flags so Refresh/Convert
                    // work again and the failure is visible.
                    self.scanning = false;
                    self.converting = false;
                    self.worker_error = Some(err);
                }
            }
        }
    }

    fn add_manual(&mut self, paths: Vec<PathBuf>) {
        for p in paths {
            match discover_manual(&p) {
                Ok(mut found) => self.manual.append(&mut found),
                Err(e) => self.manual_error = Some(e.to_string()),
            }
        }
    }

    fn selected_count(&self) -> usize {
        self.rows.iter().filter(|r| r.checked).count() + self.manual.len()
    }
}

impl eframe::App for ConverterApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain_messages();
        if self.scanning || self.converting {
            ui.ctx().request_repaint_after(Duration::from_millis(120));
        }

        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Convert Xbox 360 saves to PC");
            ui.add_space(6.0);

            // ---- drives ----
            ui.horizontal(|ui| {
                ui.strong("Saves on flash drives");
                if ui
                    .add_enabled(!self.scanning, egui::Button::new("Refresh"))
                    .clicked()
                {
                    self.start_scan();
                }
            });
            if self.scanning {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("scanning removable/physical drives…");
                });
            } else if self.rows.is_empty() {
                ui.weak("no saves found on flash drives");
            } else {
                egui::ScrollArea::vertical()
                    .id_salt("drive-saves")
                    .max_height(160.0)
                    .show(ui, |ui| {
                        for row in &mut self.rows {
                            ui.checkbox(
                                &mut row.checked,
                                format!(
                                    "{}  ({}, {} KiB)",
                                    row.save.friendly_name,
                                    row.save.source_path,
                                    row.save.bytes.len() / 1024
                                ),
                            );
                        }
                    });
            }
            for note in &self.scan_notes {
                ui.colored_label(Color32::from_rgb(180, 120, 0), format!("note: {note}"));
            }

            ui.separator();

            // ---- manual selection ----
            ui.strong("Choose saves manually");
            ui.horizontal(|ui| {
                if ui.button("Add a single file…").clicked()
                    && let Some(f) = rfd::FileDialog::new()
                        .set_title("Pick a CON container or a raw MC02 save")
                        .pick_file()
                {
                    self.add_manual(vec![f]);
                }
                if ui.button("Add a folder…").clicked()
                    && let Some(f) = rfd::FileDialog::new()
                        .set_title("Pick a folder with CAREER_*/ALIAS_* saves")
                        .pick_folder()
                {
                    self.add_manual(vec![f]);
                }
            });
            if !self.manual.is_empty() {
                egui::ScrollArea::vertical()
                    .id_salt("manual-saves")
                    .max_height(120.0)
                    .show(ui, |ui| {
                        let mut remove: Option<usize> = None;
                        for (i, m) in self.manual.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.monospace(m.path.display().to_string());
                                if ui.small_button("✕").clicked() {
                                    remove = Some(i);
                                }
                            });
                        }
                        if let Some(i) = remove {
                            self.manual.remove(i);
                        }
                    });
            }
            if let Some(err) = &self.manual_error {
                ui.colored_label(Color32::RED, err.as_str());
            }
            if let Some(err) = &self.worker_error {
                ui.colored_label(Color32::RED, err.as_str());
            }

            ui.separator();

            // ---- destination ----
            ui.strong("Export location");
            if let Some(picked) = &self.picked {
                let dest = destination::resolve_dry(picked);
                ui.label("Saves will be exported to:");
                ui.monospace(dest.root.display().to_string());
                if dest.created {
                    ui.weak("(this folder will be created)");
                }
            } else if let Some(sug) = &self.suggested {
                ui.weak(format!("suggested: {}", sug.display()));
            } else {
                ui.weak("no export location chosen yet");
            }
            let mut dialog = rfd::FileDialog::new().set_title("Select location to export saves");
            if let Some(sug) = &self.suggested
                && let Some(parent) = sug.parent().filter(|p| p.is_dir()).or(Some(sug))
            {
                dialog = dialog.set_directory(parent);
            }
            if ui.button("Select location to export saves…").clicked()
                && let Some(f) = dialog.pick_folder()
            {
                self.picked = Some(f);
            }

            ui.separator();

            // ---- convert ----
            let can_convert =
                !self.converting && self.picked.is_some() && self.selected_count() > 0;
            ui.horizontal(|ui| {
                let btn = ui.add_enabled(
                    can_convert,
                    egui::Button::new(egui::RichText::new("Convert").strong().heading()),
                );
                if btn.clicked() {
                    self.start_convert();
                }
                if self.converting {
                    ui.spinner();
                    ui.label("converting…");
                } else {
                    ui.weak(format!("{} save(s) selected", self.selected_count()));
                }
            });

            // ---- results ----
            if let Some(batch) = &self.batch {
                ui.add_space(6.0);
                for result in &batch.results {
                    show_save_result(ui, result);
                }
                if let Some(msg) = batch.success_message() {
                    ui.add_space(4.0);
                    ui.colored_label(
                        Color32::from_rgb(0, 140, 60),
                        egui::RichText::new(msg).strong(),
                    );
                } else {
                    ui.colored_label(Color32::RED, "no saves were converted");
                }
            }
        });
    }
}

fn show_save_result(ui: &mut egui::Ui, result: &SaveResult) {
    match &result.status {
        SaveStatus::Converted {
            chunks,
            warnings,
            target: _,
        } => {
            ui.colored_label(
                Color32::from_rgb(0, 140, 60),
                format!("[+] {}: {} chunks", result.label, chunks),
            );
            for w in warnings {
                ui.weak(format!("      ! {w}"));
            }
        }
        SaveStatus::Refused { reason } => {
            ui.colored_label(
                Color32::RED,
                format!("[!] {} refused: {}", result.label, reason),
            );
        }
    }
}

/// Launch the GUI. Blocks until the window closes.
pub fn run() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([720.0, 640.0])
            .with_min_inner_size([480.0, 420.0]),
        ..Default::default()
    };
    eframe::run_native(
        "NFS ProStreet Save Converter",
        options,
        Box::new(|_cc| Ok(Box::new(ConverterApp::new()))),
    )
}
