# Fetch the pinned Windows FFmpeg into <repo>/external/ffmpeg/win64 (gitignored) and print the FFMPEG_DIR to use.
# Verifies the sha256 in ffmpeg.pin (the pin names a DATED BtbN release asset, which is immutable; a mismatch means a
# corrupt download). To move the pin: pick the dated tag, update WIN64_TAG / WIN64_URL / WIN64_BUILD / WIN64_SHA256 /
# WIN64_BYTES after checking `ffmpeg -version` and `-h decoder=prores` on the new build.
$ErrorActionPreference = "Stop"
$repo = Resolve-Path (Join-Path $PSScriptRoot "..\..")
$pin = @{}; Get-Content (Join-Path $PSScriptRoot "ffmpeg.pin") | Where-Object { $_ -match "^[A-Z0-9_]+=" } | ForEach-Object { $k, $v = $_ -split "=", 2; $pin[$k] = $v }
$dst = Join-Path $repo "external\ffmpeg\win64"
$zip = Join-Path $repo "external\ffmpeg\$($pin.WIN64_BUILD)-win64-gpl-shared.zip"
New-Item -ItemType Directory -Force (Split-Path $zip) | Out-Null
if (-not (Test-Path $zip)) { Write-Host "downloading $($pin.WIN64_URL)"; Invoke-WebRequest -Uri $pin.WIN64_URL -OutFile $zip }
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash
if ($hash -ne $pin.WIN64_SHA256) { Remove-Item $zip; throw "sha256 mismatch: got $hash, the pin says $($pin.WIN64_SHA256)" }
if (Test-Path $dst) { Remove-Item -Recurse -Force $dst }
$tmp = Join-Path $repo "external\ffmpeg\.unzip"; if (Test-Path $tmp) { Remove-Item -Recurse -Force $tmp }
Expand-Archive -Path $zip -DestinationPath $tmp -Force
Move-Item (Get-ChildItem $tmp | Select-Object -First 1).FullName $dst; Remove-Item -Recurse -Force $tmp
$ver = & (Join-Path $dst "bin\ffmpeg.exe") -version 2>&1 | Select-Object -First 1
Write-Host $ver
Write-Host "FFMPEG_DIR=$dst"
Write-Host "PATH += $dst\bin (the shared DLLs at run time)"
