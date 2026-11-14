#!/usr/bin/env python3
"""A small three-page PDF of made-up delivery specifications, written by hand (no library): for the manual's pictures."""
import sys
PAGES = [
    ("Delivery Specification", ["Tidewater - short film", "", "1. Picture", "   Master: 3840 x 2160, 24 fps, Rec.709", "   Web: 1920 x 1080, H.264", "", "2. Sound", "   Stereo and 5.1, 48 kHz, 24 bit", "", "3. Naming", "   TW_<reel>_<version>.<ext>"]),
    ("Safe Areas", ["Titles stay inside 90 % of the frame.", "Action stays inside 93 %.", "", "Vertical cut: the centre 9:16 is protected.", "Square cut: the centre 1:1 is protected."]),
    ("Checklist", ["[ ] Picture conformed to the final cut", "[ ] Colour approved on the reference display", "[ ] Mix approved on small speakers", "[ ] Subtitles proofread", "[ ] Files named by the rule on page 1"]),
]
def esc(t): return t.replace("\\", "\\\\").replace("(", "\\(").replace(")", "\\)")
objs = []
def add(body): objs.append(body); return len(objs)
font = add(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
bold = add(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>")
pages_id = len(objs) + 1 + 2 * len(PAGES)
kids = []
for title, lines in PAGES:
    s = "BT /F2 26 Tf 72 720 Td (%s) Tj ET\n0.85 0.36 0.22 rg 72 704 468 3 re f 0 g\nBT /F1 13 Tf 72 670 Td 20 TL\n" % esc(title)
    for l in lines: s += "(%s) Tj T*\n" % esc(l)
    s += "ET\n"; data = s.encode("latin-1")
    c = add(b"<< /Length %d >>\nstream\n" % len(data) + data + b"endstream")
    kids.append(add(b"<< /Type /Page /Parent %d 0 R /MediaBox [0 0 612 792] /Contents %d 0 R /Resources << /Font << /F1 %d 0 R /F2 %d 0 R >> >> >>" % (pages_id, c, font, bold)))
assert add(b"<< /Type /Pages /Count %d /Kids [%s] >>" % (len(kids), b" ".join(b"%d 0 R" % k for k in kids))) == pages_id
cat = add(b"<< /Type /Catalog /Pages %d 0 R >>" % pages_id)
out = b"%PDF-1.4\n"; offs = []
for i, o in enumerate(objs): offs.append(len(out)); out += b"%d 0 obj\n" % (i + 1) + o + b"\nendobj\n"
x = len(out); out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objs) + 1)
for o in offs: out += b"%010d 00000 n \n" % o
out += b"trailer\n<< /Size %d /Root %d 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objs) + 1, cat, x)
open(sys.argv[1], "wb").write(out)
