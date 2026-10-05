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

October 2026: a reading bug that cut off the end of every console career
(race progress and unlocks) is fixed, and cars now keep their installed
parts. Waiting on an in-game check; the garage crash is still being
investigated. See `docs/HANDOFF.md`.
