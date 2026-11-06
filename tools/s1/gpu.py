#!/usr/bin/env python3
"""GPU time per presented frame from an attached Metal System Trace: sums every command-buffer interval of the process
("… (Frame N)" rows, depth ≡ 4 mod 10 in xctrace's nesting; their encoders sit one level below) per frame number.
Usage: gpu.py <trace> [process=s1-present]. Also prints the per-encoder-label breakdown."""
import collections, re, sys
sys.path.insert(0, __file__.rsplit("/", 2)[0] + "/p0")
from xctrace_tables import table, num
from qcv_trace import pct
trace = sys.argv[1]; proc = sys.argv[2] if len(sys.argv) > 2 else "s1-present"
rows = [r for r in table(trace, "metal-application-intervals") if proc in (r["process"][1] or "")]
per_frame = collections.defaultdict(float); enc = collections.defaultdict(list)
for r in rows:
    lab = r["event-label"][1] or r["event-label"][0] or "?"; depth = int(r["event-depth"][0] or 0); d = num(r["duration"]) / 1e6
    m = re.search(r"\(Frame (\d+)", lab)
    if depth % 10 == 4 and m: per_frame[int(m.group(1))] += d
    elif depth % 10 == 5: enc[re.sub(r"\s*\(.*", "", lab)[:40]].append(d)
v = list(per_frame.values())
print(f"  GPU ms per frame (all command buffers): n={len(v)} p50={pct(v,50):.3f} p95={pct(v,95):.3f} max={max(v):.3f}")
for k, d in sorted(enc.items(), key=lambda kv: -sum(kv[1]))[:8]:
    print(f"    encoder {k:<40} n={len(d):4d} ms p50={pct(d,50):.3f} p95={pct(d,95):.3f} max={max(d):.3f}")
