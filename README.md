# NFS ProStreet 360 → PC Career Converter

Converts Xbox 360 Need for Speed ProStreet career saves into PC saves.

## Windows app (easiest way)

1. Download `NFSPS-SaveConverter.exe` from GitHub Releases and double-click it.
2. If Windows shows "**Windows protected your PC**", click **More info**, then
   **Run anyway**. That screen appears because the program is new, not because
   something is wrong.
3. Plug in the flash drive you use with your Xbox 360 — the app finds the
   saves on it by itself. If the saves are already on your computer, use
   **Add a single file…** or **Add a folder…** and pick them.
4. Click **Select location to export saves…** and choose where the converted
   saves should go. The app already suggests the game's own save folder.
5. Click **Convert**. Then start the game, pick your alias, and your career
   is there to load.

To build the program yourself instead: `cargo build --release` inside `src/`.

## Command line (other setups)

This assumes you are using the [community repack with the update and DLC](https://www.reddit.com/r/abandonware/comments/zr41zu/the_complete_abandonware_need_for_speed_download/). Other types of installations have not been tested.

    python scripts/python/convert.py "<path to a 360 save file>"

Converted saves land in the game's save folder
(`Documents\Need for Speed ProStreet\SAVE\NFS ProStreet`) — or pass
`--out-root` to put them somewhere else first. Add `--flash F: --all` to
convert every save on the 360-formatted flash drive at once.

After converting, launch the game and the career should appear in the
load-menu after selecting your alias.

## What it does

- Reads the console's save container and the `MC02` save inside it.
- Re-encodes every chunk from console to PC byte order (the two versions
  use the same save structure, just mirrored).
- Recomputes all checksums so the PC game accepts the file.

## Status

Verified in-game (October 2026): converted careers resume exactly where
they were saved on the console (including a race day in progress), and
every car loads in the garage with its blueprints, paint, decals and
vinyls - including heavily decorated DLC cars.

## Repository layout

- `src/` — the Windows app (Rust).
- `scripts/` — the cross-platform converter for other setups (one folder per
  language; Python works today, quickBMS and PowerShell versions are planned).
- `docs/` — research notes and findings behind the converter.
