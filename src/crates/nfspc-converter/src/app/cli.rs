//! Command-line parsing for the exe.

use std::path::PathBuf;

/// GUI flag the app passes to its own elevated relaunch: start with the raw
/// FATX drive scan enabled.
pub const SCAN_FATX_FLAG: &str = "--scan-fatx";

pub const USAGE: &str = "\
NFSPS-SaveConverter - Xbox 360 -> PC save converter for NFS ProStreet

GUI:
  NFSPS-SaveConverter.exe [--scan-fatx]

    --scan-fatx  also scan raw drives for the Xbox 360 FATX format
                 (needs administrator rights; the app relaunches itself
                 with this flag when asked to)

Headless (no window):
  NFSPS-SaveConverter.exe --convert <file-or-folder> --out <dir>

    --convert  a CON container, a raw MC02 file, or a folder that is
               searched (depth-bounded) for CAREER_*/ALIAS_* CON files
    --out      directory the converted saves are written to
               (layout: <out>/<NAME>/<NAME>)

Exit code 0 when every requested save converted; nonzero with a stderr
message otherwise.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cli {
    Gui { scan_fatx: bool },
    Convert { src: PathBuf, out: PathBuf },
    Help,
}

pub fn parse_args(args: &[String]) -> Result<Cli, String> {
    let mut src: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut scan_fatx = false;
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
            SCAN_FATX_FLAG => scan_fatx = true,
            "--help" | "-h" | "/?" => return Ok(Cli::Help),
            other => return Err(format!("unknown argument {other:?}")),
        }
        i += 1;
    }
    match (src, out) {
        (None, None) => Ok(Cli::Gui { scan_fatx }),
        (Some(_), Some(_)) if scan_fatx => Err(format!("{SCAN_FATX_FLAG} only applies to the GUI")),
        (Some(src), Some(out)) => Ok(Cli::Convert { src, out }),
        (None, Some(_)) => Err("--out given without --convert".into()),
        (Some(_), None) => Err("--convert given without --out".into()),
    }
}
