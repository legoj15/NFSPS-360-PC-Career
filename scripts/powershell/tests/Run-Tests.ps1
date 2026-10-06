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
    @('Extracted\Career\CAREER_01', '7b7e1893047b01b00f7037ef54ceca44', 'CAREER_01'),
    @('docs\re\pair\CAREER_02_360_fresh', '8dd15c6cb5736cf14aa2694289d8480d', 'CAREER_02'),
    @('Extracted\Career\CAREER_03', '0dcfed80eab3dfcc246499b07ae54c37', 'CAREER_03'),
    @('Extracted\Alias\ALIAS_360', '578a10cb583785bb6cb00fa64bc69439', 'ALIAS_JOSHUA S 10'),
    @('docs\re\c1_latest\CAREER_01_360', '718b6b6b8494decde59eb6b1defcc01d', 'CAREER_01'),
    @('docs\re\pair_raceday\CAREER_02_360', '2bb7d00963509e71d6eaeccbed496b65', 'CAREER_02'),
    # anonymized copy of the personal alias save (docs\re\alias_anon\README.md)
    @('docs\re\alias_anon\ALIAS_360', '8ae3d82a3c9cb1c9500d6fcce8c01b9d', 'ALIAS_ANONYMOUS 1')
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
    $replaced = (Get-Md5Hex $old) -eq '8dd15c6cb5736cf14aa2694289d8480d'
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
    if ($r.Code -eq 0 -and (Test-Path $t) -and (Get-Md5Hex $t) -eq '8dd15c6cb5736cf14aa2694289d8480d') { Pass 'usb drive walk' }
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
    if ($r.Code -eq 1 -and (Test-Path $t) -and (Get-Md5Hex $t) -eq '8dd15c6cb5736cf14aa2694289d8480d' -and $r.Text -match 'also named') { Pass 'duplicate container name refused' }
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
    if ($r.Code -eq 0 -and (Test-Path $t) -and (Get-Md5Hex $t) -eq '8dd15c6cb5736cf14aa2694289d8480d' -and $no99 -and $r.Text -notmatch 'CAREER_99') { Pass 'folder input picks CON saves only' }
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
    if ($r.Code -eq 0 -and (Test-Path $t) -and (Get-Md5Hex $t) -eq '8dd15c6cb5736cf14aa2694289d8480d') { Pass 'file input with any name' }
    else { Fail 'file input with any name' "exit $($r.Code): $($r.Text)" }

    $r = Invoke-Converter @($pair, '-OutRoot', $anyName)
    if ($r.Code -eq 2) { Pass '-OutRoot that is a file is a usage error' } else { Fail '-OutRoot that is a file is a usage error' "exit $($r.Code)" }

    # --- default output root is the current directory (plain mode)
    $cw = New-TempDir; $tmpRoots += $cw
    $r = Invoke-Converter @($pair) $cw
    $t = Join-Path $cw 'CAREER_02\CAREER_02'
    if ($r.Code -eq 0 -and (Test-Path $t) -and (Get-Md5Hex $t) -eq '8dd15c6cb5736cf14aa2694289d8480d' -and $r.Text -match 'output folder') { Pass 'default output is the current directory' }
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
    if ($r.Code -eq 0 -and $inside.Count -eq 1 -and $inside[0].Length -eq 3 -and $parentClean -and (Get-Md5Hex $pbOld) -eq '8dd15c6cb5736cf14aa2694289d8480d') { Pass 'plain mode backs up inside R' }
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

    # --- unit: _to_pc_record on a 1-3 byte payload -> payload[4:] + 4 zeros
    #     (Python grows it to 4 bytes; no committed save has one). Runs the
    #     converter in-process (dry run) so the NfsPs.Save type is loaded here.
    & $converter $pair -OutRoot $bad -DryRun *> $null
    $m = [NfsPs.Save].GetMethod('ToPcRecord', [System.Reflection.BindingFlags]'NonPublic,Public,Static')
    $rec = New-Object NfsPs.Rec
    $rec.Payload = [byte[]](1, 2)
    $threw = $null
    try { [void]$m.Invoke($null, [object[]]@($rec.PSObject.BaseObject)) } catch { $threw = $_.Exception.InnerException.Message }
    if (-not $threw -and $rec.Payload.Length -eq 4 -and -not ($rec.Payload | Where-Object { $_ })) { Pass 'short record payload framed like Python' }
    else { Fail 'short record payload framed like Python' "threw '$threw', payload $($rec.Payload -join ',')" }

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
}
finally {
    foreach ($t in $tmpRoots) { Remove-Item -Recurse -Force $t -ErrorAction SilentlyContinue }
}

Write-Host ("{0} passed, {1} failed, {2} skipped" -f $script:passed, $script:failed, $script:skipped)
if ($script:failed) { exit 1 } else { exit 0 }
