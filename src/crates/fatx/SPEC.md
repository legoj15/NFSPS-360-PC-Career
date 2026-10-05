# FATX / Xbox 360 USB — on-disk format notes (agent-facing)

Verified 2026-10-05 against the sources listed at the bottom. Anything not
confirmed by at least two independent sources (or by the tracked oracle files)
is marked **UNVERIFIED**.

This crate targets the **Xbox 360** variant of FATX (USB sticks and HDD
images). The original-Xbox variant (`FATX` magic, little-endian fields,
different superblock layout) is intentionally *not* supported.

## 1. Xbox 360 FATX ("XTAF") endianness and magic

* On-disk magic bytes at partition start are literally `XTAF` (`58 54 41 46`).
* **All multi-byte fields in the superblock, FAT and directory entries are
  big-endian** on the Xbox 360.
* The nickname "XTAF" is "FATX" reversed; free60's intro sentence ("little
  endian header") is stale text contradicted by its own tables.

Sources: free60 FATX page (partition-header table: magic `XTAF`, "All
multi-byte values … are big-endian"); FATXTools `Volume.cs`
(`VolumeSignature = 0x58544146` with big-endian reader);
`emoose/xbox-winfsp` (`FatxDevice`/`FatxFileSystem`, checks XTAF BE);
`dpteam/Xbox360_USB_Explorer` (`MAGIC = 0x58544146 // "XTAF" big-endian`);
`lornix/xboxfs-tools` (`FATXMAGIC_BE = 0x58544146`, `getintBE` everywhere);
Party Buffalo (`Drive.cs` `IsFATXDrive` reads `0x58544146 /*XTAF*/`).

## 2. Partition header (superblock), first 0x1000 bytes of a partition

| Offset | Size | Type | Meaning |
|--------|------|------|---------|
| 0x0    | 4    | ascii | magic `XTAF` |
| 0x4    | 4    | u32 BE | volume/partition id (random) |
| 0x8    | 4    | u32 BE | sectors per cluster (cluster size = value * 0x200) |
| 0xC    | 4    | u32 BE | root directory first cluster (normally 1) |

Rest of the 0x1000-byte header region is reserved/zero. Sources: free60
"Partition Header"; FATXTools `ReadVolumeMetadata`; winfsp `FatxHeader`;
Xbox360_USB_Explorer `FatxHeader.Read`.

## 3. FAT ("chainmap") and data area

* FAT starts at partition offset **0x1000**. Exactly one FAT on the 360.
* Entry width: 2 bytes if cluster count < **0xFFF0** (FAT16-style), else 4
  bytes (FAT32-style). On USB data partitions the count is far above the
  threshold, so 4-byte entries in practice.
* Cluster count = `partition_length / cluster_size` (+1 reserved FAT slot,
  i.e. FAT covers entries for cluster 0 .. cluster N).
* **FAT size in bytes is rounded up to the 0x1000 page boundary.**
  FATXTools (`_bytesPerFat = (bytesPerFat + 0xFFF) & ~0xFFF`),
  xbox-winfsp (`RoundToPages(chainMapLength, 0x1000)`),
  Xbox360_USB_Explorer (`((rawFat + 0xFFF) / 0x1000) * 0x1000`) and
  `fatx360fs` (`fat_size_aligned`) all round up. The decompiled USB XTAF
  Explorer and the free60 prose do *not* round — **discrepancy noted**; we
  follow the 4-tool majority because two of those tools are verified against
  real retail drives today.
* Data area starts at `0x1000 + fat_size`. **Cluster numbering starts at 1**;
  byte offset of cluster N = `data_start + (N - 1) * cluster_size`.
* Special FAT entry values (4-byte set; 16-bit set analogous below 0xFFF0):
  * `0x00000000` — free
  * `0xFFFFFFF8` — media descriptor, written to FAT[0]
  * `0xFFFFFFF7` — bad cluster
  * `>= 0xFFFFFFF0` — end of chain (in practice `0xFFFFFFFF`)
* The root directory lives at the cluster named by superblock[0xC]; its FAT
  entry terminates the chain (`0xFFFFFFFF`).

## 4. Directory entries (0x40 bytes each)

| Offset | Size | Type | Meaning |
|--------|------|------|---------|
| 0x00 | 1 | u8 | file name length; `0xE5` = deleted entry; `0x00`/`0xFF` = never used → end of used directory slots |
| 0x01 | 1 | u8 | attributes (FAT-style): `0x10` directory, `0x20` archive, `0x01` read-only, `0x02` hidden, `0x04` system |
| 0x02 | 0x2A | ascii | file name, 0x00/0xFF padded, max **42** bytes |
| 0x2C | 4 | u32 BE | first cluster (0 = empty file) |
| 0x30 | 4 | u32 BE | file size in bytes (for directories: allocated size) |
| 0x34–0x3F | 12 | 6×u16 BE | FAT-style create/write/access date+time pairs |

* Deleting a file writes `0xE5` into the **name-length byte** (offset 0) and
  frees the FAT chain (verified in decompiled USB XTAF Explorer `delete()`;
  FATXTools/Xbox360_USB_Explorer both test `nameLength == 0xE5`). free60
  additionally mentions 0xE5 in the attributes byte — we treat offset 0 as
  authoritative.
* No `.`/`..` entries exist. Path walking must track the parent itself.
* Fresh directory clusters are 0xFF-filled (Xbox360_USB_Explorer `InjectFolder`).
* Limits (free60): max filename 42, max path 240 chars, max 4096 entries per
  directory, max file 4 GiB, cluster sizes 4/8/16/32/64 KiB.
* Names are compared case-insensitively on console; we match
  `eq_ignore_ascii_case`.

## 5. Whole-drive layout (where the FATX Data partition lives)

### 5.1 Retail USB stick (formatted by the console as a Memory Unit)

Two equivalent views exist:

1. **Host (FAT) view** — the stick carries a FAT16/FAT32 volume with a hidden
   `Xbox360` folder holding `Data0000`, `Data0001`, … `DataNNNN` files.
   `Data0000` (~512 MiB) holds the cache/SysExt partitions and the signed
   device-configuration sectors; `Data0001`+ concatenated **are** the FATX
   Data partition. This is what USB XTAF Explorer and Xbox360_USB_Explorer
   open.
2. **Raw view** — fixed byte offsets on the raw device (free60 "USB Drive"
   table, Party Buffalo `Geometry.USBOffsets`):

| Offset | Length | Content |
|--------|--------|---------|
| 0x8000400 | 0x12000400 | System Cache (FATX) |
| 0x8115200 | 0x8000000 | SysExt (FATX sub-partition) |
| 0x12000400 | 0xDFFFC00 | SysExt2 (FATX sub-partition) |
| **0x20000000** | end of media | **Data partition (FATX) — where `Content/` lives** |

There is **no raw partition table** on retail media: "the media which contain
this file system do not have a master file table … it is up to the consumer to
know this" (free60). We therefore detect by probing the fixed offsets and
validating the `XTAF` magic.

### 5.2 Retail HDD

Data partition at 0x130EB0000 (stock 20 GB geometry; differs on larger
drives), security sector at 0x2000. We probe the fixed offset and validate the
magic.

### 5.3 Devkit HDD partition table (0x18 bytes at LBA 0, little-endian)

| Offset | Size | Meaning |
|--------|------|---------|
| 0x0 | 4 | devkit magic `0x00020000` |
| 0x4 | 4 | unknown |
| 0x8 | 4 | Content (Data) volume start LBA |
| 0xC | 4 | Content volume length in sectors (×0x200) |
| 0x10 | 4 | Dashboard volume start LBA |
| 0x14 | 4 | Dashboard volume length in sectors |

Sources: free60 "Development Kit HDD Partition Table"; Party Buffalo
`Drive.DevPartitions()` (reads u32 sector + u32 sector count at 0x8/0xC).

### 5.4 `MICROSOFT*XBOX360` signature — **UNVERIFIED / not relied upon**

The task brief mentions a `MICROSOFT*XBOX360` signature at sector 0
(community lore places it at byte 0x1FF, 17 bytes, overlapping into sector 1).
**No primary source or open-source tool I could reach reads it**: it is absent
from the free60 wiki, xboxdevwiki, FATXTools, Party Buffalo, xbox-winfsp,
Xbox360_USB_Explorer, usb-xtaf-explorer, fatx360fs and lornix/xboxfs-tools.
Web search associates the string with the Xbox 360 **DVD-drive ATA inquiry
data**, not with storage-media sector 0. Our `partition` module *tolerates*
the signature anywhere in the first 0x400 bytes as a detection hint
(`signature_found`) but never requires it; Data-partition location always
comes from the devkit table (if present) or the fixed retail offsets with
`XTAF` validation. The synthetic-image builder writes it at 0x1FF purely to
exercise this tolerant path. If a primary source for a USB sector-0 partition
table materializes, extend `partition.rs` — the API already returns parsed
regions.

## 6. Where saves live inside the Data partition

`Content/0000000000000000/<profile-id>/<titleID-hex>/00000001|00000002/<file>`

* `Content` sits at the FATX root (alongside `name.txt`, which stores the
  volume label as UTF-16BE from byte offset 2).
* `0000000000000000` is the "all profiles"/signed-for-console bucket; profile
  folders are 16 hex characters.
* The title folder name is the 8-hex-digit title ID.
* `00000001` = saved game (the CON package), `00000002` = publisher/
  marketplace-ish save data. Both are scanned.

## 7. CON (STFS) header fields we parse

Empirically verified against the tracked oracles
`docs/re/c1_latest/CAREER_01_360` and `docs/re/pair/CAREER_02_360_fresh`
(identical bytes in both at these offsets), cross-checked with Party Buffalo's
`STFSOffsets` enum and the repo's own Python spec
`scripts/python/nfssave/container360.py`:

| Offset | Meaning |
|--------|---------|
| 0x000 | magic `CON ` (saves are console-signed; `PIRS`/`LIVE` also exist) |
| 0x340 | u32 **BE** header size (0x971A in both oracles → first STFS hash table at 0xA000) |
| **0x360** | title ID, 4 raw bytes — `45 41 08 22` = **45410822 = NFS ProStreet** |
| 0x37E | u24 LE file-table block (per Python spec, not needed here) |
| 0x1691 | display name, UTF-16BE, NUL-padded (reads "NFS ProStreet") |

Title-ID cross-check: `45410822` is listed for NFS ProStreet on Xbox 360
title-ID lists (se7ensins game-ID thread, iso2god lists). A second regional
ProStreet ID (`45418827`) exists; our filter defaults to the oracle-verified
`45410822` and accepts a configurable extra set.

## 8. Windows raw-drive access

`\\.\PhysicalDriveN` opened with `CreateFileW(GENERIC_READ,
FILE_SHARE_READ|FILE_SHARE_WRITE, OPEN_EXISTING)`:

* Without elevation: handle open on a *system* disk fails with
  `ERROR_ACCESS_DENIED (5)` — a read-only open still requires admin for raw
  physical drives on modern Windows (with rare exceptions, e.g. some
  removable-bit devices where a handle is granted if no volume is mounted).
  The crate exposes `device::WindowsPhysicalDrives::probe()` so the app can
  detect this at runtime. **Measured on the development machine (2026-10-05,
  unelevated `cargo test -p fatx --test device_probe`)**: PhysicalDrive0–3 →
  `AccessDenied` (drives exist), PhysicalDrive4–15 → `NotFound`. App-level
  decision (2026-10-05): the converter exe's manifest is `asInvoker` — the
  headless `--convert` mode and unelevated GUI launches must not demand UAC —
  and the GUI surfaces an access-denied scan note asking the user to relaunch
  elevated (`nfspc-converter/src/app/drivescan.rs`).
* Length: `IOCTL_DISK_GET_LENGTH_INFO` (0x0007405C) — same approach as the
  reference tools; fallback `SetFilePointerEx(End)`.
* Note: the `windows` crate surfaces kernel32 failures as HRESULTs
  (`0x8007####`); `device::imp` unwraps FACILITY_WIN32 to the plain Win32
  code so `ERROR_ACCESS_DENIED` maps to `OpenStatus::AccessDenied`.

## 9. Sources

* free60 FATX wiki page: https://www.free60.org/FATX (mirror used for
  diffing: https://github.com/gligli/Free60-Wiki/blob/MoreFixes/FATX.md,
  https://github.com/ralf1307/Free60-Wiki)
* xboxdevwiki FATX stub: https://xboxdevwiki.net/FATX
* FATXTools (C#): https://github.com/aerosoul94/FATXTools — `Volume.cs`,
  `DirectoryEntry.cs`, `Constants.cs`
* xbox-winfsp: https://github.com/emoose/xbox-winfsp — `FatxFileSystem.cs`,
  `FatxDevice.cs`
* Xbox360_USB_Explorer (reForge): https://github.com/dpteam/Xbox360_USB_Explorer —
  `Program.cs` (raw `\\.\PhysicalDrive` handling, offsets, dirent format)
* USB XTAF Explorer (decompiled): https://github.com/barrenechea/usb-xtaf-explorer —
  `XtafRewrite/Xtaf.cs` (superblock/FAT math, delete semantics)
* Party Buffalo Drive Explorer: https://github.com/landaire/party-buffalo —
  `CLKsFATXLib/FATX/Drive.cs`, `Geometry.cs` (STFSOffsets, USB/HDD offsets,
  devkit table)
* fatx360fs (Rust): https://github.com/henriqueclaranhan/fatx360fs —
  `src/filesystem.rs` (FAT size rounding)
* lornix/xboxfs-tools: https://github.com/lornix/xboxfs-tools — `xboxfs.h`
  (XTAF BE magic, USB Data0000-N file view)
* Tracked oracles: `docs/re/c1_latest/CAREER_01_360`,
  `docs/re/pair/CAREER_02_360_fresh` (CON header offsets, byte sizes)
* Python spec: `scripts/python/nfssave/container360.py` (STFS header size at
  0x340 BE, hash-table interleaving — the verified in-game converter)
