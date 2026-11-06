#!/usr/bin/env python3
"""P0: startup milestones + decoder telemetry from one QCView log. Usage: qcview-log.py <log>"""
import re, sys
L = [l.rstrip('\n') for l in open(sys.argv[1], errors='replace')]
def t(line):
    m = re.match(r'\[(\d+):(\d+):(\d+)\.(\d+)', line)
    return int(m[1])*3600 + int(m[2])*60 + int(m[3]) + int(m[4])/1000 if m else None
def body(l): return re.sub(r'^\S+ \S+ \S+ +', '', l)
t0 = t(L[0])
def first(pat):
    return next((l for l in L if re.search(pat, l)), None)
for name, pat in [("present loop", r"present loop started"), ("window on", r"restored window frame"),
                  ("avformat open", r"VideoDecoder: avformat_open_input done"), ("get_format", r"get_format"),
                  ("first automation", r"QCV_AUTOPLAY_SECONDS: play|QCV_START_SECONDS: seek|QCV_SCRUB_FRAMES: scrub"),
                  ("quit", r"QCV_QML_EVAL: Qt.quit")]:
    l = first(pat)
    print(f"  {name:<18} +{((t(l)-t0)*1000 if l else float('nan')):7.0f} ms  {body(l)[:120] if l else '—'}")
for l in L:
    if re.search(r"swscale|ReadAhead:.*MB/s|QCV_START_SECONDS|QCV_SCRUB_FRAMES|drop|ScrubDecoder: (?!avformat)|zero.?copy|hwfc=|MetalTexturePool: init", l, re.I):
        print(f"    +{(t(l)-t0)*1000:7.0f}  {body(l)[:140]}")
