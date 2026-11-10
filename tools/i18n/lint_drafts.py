#!/usr/bin/env python3
"""Lint the DRAFTED translations in apps/qcv/i18n (the entries marked "drafted … unreviewed") against QCView's glossary rules.

Per language: a never-translate term missing (TERM), the ellipsis or the final full stop not matching the English
(ELLIPSIS, PERIOD), a tooltip's parenthesised key hint changed (KEYHINT — a translated word inside it, as in "(hold; L)",
is fine), stray spaces, an informal form of address where the glossary chose the formal one (REGISTER), an untranslated
sentence (SAME), a short label grown long enough to clip (LONG), and a draft that differs from QCView's own translation of
the same English elsewhere (VS-CARRIED: check it is a different meaning, as the View menu and OCIO's View are).

  python3 -I tools/i18n/lint_drafts.py [findings.json]
"""
import glob, os, re, sys, collections, json
ROOT=os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))), 'apps/qcv/i18n')
NEVER="QCView minNotes minColor QCBridge QCBridgeAE QCBridgeN Louper After\\ Effects Premiere\\ Pro Blender Sparkle OpenColorIO OCIO OpenDRT AgX LUT CDL EXR ProRes DNxHR DNxHD H.264 H.265 AAC SRT RTMP NDI AJA HDR HDR10 SDR EDR PQ HLG Rec.709 Rec.2020 Rec.2100 P3-D65 Display\\ P3 sRGB BT.2390 MaxCLL MaxFALL XMP Vulkan GPU fps kbps nits UHD 1080p RGB YUV Markdown HTML PDF DOCX PNG JPEG TIFF".replace('\\ ','\x00').split()
NEVER=[t.replace('\x00',' ') for t in NEVER]
def unq(s): return bytes(s,'utf-8').decode('unicode_escape').encode('latin-1').decode('utf-8')
STR=r'"((?:[^"\\]|\\.)*)"'
def field(block, name):
    m=re.search(r'^'+name+r' '+STR+r'((?:\n"(?:[^"\\]|\\.)*")*)', block, re.M)
    if not m: return None
    return unq(m.group(1))+''.join(unq(x) for x in re.findall(STR, m.group(2) or ''))
def load(code):
    out=[]
    for b in open(f'{ROOT}/qcview_{code}.po',encoding='utf-8').read().split('\n\n'):
        mid=field(b,'msgid')
        if mid is None or mid=='': continue
        strs=[field(b,'msgstr')] if 'msgstr[' not in b else [unq(x) for x in re.findall(r'^msgstr\[\d\] '+STR, b, re.M)]
        out.append(dict(ctx=field(b,'msgctxt') or '', id=mid, strs=[s for s in strs if s is not None], draft='drafted' in b))
    return out
REG={'de':r'\b(du|dein|deine|deinen|dir|dich)\b','fr':r"\b(tu|ton|ta|tes|toi)\b",'es':r'\b(tú|tus|tuyo|vos)\b','pt_BR':r'\b(tu|teu|tua)\b','ru':r'\b(ты|твой|твои|тебя|тебе)\b','uk':r'\b(ти|твій|твої|тебе|тобі)\b'}
SENT_END='.。!?！？'
findings=collections.defaultdict(list)
langs=['de','es','fr','it','ja','ko','pl','pt_BR','ru','uk','zh_CN']
for code in langs:
    ents=load(code); carried=collections.defaultdict(set)
    for e in ents:
        if not e['draft']:
            for s in e['strs']: carried[e['id']].add(s)
    drafts=[e for e in ents if e['draft']]
    byid=collections.defaultdict(set)
    for e in drafts:
        for s in e['strs']: byid[e['id']].add(s)
    for e in drafts:
        en=e['id']
        for tr in e['strs']:
            f=lambda kind,why: findings[code].append((kind, e['ctx'], en, tr, why))
            if tr=='' : f('EMPTY',''); continue
            for t in NEVER:
                if re.search(r'(?<![A-Za-z0-9])'+re.escape(t)+r'(?![A-Za-z0-9])', en) and t not in tr: f('TERM', t)
            if en.rstrip().endswith('…') != (tr.rstrip().endswith('…') or tr.rstrip().endswith('...')): f('ELLIPSIS','')
            en_dot=en.rstrip().endswith('.') and not en.rstrip().endswith('…'); tr_dot=tr.rstrip()[-1:] in SENT_END
            if en_dot != tr_dot and not en.rstrip().endswith(('…',':')) and len(en)>3: f('PERIOD', 'en ends with . : %s' % en_dot)
            m=re.search(r' \(([^()]*)\)$', en)
            if m and re.search(r'Ctrl|Shift|Alt|⌘|⌥|⇧|⌃|Space|Esc|\b[A-Z0-9]\b|←|→|Home|End', m.group(1)) and ('('+m.group(1)+')') not in tr and ('（'+m.group(1)+'）') not in tr: f('KEYHINT', m.group(1))
            if (en[:1]==' ')!=(tr[:1]==' ') or (en[-1:]==' ')!=(tr[-1:]==' '): f('SPACE','')
            if '  ' in tr: f('DOUBLESPACE','')
            if code in REG and re.search(REG[code], tr, re.I if code not in ('de',) else 0): f('REGISTER', re.search(REG[code], tr, re.I).group(0))
            if tr==en and len(re.findall(r'[a-z]{4,}', en))>=2 and not all(w in ' '.join(NEVER) for w in en.split()): f('SAME','')
            if code not in ('ja','ko','zh_CN') and en[:1].isalpha() and tr[:1].isalpha() and en[:1].islower()!=tr[:1].islower() and not (code=='de'): f('CASE','')
            if code not in ('ja','ko','zh_CN') and len(en)<=22 and len(tr)>max(len(en)*2.2, len(en)+14): f('LONG', f'{len(en)}→{len(tr)}')
            amp_en=len(re.findall(r'(?<!&)&(?!&)', en)); amp_tr=len(re.findall(r'(?<!&)&(?!&)', tr))
            if amp_en!=amp_tr and amp_tr>1: f('AMP','')
        if len(byid[en])>1 and len(e['strs'])==1: findings[code].append(('VARIANTS', e['ctx'], en, ' | '.join(sorted(byid[en])), 'the same English drafted differently'))
        if en in carried and e['strs'] and e['strs'][0] not in carried[en] and len(e['strs'])==1: findings[code].append(('VS-CARRIED', e['ctx'], en, e['strs'][0], 'QCView has: '+' | '.join(sorted(carried[en]))))
    print(code, 'drafts', len(drafts), 'findings', collections.Counter(k for k,*_ in findings[code]).most_common())
for code in langs:
    for kind,ctx,en,tr,why in findings[code]: print(f"  {code:6} {kind:11} [{ctx}] {en[:60]!r} -> {tr[:70]!r}  {why[:80]}")
if len(sys.argv) > 1: json.dump({k:v for k,v in findings.items()}, open(sys.argv[1],'w',encoding='utf-8'), ensure_ascii=False, indent=0)
