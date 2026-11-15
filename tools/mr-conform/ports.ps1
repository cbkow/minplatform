# A farm of Rust nodes ALONE on a Windows machine (canon row 397): a scratch share in a local folder, loopback
# addresses, their own ports. It cannot reach a real farm and uses no real data folder.
#   powershell -NoProfile -ExecutionPolicy Bypass -File tools\mr-conform\ports.ps1 -Work C:\some\mr-farm [-Nodes 2]
# (the folder's name must be `mr-farm`.) It records and checks one node, drives the farm through every job command,
# checks that, stops the nodes, and writes <Work>\result.txt.
param([Parameter(Mandatory)] [string] $Work, [int] $Nodes = 2)
$ErrorActionPreference = "Stop"
$repo = Resolve-Path (Join-Path $PSScriptRoot "..\.."); $bin = Join-Path $repo "target\release"
if ((Split-Path $Work -Leaf) -ne "mr-farm") { throw "the folder must be named mr-farm" }
function Ours { @(Get-Process minRender -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$bin*" }) }
Ours | Stop-Process -Force; Remove-Item $Work -Recurse -Force -ErrorAction SilentlyContinue; New-Item -ItemType Directory -Force "$Work\share" | Out-Null
$share = ("$Work\share" -replace "\\", "/"); $procs = @()
for ($i = 1; $i -le $Nodes; $i++) {
    New-Item -ItemType Directory -Force "$Work\p$i\data" | Out-Null
    Set-Content "$Work\p$i\data\config.json" ('{"sync_root":"' + $share + '","http_port":' + (8440 + $i) + ',"ip_override":"127.0.0.1","udp_enabled":false,"udp_port":' + (4340 + $i) + ',"node_stopped":false,"tags":[],"staging_enabled":false,"path_mappings":[]}') -Encoding ascii
    $procs += Start-Process "$bin\minRender.exe" -ArgumentList "--data", "$Work\p$i\data", "--no-window" -WindowStyle Hidden -PassThru -RedirectStandardError "$Work\p$i\err.log" -RedirectStandardOutput "$Work\p$i\out.log"
    Start-Sleep 1
}
$out = "$Work\result.txt"; function Say([string] $s) { Add-Content $out $s -Encoding ascii }
try {
    Start-Sleep 13; $cfg = "$Work\p1\data\config.json"
    & "$bin\mr-conform.exe" record --config $cfg --out "$Work\rec" | Out-Null; Say "== one node recorded"; & "$bin\mr-conform.exe" check "$Work\rec" 2>&1 | Where-Object { $_ -notmatch "^note" } | ForEach-Object { Say $_ }
    & "$bin\mr-conform.exe" drive --config $cfg --out "$Work\drive" 2>&1 | Select-Object -Last 1 | ForEach-Object { Say $_ }; Say "== the farm driven"; & "$bin\mr-conform.exe" check "$Work\drive" 2>&1 | Where-Object { $_ -notmatch "^note" } | ForEach-Object { Say $_ }
} catch { Say "FAILED: $_" } finally { $procs | ForEach-Object { Stop-Process -Id $_.Id -Force -ErrorAction SilentlyContinue } }
Start-Sleep 2
Say "left running: minRender $((Ours).Count), stand-in renderers $(@(Get-Process mr-conform -ErrorAction SilentlyContinue).Count)"
Get-Process mr-conform -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
