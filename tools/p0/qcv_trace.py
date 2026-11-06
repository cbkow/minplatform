#!/usr/bin/env python3
"""Shared loader for QCView traces (attached Metal System Trace).
Per CAMetalLayer (told apart by IOSurface size): the app's PRESENT REQUESTS (ca-client-present-request joined to
qcview's command buffers) and the COMPOSITOR READS of those surfaces (metal-io-surface-access rows by WindowServer
= the frame was composited to the display). The window-sized surface is Qt Quick's layer; the viewport-sized one is
the native player layer, whose cadence is the video's."""
import collections
from xctrace_tables import table, num

def load(trace):
    size, ws_reads = {}, collections.defaultdict(list)
    qsurf = set()
    for r in table(trace, "metal-io-surface-access"):
        sid = r["surface-id"][0]
        if not sid: continue
        if r["width"][0]: size[sid] = (int(r["width"][0]), int(r["height"][0]))
        if "qcview" in (r["process"][1] or ""): qsurf.add(sid)
        elif "WindowServer" in (r["process"][1] or ""): ws_reads[sid].append(num(r["timestamp"]) / 1e6)
    qcb = {r["cmdbuffer-id"][0] for r in table(trace, "metal-application-intervals") if "qcview" in (r["process"][1] or "")}
    req = collections.defaultdict(list)       # (w,h) -> [t_ms] present requests
    for r in table(trace, "ca-client-present-request"):
        if r["cmdbuffer-id"][0] in qcb and r["surface-id"][0]:
            req[size.get(r["surface-id"][0], ("?", r["surface-id"][0]))].append(num(r["timestamp"]) / 1e6)
    comp = collections.defaultdict(list)      # (w,h) -> [t_ms] compositor reads of qcview surfaces
    for sid in qsurf:
        comp[size.get(sid, ("?", sid))].extend(ws_reads.get(sid, []))
    for d in (req, comp):
        for k in d: d[k].sort()
    gpu = [num(r["duration"]) / 1e6 for r in table(trace, "metal-application-intervals")
           if "qcview" in (r["process"][1] or "") and r["event-depth"][0] in ("0", None)
           and (r["event-label"][1] or "").startswith("Command Buffer") and num(r["duration"])]
    cpu = sorted((num(r["start"]) / 1e6, num(r["cpu-usage"])) for r in table(trace, "ProcessCPUUsage")
                 if "qcview" in (r["process"][1] or ""))
    return req, comp, gpu, cpu

def pct(v, p):
    if not v: return float("nan")
    v = sorted(v); return v[min(len(v) - 1, int(round(p / 100 * (len(v) - 1))))]
