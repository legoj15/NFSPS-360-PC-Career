# Black-box tests for Convert-NfsSave.ps1. Runs the converter in a child
# process of the SAME PowerShell host as this runner, so:
#   powershell.exe -NoProfile -ExecutionPolicy Bypass -File Run-Tests.ps1   (5.1)
#   pwsh -NoProfile -File Run-Tests.ps1                                     (7.x)
# Golden md5 pins are shared with tests/test_golden.py and
# src/crates/nfssave-core/tests/test_golden.rs - keep all three identical.
# Sources under Extracted/ are personal saves (gitignored); absent ones skip.
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

function Invoke-Converter([string[]]$ArgList) {
    # 5.1 turns native stderr into ErrorRecords; under 'Stop' that would throw
    $ErrorActionPreference = 'Continue'
    $out = & $hostExe -NoProfile -ExecutionPolicy Bypass -File $converter @ArgList 2>&1
    return @{ Code = $LASTEXITCODE; Text = (($out | ForEach-Object { "$_" }) -join "`n") }
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
    @('docs\re\pair_raceday\CAREER_02_360', '2bb7d00963509e71d6eaeccbed496b65', 'CAREER_02')
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
        $name = Split-Path -Leaf $src
        if (-not (Test-Path $src -PathType Leaf)) { Skip "golden $name" 'source not present'; continue }
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
    $r = Invoke-Converter @('-Flash', $drive, '-OutRoot', $flOut)
    $t = Join-Path $flOut 'CAREER_02\CAREER_02'
    if ($r.Code -eq 0 -and (Test-Path $t) -and (Get-Md5Hex $t) -eq '8dd15c6cb5736cf14aa2694289d8480d') { Pass 'flash drive walk' }
    else { Fail 'flash drive walk' "exit $($r.Code): $($r.Text)" }

    # --- bare drive letter (no colon) means that drive's root; names match
    #     case-insensitively like the exe (fatx discovery is_save_name)
    $letter = $null
    foreach ($l in [char[]]'QRSTUVWXYZ') { if (-not (Test-Path "${l}:\")) { $letter = "$l"; break } }
    if (-not $letter) { Skip 'flash bare drive letter' 'no free drive letter for subst' }
    else {
        Rename-Item (Join-Path $cdir 'CAREER_02') 'career_02'
        subst "${letter}:" $drive | Out-Null
        try {
            $r = Invoke-Converter @('-Flash', $letter, '-OutRoot', (Join-Path $fl 'out2'))
            $t = Join-Path $fl 'out2\CAREER_02\CAREER_02'
            if ($r.Code -eq 0 -and (Test-Path $t)) { Pass 'flash bare drive letter, lower-case name' }
            else { Fail 'flash bare drive letter, lower-case name' "exit $($r.Code): $($r.Text)" }
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

    # --- flash root with no saves fails
    $empty = New-TempDir; $tmpRoots += $empty
    $r = Invoke-Converter @('-Flash', $empty, '-OutRoot', (Join-Path $empty 'out'))
    if ($r.Code -ne 0) { Pass 'flash with no saves fails' } else { Fail 'flash with no saves fails' 'exit 0' }

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
}
finally {
    foreach ($t in $tmpRoots) { Remove-Item -Recurse -Force $t -ErrorAction SilentlyContinue }
}

Write-Host ("{0} passed, {1} failed, {2} skipped" -f $script:passed, $script:failed, $script:skipped)
if ($script:failed) { exit 1 } else { exit 0 }
