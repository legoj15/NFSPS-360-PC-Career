"""CLI behaviour of scripts/python/convert.py (contract: docs/scripts-cli.md):
inputs (files, folders, USB drives), output-folder detection, and backups
of replaced saves (game-folder convention shared with the Windows app,
src/crates/nfspc-converter/src/app/batch.rs back_up_existing)."""

import inspect
import re
import sys
from argparse import Namespace
from datetime import datetime, timezone
from pathlib import Path

import pytest

import convert as cli
import nfssave.convert as lib

ROOT = Path(__file__).parent.parent
PAIR_360 = ROOT / "docs/re/pair/CAREER_02_360_fresh"


# --- 1. no author-specific default path ------------------------------------

def test_no_hardcoded_author_path():
    assert not hasattr(lib, "PC_SAVE_ROOT")
    for mod in (lib, cli):
        assert "legoj" not in Path(mod.__file__).read_text(encoding="utf-8")


def test_write_pc_save_requires_explicit_root():
    param = inspect.signature(lib.write_pc_save).parameters["save_root"]
    assert param.default is inspect.Parameter.empty


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
    args = Namespace(out_root=str(out), dry_run=False)
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
    cli.convert_one(PAIR_360, Namespace(out_root=str(out), dry_run=True))
    assert not (tmp_path / lib.BACKUP_DIR).exists()


# --- review follow-ups (parity with app/batch.rs run_batch) ------------------

@pytest.mark.skipif(not PAIR_360.is_file(), reason="pair fixture missing")
def test_batch_refuses_second_save_with_same_name(tmp_path):
    out = tmp_path / "out"
    claimed = {}
    args = Namespace(out_root=str(out), dry_run=False, backup_stamp="s")
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


# --- CLI redesign (docs/scripts-cli.md) --------------------------------------

PAIR_MD5 = "3da9f4c0a5a2b7d5c55863d49de4852c"


def _md5(p: Path) -> str:
    import hashlib
    return hashlib.md5(p.read_bytes()).hexdigest()


def _run(monkeypatch, *argv) -> int:
    monkeypatch.setattr(sys, "argv", ["convert.py", *map(str, argv)])
    return cli.main()


@pytest.mark.parametrize("layout, expect, game", [
    ("plain", "", False),
    ("SAVE/NFS ProStreet", "SAVE/NFS ProStreet", True),
    ("NFS ProStreet", "NFS ProStreet", True),
    ("save/nfs prostreet", "save/nfs prostreet", True),  # case-insensitive
])
def test_resolve_save_folder(tmp_path, layout, expect, game):
    root = tmp_path / "R"
    if layout != "plain":
        (root / layout).mkdir(parents=True)
    root.mkdir(exist_ok=True)
    save, backup_base, is_game = cli.resolve_save_folder(root)
    assert save == root / expect if expect else save == root
    assert is_game is game
    assert backup_base == (save.parent if game else root)


def test_resolve_save_folder_root_is_save_folder(tmp_path):
    root = tmp_path / "SAVE" / "NFS ProStreet"
    root.mkdir(parents=True)
    save, backup_base, is_game = cli.resolve_save_folder(root)
    assert (save, backup_base, is_game) == (root, root.parent, True)


def test_resolve_save_folder_bare_save_dir_is_plain(tmp_path):
    # a generic SAVE folder without NFS ProStreet inside is not the game's
    (tmp_path / "SAVE").mkdir()
    save, backup_base, is_game = cli.resolve_save_folder(tmp_path)
    assert (save, backup_base, is_game) == (tmp_path, tmp_path, False)


def test_find_saves_in_folder(tmp_path):
    sub = tmp_path / "a" / "b"
    sub.mkdir(parents=True)
    (sub / "CAREER_02").write_bytes(b"CON " + b"\0" * 16)
    (sub / "alias_x").write_bytes(b"CON " + b"\0" * 16)
    (sub / "CAREER_99").write_bytes(b"MC02" + b"\0" * 16)  # not a container
    (sub / "notes.txt").write_bytes(b"CON hello")
    bk = tmp_path / "SaveConverter backups" / "s" / "CAREER_01"
    bk.mkdir(parents=True)
    (bk / "CAREER_01").write_bytes(b"CON " + b"\0" * 16)
    assert cli.find_saves_in(tmp_path) == sorted([sub / "CAREER_02", sub / "alias_x"])


@pytest.mark.skipif(not PAIR_360.is_file(), reason="pair fixture missing")
def test_default_output_is_current_directory(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    assert _run(monkeypatch, PAIR_360) == 0
    assert _md5(tmp_path / "CAREER_02" / "CAREER_02") == PAIR_MD5


@pytest.mark.skipif(not PAIR_360.is_file(), reason="pair fixture missing")
def test_folder_input(tmp_path, monkeypatch):
    src = tmp_path / "in" / "deep"
    src.mkdir(parents=True)
    (src / "CAREER_02").write_bytes(PAIR_360.read_bytes())
    (src / "CAREER_99").write_bytes(b"not a save")
    out = tmp_path / "out"
    assert _run(monkeypatch, tmp_path / "in", "--out-root", out) == 0
    assert _md5(out / "CAREER_02" / "CAREER_02") == PAIR_MD5
    assert not (out / "CAREER_99").exists()


def test_folder_without_saves_fails(tmp_path, monkeypatch, capsys):
    (tmp_path / "empty").mkdir()
    assert _run(monkeypatch, tmp_path / "empty", "--out-root", tmp_path / "o") == 1
    assert "no saves found" in capsys.readouterr().err


@pytest.mark.skipif(not PAIR_360.is_file(), reason="pair fixture missing")
@pytest.mark.parametrize("layout", ["SAVE/NFS ProStreet", "NFS ProStreet"])
def test_game_folder_detected(tmp_path, monkeypatch, capsys, layout):
    game = tmp_path / "game"
    (game / layout).mkdir(parents=True)
    old = _seed(game / layout, "CAREER_02", b"old")
    assert _run(monkeypatch, PAIR_360, "--out-root", game) == 0
    assert _md5(old) == PAIR_MD5
    assert "game save folder" in capsys.readouterr().out
    backups = list((old.parent.parent.parent / lib.BACKUP_DIR).glob("*/CAREER_02/CAREER_02"))
    assert [b.read_bytes() for b in backups] == [b"old"]


@pytest.mark.skipif(not PAIR_360.is_file(), reason="pair fixture missing")
def test_plain_output_backs_up_inside_root(tmp_path, monkeypatch, capsys):
    out = tmp_path / "plain"
    _seed(out, "CAREER_02", b"old")
    assert _run(monkeypatch, PAIR_360, "--out-root", out) == 0
    backups = list((out / lib.BACKUP_DIR).glob("*/CAREER_02/CAREER_02"))
    assert [b.read_bytes() for b in backups] == [b"old"]
    assert not (tmp_path / lib.BACKUP_DIR).exists()
    text = capsys.readouterr().out
    assert "output folder" in text and "NFS ProStreet" in text


@pytest.mark.skipif(not PAIR_360.is_file(), reason="pair fixture missing")
def test_missing_out_root_created_but_not_on_dry_run(tmp_path, monkeypatch):
    dry = tmp_path / "dry" / "x"
    assert _run(monkeypatch, PAIR_360, "--out-root", dry, "--dry-run") == 0
    assert not dry.exists()
    real = tmp_path / "real" / "x"
    assert _run(monkeypatch, PAIR_360, "--out-root", real) == 0
    assert _md5(real / "CAREER_02" / "CAREER_02") == PAIR_MD5


@pytest.mark.skipif(not PAIR_360.is_file(), reason="pair fixture missing")
@pytest.mark.parametrize("flag", ["--usb", "--flash"])
def test_usb_walk(tmp_path, monkeypatch, flag):
    stick = tmp_path / "stick"
    d = stick / "Content" / "E00001CFFAB204C4" / "45410822" / "00000001"
    d.mkdir(parents=True)
    (d / "career_02").write_bytes(PAIR_360.read_bytes())
    (d / "name.txt").write_text("not a save")
    out = tmp_path / "out"
    assert _run(monkeypatch, flag, stick, "--out-root", out) == 0
    assert _md5(out / "CAREER_02" / "CAREER_02") == PAIR_MD5


def test_usb_without_saves_fails(tmp_path, monkeypatch):
    assert _run(monkeypatch, "--usb", tmp_path, "--out-root", tmp_path / "o") == 1
    assert not (tmp_path / "o").exists()


@pytest.mark.skipif(not PAIR_360.is_file(), reason="pair fixture missing")
def test_empty_usb_does_not_stop_other_inputs(tmp_path, monkeypatch):
    out = tmp_path / "out"
    assert _run(monkeypatch, PAIR_360, "--usb", tmp_path / "stick", "--out-root", out) == 1
    assert (out / "CAREER_02" / "CAREER_02").is_file()


def test_no_input_is_usage_error(tmp_path, monkeypatch):
    with pytest.raises(SystemExit) as e:
        _run(monkeypatch, "--out-root", tmp_path)
    assert e.value.code == 2


@pytest.mark.skipif(not PAIR_360.is_file(), reason="pair fixture missing")
def test_several_inputs(tmp_path, monkeypatch):
    c1 = ROOT / "docs/re/c1_latest/CAREER_01_360"
    out = tmp_path / "out"
    assert _run(monkeypatch, PAIR_360, c1, "--out-root", out) == 0
    assert (out / "CAREER_01" / "CAREER_01").is_file()
    assert (out / "CAREER_02" / "CAREER_02").is_file()


# --- review follow-ups: scripts-cli-redesign (Qwen 27B + GLM flash) ---------

C1_360 = ROOT / "docs/re/c1_latest/CAREER_01_360"


@pytest.mark.skipif(not PAIR_360.is_file(), reason="pair fixture missing")
def test_folder_with_several_saves(tmp_path, monkeypatch):
    src = tmp_path / "in"
    src.mkdir()
    (src / "CAREER_02").write_bytes(PAIR_360.read_bytes())
    (src / "CAREER_01").write_bytes(C1_360.read_bytes())
    out = tmp_path / "out"
    assert _run(monkeypatch, src, "--out-root", out) == 0
    assert _md5(out / "CAREER_02" / "CAREER_02") == PAIR_MD5
    assert (out / "CAREER_01" / "CAREER_01").is_file()


@pytest.mark.skipif(not PAIR_360.is_file(), reason="pair fixture missing")
def test_file_input_any_name(tmp_path, monkeypatch):
    f = tmp_path / "my save.bin"
    f.write_bytes(PAIR_360.read_bytes())
    out = tmp_path / "out"
    assert _run(monkeypatch, f, "--out-root", out) == 0
    assert _md5(out / "CAREER_02" / "CAREER_02") == PAIR_MD5


@pytest.mark.skipif(not PAIR_360.is_file(), reason="pair fixture missing")
def test_overlapping_inputs_convert_once(tmp_path, monkeypatch):
    src = tmp_path / "in"
    (src / "sub").mkdir(parents=True)
    (src / "sub" / "CAREER_02").write_bytes(PAIR_360.read_bytes())
    out = tmp_path / "out"
    assert _run(monkeypatch, src, src / "sub", src / "sub" / "CAREER_02",
                "--out-root", out) == 0
    assert not (out / lib.BACKUP_DIR).exists()


@pytest.mark.skipif(not PAIR_360.is_file(), reason="pair fixture missing")
def test_failed_save_does_not_claim_its_name(tmp_path, monkeypatch):
    # exe parity (nfspc-converter tests/batch.rs failed_save_does_not_claim_its_name)
    real = cli.convert_payload
    calls = []

    def flaky(*a, **k):  # the first save fails mid-conversion
        calls.append(1)
        if len(calls) == 1:
            raise ValueError("boom")
        return real(*a, **k)
    monkeypatch.setattr(cli, "convert_payload", flaky)
    claimed = {}
    args = Namespace(out_root=str(tmp_path / "out"), dry_run=False, backup_stamp="s")
    with pytest.raises(ValueError, match="boom"):
        cli.convert_one(PAIR_360, args, claimed)
    cli.convert_one(PAIR_360, args, claimed)
    assert _md5(tmp_path / "out" / "CAREER_02" / "CAREER_02") == PAIR_MD5


def test_claim_key_folds_like_windows():
    assert cli.windows_name_key("CAREER_02. ") == cli.windows_name_key("career_02")


def test_out_root_is_a_file_is_usage_error(tmp_path, monkeypatch):
    f = tmp_path / "file"
    f.write_bytes(b"x")
    with pytest.raises(SystemExit) as e:
        _run(monkeypatch, PAIR_360, "--out-root", f)
    assert e.value.code == 2


def test_empty_string_input_is_not_cwd(tmp_path, monkeypatch, capsys):
    monkeypatch.chdir(tmp_path)
    (tmp_path / "CAREER_02").write_bytes(PAIR_360.read_bytes())
    assert _run(monkeypatch, "", "--out-root", tmp_path / "o") == 1
    assert not (tmp_path / "o").exists()


def test_all_inputs_failed_prints_no_banner(tmp_path, monkeypatch, capsys):
    (tmp_path / "empty").mkdir()
    assert _run(monkeypatch, tmp_path / "empty", "--out-root", tmp_path / "o") == 1
    assert "output folder" not in capsys.readouterr().out


@pytest.mark.skipif(not PAIR_360.is_file(), reason="pair fixture missing")
def test_old_all_flag_still_accepted(tmp_path, monkeypatch):
    stick = tmp_path / "stick"
    d = stick / "Content" / "P" / "45410822" / "00000001"
    d.mkdir(parents=True)
    (d / "CAREER_02").write_bytes(PAIR_360.read_bytes())
    assert _run(monkeypatch, "--flash", stick, "--all", "--out-root", tmp_path / "o") == 0


@pytest.mark.skipif(not PAIR_360.is_file(), reason="pair fixture missing")
def test_named_save_folder_end_to_end(tmp_path, monkeypatch):
    s = tmp_path / "SAVE" / "NFS ProStreet"
    _seed(s, "CAREER_02", b"old")
    assert _run(monkeypatch, PAIR_360, "--out-root", s) == 0
    backups = list((tmp_path / "SAVE" / lib.BACKUP_DIR).glob("*/CAREER_02/CAREER_02"))
    assert [b.read_bytes() for b in backups] == [b"old"]


# --- dry run = same exit code and refusals as a real run ---------------------

def _unsafe_name_save(dst: Path, bad: str = "CAREER/02") -> Path:
    """Copy of the pair fixture whose STFS file-table name is `bad` (same
    length as the original, so the container still parses)."""
    from nfssave import read_container
    from nfssave.container360 import stfs_block_offset
    data = bytearray(PAIR_360.read_bytes())
    old = read_container(PAIR_360).name.encode("ascii")
    assert len(bad) == len(old)
    first_table = (int.from_bytes(data[0x340:0x344], "big") + 0xFFF) & ~0xFFF
    block = int.from_bytes(data[0x37E:0x381], "little")
    off = stfs_block_offset(block, first_table, 0 if data[0x37B] & 1 else 1)
    assert bytes(data[off:off + len(old)]) == old
    data[off:off + len(old)] = bad.encode("ascii")
    dst.parent.mkdir(parents=True, exist_ok=True)
    dst.write_bytes(bytes(data))
    return dst


@pytest.mark.skipif(not PAIR_360.is_file(), reason="pair fixture missing")
@pytest.mark.parametrize("dry", [False, True])
def test_unsafe_save_name_fails_the_same_in_dry_run(tmp_path, monkeypatch, capsys, dry):
    src = _unsafe_name_save(tmp_path / "in" / "bad")
    out = tmp_path / "out"
    argv = [src, "--out-root", out] + (["--dry-run"] if dry else [])
    assert _run(monkeypatch, *argv) == 1
    assert "unsafe save name 'CAREER/02'" in capsys.readouterr().err
    assert not out.exists() or not any(out.iterdir())
