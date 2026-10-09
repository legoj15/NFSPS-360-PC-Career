param([Parameter(Mandatory)][string]$Addr, [int]$Bytes = 0x400, [Parameter(Mandatory)][string]$Out)
# Pause a live VS debug session, save raw process memory at $Addr (any
# expression) to $Out as binary, then resume if it was running. Used to read
# code that is only decrypted at runtime (copy-protected nfs.exe regions).
# Windows PowerShell 5.1 only (see vsdbg.ps1).
$d = [Runtime.InteropServices.Marshal]::GetActiveObject("VisualStudio.DTE.18.0")
$dbg = $d.Debugger
$wasRunning = $dbg.CurrentMode -eq 3   # dbgRunMode
if ($wasRunning) { $dbg.Break($true) }
try {
    $base = [uint32]($dbg.GetExpression("(unsigned int)($Addr)", $true, 2000).Value)
    $words = [int][math]::Ceiling($Bytes / 4)
    $r = $dbg.GetExpression("*(unsigned int(*)[$words])$base,x", $true, 20000)
    $buf = New-Object byte[] ($words * 4)
    $i = 0
    foreach ($m in $r.DataMembers) {
        $v = [Convert]::ToUInt32(($m.Value -replace '^0x', ''), 16)
        [BitConverter]::GetBytes($v).CopyTo($buf, 4 * $i)
        $i++
    }
    [IO.File]::WriteAllBytes($Out, $buf)
    "base=0x{0:x8} bytes={1} -> {2}" -f $base, $buf.Length, $Out
} finally {
    if ($wasRunning) { $dbg.Go($false) }
}
