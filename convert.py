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


def find_flash_saves(drive: str):
    root = Path(drive) / "Content"
    if not root.is_dir():
        return []
    return sorted(root.glob("*/*/0000000[12]/*"))


def convert_one(src: Path, args) -> None:
    cont = read_container(src)
    mc02 = MC02.parse(cont.payload)
    report = ConversionReport(source=str(src))
    twin = None
    twin_path = getattr(args, "twin", None)
    if twin_path is None and cont.name.startswith("CAREER"):
        auto = Path(r"E:/GitHub/NFSPS360/user_data/B13EBABEBABEBABE/45410822/00000001") / cont.name / cont.name
        if auto.is_file():
            twin_path = auto
    if twin_path and Path(twin_path).is_file():
        twin = Path(twin_path).read_bytes()
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


def main() -> int:
    ap = argparse.ArgumentParser(description="NFS ProStreet 360 -> PC save converter")
    ap.add_argument("source", nargs="?", help="360 save container file (CON)")
    ap.add_argument("--flash", help="flash drive letter with Content/ tree")
    ap.add_argument("--all", action="store_true", help="convert every save found")
    ap.add_argument("--out-root", default=PC_SAVE_ROOT)
    ap.add_argument("--twin", default=None,
                    help="re-saved MC02 twin used to recover damaged record "
                         "tails (auto-detected from the NFSPS360 user_data "
                         "path when present)")
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args()

    if args.flash and args.all:
        saves = [p for p in find_flash_saves(args.flash) if p.name.startswith(("CAREER_", "ALIAS_"))]
        if not saves:
            print(f"no saves found under {args.flash}/Content", file=sys.stderr)
            return 1
        for p in saves:
            convert_one(p, args)
        return 0
    if not args.source:
        ap.error("provide a source file, or --flash X --all")
    convert_one(Path(args.source), args)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
