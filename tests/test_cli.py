"""CLI behaviour of scripts/python/convert.py: default output folder, flash
drive paths, and backups of replaced saves (same convention as the Windows
app, src/crates/nfspc-converter/src/app/batch.rs back_up_existing)."""

import inspect
import re
import subprocess
import sys
from argparse import Namespace
from datetime import datetime, timezone
from pathlib import Path

import pytest

import convert as cli
import nfssave.convert as lib

ROOT = Path(__file__).parent.parent
PAIR_360 = ROOT / "docs/re/pair/CAREER_02_360_fresh"
GAME_SUBDIR = Path("Need for Speed ProStreet") / "SAVE" / "NFS ProStreet"


# --- 1. no author-specific default path ------------------------------------

def test_no_hardcoded_author_path():
    assert not hasattr(lib, "PC_SAVE_ROOT")
    for mod in (lib, cli):
        assert "legoj" not in Path(mod.__file__).read_text(encoding="utf-8")


def test_write_pc_save_requires_explicit_root():
    param = inspect.signature(lib.write_pc_save).parameters["save_root"]
    assert param.default is inspect.Parameter.empty


@pytest.mark.skipif(sys.platform != "win32", reason="shell known folders are Windows-only")
def test_known_folder_documents_matches_shell():
    # the fallback hid a wrong FOLDERID_Documents GUID: redirected Documents
    # folders (e.g. moved to another drive) were never found
    shell = subprocess.run(
        ["powershell", "-NoProfile", "-Command",
         "[Environment]::GetFolderPath('MyDocuments')"],
        capture_output=True, text=True, check=True).stdout.strip()
    assert cli.known_folder_documents() == Path(shell)


def test_resolve_out_root_explicit_wins(tmp_path, monkeypatch):
    monkeypatch.setattr(cli, "documents_dir", lambda: tmp_path)
    assert cli.resolve_out_root("X:/elsewhere") == "X:/elsewhere"


def test_resolve_out_root_uses_documents(tmp_path, monkeypatch):
    (tmp_path / GAME_SUBDIR).mkdir(parents=True)
    monkeypatch.setattr(cli, "documents_dir", lambda: tmp_path)
    assert Path(cli.resolve_out_root(None)) == tmp_path / GAME_SUBDIR


def test_resolve_out_root_missing_game_folder(tmp_path, monkeypatch):
    monkeypatch.setattr(cli, "documents_dir", lambda: tmp_path)
    with pytest.raises(SystemExit, match="--out-root"):
        cli.resolve_out_root(None)


# --- 2. bare drive letter means the drive root ------------------------------

@pytest.mark.parametrize("drive", ["F:", "F", "F:/", "F:\\"])
def test_flash_content_dir_is_drive_root(drive):
    got = cli.flash_content_dir(drive)
    assert got == Path("F:/") / "Content"
    assert str(got) != "F:Content"


def test_flash_content_dir_keeps_other_paths(tmp_path):
    assert cli.flash_content_dir(str(tmp_path)) == tmp_path / "Content"


# --- 3. replaced saves are backed up ----------------------------------------

def _seed(out_root: Path, name: str, data: bytes) -> Path:
    p = out_root / name / name
    p.parent.mkdir(parents=True)
    p.write_bytes(data)
    return p


def test_backup_nothing_to_keep(tmp_path):
    assert lib.back_up_existing(tmp_path / "out", "CAREER_01", "2026-10-06_12-00-00") is None
    assert not (tmp_path / lib.BACKUP_DIR).exists()


def test_backup_copies_to_sibling_folder(tmp_path):
    out = tmp_path / "out"
    _seed(out, "CAREER_01", b"old")
    dest = lib.back_up_existing(out, "CAREER_01", "2026-10-06_12-00-00")
    assert dest == tmp_path / "SaveConverter backups" / "2026-10-06_12-00-00" / "CAREER_01" / "CAREER_01"
    assert dest.read_bytes() == b"old"
    assert (out / "CAREER_01" / "CAREER_01").read_bytes() == b"old"


def test_backup_never_overwrites_earlier_backup(tmp_path):
    out = tmp_path / "out"
    src = _seed(out, "CAREER_01", b"v1")
    stamp = "2026-10-06_12-00-00"
    first = lib.back_up_existing(out, "CAREER_01", stamp)
    src.write_bytes(b"v2")
    second = lib.back_up_existing(out, "CAREER_01", stamp)
    src.write_bytes(b"v3")
    third = lib.back_up_existing(out, "CAREER_01", stamp)
    assert first.parent.parent.name == stamp
    assert second.parent.parent.name == f"{stamp}-2"
    assert third.parent.parent.name == f"{stamp}-3"
    assert [p.read_bytes() for p in (first, second, third)] == [b"v1", b"v2", b"v3"]


def test_utc_stamp_format():
    t = datetime(2026, 1, 2, 3, 4, 5, tzinfo=timezone.utc)
    assert lib.utc_stamp(t) == "2026-01-02_03-04-05"
    assert re.fullmatch(r"\d{4}-\d\d-\d\d_\d\d-\d\d-\d\d", lib.utc_stamp())


@pytest.mark.skipif(not PAIR_360.is_file(), reason="pair fixture missing")
def test_cli_convert_backs_up_replaced_save(tmp_path, capsys):
    out = tmp_path / "SAVE" / "NFS ProStreet"
    from nfssave import read_container
    name = read_container(PAIR_360).name
    old = _seed(out, name, b"previous career")
    args = Namespace(out_root=str(out), dry_run=False, twin=None)
    cli.convert_one(PAIR_360, args)
    backups = list((tmp_path / "SAVE" / lib.BACKUP_DIR).glob(f"*/{name}/{name}"))
    assert len(backups) == 1
    assert backups[0].read_bytes() == b"previous career"
    assert old.read_bytes() != b"previous career"
    assert "backed up" in capsys.readouterr().out


@pytest.mark.skipif(not PAIR_360.is_file(), reason="pair fixture missing")
def test_cli_dry_run_makes_no_backup(tmp_path):
    out = tmp_path / "out"
    from nfssave import read_container
    name = read_container(PAIR_360).name
    _seed(out, name, b"previous career")
    cli.convert_one(PAIR_360, Namespace(out_root=str(out), dry_run=True, twin=None))
    assert not (tmp_path / lib.BACKUP_DIR).exists()


# --- review follow-ups (parity with app/batch.rs run_batch) ------------------

@pytest.mark.skipif(not PAIR_360.is_file(), reason="pair fixture missing")
def test_batch_refuses_second_save_with_same_name(tmp_path):
    out = tmp_path / "out"
    claimed = {}
    args = Namespace(out_root=str(out), dry_run=False, twin=None, backup_stamp="s")
    other = tmp_path / "G" / PAIR_360.name  # same STFS name, other stick
    other.parent.mkdir()
    other.write_bytes(PAIR_360.read_bytes())
    cli.convert_one(PAIR_360, args, claimed)
    with pytest.raises(ValueError, match="also named"):
        cli.convert_one(other, args, claimed)
    assert not (tmp_path / lib.BACKUP_DIR).exists()


@pytest.mark.skipif(not PAIR_360.is_file(), reason="pair fixture missing")
def test_backup_failure_leaves_existing_untouched(tmp_path, monkeypatch, capsys):
    from nfssave import read_container
    name = read_container(PAIR_360).name
    out = tmp_path / "out"
    old = _seed(out, name, b"previous career")

    def boom(*a, **k):
        raise PermissionError("denied")
    monkeypatch.setattr(cli, "back_up_existing", boom)
    monkeypatch.setattr(sys, "argv", ["convert.py", str(PAIR_360), "--out-root", str(out)])
    assert cli.main() == 1
    assert old.read_bytes() == b"previous career"
    assert "left it untouched" in capsys.readouterr().err
