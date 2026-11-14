# make-msix-minnotes.ps1 - the Microsoft Store package of minNotes (canon row 379): minNotes 2.x's identity
# (cbkow.minNotes, its file types .mnd / .mnpkg, the minnotes: protocol), so the Store - the only official channel on
# Windows, and the updater - replaces 2.x in place. The staged folder is make-walk-minnotes.ps1's (the exe, every DLL it
# imports, the Visual C++ runtime, ffmpeg.exe, dxcompiler.dll, the User Guide); the notices are checked as QCView's are
# (make-msix.ps1, whose parts these are). The result is UNSIGNED, for Partner Center; -SignThumbprint / -Pfx also make a
# signed copy for private testing (the publisher's test certificate; signtool must run in the desktop session).
#   powershell -NoProfile -ExecutionPolicy Bypass -File packaging\windows\make-msix-minnotes.ps1 [-SignThumbprint <sha1>]
param(
    [string] $Ffmpeg = "",
    [string] $Vcpkg = "C:\vcpkg\installed\x64-windows",
    [string] $Pfx = "",
    [string] $SignThumbprint = ""
)
$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
if (-not $Ffmpeg) { $Ffmpeg = Join-Path $root "external\ffmpeg\win64\bin" }
$version = ([regex]::Match((Get-Content (Join-Path $root "apps\mn\Cargo.toml") -Raw), '(?m)^version\s*=\s*"([^"]+)"')).Groups[1].Value
if (-not $version) { throw "No version in apps\mn\Cargo.toml." }
Write-Host "minNotes $version"
$dist  = Join-Path $root "target\dist"
$stage = Join-Path $dist "minNotes-win64"
& (Join-Path $PSScriptRoot "make-walk-minnotes.ps1") -Out $stage -Ffmpeg $Ffmpeg -Vcpkg $Vcpkg
# the exe must BE this version (a failed build leaves the previous exe in target\release)
$built = (Get-Item (Join-Path $stage "minNotes.exe")).VersionInfo.ProductVersion
if ($built -ne $version) { throw "target\release\minNotes.exe is $built, not $version - the build did not run or failed." }

# --- the third-party notices ---------------------------------------------------------------------------------------------
$notices = Join-Path $root "packaging\notices\generated\minnotes-THIRD-PARTY-NOTICES-windows.html"
$check = Get-Content "$notices.check"
$want = @{}; $filePats = @(); $ports = @{}
foreach ($l in $check) {
    $k, $v = $l -split " ", 2
    if ($k -eq "file") { $filePats += $v } elseif ($k -eq "port") { $pn, $pv = $v -split " ", 2; $ports[$pn] = $pv } else { $want[$k] = $v }
}
# the vcpkg libraries are the pinned ones (tools\deps\vcpkg-windows.pin, carried by the sidecar): read vcpkg's own record
$statusFile = Join-Path (Split-Path $Vcpkg -Parent) "vcpkg\status"
if (-not (Test-Path $statusFile)) { throw "No $statusFile - cannot check the installed vcpkg libraries against the pin." }
$triplet = Split-Path $Vcpkg -Leaf; $have = @{}
foreach ($block in ((Get-Content $statusFile -Raw) -split "(\r?\n){2,}")) {
    if ($block -match "(?m)^Feature:" -or $block -notmatch "(?m)^Status: install ok installed" -or $block -notmatch "(?m)^Architecture: $([regex]::Escape($triplet))\s*$") { continue }
    $n = [regex]::Match($block, "(?m)^Package: (\S+)").Groups[1].Value
    $ver = [regex]::Match($block, "(?m)^Version: (\S+)").Groups[1].Value
    $pv = [regex]::Match($block, "(?m)^Port-Version: (\d+)").Groups[1].Value
    if ($pv -and $pv -ne "0") { $ver = "$ver#$pv" }
    $have[$n] = $ver
}
$drift = $ports.Keys | Sort-Object | Where-Object { $have[$_] -ne $ports[$_] } | ForEach-Object { "$_ (pinned $($ports[$_]), installed $($have[$_]))" }
if (-not $ports.Count) { throw "The notices' sidecar carries no vcpkg pins - regenerate it on the Mac." }
if ($drift) { throw ("The installed vcpkg libraries differ from tools\deps\vcpkg-windows.pin: " + ($drift -join ", ")) }
Write-Host "  vcpkg  : $($ports.Count) pinned libraries match"
$lockNow = (& git -C $root rev-parse "HEAD:Cargo.lock").Trim()
if ($want["lock"] -ne $lockNow) { throw "The notices were generated from another Cargo.lock - regenerate them on the Mac (make_notices.py windows --app mn) and commit." }
if ($want["version"] -ne $version) { throw "The notices are for minNotes $($want['version']), this is $version - regenerate them." }
$conf = (& (Join-Path $Ffmpeg "ffmpeg.exe") -hide_banner -buildconf 2>&1 | Out-String)
$flags = ([regex]::Matches($conf, "--enable-([a-z0-9_-]+)") | ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique -CaseSensitive) -join " "
if ((($want["ffmpeg"] -split " " | Sort-Object -Unique -CaseSensitive) -join " ") -ne $flags) { throw "The shipped FFmpeg's libraries differ from the notices' - capture ffmpeg -buildconf into packaging\notices\ffmpeg-win64.enabled and regenerate." }
$orphans = Get-ChildItem $stage -File | Where-Object { $_.Extension -in ".dll", ".exe" -and $_.Name -ne "minNotes.exe" } | Where-Object { $n = $_.Name; -not ($filePats | Where-Object { $n -like $_ }) } | ForEach-Object { $_.Name }
if ($orphans) { throw ("Shipped without a notices entry: " + ($orphans -join ", ") + " - add them to packaging\notices\components.json and regenerate.") }
Copy-Item $notices (Join-Path $stage "THIRD-PARTY-NOTICES.html")

# --- the manifest ------------------------------------------------------------------------------------------------------
$manifest = (Get-Content (Join-Path $PSScriptRoot "msix-minnotes\AppxManifest.xml.in") -Raw).Replace("@VERSION@", "$version.0")
[IO.File]::WriteAllText((Join-Path $stage "AppxManifest.xml"), $manifest, (New-Object Text.UTF8Encoding($false)))
Copy-Item -Recurse (Join-Path $PSScriptRoot "msix-minnotes\images") (Join-Path $stage "images")
Copy-Item (Join-Path $root "LICENSE") $stage

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
$out = Join-Path $dist "minNotes-$version-win64.msix"
if (Test-Path $out) { Remove-Item $out }
& $makeAppx pack /d $stage /p $out /o | Out-Null
if ($LASTEXITCODE -ne 0) { throw "MakeAppx failed ($LASTEXITCODE)." }
Write-Host ("  packed {0} ({1:N0} MB) - unsigned, for Partner Center" -f $out, ((Get-Item $out).Length / 1MB))
if ($Pfx -or $SignThumbprint) {
    $signTool = Find-SdkTool "signtool.exe"
    $side = Join-Path $dist "minNotes-$version-win64-sideload.msix"
    Copy-Item $out $side -Force
    # the PFX's password comes from the environment (QCV_SIGNING_PASSWORD, the family's variable) - never from a file in the repo.
    # signtool cannot open a PFX from an SSH session: run this from the logged-on desktop.
    # -SignThumbprint: a certificate in the user's own store (Cert:\CurrentUser\My) - no file, no password (the 2026-11-09 test
    # certificate: same subject as the manifest's Publisher, so it upgrades any earlier sideload in place).
    if ($SignThumbprint) { $signArgs = @("sign", "/fd", "SHA256", "/sha1", $SignThumbprint) }
    else {
        $signArgs = @("sign", "/fd", "SHA256", "/f", $Pfx)
        if ($env:QCV_SIGNING_PASSWORD) { $signArgs += @("/p", $env:QCV_SIGNING_PASSWORD) }
    }
    $signArgs += $side
    $signOut = & $signTool @signArgs 2>&1
    if ($LASTEXITCODE -ne 0) { throw ("signtool failed ($LASTEXITCODE): " + (($signOut | Select-Object -Last 3) -join " | ") + " (the certificate's subject must equal the manifest's Publisher)") }
    Write-Host "  signed $side (private testing only)"
}
