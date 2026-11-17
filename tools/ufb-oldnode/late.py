import subprocess, time, os, sys, threading, shutil, re
S=os.environ['S']; R=S+'/old-run'
for d in ['home','tmp','farm','store','newset']: shutil.rmtree(R+'/'+d, ignore_errors=True)
for d in ['home','tmp','farm','store/JobA/shots/sh010','store/JobA/shots/sh020','newset']: os.makedirs(R+'/'+d)
job=R+'/store/JobA'
demo=';'.join(['wait:1500',f'subscribe:{job}','job:0','wait:500','section:shots','wait:500','col:text:Status','wait:300','cell:0:0:before-old-came','star','wait:30000','mesh','meta'])
nenv=dict(os.environ, UFB_SETTINGS_DIR=R+'/newset', UFB_MESH_PORTS='49421,49422,4465', UFB_MESH_LOG='1', UFB_SIZE='1200x760', UFB_DEMO=demo, UFB_SHOT=R+'/new.png', UFB_MESH_SNAPSHOT_SECS='5')
new=subprocess.Popen([os.environ['UFB_BIN'], R+'/store', '--jobs', R+'/store', '--map', 'C:\\Volumes\\store='+R+'/store', '--mesh', R+'/farm', '--mesh-node', 'new', '--mesh-only', job], stdout=subprocess.DEVNULL, stderr=open(R+'/new.err','w'), env=nenv)
time.sleep(14)   # the new node, alone, has written its snapshot by now
print('-- snapshot before the old node starts:', os.path.exists(R+'/farm/v9006/snapshots/ufb_snapshot_v9006.db'), open(R+'/farm/v9006/snapshots/ufb_snapshot_v9006.meta.json').read()[:120] if os.path.exists(R+'/farm/v9006/snapshots/ufb_snapshot_v9006.meta.json') else '')
env=dict(os.environ, HOME=R+'/home', TMPDIR=R+'/tmp', RUST_LOG='info')
old=subprocess.Popen([S+'/old-target/release/oldnode', R, R+'/farm', 'old', '49521', '49522', ''], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, env=env, text=True, bufsize=1)
out=[]
threading.Thread(target=lambda: [out.append(l.rstrip()) for l in old.stdout], daemon=True).start()
def cmd(c, wait=0.6):
    n=len(out); old.stdin.write(c+'\n'); old.stdin.flush(); time.sleep(wait); return [l for l in out[n:] if not l.startswith('[')]
for t in (4, 10, 16):
    time.sleep(6 if t>4 else 4); print('-- old,', t, 's after its start'); [print('   ', l[:400]) for l in cmd('status')+cmd('rows')+cmd('cols')]
try: new.wait(timeout=25)
except Exception: new.kill()
cmd('quit', 2.0); old.kill()
print('-- old node log: snapshot / restore lines'); [print('   ', l[:260]) for l in out if re.search(r'(?i)snapshot|restore', l)][:14]
print('-- new node says'); [print('   ', l.rstrip()[:260]) for l in open(R+'/new.err') if re.search(r'UFB_DEMO: (mesh|columns)|snapshot|panick', l)]
