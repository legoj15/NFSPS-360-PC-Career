param([Parameter(Mandatory)][string[]]$Ascii, [string]$Process = 'nfs', [int]$Max = 20, [int]$Context = 16)
# Search the running game's committed, readable memory for ASCII strings and
# print each hit's address with surrounding bytes. No debugger needed.
#   pwsh -File docs/re/procscan.ps1 -Ascii 'JOSHUA S 10','Player'
Add-Type -Namespace W -Name S -MemberDefinition @'
[StructLayout(LayoutKind.Sequential)] public struct MBI { public IntPtr Base; public IntPtr AllocBase; public uint AllocProtect; public IntPtr Size; public uint State; public uint Protect; public uint Type; }
[DllImport("kernel32.dll", SetLastError=true)] public static extern IntPtr OpenProcess(int a, bool i, int pid);
[DllImport("kernel32.dll", SetLastError=true)] public static extern bool ReadProcessMemory(IntPtr h, IntPtr addr, byte[] buf, IntPtr size, out IntPtr read);
[DllImport("kernel32.dll", SetLastError=true)] public static extern IntPtr VirtualQueryEx(IntPtr h, IntPtr addr, out MBI mbi, IntPtr len);
[DllImport("kernel32.dll")] public static extern bool CloseHandle(IntPtr h);
'@
$p = Get-Process $Process | Select-Object -First 1
$h = [W.S]::OpenProcess(0x0410, $false, $p.Id)
$needles = $Ascii | ForEach-Object { ,([Text.Encoding]::ASCII.GetBytes($_)) }
$hits = @{}; foreach ($a in $Ascii) { $hits[$a] = 0 }
$addr = [int64]0
$mbi = New-Object W.S+MBI
while ($addr -lt 0x7FFF0000) {
    if ([W.S]::VirtualQueryEx($h, [IntPtr]$addr, [ref]$mbi, [IntPtr][Runtime.InteropServices.Marshal]::SizeOf($mbi)) -eq [IntPtr]::Zero) { break }
    $size = [int64]$mbi.Size
    $readable = $mbi.State -eq 0x1000 -and ($mbi.Protect -band 0x66) -and -not ($mbi.Protect -band 0x100)
    if ($readable -and $size -lt 256MB) {
        $buf = New-Object byte[] $size; $n = [IntPtr]::Zero
        if ([W.S]::ReadProcessMemory($h, $mbi.Base, $buf, [IntPtr]$size, [ref]$n)) {
            for ($k = 0; $k -lt $needles.Count; $k++) {
                $nd = $needles[$k]; $name = $Ascii[$k]
                $i = [Array]::IndexOf($buf, $nd[0])
                while ($i -ge 0 -and $i -le $buf.Length - $nd.Length) {
                    $ok = $true
                    for ($j = 1; $j -lt $nd.Length; $j++) { if ($buf[$i + $j] -ne $nd[$j]) { $ok = $false; break } }
                    if ($ok -and $hits[$name] -lt $Max) {
                        $hits[$name]++
                        $s = [Math]::Max(0, $i - $Context); $e = [Math]::Min($buf.Length, $i + $nd.Length + $Context)
                        "{0} @0x{1:x8}: {2}" -f $name, ($mbi.Base.ToInt64() + $i), (($buf[$s..($e - 1)] | ForEach-Object { $_.ToString('x2') }) -join ' ')
                    }
                    $i = [Array]::IndexOf($buf, $nd[0], $i + 1)
                }
            }
        }
    }
    $addr = $mbi.Base.ToInt64() + $size
}
[void][W.S]::CloseHandle($h)
foreach ($a in $Ascii) { "total hits (capped at $Max) for '$a': $($hits[$a])" }
