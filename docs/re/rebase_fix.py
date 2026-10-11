"""rebase_fix.py <old git rev> <360 source save> <played PC save> <out>

Carry a converter fix into a save the player has already used on PC, without
losing the PC progress. Converts the 360 source with the converter at <old
git rev> (what the player converted with) and with the working-tree converter,
then three-way merges per u32 word: a word the fix changed is applied only
where the played save still holds the old converter's value; words the game
rewrote since are kept and listed (check each one by hand: a word that is
neither the old nor the fixed value may mix PC-written and stale bytes).
Records whose size differs between
the conversions and the played save are left alone. GameplayData is rehashed
if touched. Rebuilds the tree hash and CRCs; the extra blob is kept (record
sizes do not change).

First use 2026-10-11: the FEPlayerCarDB record-handle fix (reward cars never
reached the garage) applied to a CAREER_01 played for a day on PC:
  python docs/re/rebase_fix.py 37a0d13 docs/re/c1_latest/CAREER_01_360 \\
      "<save dir>/CAREER_01/CAREER_01" <out>
The <old git rev> must be a commit whose converter produced the played save's
original conversion. The tool cannot prove that; it prints how many words of
each touched record differ between the old conversion and the played save
(the game's own changes since). A large count suggests the wrong <old rev>.
"""
import hashlib
import importlib
import os
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.normpath(os.path.join(HERE, "..", ".."))
sys.path.insert(0, os.path.join(REPO, "scripts", "python"))


def load_old_package(rev: str, tmp: str):
    """Import scripts/python/nfssave at <rev> as package 'nfssave_old'."""
    dst = os.path.join(tmp, "nfssave_old")
    os.makedirs(dst)
    names = subprocess.run(["git", "-C", REPO, "ls-tree", "-z", "--name-only", rev,
                            "scripts/python/nfssave/"], capture_output=True, check=True,
                           text=True).stdout.split("\0")
    names = [n for n in names if n]
    if not names:
        raise SystemExit(f"{rev}: no scripts/python/nfssave/ in that revision")
    for n in names:
        blob = subprocess.run(["git", "-C", REPO, "show", f"{rev}:{n}"],
                              capture_output=True, check=True).stdout
        with open(os.path.join(dst, os.path.basename(n)), "wb") as f:
            f.write(blob)
    sys.path.insert(0, tmp)
    return importlib.import_module("nfssave_old.convert"), importlib.import_module("nfssave_old")


def converted_records(pkg_convert, pkg, src_path):
    mc = pkg.MC02.parse(pkg.read_container(src_path).payload)
    pc = pkg_convert.convert_payload(mc, pkg_convert.ConversionReport())
    tree = pkg_convert.Tree.parse(pc.tree, big=False)
    return {r.id: r.payload for r in tree.records}


def main(rev, src_path, played_path, out_path):
    import nfssave
    from nfssave import convert as new
    from nfssave.mc02 import MC02
    from nfssave.tree import Tree
    from nfssave.treehash import tree_hash

    with tempfile.TemporaryDirectory() as tmp:
        old_convert, old_pkg = load_old_package(rev, tmp)
        old = converted_records(old_convert, old_pkg, src_path)
    fixed = converted_records(new, nfssave, src_path)

    m = MC02.parse(open(played_path, "rb").read())
    tree = Tree.parse(m.tree, big=False)
    total_applied = 0
    for rec in tree.records:
        a, b, p = old.get(rec.id), fixed.get(rec.id), rec.payload
        name = new.CHUNK_NAMES.get(rec.id, hex(rec.id))
        if a is None or b is None:
            if a != b:
                print(f"  {name}: missing from one conversion, left alone")
            continue
        if not len(a) == len(b) == len(p):
            if a != b:
                print(f"  {name}: sizes differ, left alone")
            continue
        # whole words, so a field the game rewrote is never half-patched
        changed = [i for i in range(0, len(a), 4) if a[i:i + 4] != b[i:i + 4]]
        if not changed:
            continue
        q = bytearray(p)
        applied, conflicts = 0, []
        for i in changed:
            if p[i:i + 4] == a[i:i + 4]:
                q[i:i + 4] = b[i:i + 4]
                applied += 1
            elif p[i:i + 4] != b[i:i + 4]:
                conflicts.append(i)
        drift = sum(1 for i in range(0, len(a), 4) if a[i:i + 4] != p[i:i + 4])
        print(f"  {name}: fix changes {len(changed)} words, applied {applied}, "
              f"kept {len(conflicts)} the game rewrote; the played save differs from "
              f"the old conversion in {drift} words in all")
        for i in conflicts[:20]:
            print(f"    kept {i:#x}: old {a[i:i + 4].hex()} fixed {b[i:i + 4].hex()} "
                  f"played {p[i:i + 4].hex()}")
        rec.payload = bytes(q)
        total_applied += applied
        if rec.id == new.GAMEPLAY_ID:
            new.rehash_gameplay(rec)
    built = bytearray(tree.build(big=False, tree_size=m.tree_size))
    built[0:16] = tree_hash(bytes(built))
    pc = MC02(m.endian, extra=m.extra, tree=bytes(built), tree_size=m.tree_size)
    data = pc.to_bytes()
    problems = MC02.parse(data).check()
    if problems:
        raise SystemExit(f"self-check failed: {problems}")
    with open(out_path, "wb") as f:
        f.write(data)
    print(out_path, hashlib.md5(data).hexdigest(), f"({total_applied} bytes applied)")


if __name__ == "__main__":
    if len(sys.argv) != 5:
        raise SystemExit(__doc__)
    main(*sys.argv[1:])
