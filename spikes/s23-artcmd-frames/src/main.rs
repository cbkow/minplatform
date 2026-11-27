//! SPIKE S23 - dies after its verdict (docs/spikes/S23-artcmd-frames.md).
//! A clip in a camera's own format is played as an image sequence that is NOT on disk: a renderer the user installed
//! (a command-line program that writes EXR frames for `--start N --duration M`) is given named pipes for its output
//! files, and this harness reads each frame's bytes from its pipe, decodes it with the family's EXR reader, and
//! "plays" at the clip's rate the way `qcv-player`'s `ImageSequenceCache` asks: the playhead's frame first, then
//! read-ahead in batches. No window: the answer is whether frames are READY when the playhead reaches them.
//!   s23-artcmd-frames --tool <renderer> --clip <file> --frames <n> [--fps 24] [--width 1920] [--seconds 20]
//!                     [--ahead 72] [--batch 36] [--workers 1] [--decode spill|fifo|none] [--seeks 8] [--platform metal]
//! Nothing here names a clip or a tool: both come from the command line.
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed}, Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone)]
struct Cfg { tool: String, clip: String, frames: i64, fps: f64, width: u32, ahead: i64, batch: i64, workers: usize, decode: String, platform: String }
static SEQ: AtomicU64 = AtomicU64::new(0);
static LAUNCHES: AtomicUsize = AtomicUsize::new(0);

/// One call of the renderer for `n` frames from `start`: a folder of pipes named by the counter, read in order.
/// `sink(frame, bytes, decoded size, when)` is called as each frame is in memory.
fn fetch(c: &Cfg, start: i64, n: i64, mut sink: impl FnMut(i64, usize, (u32, u32))) -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("s23-{}-{}", std::process::id(), SEQ.fetch_add(1, Relaxed)));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let pipe = |i: i64| dir.join(format!("{i:07}.exr"));
    for i in 0..n { let p = std::ffi::CString::new(pipe(i).to_string_lossy().as_bytes()).unwrap(); if unsafe { libc::mkfifo(p.as_ptr(), 0o600) } != 0 { return Err("mkfifo failed".into()) } }
    LAUNCHES.fetch_add(1, Relaxed);
    let mut child = Command::new(&c.tool).args(["process", "--input", &c.clip, "--render-platform", &c.platform, "--start", &start.to_string(), "--duration", &n.to_string(),
        "--output-width", &c.width.to_string(), "--output", &dir.join("%07d.exr").to_string_lossy(), "--logpath", ""]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|e| format!("the renderer did not start: {e}"))?;
    let mut res = Ok(());
    for i in 0..n {
        let p = pipe(i);
        let size = match c.decode.as_str() {
            // the family's reader on the pipe itself (it may need to seek: then this fails and says so)
            "fifo" => min_image::load(&p.to_string_lossy(), "", 0).map(|im| (0usize, (im.width, im.height))).map_err(|e| format!("decode from the pipe: {e}")),
            _ => { let mut b = Vec::with_capacity(16 << 20); match std::fs::File::open(&p).and_then(|mut f| f.read_to_end(&mut b)) { Err(e) => Err(e.to_string()), Ok(_) if b.is_empty() => Err("an empty frame".into()),
                Ok(_) => if c.decode == "none" { Ok((b.len(), (0, 0))) } else {
                    // the reader takes a path: the bytes are put in a file of their own for it and the file is taken away again
                    let t = dir.join(format!("spill-{i}.exr")); std::fs::write(&t, &b).map_err(|e| e.to_string()).and_then(|_| min_image::load(&t.to_string_lossy(), "", 0).map_err(|e| e.to_string())).map(|im| { let _ = std::fs::remove_file(&t); (b.len(), (im.width, im.height)) }) } } }
        };
        match size { Ok((bytes, wh)) => sink(start + i, bytes, wh), Err(e) => { res = Err(format!("frame {}: {e}", start + i)); break } }
    }
    if res.is_err() { let _ = child.kill(); }
    let st = child.wait().map_err(|e| e.to_string())?; let _ = std::fs::remove_dir_all(&dir);
    if res.is_ok() && !st.success() { res = Err(format!("the renderer ended with {st}")); }
    res
}

/// As `fetch`, but only the bytes: `sink(frame, bytes)` as each frame has left its pipe.
fn fetch_bytes(c: &Cfg, start: i64, n: i64, mut sink: impl FnMut(i64, Vec<u8>)) -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!("s23-{}-{}", std::process::id(), SEQ.fetch_add(1, Relaxed)));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let pipe = |i: i64| dir.join(format!("{i:07}.exr"));
    for i in 0..n { let p = std::ffi::CString::new(pipe(i).to_string_lossy().as_bytes()).unwrap(); if unsafe { libc::mkfifo(p.as_ptr(), 0o600) } != 0 { return Err("mkfifo failed".into()) } }
    LAUNCHES.fetch_add(1, Relaxed);
    let mut child = Command::new(&c.tool).args(["process", "--input", &c.clip, "--render-platform", &c.platform, "--start", &start.to_string(), "--duration", &n.to_string(),
        "--output-width", &c.width.to_string(), "--output", &dir.join("%07d.exr").to_string_lossy(), "--logpath", ""]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|e| format!("the renderer did not start: {e}"))?;
    let mut res = Ok(());
    for i in 0..n { let mut b = Vec::with_capacity(16 << 20); match std::fs::File::open(pipe(i)).and_then(|mut f| f.read_to_end(&mut b)) { Ok(_) if !b.is_empty() => sink(start + i, b), Ok(_) => { res = Err(format!("frame {}: empty", start + i)); break } Err(e) => { res = Err(format!("frame {}: {e}", start + i)); break } } }
    if res.is_err() { let _ = child.kill(); }
    let st = child.wait().map_err(|e| e.to_string())?; let _ = std::fs::remove_dir_all(&dir);
    if res.is_ok() && !st.success() { res = Err(format!("the renderer ended with {st}")); }
    res
}

struct Shared { ready: Mutex<BTreeMap<i64, Instant>>, cv: Condvar, playhead: Mutex<i64>, next: Mutex<i64>, stop: Mutex<bool>, bytes: AtomicU64, err: Mutex<Option<String>> }

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect(); let val = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1).cloned());
    let (Some(tool), Some(clip), Some(frames)) = (val("--tool"), val("--clip"), val("--frames").and_then(|v| v.parse::<i64>().ok())) else { eprintln!("s23-artcmd-frames --tool <renderer> --clip <file> --frames <n> [--fps 24] [--width 1920] [--seconds 20] [--ahead 72] [--batch 36] [--workers 1] [--decode spill|fifo|none] [--seeks 8] [--platform metal]"); std::process::exit(2) };
    let num = |k: &str, d: f64| val(k).and_then(|v| v.parse::<f64>().ok()).unwrap_or(d);
    let c = Cfg { tool, clip, frames, fps: num("--fps", 24.0), width: num("--width", 1920.0) as u32, ahead: num("--ahead", 72.0) as i64, batch: num("--batch", 36.0) as i64, workers: num("--workers", 1.0) as usize, decode: val("--decode").unwrap_or_else(|| "spill".into()), platform: val("--platform").unwrap_or_else(|| "metal".into()) };
    let (seconds, seeks) = (num("--seconds", 20.0), num("--seeks", 8.0) as i64);
    let decode_threads = num("--decoders", 8.0) as usize;
    if !Path::new(&c.tool).is_file() || !Path::new(&c.clip).is_file() { eprintln!("the renderer or the clip is not there"); std::process::exit(2) }
    println!("S23: {} frames at {} fps, {} px wide, ahead {}, batch {}, workers {}, decode {}, platform {}", c.frames, c.fps, c.width, c.ahead, c.batch, c.workers, c.decode, c.platform);

    // ---- 1. the first frame, cold: one call for one frame (the clip is opened, the GPU is set up)
    let t = Instant::now(); let mut first = (0usize, (0u32, 0u32));
    if let Err(e) = fetch(&c, 0, 1, |_, b, wh| first = (b, wh)) { eprintln!("FAIL first frame: {e}"); std::process::exit(1) }
    println!("first frame: {:.0} ms, {:.1} MB, decoded {}x{}", t.elapsed().as_secs_f64() * 1e3, first.0 as f64 / 1e6, first.1 .0, first.1 .1);

    // ---- 2. playback: the playhead runs at the clip's rate (the clip is taken round and round); workers keep the window filled
    let sh = Arc::new(Shared { ready: Mutex::new(BTreeMap::new()), cv: Condvar::new(), playhead: Mutex::new(0), next: Mutex::new(0), stop: Mutex::new(false), bytes: AtomicU64::new(0), err: Mutex::new(None) });
    LAUNCHES.store(0, Relaxed);
    // the decoders: each frame's bytes go to a file of their own for the reader (it takes a path), are decoded, and the file is taken away
    let (tx, rx) = std::sync::mpsc::channel::<(i64, Vec<u8>)>(); let (tx, rx) = (Arc::new(Mutex::new(tx)), Arc::new(Mutex::new(rx)));
    let decoders: Vec<_> = (0..decode_threads).map(|d| { let (sh, rx, mode) = (sh.clone(), rx.clone(), c.decode.clone()); std::thread::spawn(move || loop {
        let got = rx.lock().unwrap().recv(); let Ok((abs, bytes)) = got else { break };
        if mode != "none" { let t = std::env::temp_dir().join(format!("s23-spill-{}-{d}-{abs}.exr", std::process::id()));
            let r = std::fs::write(&t, &bytes).map_err(|e| e.to_string()).and_then(|_| min_image::load(&t.to_string_lossy(), "", 0).map_err(|e| e.to_string())); let _ = std::fs::remove_file(&t);
            if let Err(e) = r { *sh.err.lock().unwrap() = Some(format!("frame {abs}: {e}")); break } }
        sh.ready.lock().unwrap().insert(abs, Instant::now()); sh.cv.notify_all(); }) }).collect();
    let hs: Vec<_> = (0..c.workers).map(|_| { let (sh, c, tx) = (sh.clone(), c.clone(), tx.clone()); std::thread::spawn(move || loop {
        if *sh.stop.lock().unwrap() { break }
        // the next range: from the first frame not yet asked for, never further than the window ahead of the playhead; a range ends at the clip's end
        let (start, n) = { let ph = *sh.playhead.lock().unwrap(); let mut nx = sh.next.lock().unwrap(); if *nx < ph { *nx = ph } let room = ph + c.ahead - *nx;
            if room <= 0 { drop(nx); std::thread::sleep(Duration::from_millis(5)); continue }
            let s = *nx; let n = c.batch.min(room).min(c.frames - s.rem_euclid(c.frames)); *nx += n; (s, n) };
        let base = start.rem_euclid(c.frames);
        if let Err(e) = fetch_bytes(&c, base, n, |f, bytes| { sh.bytes.fetch_add(bytes.len() as u64, Relaxed); let abs = start + (f - base); let _ = tx.lock().unwrap().send((abs, bytes)); }) { *sh.err.lock().unwrap() = Some(e); break }
    }) }).collect();
    // wait for the first frame of the run, then play
    { let mut r = sh.ready.lock().unwrap(); while !r.contains_key(&0) { if sh.err.lock().unwrap().is_some() { break } r = sh.cv.wait_timeout(r, Duration::from_millis(50)).unwrap().0; } }
    let t0 = Instant::now(); let total = (seconds * c.fps) as i64; let (mut shown, mut late, mut worst_late, mut min_lead) = (0i64, 0i64, 0f64, f64::MAX);
    for f in 0..total {
        let due = t0 + Duration::from_secs_f64(f as f64 / c.fps); let now = Instant::now(); if due > now { std::thread::sleep(due - now) }
        *sh.playhead.lock().unwrap() = f;
        let mut r = sh.ready.lock().unwrap();
        match r.get(&f) { Some(at) if *at <= due + Duration::from_millis(1) => { shown += 1; let lead = r.range(f..).count() as f64; if lead < min_lead { min_lead = lead } }
            _ => { late += 1; let t = Instant::now(); while !r.contains_key(&f) && t.elapsed() < Duration::from_secs(5) && sh.err.lock().unwrap().is_none() { r = sh.cv.wait_timeout(r, Duration::from_millis(5)).unwrap().0; } let l = (Instant::now() - due).as_secs_f64() * 1e3; if l > worst_late { worst_late = l } } }
        let old: Vec<i64> = r.range(..f - 12).map(|x| *x.0).collect(); for k in old { r.remove(&k); }   // read-behind 12, as the cache keeps
        if sh.err.lock().unwrap().is_some() { break }
    }
    let played = t0.elapsed().as_secs_f64(); *sh.stop.lock().unwrap() = true; for h in hs { let _ = h.join(); } drop(tx); for d in decoders { let _ = d.join(); }
    if let Some(e) = sh.err.lock().unwrap().clone() { eprintln!("FAIL playback: {e}"); std::process::exit(1) }
    println!("playback: {total} frames asked in {played:.1} s: {shown} ready on time, {late} late (worst {worst_late:.0} ms); fewest frames in hand {min_lead:.0}; {} renderer starts; {:.0} MB/s through the pipes", LAUNCHES.load(Relaxed), sh.bytes.load(Relaxed) as f64 / 1e6 / played);

    // ---- 3. seeks: a jump to a frame far away, nothing in hand: one call for that frame alone, then (not timed) the window
    let mut ms: Vec<f64> = Vec::new();
    for k in 0..seeks { let target = (c.frames * (2 * k + 1) / (2 * seeks)).min(c.frames - 1); let t = Instant::now(); if let Err(e) = fetch(&c, target, 1, |_, _, _| {}) { eprintln!("FAIL seek to {target}: {e}"); std::process::exit(1) } ms.push(t.elapsed().as_secs_f64() * 1e3); }
    ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if !ms.is_empty() { println!("seek to a far frame ({} tries): median {:.0} ms, worst {:.0} ms", ms.len(), ms[ms.len() / 2], ms[ms.len() - 1]); }
    let _: PathBuf = std::env::temp_dir();
}
