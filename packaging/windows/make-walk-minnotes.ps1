# make-walk-minnotes.ps1 - a folder minNotes STARTS from on its own (row 372): the release exe with every DLL it imports
# beside it (the import walk of make-msix.ps1: the patched FFmpeg, vcpkg's OpenEXR / TIFF / OpenColorIO / ..., the Visual
# C++ runtime), ffmpeg.exe for the web folder's videos and the User Guide's pages. `target\release\minNotes.exe` alone
# does not start outside a shell that has those folders on PATH. This is the walk's folder, not the Store package.
#   powershell -NoProfile -ExecutionPolicy Bypass -File packaging\windows\make-walk-minnotes.ps1 [-Out <folder>]
param(
    [string] $Out = "",
    [string] $Ffmpeg = "",
    [string] $Vcpkg = "C:\vcpkg\installed\x64-windows",
    [string] $Redist = ""
)
$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
if (-not $Ffmpeg) { $Ffmpeg = Join-Path $root "external\ffmpeg\win64\bin" }
if (-not $Out)    { $Out    = Join-Path $root "target\dist\minNotes-walk" }
if (-not $Redist) {
    $r = Get-ChildItem "C:\Program Files\Microsoft Visual Studio\*\*\VC\Redist\MSVC\*\x64\Microsoft.VC*.CRT" -ErrorAction SilentlyContinue | Sort-Object FullName | Select-Object -Last 1
    if (-not $r) { throw "No Visual C++ redistributable folder found; pass -Redist." }
    $Redist = $r.FullName
}
$exe = Join-Path $root "target\release\minNotes.exe"
if (-not (Test-Path $exe)) { throw "No $exe - build first (cargo build --release -p mn)." }
if (Get-Process minNotes -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$Out*" }) { throw "minNotes is running from $Out - quit it first." }
$stage = $Out
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Path $stage | Out-Null
Copy-Item $exe $stage
Copy-Item (Join-Path $Ffmpeg "ffmpeg.exe") $stage
Copy-Item -Recurse (Join-Path $root "apps\mn\guide") (Join-Path $stage "guide")
# the DX12 shader compiler the shared presenter looks for beside the exe
$dxc = Join-Path $root "external\dxc\win64\dxcompiler.dll"
if (Test-Path $dxc) { Copy-Item $dxc $stage }

function Get-PeImports([string] $path) {
    $b = [IO.File]::ReadAllBytes($path)
    $pe = [BitConverter]::ToInt32($b, 0x3C)
    $nsec = [BitConverter]::ToUInt16($b, $pe + 6)
    $optSize = [BitConverter]::ToUInt16($b, $pe + 20)
    $opt = $pe + 24
    $dd = if ([BitConverter]::ToUInt16($b, $opt) -eq 0x20b) { $opt + 112 } else { $opt + 96 }
    $secs = @()
    for ($i = 0; $i -lt $nsec; $i++) {
        $o = $opt + $optSize + 40 * $i
        $secs += ,@([BitConverter]::ToUInt32($b, $o + 12), [Math]::Max([BitConverter]::ToUInt32($b, $o + 8), [BitConverter]::ToUInt32($b, $o + 16)), [BitConverter]::ToUInt32($b, $o + 20))
    }
    $off = { param($rva) foreach ($s in $secs) { if ($rva -ge $s[0] -and $rva -lt ($s[0] + $s[1])) { return [int]($s[2] + $rva - $s[0]) } } return -1 }
    $str = { param($o) $e = $o; while ($b[$e] -ne 0) { $e++ } return [Text.Encoding]::ASCII.GetString($b, $o, $e - $o) }
    $names = @()
    # directory 1 = imports (20-byte descriptors, the name RVA at +12); directory 13 = delay imports (32 bytes, the name at +4)
    foreach ($spec in @(@(1, 20, 12), @(13, 32, 4))) {
        $rva = [BitConverter]::ToUInt32($b, $dd + 8 * $spec[0])
        if ($rva -eq 0) { continue }
        $p = & $off $rva
        if ($p -lt 0) { continue }
        while ($true) {
            $n = [BitConverter]::ToUInt32($b, $p + $spec[2])
            if ($n -eq 0) { break }
            $no = & $off $n
            if ($no -ge 0) { $names += (& $str $no) }
            $p += $spec[1]
        }
    }
    return $names | Sort-Object -Unique
}
$searchDirs = @($Ffmpeg, (Join-Path $Vcpkg "bin"), (Join-Path $Vcpkg "tools\tiff")) | Where-Object { Test-Path $_ }
$sys32 = Join-Path $env:WINDIR "System32"
$runtime = '^(vcruntime|msvcp|concrt|vccorlib|vcomp|vcamp|mfc)\d'
$seen = @{}; $missing = @(); $staged = @(); $queue = New-Object System.Collections.Queue
Get-ChildItem $stage -File | ForEach-Object { $queue.Enqueue($_.FullName) }
while ($queue.Count -gt 0) {
    $bin = $queue.Dequeue()
    foreach ($imp in (Get-PeImports $bin)) {
        $key = $imp.ToLowerInvariant()
        if ($seen.ContainsKey($key)) { continue }
        $seen[$key] = $true
        if ($key -match '^(api-ms-|ext-ms-)') { continue }
        if (Test-Path (Join-Path $stage $imp)) { continue }
        $src = $null
        if ($key -match $runtime) { $c = Join-Path $Redist $imp; if (Test-Path $c) { $src = $c } }
        else {
            foreach ($d in $searchDirs) { $c = Join-Path $d $imp; if (Test-Path $c) { $src = $c; break } }
            if (-not $src -and (Test-Path (Join-Path $sys32 $imp))) { continue }
        }
        if (-not $src) { $missing += "$imp (needed by $(Split-Path $bin -Leaf))"; continue }
        Copy-Item $src $stage
        $staged += $imp
        $queue.Enqueue((Join-Path $stage $imp))
    }
}
if ($missing.Count) { $missing | ForEach-Object { Write-Host "MISSING $_" }; throw "minNotes would not start from this folder: $($missing.Count) DLL(s) unresolved." }
Write-Host ("staged " + $staged.Count + " DLLs into " + $stage)
