#!/bin/zsh
# One traced s1-present run (S1 step 2). Usage: run-trace.sh <label> <seconds> [s1-present args ...]
# Launches the release binary (built with --features ocio), attaches a Metal System Trace after ~1.5 s, then
# prints pacing + GPU command-buffer time from the trace (tools/p0/pacing.py with PROC=s1-present).
# Output: fixtures/s1-trace/<label>.trace|.out (ignored by git).
set -u
ROOT=${0:A:h:h:h}; OUT=$ROOT/fixtures/s1-trace; mkdir -p $OUT
label=$1; secs=$2; shift 2
BIN=$ROOT/target/release/s1-present
rm -rf $OUT/$label.trace
$BIN --seconds $secs "$@" > $OUT/$label.out 2>&1 & pid=$!
sleep 1.5
xcrun xctrace record --template 'Metal System Trace' --time-limit $(( secs - 2 ))s --output $OUT/$label.trace --attach $pid 2>&1 \
  | grep -v "^Ctrl-C\|^Starting\|^Reached\|^Recording\|^Output"
wait $pid 2>/dev/null
echo "== $label: $(grep -E 'first frame presented|OCIO pass|verify' $OUT/$label.out | tr '\n' ' ')"
grep -E 'present ms' $OUT/$label.out | tail -2 | sed 's/^/   app: /'
PROC=s1-present python3 $ROOT/tools/p0/pacing.py $OUT/$label.trace
python3 $ROOT/tools/s1/gpu.py $OUT/$label.trace
