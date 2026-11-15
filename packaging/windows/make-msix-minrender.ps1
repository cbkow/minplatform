# make-msix-minrender.ps1 - the Microsoft Store package of minRender (canon rows 421, 423, 426;
# docs/phase4/step10-store-release-flow.md). It carries 0.5.x's identity (cbkow.minRender) and the manifest of the
# Store-installed 0.5.11.0 line for line, so the Store - the only Windows channel, and the updater - replaces 0.5.x
# in place and each node comes back as itself.
#   - the staged folder: the release exe, every DLL it imports (vcpkg's OpenEXR / TIFF / ..., the Visual C++ runtime),
#     dxcompiler.dll, the logos, the licence, the third-party notices;
#   - GUARD 1: the capabilities are EXACTLY internetClientServer, privateNetworkClientServer, runFullTrust;
#   - GUARD 2: the manifest is compared with the installed 0.5.11.0's (-Installed, default
#     fixtures\minrender\store-0.5.11-AppxManifest.xml when it is there); for the real identity only the version,
#     the startup task's executable and the description may differ - anything else stops the script;
#   - the result is UNSIGNED, for Partner Center (the Store signs); -SignThumbprint / -Pfx also make a signed copy
#     for private testing (signtool must run in the desktop session).
# -TestIdentity packs a TEST package that can be installed BESIDE the real one: identity cbkow.minRenderTest, shown as
# "minRender Test", firewall rules for 8520 / 4343; the application then keeps its data in %USERPROFILE%\minRender-test
# and never touches the real node's folder. It is not for the Store.
#   powershell -NoProfile -ExecutionPolicy Bypass -File packaging\windows\make-msix-minrender.ps1 [-TestIdentity] [-SignThumbprint <sha1>]
param(
    [string] $Vcpkg = "C:\vcpkg\installed\x64-windows",
    [string] $Redist = "",
    [string] $Installed = "",
    [switch] $TestIdentity,
    [switch] $AllowNoNotices,
    [string] $Pfx = "",
    [string] $SignThumbprint = ""
)
$ErrorActionPreference = "Stop"
$root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
$version = ([regex]::Match((Get-Content (Join-Path $root "apps\mr\Cargo.toml") -Raw), '(?m)^version\s*=\s*"([^"]+)"')).Groups[1].Value
if (-not $version) { throw "No version in apps\mr\Cargo.toml." }
$kind = if ($TestIdentity) { "TEST identity - not for the Store" } else { "the Store's identity" }
Write-Host "minRender $version ($kind)"
if (-not $Redist) {
    $r = Get-ChildItem "C:\Program Files\Microsoft Visual Studio\*\*\VC\Redist\MSVC\*\x64\Microsoft.VC*.CRT" -ErrorAction SilentlyContinue | Sort-Object FullName | Select-Object -Last 1
    if (-not $r) { throw "No Visual C++ redistributable folder found; pass -Redist." }
    $Redist = $r.FullName
}
$exe = Join-Path $root "target\release\minRender.exe"
if (-not (Test-Path $exe)) { throw "No $exe - build first (cargo build --release -p mr)." }
# the exe must BE this version (a failed build leaves the previous exe in target\release)
$built = (Get-Item $exe).VersionInfo.ProductVersion
if ($built -ne $version) { throw "target\release\minRender.exe is $built, not $version - the build did not run or failed." }

# --- stage: the exe and everything it imports ---------------------------------------------------------------------------
$dist = Join-Path $root "target\dist"
$stage = Join-Path $dist ($(if ($TestIdentity) { "minRender-test-win64" } else { "minRender-win64" }))
if (Test-Path $stage) { Remove-Item -Recurse -Force $stage }
New-Item -ItemType Directory -Path $stage | Out-Null
Copy-Item $exe $stage
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
$searchDirs = @((Join-Path $Vcpkg "bin"), (Join-Path $Vcpkg "tools\tiff")) | Where-Object { Test-Path $_ }
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
if ($missing.Count) { $missing | ForEach-Object { Write-Host "MISSING $_" }; throw "minRender would not start from this folder: $($missing.Count) DLL(s) unresolved." }
Write-Host ("  staged : " + $staged.Count + " DLLs")
Copy-Item (Join-Path $root "LICENSE") $stage

# --- the third-party notices ---------------------------------------------------------------------------------------------
$notices = Join-Path $root "packaging\notices\generated\minrender-THIRD-PARTY-NOTICES-windows.html"
if (Test-Path $notices) {
    $check = Get-Content "$notices.check"; $want = @{}; $filePats = @()
    foreach ($l in $check) { $k, $v = $l -split " ", 2; if ($k -eq "file") { $filePats += $v } elseif ($k -ne "port") { $want[$k] = $v } }
    $lockNow = (& git -C $root rev-parse "HEAD:Cargo.lock").Trim()
    if ($want["lock"] -ne $lockNow) { throw "The notices were generated from another Cargo.lock - regenerate them on the Mac (make_notices.py windows --app mr) and commit." }
    if ($want["version"] -ne $version) { throw "The notices are for minRender $($want['version']), this is $version - regenerate them." }
    $orphans = Get-ChildItem $stage -File | Where-Object { $_.Extension -in ".dll", ".exe" -and $_.Name -ne "minRender.exe" } | Where-Object { $n = $_.Name; -not ($filePats | Where-Object { $n -like $_ }) } | ForEach-Object { $_.Name }
    if ($orphans) { throw ("Shipped without a notices entry: " + ($orphans -join ", ") + " - add them to packaging\notices\components.json and regenerate.") }
    Copy-Item $notices (Join-Path $stage "THIRD-PARTY-NOTICES.html")
    Write-Host "  notices: checked"
} elseif ($AllowNoNotices -or $TestIdentity) { Write-Host "  notices: NONE - this package is not for submission" }
else { throw "No third-party notices ($notices) - generate them on the Mac (make_notices.py windows --app mr); -AllowNoNotices packs without, not for submission." }

# --- the manifest, and its two guards ----------------------------------------------------------------------------------------
$name = if ($TestIdentity) { "cbkow.minRenderTest" } else { "cbkow.minRender" }
$display = if ($TestIdentity) { "minRender Test" } else { "minRender" }
$tcp = if ($TestIdentity) { "8520" } else { "8420" }; $udp = if ($TestIdentity) { "4343" } else { "4243" }
$manifest = (Get-Content (Join-Path $PSScriptRoot "msix-minrender\AppxManifest.xml.in") -Raw)
$manifest = [regex]::Replace($manifest, '(?s)\s*<!--.*?-->', '')   # the template's notes are ours, not the package's
$manifest = $manifest.Replace("@VERSION@", "$version.0").Replace("@NAME@", $name).Replace("@DISPLAY@", $display).Replace("@TCP@", $tcp).Replace("@UDP@", $udp)
[xml] $mx = $manifest
$caps = @($mx.Package.Capabilities.ChildNodes | ForEach-Object { $_.Name } | Sort-Object)
$expected = @("internetClientServer", "privateNetworkClientServer", "runFullTrust")
if (($caps -join ",") -ne ($expected -join ",")) { throw "The manifest's capabilities are [$($caps -join ', ')] - they must be exactly [$($expected -join ', ')], as the installed 0.5.11.0 has them (row 423)." }
Write-Host "  capabilities: $($caps -join ', ')  (exactly 0.5.11.0's)"
if (-not $Installed) { $c = Join-Path $root "fixtures\minrender\store-0.5.11-AppxManifest.xml"; if (Test-Path $c) { $Installed = $c } }
if ($Installed) {
    # one line for each element with its attributes in a fixed order; the Store's own additions and the namespace declarations are left out
    function Flatten([xml] $x) {
        $out = @()
        foreach ($n in $x.SelectNodes("//*")) {
            if ($n.LocalName -eq "PhoneIdentity") { continue }
            $path = @(); $p = $n; while ($p -and $p.NodeType -eq "Element") { $path = ,$p.LocalName + $path; $p = $p.ParentNode }
            $attrs = @($n.Attributes | Where-Object { $_.Prefix -ne "xmlns" -and $_.Name -ne "xmlns" -and $_.Name -ne "IgnorableNamespaces" } | Sort-Object Name | ForEach-Object { "$($_.Name)=$($_.Value)" })
            $text = if (-not $n.HasChildNodes -or ($n.ChildNodes.Count -eq 1 -and $n.FirstChild.NodeType -eq "Text")) { $n.InnerText.Trim() } else { "" }
            $out += (($path -join "/") + " [" + ($attrs -join "; ") + "] " + $text).Trim()
        }
        return $out
    }
    [xml] $old = Get-Content $Installed -Raw
    $a = Flatten $old; $b = Flatten $mx
    $diff = @(Compare-Object $a $b | ForEach-Object { $(if ($_.SideIndicator -eq "<=") { "  0.5.11 : " } else { "  now    : " }) + $_.InputObject })
    Write-Host "  against the installed 0.5.11.0's manifest: $($diff.Count / 2) line(s) differ"
    $diff | ForEach-Object { Write-Host $_ }
    if (-not $TestIdentity) {
        $allowed = '^Package/Identity \[|^Package/Applications/Application/VisualElements \[|^Package/Applications/Application/Extensions/Extension \[Category=windows\.startupTask'
        $bad = @(Compare-Object $a $b | Where-Object { $_.InputObject -notmatch $allowed })
        if ($bad.Count) { throw "The manifest differs from the installed 0.5.11.0's in more than the version, the startup task's executable and the description." }
    }
} else { Write-Host "  (no installed manifest to compare with - pass -Installed; C:\qcb-lab\read-store-manifest.ps1 reads it)" }
[IO.File]::WriteAllText((Join-Path $stage "AppxManifest.xml"), $manifest, (New-Object Text.UTF8Encoding($false)))
Copy-Item -Recurse (Join-Path $PSScriptRoot "msix-minrender\Assets") (Join-Path $stage "Assets")

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
# resources.pri: without it Windows does not find the logos' sizes (the taskbar's 16 / 24 / 32 / 48, the 200 % ones) and scales the 44 px one
$priConfig = Join-Path $dist "priconfig-mr.xml"
& $makePri createconfig /cf $priConfig /dq "en-us" /o | Out-Null
& $makePri new /pr $stage /cf $priConfig /mn (Join-Path $stage "AppxManifest.xml") /of (Join-Path $stage "resources.pri") /o | Out-Null
if ($LASTEXITCODE -ne 0) { throw "MakePri failed ($LASTEXITCODE)." }
Remove-Item $priConfig
$base = if ($TestIdentity) { "minRender-test-$version-win64" } else { "minRender-$version-win64" }
$out = Join-Path $dist "$base.msix"
if (Test-Path $out) { Remove-Item $out }
& $makeAppx pack /d $stage /p $out /o | Out-Null
if ($LASTEXITCODE -ne 0) { throw "MakeAppx failed ($LASTEXITCODE)." }
Write-Host ("  packed {0} ({1:N0} MB) - unsigned" -f $out, ((Get-Item $out).Length / 1MB))
if ($Pfx -or $SignThumbprint) {
    $signTool = Find-SdkTool "signtool.exe"
    $side = Join-Path $dist "$base-sideload.msix"
    Copy-Item $out $side -Force
    if ($SignThumbprint) { $signArgs = @("sign", "/fd", "SHA256", "/sha1", $SignThumbprint) }
    else { $signArgs = @("sign", "/fd", "SHA256", "/f", $Pfx); if ($env:QCV_SIGNING_PASSWORD) { $signArgs += @("/p", $env:QCV_SIGNING_PASSWORD) } }
    $signArgs += $side
    $signOut = & $signTool @signArgs 2>&1
    if ($LASTEXITCODE -ne 0) { throw ("signtool failed ($LASTEXITCODE): " + (($signOut | Select-Object -Last 3) -join " | ") + " (the certificate's subject must equal the manifest's Publisher; run in the desktop session)") }
    Write-Host "  signed $side (private testing only)"
}
