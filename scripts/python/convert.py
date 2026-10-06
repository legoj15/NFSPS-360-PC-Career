"""CLI: convert Xbox 360 NFS ProStreet saves to PC saves.

Examples:
  python convert.py CAREER_01                  # -> ./CAREER_01/CAREER_01
  python convert.py "D:/my 360 saves" --out-root "D:/Games/NFS ProStreet"
  python convert.py --usb F:                   # every save on the USB drive

Inputs are files or folders (searched for CAREER_*/ALIAS_* containers).
Output goes to --out-root (default: the current directory); when that is the
game's folder (holding SAVE/NFS ProStreet or NFS ProStreet) the saves land in
the game's save folder. Replaced saves are backed up first.
Full contract: docs/scripts-cli.md.
"""

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from nfssave import MC02, read_container
from nfssave.convert import (convert_payload, ConversionReport, write_pc_save,
                              back_up_existing, utc_stamp, BACKUP_DIR)
from nfssave.container360 import parse_container

SAVE_DIR_NAME = "NFS ProStreet"
SAVE_PREFIXES = ("career_", "alias_")
CON_MAGIC = b"CON "


def flash_content_dir(drive: str) -> Path:
    """<drive>/Content. A bare drive letter ("F" or "F:") means the drive
    root; Path("F:") / "Content" would be the drive-relative "F:Content"."""
    d = drive.strip()
    if 1 <= len(d) <= 3 and d[0].isalpha() and d[1:] in ("", ":", ":/", ":\\"):
        d = d[0] + ":/"
    return Path(d) / "Content"


def find_flash_saves(drive: str):
    root = flash_content_dir(drive)
    if not root.is_dir():
        return []
    # case-insensitive names, like the exe (fatx discovery is_save_name)
    return sorted(p for p in root.glob("*/*/0000000[12]/*")
                  if p.is_file() and p.name.lower().startswith(SAVE_PREFIXES))


def _is_container(p: Path) -> bool:
    try:
        with open(p, "rb") as f:
            return f.read(4) == CON_MAGIC
    except OSError:
        return False


def find_saves_in(folder: Path) -> list[Path]:
    """CAREER_*/ALIAS_* 360 containers anywhere under folder, skipping
    earlier backups."""
    found = []
    for p in Path(folder).rglob("*"):
        if (p.name.lower().startswith(SAVE_PREFIXES) and p.is_file()
                and BACKUP_DIR.lower() not in (x.lower() for x in p.relative_to(folder).parts)
                and _is_container(p)):
            found.append(p)
    return sorted(found)


def _child_dir(parent: Path, *names: str) -> Path | None:
    """parent/names... matched case-insensitively, if every level exists."""
    cur = parent
    for name in names:
        try:
            cur = next(c for c in cur.iterdir()
                       if c.is_dir() and c.name.lower() == name.lower())
        except (StopIteration, OSError):
            return None
    return cur


def resolve_save_folder(root) -> tuple[Path, Path, bool]:
    """(save folder, backup base, is the game's save folder) for an output
    root: the game folder, its SAVE folder, the save folder itself, or any
    other folder (used as is, backups kept inside it)."""
    root = Path(root)
    found = _child_dir(root, "SAVE", SAVE_DIR_NAME) or _child_dir(root, SAVE_DIR_NAME)
    if found is None and root.absolute().name.lower() == SAVE_DIR_NAME.lower():
        found = root
    if found is not None:
        return found, found.absolute().parent, True
    return root, root, False


def load_twin(path: str) -> bytes:
    """Accept either a raw MC02 re-save or one still inside its CON wrapper."""
    data = Path(path).read_bytes()
    if data[:4] == b"CON ":
        return parse_container(data, path).payload
    return data


def convert_one(src: Path, args, claimed: dict | None = None) -> None:
    """`claimed` (export name, casefolded -> source) refuses a second save
    with the same name in one batch instead of replacing the first."""
    cont = read_container(src)
    if claimed is not None:
        owner = claimed.setdefault(cont.name.casefold(), src)
        if owner != src:
            raise ValueError(f"another selected save ({owner}) is also named "
                             f"{cont.name}; skipped")
    mc02 = MC02.parse(cont.payload)
    bad = mc02.check()
    if "extra CRC mismatch" in bad:
        raise ValueError(
            f"{src.name}: extra-blob CRC mismatch - the source file is corrupted; "
            "refusing to convert")
    for prob in bad:
        print(f"! {src.name}: {prob} (CRCs are recomputed on write)")
    report = ConversionReport(source=str(src))
    twin = None
    # a re-save twin is only a last resort for a genuinely damaged file;
    # the old "damaged console tail" was a container-reader bug (STFS hash
    # blocks), fixed in container360
    twin_path = getattr(args, "twin", None)
    if twin_path and Path(twin_path).is_file():
        twin = load_twin(twin_path)
        print(f"[+] using re-save twin for tail recovery: {twin_path}")
    pc = convert_payload(mc02, report, twin_payload=twin)
    print(f"[+] {src.name} ({report.kind}): {report.records} chunks")
    for name in report.chunk_list:
        print(f"      - {name}")
    for w in report.warnings:
        print(f"      ! {w}")
    if args.dry_run:
        print("[.] dry run - not writing")
        return
    stamp = getattr(args, "backup_stamp", None) or utc_stamp()
    try:
        backup = back_up_existing(args.out_root, cont.name, stamp,
                                  getattr(args, "backup_base", None))
    except OSError as exc:
        raise RuntimeError(f"an existing {cont.name} could not be backed up "
                           f"({exc}); left it untouched") from exc
    if backup:
        print(f"[+] backed up existing save to {backup}")
    target = write_pc_save(pc, cont.name, args.out_root)
    check = MC02.parse(target.read_bytes()).check()
    print(f"[+] wrote {target} {'(self-check OK)' if not check else check}")


def collect_sources(inputs, usb) -> tuple[list[Path], list[str]]:
    """Files to convert, plus an error line per input that yielded none."""
    saves, errors = [], []
    for raw in inputs:
        p = Path(raw)
        if p.is_dir():
            found = find_saves_in(p)
            if not found:
                errors.append(f"no saves found in folder {p}")
            saves += found
        else:
            saves.append(p)  # a missing file fails in convert_one
    if usb:
        found = find_flash_saves(usb)
        if not found:
            errors.append(f"no saves found under {flash_content_dir(usb)}")
        saves += found
    return saves, errors


def main() -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(errors="replace")
    ap = argparse.ArgumentParser(
        description="NFS ProStreet 360 -> PC save converter (docs/scripts-cli.md)")
    ap.add_argument("sources", nargs="*", metavar="file-or-folder",
                    help="360 save files, or folders to search for them")
    ap.add_argument("--usb", "--flash", dest="usb", metavar="DRIVE",
                    help="convert every save on a USB drive copied from the "
                         "Xbox 360 (drive letter, or the folder holding Content)")
    ap.add_argument("--all", action="store_true", help=argparse.SUPPRESS)
    ap.add_argument("--out-root", default=None, metavar="FOLDER",
                    help="where to write (default: the current folder); the "
                         "game's folder puts saves straight into its save folder")
    ap.add_argument("--twin", default=None,
                    help="re-saved MC02 twin used to recover damaged record tails")
    ap.add_argument("--dry-run", action="store_true",
                    help="check the saves without writing anything")
    args = ap.parse_args()
    if not args.sources and not args.usb:
        ap.error("give one or more save files or folders, or --usb DRIVE")

    saves, errors = collect_sources(args.sources, args.usb)
    root = Path(args.out_root) if args.out_root else Path.cwd()
    save_dir, backup_base, is_game = resolve_save_folder(root)
    args.out_root, args.backup_base = str(save_dir), backup_base
    args.backup_stamp = utc_stamp()  # one backup folder per run
    if is_game:
        print(f"[+] game save folder: {save_dir}")
    else:
        print(f"[+] output folder: {save_dir.absolute()}")
        print(f"    (copy the converted folders into the game's "
              f"SAVE/{SAVE_DIR_NAME} folder, or rerun with --out-root <game folder>)")

    failures = len(errors)
    for e in errors:
        print(f"[!] {e}", file=sys.stderr)
    claimed = {}
    for p in saves:
        try:
            convert_one(p, args, claimed)
        except Exception as exc:
            failures += 1
            print(f"[!] FAILED {p}: {exc}", file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
