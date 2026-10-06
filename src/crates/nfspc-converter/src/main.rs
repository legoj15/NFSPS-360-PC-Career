//! Entry point: headless `--convert <file-or-folder> --out <dir>` for
//! automated cross-checks and power users, GUI otherwise.

use std::process::ExitCode;

use nfspc_converter::app::cli::{Cli, USAGE, parse_args};
use nfspc_converter::app::headless;
use nfspc_converter::ui;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cli = match parse_args(&args) {
        Ok(cli) => cli,
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match cli {
        Cli::Help => {
            print!("{USAGE}");
            ExitCode::SUCCESS
        }
        Cli::Gui { scan_fatx } => {
            env_logger::init();
            match ui::run(scan_fatx) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("error: the window could not be opened: {e}");
                    ExitCode::FAILURE
                }
            }
        }
        Cli::Convert { src, out } => headless::run(&src, &out),
    }
}
