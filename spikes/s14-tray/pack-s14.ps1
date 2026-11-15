# S14, part C, on a Windows machine, in the DESKTOP session:
#   powershell -NoProfile -ExecutionPolicy Bypass -File spikes\s14-tray\pack-s14.ps1 -Work <folder> -SignThumbprint <test certificate>
# Packs the spike with a startup task that names the spike itself, signs it, installs it for this user and starts it
# once with a script that makes it restart itself. It then stays installed for a sign-out and sign-in.
# Its log is %USERPROFILE%\s14.log. Remove with:  Get-AppxPackage cbkow.S14Spike | Remove-AppxPackage
param([Parameter(Mandatory)] [string] $Work, [Parameter(Mandatory)] [string] $SignThumbprint)
$ErrorActionPreference = "Stop"
$repo = Resolve-Path (Join-Path $PSScriptRoot "..\.."); $exe = Join-Path $repo "target\release\s14.exe"
function Find-SdkTool([string] $name) { foreach ($kits in "${env:ProgramFiles(x86)}\Windows Kits\10\bin", "${env:ProgramFiles}\Windows Kits\10\bin") { if (-not (Test-Path $kits)) { continue }
    $hit = Get-ChildItem $kits -Recurse -Filter $name -ErrorAction SilentlyContinue | Where-Object { $_.FullName -match "\\x64\\" } | Sort-Object FullName | Select-Object -Last 1; if ($hit) { return $hit.FullName } }; throw "$name not found" }
New-Item -ItemType Directory -Force $Work | Out-Null; $out = "$Work\pack.txt"; Remove-Item $out -ErrorAction SilentlyContinue; function Say($s) { Add-Content $out $s -Encoding ascii }
try {
    Get-Process s14 -ErrorAction SilentlyContinue | Stop-Process -Force; Get-AppxPackage cbkow.S14Spike | Remove-AppxPackage -ErrorAction SilentlyContinue
    $stage = "$Work\stage"; Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory -Force "$stage\images" | Out-Null
    Copy-Item $exe "$stage\s14.exe"; Copy-Item (Join-Path $PSScriptRoot "msix\AppxManifest.xml") $stage
    foreach ($n in "StoreLogo.png", "Square150x150Logo.png", "Square44x44Logo.png") { Copy-Item (Join-Path $repo "packaging\windows\msix-minnotes\images\$n") "$stage\images\" }
    $msix = "$Work\s14.msix"; & (Find-SdkTool "MakeAppx.exe") pack /d $stage /p $msix /o | Out-Null; if ($LASTEXITCODE -ne 0) { throw "MakeAppx failed" }
    $o = & (Find-SdkTool "signtool.exe") sign /fd SHA256 /sha1 $SignThumbprint $msix 2>&1; if ($LASTEXITCODE -ne 0) { throw "signtool failed: $($o | Select-Object -Last 2)" }
    Remove-Item "$env:USERPROFILE\s14.log" -ErrorAction SilentlyContinue
    Add-AppxPackage $msix; $pkg = Get-AppxPackage cbkow.S14Spike; Say "installed $($pkg.PackageFullName)"
    Set-Content "$env:USERPROFILE\s14-script.txt" "restart@2.0" -Encoding ascii
    Start-Process "shell:AppsFolder\$($pkg.PackageFamilyName)!S14"; Start-Sleep 8
    Say "processes after the restart: $(@(Get-Process s14 -ErrorAction SilentlyContinue).Count)"; Get-Content "$env:USERPROFILE\s14.log" | ForEach-Object { Say $_ }
} catch { Say "FAILED: $_" }
Set-Content "$Work\pack-done.txt" "done"
