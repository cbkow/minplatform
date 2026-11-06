#!/usr/bin/env python3
"""P0 seek/scrub latency, no app instrumentation: a seek = idle→busy CPU transition (10 ms buckets, ≥40 % after
300 ms under 12 % mean); latency = onset → next present REQUEST of the player layer → next COMPOSITOR READ of it."""
import sys
from qcv_trace import load, pct
req, comp, gpu, cpu = load(sys.argv[1])
def player(d):
    ks = [k for k in d if isinstance(k[0], int)]
    return d[min(ks, key=lambda k: k[0] * k[1])] if ks else []
preq, pcomp = player(req), player(comp)
onsets = []
for i, (t, u) in enumerate(cpu):
    if u >= 40:
        prev = [v for s, v in cpu[max(0, i - 30):i]]
        if len(prev) == 30 and sum(prev) / 30 < 12 and (not onsets or t - onsets[-1] > 500): onsets.append(t)
print(f"  layers req={[(k, len(v)) for k, v in req.items()]} onsets={len(onsets)}")
lr, lc = [], []
for o in onsets:
    a = next((s for s in preq if s > o), None); b = next((s for s in pcomp if s > o), None)
    if a: lr.append(a - o)
    if b: lc.append(b - o)
    print(f"    onset {o:8.0f} → player present-request +{(a-o) if a else float('nan'):4.0f} ms, composited +{(b-o) if b else float('nan'):4.0f} ms")
for name, v in (("present-request", lr), ("composited", lc)):
    if v: print(f"  onset→{name} ms: p50={pct(v,50):.0f} min={min(v):.0f} max={max(v):.0f} (n={len(v)}, ±10 ms)")
