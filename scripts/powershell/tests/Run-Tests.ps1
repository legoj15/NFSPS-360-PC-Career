# Black-box tests for Convert-NfsSave.ps1. Runs the converter in a child
# process of the SAME PowerShell host as this runner, so:
#   powershell.exe -NoProfile -ExecutionPolicy Bypass -File Run-Tests.ps1   (5.1)
#   pwsh -NoProfile -File Run-Tests.ps1                                     (7.x)
# Golden md5 pins are shared with tests/test_golden.py and
# src/crates/nfssave-core/tests/test_golden.rs - keep all three identical.
# Sources under Extracted/ are personal saves (gitignored); absent ones skip.
# Tracked fixtures (docs/re/...) must be present.
# Exit code 0 = all passed.

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2

$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$repo = (Resolve-Path (Join-Path $here '..\..\..')).Path
$converter = Join-Path $here '..\Convert-NfsSave.ps1'
$hostExe = (Get-Process -Id $PID).Path

$script:failed = 0
$script:passed = 0
$script:skipped = 0

function Pass([string]$name) { $script:passed++; Write-Host "  ok    $name" }
function Fail([string]$name, [string]$why) { $script:failed++; Write-Host "  FAIL  $name - $why" -ForegroundColor Red }
function Skip([string]$name, [string]$why) { $script:skipped++; Write-Host "  skip  $name - $why" -ForegroundColor Yellow }

function Invoke-Converter([string[]]$ArgList, [string]$WorkDir) {
    # 5.1 turns native stderr into ErrorRecords; under 'Stop' that would throw
    $ErrorActionPreference = 'Continue'
    $oldCwd = [Environment]::CurrentDirectory
    if ($WorkDir) { Push-Location -LiteralPath $WorkDir; [Environment]::CurrentDirectory = $WorkDir }
    try {
        $out = & $hostExe -NoProfile -ExecutionPolicy Bypass -File $converter @ArgList 2>&1
        $code = $LASTEXITCODE
    } finally {
        if ($WorkDir) { Pop-Location; [Environment]::CurrentDirectory = $oldCwd }
    }
    return @{ Code = $code; Text = (($out | ForEach-Object { "$_" }) -join "`n") }
}

function Get-Md5Hex([string]$path) {
    $md5 = [System.Security.Cryptography.MD5]::Create()
    try { $h = $md5.ComputeHash([System.IO.File]::ReadAllBytes($path)) } finally { $md5.Dispose() }
    return (($h | ForEach-Object { $_.ToString('x2') }) -join '')
}

function New-TempDir {
    $p = Join-Path ([System.IO.Path]::GetTempPath()) ('nfsps-ps-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $p | Out-Null
    return $p
}

# (source, golden md5, container name). Output lands under the STFS
# file-table name, not the source file name - the game looks saves up by it.
$cases = @(
    @('Extracted\Career\CAREER_01', '4afedb367a0fe2c8dee178e4ed3daca1', 'CAREER_01'),
    @('docs\re\pair\CAREER_02_360_fresh', '3da9f4c0a5a2b7d5c55863d49de4852c', 'CAREER_02'),
    @('Extracted\Career\CAREER_03', '77e5b3e95f3734b5460a986a9b74a92b', 'CAREER_03'),
    @('Extracted\Alias\ALIAS_360', 'a5a24e0f79571d2b5f819a76704a6b89', 'ALIAS_JOSHUA S 10'),
    @('docs\re\c1_latest\CAREER_01_360', '5b7d3fcb229ba2ec135d68de121bb0ff', 'CAREER_01'),
    @('docs\re\pair_raceday\CAREER_02_360', 'ec77c9309356db48faeae8e66f840176', 'CAREER_02'),
    # anonymized copy of the personal alias save (docs\re\alias_anon\README.md)
    @('docs\re\alias_anon\ALIAS_360', '377651916f0e1bd488561b7481a66da1', 'ALIAS_ANONYMOUS 1'),
    # junk-padded one-byte nodes (docs\re\alias_anon_junkpad\README.md)
    @('docs\re\alias_anon_junkpad\ALIAS_360', '43a94087bf77b4a88991ce411e99cf15', 'ALIAS_ANONYMOUS 1')
)

Write-Host "PowerShell $($PSVersionTable.PSVersion) ($hostExe)"
$tmpRoots = @()
try {
    # --- converter present and ASCII-only (5.1 reads BOM-less files as ANSI)
    if (-not (Test-Path $converter)) {
        Fail 'converter exists' "missing: $converter"
    } else {
        Pass 'converter exists'
        $bytes = [System.IO.File]::ReadAllBytes((Resolve-Path $converter).Path)
        $nonAscii = @($bytes | Where-Object { $_ -gt 0x7F }).Count
        if ($nonAscii) { Fail 'converter is ASCII-only' "$nonAscii non-ASCII bytes" } else { Pass 'converter is ASCII-only' }
    }

    # --- golden outputs: <OutRoot>\<NAME>\<NAME>, byte-exact
    foreach ($c in $cases) {
        $src = Join-Path $repo $c[0]
        $name = $c[0]   # full relative path: two cases share the leaf ALIAS_360
        if (-not (Test-Path $src -PathType Leaf)) {
            # personal saves (gitignored) may be absent; tracked fixtures may not
            if ($c[0] -like 'Extracted\*') { Skip "golden $name" 'source not present' }
            else { Fail "golden $($c[0])" 'tracked fixture missing' }
            continue
        }
        # fresh root per case: several sources share a container name
        $out = New-TempDir; $tmpRoots += $out
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $r = Invoke-Converter @($src, '-OutRoot', $out)
        $sw.Stop()
        $target = Join-Path (Join-Path $out $c[2]) $c[2]
        if ($r.Code -ne 0) { Fail "golden $name" "exit $($r.Code): $($r.Text)"; continue }
        if (-not (Test-Path $target -PathType Leaf)) { Fail "golden $name" "no output at $target"; continue }
        $md5 = Get-Md5Hex $target
        if ($md5 -ne $c[1]) { Fail "golden $name" "md5 $md5 != $($c[1])" }
        else { Pass ("golden $name ({0:N1}s)" -f $sw.Elapsed.TotalSeconds) }
    }

    $pair = Join-Path $repo 'docs\re\pair\CAREER_02_360_fresh'

    # --- several sources in one call
    $multi = New-TempDir; $tmpRoots += $multi
    $c1 = Join-Path $repo 'docs\re\c1_latest\CAREER_01_360'
    $r = Invoke-Converter @($pair, $c1, '-OutRoot', $multi)
    $okA = Test-Path (Join-Path $multi 'CAREER_02\CAREER_02')
    $okB = Test-Path (Join-Path $multi 'CAREER_01\CAREER_01')
    if ($r.Code -eq 0 -and $okA -and $okB) { Pass 'multiple sources' } else { Fail 'multiple sources' "exit $($r.Code), outputs $okA/$okB" }

    # --- dry run writes nothing
    $dry = New-TempDir; $tmpRoots += $dry
    $r = Invoke-Converter @($pair, '-OutRoot', $dry, '-DryRun')
    $written = @(Get-ChildItem -Recurse -File $dry).Count
    if ($r.Code -eq 0 -and $written -eq 0) { Pass 'dry run writes nothing' } else { Fail 'dry run writes nothing' "exit $($r.Code), $written files" }

    # --- dry run = same exit code and refusal as a real run: a save whose STFS
    #     name is unsafe ("CAREER/02", same length) fails both, writes nothing
    $badDir = New-TempDir; $tmpRoots += $badDir
    $badBytes = [System.IO.File]::ReadAllBytes($pair)
    $needle = [System.Text.Encoding]::ASCII.GetBytes('CAREER_02') + [byte]0
    $at = -1
    for ($i = 0; $i -le $badBytes.Length - $needle.Length -and $at -lt 0; $i++) {
        $hit = $true
        for ($j = 0; $j -lt $needle.Length; $j++) { if ($badBytes[$i + $j] -ne $needle[$j]) { $hit = $false; break } }
        if ($hit) { $at = $i }
    }
    if ($at -lt 0) { Fail 'unsafe name: dry run and real run both refuse' 'STFS name not found in fixture' }
    else {
        $badBytes[$at + 6] = [byte][char]'/'
        $badSrc = Join-Path $badDir 'bad'
        [System.IO.File]::WriteAllBytes($badSrc, $badBytes)
        $badOut = Join-Path $badDir 'out'
        $rd = Invoke-Converter @($badSrc, '-OutRoot', $badOut, '-DryRun')
        $rr = Invoke-Converter @($badSrc, '-OutRoot', $badOut)
        $refused = ($rd.Text -match "unsafe save name 'CAREER/02'") -and ($rr.Text -match "unsafe save name 'CAREER/02'")
        $wroteNothing = -not (Test-Path -LiteralPath $badOut) -or @(Get-ChildItem -Recurse -File $badOut).Count -eq 0
        if ($rd.Code -eq 1 -and $rr.Code -eq 1 -and $refused -and $wroteNothing) { Pass 'unsafe name: dry run and real run both refuse' }
        else { Fail 'unsafe name: dry run and real run both refuse' "dry exit $($rd.Code), real exit $($rr.Code), refused=$refused, wroteNothing=$wroteNothing" }

        # --- precedence (name, then duplicate, then corruption): an unsafe-named
        #     save that is also extra-CRC-corrupt reports the name, like Python
        $mc = -1
        for ($i = 0; $i -le $badBytes.Length - 4 -and $mc -lt 0; $i++) {
            if ($badBytes[$i] -eq 0x4D -and $badBytes[$i + 1] -eq 0x43 -and $badBytes[$i + 2] -eq 0x30 -and $badBytes[$i + 3] -eq 0x32) { $mc = $i }
        }
        if ($mc -lt 0) { Fail 'unsafe name beats corruption' 'MC02 payload not found in fixture' }
        else {
            $both = [byte[]]$badBytes.Clone()
            $both[$mc + 0x1C + 2] = $both[$mc + 0x1C + 2] -bxor 0xFF
            $bothSrc = Join-Path $badDir 'both'
            [System.IO.File]::WriteAllBytes($bothSrc, $both)
            $rb = Invoke-Converter @($bothSrc, '-OutRoot', $badOut)
            # control: the same corruption with a safe name does report the CRC
            $ok = [byte[]]$both.Clone()
            $ok[$at + 6] = [byte][char]'_'
            $okSrc = Join-Path $badDir 'corrupt'
            [System.IO.File]::WriteAllBytes($okSrc, $ok)
            $rc = Invoke-Converter @($okSrc, '-OutRoot', $badOut)
            $noBackup = -not (Test-Path -LiteralPath (Join-Path $badDir 'SaveConverter backups'))
            if ($rb.Code -eq 1 -and $rb.Text -match "unsafe save name 'CAREER/02'" -and $rb.Text -notmatch 'extra-blob CRC' -and $rc.Text -match 'extra-blob CRC' -and $noBackup) { Pass 'unsafe name beats corruption' }
            else { Fail 'unsafe name beats corruption' "exit $($rb.Code): $($rb.Text) | control: $($rc.Text)" }
        }

        # --- a name that is only dots (Windows drops them -> the output root) is unsafe
        $dots = [byte[]]$badBytes.Clone()
        for ($k = 0; $k -lt 9; $k++) { $dots[$at + $k] = [byte][char]'.' }
        $dotsSrc = Join-Path $badDir 'dots'
        [System.IO.File]::WriteAllBytes($dotsSrc, $dots)
        $rdd = Invoke-Converter @($dotsSrc, '-OutRoot', $badOut, '-DryRun')
        $rdr = Invoke-Converter @($dotsSrc, '-OutRoot', $badOut)
        $wroteNothing = -not (Test-Path -LiteralPath $badOut) -or @(Get-ChildItem -Recurse -File $badOut).Count -eq 0
        if ($rdd.Code -eq 1 -and $rdr.Code -eq 1 -and $rdd.Text -match "unsafe save name '\.\.\.\.\.\.\.\.\.'" -and $rdr.Text -match "unsafe save name '\.\.\.\.\.\.\.\.\.'" -and $wroteNothing -and -not (Test-Path -LiteralPath (Join-Path $badDir 'SaveConverter backups'))) { Pass 'dots-only name refused' }
        else { Fail 'dots-only name refused' "dry $($rdd.Code), real $($rdr.Code), wroteNothing=$wroteNothing" }
    }

    # --- existing save is backed up before it is replaced (exe convention:
    #     <parent of OutRoot>\SaveConverter backups\<stamp>\<NAME>\<NAME>)
    $bk = New-TempDir; $tmpRoots += $bk
    $saveRoot = Join-Path $bk 'NFS ProStreet'
    $old = Join-Path $saveRoot 'CAREER_02\CAREER_02'
    New-Item -ItemType Directory -Force -Path (Split-Path $old) | Out-Null
    [System.IO.File]::WriteAllBytes($old, [byte[]](1, 2, 3, 4))
    $r = Invoke-Converter @($pair, '-OutRoot', $saveRoot)
    $backups = @(Get-ChildItem -Recurse -File (Join-Path $bk 'SaveConverter backups') -ErrorAction SilentlyContinue)
    $kept = $backups.Count -eq 1 -and $backups[0].Name -eq 'CAREER_02' -and $backups[0].Length -eq 4
    $replaced = (Get-Md5Hex $old) -eq '3da9f4c0a5a2b7d5c55863d49de4852c'
    if ($r.Code -eq 0 -and $kept -and $replaced) { Pass 'existing save backed up then replaced' }
    else { Fail 'existing save backed up then replaced' "exit $($r.Code), backups $($backups.Count), replaced $replaced" }

    # --- -Flash walks <root>\Content\<profile>\45410822\0000000[12]\<file>
    $fl = New-TempDir; $tmpRoots += $fl
    $drive = Join-Path $fl 'stick'
    $cdir = Join-Path $drive 'Content\E00001CFFAB204C4\45410822\00000001'
    New-Item -ItemType Directory -Force -Path $cdir | Out-Null
    Copy-Item $pair (Join-Path $cdir 'CAREER_02')
    [System.IO.File]::WriteAllText((Join-Path $cdir 'name.txt'), 'not a save')
    $flOut = Join-Path $fl 'out'
    $r = Invoke-Converter @('-Usb', $drive, '-OutRoot', $flOut)
    $t = Join-Path $flOut 'CAREER_02\CAREER_02'
    if ($r.Code -eq 0 -and (Test-Path $t) -and (Get-Md5Hex $t) -eq '3da9f4c0a5a2b7d5c55863d49de4852c') { Pass 'usb drive walk' }
    else { Fail 'usb drive walk' "exit $($r.Code): $($r.Text)" }

    # --- -Flash is kept as an alias of -Usb
    $flOutA = Join-Path $fl 'out-alias'
    $r = Invoke-Converter @('-Flash', $drive, '-OutRoot', $flOutA)
    if ($r.Code -eq 0 -and (Test-Path (Join-Path $flOutA 'CAREER_02\CAREER_02'))) { Pass '-Flash still works as alias' }
    else { Fail '-Flash still works as alias' "exit $($r.Code): $($r.Text)" }

    # --- bare drive letter (no colon) means that drive's root; names match
    #     case-insensitively like the exe (fatx discovery is_save_name)
    $letter = $null
    foreach ($l in [char[]]'QRSTUVWXYZ') { if (-not (Test-Path "${l}:\")) { $letter = "$l"; break } }
    if (-not $letter) { Skip 'usb bare drive letter' 'no free drive letter for subst' }
    else {
        Rename-Item (Join-Path $cdir 'CAREER_02') 'career_02'
        subst "${letter}:" $drive | Out-Null
        try {
            $r = Invoke-Converter @('-Usb', $letter, '-OutRoot', (Join-Path $fl 'out2'))
            $t = Join-Path $fl 'out2\CAREER_02\CAREER_02'
            if ($r.Code -eq 0 -and (Test-Path $t)) { Pass 'usb bare drive letter, lower-case name' }
            else { Fail 'usb bare drive letter, lower-case name' "exit $($r.Code): $($r.Text)" }
        } finally { subst "${letter}:" /D | Out-Null }
    }

    # --- backup folder for this second already taken -> <stamp>-2
    $bc = New-TempDir; $tmpRoots += $bc
    $bcRoot = Join-Path $bc 'NFS ProStreet'
    $bcOld = Join-Path $bcRoot 'CAREER_02\CAREER_02'
    New-Item -ItemType Directory -Force -Path (Split-Path $bcOld) | Out-Null
    [System.IO.File]::WriteAllBytes($bcOld, [byte[]](9, 9))
    $now = [DateTime]::UtcNow
    foreach ($sec in 0..20) {
        $st = $now.AddSeconds($sec).ToString('yyyy-MM-dd_HH-mm-ss', [System.Globalization.CultureInfo]::InvariantCulture)
        New-Item -ItemType Directory -Force -Path (Join-Path $bc "SaveConverter backups\$st\CAREER_02") | Out-Null
        [System.IO.File]::WriteAllBytes((Join-Path $bc "SaveConverter backups\$st\CAREER_02\CAREER_02"), [byte[]](7))
    }
    $r = Invoke-Converter @($pair, '-OutRoot', $bcRoot)
    $second = @(Get-ChildItem -Directory (Join-Path $bc 'SaveConverter backups') | Where-Object { $_.Name -like '*-2' })
    $firstIntact = @(Get-ChildItem -Recurse -File (Join-Path $bc 'SaveConverter backups') | Where-Object { $_.Length -eq 1 }).Count -eq 21
    $okSecond = $second.Count -eq 1 -and (Get-Item (Join-Path $second[0].FullName 'CAREER_02\CAREER_02')).Length -eq 2
    if ($r.Code -eq 0 -and $okSecond -and $firstIntact) { Pass 'backup collision goes to <stamp>-2' }
    else { Fail 'backup collision goes to <stamp>-2' "exit $($r.Code), -2 dirs $($second.Count), earlier intact $firstIntact" }

    # --- two sources with the same container name: second refused (exe batch.rs)
    $dup = New-TempDir; $tmpRoots += $dup
    $raceday = Join-Path $repo 'docs\re\pair_raceday\CAREER_02_360'
    $r = Invoke-Converter @($pair, $raceday, '-OutRoot', $dup)
    $t = Join-Path $dup 'CAREER_02\CAREER_02'
    if ($r.Code -eq 1 -and (Test-Path $t) -and (Get-Md5Hex $t) -eq '3da9f4c0a5a2b7d5c55863d49de4852c' -and $r.Text -match 'also named') { Pass 'duplicate container name refused' }
    else { Fail 'duplicate container name refused' "exit $($r.Code): $($r.Text)" }

    # --- usb root with no saves fails
    $empty = New-TempDir; $tmpRoots += $empty
    $r = Invoke-Converter @('-Usb', $empty, '-OutRoot', (Join-Path $empty 'out'))
    if ($r.Code -ne 0) { Pass 'usb with no saves fails' } else { Fail 'usb with no saves fails' 'exit 0' }

    # --- folder input: walked recursively; only CAREER_/ALIAS_ files that start
    #     with "CON " are picked (CAREER_99 is junk, notes.txt is not a save name)
    $fi = New-TempDir; $tmpRoots += $fi
    $fiIn = Join-Path $fi 'in'
    New-Item -ItemType Directory -Force -Path (Join-Path $fiIn 'sub\deeper') | Out-Null
    Copy-Item $pair (Join-Path $fiIn 'sub\deeper\CAREER_02')
    [System.IO.File]::WriteAllText((Join-Path $fiIn 'sub\notes.txt'), 'not a save')
    [System.IO.File]::WriteAllBytes((Join-Path $fiIn 'sub\CAREER_99'), [byte[]](0..255))
    # a copy inside a "SaveConverter backups" folder must be skipped (would be a duplicate name)
    New-Item -ItemType Directory -Force -Path (Join-Path $fiIn 'SaveConverter backups\2026-01-01_00-00-00\CAREER_02') | Out-Null
    Copy-Item $pair (Join-Path $fiIn 'SaveConverter backups\2026-01-01_00-00-00\CAREER_02\CAREER_02')
    $fiOut = Join-Path $fi 'out'
    $r = Invoke-Converter @($fiIn, '-OutRoot', $fiOut)
    $t = Join-Path $fiOut 'CAREER_02\CAREER_02'
    $no99 = -not (Test-Path (Join-Path $fiOut 'CAREER_99'))
    if ($r.Code -eq 0 -and (Test-Path $t) -and (Get-Md5Hex $t) -eq '3da9f4c0a5a2b7d5c55863d49de4852c' -and $no99 -and $r.Text -notmatch 'CAREER_99') { Pass 'folder input picks CON saves only' }
    else { Fail 'folder input picks CON saves only' "exit $($r.Code), no99 $no99 : $($r.Text)" }

    # --- folder with no saves: exit 1 with a message
    $fe = New-TempDir; $tmpRoots += $fe
    [System.IO.File]::WriteAllText((Join-Path $fe 'readme.txt'), 'nothing here')
    [System.IO.File]::WriteAllBytes((Join-Path $fe 'CAREER_77'), [byte[]](0..255))
    $r = Invoke-Converter @($fe, '-OutRoot', (Join-Path $fe 'out'))
    if ($r.Code -eq 1 -and $r.Text -match 'no saves') { Pass 'folder with no saves exits 1 with a message' }
    else { Fail 'folder with no saves exits 1 with a message' "exit $($r.Code): $($r.Text)" }

    # --- review follow-ups (scripts-cli-redesign; same cases in tests/test_cli.py)
    $rf = New-TempDir; $tmpRoots += $rf
    $rfIn = Join-Path $rf 'in'
    New-Item -ItemType Directory -Force -Path (Join-Path $rfIn 'sub') | Out-Null
    Copy-Item $pair (Join-Path $rfIn 'sub\CAREER_02')
    Copy-Item (Join-Path $repo 'docs\re\c1_latest\CAREER_01_360') (Join-Path $rfIn 'CAREER_01')
    $r = Invoke-Converter @($rfIn, '-OutRoot', (Join-Path $rf 'o1'))
    $okA = (Test-Path (Join-Path $rf 'o1\CAREER_02\CAREER_02')) -and (Test-Path (Join-Path $rf 'o1\CAREER_01\CAREER_01'))
    if ($r.Code -eq 0 -and $okA) { Pass 'folder with several saves' } else { Fail 'folder with several saves' "exit $($r.Code): $($r.Text)" }

    $r = Invoke-Converter @($rfIn, (Join-Path $rfIn 'sub'), (Join-Path $rfIn 'sub\CAREER_02'), '-OutRoot', (Join-Path $rf 'o2'))
    if ($r.Code -eq 0 -and -not (Test-Path (Join-Path $rf 'o2\SaveConverter backups'))) { Pass 'overlapping inputs convert once' }
    else { Fail 'overlapping inputs convert once' "exit $($r.Code): $($r.Text)" }

    $anyName = Join-Path $rf 'my save.bin'
    Copy-Item $pair $anyName
    $r = Invoke-Converter @($anyName, '-OutRoot', (Join-Path $rf 'o3'))
    $t = Join-Path $rf 'o3\CAREER_02\CAREER_02'
    if ($r.Code -eq 0 -and (Test-Path $t) -and (Get-Md5Hex $t) -eq '3da9f4c0a5a2b7d5c55863d49de4852c') { Pass 'file input with any name' }
    else { Fail 'file input with any name' "exit $($r.Code): $($r.Text)" }

    $r = Invoke-Converter @($pair, '-OutRoot', $anyName)
    if ($r.Code -eq 2) { Pass '-OutRoot that is a file is a usage error' } else { Fail '-OutRoot that is a file is a usage error' "exit $($r.Code)" }

    # --- default output root is the current directory (plain mode)
    $cw = New-TempDir; $tmpRoots += $cw
    $r = Invoke-Converter @($pair) $cw
    $t = Join-Path $cw 'CAREER_02\CAREER_02'
    if ($r.Code -eq 0 -and (Test-Path $t) -and (Get-Md5Hex $t) -eq '3da9f4c0a5a2b7d5c55863d49de4852c' -and $r.Text -match 'output folder') { Pass 'default output is the current directory' }
    else { Fail 'default output is the current directory' "exit $($r.Code): $($r.Text)" }

    # --- game folder detection: R\SAVE\NFS ProStreet, R\NFS ProStreet, R named NFS ProStreet
    $gm = New-TempDir; $tmpRoots += $gm
    $g1 = Join-Path $gm 'game'
    New-Item -ItemType Directory -Force -Path (Join-Path $g1 'SAVE\NFS ProStreet') | Out-Null
    $r = Invoke-Converter @($pair, '-OutRoot', $g1)
    if ($r.Code -eq 0 -and (Test-Path (Join-Path $g1 'SAVE\NFS ProStreet\CAREER_02\CAREER_02')) -and $r.Text -match 'game save folder') { Pass 'game folder detected (R\SAVE\NFS ProStreet)' }
    else { Fail 'game folder detected (R\SAVE\NFS ProStreet)' "exit $($r.Code): $($r.Text)" }

    $g2 = Join-Path $gm 'savedir'
    New-Item -ItemType Directory -Force -Path (Join-Path $g2 'NFS ProStreet') | Out-Null
    $r = Invoke-Converter @($pair, '-OutRoot', $g2)
    if ($r.Code -eq 0 -and (Test-Path (Join-Path $g2 'NFS ProStreet\CAREER_02\CAREER_02')) -and -not (Test-Path (Join-Path $g2 'CAREER_02')) -and $r.Text -match 'game save folder') { Pass 'game folder detected (R\NFS ProStreet)' }
    else { Fail 'game folder detected (R\NFS ProStreet)' "exit $($r.Code): $($r.Text)" }

    $g3 = Join-Path $gm 'x\NFS ProStreet'
    New-Item -ItemType Directory -Force -Path $g3 | Out-Null
    $r = Invoke-Converter @($pair, '-OutRoot', $g3)
    if ($r.Code -eq 0 -and (Test-Path (Join-Path $g3 'CAREER_02\CAREER_02')) -and $r.Text -match 'game save folder') { Pass 'game folder detected (R named NFS ProStreet)' }
    else { Fail 'game folder detected (R named NFS ProStreet)' "exit $($r.Code): $($r.Text)" }

    # --- plain mode backup stays inside R, nothing written to R's parent
    $pb = New-TempDir; $tmpRoots += $pb
    $pbR = Join-Path $pb 'plain'
    $pbOld = Join-Path $pbR 'CAREER_02\CAREER_02'
    New-Item -ItemType Directory -Force -Path (Split-Path $pbOld) | Out-Null
    [System.IO.File]::WriteAllBytes($pbOld, [byte[]](5, 6, 7))
    $r = Invoke-Converter @($pair, '-OutRoot', $pbR)
    $inside = @(Get-ChildItem -Recurse -File (Join-Path $pbR 'SaveConverter backups') -ErrorAction SilentlyContinue)
    $parentClean = -not (Test-Path (Join-Path $pb 'SaveConverter backups'))
    if ($r.Code -eq 0 -and $inside.Count -eq 1 -and $inside[0].Length -eq 3 -and $parentClean -and (Get-Md5Hex $pbOld) -eq '3da9f4c0a5a2b7d5c55863d49de4852c') { Pass 'plain mode backs up inside R' }
    else { Fail 'plain mode backs up inside R' "exit $($r.Code), inside $($inside.Count), parentClean $parentClean : $($r.Text)" }

    # --- missing -OutRoot is created; not on a dry run
    $oc = New-TempDir; $tmpRoots += $oc
    $ocR = Join-Path $oc 'a\b\new'
    $r = Invoke-Converter @($pair, '-OutRoot', $ocR)
    if ($r.Code -eq 0 -and (Test-Path (Join-Path $ocR 'CAREER_02\CAREER_02'))) { Pass '-OutRoot created when missing' }
    else { Fail '-OutRoot created when missing' "exit $($r.Code): $($r.Text)" }
    $ocD = Join-Path $oc 'dry\new'
    $r = Invoke-Converter @($pair, '-OutRoot', $ocD, '-DryRun')
    if ($r.Code -eq 0 -and -not (Test-Path (Join-Path $oc 'dry'))) { Pass '-OutRoot not created on dry run' }
    else { Fail '-OutRoot not created on dry run' "exit $($r.Code): $($r.Text)" }
    $ocF = Join-Path $oc 'fail\new'
    $r = Invoke-Converter @((Join-Path $oc 'does-not-exist'), '-OutRoot', $ocF)
    if ($r.Code -eq 1 -and -not (Test-Path (Join-Path $oc 'fail'))) { Pass '-OutRoot not created when every source fails' }
    else { Fail '-OutRoot not created when every source fails' "exit $($r.Code): $($r.Text)" }

    # --- an empty -Usb is one failure; the other inputs still convert
    $ue = New-TempDir; $tmpRoots += $ue
    $r = Invoke-Converter @($pair, '-Usb', (Join-Path $ue 'stick'), '-OutRoot', (Join-Path $ue 'out'))
    if ($r.Code -eq 1 -and (Test-Path (Join-Path $ue 'out\CAREER_02\CAREER_02'))) { Pass 'empty -Usb does not stop other inputs' }
    else { Fail 'empty -Usb does not stop other inputs' "exit $($r.Code): $($r.Text)" }

    # --- bad inputs: nonzero exit, nothing written
    $bad = New-TempDir; $tmpRoots += $bad
    $junk = Join-Path $bad 'CAREER_09'
    [System.IO.File]::WriteAllBytes($junk, [byte[]](0..255))
    $badOut = Join-Path $bad 'out'
    $r = Invoke-Converter @($junk, '-OutRoot', $badOut)
    if ($r.Code -ne 0 -and -not (Test-Path $badOut\CAREER_09)) { Pass 'non-container rejected' } else { Fail 'non-container rejected' "exit $($r.Code)" }

    $trunc = Join-Path $bad 'CAREER_08'
    $all = [System.IO.File]::ReadAllBytes($pair)
    [System.IO.File]::WriteAllBytes($trunc, $all[0..0x3FFF])
    $r = Invoke-Converter @($trunc, '-OutRoot', $badOut)
    if ($r.Code -ne 0 -and -not (Test-Path $badOut\CAREER_08)) { Pass 'truncated container rejected' } else { Fail 'truncated container rejected' "exit $($r.Code)" }

    $r = Invoke-Converter @((Join-Path $bad 'does-not-exist'), '-OutRoot', $badOut)
    if ($r.Code -ne 0) { Pass 'missing source rejected' } else { Fail 'missing source rejected' 'exit 0' }

    $r = Invoke-Converter @('-OutRoot', $badOut)
    if ($r.Code -eq 2) { Pass 'no source is a usage error (exit 2)' } else { Fail 'no source is a usage error (exit 2)' "exit $($r.Code)" }

    # --- one bad source among good ones: good converts, exit 1
    $mixOut = Join-Path $bad 'mix'
    $r = Invoke-Converter @($junk, $pair, '-OutRoot', $mixOut)
    if ($r.Code -eq 1 -and (Test-Path (Join-Path $mixOut 'CAREER_02\CAREER_02'))) { Pass 'partial failure converts the rest, exit 1' }
    else { Fail 'partial failure converts the rest, exit 1' "exit $($r.Code)" }

    # --- unit: _to_pc_record on a 1-3 byte payload -> payload[4:] + the tail word
    #     (Python grows it to 4 bytes; no committed save has one). Runs the
    #     converter in-process (dry run) so the NfsPs.Save type is loaded here.
    & $converter $pair -OutRoot $bad -DryRun *> $null
    $m = [NfsPs.Save].GetMethod('ToPcRecord', [System.Reflection.BindingFlags]'NonPublic,Public,Static')
    $rec = New-Object NfsPs.Rec
    $rec.Payload = [byte[]](1, 2)
    $threw = $null
    try { [void]$m.Invoke($null, [object[]]@($rec.PSObject.BaseObject, [byte[]](9, 8, 7, 6))) } catch { $threw = $_.Exception.InnerException.Message }
    if (-not $threw -and ($rec.Payload -join ',') -eq '9,8,7,6') { Pass 'short record payload framed like Python' }
    else { Fail 'short record payload framed like Python' "threw '$threw', payload $($rec.Payload -join ',')" }

    # --- unit: RehashGameplay on a payload shorter than the digest slot
    #     (vector from test_short_records.rs rehash_grows_tiny_gameplay_payload:
    #     Python's bytearray slice assignment grows it to 0x24 = md5 of empty)
    $flags = [System.Reflection.BindingFlags]'NonPublic,Public,Static'
    $toHex = { param([byte[]]$b) (($b | ForEach-Object { $_.ToString('x2') }) -join '') }
    $m = [NfsPs.Save].GetMethod('RehashGameplay', $flags)
    $rec = New-Object NfsPs.Rec
    $rec.Id = [NfsPs.Save].GetField('GameplayId', $flags).GetValue($null)
    $rec.Payload = [byte[]](0..0x1F)
    $threw = $null
    try { [void]$m.Invoke($null, [object[]]@($rec.PSObject.BaseObject)) } catch { $threw = $_.Exception.InnerException.Message }
    $got = & $toHex $rec.Payload
    if (-not $threw -and $got -eq '000102030405060708090a0b0c0d0e0f10111213d41d8cd98f00b204e9800998ecf8427e') { Pass 'tiny GameplayData rehash grows like Python' }
    else { Fail 'tiny GameplayData rehash grows like Python' "threw '$threw', payload $got" }

    # --- unit: CARDB struct fixes on a 0x100-byte payload (vectors from
    #     test_short_records.rs cardb_fixes_clamp_on_short_payload)
    $fixParts = [NfsPs.Save].GetMethod('FixCarDbParts', $flags)
    $fixPacked = [NfsPs.Save].GetMethod('FixCarDbPacked', $flags)
    $src = [byte[]](0..255)
    $threw = $null; $idOk = $false; $got = $null
    try {
        $o = [byte[]]$src.Clone()
        [void]$fixParts.Invoke($null, [object[]]@($src, $o)); [void]$fixPacked.Invoke($null, [object[]]@($src, $o))
        $idOk = (& $toHex $o) -eq (& $toHex $src)
        $o = [byte[]]$src.Clone()
        for ($w = 0; $w -lt $o.Length; $w += 4) { [Array]::Reverse($o, $w, 4) }
        [void]$fixParts.Invoke($null, [object[]]@($src, $o)); [void]$fixPacked.Invoke($null, [object[]]@($src, $o))
        $got = & $toHex $o
    } catch { $threw = $_.Exception.InnerException.Message }
    $want = '03020100070605040b0a09080f0e0d0c13121110171615141b1a19181f1e1d1c23222120272625242b2a29282c2d2e2f33323130373635343b3a39383f3e3d3c43424140444546474b4a49484f4e4d4c53525150575655545b5a59585c5d5e5f63626160676665646b6a69686f6e6d6c73727170747576777b7a79787f7e7d7c83828180878685848b8a89888c8d8e8f93929190979695949b9a99989f9e9d9ca3a2a1a0a4a5a6a7abaaa9a8afaeadacb3b2b1b0b7b6b5b4bbbab9b8bcbdbebfc3c2c1c0c7c6c5c4cbcac9c8cfcecdccd3d2d1d0d4d5d6d7dbdad9d8dfdedddce3e2e1e0e7e6e5e4ebeae9e8ecedeeeff3f2f1f0f7f6f5f4fbfaf9f8fffefdfc'
    if (-not $threw -and $idOk -and $got -eq $want) { Pass 'CARDB fixes clamp on a 0x100-byte payload like Python' }
    else { Fail 'CARDB fixes clamp on a 0x100-byte payload like Python' "threw '$threw', identity $idOk, swapped $got" }

    # --- unit: ConvertExtra on a 64-byte alias extra whose name has no NUL
    #     (same vector as tests/test_extra.py; golden aliases all terminate it)
    $x = [byte[]]::new(64)   # New-Object would hand Invoke a PSObject wrapper
    for ($i = 0; $i -lt 0x14; $i++) { $x[$i] = $i + 1 }
    $nm = [System.Text.Encoding]::ASCII.GetBytes('ANONYMOUS 1')
    [Array]::Copy($nm, 0, $x, 0x14, $nm.Length)
    for ($i = 0x14 + $nm.Length; $i -lt 0x38; $i++) { $x[$i] = 0xAA }
    $tail = [byte[]](0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88)
    [Array]::Copy($tail, 0, $x, 0x38, 8)
    $want = [byte[]]$x.Clone()
    foreach ($w in @(0, 4, 8, 12, 16, 0x38, 0x3C)) { [Array]::Reverse($want, $w, 4) }
    $m = [NfsPs.Save].GetMethod('ConvertExtra', [System.Reflection.BindingFlags]'NonPublic,Public,Static')
    $got = $m.Invoke($null, [object[]]@(, $x))
    if ((($got | ForEach-Object { $_.ToString('x2') }) -join '') -eq (($want | ForEach-Object { $_.ToString('x2') }) -join '')) { Pass 'alias extra without NUL converted like Python' }
    else { Fail 'alias extra without NUL converted like Python' "got $(($got | ForEach-Object { $_.ToString('x2') }) -join '')" }

    # --- crafted short records (same fixtures and pins as
    #     src/crates/nfssave-core/tests/test_short_records.rs, md5s from the Python):
    #     the tracked CAREER_01_360 with one record's payload replaced, tree
    #     rebuilt big-endian, MC02 header CRCs recomputed
    $crcM = [NfsPs.Save].GetMethod('Crc', [type[]]@([byte[]], [int], [int]))
    function New-ShortRecordMc02([uint32]$recId, [byte[]]$payload) {
        $mc = [NfsPs.Save]::ParseContainer([System.IO.File]::ReadAllBytes($c1), 'c1').Payload
        $rd = { param([byte[]]$b, [int]$o) ([uint32]$b[$o] -shl 24) -bor ([uint32]$b[$o + 1] -shl 16) -bor ([uint32]$b[$o + 2] -shl 8) -bor [uint32]$b[$o + 3] }
        $sub = { param([byte[]]$b, [int]$at, [int]$n) $r = [byte[]]::new($n); [Array]::Copy($b, $at, $r, 0, $n); , $r }
        $wr = { param([byte[]]$b, [int]$o, [uint32]$v) for ($k = 0; $k -lt 4; $k++) { $b[$o + $k] = [byte](($v -shr (24 - 8 * $k)) -band 0xFF) } }
        $extraSize = [int](& $rd $mc 8); $treeSize = [int](& $rd $mc 12)
        $tree = & $sub $mc (0x1C + $extraSize) ($mc.Length - 0x1C - $extraSize)
        $magic = 0x14; while ((& $rd $tree $magic) -ne 0x59F2D89B) { $magic += 4 }
        $end = 0x48 + [int](& $rd $tree ($magic + 4))
        $body = New-Object System.IO.MemoryStream
        $o = 0x48
        while ($o + 12 -le $end) {
            $size = [int](& $rd $tree ($o + 8))
            $p = if ((& $rd $tree ($o + 4)) -eq $recId) { $payload } else { & $sub $tree ($o + 12) $size }
            $h = & $sub $tree $o 12; & $wr $h 8 $p.Length
            $body.Write($h, 0, 12); $body.Write($p, 0, $p.Length)
            $o += 12 + $size
        }
        $nt = [byte[]]::new($treeSize)
        [Array]::Copy($tree, $nt, 0x48)
        & $wr $nt ($magic + 4) ([uint32]$body.Length)
        $b = $body.ToArray(); [Array]::Copy($b, 0, $nt, 0x48, $b.Length)
        $post = [Math]::Min($tree.Length - $end, $treeSize - 0x48 - $b.Length)
        if ($post -gt 0) { [Array]::Copy($tree, $end, $nt, 0x48 + $b.Length, $post) }
        $out = [byte[]]::new(0x1C + $extraSize + $treeSize)
        [Array]::Copy($mc, $out, 0x1C + $extraSize)
        [Array]::Copy($nt, 0, $out, 0x1C + $extraSize, $treeSize)
        & $wr $out 4 ([uint32]$out.Length)
        & $wr $out 0x14 ([uint32]$crcM.Invoke($null, [object[]]@($nt, 0, $treeSize)))
        & $wr $out 0x18 ([uint32]$crcM.Invoke($null, [object[]]@($out, 0, 0x18)))
        , $out
    }
    function Get-BytesMd5([byte[]]$b) {
        $md5 = [System.Security.Cryptography.MD5]::Create()
        try { (($md5.ComputeHash($b) | ForEach-Object { $_.ToString('x2') }) -join '') } finally { $md5.Dispose() }
    }
    # Convert-One's tail (tree hash + MC02 build) without the container/CLI layer
    function Convert-Mc02([byte[]]$mc) {
        $rules = [NfsPs.Save]::LoadRules([System.IO.File]::ReadAllText((Join-Path $here '..\fieldmaps.rules')))
        $res = [NfsPs.Save]::ConvertSave($mc, $rules)
        [Array]::Copy((Get-TreeHash $res.Tree), 0, $res.Tree, 0, 16)
        , [NfsPs.Save]::BuildMc02($res.Extra, $res.Tree, $res.TreeSize)
    }
    # the tree hash lives in converter-script functions/variables, not the C#
    # type: define them here from the converter's own top-level statements
    $ast = [System.Management.Automation.Language.Parser]::ParseFile((Resolve-Path $converter).Path, [ref]$null, [ref]$null)
    foreach ($st in $ast.EndBlock.Statements) {
        $isFn = $st -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $st.Name -in @('ConvertFrom-HexString', 'Get-TreeHash')
        $isKey = $st -is [System.Management.Automation.Language.AssignmentStatementAst] -and "$($st.Left)" -match '^\$script:Tree[EN]$'
        if ($isFn -or $isKey) { Invoke-Expression $st.Extent.Text }
    }

    $cardb = New-ShortRecordMc02 0x47A07113 ([byte[]](0..255))
    $fx = Get-BytesMd5 $cardb
    if ($fx -ne '5fe66a67dffda37ec3e632f52c67b7b0') { Fail 'short CARDB record converts like Python' "fixture drift: $fx" }
    else {
        $got = $null
        try { $got = Get-BytesMd5 (Convert-Mc02 $cardb) } catch { $got = "threw: $($_.Exception.Message)" }
        if ($got -eq 'c4713f6bb58e63b393afc8cfa35bec58') { Pass 'short CARDB record converts like Python' }
        else { Fail 'short CARDB record converts like Python' "got $got" }
    }

    $gp = New-ShortRecordMc02 0x3B309E09 ([byte[]](@(0x11) * 0x2D8))
    $fx = Get-BytesMd5 $gp
    if ($fx -ne 'acd56e6ada2458d1efc25a22e6876ae7') { Fail 'GameplayData below race-day state refused' "fixture drift: $fx" }
    else {
        $msg = $null
        try { [void](Convert-Mc02 $gp) } catch { $msg = $_.Exception.Message }
        if ($msg -match 'GameplayData chunk too short \(0x2d8 B\) .*source file is corrupted') { Pass 'GameplayData below race-day state refused' }
        else { Fail 'GameplayData below race-day state refused' "got '$msg'" }
    }

    # --- internal gap (same fixture and pin as tests/test_gap.py and
    #     tests/test_gap.rs): record 0xD548266C overwritten with 0xAA, the
    #     word after the record area set to 7. The last record before the
    #     noise gets no spill; the re-anchored records, the last one included,
    #     convert as in a clean tree.
    function New-InternalGapMc02([uint32]$recId, [byte[]]$postWord) {
        $mc = [NfsPs.Save]::ParseContainer([System.IO.File]::ReadAllBytes($c1), 'c1').Payload
        $rd = { param([byte[]]$b, [int]$o) ([uint32]$b[$o] -shl 24) -bor ([uint32]$b[$o + 1] -shl 16) -bor ([uint32]$b[$o + 2] -shl 8) -bor [uint32]$b[$o + 3] }
        $wr = { param([byte[]]$b, [int]$o, [uint32]$v) for ($k = 0; $k -lt 4; $k++) { $b[$o + $k] = [byte](($v -shr (24 - 8 * $k)) -band 0xFF) } }
        $extraSize = [int](& $rd $mc 8); $treeSize = [int](& $rd $mc 12)
        $t0 = 0x1C + $extraSize
        $out = [byte[]]$mc.Clone()
        $magic = 0x14; while ((& $rd $out ($t0 + $magic)) -ne 0x59F2D89B) { $magic += 4 }
        $end = 0x48 + [int](& $rd $out ($t0 + $magic + 4))
        $o = 0x48
        while ($o + 12 -le $end) {
            $size = [int](& $rd $out ($t0 + $o + 8))
            if ((& $rd $out ($t0 + $o + 4)) -eq $recId) { for ($k = 0; $k -lt 12 + $size; $k++) { $out[$t0 + $o + $k] = 0xAA } }
            $o += 12 + $size
        }
        if ($postWord) { [Array]::Copy($postWord, 0, $out, $t0 + $end, 4) }
        & $wr $out 0x14 ([uint32]$crcM.Invoke($null, [object[]]@($out, $t0, $treeSize)))
        & $wr $out 0x18 ([uint32]$crcM.Invoke($null, [object[]]@($out, 0, 0x18)))
        , $out
    }
    $ig = New-InternalGapMc02 ([Convert]::ToUInt32('D548266C', 16)) ([byte[]](0, 0, 0, 7))
    $fx = Get-BytesMd5 $ig
    if ($fx -ne 'bacda5eeb061896b540221dc26fb8aa8') { Fail 'internal-gap career converts like Python' "fixture drift: $fx" }
    else {
        $got = $null
        try { $got = Get-BytesMd5 (Convert-Mc02 $ig) } catch { $got = "threw: $($_.Exception.Message)" }
        if ($got -eq '743068fdd49685367041faa4df7c3903') { Pass 'internal-gap career converts like Python' }
        else { Fail 'internal-gap career converts like Python' "got $got" }
    }
    # --- trailing gap (tests/test_gap.py build_gapped() default): the last
    #     record overwritten, nothing to re-anchor; pin unchanged since twin removal
    $tg = New-InternalGapMc02 ([Convert]::ToUInt32('CA269650', 16)) $null
    $fx = Get-BytesMd5 $tg
    if ($fx -ne 'c57cfaefded23cd1d2b3ed9c01b296aa') { Fail 'trailing-gap career converts like Python' "fixture drift: $fx" }
    else {
        $got = $null
        try { $got = Get-BytesMd5 (Convert-Mc02 $tg) } catch { $got = "threw: $($_.Exception.Message)" }
        if ($got -eq 'd00a8fdce99bea2068efdfa961451f6d') { Pass 'trailing-gap career converts like Python' }
        else { Fail 'trailing-gap career converts like Python' "got $got" }
    }

    # --- unit: scalar_tail / fix_node_flags u8 rule (vectors of tests/test_alias_settings.py)
    function BeWords([uint32[]]$ws) {
        $b = [byte[]]::new(4 * $ws.Count)
        for ($i = 0; $i -lt $ws.Count; $i++) { $v = [BitConverter]::GetBytes($ws[$i]); [Array]::Reverse($v); [Array]::Copy($v, 0, $b, 4 * $i, 4) }
        , $b
    }
    function Hx([byte[]]$b) { ($b | ForEach-Object { $_.ToString('x2') }) -join '' }
    $at = [NfsPs.Save].GetMethod('ScalarTail', [System.Reflection.BindingFlags]'NonPublic,Public,Static')
    $cases = @(
        @((BeWords @(0, 4, 0x00FFFFFF)), [byte[]](0, 0, 0, 3), '03000000', 'u32 swap'),
        @((BeWords @(0, 1, 0x00FFFFFF)), [byte[]](1, 0, 0, 0), '01000000', 'u8 natural'),
        @((BeWords @(0, 1, 0x00FFFFFF)), [byte[]](0, 0, 0, 4), '04000000', 'u8 pad set: swap'),
        @((BeWords @(0, 5, 0x00FFFFFF)), [byte[]](0, 0, 0, 3), '00000000', 'len 5 not scalar at end'),
        @((BeWords @(0, 8, 0x00FFFFFF, 0)), [byte[]](0x3F, 0x80, 0, 0), '0000803f', '8-byte node tail'),
        @((BeWords @(0, 4, 0x01234567)), [byte[]](0, 0, 0, 3), '00000000', 'junk flag word'),
        @((BeWords @(4, 0x00FFFFFF)), [byte[]](0, 0, 0, 3), '00000000', 'payload < 12'),
        @((BeWords @(0, 0, [uint32]::MaxValue)), [byte[]](0, 0, 0, 3), '00000000', 'len 0'),
        @((BeWords @(0, 4, 0x00FFFFFF)), [byte[]](0, 3), '00000000', 'truncated tail'))
    $bad_cases = @()
    foreach ($c in $cases) {
        $got = Hx ($at.Invoke($null, [object[]]@($c[0], $c[1])))
        if ($got -ne $c[2]) { $bad_cases += "$($c[3]): $got" }
    }
    if (-not $bad_cases) { Pass 'scalar_tail rules like Python' } else { Fail 'scalar_tail rules like Python' ($bad_cases -join '; ') }
    $fx = [NfsPs.Save].GetMethod('FixNodeFlags', [System.Reflection.BindingFlags]'NonPublic,Public,Static')
    $fails = @()
    foreach ($c in @(@(0x00FFFFFF, 0x01000000, '01000000'), @(0x00FFFFFF, 4, '04000000'), @(0x01234567, 0x01000000, '00000001'))) {
        $src = BeWords @(0, 1, $c[0], $c[1])
        $o = [byte[]]$src.Clone()
        for ($w = 0; $w -lt 16; $w += 4) { [Array]::Reverse($o, $w, 4) }
        [void]$fx.Invoke($null, [object[]]@($src, $o))
        $got = Hx $o[12..15]
        if ($got -ne $c[2]) { $fails += "flag $($c[0].ToString('x8')) data $($c[1].ToString('x8')): $got" }
    }
    if (-not $fails) { Pass 'u8 node rule like Python' } else { Fail 'u8 node rule like Python' ($fails -join '; ') }

    # --- unit: embedded PCControllerSettings defaults == the shared data file
    $pcd = [NfsPs.Save].GetField('PcControllerDefault', [System.Reflection.BindingFlags]'NonPublic,Public,Static').GetValue($null)
    $want = [System.IO.File]::ReadAllBytes((Join-Path $here '..\..\python\nfssave\pc_controller_default.bin'))
    if ((Hx $pcd) -eq (Hx $want)) { Pass 'PCControllerSettings defaults match the data file' }
    else { Fail 'PCControllerSettings defaults match the data file' "len $($pcd.Length) vs $($want.Length)" }
}
finally {
    foreach ($t in $tmpRoots) { Remove-Item -Recurse -Force $t -ErrorAction SilentlyContinue }
}

Write-Host ("{0} passed, {1} failed, {2} skipped" -f $script:passed, $script:failed, $script:skipped)
if ($script:failed) { exit 1 } else { exit 0 }
