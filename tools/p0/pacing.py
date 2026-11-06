#!/usr/bin/env python3
"""P0 playback pacing. Usage: pacing.py <trace> [window-start-ms window-end-ms]"""
import sys
from qcv_trace import load, pct
trace = sys.argv[1]; lo = float(sys.argv[2]) if len(sys.argv) > 2 else 0; hi = float(sys.argv[3]) if len(sys.argv) > 3 else 1e12
req, comp, gpu, cpu = load(trace)
period = 1000 / 23.976
def report(name, d):
    for wh, ts in sorted(d.items(), key=lambda kv: -len(kv[1])):
        t = [x for x in ts if lo <= x <= hi]; iv = [b - a for a, b in zip(t, t[1:])]
        if len(iv) < 5: continue
        print(f"  {name:<16} layer {wh[0]}x{wh[1]}: n={len(t)} interval ms p50={pct(iv,50):.1f} p95={pct(iv,95):.1f} max={max(iv):.1f}"
              f"  >1.5x period: {sum(1 for d in iv if d > 1.5*period)}  <0.5x: {sum(1 for d in iv if d < 0.5*period)}")
report("present-request", req); report("compositor-read", comp)
if gpu: print(f"  GPU command buffers: n={len(gpu)} ms p50={pct(gpu,50):.2f} p95={pct(gpu,95):.2f} max={max(gpu):.2f}")
busy = [u for t, u in cpu if lo <= t <= hi]
if busy: print(f"  process CPU % (10 ms buckets): mean={sum(busy)/len(busy):.0f} p95={pct(busy,95):.0f} max={max(busy):.0f}")
