#!/usr/bin/env python3
"""check-glyphs.py - every character of the manual's pages must be in the application's own text faces (Geist, Geist
Mono): one that is not is drawn from whatever the system has, or as an empty box (row 376). Run by sync-guide.sh.
  python3 tools/mn-shots/check-glyphs.py <folder of .md pages>     exit 1 and a list when a character is missing"""
import glob, struct, sys, os

def cmap(path):
    d = open(path, 'rb').read(); n = struct.unpack('>H', d[4:6])[0]; tabs = {}
    for i in range(n):
        tag, _, off, _ = struct.unpack('>4sIII', d[12 + 16 * i:28 + 16 * i]); tabs[tag] = off
    o = tabs[b'cmap']; nt = struct.unpack('>H', d[o + 2:o + 4])[0]; cps = set()
    for i in range(nt):
        _, _, so = struct.unpack('>HHI', d[o + 4 + 8 * i:o + 12 + 8 * i]); s = o + so; fmt = struct.unpack('>H', d[s:s + 2])[0]
        if fmt == 4:
            segx2 = struct.unpack('>H', d[s + 6:s + 8])[0]; seg = segx2 // 2
            ends = struct.unpack(f'>{seg}H', d[s + 14:s + 14 + segx2]); starts = struct.unpack(f'>{seg}H', d[s + 16 + segx2:s + 16 + 2 * segx2])
            deltas = struct.unpack(f'>{seg}h', d[s + 16 + 2 * segx2:s + 16 + 3 * segx2]); ro = s + 16 + 3 * segx2; ros = struct.unpack(f'>{seg}H', d[ro:ro + segx2])
            for k in range(seg):
                for c in range(starts[k], ends[k] + 1):
                    if ros[k] == 0: g = (c + deltas[k]) & 0xFFFF
                    else: p = ro + 2 * k + ros[k] + 2 * (c - starts[k]); g = struct.unpack('>H', d[p:p + 2])[0]
                    if g: cps.add(c)
        elif fmt == 12:
            ng = struct.unpack('>I', d[s + 12:s + 16])[0]
            for k in range(ng):
                a, b, _ = struct.unpack('>III', d[s + 16 + 12 * k:s + 28 + 12 * k]); cps.update(range(a, b + 1))
    return cps

root = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'crates', 'min-theme', 'assets', 'fonts')
faces = [f for f in sorted(glob.glob(os.path.join(root, 'Geist*.ttf')))]
have = set.intersection(*[cmap(f) for f in faces])   # a character may come in any weight and in code: it must be in all of them
bad = {}
for page in sorted(glob.glob(os.path.join(sys.argv[1], '*.md'))):
    for n, line in enumerate(open(page, encoding='utf-8'), 1):
        for ch in line:
            if ord(ch) > 126 and ord(ch) not in have: bad.setdefault(ch, []).append(f'{os.path.basename(page)}:{n}')
for ch, where in bad.items(): print(f'not in the faces: {ch!r} U+{ord(ch):04X} x{len(where)} - {", ".join(where[:6])}{" ..." if len(where) > 6 else ""}')
sys.exit(1 if bad else 0)
