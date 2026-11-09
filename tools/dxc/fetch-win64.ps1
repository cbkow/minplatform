# Fetch the pinned DXC (tools/dxc/dxc.pin) into <repo>/external/dxc/win64 (gitignored): dxcompiler.dll + its licences only.
# qcv finds it beside qcv.exe (packaging) or here (development); canon row 226.
$ErrorActionPreference = "Stop"
$repo = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$pin = @{}; Get-Content (Join-Path $PSScriptRoot "dxc.pin") | Where-Object { $_ -match "^[A-Z0-9_]+=" } | ForEach-Object { $k, $v = $_ -split "=", 2; $pin[$k] = $v }
$dst = Join-Path $repo "external\dxc\win64"
$zip = Join-Path $repo "external\dxc\dxc-$($pin.DXC_VERSION).zip"
New-Item -ItemType Directory -Force (Split-Path $zip) | Out-Null
if (-not (Test-Path $zip)) { Write-Host "downloading $($pin.DXC_URL)"; Invoke-WebRequest -Uri $pin.DXC_URL -OutFile $zip }
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash
if ($hash -ne $pin.DXC_SHA256) { Remove-Item $zip; throw "sha256 mismatch: got $hash, the pin says $($pin.DXC_SHA256)" }
$tmp = Join-Path $repo "external\dxc\.unzip"; if (Test-Path $tmp) { Remove-Item -Recurse -Force $tmp }
Expand-Archive -Path $zip -DestinationPath $tmp -Force
if (Test-Path $dst) { Remove-Item -Recurse -Force $dst }
New-Item -ItemType Directory -Force $dst | Out-Null
Copy-Item (Join-Path $tmp "bin\x64\dxcompiler.dll") $dst
foreach ($l in @("LICENSE-LLVM.txt", "LICENCE-MIT.txt")) { Copy-Item (Join-Path $tmp $l) $dst }
Remove-Item -Recurse -Force $tmp
Write-Host "DXC $($pin.DXC_VERSION) -> $dst"
