param([int]$Words = 2048, [string]$Base = '$esp')
# Dump raw stack words from a live Visual Studio debug session (EnvDTE), one
# "address value" line per word, so return addresses can be grepped:
#   powershell.exe -File docs/re/vsstack.ps1 -Words 4096 | Select-String ' 0056b9'
# Windows PowerShell 5.1 only (see vsdbg.ps1).
$d = [Runtime.InteropServices.Marshal]::GetActiveObject("VisualStudio.DTE.18.0")
$dbg = $d.Debugger
$sp = [uint32]($dbg.GetExpression("(unsigned int)$Base", $true, 2000).Value)
$chunk = 256
for ($off = 0; $off -lt $Words; $off += $chunk) {
    $addr = $sp + 4 * $off
    $r = $dbg.GetExpression("*(unsigned int(*)[$chunk])$addr,x", $true, 10000)
    $i = 0
    foreach ($m in $r.DataMembers) {
        "{0:x8} {1}" -f ($addr + 4 * $i), $m.Value
        $i++
    }
}
