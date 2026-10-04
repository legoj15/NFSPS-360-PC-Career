# NFSPS 360 → PC Career Converter

Turns Xbox 360 Need for Speed ProStreet career saves into PC saves.

## How to use

    python convert.py "<path to a 360 save file>"

Converted saves land in the game's save folder
(`Documents\Need for Speed ProStreet\SAVE\NFS ProStreet`) — or pass
`--out-root` to put them somewhere else first. Add `--flash F: --all` to
convert every save on the 360-formatted flash drive at once.

After converting, launch the game and the career should appear in the
load-menu under your profile name (e.g. `JOSHUA S 10`).

## What it does

- Reads the console's save container and the `MC02` save inside it.
- Re-encodes every chunk from console to PC byte order (the two versions
  use the same save structure, mirrored).
- Recomputes all checksums so the PC game accepts the file.

## Status

Verified working in-game (October 2026): the `JOSHUA S 10` profile and its
converted career load on PC with all progress intact — money, career day,
repair markers, and cars.

- All three careers and the profile are installed in the game's save
  folder, ready to play.
- Your console's CAREER_01 had two damaged parts (a console-side writing
  bug); the converter fully recovers them from a re-saved copy of the same
  session (auto-detected from the NFSPS360 folder).
- CAREER_02/03 had the same damage with no matching re-save, so they
  convert without their race-day/unlock parts — the game fills in defaults
  for those. Re-saving each career once in the NFSPS360 recomp and
  re-running the converter would complete them.
