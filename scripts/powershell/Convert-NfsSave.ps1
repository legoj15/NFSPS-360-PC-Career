<#
.SYNOPSIS
  Convert Xbox 360 NFS ProStreet saves (STFS CON) to PC saves. Windows PowerShell 5.1 / pwsh 7.

.DESCRIPTION
  Byte-exact port of the Python converter (scripts/python/convert.py and the
  scripts/python/nfssave package, which are the specification):
    container360.py  STFS reader        mc02.py      MC02 header/CRCs
    crc.py           EA CRC-32          tree.py      chunk tree parse/build
    payload_rules.py BE->LE engines     convert.py   struct fixes + pipeline
    treehash.py      chained MD5 + modpow tree hash
  The whole binary transform lives in one inline C# class (NfsPs.Save),
  because byte loops are far too slow in PowerShell 5.1. This script keeps the
  orchestration: CLI, file walking, backups, reporting, and the tree hash
  (BigInteger, so the C# needs no edition-specific assembly reference).
  Per-chunk rules come from fieldmaps.rules next to this script (generated from
  fieldmaps_parsed.json by nfssave.payload_rules.write_flat_rules). Missing
  file -> every chunk converts in auto mode, with a warning.

  Inputs (positional): files and/or folders. A file is converted as given. A
  folder is walked recursively; CAREER_* / ALIAS_* files whose first 4 bytes
  are "CON " are picked (anything under a "SaveConverter backups" folder is
  skipped). -Usb <drive-or-folder> (alias -Flash) takes the saves from a console
  USB stick's Content\*\*\0000000[12]\ folders.

  Output root R = -OutRoot, else the CURRENT DIRECTORY (created if missing,
  not on -DryRun). The save folder S is R\SAVE\NFS ProStreet if it exists, else
  R\NFS ProStreet if it exists, else R itself when R is named "NFS ProStreet"
  (game mode), else R (plain mode). Writes S\<NAME>\<NAME>, NAME = the STFS
  file-table name. An existing target is copied first to
  <B>\SaveConverter backups\<UTC stamp>\<NAME>\<NAME>, B = the parent of S in
  game mode and R itself in plain mode.

.EXAMPLE
  .\Convert-NfsSave.ps1 Extracted\Career\CAREER_01 -OutRoot D:\out
  .\Convert-NfsSave.ps1 C:\saves\from-xbox                (folder, output in the current directory)
  .\Convert-NfsSave.ps1 -Usb F: -DryRun

.NOTES
  Exit codes: 0 all ok, 1 a source failed (the rest still convert), 2 usage error.
#>
[CmdletBinding()]
param(
    [Parameter(Position = 0, ValueFromRemainingArguments = $true)]
    [string[]]$Source,
    [string]$OutRoot,
    [Alias('Flash')]
    [string]$Usb,
    [switch]$DryRun
)

Set-StrictMode -Version 2
$ErrorActionPreference = 'Stop'

$csharp = @'
using System;
using System.Collections.Generic;
using System.Security.Cryptography;
using System.Text;

namespace NfsPs
{
    public sealed class Rule { public int RefSize; public byte[] Cls; }   // Cls[word]: 0 swap, 1 copy, 2 diff/zero
    public sealed class Container { public string Name; public byte[] Payload; }
    public sealed class Rec { public uint Type; public uint Id; public byte[] Payload; public byte[] Tail = new byte[0]; }
    public sealed class SaveResult
    {
        public string Kind;
        public int Records;
        public List<string> Chunks = new List<string>();
        public List<string> Warnings = new List<string>();
        public byte[] Extra;
        public byte[] Tree;     // tree hash (bytes 0..15) still unpatched
        public int TreeSize;
    }

    sealed class Mc02
    {
        public bool Big;
        public byte[] Extra;
        public byte[] Tree;
        public int TreeSize;
        public uint CrcExtra, CrcTree, CrcHdr;
    }

    sealed class Tree
    {
        public byte[] Noise;
        public uint Count;
        public byte[] Pre;
        public List<Rec> Records;
        public byte[] Post;
        public long Used;
        public long Gap;
        // index in Records of the first record re-anchored after the noise
        // (internal gap); -1 when there is no gap or it is trailing
        public long GapAt = -1;
    }

    public static class Save
    {
        const int Block = 0x1000;
        const uint TreeMagic = 0x59F2D89B;
        const int RecStart360 = 0x48;
        const int RecStartPc = 0x1CC;
        const int ReanchorMaxGap = 0x4000;
        const int HeaderSize = 0x1C;
        const uint GameplayId = 0x3B309E09;
        const int GameplayPcSize = 0x10014;
        const uint CarDbId = 0x47A07113;
        // RaceData: u32/float only; the fieldmap's empty slots fell back to the string
        // heuristic and left race times big-endian (PC race HUD lost its speedometer)
        const uint RaceDataId = 0x51A41B14;
        const int PcHeadStructSize = 0x1AC;

        static readonly Dictionary<uint, string> ChunkNames = MakeNames();
        static Dictionary<uint, string> MakeNames()
        {
            var d = new Dictionary<uint, string>();
            d[0x59F2D89B] = "MEMCARD_ROOT";
            d[0x8FFBE3E8] = "GStatsImpl::SavableStats";
            d[0x4E8AA143] = "AchievementManager";
            d[0x9F72F194] = "OnlineUserProfile";
            d[0xDC6B027F] = "ProfileStats";
            d[0x6CC89C57] = "Jukebox";
            d[0xC3EC4947] = "VideoSettings";
            d[0xB74C1044] = "ForceFeedbackSettings";
            d[0x8C4A2DC0] = "GameplaySettings";
            d[0x9CB326C2] = "AudioSettings";
            d[0x8B7D0AAD] = "PlayerSettings0";
            d[0x8B7D0AAE] = "PlayerSettings1";
            d[0x8B7D0AAF] = "PlayerSettings2";
            d[0x8B7D0AB0] = "PlayerSettings3";
            d[0x322ED42F] = "UserProfile";
            d[0x328C6431] = "SPEECH DATA";
            d[0xB67F6CC6] = "MarkerSystem";
            d[0x3B309E09] = "GameplayData";
            d[0x1FB48CF2] = "GameplayBinarySavableHelper";
            d[0x51A41B14] = "RaceData";
            d[0x47A07113] = "FEPlayerCarDB";
            d[0x34B74942] = "VehicleBinarySavableHelper";
            d[0x885B4DDC] = "FECareer";
            d[0x39156567] = "PCControllerSettings";
            d[0xD548266C] = "CustomRaceDayMemcard";
            d[0xCA269650] = "UnlockSystem";
            return d;
        }

        static string Hex(long v) { return "0x" + v.ToString("x"); }

        // ---- byte helpers ---------------------------------------------------
        static uint Rd32(byte[] b, long o, bool big)
        {
            int i = (int)o;
            if (big) return ((uint)b[i] << 24) | ((uint)b[i + 1] << 16) | ((uint)b[i + 2] << 8) | b[i + 3];
            return ((uint)b[i + 3] << 24) | ((uint)b[i + 2] << 16) | ((uint)b[i + 1] << 8) | b[i];
        }
        static void Wr32(byte[] b, long o, uint v, bool big)
        {
            int i = (int)o;
            if (big) { b[i] = (byte)(v >> 24); b[i + 1] = (byte)(v >> 16); b[i + 2] = (byte)(v >> 8); b[i + 3] = (byte)v; }
            else { b[i + 3] = (byte)(v >> 24); b[i + 2] = (byte)(v >> 16); b[i + 1] = (byte)(v >> 8); b[i] = (byte)v; }
        }
        static void Swap4(byte[] b, int o)
        {
            byte t = b[o]; b[o] = b[o + 3]; b[o + 3] = t;
            t = b[o + 1]; b[o + 1] = b[o + 2]; b[o + 2] = t;
        }
        static void Swap2(byte[] b, int o)
        {
            byte t = b[o]; b[o] = b[o + 1]; b[o + 1] = t;
        }
        static void Copy(byte[] src, int so, byte[] dst, int d, int n) { Buffer.BlockCopy(src, so, dst, d, n); }
        static byte[] Slice(byte[] b, long from, long to)
        {
            if (to > b.Length) to = b.Length;
            if (from > to) from = to;
            byte[] r = new byte[to - from];
            Buffer.BlockCopy(b, (int)from, r, 0, r.Length);
            return r;
        }

        // ---- crc.py ---------------------------------------------------------
        static readonly uint[] CrcTbl = MakeCrcTable();
        static uint[] MakeCrcTable()
        {
            uint[] t = new uint[256];
            for (int i = 0; i < 256; i++)
            {
                uint c = (uint)i << 24;
                for (int k = 0; k < 8; k++) c = (c & 0x80000000u) != 0 ? (c << 1) ^ 0x04C11DB7u : (c << 1);
                t[i] = c;
            }
            return t;
        }
        public static uint Crc(byte[] d, int off, int len)
        {
            if (len < 4) return 0;
            uint crc = ((uint)d[off] << 24) | ((uint)d[off + 1] << 16) | ((uint)d[off + 2] << 8) | d[off + 3];
            crc ^= 0xFFFFFFFFu;
            int end = off + len;
            for (int i = off + 4; i < end; i++) crc = ((crc << 8) | d[i]) ^ CrcTbl[crc >> 24];
            return crc ^ 0xFFFFFFFFu;
        }
        static uint Crc(byte[] d) { return Crc(d, 0, d.Length); }

        // ---- container360.py ------------------------------------------------
        static long StfsBlockOffset(long block, long firstTable, int shift)
        {
            long backing = (((block + 0xAA) / 0xAA) << shift) + block;
            if (block >= 0xAA)
            {
                backing += ((block + 0x70E4) / 0x70E4) << shift;
                if (block >= 0x70E4) backing += 1L << shift;
            }
            return firstTable + backing * Block;
        }

        static byte[] ReadBlock(byte[] data, long n, long first, int shift, string label)
        {
            long off = StfsBlockOffset(n, first, shift);
            if (off >= data.Length)
                throw new InvalidOperationException(label + ": data block " + n + " at " + Hex(off) + " beyond file end");
            return Slice(data, off, off + Block);
        }

        public static Container ParseContainer(byte[] data, string label)
        {
            if (data.Length < 4 || data[0] != (byte)'C' || data[1] != (byte)'O' || data[2] != (byte)'N' || data[3] != (byte)' ')
                throw new InvalidOperationException(label + ": not a CON container (magic "
                    + BitConverter.ToString(Slice(data, 0, 4)).Replace('-', ' ') + ")");
            if (data.Length < 0x381)
                throw new InvalidOperationException(label + ": truncated CON header (" + Hex(data.Length) + " B)");
            long headerSize = Rd32(data, 0x340, true);
            long first = (headerSize + Block - 1) & ~(long)(Block - 1);
            int shift = (data[0x37B] & 1) != 0 ? 0 : 1;
            long tableBlock = data[0x37E] | ((long)data[0x37F] << 8) | ((long)data[0x380] << 16);
            byte[] entry = ReadBlock(data, tableBlock, first, shift, label);
            if (entry.Length < 0x40) throw new InvalidOperationException(label + ": short STFS file table");
            int nul = Array.IndexOf(entry, (byte)0, 0, 0x28);
            int nameLen = nul != -1 ? nul : 0x28;
            StringBuilder sb = new StringBuilder();
            for (int i = 0; i < nameLen; i++) sb.Append(entry[i] < 0x80 ? (char)entry[i] : '\uFFFD');
            string name = sb.ToString();
            if (name.Length == 0) throw new InvalidOperationException(label + ": empty STFS file table");
            int flags = entry[0x28];
            long nBlocks = entry[0x29] | ((long)entry[0x2A] << 8) | ((long)entry[0x2B] << 16);
            long start = entry[0x2F] | ((long)entry[0x30] << 8) | ((long)entry[0x31] << 16);
            long size = Rd32(entry, 0x34, true);
            if ((flags & 0x40) == 0)
                throw new InvalidOperationException(label + ": non-contiguous STFS file '" + name + "' is not supported");
            if (nBlocks * Block < size)
                throw new InvalidOperationException(label + ": '" + name + "' size " + Hex(size) + " exceeds its " + nBlocks + " blocks");
            System.IO.MemoryStream ms = new System.IO.MemoryStream();
            for (long i = 0; i < nBlocks; i++)
            {
                byte[] b = ReadBlock(data, start + i, first, shift, label);
                ms.Write(b, 0, b.Length);
            }
            byte[] all = ms.ToArray();
            if (all.Length < size)
                throw new InvalidOperationException(label + ": '" + name + "' truncated (" + Hex(all.Length) + " of " + Hex(size) + " B)");
            Container c = new Container();
            c.Name = name;
            c.Payload = Slice(all, 0, size);
            return c;
        }

        // ---- mc02.py --------------------------------------------------------
        static Mc02 ParseMc02(byte[] data)
        {
            if (data.Length < HeaderSize) throw new InvalidOperationException("MC02: truncated header");
            bool big;
            uint magicLe = Rd32(data, 0, false);
            if (magicLe == 0x4D433032) big = false;
            else if (Rd32(data, 0, true) == 0x4D433032) big = true;
            else throw new InvalidOperationException("MC02: bad magic " + Hex(magicLe));
            uint total = Rd32(data, 4, big);
            uint extraSize = Rd32(data, 8, big);
            uint treeSize = Rd32(data, 12, big);
            if (total != data.Length)
                throw new InvalidOperationException("MC02: size field " + Hex(total) + " != file size " + Hex(data.Length));
            Mc02 m = new Mc02();
            m.Big = big;
            m.Extra = Slice(data, HeaderSize, HeaderSize + (long)extraSize);
            m.Tree = Slice(data, HeaderSize + (long)extraSize, data.Length);
            m.TreeSize = (int)treeSize;
            m.CrcExtra = Rd32(data, 0x10, big);
            m.CrcTree = Rd32(data, 0x14, big);
            m.CrcHdr = Rd32(data, 0x18, big);
            return m;
        }

        static byte[] PadTree(byte[] tree, int treeSize)
        {
            if (tree.Length >= treeSize) return tree;
            byte[] t = new byte[treeSize];
            Buffer.BlockCopy(tree, 0, t, 0, tree.Length);
            return t;
        }

        // Mc02.header_bytes: 0x1C-byte header with fresh CRCs (tree zero-padded to its declared size).
        static byte[] HeaderBytes(bool big, byte[] extra, byte[] tree, int treeSize)
        {
            tree = PadTree(tree, treeSize);
            byte[] h = new byte[HeaderSize];
            Wr32(h, 0, 0x4D433032, big);
            Wr32(h, 4, (uint)(HeaderSize + extra.Length + tree.Length), big);
            Wr32(h, 8, (uint)extra.Length, big);
            Wr32(h, 12, (uint)tree.Length, big);
            Wr32(h, 0x10, Crc(extra), big);
            Wr32(h, 0x14, Crc(tree), big);
            Wr32(h, 0x18, Crc(h, 0, 0x18), big);
            return h;
        }

        // Mc02.check on raw file bytes; throws like Mc02.parse on a malformed header.
        public static List<string> CheckMc02(byte[] data)
        {
            Mc02 m = ParseMc02(data);
            List<string> probs = new List<string>();
            byte[] h = HeaderBytes(m.Big, m.Extra, m.Tree, m.TreeSize);
            if (m.CrcHdr != Crc(h, 0, 0x18)) probs.Add("header CRC mismatch");
            if (m.CrcExtra != Crc(m.Extra)) probs.Add("extra CRC mismatch");
            if (m.CrcTree != Crc(m.Tree)) probs.Add("tree CRC mismatch");
            return probs;
        }

        // Mc02.to_bytes for a little-endian PC save.
        public static byte[] BuildMc02(byte[] extra, byte[] tree, int treeSize)
        {
            if (tree.Length > treeSize)
                throw new InvalidOperationException("tree data (" + Hex(tree.Length) + " B) exceeds declared buffer "
                    + Hex(treeSize) + " - refusing to truncate");
            byte[] t = PadTree(tree, treeSize);
            byte[] h = HeaderBytes(false, extra, t, treeSize);
            byte[] o = new byte[h.Length + extra.Length + t.Length];
            Buffer.BlockCopy(h, 0, o, 0, h.Length);
            Buffer.BlockCopy(extra, 0, o, h.Length, extra.Length);
            Buffer.BlockCopy(t, 0, o, h.Length + extra.Length, t.Length);
            return o;
        }

        // ---- tree.py --------------------------------------------------------
        static Rec ReadRec(byte[] tree, long off, bool big, out long size)
        {
            uint t, i, s;
            if (big) { t = Rd32(tree, off, true); i = Rd32(tree, off + 4, true); s = Rd32(tree, off + 8, true); }
            else { i = Rd32(tree, off, false); s = Rd32(tree, off + 4, false); t = Rd32(tree, off + 8, false); }
            size = s;
            Rec r = new Rec();
            r.Type = t; r.Id = i;
            r.Payload = null;
            if (s == 0 && i == 0 && t == 0) size = -1;
            return r;
        }

        static Tree ParseTree(byte[] tree, bool big)
        {
            uint count = Rd32(tree, 0x10, big);
            int recStart = big ? RecStart360 : RecStartPc;
            int magicOff = -1;
            for (int off = 0x14; off < recStart - 4; off += 4)
                if (Rd32(tree, off, big) == TreeMagic) { magicOff = off; break; }
            if (magicOff < 0) throw new InvalidOperationException("tree magic 0x59F2D89B not found");
            long used = Rd32(tree, magicOff + 4, big);
            if (used > tree.Length - recStart)
                throw new InvalidOperationException("corrupt used size " + Hex(used) + " exceeds tree buffer");
            List<Rec> records = new List<Rec>();
            long o2 = recStart;
            long end = recStart + used;
            long stopped = -1;
            long lim = Math.Min(end, (long)tree.Length);
            while (o2 + 12 <= lim)
            {
                long s;
                Rec r = ReadRec(tree, o2, big, out s);
                if (s < 0) s = 0;
                bool zero = r.Type == 0 && r.Id == 0 && s == 0;
                if (o2 + 12 + s > end || zero) { stopped = o2; break; }
                r.Payload = Slice(tree, o2 + 12, o2 + 12 + s);
                r.Tail = TailAfter(tree, o2 + 12 + s, big);
                records.Add(r);
                o2 += 12 + s;
            }
            if (stopped < 0) stopped = o2;
            long gap = Math.Max(0, end - stopped);
            long gapAt = -1;
            if (gap > 0)
            {
                if (records.Count > 0) records[records.Count - 1].Tail = new byte[0];   // noise, not a value
                List<Rec> after = ReAfterGap(tree, stopped, end, big);
                if (after.Count > 0) gapAt = records.Count;
                records.AddRange(after);
            }
            Tree t = new Tree();
            t.Noise = Slice(tree, 0, 0x10);
            t.Count = count;
            t.Pre = Slice(tree, 0x14, recStart);
            t.Records = records;
            t.Post = Slice(tree, end, tree.Length);
            t.Used = used;
            t.Gap = gap;
            t.GapAt = gapAt;
            return t;
        }

        // Record.tail (360 only): the word after the payload = the last node's value
        static byte[] TailAfter(byte[] tree, long at, bool big)
        {
            if (!big) return new byte[0];
            long a = Math.Min(at, (long)tree.Length);
            return Slice(tree, a, Math.Min(a + 4, (long)tree.Length));
        }

        // Tree._reafter_gap: internal-gap recovery.
        static List<Rec> ReAfterGap(byte[] tree, long stopped, long end, bool big)
        {
            List<Rec> none = new List<Rec>();
            if (end - stopped > ReanchorMaxGap) return none;
            for (long cand = (stopped + 4 + 3) & ~3L; cand < end - 11; cand += 4)
            {
                long off = cand;
                List<Rec> recs = new List<Rec>();
                bool ok = true;
                while (off + 12 <= end)
                {
                    long s;
                    Rec r = ReadRec(tree, off, big, out s);
                    if (s < 0) s = 0;
                    bool zero = r.Type == 0 && r.Id == 0 && s == 0;
                    if (off + 12 + s > end || zero) { ok = false; break; }
                    r.Payload = Slice(tree, off + 12, off + 12 + s);
                    r.Tail = TailAfter(tree, off + 12 + s, big);
                    recs.Add(r);
                    off += 12 + s;
                }
                if (ok && off == end) return recs;
            }
            return none;
        }

        // Tree.build for the little-endian PC layout.
        static byte[] BuildTree(Tree t, int treeSize)
        {
            long used = 0;
            foreach (Rec r in t.Records) used += 12 + r.Payload.Length;
            byte[] pre = (byte[])t.Pre.Clone();
            int magicOff = -1;
            for (int off = 0; off < pre.Length - 4; off += 4)
                if (Rd32(pre, off, false) == TreeMagic) { magicOff = off; break; }
            if (magicOff < 0) throw new InvalidOperationException("tree magic lost in pre_records");
            Wr32(pre, magicOff + 4, (uint)used, false);
            long headLen = 0x10 + 4 + pre.Length + used;
            long room = treeSize - headLen;
            if (room < 0) throw new InvalidOperationException("tree overflow: records end at " + Hex(headLen) + " > " + Hex(treeSize));
            byte[] o = new byte[treeSize];
            Buffer.BlockCopy(t.Noise, 0, o, 0, 0x10);
            Wr32(o, 0x10, t.Count, false);
            Buffer.BlockCopy(pre, 0, o, 0x14, pre.Length);
            int p = 0x14 + pre.Length;
            foreach (Rec r in t.Records)
            {
                Wr32(o, p, r.Id, false);
                Wr32(o, p + 4, (uint)r.Payload.Length, false);
                Wr32(o, p + 8, r.Type, false);
                Buffer.BlockCopy(r.Payload, 0, o, p + 12, r.Payload.Length);
                p += 12 + r.Payload.Length;
            }
            // the post-records directory is variable length: keep as much as fits
            long n = Math.Min(room, (long)t.Post.Length);
            Buffer.BlockCopy(t.Post, 0, o, p, (int)n);
            return o;
        }

        // ---- payload_rules.py ----------------------------------------------
        static bool Printable(int b) { return b >= 0x20 && b < 0x7F; }

        // _ea_string_ranges (min_len 5): [0x40|len][len-1 printable chars]
        static void EaStringRanges(byte[] p, List<int[]> runs)
        {
            int i = 0, n = p.Length;
            while (i < n)
            {
                int b = p[i];
                if (0x45 <= b && b <= 0x7F)
                {
                    int ln = b & 0x3F;
                    if (ln >= 5 && i + ln <= n)
                    {
                        bool all = true;
                        for (int k = i + 1; k < i + ln; k++) if (!Printable(p[k])) { all = false; break; }
                        if (all) { runs.Add(new int[] { i, i + ln }); i += ln; continue; }
                    }
                }
                i++;
            }
        }

        // _fixed_string_ranges: printable text in a NUL/0xAA padded slot
        static void FixedStringRanges(byte[] p, List<int[]> runs)
        {
            int i = 0, n = p.Length;
            while (i < n)
            {
                int j = i;
                while (j < n && Printable(p[j])) j++;
                if (j - i >= 5)
                {
                    int k = j, lastTextEnd = j;
                    while (k < n)
                    {
                        int c = p[k];
                        if (c == 0x00 || c == 0xAA) k++;
                        else if (Printable(c))
                        {
                            int m2 = k;
                            while (m2 < n && Printable(p[m2])) m2++;
                            if (m2 - k >= 4) { k = m2; lastTextEnd = m2; }
                            else break;
                        }
                        else break;
                    }
                    int fillAfter = k - lastTextEnd;
                    if (k >= n || fillAfter >= 2) { runs.Add(new int[] { i, k }); i = k; continue; }
                }
                i = j > i ? j + 1 : i + 1;
            }
        }

        static int CompareRuns(int[] a, int[] b)
        {
            if (a[0] != b[0]) return a[0].CompareTo(b[0]);
            return a[1].CompareTo(b[1]);
        }

        static List<int[]> Merge(List<int[]> ranges)
        {
            List<int[]> sorted = new List<int[]>(ranges);
            sorted.Sort(CompareRuns);
            List<int[]> merged = new List<int[]>();
            foreach (int[] r in sorted)
            {
                if (merged.Count > 0 && r[0] <= merged[merged.Count - 1][1])
                    merged[merged.Count - 1][1] = Math.Max(merged[merged.Count - 1][1], r[1]);
                else merged.Add(new int[] { r[0], r[1] });
            }
            return merged;
        }

        // _quantize: snap to the u32 grid, re-merge
        static List<int[]> Quantize(List<int[]> ranges)
        {
            List<int[]> q = new List<int[]>();
            foreach (int[] r in ranges) q.Add(new int[] { r[0] & ~3, (r[1] + 3) & ~3 });
            return Merge(q);
        }

        static List<int[]> RawStringRuns(byte[] p)
        {
            List<int[]> raw = new List<int[]>();
            EaStringRanges(p, raw);
            FixedStringRanges(p, raw);
            return raw;
        }

        static List<int[]> FindStringRuns(byte[] p) { return Quantize(RawStringRuns(p)); }

        // _subword_garbage: [value byte][FF FF FF], kept natural
        static bool SubwordGarbage(byte[] p, int o)
        {
            return p[o] != 0xFF && p[o + 1] == 0xFF && p[o + 2] == 0xFF && p[o + 3] == 0xFF;
        }

        // convert_payload_auto
        static byte[] ConvertAuto(byte[] p, List<string> warnings, string label)
        {
            List<int[]> raw = RawStringRuns(p);
            if (warnings != null)
            {
                int nUn = 0;
                foreach (int[] r in raw) if ((r[0] % 4) != 0 || (r[1] % 4) != 0) nUn++;
                if (nUn > 0)
                    warnings.Add(label + ": " + nUn + " string run(s) not word-aligned; "
                        + "padded to word grid (leading bytes are format padding)");
            }
            int n = p.Length;
            byte[] o = (byte[])p.Clone();
            for (int i = 0; i + 4 <= n; i += 4) Swap4(o, i);
            foreach (int[] r in Quantize(raw))
            {
                int e = Math.Min(r[1], n);
                if (r[0] < e) Copy(p, r[0], o, r[0], e - r[0]);
            }
            for (int i = 0; i + 4 <= n; i += 4) if (SubwordGarbage(p, i)) Copy(p, i, o, i, 4);
            return o;
        }

        // convert_payload_mapped: C words copy, D words use the auto grammar, the rest swap
        static byte[] ConvertMapped(byte[] p, byte[] cls)
        {
            int n = p.Length;
            bool[] str = new bool[n / 4 + 2];
            foreach (int[] r in FindStringRuns(p))
                for (int w = r[0]; w < r[1]; w += 4) if ((w >> 2) < str.Length) str[w >> 2] = true;
            byte[] o = (byte[])p.Clone();
            for (int off = 0; off + 4 <= n; off += 4)
            {
                int wi = off >> 2;
                int c = wi < cls.Length ? cls[wi] : 0;
                if (c == 1) continue;
                if (c == 2 && (str[wi] || SubwordGarbage(p, off))) continue;
                Swap4(o, off);
            }
            return o;
        }

        // Parses fieldmaps.rules (see payload_rules.flat_rules_text).
        public static Dictionary<string, Rule> LoadRules(string text)
        {
            Dictionary<string, Rule> d = new Dictionary<string, Rule>();
            Rule cur = null;
            List<int[]> runs = null;
            foreach (string raw in text.Split('\n'))
            {
                string line = raw.Trim();
                if (line.Length == 0 || line[0] == '#') continue;
                string[] f = line.Split(new char[] { ' ' }, StringSplitOptions.RemoveEmptyEntries);
                if (f[0] == "chunk")
                {
                    Finish(cur, runs);
                    cur = new Rule();
                    cur.RefSize = f[3] == "-" ? -1 : Convert.ToInt32(f[3], 16);
                    runs = new List<int[]>();
                    d[f[1] + ":" + Convert.ToUInt32(f[2], 16).ToString("X8")] = cur;
                }
                else
                {
                    if (cur == null || f.Length != 3) throw new FormatException("bad rules line: " + line);
                    runs.Add(new int[] { Convert.ToInt32(f[0], 16), Convert.ToInt32(f[1], 16), f[2] == "C" ? 1 : 2 });
                }
            }
            Finish(cur, runs);
            return d;
        }
        static void Finish(Rule r, List<int[]> runs)
        {
            if (r == null) return;
            int max = 0;
            foreach (int[] x in runs) max = Math.Max(max, x[1]);
            r.Cls = new byte[(max + 3) / 4];
            foreach (int[] x in runs)
                for (int o = x[0]; o < x[1]; o += 4) r.Cls[o >> 2] = (byte)x[2];
        }

        // convert_record: returns the mode used ("auto" or "mapped")
        static string ConvertRecord(string kind, Rec rec, List<string> warnings, Dictionary<string, Rule> rules)
        {
            string label = "chunk " + Hex(rec.Id);
            Rule rule;
            if (rules != null && rules.TryGetValue(kind + ":" + rec.Id.ToString("X8"), out rule))
            {
                if (rule.RefSize >= 0 && rule.RefSize != rec.Payload.Length)
                {
                    warnings.Add(label + " size " + Hex(rec.Payload.Length) + " != map reference " + Hex(rule.RefSize)
                        + "; converted in auto mode (positional rules unsafe)");
                    rec.Payload = ConvertAuto(rec.Payload, warnings, label);
                    return "auto";
                }
                rec.Payload = ConvertMapped(rec.Payload, rule.Cls);
                return "mapped";
            }
            rec.Payload = ConvertAuto(rec.Payload, warnings, label);
            return "auto";
        }

        // ---- convert.py -----------------------------------------------------
        static byte[] SwapU32s(byte[] p)
        {
            if (p.Length % 4 != 0) throw new InvalidOperationException("payload is not u32-tiled");
            byte[] o = (byte[])p.Clone();
            for (int i = 0; i < o.Length; i += 4) Swap4(o, i);
            return o;
        }

        // normalize_gameplay: trim the 360-only zero tail (BE helper size word rewritten)
        static void NormalizeGameplay(Rec rec, List<string> warnings)
        {
            byte[] p = rec.Payload;
            if (rec.Id != GameplayId || p.Length <= GameplayPcSize) return;
            for (int i = GameplayPcSize; i < p.Length; i++)
                if (p[i] != 0)
                {
                    warnings.Add("GameplayData content (" + Hex(p.Length) + " B) exceeds the PC buffer ("
                        + Hex(GameplayPcSize) + " B); converting untrimmed - the game may reject the chunk");
                    return;
                }
            byte[] t = Slice(p, 0, GameplayPcSize);
            Wr32(t, 0x0C, (uint)(GameplayPcSize - 0x10), true);
            rec.Payload = t;
        }

        // convert_extra: MC02 extra preamble, 360 -> PC
        static byte[] ConvertExtra(byte[] x)
        {
            int n = x.Length;
            byte[] o = (byte[])x.Clone();
            if (n == 28)
            {
                for (int i = 0; i < n; i += 4) Swap4(o, i);
                return o;
            }
            if (n != 64) throw new InvalidOperationException("unexpected extra size " + n);
            for (int i = 0; i < 0x14; i += 4) Swap4(o, i);
            int nul = Array.IndexOf(x, (byte)0, 0x14, n - 0x14);
            int end;
            if (nul != -1) end = nul + 1;
            else
            {
                end = 0x14;
                while (end < n && x[end] != 0x00 && x[end] != 0xAA && x[end] >= 0x20 && x[end] < 0x7F) end++;
                if (end < n && x[end] == 0x00) end++;
            }
            while (end + 4 <= n && x[end] == 0xAA && x[end + 1] == 0xAA && x[end + 2] == 0xAA && x[end + 3] == 0xAA) end += 4;
            end = (end + 3) & ~3;
            for (int i = end; i + 4 <= n; i += 4) Swap4(o, i);
            return o;
        }

        // fix_node_flags: flag word after each [0][len] pair stays natural; the
        // [0][len] header itself always swaps (the string heuristic can swallow it:
        // a NUL-padded name node runs into the next header, leaving len big-endian,
        // and the PC drops that node and everything after it - custom race days lost
        // their event lists, Race Day menu crash at nfs.exe 0x7F6480)
        static void FixNodeFlags(byte[] src, byte[] o)
        {
            // data offsets of the one-byte nodes on the property chain (NodeSpans walk):
            // their [0][len] headers are real, unlike matches inside numeric data
            HashSet<int> chain = new HashSet<int>();
            foreach (int[] sp in NodeSpans(src, 4)) if (sp[1] == 1) chain.Add(sp[0]);
            for (int p = 8; p < src.Length - 3; p += 4)
            {
                if (chain.Contains(p - 4)) continue;   // [flag 0][u8 node data 00 00 00 08] is not a header
                uint zero = Rd32(src, p - 8, true), ln = Rd32(src, p - 4, true);
                if (zero == 0 && ln >= 1 && ln <= 0x400)
                {
                    for (int i = 0; i < 4; i++) o[p - 4 + i] = src[p - 1 - i];
                    Copy(src, p, o, p, 4);
                    // one-byte node [u8][3 pad]: the value is the first byte (a u32 swap reads 0
                    // on PC). The 360 pad/flag bytes can hold heap junk (01 00 13 10), so a node
                    // on the chain keeps its first byte whatever the pad; off the chain a [0][1]
                    // match may be numeric data, so there the pad must be zero and the flag valid.
                    if (ln == 1 && chain.Contains(p + 4) && p + 8 <= src.Length)
                        Copy(src, p + 4, o, p + 4, 4);
                    else if (ln == 1 && p + 8 <= src.Length && src[p + 5] == 0 && src[p + 6] == 0 && src[p + 7] == 0
                        && IsNodeFlag(src, p))
                        Copy(src, p + 4, o, p + 4, 4);
                }
            }
        }

        const uint CustomRacedayId = 0xD548266C;

        // node_spans: (offset, length) of each node's data in a 360 property-node
        // stream. The first value sits at start (after the 360 marker word); every
        // later node is [u32 0][u32 len][flag word][data], its header on the u32 grid
        // after the previous data (junk bytes may sit in between).
        static List<int[]> NodeSpans(byte[] src, int start)
        {
            List<int[]> spans = new List<int[]>();
            spans.Add(new int[] { start, 4 });
            int o = start + 4;
            while (o + 12 <= src.Length)
            {
                int h = (o + 3) & ~3;
                bool found = false;
                while (h + 8 <= src.Length)
                {
                    uint l = Rd32(src, h + 4, true);
                    if (Rd32(src, h, true) == 0 && l > 0 && l <= 0x400) { found = true; break; }
                    h += 4;
                }
                if (!found) break;
                int ln = (int)Rd32(src, h + 4, true);
                int d = h + 12;
                if (d + ln > src.Length) break;
                spans.Add(new int[] { d, ln });
                o = d + ln;
            }
            return spans;
        }

        // fix_custom_raceday_strings: CustomRaceDayMemcard nodes are u32 values
        // (swapped by the generic pass) or strings: GUID (25 B) and name (36 B), each
        // [4 junk][chars, NUL]. String nodes copy byte for byte; the generic string
        // heuristic misses GUIDs followed by a junk byte and half-swapped them.
        static void FixCustomRacedayStrings(byte[] src, byte[] o)
        {
            foreach (int[] sp in NodeSpans(src, 4))
                if (sp[1] > 4) Copy(src, sp[0], o, sp[0], sp[1]);
        }

        // fix_career_name: FECareer's one 36-byte node is the career-slot name
        // [4 junk][32 chars, NUL]; the PC names the save file after it. The 360 pads
        // "01\0" with 0xAA fill, so the u32 swap saved every converted career as
        // CAREER_<0xAA>. Copy the chars naturally and zero after the NUL.
        const uint FeCareerId = 0x885B4DDC;
        const int CareerNameLen = 0x24;
        static void FixCareerName(byte[] src, byte[] o)
        {
            foreach (int[] sp in NodeSpans(src, 4))
            {
                if (sp[1] != CareerNameLen) continue;
                int d = sp[0];
                Copy(src, d, o, d, 4);
                bool nul = false;
                for (int i = d + 4; i < d + CareerNameLen; i++)
                {
                    if (src[i] == 0) nul = true;
                    o[i] = nul ? (byte)0 : src[i];
                }
            }
        }

        // convert_packed_entry / fix_cardb_packed
        static void FixCarDbPacked(byte[] src, byte[] o)
        {
            for (int p = 0x7C980; p < 0x90660; p += 8)
            {
                int e = p + 4;
                if (e + 8 > src.Length) continue;
                if (src[e] == 0x2A && src[e + 1] == 0xAA && src[e + 6] == 0x2A && src[e + 7] == 0xAA)
                {
                    uint w1 = Rd32(src, e, true), w2 = Rd32(src, e + 4, true);
                    w1 = (w1 & 0xC000FFFFu) | (0x3FFFu << 16);
                    w2 = (w2 & 0xFFFE0000u) | 0xFFFFu;
                    Wr32(o, e, w1, false);
                    Wr32(o, e + 4, w2, false);
                }
            }
        }

        // Python slice semantics for the struct fixes: o[a:b] = f(src[a:b]) clamps to
        // the payload end (src and o always have the same length), so fixed offsets
        // past a short (corrupt-size) record are partial or empty, never a throw.
        static void CopyNat(byte[] src, byte[] o, int a, int b)
        {
            int hi = Math.Min(b, src.Length);
            if (a < hi) Buffer.BlockCopy(src, a, o, a, hi - a);
        }

        // o[a:b] = swap16s(src[a:b]): u16 pairs from a; a trailing odd byte copies through
        static void Swap16Nat(byte[] src, byte[] o, int a, int b)
        {
            int hi = Math.Min(b, src.Length);
            for (int i = a; i < hi; i += 2)
            {
                if (i + 1 < hi) { o[i] = src[i + 1]; o[i + 1] = src[i]; }
                else o[i] = src[i];
            }
        }

        // convert_decal_entry: all u16s swap except the 4 bytes at +6..+9
        static void ConvertDecal(byte[] src, byte[] o, int at, int step)
        {
            Swap16Nat(src, o, at, at + 6);
            CopyNat(src, o, at + 6, at + 10);
            Swap16Nat(src, o, at + 10, at + step);
        }

        // fix_blueprint_set: one customization set at 360 payload offset s
        static void FixBlueprintSet(byte[] src, byte[] o, int s)
        {
            for (int k = 0; k < 12; k++) { int p = s + 0x194 + k * 12; Swap16Nat(src, o, p, p + 4); }
            for (int k = 0; k < 20; k++) ConvertDecal(src, o, s + 0x240 + k * 26, 26);
            for (int k = 0; k < 20; k++) ConvertDecal(src, o, s + 0x450 + k * 14, 14);
            CopyNat(src, o, s + 0x574, s + 0x628);
        }

        static readonly int[] BlueprintSets = new int[] { 0x0, 0x7B4, 0xF68 };

        // fix_cardb_parts: u16 part slots, u8 flags, blueprint sets and the car table
        static void FixCarDbParts(byte[] src, byte[] o)
        {
            for (int r = 0; r < 80; r++)
            {
                int rec = 0x2680 + r * 0x1870 + 4;
                CopyNat(src, o, rec, rec + 4);
                Swap16Nat(src, o, rec + 4, rec + 8);   // two u16s
                foreach (int bp in BlueprintSets)
                {
                    int rec0 = rec + bp;
                    Swap16Nat(src, o, rec0 + 0x3C, rec0 + 0x186);
                    CopyNat(src, o, rec0 + 0x186, rec0 + 0x190);
                    FixBlueprintSet(src, o, rec0);
                }
            }
            for (int k = 0; k < 410; k++)
            {
                int p = 0x14 + k * 24 + 20 + 4;
                CopyNat(src, o, p, p + 4);
            }
        }

        static uint U32BeAt(byte[] src, int o) { return Rd32(src, o + 4, true); }   // PC offset -> 360 payload

        // raceday_block_end: start of the [u32 0][u32 0x11][hash] list after the block, or -1
        static int RacedayBlockEnd(byte[] src)
        {
            for (int o = 0x2E0 + 0x40; o < src.Length - 16; o += 16)
                if (U32BeAt(src, o) == 0 && U32BeAt(src, o + 4) == 0x11 && U32BeAt(src, o + 8) != 0) return o;
            return -1;
        }

        static bool IsRecordKind(uint w)
        {
            uint mid = (w >> 8) & 0xFF;
            return (w & 0xFF) == 0x10 && (mid == 0x11 || mid == 0x13 || mid == 0x15 || mid == 0x17 || mid == 0x19 || mid == 0x1B)
                && (w >> 24) == 0;
        }

        // fix_raceday_block: GameplayData in-progress race-day block (0x2E0..end)
        static void FixRacedayBlock(byte[] src, byte[] o, List<string> warnings)
        {
            CopyNat(src, o, 0x1F4 + 4, 0x1F4 + 8);
            CopyNat(src, o, 0x2D0 + 4, 0x2D0 + 8);
            if (src.Length < 0x2D4 + 8)
                throw new InvalidOperationException("GameplayData chunk too short (" + Hex(src.Length)
                    + " B) to hold the race-day state - the source file is corrupted");
            bool active = U32BeAt(src, 0x2D4) != 0;
            int end = active ? RacedayBlockEnd(src) : 0x2E0;
            if (end >= 0)
            {
                byte[] tail = Slice(src, end + 4, src.Length);
                foreach (int[] r in FindStringRuns(tail))
                {
                    int e = Math.Min(r[1], tail.Length);
                    if (r[0] < e) Copy(src, end + 4 + r[0], o, end + 4 + r[0], e - r[0]);
                }
            }
            if (!active) return;
            if (end < 0)
            {
                warnings.Add("GameplayData: active race day but block end not found - race day will not resume");
                return;
            }
            CopyNat(src, o, 0x300 + 4, 0x310 + 4);
            bool prevKind = false;
            for (int p = 0x314 + 4; p < end; p += 4)
            {
                int w = p + 4;
                if (prevKind)
                {
                    o[w] = src[w + 1]; o[w + 1] = src[w]; o[w + 2] = src[w + 3]; o[w + 3] = src[w + 2];
                }
                else if (src[w + 1] == 0 && src[w + 2] == 0xAA && src[w + 3] == 0xAA)
                    Copy(src, w, o, w, 4);
                prevKind = IsRecordKind(Rd32(src, w, true));
            }
            int a = 0x314 + 4, e2 = end + 4;
            Buffer.BlockCopy(o, a + 4, o, a, e2 - 4 - a);   // the 360 pad word is dropped
            o[e2 - 4] = 0; o[e2 - 3] = 0; o[e2 - 2] = 0; o[e2 - 1] = 0;
        }

        // Race-day progress table in GameplayData: 90 x [u32 key][u32 state][u32 score],
        // after the race-day block (position varies). Located by its first and last key.
        const uint ProgressFirst = 0xA70EA9B0, ProgressLast = 0xFA5D360A;
        const int ProgressLen = 90;

        // Race days the 360 marks with console-only state (bits 0x04/0x08/0x10 and a
        // score, even in a fresh career) but the PC never plays: every PC save has
        // them at state 0 or 2, score 0 - fresh, mid-career and 100%. Five of them
        // (state 0 here) do not exist in the PC gameplay database at all. Left as is,
        // the PC Race Day map builds a hub for each with zero events and crashes on
        // event 0 (nfs.exe 0x7F6480; crash hub 0x8F7CCCE0). Values = native PC side of
        // the matched pairs; 0x8DA1975B from the PC 100% save.
        static readonly uint[] ConsoleOnlyRaceDays = {
            0xB48C11C4, 0x0A6C2097, 0xF841FB9F, 0x8DA1975B, 0x8F7CCCE0, 0x92407122,
            0x5C838C1A, 0xAF51A403, 0xDDCEF290, 0x46AE8E2F, 0xB3F02D70, 0x8FEB3CC6,
            0xC8A0888E, 0x66705CF6, 0x150B07D4, 0xD663D2A8, 0x21471712 };
        static readonly uint[] ConsoleOnlyStates = { 2, 0, 2, 0, 0, 2, 0, 0, 2, 0, 2, 2, 0, 2, 2, 2, 2 };

        // progress_table_offset: offset of the table in a little-endian payload, or -1
        static int ProgressTableOffset(byte[] p)
        {
            for (int o = 0; o + 4 <= p.Length; o++)
            {
                if (Rd32(p, o, false) != ProgressFirst) continue;
                int last = o + 12 * (ProgressLen - 1);
                if (last + 4 <= p.Length && Rd32(p, last, false) == ProgressLast) return o;
            }
            return -1;
        }

        // fix_raceday_progress: reset console-only race days to their PC-native state
        static void FixRacedayProgress(byte[] o, List<string> warnings)
        {
            int t = ProgressTableOffset(o);
            if (t < 0)
            {
                warnings.Add("GameplayData: race-day progress table not found - the PC Race Day menu may crash");
                return;
            }
            for (int i = 0; i < ProgressLen; i++)
            {
                int e = t + 12 * i;
                uint key = Rd32(o, e, false);
                for (int j = 0; j < ConsoleOnlyRaceDays.Length; j++)
                    if (ConsoleOnlyRaceDays[j] == key)
                    {
                        Wr32(o, e + 4, ConsoleOnlyStates[j], false);
                        Wr32(o, e + 8, 0, false);
                    }
            }
        }

        static void ApplyStructFixes(Rec rec, byte[] src, List<string> warnings)
        {
            byte[] o = (byte[])rec.Payload.Clone();
            if (rec.Id == GameplayId) { FixRacedayBlock(src, o, warnings); FixRacedayProgress(o, warnings); }
            else if (rec.Id == CarDbId) { FixCarDbParts(src, o); FixCarDbPacked(src, o); }
            else
            {
                FixNodeFlags(src, o);
                if (rec.Id == CustomRacedayId) FixCustomRacedayStrings(src, o);
                else if (rec.Id == FeCareerId) FixCareerName(src, o);
            }
            rec.Payload = o;
        }

        // rehash_gameplay: blob = [MD5(rest)][rest] at payload 0x14, size 0x10000
        static void RehashGameplay(Rec rec)
        {
            if (rec.Id != GameplayId) return;
            byte[] p = rec.Payload;
            int lo = Math.Min(0x24, p.Length), hi = Math.Min(0x14 + 0x10000, p.Length);
            int at = Math.Min(0x14, p.Length);
            byte[] h;
            using (MD5 md5 = MD5.Create()) h = md5.ComputeHash(p, lo, hi - lo);
            if (p.Length < 0x24)
            {
                // Python bytearray slice assignment grows the buffer: the digest
                // replaces the tail and the payload becomes min(len, 0x14) + 16 bytes
                byte[] n = new byte[at + 16];
                Buffer.BlockCopy(p, 0, n, 0, at);
                p = rec.Payload = n;
            }
            Buffer.BlockCopy(h, 0, p, at, 16);
        }

        // is_node_flag: 360 node flag word [u8 flag][FF FF FF] (or zeroed)
        static bool IsNodeFlag(byte[] b, int at)
        {
            return (b[at + 1] == 0xFF && b[at + 2] == 0xFF && b[at + 3] == 0xFF)
                || (b[at + 1] == 0 && b[at + 2] == 0 && b[at + 3] == 0);
        }

        // scalar_tail: PC last payload word when the payload ends in a scalar node
        // (else zeros): [0][len 1..4][flag] (u8 natural, else u32 swap)
        // or [0][len 5..8][flag][d1] (tail = d2, u32 swap)
        static byte[] ScalarTail(byte[] src, byte[] tail)
        {
            byte[] w = new byte[4];
            if (tail.Length != 4) return w;
            for (int k = 0; k <= 4; k += 4)
            {
                int h = src.Length - 12 - k;
                if (h < 0) continue;
                uint lo = k == 0 ? 1u : 5u, hi = k == 0 ? 4u : 8u;
                uint zero = Rd32(src, h, true), ln = Rd32(src, h + 4, true);
                if (zero != 0 || ln < lo || ln > hi || !IsNodeFlag(src, h + 8)) continue;
                if (ln == 1 && tail[1] == 0 && tail[2] == 0 && tail[3] == 0) return (byte[])tail.Clone();
                w[0] = tail[3]; w[1] = tail[2]; w[2] = tail[1]; w[3] = tail[0];
                return w;
            }
            return w;
        }

        // VideoSettings keeps the 360's two extra trailing nodes: the PC loads them fine
        // (in-game 2026-10-09); the trim to 0x74 only ever appeared in failing runs.

        // PC_CONTROLLER_DEFAULT (scripts/python/nfssave/pc_controller_default.bin): native
        // PC default bindings; the 360 has no such chunk. A size-0 filler loaded, but the
        // PC dropped the profile mid-session for a default 'Player' (in-game 2026-10-09).
        const uint PcControllerId = 0x39156567;
        static readonly byte[] PcControllerDefault = Convert.FromBase64String(
            "AAAAAAAAAAAEAAAAAP///wAAAAAAAAAABAAAAAD///8AAAAAAAAAAAQAAAAA////GQAAAAAAAAAEAAAAAP///wEAAAAAAAAA"
            + "BAAAAAD////IAAAAAAAAAAQAAAAA////AQAAAAAAAAAEAAAAAP///x4AAAAAAAAABAAAAAD///8BAAAAAAAAAAQAAAAA////"
            + "0AAAAAAAAAAEAAAAAP///wEAAAAAAAAABAAAAAD///8sAAAAAAAAAAQAAAAA////AQAAAAAAAAAEAAAAAP///8sAAAAAAAAA"
            + "BAAAAAD///8AAAAAAAAAAAQAAAAA////AAAAAAAAAAAEAAAAAP///wEAAAAAAAAABAAAAAD////NAAAAAAAAAAQAAAAA////"
            + "AAAAAAAAAAAEAAAAAP///wAAAAAAAAAABAAAAAD///8BAAAAAAAAAAQAAAAA////UgAAAAAAAAAEAAAAAP///wAAAAAAAAAA"
            + "BAAAAAD///8AAAAAAAAAAAQAAAAA////AQAAAAAAAAAEAAAAAP///zEAAAAAAAAABAAAAAD///8AAAAAAAAAAAQAAAAA////"
            + "AAAAAAAAAAAEAAAAAP///wEAAAAAAAAABAAAAAD///8dAAAAAAAAAAQAAAAA////AAAAAAAAAAAEAAAAAP///wAAAAAAAAAA"
            + "BAAAAAD///8BAAAAAAAAAAQAAAAA////KgAAAAAAAAAEAAAAAP///wAAAAAAAAAABAAAAAD///8AAAAAAAAAAAQAAAAA////"
            + "AQAAAAAAAAAEAAAAAP///zkAAAAAAAAABAAAAAD///8AAAAAAAAAAAQAAAAA////AAAAAAAAAAAEAAAAAP///wAAAAAAAAAA"
            + "BAAAAAD///8AAAAAAAAAAAQAAAAA////AAAAAAAAAAAEAAAAAP///wAAAAAAAAAABAAAAAD///8AAAAAAAAAAAQAAAAA////"
            + "AAAAAAAAAAAEAAAAAP///wAAAAAAAAAABAAAAAD///8AAAAAAAAAAAQAAAAA////AAAAAAAAAAAEAAAAAP///wAAAAAAAAAA"
            + "BAAAAAD///8AAAAAAAAAAAQAAAAA////AAAAAAAAAAAEAAAAAP///wAAAAAAAAAABAAAAAD///8AAAAAAAAAAAQAAAAA////"
            + "AAAAAAAAAAAEAAAAAP///wAAAAAAAAAABAAAAAD///8AAAAAAAAAAAQAAAAA////AAAAAAAAAAAEAAAAAP///wAAAAAAAAAA"
            + "BAAAAAD///8AAAAAAAAAAAQAAAAA////AAAAAAAAAAAEAAAAAP///wAAAAAAAAAABAAAAAD///8AAAAAAAAAAAQAAAAA////"
            + "AAAAAAAAAAAEAAAAAP///wAAAAAAAAAABAAAAAD///8AAAAAAAAAAAQAAAAA////AAAAAAAAAAAEAAAAAP///wAAAAAAAAAA"
            + "BAAAAAD///8AAAAAAAAAAAQAAAAA////AAAAAAAAAAAEAAAAAP///wAAAAAAAAAABAAAAAD///8AAAAAAAAAAAQAAAAA////"
            + "AQAAAAAAAAAEAAAAAP///xMAAAAAAAAABAAAAAD///8AAAAAAAAAAAQAAAAA////AAAAAAAAAAAEAAAAAP///wEAAAAAAAAA"
            + "BAAAAAD///8FAAAAAAAAAAQAAAAA////AAAAAAAAAAAEAAAAAP///wAAAAAAAAAABAAAAAD///8BAAAAAAAAAAQAAAAA////"
            + "BAAAAAAAAAAEAAAAAP///wAAAAAAAAAABAAAAAD///8AAAAAAAAAAAQAAAAA////AQAAAAAAAAAEAAAAAP///wMAAAAAAAAA"
            + "BAAAAAD///8AAAAAAAAAAAQAAAAA////AAAAAAAAAAAEAAAAAP///wEAAAAAAAAABAAAAAD///8CAAAAAAAAAAQAAAAA////"
            + "AAAAAAAAAAAEAAAAAP///wAAAAAAAAAABAAAAAD///8BAAAAAAAAAAQAAAAA////LgAAAAAAAAAEAAAAAP///wAAAAAAAAAA"
            + "BAAAAAD///8AAAAAAAAAAAQAAAAA////AQAAAAAAAAAEAAAAAP///zAAAAAAAAAABAAAAAD///8AAAAAAAAAAAQAAAAA////"
            + "AAAAAAAAAAAEAAAAAP///wEAAAAAAAAABAAAAAD///9YAAAAAAAAAAQAAAAA////AAAAAAAAAAAEAAAAAP///wAAAAAAAAAA"
            + "BAAAAAD///8AAAAA");

        // _to_pc_record: [id][size][flags=1][content + last word], same total size.
        // The last word is the record's final value, which the 360 stores in the NEXT
        // record's header slot (see TailWord).
        static void ToPcRecord(Rec rec, byte[] last)
        {
            rec.Type = 1;
            int len = rec.Payload.Length;
            if (len > 0)
            {
                // payload[4:] + last word; a 1-3 byte payload grows to 4 (as in Python)
                byte[] n = new byte[Math.Max(len, 4)];
                if (len > 4) Buffer.BlockCopy(rec.Payload, 4, n, 0, len - 4);
                Buffer.BlockCopy(last, 0, n, n.Length - 4, 4);
                rec.Payload = n;
            }
        }

        // tail_word: PC byte order for a record's final word, taken from the 360 word
        // at the next record's header slot (spill, big-endian as stored). Verified:
        // CAREER_01 CustomRaceDayMemcard spills 0x00000001 (its last event flag; the
        // PC-written race day ends 01 00 00 00 too) and FECareer spills 0x2848 in
        // every 360 sample. Node streams ending in a u32 node ([0][4][flag] before the
        // spill) swap it; raw memcpy blobs swap like the rest of the blob; anything
        // else (a string node running into the spill) stays natural. GameplayData
        // keeps zero: its 360 buffer is trimmed, so the spilled word is 360-only padding.
        // Other trailing scalar nodes (u8 options, 8-byte nodes) convert as in ScalarTail.
        static byte[] TailWord(uint recId, byte[] src, byte[] spill)
        {
            if (recId == GameplayId || spill.Length != 4) return new byte[4];
            int n = src.Length;
            bool u32NodeEnd = n >= 12 && Rd32(src, n - 12, true) == 0 && Rd32(src, n - 8, true) == 4;
            if (recId == CarDbId || u32NodeEnd)   // RAW_BLOB_IDS minus GameplayData (returned above)
                return new byte[] { spill[3], spill[2], spill[1], spill[0] };
            byte[] w = ScalarTail(src, spill);
            return (w[0] | w[1] | w[2] | w[3]) != 0 ? w : spill;
        }

        static string ChunkName(uint id)
        {
            string n;
            return ChunkNames.TryGetValue(id, out n) ? n : Hex(id);
        }

        // convert_tree
        static Tree ConvertTree(Tree t, SaveResult rep, Dictionary<string, Rule> rules)
        {
            if (t.Gap > 0)
                rep.Warnings.Add(Hex(t.Gap) + " bytes of damaged noise inside the console record region (known console writing bug)");
            if (rules == null)
                rep.Warnings.Add("fieldmap rules unavailable - all chunks converted in auto mode");
            Tree pc = new Tree();
            pc.Noise = t.Noise;
            pc.Records = new List<Rec>();
            pc.Pre = new byte[0];
            pc.Post = t.Post.Length > 0 ? ConvertAuto(t.Post, null, "chunk") : new byte[0];
            // a 360 record's final word sits in the next record's header slot; the
            // last record's in the word after the record area. Noise breaks the
            // chain: the record right before a damaged region has no spill, and a
            // trailing gap leaves the last record with none too; after a
            // successful re-anchor everything is as in an undamaged tree.
            List<byte[]> spills = new List<byte[]>();
            for (int i = 1; i < t.Records.Count; i++)
            {
                byte[] w = new byte[4];
                Wr32(w, 0, t.Records[i].Type, true);
                spills.Add(w);
            }
            if (t.GapAt > 0) spills[(int)t.GapAt - 1] = new byte[0];
            spills.Add(t.Gap == 0 || t.GapAt >= 0 ? Slice(t.Post, 0, Math.Min(4, t.Post.Length)) : new byte[0]);
            int ri = 0;
            foreach (Rec rec in t.Records)
            {
                byte[] spill = spills[ri++];
                NormalizeGameplay(rec, rep.Warnings);
                byte[] src = rec.Payload;
                string mode;
                if (rec.Id == GameplayId)
                {
                    rec.Payload = SwapU32s(rec.Payload);
                    mode = "gameplay";
                }
                else if (rec.Id == RaceDataId)
                {
                    // NUMERIC_IDS: u32/float node stream, no strings (fix_node_flags follows)
                    rec.Payload = SwapU32s(rec.Payload);
                    mode = "numeric";
                }
                else mode = ConvertRecord(rep.Kind, rec, rep.Warnings, rules);
                ApplyStructFixes(rec, src, rep.Warnings);
                if (mode == "auto" && rec.Payload.Length > 0x1000 && rec.Id != GameplayId)
                    rep.Warnings.Add("chunk " + ChunkName(rec.Id) + " (" + Hex(rec.Payload.Length)
                        + " B) converted in auto mode (no fieldmap)");
                ToPcRecord(rec, TailWord(rec.Id, src, spill));
                RehashGameplay(rec);
                pc.Records.Add(rec);
            }
            if (t.Gap > 0 && pc.Records.Count < (long)t.Count)
                rep.Warnings.Add("console record region damaged - missing chunks "
                    + "convert as absent and the game fills defaults");
            pc.Count = (uint)pc.Records.Count;
            // PC tree head: [allocator garbage][root record: magic/used/flags=1]
            byte[] pre = new byte[PcHeadStructSize + 12];
            Wr32(pre, PcHeadStructSize, TreeMagic, false);
            Wr32(pre, PcHeadStructSize + 8, 1, false);
            pc.Pre = pre;
            // positional pairing: PCControllerSettings holds its slot, with the native
            // default bindings (an empty one made the PC drop the profile mid-session)
            if (rep.Kind == "alias")
            {
                bool has = false;
                foreach (Rec r in pc.Records) if (r.Id == PcControllerId) has = true;
                if (!has)
                {
                    int ps0 = pc.Records.Count;
                    for (int k = 0; k < pc.Records.Count; k++) if (pc.Records[k].Id == 0x8B7D0AAD) { ps0 = k; break; }
                    Rec f = new Rec();
                    f.Type = 1; f.Id = PcControllerId; f.Payload = (byte[])PcControllerDefault.Clone();
                    pc.Records.Insert(ps0, f);
                    pc.Count = (uint)pc.Records.Count;
                }
            }
            foreach (Rec r in pc.Records) rep.Chunks.Add(ChunkName(r.Id));
            return pc;
        }

        // convert_payload; rules == null means all-auto. The tree hash (bytes 0..15) is left
        // for the caller to patch before BuildMc02.
        public static SaveResult ConvertSave(byte[] mc02Payload, Dictionary<string, Rule> rules)
        {
            Mc02 m = ParseMc02(mc02Payload);
            SaveResult rep = new SaveResult();
            Tree t360 = ParseTree(m.Tree, true);
            rep.Kind = m.Extra.Length == 64 ? "alias" : "career";
            Tree pc = ConvertTree(t360, rep, rules);
            rep.Records = pc.Records.Count;
            byte[] tree = BuildTree(pc, m.TreeSize);
            long used = 0;
            foreach (Rec r in pc.Records) used += 12 + r.Payload.Length;
            byte[] extra = ConvertExtra(m.Extra);
            // extra word 1 = tree used size on careers AND aliases; a stale
            // alias value (short of the inserted PCControllerSettings, 12 + 0x684 B)
            // made the PC skip the alias for a default 'Player' profile
            Wr32(extra, 4, (uint)used, false);
            rep.Extra = extra;
            rep.Tree = tree;
            rep.TreeSize = m.TreeSize;
            return rep;
        }
    }
}
'@

if (-not ([System.Management.Automation.PSTypeName]'NfsPs.Save').Type) {
    Add-Type -TypeDefinition $csharp -Language CSharp
}
try { Add-Type -AssemblyName System.Numerics } catch { }

# treehash.py: chained MD5 + modpow over tree[0x10:]; returns the 16 bytes for tree[0:0x10]
function ConvertFrom-HexString([string]$hex) {
    $b = New-Object byte[] ($hex.Length / 2)
    for ($i = 0; $i -lt $b.Length; $i++) { $b[$i] = [Convert]::ToByte($hex.Substring(2 * $i, 2), 16) }
    , $b
}
$script:TreeE = ConvertFrom-HexString ('41712698348b53b247ac4b0cfe32162265c1bdeb66590ed156707de646d307b4' +
    '18e67f5a51c987be4b0bc80369920669e02bdcebfcdc40dba7169e1c7b22a62e')
$script:TreeN = ConvertFrom-HexString ('57a11e76c0fea0c76e43ac00cf073334444da3b91b462aa4bdfe3c389b383bb5' +
    'a081c6d8d0b5ee6d1a2fcdf965a14743de47cc4f7eaf309e22cb6be37dcadaaf')

function Get-TreeHash([byte[]]$tree) {
    $md5 = [System.Security.Cryptography.MD5]::Create()
    try {
        $cur = $md5.ComputeHash($tree, 0x10, $tree.Length - 0x10)
        $m = New-Object byte[] 65          # 4 digests, little-endian, + 0x00 sign byte
        for ($r = 0; $r -lt 4; $r++) {
            [Array]::Copy($cur, 0, $m, 16 * $r, 16)
            $cur = $md5.ComputeHash($cur)
        }
    } finally { $md5.Dispose() }
    $pad = { param([byte[]]$b) $x = New-Object byte[] ($b.Length + 1); [Array]::Copy($b, $x, $b.Length); , $x }
    $big = [System.Numerics.BigInteger]
    $res = $big::ModPow((New-Object System.Numerics.BigInteger (, $m)),
                        (New-Object System.Numerics.BigInteger (, (& $pad $script:TreeE))),
                        (New-Object System.Numerics.BigInteger (, (& $pad $script:TreeN))))
    $rb = $res.ToByteArray()
    $out = New-Object byte[] 16
    [Array]::Copy($rb, $out, [Math]::Min(16, $rb.Length))
    , $out
}

function Get-FullPath([string]$p) {
    $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($p)
}

# Same convention as nfspc-converter batch.rs back_up_existing.
function Backup-Existing([string]$root, [string]$base, [string]$name, [string]$stamp) {
    $existing = Join-Path (Join-Path $root $name) $name
    if (-not (Test-Path -LiteralPath $existing -PathType Leaf)) { return $null }
    $n = 1
    while ($true) {
        $dir = if ($n -eq 1) { $stamp } else { "$stamp-$n" }
        $dest = Join-Path (Join-Path (Join-Path (Join-Path $base 'SaveConverter backups') $dir) $name) $name
        if (-not (Test-Path -LiteralPath $dest)) { break }
        $n++
    }
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $dest) | Out-Null
    Copy-Item -LiteralPath $existing -Destination $dest
    return $dest
}

# convert.py check_save_name: a name that cannot be a plain folder name. Windows
# drops trailing dots/spaces, so "..." or "  " would collapse onto the output root.
function Test-SaveName([string]$name) {
    if (-not $name -or $name.IndexOfAny([char[]]'\/:') -ge 0 -or $name -eq '.' -or $name -eq '..' -or -not $name.TrimEnd('.', ' ')) {
        throw "unsafe save name '$name'"
    }
}

# convert.py convert_one; throws on failure (nothing is written before the output is complete)
function Convert-One([string]$path, $rules, [string]$outRoot, [string]$backupBase, [string]$stamp, [hashtable]$claimed) {
    $leaf = Split-Path -Leaf $path
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "$path : source file not found" }
    $data = [System.IO.File]::ReadAllBytes($path)
    $cont = [NfsPs.Save]::ParseContainer($data, $path)
    # Order matches convert.py: name, then same-name-in-batch, then corruption.
    $name = $cont.Name
    Test-SaveName $name
    # exe batch.rs: two saves in one run must not export to the same folder
    # (key = the name Windows creates: case-insensitive, trailing dots/spaces dropped)
    $key = $name.TrimEnd('.', ' ').ToLowerInvariant()
    if ($claimed.ContainsKey($key)) {
        throw "another selected save ($($claimed[$key])) is also named $name; converting both would overwrite it - convert it separately"
    }
    # only a converted save claims its name (exe batch.rs; set at both exits below)

    $bad = [NfsPs.Save]::CheckMc02($cont.Payload)
    if ($bad -contains 'extra CRC mismatch') {
        throw "${leaf}: extra-blob CRC mismatch - the source file is corrupted; refusing to convert"
    }
    foreach ($prob in $bad) { Write-Host "! ${leaf}: $prob (CRCs are recomputed on write)" }

    $res = [NfsPs.Save]::ConvertSave($cont.Payload, $rules)
    [Array]::Copy((Get-TreeHash $res.Tree), 0, $res.Tree, 0, 16)
    $bytes = [NfsPs.Save]::BuildMc02($res.Extra, $res.Tree, $res.TreeSize)
    Write-Host "[+] $leaf ($($res.Kind)): $($res.Records) chunks"
    foreach ($c in $res.Chunks) { Write-Host "      - $c" }
    foreach ($w in $res.Warnings) { Write-Host "      ! $w" }
    if ($DryRun) { Write-Host '[.] dry run - not writing'; $claimed[$key] = $leaf; return }

    $backup = Backup-Existing $outRoot $backupBase $name $stamp
    if ($backup) { Write-Host "[+] backed up existing save to $backup" }
    $folder = Join-Path $outRoot $name
    New-Item -ItemType Directory -Force -Path $folder | Out-Null
    $target = Join-Path $folder $name
    $tmp = "$target.tmp"
    try {
        [System.IO.File]::WriteAllBytes($tmp, $bytes)
        Move-Item -LiteralPath $tmp -Destination $target -Force
    } finally {
        if (Test-Path -LiteralPath $tmp) { Remove-Item -LiteralPath $tmp -Force -ErrorAction SilentlyContinue }
    }
    $check = [NfsPs.Save]::CheckMc02([System.IO.File]::ReadAllBytes($target))
    if ($check.Count -eq 0) { Write-Host "[+] wrote $target (self-check OK)"; $claimed[$key] = $leaf }
    else { throw "wrote $target but the self-check failed: $($check -join ', ')" }
}

function Exit-Usage([string]$msg) {
    [Console]::Error.WriteLine($msg)
    exit 2
}

# ---- main ------------------------------------------------------------------
function Test-ConMagic([string]$path) {
    try {
        $fs = [System.IO.File]::Open($path, 'Open', 'Read', 'ReadWrite')
        try {
            $b = New-Object byte[] 4
            $n = $fs.Read($b, 0, 4)
            return ($n -eq 4 -and $b[0] -eq 0x43 -and $b[1] -eq 0x4F -and $b[2] -eq 0x4E -and $b[3] -eq 0x20)
        } finally { $fs.Dispose() }
    } catch { return $false }
}

# Recursive folder walk: CAREER_* / ALIAS_* files that start with "CON ".
# Anything under a "SaveConverter backups" folder (relative to $dir) is skipped.
function Find-SaveFiles([string]$dir) {
    $root = $dir.TrimEnd('\', '/')
    $hits = @(Get-ChildItem -LiteralPath $dir -Recurse -File -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -like 'CAREER_*' -or $_.Name -like 'ALIAS_*' } |
        Where-Object {
            $rel = $_.FullName.Substring([Math]::Min($root.Length, $_.FullName.Length))
            -not (@($rel -split '[\\/]') -contains 'SaveConverter backups') -and (Test-ConMagic $_.FullName)
        } | Sort-Object FullName | ForEach-Object { $_.FullName })
    , $hits
}

$saves = @()
$failures = 0
if ($Source) {
    foreach ($p in $Source) {
        if (-not $p.Trim()) { $failures++; [Console]::Error.WriteLine('[!] empty input path'); continue }
        $full = Get-FullPath $p
        if (Test-Path -LiteralPath $full -PathType Container) {
            $hits = Find-SaveFiles $full
            if ($hits.Count -eq 0) {
                $failures++
                [Console]::Error.WriteLine("[!] no saves found in folder $full (looked for CAREER_*/ALIAS_* files starting with CON)")
            } else { $saves += $hits }
        } else { $saves += $full }
    }
}
if ($Usb) {
    $found = @()
    # a bare "F" means the drive root, not a folder named F in the current directory
    if ($Usb -match '^[A-Za-z]:?$') { $Usb = $Usb.Substring(0, 1) + ':\' }
    try {
        $content = Join-Path $Usb 'Content'
        if (Test-Path -LiteralPath $content -PathType Container) {
            # case-insensitive, like the exe (fatx discovery is_save_name)
            $found = @(Get-ChildItem -Path (Join-Path $content '*\*\0000000[12]\*') -File -ErrorAction SilentlyContinue |
                Where-Object { $_.Name -like 'CAREER_*' -or $_.Name -like 'ALIAS_*' } |
                Sort-Object FullName | ForEach-Object { $_.FullName })
        }
    } catch { $found = @() }
    if ($found.Count -eq 0) {
        $failures++
        [Console]::Error.WriteLine("[!] no saves found under $Usb\Content")
    } else { $saves += $found }
}
# overlapping inputs (a folder and a file inside it) convert each file once
$seen = @{}
$saves = @($saves | Where-Object { $k = $_.ToLowerInvariant(); if ($seen.ContainsKey($k)) { $false } else { $seen[$k] = 1; $true } })
if ($saves.Count -eq 0) {
    if ($failures) { exit 1 }   # every input was a folder without saves (already reported)
    Exit-Usage 'usage: Convert-NfsSave.ps1 <file-or-folder>... [-OutRoot <dir>] [-DryRun]  |  -Usb <drive-or-folder> [-OutRoot <dir>]  (output defaults to the current directory)'
}

# Output root R: -OutRoot, else the current directory.
try {
    $rArg = if ($OutRoot) { $OutRoot } else { (Get-Location).ProviderPath }
    $rootFull = [System.IO.Path]::GetFullPath((Get-FullPath $rArg))
} catch { Exit-Usage "bad -OutRoot '$OutRoot': $($_.Exception.Message)" }
if ((Test-Path -LiteralPath $rootFull) -and -not (Test-Path -LiteralPath $rootFull -PathType Container)) {
    Exit-Usage "-OutRoot '$rootFull' exists and is not a folder"
}
# a missing R is created by the first write (Convert-One), so a run where
# every source fails leaves nothing behind

# Save folder S (game mode) or R itself (plain mode); backups next to S in game mode.
$rootTrim = $rootFull.TrimEnd('\', '/')
$gameMode = $true
if (Test-Path -LiteralPath (Join-Path $rootFull 'SAVE\NFS ProStreet') -PathType Container) {
    $outRootFull = Join-Path $rootFull 'SAVE\NFS ProStreet'
} elseif (Test-Path -LiteralPath (Join-Path $rootFull 'NFS ProStreet') -PathType Container) {
    $outRootFull = Join-Path $rootFull 'NFS ProStreet'
} elseif ((Split-Path -Leaf $rootTrim) -eq 'NFS ProStreet') {
    $outRootFull = $rootFull
} else {
    $gameMode = $false
    $outRootFull = $rootFull
}
if ($gameMode) {
    $sTrim = $outRootFull.TrimEnd('\', '/')
    $backupBase = [System.IO.Path]::GetDirectoryName($sTrim)
    if (-not $backupBase) { $backupBase = $sTrim }
    Write-Host "[+] game save folder: $outRootFull"
} else {
    $backupBase = $outRootFull
    Write-Host "[+] output folder: $outRootFull"
    Write-Host "    (copy the converted folders into the game's SAVE\NFS ProStreet folder to use them)"
}

$rules = $null
$rulesPath = Join-Path $PSScriptRoot 'fieldmaps.rules'
if (Test-Path -LiteralPath $rulesPath -PathType Leaf) {
    try { $rules = [NfsPs.Save]::LoadRules([System.IO.File]::ReadAllText($rulesPath)) } catch { $rules = $null }
}

$stamp = [DateTime]::UtcNow.ToString('yyyy-MM-dd_HH-mm-ss', [System.Globalization.CultureInfo]::InvariantCulture)
$claimed = @{}
foreach ($s in $saves) {
    try { Convert-One $s $rules $outRootFull $backupBase $stamp $claimed }
    catch {
        $failures++
        [Console]::Error.WriteLine("[!] FAILED ${s}: $($_.Exception.Message)")
    }
}
if ($failures) { exit 1 }
exit 0
