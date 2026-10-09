param([Parameter(Mandatory)][string]$Addr, [int]$Words = 64)
# Hex-dump u32 words at an address (any debugger expression) from a live
# Visual Studio debug session. Windows PowerShell 5.1 only (see vsdbg.ps1).
#   powershell.exe -File docs/re/vsmem.ps1 -Addr '*(unsigned int*)0xab9dc8' -Words 64
$d = [Runtime.InteropServices.Marshal]::GetActiveObject("VisualStudio.DTE.18.0")
$dbg = $d.Debugger
$base = [uint32]($dbg.GetExpression("(unsigned int)($Addr)", $true, 2000).Value)
$r = $dbg.GetExpression("*(unsigned int(*)[$Words])$base,x", $true, 10000)
$i = 0; $line = ""
foreach ($m in $r.DataMembers) {
    if ($i % 4 -eq 0) { if ($line) { $line }; $line = "{0:x8} +{1:x3}:" -f ($base + 4 * $i), (4 * $i) }
    $line += " " + ($m.Value -replace '^0x', '')
    $i++
}
if ($line) { $line }
