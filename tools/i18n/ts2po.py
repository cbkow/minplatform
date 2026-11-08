#!/usr/bin/env python3
"""ts2po.py: QCView's Qt Linguist catalogs -> gettext .po (canon row 195, 2026-11-08).

    python3 -I tools/i18n/ts2po.py <QCView-Player>/translations apps/qcv/i18n

One-time carry of the 12 catalogs. English text stays the key (msgid), QCView's context becomes msgctxt
(plus "|disambiguation" when Qt had one), a developer comment becomes "#.", plurals become msgid_plural +
msgstr[i] in Qt's form order, which is gettext's order for these languages. Obsolete / vanished rows are
dropped. Every row is carried as reviewed: the user ruled the "machine translation" label off (row 196).
"""
import sys, pathlib, xml.etree.ElementTree as ET

# Plural-Forms headers matching Qt's numerus rules for the shipped languages.
PLURALS = {
    "one":    "nplurals=1; plural=0;",
    "en":     "nplurals=2; plural=(n != 1);",
    "fr":     "nplurals=2; plural=(n > 1);",
    "slavic": "nplurals=3; plural=(n%10==1 && n%100!=11 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);",
    "pl":     "nplurals=3; plural=(n==1 ? 0 : n%10>=2 && n%10<=4 && (n%100<10 || n%100>=20) ? 1 : 2);",
}
RULE = {"ja": "one", "ko": "one", "zh_CN": "one", "fr": "fr", "pt_BR": "fr", "pl": "pl", "ru": "slavic", "uk": "slavic"}

def q(s):
    s = s.replace("\\", "\\\\").replace('"', '\\"').replace("\t", "\\t")
    if "\n" not in s: return f'"{s}"'
    parts = s.split("\n")
    return '""\n' + "\n".join(f'"{p}\\n"' for p in parts[:-1]) + (f'\n"{parts[-1]}"' if parts[-1] else "")

def main(src, dst):
    for ts in sorted(pathlib.Path(src).glob("qcview_*.ts")):
        lang = ts.stem[len("qcview_"):]
        root = ET.parse(ts).getroot()
        out = ['msgid ""', 'msgstr ""', f'"Language: {lang}\\n"', '"MIME-Version: 1.0\\n"',
               '"Content-Type: text/plain; charset=UTF-8\\n"', f'"Plural-Forms: {PLURALS[RULE.get(lang, "en")]}\\n"',
               '"X-Source: QCView translations/qcview_' + lang + '.ts (ts2po.py)\\n"', ""]
        n = 0
        for ctx in root.iter("context"):
            cname = ctx.findtext("name")
            for m in ctx.iter("message"):
                tr = m.find("translation")
                if tr is not None and tr.get("type") in ("obsolete", "vanished"): continue
                source = m.findtext("source") or ""
                dis = m.findtext("comment")
                note = m.findtext("extracomment")
                if note: out += [f"#. {l}" for l in note.splitlines()]
                out.append("msgctxt " + q(cname + (f"|{dis}" if dis else "")))
                out.append("msgid " + q(source))
                if m.get("numerus") == "yes":
                    out.append("msgid_plural " + q(source))
                    forms = [f.text or "" for f in tr.iter("numerusform")] if tr is not None else [""]
                    out += [f"msgstr[{i}] " + q(f) for i, f in enumerate(forms)]
                else:
                    out.append("msgstr " + q((tr.text if tr is not None else "") or ""))
                out.append(""); n += 1
        pathlib.Path(dst, f"qcview_{lang}.po").write_text("\n".join(out), encoding="utf-8")
        print(f"{lang}: {n} messages")

if __name__ == "__main__": main(sys.argv[1], sys.argv[2])
