#!/bin/zsh
# One traced `qcv` run (Phase 2 step 1 evidence). Usage: trace.sh <label> <seconds> [qcv args ...]
# Launches the release binary with --stats, attaches a Metal System Trace after ~1.5 s, then prints pacing + GPU
# command-buffer time from the trace (tools/p0/pacing.py + tools/s1/gpu.py with PROC=qcv).
# Output: fixtures/phase2/<label>.trace|.out (ignored by git).
set -u
ROOT=${0:A:h:h:h}; OUT=$ROOT/fixtures/phase2; mkdir -p $OUT
label=$1; secs=$2; shift 2
BIN=$ROOT/target/release/QCView
rm -rf $OUT/$label.trace
$BIN --stats --seconds $secs "$@" > $OUT/$label.out 2>&1 & pid=$!
sleep 1.5
xcrun xctrace record --template 'Metal System Trace' --time-limit $(( secs - 2 ))s --output $OUT/$label.trace --attach $pid 2>&1 \
  | grep -v "^Ctrl-C\|^Starting\|^Reached\|^Recording\|^Output"
wait $pid 2>/dev/null
echo "== $label: $(grep -E 'first frame|opened' $OUT/$label.out | tr '\n' ' ')"
grep -E 'present ms|lateness|cadence|CPU|RSS|EDR' $OUT/$label.out | tail -6 | sed 's/^/   app: /'
PROC=qcv python3 $ROOT/tools/p0/pacing.py $OUT/$label.trace
python3 $ROOT/tools/s1/gpu.py $OUT/$label.trace qcv
