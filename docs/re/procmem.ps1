param([Parameter(Mandatory)][string]$Addr, [int]$Bytes = 0x400, [Parameter(Mandatory)][string]$Out,
      [string]$Process = 'nfs', [switch]$Deref)
# Read raw memory from the running game (no debugger needed; works alongside
# one). -Deref treats $Addr as a pointer slot and reads where it points - used
# for copy-protected functions that start with `jmp [slot]` and are only
# decrypted at runtime. Writes $Out as binary and prints the base address.
#   pwsh -File docs/re/procmem.ps1 -Addr 0x19ec920 -Deref -Bytes 0x800 -Out reader.bin
Add-Type -Namespace W -Name M -MemberDefinition @'
[DllImport("kernel32.dll", SetLastError=true)] public static extern IntPtr OpenProcess(int a, bool i, int pid);
[DllImport("kernel32.dll", SetLastError=true)] public static extern bool ReadProcessMemory(IntPtr h, IntPtr addr, byte[] buf, int size, out IntPtr read);
[DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr h);
'@
$p = Get-Process $Process | Select-Object -First 1
$h = [W.M]::OpenProcess(0x0010 -bor 0x0400, $false, $p.Id)   # VM_READ | QUERY_INFORMATION
if ($h -eq [IntPtr]::Zero) { throw "OpenProcess failed: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
try {
    $a = [Convert]::ToUInt32(($Addr -replace '^0x', ''), 16)
    $n = [IntPtr]::Zero
    if ($Deref) {
        $ptr = New-Object byte[] 4
        if (-not [W.M]::ReadProcessMemory($h, [IntPtr][int64]$a, $ptr, 4, [ref]$n)) { throw "read of pointer slot failed" }
        $a = [BitConverter]::ToUInt32($ptr, 0)
    }
    $buf = New-Object byte[] $Bytes
    if (-not [W.M]::ReadProcessMemory($h, [IntPtr][int64]$a, $buf, $Bytes, [ref]$n)) {
        throw ("ReadProcessMemory at 0x{0:x8} failed: {1}" -f $a, [Runtime.InteropServices.Marshal]::GetLastWin32Error())
    }
    [IO.File]::WriteAllBytes($Out, $buf)
    "base=0x{0:x8} bytes={1} -> {2}" -f $a, $n, $Out
} finally { [void][W.M]::CloseHandle($h) }
