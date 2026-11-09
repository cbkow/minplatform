#!/usr/bin/env python3
"""Append drafted translations to apps/qcv/i18n/qcview_<code>.po (canon row 197).

Input: a JSON object {"<context>\\u0004<English>": {"de": "…", "fr": "…", …}}. Each new entry is written with the
comment `#. drafted <date>, unreviewed` so a reviewer finds every draft with one search; an entry the catalog already
holds (same context and English) is left alone. Refuses a draft whose %1..%9 / %n placeholders differ from the English
or that drops the English's trailing "(key)" hint (QCView's import rules, translations/README.md).

  python3 -I tools/i18n/add_drafts.py drafts.json [--date 2026-11-09]
"""
import json, os, re, sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
def q(s): return '"' + s.replace('\\', '\\\\').replace('"', '\\"').replace('\n', '\\n') + '"'
def ph(s): return sorted(set(re.findall(r'%[1-9n]', s)))

def main():
    src = sys.argv[1]; date = sys.argv[sys.argv.index('--date') + 1] if '--date' in sys.argv else '2026-11-09'
    drafts = json.load(open(src, encoding='utf-8'))
    bad = []
    for key, by_lang in drafts.items():
        ctx, eng = key.split('\x04', 1)
        hint = re.search(r' (\([^()]*\))$', eng)
        for code, t in by_lang.items():
            if ph(t) != ph(eng): bad.append((code, eng, t, 'placeholders'))
            # the hint's keys stay as written ("(hold; L)" may translate "hold", never the L)
            keys = re.findall(r'(?:Ctrl|Shift|Alt|Esc|Space|Home|End|[⌘⌥⇧←→])[\w+⌘⌥⇧←→]*|\b[A-Z]\b', hint.group(1)) if hint else []
            if ctx in ('TransportBar', 'CompactStrip') and any(k not in t for k in keys): bad.append((code, eng, t, 'key hint'))
    if bad:
        for b in bad: print('REFUSED', b)
        sys.exit(1)
    langs = sorted({c for v in drafts.values() for c in v})
    for code in langs:
        p = os.path.join(ROOT, 'apps/qcv/i18n', f'qcview_{code}.po')
        po = open(p, encoding='utf-8').read()
        have = set(re.findall(r'msgctxt "((?:[^"\\]|\\.)*)"\nmsgid "((?:[^"\\]|\\.)*)"', po))
        add = []
        for key, by_lang in drafts.items():
            ctx, eng = key.split('\x04', 1)
            if code not in by_lang: continue
            if (q(ctx)[1:-1], q(eng)[1:-1]) in have: continue
            add.append(f'#. drafted {date}, unreviewed\nmsgctxt {q(ctx)}\nmsgid {q(eng)}\nmsgstr {q(by_lang[code])}\n')
        if add:
            po = po.rstrip('\n') + '\n\n' + '\n'.join(add)
            open(p, 'w', encoding='utf-8').write(po)
        print(f'{code}: {len(add)} drafts added')

if __name__ == '__main__': main()
