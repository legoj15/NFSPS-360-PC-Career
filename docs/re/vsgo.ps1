param([string]$ExprList = '$eip,x;$eax,x;$ebx,x;$ecx,x;$edx,x;$esi,x;$edi,x;$ebp,x;$esp,x', [int]$TimeoutSec = 60)
# Resume a live VS debug session, wait for the next break (breakpoint), then
# print the break location and expressions. Windows PowerShell 5.1 only.
$d = [Runtime.InteropServices.Marshal]::GetActiveObject("VisualStudio.DTE.18.0")
$dbg = $d.Debugger
function Retry($sb) { for ($i = 0; $i -lt 40; $i++) { try { return & $sb } catch { if ($_.Exception.Message -notmatch 'REJECTED|busy') { throw }; Start-Sleep -Milliseconds 250 } } }
Retry { $dbg.Go($false) } | Out-Null
$t0 = Get-Date
do { Start-Sleep -Milliseconds 300; $m = Retry { $dbg.CurrentMode } } while ($m -ne 2 -and ((Get-Date) - $t0).TotalSeconds -lt $TimeoutSec)
if ($m -ne 2) { "still running after $TimeoutSec s (mode $m)"; exit 3 }
foreach ($e in ($ExprList -split ';' | Where-Object { $_ })) {
    $v = Retry { $dbg.GetExpression($e, $true, 3000).Value }
    "$e = $v"
}
