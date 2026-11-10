# The Windows package (canon row 248): stages target\dist\win64 and packs target\dist\QCView-<version>-win64.msix.
# QCView 2.x's identity (name, publisher, application id, alias, protocol), so the Store and a sideload replace 2.x in place.
#   - the exe (built by the caller: cargo build --release -p qcv), renamed qcview.exe as the manifest and the alias name it
#   - ffmpeg.exe / ffprobe.exe (QCView is the suite's ffmpeg provider: toolbox.rs mirrors them for the Blender add-on)
#   - every DLL any of those imports, found by WALKING THE IMPORT TABLES: from the FFmpeg and vcpkg folders, and the
#     Visual C++ runtime from Visual Studio's redistributable folder (2.x shipped NO runtime and ran only where one
#     happened to be installed). A DLL that is neither staged nor part of Windows FAILS the build.
#   - dxcompiler.dll (the DX12 renderer's shader compiler), assets\OCIO, LICENSE
#   - AppxManifest.xml generated here: the version from apps\qcv\Cargo.toml, the file types from
#     crates\qcv-project\src\extensions.rs (one list for both OSes, only what the app opens)
# The .msix is UNSIGNED - what Partner Center takes (Microsoft signs it). -Pfx signs a private sideload copy for testing
# (QCV_SIGNING_PASSWORD in the environment if the PFX has one; from the desktop session).
# Usage (from the repo root):  powershell -NoProfile -ExecutionPolicy Bypass -File packaging\windows\make-msix.ps1 [-StageOnly] [-Pfx <file>]
[CmdletBinding()]
param(
    [string] $Ffmpeg = "",
    [string] $Vcpkg = "C:\vcpkg\installed\x64-windows",
    [string] $Dxc = "",
    [string] $Redist = "",
    [switch] $StageOnly,
    [string] $Pfx = ""
)
$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
if (-not $Ffmpeg) { $Ffmpeg = Join-Path $root "external\ffmpeg\win64\bin" }
if (-not $Dxc)    { $Dxc    = Join-Path $root "external\dxc\win64" }
if (-not $Redist) {
    $r = Get-ChildItem "C:\Program Files\Microsoft Visual Studio\*\*\VC\Redist\MSVC\*\x64\Microsoft.VC*.CRT" -ErrorAction SilentlyContinue | Sort-Object FullName | Select-Object -Last 1
    if (-not $r) { throw "No Visual C++ redistributable folder found; pass -Redist." }
    $Redist = $r.FullName
}
$exe = Join-Path $root "target\release\QCView.exe"
if (-not (Test-Path $exe)) { throw "No $exe - build first (cargo build --release -p qcv)." }
$version = ([regex]::Match((Get-Content (Join-Path $root "apps\qcv\Cargo.toml") -Raw), '(?m)^version\s*=\s*"([^"]+)"')).Groups[1].Value
if (-not $version) { throw "No version in apps\qcv\Cargo.toml." }
Write-Host "QCView $version"
Write-Host "  ffmpeg : $Ffmpeg"
Write-Host "  vcpkg  : $Vcpkg"
Write-Host "  redist : $Redist"

$dist  = Join-Path $root "target\dist"
$stage = Join-Path $dist "win64"
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Path $stage | Out-Null
Copy-Item $exe (Join-Path $stage "qcview.exe")
foreach ($t in "ffmpeg.exe", "ffprobe.exe") {
    $p = Join-Path $Ffmpeg $t
    if (-not (Test-Path $p)) { throw "No $t in $Ffmpeg." }
    Copy-Item $p $stage
}

# --- the import walk -------------------------------------------------------------------------------------------------
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
$runtime = '^(vcruntime|msvcp|concrt|vccorlib|vcomp|vcamp|mfc)\d'   # must ship with the app, never count as "Windows has it"
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
            if (-not $src -and (Test-Path (Join-Path $sys32 $imp))) { continue }   # part of Windows
        }
        if (-not $src) { $missing += "$imp (needed by $(Split-Path $bin -Leaf))"; continue }
        Copy-Item $src $stage
        $staged += $imp
        $queue.Enqueue((Join-Path $stage $imp))
    }
}
if ($missing.Count) { $missing | ForEach-Object { Write-Host "MISSING $_" }; throw "The package would not start on a clean Windows: $($missing.Count) DLL(s) unresolved." }
Write-Host ("  staged by the import walk: " + (($staged | Sort-Object) -join " "))

# the DX12 shader compiler (tools\dxc\dxc.pin: dxcompiler.dll alone - dxil.dll is not needed) with its two licence files
foreach ($d in "dxcompiler.dll", "LICENSE-LLVM.txt", "LICENCE-MIT.txt") {
    $p = Join-Path $Dxc $d
    if (-not (Test-Path $p)) { throw "No $d in $Dxc (tools\dxc\fetch-win64.ps1)." }
    Copy-Item $p $(if ($d -like "*.dll") { $stage } else { Join-Path $stage ("dxc-" + $d) })
}
New-Item -ItemType Directory -Path (Join-Path $stage "assets") | Out-Null
Copy-Item -Recurse (Join-Path $root "apps\qcv\assets\OCIO") (Join-Path $stage "assets\OCIO")
Copy-Item (Join-Path $root "LICENSE") $stage
$bytes = (Get-ChildItem $stage -Recurse -File | Measure-Object Length -Sum).Sum
Write-Host ("  staged {0} files, {1:N0} MB -> {2}" -f (Get-ChildItem $stage -Recurse -File).Count, ($bytes / 1MB), $stage)
if ($StageOnly) { return }

# --- the manifest ------------------------------------------------------------------------------------------------------
$extSrc = Get-Content (Join-Path $root "crates\qcv-project\src\extensions.rs") -Raw
function Get-Exts([string] $name) {
    $m = [regex]::Match($extSrc, "pub const ${name}: &\[&str\] = &\[(.*?)\];")
    if (-not $m.Success) { throw "No $name list in extensions.rs." }
    return [regex]::Matches($m.Groups[1].Value, '"([a-z0-9]+)"') | ForEach-Object { $_.Groups[1].Value }
}
function Get-Assoc([string] $name, [string] $display, $exts) {
    $types = ($exts | ForEach-Object { "              <uap:FileType>.$_</uap:FileType>" }) -join "`r`n"
    return @"
        <uap:Extension Category="windows.fileTypeAssociation">
          <uap:FileTypeAssociation Name="$name">
            <uap:DisplayName>$display</uap:DisplayName>
            <uap:SupportedFileTypes>
$types
            </uap:SupportedFileTypes>
          </uap:FileTypeAssociation>
        </uap:Extension>
"@
}
$assoc = @(
    (Get-Assoc "qcviewproject" "QCView Project" @("qcv", "qcvproj")),
    (Get-Assoc "qcviewvideo" "Video File" (Get-Exts "VIDEO")),
    (Get-Assoc "qcviewimage" "Image File" (Get-Exts "IMAGE")),
    (Get-Assoc "qcviewaudio" "Audio File" (Get-Exts "AUDIO"))
) -join "`r`n"
$manifest = (Get-Content (Join-Path $PSScriptRoot "msix\AppxManifest.xml.in") -Raw).Replace("@VERSION@", "$version.0").Replace("@FILE_TYPES@", $assoc)
[IO.File]::WriteAllText((Join-Path $stage "AppxManifest.xml"), $manifest, (New-Object Text.UTF8Encoding($false)))
Copy-Item -Recurse (Join-Path $PSScriptRoot "msix\images") (Join-Path $stage "images")

# --- pack ---------------------------------------------------------------------------------------------------------------
function Find-SdkTool([string] $name) {
    foreach ($kits in "${env:ProgramFiles(x86)}\Windows Kits\10\bin", "${env:ProgramFiles}\Windows Kits\10\bin") {
        if (-not (Test-Path $kits)) { continue }
        $hit = Get-ChildItem $kits -Recurse -Filter $name -ErrorAction SilentlyContinue | Where-Object { $_.FullName -match "\\x64\\" } | Sort-Object FullName | Select-Object -Last 1
        if ($hit) { return $hit.FullName }
    }
    throw "$name not found - install the Windows SDK."
}
$makePri = Find-SdkTool "MakePri.exe"; $makeAppx = Find-SdkTool "MakeAppx.exe"
$priConfig = Join-Path $dist "priconfig.xml"
& $makePri createconfig /cf $priConfig /dq "en-us" /o | Out-Null
& $makePri new /pr $stage /cf $priConfig /mn (Join-Path $stage "AppxManifest.xml") /of (Join-Path $stage "resources.pri") /o | Out-Null
if ($LASTEXITCODE -ne 0) { throw "MakePri failed ($LASTEXITCODE)." }
Remove-Item $priConfig
$out = Join-Path $dist "QCView-$version-win64.msix"
if (Test-Path $out) { Remove-Item $out }
& $makeAppx pack /d $stage /p $out /o | Out-Null
if ($LASTEXITCODE -ne 0) { throw "MakeAppx failed ($LASTEXITCODE)." }
Write-Host ("  packed {0} ({1:N0} MB) - unsigned, for Partner Center" -f $out, ((Get-Item $out).Length / 1MB))
if ($Pfx) {
    $signTool = Find-SdkTool "signtool.exe"
    $side = Join-Path $dist "QCView-$version-win64-sideload.msix"
    Copy-Item $out $side -Force
    # the PFX's password comes from the environment (QCV_SIGNING_PASSWORD, QCView's variable) - never from a file in the repo.
    # signtool cannot open a PFX from an SSH session: run this from the logged-on desktop.
    $signArgs = @("sign", "/fd", "SHA256", "/f", $Pfx)
    if ($env:QCV_SIGNING_PASSWORD) { $signArgs += @("/p", $env:QCV_SIGNING_PASSWORD) }
    $signArgs += $side
    $signOut = & $signTool @signArgs 2>&1
    if ($LASTEXITCODE -ne 0) { throw ("signtool failed ($LASTEXITCODE): " + (($signOut | Select-Object -Last 3) -join " | ") + " (the certificate's subject must equal the manifest's Publisher)") }
    Write-Host "  signed $side (private testing only)"
}
