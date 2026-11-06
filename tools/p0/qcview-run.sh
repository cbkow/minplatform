#!/bin/zsh
# One traced QCView run. Usage: qcview-run.sh <label> <fixture> <seconds> [ENV=VAL ...]
# Launches the DEV build directly (xctrace --launch went through LaunchServices and also started the
# installed /Applications copy), attaches a Metal System Trace ~1 s later, copies QCView's own log beside
# it, samples RSS every 0.5 s and takes one `footprint` snapshot mid-run. Startup timing comes from the
# log's own timestamps (first line → present loop → decoder ready), pacing/seek latency from the trace.
# Output: fixtures/p0-qcview/<label>.trace|.log|.mem|.footprint
set -u
ROOT=${0:A:h:h:h}; OUT=$ROOT/fixtures/p0-qcview; mkdir -p $OUT
label=$1; fix=$2; secs=$3; shift 3
APP=${QCV_APP:-$ROOT/../QCView-Player/build/dist/qcview.app/Contents/MacOS/qcview}
if pgrep -x qcview >/dev/null; then echo "qcview already running — single-instance would swallow the args"; exit 2; fi
rm -rf $OUT/$label.trace; : > $OUT/$label.mem
env QCV_QML_EVAL="$(( secs - 2 )):Qt.quit()" "$@" $APP "$fix" >/dev/null 2>&1 & pid=$!
( while kill -0 $pid 2>/dev/null; do ps -o rss= -p $pid >> $OUT/$label.mem; sleep 0.5; done ) & sampler=$!
( sleep $(( secs * 2 / 3 )); footprint -p $pid > $OUT/$label.footprint 2>&1 ) &
xcrun xctrace record --template 'Metal System Trace' --time-limit $(( secs + 1 ))s --output $OUT/$label.trace --attach $pid 2>&1 \
  | grep -v "^Ctrl-C\|^Starting\|^Reached\|^Recording\|^Output"
wait $pid 2>/dev/null; kill $sampler 2>/dev/null; wait 2>/dev/null
sleep 1; cp ~/Library/Logs/QCView/qcview-log.txt $OUT/$label.log
other=$(pgrep -x qcview); [ -n "$other" ] && echo "WARNING: another qcview appeared: $(ps -o lstart=,command= -p $other | cut -c1-90)"
printf "%-16s maxRSS=%5.0f MB  log lines=%s  %s\n" $label "$(sort -n $OUT/$label.mem | tail -1 | awk '{print $1/1024}')" "$(wc -l < $OUT/$label.log | tr -d ' ')" "$(grep -m1 'phys_footprint' $OUT/$label.footprint 2>/dev/null | tr -s ' ' | cut -c1-60)"
