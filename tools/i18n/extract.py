#!/usr/bin/env python3
"""List the interface strings apps/qcv looks up that a catalog lacks (canon row 197's extraction).

Reads every `apps/qcv/src/*.rs`, learns each file's helpers (`fn tr(s) -> … trc("Ctx", s)`), and collects the
literal lookups: `helper("…")`, `min_i18n::trc("Ctx", "…")`, `trc_hint`, `trn`/`trnc`. A helper called on a
non-literal is reported as INDIRECT (the strings reach it from a table: check them by hand, or run the app with
`QCV_I18N_MISSES=<file>` under a language and read what it recorded).

  python3 -I tools/i18n/extract.py [--lang de] [--json]
"""
import glob, json, os, re, sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
STR = r'"((?:[^"\\]|\\.)*)"'

def unesc(s): return bytes(s, 'utf-8').decode('unicode_escape').encode('latin-1').decode('utf-8')

def catalog(code):
    po = open(os.path.join(ROOT, 'apps/qcv/i18n', f'qcview_{code}.po'), encoding='utf-8').read()
    have = {}
    for block in po.split('\n\n'):
        ctx = re.search(r'^msgctxt ' + STR, block, re.M); mid = re.search(r'^msgid ' + STR + r'((?:\n"(?:[^"\\]|\\.)*")*)', block, re.M)
        if not mid: continue
        idtext = unesc(mid.group(1)) + ''.join(unesc(x) for x in re.findall(STR, mid.group(2) or ''))
        strs = re.findall(r'^msgstr(?:\[\d\])? ' + STR + r'((?:\n"(?:[^"\\]|\\.)*")*)', block, re.M)
        val = ''.join(unesc(a) + ''.join(unesc(x) for x in re.findall(STR, b)) for a, b in strs[:1])
        c = ctx.group(1).split('|')[0] if ctx else ''
        have[(c, idtext)] = val
    return have

def lookups():
    found, indirect = {}, []
    for f in sorted(glob.glob(os.path.join(ROOT, 'apps/qcv/src/*.rs'))):
        src = open(f, encoding='utf-8').read(); name = os.path.basename(f)
        helpers = {m.group(1): m.group(3) for m in re.finditer(r'fn (\w+)\s*\(\s*s\s*:[^)]*\)[^{]*\{\s*min_i18n::(trc|trc_hint)\("([^"]+)",\s*s\)', src)}
        for h, ctx in helpers.items():
            for m in re.finditer(r'(?<![\w:.])' + h + r'\(\s*(' + STR + r'|[^)"]+)', src):
                arg = m.group(1)
                if arg.startswith('"'): found.setdefault((ctx, unesc(m.group(2))), set()).add(name)
                elif not arg.strip().startswith('s)') and 'fn ' not in src[max(0, m.start() - 4):m.start()]:
                    indirect.append((name, h, arg.strip()[:40]))
        for m in re.finditer(r'min_i18n::(?:trc|trc_hint)\(\s*"([^"]+)",\s*' + STR, src): found.setdefault((m.group(1), unesc(m.group(2))), set()).add(name)
        for m in re.finditer(r'min_i18n::trnc?\(\s*(?:(?:Some\()?"([^"]+)"\)?,\s*)?' + STR, src): found.setdefault((m.group(1) or '', unesc(m.group(2))), set()).add(name)
    return found, indirect

def missing(code, found):
    have = catalog(code); ids = {}
    for (c, i), v in have.items(): ids.setdefault(i, v or ids.get(i, ''))
    out = []
    for (c, i), files in sorted(found.items()):
        v = have.get((c, i)) or ids.get(i)
        # a tooltip with a key hint reads as its translated head + the hint (min_i18n::trc_hint)
        if not v and i.endswith(')') and ' (' in i: h = i[:i.rfind(' (')]; v = have.get((c, h)) or ids.get(h)
        if not v: out.append({'ctx': c, 'id': i, 'files': sorted(files)})
    return out

if __name__ == '__main__':
    code = sys.argv[sys.argv.index('--lang') + 1] if '--lang' in sys.argv else 'de'
    found, indirect = lookups(); miss = missing(code, found)
    if '--json' in sys.argv: print(json.dumps({'missing': miss, 'indirect': indirect}, ensure_ascii=False, indent=1)); sys.exit()
    print(f'{len(found)} literal lookups; {len(miss)} without a {code} translation')
    for m in miss: print(f"  [{m['ctx']}] {m['id']!r}  ({', '.join(m['files'])})")
    print(f'{len(indirect)} indirect calls (strings from tables):')
    for n, h, a in indirect: print(f'  {n}: {h}({a}…)')
