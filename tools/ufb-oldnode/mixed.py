import subprocess, time, os, sys, threading, shutil, re, json
S=os.environ['S']; R=S+'/old-run'; lead_old = len(sys.argv)>1 and sys.argv[1]=='oldleads'
for d in ['home','tmp','farm','store','newset']: shutil.rmtree(R+'/'+d, ignore_errors=True)
for d in ['home','tmp','farm','store/JobA/shots/sh010','store/JobA/shots/sh020','newset']: os.makedirs(R+'/'+d)
env=dict(os.environ, HOME=R+'/home', TMPDIR=R+'/tmp', RUST_LOG='warn')
old=subprocess.Popen([S+'/old-target/release/oldnode', R, R+'/farm', 'old', '49521', '49522', 'leader' if lead_old else ''], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, env=env, text=True, bufsize=1)
out=[]
def rd():
    for l in old.stdout: out.append(l.rstrip()); 
threading.Thread(target=rd, daemon=True).start()
def cmd(c, wait=0.6):
    n=len(out); old.stdin.write(c+'\n'); old.stdin.flush(); time.sleep(wait); return out[n:]
def show(tag, lines): print('--', tag); [print('   ', l[:420]) for l in lines]
time.sleep(4); show('old alone', cmd('status'))
job=R+'/store/JobA'
demo=';'.join(['wait:7000','mesh',f'subscribe:{job}','job:0','wait:500','section:shots','wait:500','col:text:Status','col:text:Notes','wait:300','cell:0:0:from-new','wait:9000','mesh','meta','cell:0:1:kept-by-new','wait:9000','mesh','meta','wait:9000','meta','mesh'] + (['wait:25000','mesh'] if lead_old else []))
nenv=dict(os.environ, UFB_SETTINGS_DIR=R+'/newset', UFB_MESH_PORTS='49421,49422,4465', UFB_MESH_LOG='1', UFB_SIZE='1200x760', UFB_DEMO=demo, UFB_SHOT=R+'/new.png', UFB_MESH_SNAPSHOT_SECS='5')
new=subprocess.Popen([os.environ['UFB_BIN'], R+'/store', '--jobs', R+'/store', '--map', 'C:\\Volumes\\store='+R+'/store', '--mesh', R+'/farm', '--mesh-node', 'new', '--mesh-only', job], stdout=subprocess.DEVNULL, stderr=open(R+'/new.err','w'), env=nenv)
t0=time.time()
time.sleep(13); show('old after the new node joined and edited (t~13)', cmd('status')+cmd('rows')+cmd('cols'))
rows=[l for l in out if l.startswith('ROWS')][-1].replace('\\"','"')
m=re.search(r'(vol:[0-9a-f-]+/JobA/shots/sh010) (\{.*?\}) tracked', rows)
if m:
    ident, blob = m.group(1), json.loads(m.group(2)); jobid = ident.rsplit('/shots/',1)[0]
    key=[k for k in blob if blob[k]=='from-new']; print('   identity', ident.replace(R,'<R>'), 'keys', list(blob.keys()))
    # the old node edits the SAME item: its whole row, with the Status cell changed (what 1.4.2 sends)
    if key: blob[key[0]]='from-old'
    time.sleep(1.2); show('old edits sh010.Status', cmd(f'set {jobid}|{ident}|shots|0|{json.dumps(blob)}', 1.0))
    # ... and a second item the new node never touched
    show('old edits sh020', cmd(f'set {jobid}|{jobid}/shots/sh020|shots|1|{json.dumps({key[0] if key else "x": "old-only"})}', 1.0))
else: print('   NO ROW ARRIVED at the old node')
time.sleep(max(0, 24-(time.time()-t0))); show('old at t~24 (after the new node edited Notes)', cmd('rows')+cmd('status'))
time.sleep(max(0, 36-(time.time()-t0))); show('old at t~36', cmd('rows')+cmd('status'))
if lead_old: time.sleep(max(0, 62-(time.time()-t0))); show('old at t~62', cmd('status'))
try: new.wait(timeout=25)
except Exception: new.kill(); print('   (new node ended by the watchdog)')
show('old, quitting', cmd('quit', 2.0)); old.kill()
print('-- new node says'); 
for l in open(R+'/new.err'):
    if re.search(r'UFB_DEMO: (mesh|columns|the cell)|UFB_MESH|panick|refused|unauthor', l): print('   ', l.rstrip().replace(R,'<R>')[:330])
print('-- snapshot author', open(R+'/farm/v9006/snapshots/ufb_snapshot_v9006.meta.json').read()[:200] if os.path.exists(R+'/farm/v9006/snapshots/ufb_snapshot_v9006.meta.json') else 'none')
print('-- farm'); 
for root,ds,fs in os.walk(R+'/farm'):
    for f in fs: print('   ', os.path.join(root,f).replace(R,'<R>'))
print('-- outside check: real app-data untouched by this run is by construction (HOME redirected); old db at', [l for l in out if l.startswith('HARNESS db')][0].replace(R,'<R>'))
show('old node log (warnings)', [l for l in out if 'WARN' in l or 'ERROR' in l][:12])
