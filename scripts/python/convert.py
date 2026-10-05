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
from nfssave.convert import (PC_SAVE_ROOT, CHUNK_NAMES, convert_payload,
                              ConversionReport, write_pc_save)
from nfssave.container360 import parse_container


def find_flash_saves(drive: str):
    root = Path(drive) / "Content"
    if not root.is_dir():
        return []
    return sorted(root.glob("*/*/0000000[12]/*"))


def load_twin(path: str) -> bytes:
    """Accept either a raw MC02 re-save or one still inside its CON wrapper."""
    data = Path(path).read_bytes()
    if data[:4] == b"CON ":
        return parse_container(data, path).payload
    return data


def convert_one(src: Path, args) -> None:
    cont = read_container(src)
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
    target = write_pc_save(pc, cont.name, args.out_root)
    check = MC02.parse(target.read_bytes()).check()
    print(f"[+] wrote {target} {'(self-check OK)' if not check else check}")


def resolve_out_root(arg: str | None) -> str:
    if arg:
        return arg
    if Path(PC_SAVE_ROOT).is_dir():
        return PC_SAVE_ROOT
    # derive the Documents folder via the shell known-folder API (handles
    # redirected Documents drives); fall back to the profile path
    try:
        import ctypes
        from ctypes import wintypes
        class GUID(ctypes.Structure):
            _fields_ = [("Data1", wintypes.DWORD), ("Data2", wintypes.WORD),
                        ("Data3", wintypes.WORD), ("Data4", ctypes.c_ubyte * 8)]
        docs = GUID(0xFDD39AD0, 0x238F, 0x46AF, (ctypes.c_ubyte * 8)(
            0xAD, 0xB9, 0x47, 0xDC, 0x85, 0x28, 0xE0, 0xD8))  # FOLDERID_Documents
        p = ctypes.c_wchar_p()
        if ctypes.windll.shell32.SHGetKnownFolderPath(
                ctypes.byref(docs), 0, None, ctypes.byref(p)) == 0 and p.value:
            cand = Path(p.value) / "Need for Speed ProStreet" / "SAVE" / "NFS ProStreet"
            if cand.is_dir():
                return str(cand)
    except Exception:
        pass
    raise SystemExit(
        f"game save folder not found (looked for {PC_SAVE_ROOT}); pass --out-root")


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

    if args.flash and args.all:
        saves = [p for p in find_flash_saves(args.flash) if p.name.startswith(("CAREER_", "ALIAS_"))]
        if not saves:
            print(f"no saves found under {args.flash}/Content", file=sys.stderr)
            return 1
        failures = 0
        for p in saves:
            try:
                convert_one(p, args)
            except Exception as exc:
                failures += 1
                print(f"[!] FAILED {p}: {exc}", file=sys.stderr)
        return 1 if failures else 0
    if not args.source:
        ap.error("provide a source file, or --flash X --all")
    convert_one(Path(args.source), args)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
