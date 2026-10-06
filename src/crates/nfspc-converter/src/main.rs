//! Entry point: headless `--convert <file-or-folder> --out <dir>` for
//! automated cross-checks and power users, GUI otherwise.
//!
//! Release builds are windowed so no console appears behind the GUI; the
//! CLI paths borrow the parent terminal's console instead (app::console).
//! Debug builds keep the console subsystem for env_logger output.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::process::ExitCode;

use nfspc_converter::app::cli::{Cli, USAGE, parse_args};
use nfspc_converter::app::console;
use nfspc_converter::app::headless;
use nfspc_converter::ui;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cli = match parse_args(&args) {
        Ok(cli) => cli,
        Err(e) => {
            console::attach_parent_console();
            eprintln!("error: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match cli {
        Cli::Help => {
            console::attach_parent_console();
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Cli::Gui { scan_fatx } => {
            env_logger::init();
            match ui::run(scan_fatx) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    let msg = format!("The window could not be opened: {e}");
                    eprintln!("error: {msg}");
                    console::error_box(&msg);
                    ExitCode::FAILURE
                }
            }
        }
        Cli::Convert { src, out } => {
            console::attach_parent_console();
            headless::run(&src, &out)
        }
    }
}
