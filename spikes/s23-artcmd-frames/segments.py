#!/usr/bin/env python3
"""S23, later the same day: the same clip asked for as short ProRes SEGMENTS (files), not EXR frames.
A dummy playback: one renderer at a time writes segment after segment ahead of a playhead that runs at the clip's rate.
A segment counts as ready when the renderer has ended (the file is whole).
  segments.py --tool <renderer> --clip <file> --frames <n> --out <scratch dir> [--ffmpeg <dir with ffmpeg, ffprobe>]
Nothing here names a clip or a tool: both come from the command line."""
import argparse, os, shutil, subprocess, sys, threading, time
ap = argparse.ArgumentParser(); ap.add_argument("--tool", required=True); ap.add_argument("--clip", required=True); ap.add_argument("--frames", type=int, required=True)
ap.add_argument("--out", required=True); ap.add_argument("--fps", type=float, default=24.0); ap.add_argument("--ffmpeg", default=""); ap.add_argument("--seconds", type=float, default=30.0)
a = ap.parse_args(); os.makedirs(a.out, exist_ok=True)

def render(start, n, codec, width, name):
    out = os.path.join(a.out, name); cmd = [a.tool, "process", "--input", a.clip, "--render-platform", "metal", "--start", str(start), "--duration", str(n),
        "--video-codec", codec, "--target-colorspace", "AWG4/D65/LogC4", "--output", out, "--logpath", ""] + (["--output-width", str(width)] if width else [])
    t = time.time(); r = subprocess.run(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL); dt = time.time() - t
    return dt, (os.path.getsize(out) if r.returncode == 0 and os.path.exists(out) else -1), out

print("== A. one segment, by length (seconds to write it; x = times faster than it plays)")
for codec, width, label in [("prores422proxy", 1920, "Proxy 1920 wide"), ("prores422", 0, "422 full size"), ("prores4444", 0, "4444 full size")]:
    row = []
    for n in (1, 6, 12, 24, 48, 96):
        dt, size, out = render(0, n, codec, width, f"a_{codec}_{n}.mxf")
        row.append(f"{n}f {dt:.2f}s x{(n / a.fps) / dt:.1f} {size / 1e6:.0f}MB" if size > 0 else f"{n}f FAILED")
    print(f"  {label:16s} " + " | ".join(row))

def playback(codec, width, label, first, seg, ahead):
    """Seek to 0: a short first segment, then segments of `seg` frames, never more than `ahead` frames beyond the playhead."""
    ready = {}; lock = threading.Lock(); state = {"playhead": 0, "stop": False, "starts": 0, "bytes": 0}
    def worker():
        nxt = 0
        while not state["stop"]:
            if nxt - state["playhead"] >= ahead: time.sleep(0.005); continue
            n = first if nxt == 0 else seg; base = nxt % a.frames; n = min(n, a.frames - base)
            dt, size, out = render(base, n, codec, width, f"p_{nxt}.mxf"); state["starts"] += 1; state["bytes"] += max(size, 0)
            now = time.time()
            with lock:
                for f in range(nxt, nxt + n): ready[f] = now
            nxt += n
            try: os.remove(out)
            except OSError: pass
    t_seek = time.time(); th = threading.Thread(target=worker); th.start()
    while 0 not in ready: time.sleep(0.001)
    first_ms = (time.time() - t_seek) * 1e3; t0 = time.time(); total = int(a.seconds * a.fps); late = 0; worst = 0.0
    for f in range(total):
        due = t0 + f / a.fps; d = due - time.time()
        if d > 0: time.sleep(d)
        state["playhead"] = f
        if ready.get(f, 1e18) > due + 0.001:
            late += 1
            while f not in ready: time.sleep(0.002)
            worst = max(worst, (time.time() - due) * 1e3)
    played = time.time() - t0; state["stop"] = True; th.join()
    print(f"  {label:16s} first {first}f, then {seg}f, {ahead}f ahead: first picture after {first_ms:.0f} ms; {total - late} of {total} frames ready on time, {late} late (worst {worst:.0f} ms); {state['starts']} renderer starts; {state['bytes'] / 1e6 / played:.0f} MB/s written")

print("== B. dummy playback at the clip's rate")
for first, seg, ahead in ((6, 24, 72), (6, 48, 96), (12, 96, 192)):
    playback("prores422proxy", 1920, "Proxy 1920 wide", first, seg, ahead)
playback("prores422", 0, "422 full size", 6, 48, 96)
playback("prores4444", 0, "4444 full size", 6, 48, 96)

print("== C. a jump to a far frame: the first short segment alone")
for codec, width, label in [("prores422proxy", 1920, "Proxy 1920 wide"), ("prores4444", 0, "4444 full size")]:
    for n in (1, 6):
        ms = sorted(render(a.frames * (2 * k + 1) // 12, n, codec, width, f"c_{k}.mxf")[0] * 1e3 for k in range(6))
        print(f"  {label:16s} {n} frame(s): median {ms[3]:.0f} ms, worst {ms[-1]:.0f} ms")

if a.ffmpeg:
    print("== D. the family's FFmpeg on a segment")
    dt, size, seg = render(48, 48, "prores422proxy", 1920, "d.mxf")
    p = subprocess.run([os.path.join(a.ffmpeg, "ffprobe"), "-v", "error", "-show_entries", "stream=codec_type,codec_name,width,height,nb_frames,duration,channels", "-of", "compact", seg], capture_output=True, text=True)
    print("  streams: " + " ; ".join(l for l in p.stdout.strip().splitlines())[:400]); print("  " + (p.stderr.strip().splitlines()[0][:200] if p.stderr.strip() else "no complaint from ffprobe"))
    t = time.time(); r = subprocess.run([os.path.join(a.ffmpeg, "ffmpeg"), "-v", "error", "-i", seg, "-f", "null", "-"], capture_output=True, text=True); print(f"  all 48 frames decoded in {time.time() - t:.2f} s (a program start included), exit {r.returncode} {r.stderr.strip()[:120]}")
    # a file still being written: is it readable before the renderer ends?
    out = os.path.join(a.out, "e.mxf"); pr = subprocess.Popen([a.tool, "process", "--input", a.clip, "--render-platform", "metal", "--start", "0", "--duration", str(a.frames), "--video-codec", "prores4444", "--target-colorspace", "AWG4/D65/LogC4", "--output", out, "--logpath", ""], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(1.5); mid = os.path.getsize(out) if os.path.exists(out) else 0
    q = subprocess.run([os.path.join(a.ffmpeg, "ffprobe"), "-v", "error", "-count_packets", "-show_entries", "stream=codec_name,nb_read_packets", "-of", "compact", out], capture_output=True, text=True)
    still = pr.poll() is None; pr.wait()
    print(f"  while it was being written ({mid / 1e6:.0f} MB so far, renderer still running: {still}): ffprobe says: {(q.stdout.strip() or q.stderr.strip()).splitlines()[0][:200] if (q.stdout.strip() or q.stderr.strip()) else 'nothing'}")
shutil.rmtree(a.out, ignore_errors=True)
