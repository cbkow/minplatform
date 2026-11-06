# S4 on Windows: two-process ring tests on the logged-on desktop (the consumer needs a window). Rust producer → Rust consumer
# (one-copy DX12), MSVC C++ producer (QCBridgeAE's SharedRing) → Rust consumer, and the QCVO producer creating its section.
param([string]$Only = "")
$Exe = "$HOME\Documents\GitHub\minplatform\target\release\s4-ring.exe"
$Cxx = "$HOME\Documents\GitHub\minplatform\target\s4-cxx\cxx_producer.exe"
$logs = "C:\qcb-lab\minplatform-logs"
$runs = [ordered]@{
  "s4-rust-rust" = "start /b `"`" `"$Exe`" --produce /qcbae-probe --seconds 16 > $logs\s4-rust-rust-prod.log 2>&1`r`nping -n 2 127.0.0.1 > nul`r`n`"$Exe`" --consume /qcbae-probe --screen 0 --seconds 12"
  "s4-cxx-rust"  = "start /b `"`" `"$Cxx`" /qcbae-probe 16 > $logs\s4-cxx-rust-prod.log 2>&1`r`nping -n 2 127.0.0.1 > nul`r`n`"$Exe`" --consume /qcbae-probe --screen 0 --seconds 12"
  "s4-qcvo"      = "`"$Exe`" --output ndi --no-launch --seconds 4"
}
foreach ($k in $runs.Keys) {
  if ($Only -and $k -ne $Only) { continue }
  $cmd = "@echo off`r`nset RUST_BACKTRACE=1`r`ncd /d $HOME\Documents\GitHub\minplatform`r`n$($runs[$k]) > $logs\$k.log 2>&1`r`necho done > $logs\$k.done`r`n"
  Set-Content -Path "C:\qcb-lab\mp-$k.cmd" -Value $cmd -Encoding ascii
  Remove-Item "$logs\$k.done" -ErrorAction SilentlyContinue
  powershell -NoProfile -ExecutionPolicy Bypass -File C:\qcb-lab\run.ps1 -Name "mp-$k" -Command "C:\qcb-lab\mp-$k.cmd"
  $t0 = Get-Date
  while (-not (Test-Path "$logs\$k.done") -and ((Get-Date) - $t0).TotalSeconds -lt 60) { Start-Sleep -Seconds 1 }
  "== $k  done=$(Test-Path "$logs\$k.done")"
  if (Test-Path "$logs\$k.log") { Get-Content "$logs\$k.log" | Select-String -Pattern "consumer:|producer|output|sink|panic|rror" | Select-Object -Last 16 | ForEach-Object { ($_.Line -replace '[^\x20-\x7E]', '?') } }
  if (Test-Path "$logs\$k-prod.log") { Get-Content "$logs\$k-prod.log" | Select-Object -Last 3 | ForEach-Object { ($_ -replace '[^\x20-\x7E]', '?') } }
  if (-not (Test-Path "$logs\$k.done")) { Stop-Process -Name s4-ring,cxx_producer -Force -ErrorAction SilentlyContinue }
}
