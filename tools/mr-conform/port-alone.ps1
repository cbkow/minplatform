# The Rust node alone on a Windows machine, led by the harness's stand-in leader (canon row 394): a scratch farm in a
# local folder, loopback addresses, its own ports. It cannot reach a real farm and uses no real data folder.
#   powershell -NoProfile -ExecutionPolicy Bypass -File tools\mr-conform\port-alone.ps1 -Work C:\some\mr-farm
# (the folder's name must be `mr-farm`: the harness refuses any other). Writes <Work>\lead\table.md.
param([Parameter(Mandatory)] [string] $Work)
$ErrorActionPreference = "Stop"
$repo = Resolve-Path (Join-Path $PSScriptRoot "..\.."); $bin = Join-Path $repo "target\release"
if ((Split-Path $Work -Leaf) -ne "mr-farm") { throw "the folder must be named mr-farm" }
Get-Process minRender -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$bin*" } | Stop-Process -Force
Remove-Item $Work -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory -Force "$Work\share", "$Work\p\data" | Out-Null
$share = ("$Work\share" -replace "\\", "/")
& "$bin\mr-conform.exe" make-farm --share $share; if ($LASTEXITCODE -ne 0) { throw "make-farm failed" }
Set-Content "$Work\p\data\config.json" ('{"sync_root":"' + $share + '","http_port":8439,"ip_override":"127.0.0.1","udp_enabled":false,"udp_port":4339,"node_stopped":false,"tags":["noleader"],"staging_enabled":false,"path_mappings":[]}') -Encoding ascii
$node = Start-Process "$bin\minRender.exe" -ArgumentList "--data", "$Work\p\data", "--no-window" -WindowStyle Hidden -PassThru -RedirectStandardError "$Work\p\err.log" -RedirectStandardOutput "$Work\p\out.log"
try { & "$bin\mr-conform.exe" lead --share $share --port 8440 --out "$Work\lead" 2>&1 | Set-Content "$Work\lead.log" } finally { Stop-Process -Id $node.Id -Force -ErrorAction SilentlyContinue }
Start-Sleep 1
"left running: minRender $(@(Get-Process minRender -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$bin*" }).Count), fake renderers $(@(Get-Process mr-conform -ErrorAction SilentlyContinue).Count)" | Add-Content "$Work\lead.log"
Get-Process mr-conform -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
