"""CLI: convert Xbox 360 NFS ProStreet saves to PC saves.

Examples:
  python convert.py "Extracted/Career/CAREER_01"
  python convert.py "F:/Content/.../CAREER_02" --out-root "E:/somewhere/NFS ProStreet"
  python convert.py --flash F: --all            # convert every save on the drive

Writes <out-root>/<NAME>/<NAME> (default: the game's SAVE folder).
"""

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from nfssave import MC02, read_container
from nfssave.convert import (convert_payload, ConversionReport, write_pc_save,
                              back_up_existing, utc_stamp)
from nfssave.container360 import parse_container

GAME_SAVE_SUBDIR = Path("Need for Speed ProStreet") / "SAVE" / "NFS ProStreet"


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
    return sorted(root.glob("*/*/0000000[12]/*"))


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
        backup = back_up_existing(args.out_root, cont.name, stamp)
    except OSError as exc:
        raise RuntimeError(f"an existing {cont.name} could not be backed up "
                           f"({exc}); left it untouched") from exc
    if backup:
        print(f"[+] backed up existing save to {backup}")
    target = write_pc_save(pc, cont.name, args.out_root)
    check = MC02.parse(target.read_bytes()).check()
    print(f"[+] wrote {target} {'(self-check OK)' if not check else check}")


def known_folder_documents() -> Path | None:
    """Documents via the shell known-folder API (follows a redirected
    Documents folder); None off Windows or on failure."""
    try:
        import ctypes
        from ctypes import wintypes
        class GUID(ctypes.Structure):
            _fields_ = [("Data1", wintypes.DWORD), ("Data2", wintypes.WORD),
                        ("Data3", wintypes.WORD), ("Data4", ctypes.c_ubyte * 8)]
        # FOLDERID_Documents {FDD39AD0-238F-46AF-ADB4-6C85480369C7}
        docs = GUID(0xFDD39AD0, 0x238F, 0x46AF, (ctypes.c_ubyte * 8)(
            0xAD, 0xB4, 0x6C, 0x85, 0x48, 0x03, 0x69, 0xC7))
        p = ctypes.c_void_p()
        hr = ctypes.windll.shell32.SHGetKnownFolderPath(
            ctypes.byref(docs), 0, None, ctypes.byref(p))
        try:
            if hr == 0 and p.value:
                return Path(ctypes.wstring_at(p.value))
        finally:
            ctypes.windll.ole32.CoTaskMemFree(p)
    except Exception:
        pass
    return None


def documents_dir() -> Path:
    """The user's Documents folder, else <home>/Documents."""
    return known_folder_documents() or Path.home() / "Documents"


def resolve_out_root(arg: str | None) -> str:
    if arg:
        return arg
    cand = documents_dir() / GAME_SAVE_SUBDIR
    if cand.is_dir():
        return str(cand)
    raise SystemExit(
        f"game save folder not found (looked for {cand}); pass --out-root")


def main() -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(errors="replace")
    ap = argparse.ArgumentParser(description="NFS ProStreet 360 -> PC save converter")
    ap.add_argument("source", nargs="?", help="360 save container file (CON)")
    ap.add_argument("--flash", help="flash drive letter with Content/ tree")
    ap.add_argument("--all", action="store_true", help="convert every save found")
    ap.add_argument("--out-root", default=None)
    ap.add_argument("--twin", default=None,
                    help="re-saved MC02 twin used to recover damaged record "
                         "tails (auto-detected from the NFSPS360 user_data "
                         "path when present)")
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()
    args.out_root = resolve_out_root(args.out_root)
    args.backup_stamp = utc_stamp()  # one backup folder per run

    if args.flash and args.all:
        saves = [p for p in find_flash_saves(args.flash) if p.name.startswith(("CAREER_", "ALIAS_"))]
        if not saves:
            print(f"no saves found under {flash_content_dir(args.flash)}", file=sys.stderr)
            return 1
        failures = 0
        claimed = {}
        for p in saves:
            try:
                convert_one(p, args, claimed)
            except Exception as exc:
                failures += 1
                print(f"[!] FAILED {p}: {exc}", file=sys.stderr)
        return 1 if failures else 0
    if not args.source:
        ap.error("provide a source file, or --flash X --all")
    try:
        convert_one(Path(args.source), args)
    except Exception as exc:
        print(f"[!] FAILED {args.source}: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
