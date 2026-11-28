//! SPIKE S24 (dies after its verdict; `docs/spikes/S24-remote-sequence.md`): a frame-addressed source in ANOTHER process.
//! The reader asks "frame N into slot S" over a pipe; the producer puts RGBA 16-bit float pixels into slot S of a shared
//! memory block and answers over a pipe. What is measured is the hand-over, not the making of a frame: the producer's
//! own time to make a frame is a number given on the command line (a wait), reported back and subtracted.
//!
//!     cargo run --release -p s24-remote-sequence            (the reader; it starts the producer itself)
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const BPP: usize = 8;                       // RGBA, four halves
const HEADER: usize = 4096;                 // one page: the generation lives at its start
const REQ: usize = 32;                      // generation, frame, slot, producer mode, make time (ns)
const DONE: usize = 32;                     // generation, frame, slot, make time as it turned out (ns)

struct Shm { ptr: *mut u8, len: usize, name: std::ffi::CString, owner: bool }
unsafe impl Send for Shm {}
unsafe impl Sync for Shm {}
impl Shm {
    fn open(name: &str, len: usize, create: bool) -> Shm {
        let c = std::ffi::CString::new(name).unwrap();
        unsafe {
            let flags = if create { libc::O_CREAT | libc::O_EXCL | libc::O_RDWR } else { libc::O_RDWR };
            let fd = libc::shm_open(c.as_ptr(), flags, 0o600);
            assert!(fd >= 0, "shm_open: {}", std::io::Error::last_os_error());
            if create { assert_eq!(libc::ftruncate(fd, len as libc::off_t), 0, "ftruncate: {}", std::io::Error::last_os_error()); }
            let p = libc::mmap(std::ptr::null_mut(), len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0);
            assert!(p != libc::MAP_FAILED, "mmap: {}", std::io::Error::last_os_error());
            libc::close(fd);
            Shm { ptr: p as *mut u8, len, name: c, owner: create }
        }
    }
    fn generation(&self) -> &AtomicU64 { unsafe { &*(self.ptr as *const AtomicU64) } }
    fn slot(&self, i: usize, stride: usize) -> *mut u8 { unsafe { self.ptr.add(HEADER + i * stride) } }
}
impl Drop for Shm { fn drop(&mut self) { unsafe { libc::munmap(self.ptr as *mut _, self.len); if self.owner { libc::shm_unlink(self.name.as_ptr()); } } } }

fn put(b: &mut [u8], at: usize, v: u64) { b[at..at + 8].copy_from_slice(&v.to_le_bytes()) }
fn get(b: &[u8], at: usize) -> u64 { u64::from_le_bytes(b[at..at + 8].try_into().unwrap()) }
/// The value every half of source picture `k` holds, so the reader can tell the pixels are the producer's.
fn fill(k: u64) -> u16 { 0x3000 + (k as u16 % 4) * 0x0101 }
/// A busy wait: a sleeping wait in the producer overslept by about 5 ms a frame, which then showed up as hand-over time.
fn wait_until(t: Instant) { while Instant::now() < t { std::hint::spin_loop() } }

// ---------------------------------------------------------------------------------------------------- the producer
fn producer(name: &str, slots: usize, stride: usize) {
    let shm = Shm::open(name, HEADER + slots * stride, false);
    // four pictures of its own, as a decoder's output buffers would be
    let own: Vec<Vec<u16>> = (0..4).map(|k| vec![fill(k); stride / 2]).collect();
    let (mut inp, mut out) = (std::io::stdin().lock(), std::io::stdout().lock());
    let mut m = [0u8; REQ];
    while inp.read_exact(&mut m).is_ok() {
        let (generation, frame, slot, mode, make) = (get(&m, 0), get(&m, 8), get(&m, 16) as u32 as usize, (get(&m, 16) >> 32) as u32, get(&m, 24));
        if generation != shm.generation().load(Ordering::Acquire) {                     // asked before a jump: not made, but ANSWERED, so the reader knows the slot is free
            let mut d = [0u8; DONE]; put(&mut d, 0, generation); put(&mut d, 8, frame); put(&mut d, 16, slot as u64); put(&mut d, 24, u64::MAX);
            if out.write_all(&d).and_then(|_| out.flush()).is_err() { break }
            continue
        }
        let t = Instant::now();
        if make > 0 { wait_until(t + Duration::from_nanos(make)) }                       // "making the frame"
        let made = t.elapsed().as_nanos() as u64;
        let dst = shm.slot(slot, stride);
        unsafe {
            if mode == 1 { std::ptr::copy_nonoverlapping(own[(frame % 4) as usize].as_ptr() as *const u8, dst, stride) }   // made elsewhere, copied in
            std::ptr::copy_nonoverlapping(frame.to_le_bytes().as_ptr(), dst, 8);                                           // mode 0: made in place
        }
        let mut d = [0u8; DONE]; put(&mut d, 0, generation); put(&mut d, 8, frame); put(&mut d, 16, slot as u64); put(&mut d, 24, made);
        if out.write_all(&d).and_then(|_| out.flush()).is_err() { break }
    }
}

// ------------------------------------------------------------------------------------------------------ the reader
struct Done { generation: u64, frame: u64, slot: usize, made: u64, at: Instant }
struct Link { shm: std::sync::Arc<Shm>, stride: usize, to: std::process::ChildStdin, from: mpsc::Receiver<Done>, child: std::process::Child }
impl Link {
    fn start(w: usize, h: usize, slots: usize) -> Link {
        let stride = (w * h * BPP).div_ceil(16384) * 16384;                     // a slot is whole pages
        let name = format!("/s24-{}-{}", std::process::id(), w);
        let shm = std::sync::Arc::new(Shm::open(&name, HEADER + slots * stride, true));
        let mut child = Command::new(std::env::current_exe().unwrap()).args(["producer", &name, &slots.to_string(), &stride.to_string()])
            .stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
        let (to, mut back) = (child.stdin.take().unwrap(), child.stdout.take().unwrap());
        let (tx, from) = mpsc::channel();
        std::thread::spawn(move || { let mut d = [0u8; DONE]; while back.read_exact(&mut d).is_ok() {
            let at = Instant::now();
            if tx.send(Done { generation: get(&d, 0), frame: get(&d, 8), slot: get(&d, 16) as usize, made: get(&d, 24), at }).is_err() { break }
        } });
        Link { shm, stride, to, from, child }
    }
    fn ask(&mut self, generation: u64, frame: u64, slot: usize, mode: u32, make: Duration) {
        let mut m = [0u8; REQ]; put(&mut m, 0, generation); put(&mut m, 8, frame); put(&mut m, 16, slot as u64 | (mode as u64) << 32); put(&mut m, 24, make.as_nanos() as u64);
        self.to.write_all(&m).unwrap(); self.to.flush().unwrap();
    }
    fn stop(mut self) { drop(self.to); let _ = self.child.wait(); }
}

fn pct(v: &mut [f64], p: f64) -> f64 { v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v[((v.len() - 1) as f64 * p).round() as usize] }

/// One frame at a time, nothing else going on: ask, wait, make the pixels usable, check them.
/// `use_as`: 0 = the slot itself is the frame, 1 = copied out as it is, 2 = taken apart into planes (G, B, R, A).
fn round_trips(w: usize, h: usize, mode: u32, use_as: u32, n: u64) -> (f64, f64, f64) {
    let slots = 8; let mut link = Link::start(w, h, slots); let stride = link.stride;
    let mut mine = vec![0u16; stride / 2];
    let mut ms = Vec::new();
    for f in 0..n + slots as u64 * 2 {
        let slot = (f % slots as u64) as usize;
        let t = Instant::now();
        link.ask(0, f, slot, mode, Duration::ZERO);
        let d = link.from.recv().unwrap();
        let src = unsafe { std::slice::from_raw_parts(link.shm.slot(d.slot, stride) as *const u16, stride / 2) };
        assert_eq!(unsafe { std::ptr::read(src.as_ptr() as *const u64) }, f, "the wrong frame came back");
        let probe = match use_as {
            0 => src[w * h * 2 + 5],
            1 => { mine.copy_from_slice(src); mine[w * h * 2 + 5] }
            _ => { let n = w * h; let (g, rest) = mine.split_at_mut(n); let (b, rest) = rest.split_at_mut(n); let (r, a) = rest.split_at_mut(n);
                   for (i, px) in src[..n * 4].chunks_exact(4).enumerate() { r[i] = px[0]; g[i] = px[1]; b[i] = px[2]; a[i] = px[3]; }
                   g[n / 2 + 1] }
        };
        let total = t.elapsed().as_secs_f64() * 1e3 - d.made as f64 / 1e6;
        if mode == 1 { assert_eq!(probe, fill(f % 4), "the pixels are not the producer's") }
        if f >= slots as u64 * 2 { ms.push(total) }                             // the first two rounds touch every page for the first time
    }
    link.stop();
    (pct(&mut ms, 0.5), pct(&mut ms, 0.99), pct(&mut ms, 1.0))
}

/// Playback as the sequence cache asks: the playhead's frame first, then ahead, never more than `PENDING` asked at once;
/// the block of slots IS the window. A jump every `jump_every` frames (0 = never): the generation moves, what was asked is forgotten.
fn playback(w: usize, h: usize, fps: f64, seconds: f64, make: Duration, jump_every: u64) -> (u64, u64, f64, Vec<f64>) {
    const AHEAD: u64 = 72; const BEHIND: u64 = 12; const PENDING: usize = 32;
    let slots = (AHEAD + BEHIND + 4) as usize; let mut link = Link::start(w, h, slots); let stride = link.stride;
    for s in 0..slots { link.ask(0, s as u64, s, 1, Duration::ZERO); } for _ in 0..slots { link.from.recv().unwrap(); }       // every page touched once
    let mut generation = 1u64; link.shm.generation().store(generation, Ordering::Release);
    let mut have: std::collections::HashMap<u64, usize> = Default::default();   // frame -> slot
    let mut asked: std::collections::HashMap<u64, usize> = Default::default();
    let mut free: Vec<usize> = (0..slots).collect();
    let (mut base, mut shown, mut late, mut worst_late) = (1000u64, 0u64, 0u64, 0f64);
    let total = (fps * seconds) as u64; let period = Duration::from_secs_f64(1.0 / fps);
    let mut jumps = Vec::new(); let mut jump_at: Option<Instant> = None;
    let mut clock: Option<Instant> = None; let mut played_since = 0u64;
    while shown < total {
        let playhead = base + played_since;
        // forget what fell out of the window, ask for what is missing
        have.retain(|f, s| { let keep = *f + BEHIND >= playhead && *f <= playhead + AHEAD; if !keep { free.push(*s) } keep });
        for f in playhead..=playhead + AHEAD { if asked.len() >= PENDING { break } if have.contains_key(&f) || asked.contains_key(&f) { continue }
            let Some(s) = free.pop() else { break }; asked.insert(f, s); link.ask(generation, f, s, 1, make); }
        // take what has arrived; wait no longer than the next frame's moment
        let due = clock.map(|c| c + period * played_since as u32);
        let got = match due { Some(d) => link.from.recv_timeout(d.saturating_duration_since(Instant::now())).ok(), None => link.from.recv().ok() };
        if let Some(d) = got {
            if d.generation == generation { asked.remove(&d.frame); have.insert(d.frame, d.slot);
                if let Some(j) = jump_at.take_if(|_| d.frame == playhead) { jumps.push(d.at.duration_since(j).as_secs_f64() * 1e3 - d.made as f64 / 1e6) } }
            else { free.push(d.slot) }                                           // made for a place we left
        }
        if clock.is_none() { if have.contains_key(&playhead) && jump_at.is_none() { clock = Some(Instant::now()) } else { continue } }
        let due = clock.unwrap() + period * played_since as u32;
        if Instant::now() < due { continue }
        // the frame's moment: is it there, and is it the right picture?
        match have.get(&playhead) {
            Some(&s) => { let p = link.shm.slot(s, stride); assert_eq!(unsafe { std::ptr::read(p as *const u64) }, playhead); }
            None => { late += 1; let t = Instant::now(); loop { let d = link.from.recv().unwrap(); if d.generation != generation { free.push(d.slot); continue } asked.remove(&d.frame); have.insert(d.frame, d.slot); if d.frame == playhead { break } }
                      let over = t.elapsed(); worst_late = worst_late.max(over.as_secs_f64() * 1e3); clock = clock.map(|c| c + over); }
        }
        shown += 1; played_since += 1;
        if jump_every > 0 && shown % jump_every == 0 && shown < total {         // a jump far away, in the middle of things
            generation += 1; link.shm.generation().store(generation, Ordering::Release); jump_at = Some(Instant::now());
            asked.clear();                                                       // their slots are NOT free yet: each comes back with the producer's answer (made or not)
            for (_, s) in have.drain() { free.push(s) }
            base += 5000; played_since = 0; clock = None;
        }
    }
    link.stop();
    (shown, late, worst_late, jumps)
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.get(1).map(String::as_str) == Some("producer") { return producer(&a[2], a[3].parse().unwrap(), a[4].parse().unwrap()) }
    if a.get(1).map(String::as_str) == Some("jumps") {                         // jumps <ms a frame takes>...: the jump test alone
        for make_ms in a[2..].iter().map(|m| m.parse::<u64>().unwrap()) {
            let (_, _, _, mut j) = playback(1920, 1080, 24.0, 60.0, Duration::from_millis(make_ms), 24);
            let n = j.len(); println!("jumps, a frame takes {make_ms} ms: {n} jumps, {:.2} / {:.2} / {:.2} ms", pct(&mut j, 0.5), pct(&mut j, 0.99), pct(&mut j, 1.0));
        }
        return
    }
    println!("S24: the hand-over only (the producer's own time is taken off). ms: median / 99th / worst");
    for (w, h) in [(1920usize, 1080usize), (4096, 2160)] {
        println!("{w} x {h}  ({:.1} MB a frame)", (w * h * BPP) as f64 / 1e6);
        for (mode, m) in [(0u32, "made in the slot    "), (1, "copied into the slot")] { for (u, un) in [(0u32, "used where it lies"), (1, "copied out        "), (2, "taken into planes ")] {
            let (p50, p99, max) = round_trips(w, h, mode, u, 300);
            println!("  {m} | {un} | {p50:7.3} / {p99:7.3} / {max:7.3}");
        } }
    }
    for make_ms in [20u64, 38] {
        let (shown, late, worst, _) = playback(1920, 1080, 24.0, 30.0, Duration::from_millis(make_ms), 0);
        println!("playback 1920 x 1080, 24 fps, 30 s, a frame takes the producer {make_ms} ms: {shown} shown, {late} late (worst wait {worst:.1} ms)");
    }
    let (shown, late, worst, _) = playback(4096, 2160, 24.0, 30.0, Duration::from_millis(20), 0);
    println!("playback 4096 x 2160, 24 fps, 30 s, a frame takes the producer 20 ms: {shown} shown, {late} late (worst wait {worst:.1} ms)");
    for make_ms in [0u64, 20] {
        let (_, _, _, mut j) = playback(1920, 1080, 24.0, 60.0, Duration::from_millis(make_ms), 24);
        let n = j.len(); println!("jumps 1920 x 1080 while playing, a frame takes {make_ms} ms: {n} jumps, first new frame after {:.2} / {:.2} / {:.2} ms on top of its own making", pct(&mut j, 0.5), pct(&mut j, 0.99), pct(&mut j, 1.0));
    }
}
