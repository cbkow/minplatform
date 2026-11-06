# Builds the C++ test producer (QCBridgeAE's own SharedRing, vendor/qcbae unmodified) with MSVC. Run from a VS developer shell,
# or let the script find vcvars64. Output: target\s4-cxx\cxx_producer.exe  (usage: cxx_producer NAME SECONDS)
$ErrorActionPreference = "Stop"
$root = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$out = Join-Path $root "target\s4-cxx"; New-Item -ItemType Directory -Force "$out\inc" | Out-Null
Remove-Item -Recurse -Force "$out\inc\common" -ErrorAction SilentlyContinue
Copy-Item -Recurse "$root\vendor\qcbae" "$out\inc\common"
if (-not (Get-Command cl.exe -ErrorAction SilentlyContinue)) {
  $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
  $vs = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
  $vcvars = Join-Path $vs "VC\Auxiliary\Build\vcvars64.bat"
  cmd /c "`"$vcvars`" >nul 2>&1 && set" | ForEach-Object { if ($_ -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($matches[1], $matches[2]) } }
}
Push-Location $out
cl /nologo /std:c++20 /O2 /EHsc /DWIN32_LEAN_AND_MEAN /I"$out\inc" /Fe:"$out\cxx_producer.exe" "$root\tools\s4\cxx_producer.cpp" "$out\inc\common\surface\shared_ring.cpp" "$out\inc\common\surface\ring_mapping.cpp"
Pop-Location
Write-Output "built $out\cxx_producer.exe"
