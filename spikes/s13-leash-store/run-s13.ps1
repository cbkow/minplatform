# S13 on a Windows machine, in the DESKTOP session (signtool and a packaged application need it):
#   powershell -NoProfile -ExecutionPolicy Bypass -File spikes\s13-leash-store\run-s13.ps1 -Work C:\some\folder -SignThumbprint <sha1> [-Kit]
# Runs the soak three ways - the program as built (unsigned), signed with the test certificate, and inside a signed
# package installed for this user - and asks Defender before and after. -Kit also runs the Windows App Certification
# Kit over the package (slow; it installs and removes the package itself). Everything it writes is under -Work and
# %USERPROFILE%\s13-out; the package is removed at the end. Writes <Work>\report.txt, and <Work>\done.txt last.
param([Parameter(Mandatory)] [string] $Work, [Parameter(Mandatory)] [string] $SignThumbprint, [switch] $Kit, [int] $Chunks = 300, [int] $Kills = 20)
$ErrorActionPreference = "Stop"
$repo = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$exe = Join-Path $repo "target\release\s13.exe"; if (-not (Test-Path $exe)) { throw "build first: cargo build --release -p s13-leash-store" }
New-Item -ItemType Directory -Force $Work | Out-Null; Remove-Item "$Work\done.txt", "$Work\report.txt" -ErrorAction SilentlyContinue
$report = "$Work\report.txt"; function Say([string] $s) { Add-Content -Path $report -Value $s -Encoding ascii }
function Find-SdkTool([string] $name) {
    foreach ($kits in "${env:ProgramFiles(x86)}\Windows Kits\10\bin", "${env:ProgramFiles}\Windows Kits\10\bin") {
        if (-not (Test-Path $kits)) { continue }
        $hit = Get-ChildItem $kits -Recurse -Filter $name -ErrorAction SilentlyContinue | Where-Object { $_.FullName -match "\\x64\\" } | Sort-Object FullName | Select-Object -Last 1
        if ($hit) { return $hit.FullName }
    }
    throw "$name not found - install the Windows SDK."
}
function Wait-Result([string] $dir, [int] $seconds) { $f = Join-Path $dir "result.txt"; for ($i = 0; $i -lt $seconds; $i++) { if ((Test-Path $f) -and (Select-String -Path $f -Pattern "^done" -Quiet)) { return $true }; Start-Sleep 1 }; return $false }
function Strays { @(Get-Process s13 -ErrorAction SilentlyContinue).Count }
try {
    $started = Get-Date; $mp = Get-MpComputerStatus
    Say "started $started"; Say "Defender: real-time $($mp.RealTimeProtectionEnabled), signatures $($mp.AntivirusSignatureLastUpdated), cloud (MAPS) $((Get-MpPreference).MAPSReporting), detections on record before: $(@(Get-MpThreatDetection).Count)"
    $signTool = Find-SdkTool "signtool.exe"; $makeAppx = Find-SdkTool "MakeAppx.exe"
    # 1 - as built
    $d = "$Work\unsigned"; New-Item -ItemType Directory -Force $d | Out-Null; Copy-Item $exe "$d\s13.exe" -Force
    Start-Process "$d\s13.exe" -ArgumentList "soak", "--chunks", $Chunks, "--kills", $Kills, "--out", "$d\out" -Wait
    Say "== unsigned"; if (Test-Path "$d\out\result.txt") { Get-Content "$d\out\result.txt" | ForEach-Object { Say $_ } } else { Say "NO RESULT (the program did not run, or was stopped)" }; Say "s13 processes left: $(Strays)"; Say "the file is still there: $(Test-Path "$d\s13.exe")"
    # 2 - signed with the test certificate
    $d = "$Work\signed"; New-Item -ItemType Directory -Force $d | Out-Null; Copy-Item $exe "$d\s13.exe" -Force
    $o = & $signTool sign /fd SHA256 /sha1 $SignThumbprint "$d\s13.exe" 2>&1; if ($LASTEXITCODE -ne 0) { Say "signtool failed: $($o | Select-Object -Last 2)" }
    Start-Process "$d\s13.exe" -ArgumentList "soak", "--chunks", $Chunks, "--kills", $Kills, "--out", "$d\out" -Wait
    Say "== signed"; if (Test-Path "$d\out\result.txt") { Get-Content "$d\out\result.txt" | ForEach-Object { Say $_ } } else { Say "NO RESULT" }; Say "s13 processes left: $(Strays)"; Say "the file is still there: $(Test-Path "$d\s13.exe")"
    # 3 - inside a package
    $stage = "$Work\stage"; Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory -Force "$stage\images" | Out-Null
    Copy-Item $exe "$stage\s13.exe"; Copy-Item (Join-Path $PSScriptRoot "msix\AppxManifest.xml") $stage
    foreach ($n in "StoreLogo.png", "Square150x150Logo.png", "Square44x44Logo.png") { Copy-Item (Join-Path $repo "packaging\windows\msix-minnotes\images\$n") "$stage\images\" }
    $msix = "$Work\s13.msix"; & $makeAppx pack /d $stage /p $msix /o | Out-Null; if ($LASTEXITCODE -ne 0) { throw "MakeAppx failed" }
    $o = & $signTool sign /fd SHA256 /sha1 $SignThumbprint $msix 2>&1; if ($LASTEXITCODE -ne 0) { throw "signtool (package) failed: $($o | Select-Object -Last 2)" }
    $outp = Join-Path $env:USERPROFILE "s13-out"; Remove-Item $outp -Recurse -Force -ErrorAction SilentlyContinue
    Get-AppxPackage cbkow.S13Spike | Remove-AppxPackage -ErrorAction SilentlyContinue
    Add-AppxPackage $msix; $pkg = Get-AppxPackage cbkow.S13Spike
    Start-Process "shell:AppsFolder\$($pkg.PackageFamilyName)!S13"
    $ok = Wait-Result $outp 600
    Say "== packaged ($($pkg.PackageFullName))"; if (Test-Path "$outp\result.txt") { Get-Content "$outp\result.txt" | ForEach-Object { Say $_ } } else { Say "NO RESULT" }; if (-not $ok) { Say "the packaged run did not finish in 600 s" }
    Start-Sleep 2; Say "s13 processes left: $(Strays)"
    Get-Process s13 -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Get-AppxPackage cbkow.S13Spike | Remove-AppxPackage; Say "package removed: $(-not (Get-AppxPackage cbkow.S13Spike))"
    # Defender, after
    $mpcmd = Join-Path $env:ProgramFiles "Windows Defender\MpCmdRun.exe"
    foreach ($f in "$Work\unsigned\s13.exe", "$Work\signed\s13.exe", $msix) { $o = & $mpcmd -Scan -ScanType 3 -File $f -DisableRemediation 2>&1; Say "scan $(Split-Path $f -Leaf | ForEach-Object { "$((Split-Path (Split-Path $f) -Leaf))\$_" }): exit $LASTEXITCODE - $(($o | Where-Object { $_ -match 'found|threat' } | Select-Object -First 1))" }
    Say "Defender detections on record after: $(@(Get-MpThreatDetection).Count)"
    $ev = @(Get-WinEvent -FilterHashtable @{ LogName = "Microsoft-Windows-Windows Defender/Operational"; Id = 1006, 1007, 1015, 1116, 1117, 1121, 1122; StartTime = $started } -ErrorAction SilentlyContinue)
    Say "Defender events since the start (detections, behaviour, attack-surface rules): $($ev.Count)"; $ev | Select-Object -First 5 | ForEach-Object { Say "  $($_.Id): $(($_.Message -split "`n")[0])" }
    # 4 - the certification kit
    if ($Kit) {
        $appcert = "${env:ProgramFiles(x86)}\Windows Kits\10\App Certification Kit\appcert.exe"; $xml = "$Work\kit.xml"; Remove-Item $xml -ErrorAction SilentlyContinue
        & $appcert reset | Out-Null; $o = & $appcert test -appxpackagepath $msix -reportoutputpath $xml 2>&1; Say "== certification kit: exit $LASTEXITCODE"
        if (Test-Path $xml) { [xml] $r = Get-Content $xml; Say "overall: $($r.REPORT.OVERALL_RESULT)"; $r.REPORT.REQUIREMENTS.REQUIREMENT | ForEach-Object { $_.TEST } | Where-Object { $_.RESULT.InnerText -ne "PASS" } | ForEach-Object { Say "  $($_.RESULT.InnerText): $($_.NAME)" } } else { Say "no report: $($o | Select-Object -Last 3)" }
        Get-AppxPackage cbkow.S13Spike | Remove-AppxPackage -ErrorAction SilentlyContinue
    }
} catch { Say "FAILED: $_" }
Say "s13 processes at the end: $(Strays)"; Say "finished $(Get-Date)"
Set-Content "$Work\done.txt" "done"
