#!/bin/sh
# run-live.sh <script> — run.sh with the synthetic QCBridge host (`qcbae://probe`, "QCBridge Test Signal") publishing while the
# script runs. Only the probe's own ring is used: a real After Effects / Premiere feed on this machine is never replaced.
set -e
cd "$(dirname "$0")/../.."
[ -x target/release/examples/probe ] || cargo build --release -p qcv-ring --example probe
./target/release/examples/probe probe 1920x1080 24 120 0 >/dev/null 2>&1 &
P=$!; trap 'kill $P 2>/dev/null' EXIT
sleep 1
sh tools/docs-shots/run.sh "$1"
