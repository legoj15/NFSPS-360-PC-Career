# NFS ProStreet 360 → PC Career Converter

Converts Xbox 360 Need for Speed ProStreet career saves into PC saves.

Instructions assume you are using the [community repack with the update and DLC](https://www.reddit.com/r/abandonware/comments/zr41zu/the_complete_abandonware_need_for_speed_download/). Other types of installations have not been tested.

## Getting your saves (if you haven't copied them already)
1. Insert a FAT32 formatted flash drive into your Xbox 360
2. Navigate to the "Settings" tab in the dashboard and go to "System", then "Storage"
3. Select either your Hard Drive or Cloud saves, then find and open ProStreet
4. Press A on the careers (01, 02, and/or 03) you wish to convert and choose "Copy" and select the flash drive.
5. Optionally copy your alias as well if you wish to keep the same name
6. Turn off the Xbox and take the flash drive and put it into your PC

## Windows app (easiest way)

1. Download and launch `NFSPS-SaveConverter.exe` from GitHub Releases.
	- If Windows shows "**Windows protected your PC**", click **More info**, then
   **Run anyway**. That screen appears because the program is new, not because
   something is wrong (and certificates cost money).
2. If you have your saves on a flashdrive, the program will automatically detect them
	- **If you used an older 360 dashboard you may need to click the "scan for FATX" button**
	- If you already have your saves on your computer, click **Add a single file…** or **Add a folder…** and choose them.
3. Click **Select location to export saves…** and choose where the converted
   saves should go. If you have the community repack in your documents folder, the program will tell you the recommended spot to chose.
4. Click **Convert**. Then start the game, pick your alias, and your career
   is there to load
	- If a save with the same name was already there, the old
   one is copied to a **SaveConverter backups** folder next to it first.

## PowerShell (built into Windows)

Download the repository, open the `scripts\powershell` folder, and run:

    powershell -ExecutionPolicy Bypass -File Convert-NfsSave.ps1 "<save file or folder>"

## Python (other setups and operating systems)

    python scripts/python/convert.py "<save file or folder>"

## Using the scripts

- The converted saves are written to the folder you run the script from.
  To choose another folder, add `-OutRoot "<folder>"` (PowerShell) or
  `--out-root "<folder>"` (Python).
- To read the saves straight from the flash drive you copied them to on the
  Xbox 360, use `-Usb F:` (PowerShell) or `--usb F:` (Python), with your
  flash drive's letter in place of `F:`.
- If you run the script inside the game's folder (the one holding the `SAVE`
  or `NFS ProStreet` folder), or point `-OutRoot` / `--out-root` there, the
  saves are put in the right place automatically. A save with the same name
  that is already there is copied to a **SaveConverter backups** folder first.
- Otherwise, copy the converted `CAREER_..` and `ALIAS_..` folders into the
  game's `SAVE\NFS ProStreet` folder yourself.
- Add `-DryRun` (PowerShell) or `--dry-run` (Python) to check saves without
  writing anything.

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

## Building the Windows app

Have the Rust toolkit installed. Enter the `src` folder, and run `cargo build --release`, the executable will be within the `release` folder inside the `target` folder.

## Repository layout

- `src/` — the Windows app (Rust).
- `scripts/` — the cross-platform converter for other setups (one folder per
  language: Python and PowerShell).
- `docs/` — research notes and findings behind the converter.
