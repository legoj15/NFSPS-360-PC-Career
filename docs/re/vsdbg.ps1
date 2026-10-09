param([string]$ExprList = "", [string]$Cmd = "", [string]$CmdArgs = "")
# Read a live Visual Studio debug session (EnvDTE via the Running Object Table):
# break mode, call stack, and any expressions, e.g.
#   powershell.exe -File docs/re/vsdbg.ps1 -ExprList '$eip,x;$esi,x;*(unsigned int*)($esi+0x18),x'
# Must run under Windows PowerShell 5.1 (pwsh 7 lacks Marshal.GetActiveObject).
# Debug.SaveDumpAs opens a dialog through DTE; read memory with expressions instead.
$d = $null
foreach ($v in '18.0', '17.0') {
    try { $d = [Runtime.InteropServices.Marshal]::GetActiveObject("VisualStudio.DTE.$v"); "DTE $v"; break }
    catch { "DTE $v : $($_.Exception.Message)" }
}
if (-not $d) { exit 1 }
$dbg = $d.Debugger
"mode=$($dbg.CurrentMode) reason=$($dbg.LastBreakReason)"
if ($dbg.CurrentProcess) { "process=$($dbg.CurrentProcess.Name) pid=$($dbg.CurrentProcess.ProcessID)" }
if ($Cmd) { $d.ExecuteCommand($Cmd, $CmdArgs); "ran $Cmd $CmdArgs" }
$t = $dbg.CurrentThread
if ($t) {
    "thread=$($t.ID) $($t.Name)"
    $i = 0
    foreach ($f in $t.StackFrames) {
        "  #$i $($f.FunctionName)  [$($f.Module)]"
        $i++; if ($i -ge 40) { break }
    }
}
foreach ($e in ($ExprList -split ";" | ? { $_ })) {
    $r = $dbg.GetExpression($e, $true, 2000)
    "$e = $($r.Value) (valid=$($r.IsValidValue))"
}
