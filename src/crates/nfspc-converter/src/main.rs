//! Entry point: headless `--convert <file-or-folder> --out <dir>` for
//! automated cross-checks and power users, GUI otherwise.

use std::path::PathBuf;
use std::process::ExitCode;

use nfspc_converter::app::headless;
use nfspc_converter::ui;

const USAGE: &str = "\
NFSPS-SaveConverter - Xbox 360 -> PC save converter for NFS ProStreet

GUI:
  NFSPS-SaveConverter.exe

Headless (no window):
  NFSPS-SaveConverter.exe --convert <file-or-folder> --out <dir>

    --convert  a CON container, a raw MC02 file, or a folder that is
               searched (depth-bounded) for CAREER_*/ALIAS_* CON files
    --out      directory the converted saves are written to
               (layout: <out>/<NAME>/<NAME>)

Exit code 0 when every requested save converted; nonzero with a stderr
message otherwise.";

enum Cli {
    Gui,
    Convert { src: PathBuf, out: PathBuf },
}

fn parse_args(args: &[String]) -> Result<Cli, String> {
    let mut src: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--convert" => {
                i += 1;
                src = Some(PathBuf::from(
                    args.get(i)
                        .ok_or("--convert needs a file or folder argument")?,
                ));
            }
            "--out" => {
                i += 1;
                out = Some(PathBuf::from(
                    args.get(i).ok_or("--out needs a directory argument")?,
                ));
            }
            "--help" | "-h" | "/?" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
        i += 1;
    }
    match (src, out) {
        (None, None) => Ok(Cli::Gui),
        (Some(src), Some(out)) => Ok(Cli::Convert { src, out }),
        (None, Some(_)) => Err("--out given without --convert".into()),
        (Some(_), None) => Err("--convert given without --out".into()),
    }
}

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
        Cli::Gui => {
            env_logger::init();
            match ui::run() {
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
