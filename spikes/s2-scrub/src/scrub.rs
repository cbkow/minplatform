//! S2 — the scrub decoder (QCView's `ScrubDecoder` shape, canon §17.4 / §4.2): its own FFmpeg context + hwaccel on a worker
//! thread, LATEST-WINS target (a newer request abandons a forward fill mid-way), an LRU of decoded frames under a byte budget,
//! and the 3-way decision: cache hit / forward-no-reseek within 90 frames (inter codecs) / keyframe seek + decode forward.
//! Intra codecs (ProRes, EXR) always seek-exact: one decode per request.
use crate::decode::{self, Frame};
use std::{collections::VecDeque, sync::{Arc, Condvar, Mutex}, time::Instant};

#[derive(Clone, Copy, Debug)] pub struct Request { pub id: u64, pub frame: i64, pub t_issued: Instant }
pub struct Result_ { pub req: Request, pub t_done: Instant, pub decoded: u32, pub seeked: bool, pub hit: bool, pub abandoned_before: u32, pub frame: Arc<Frame> }

struct Shared { pending: Mutex<Option<Request>>, cv: Condvar, quit: Mutex<bool> }
pub struct ScrubDecoder { shared: Arc<Shared>, pub rx: crossbeam_channel::Receiver<Result_>, next_id: u64, pub fps: f64, pub frames: i64, pub intra: bool }

pub const FORWARD_REACH: i64 = 90;

impl ScrubDecoder {
    pub fn open(path: &str, hw: bool, budget_bytes: usize) -> Result<ScrubDecoder, String> {
        // intra codecs: SLICE threads (one frame in, one out — a flush costs nothing); inter codecs: FRAME threads (a GOP fill
        // needs throughput; the pipeline-fill latency after a keyframe seek is paid once per fill, not per frame)
        let intra = decode::Decoder::open_opts(path, 0, false, true)?.is_intra();
        let mut dec = decode::Decoder::open_opts(path, 0, hw, intra)?;
        dec.loop_at_eof = false;
        let (fps, frames) = (dec.fps, (dec.duration * dec.fps).round() as i64);
        let per_frame = (dec.width as usize * dec.height as usize) * if dec.bytes_per_sample_hint() == 2 { 4 } else { 2 };
        let shared = Arc::new(Shared { pending: Mutex::new(None), cv: Condvar::new(), quit: Mutex::new(false) });
        let (tx, rx) = crossbeam_channel::unbounded::<Result_>();
        let sh = shared.clone();
        std::thread::Builder::new().name("scrub".into()).spawn(move || {
            let mut cache: VecDeque<(i64, Arc<Frame>)> = VecDeque::new();   // LRU: back = most recent
            let cap = (budget_bytes / per_frame.max(1)).max(2);
            let mut pos: Option<i64> = None;   // the decoder's position = last decoded frame number
            let mut abandoned = 0u32;
            loop {
                let req = {
                    let mut p = sh.pending.lock().unwrap();
                    while p.is_none() { if *sh.quit.lock().unwrap() { return } p = sh.cv.wait(p).unwrap(); }
                    p.take().unwrap()
                };
                // 1. hit
                if let Some(i) = cache.iter().position(|(f, _)| *f == req.frame) {
                    let e = cache.remove(i).unwrap(); let fr = e.1.clone(); cache.push_back(e);
                    tx.send(Result_ { req, t_done: Instant::now(), decoded: 0, seeked: false, hit: true, abandoned_before: std::mem::take(&mut abandoned), frame: fr }).ok();
                    continue;
                }
                // 2. forward-no-reseek (inter codecs only) / 3. keyframe seek
                let forward = !intra && pos.map(|p| req.frame > p && req.frame - p <= FORWARD_REACH).unwrap_or(false);
                let mut decoded = 0u32; let mut seeked = false;
                if !forward { dec.seek((req.frame as f64 + 0.01) / fps + dec.start); seeked = true; pos = None; }
                let mut req = req;
                let mut got: Option<Frame> = None; let mut bail = false;
                loop {
                    let Some(f) = dec.next() else { break };
                    decoded += 1; let n = dec.frame_no(f.pts); pos = Some(n);
                    if n >= req.frame { got = Some(f); break; }
                    // decode-once-per-GOP: every frame of a fill goes into the LRU (inter codecs), so a backward sweep inside the GOP hits
                    if !intra { cache.push_back((n, Arc::new(f))); while cache.len() > cap { cache.pop_front(); } }
                    // a newer target arrived mid-fill (latest wins — without starving): intra → abandon, a re-seek is one decode;
                    // inter → RETARGET when the new frame is ahead within reach, else finish this fill and serve the newer one next
                    let newer = sh.pending.lock().unwrap().as_ref().copied();
                    if let Some(nr) = newer {
                        if intra { bail = true; break; }
                        if nr.frame >= n && nr.frame - n <= FORWARD_REACH { sh.pending.lock().unwrap().take(); req = nr; }
                    }
                }
                if bail || got.is_none() { abandoned += 1; continue; }
                let fr = Arc::new(got.unwrap());
                cache.push_back((req.frame, fr.clone())); while cache.len() > cap { cache.pop_front(); }
                tx.send(Result_ { req, t_done: Instant::now(), decoded, seeked, hit: false, abandoned_before: std::mem::take(&mut abandoned), frame: fr }).ok();
            }
        }).unwrap();
        Ok(ScrubDecoder { shared, rx, next_id: 0, fps, frames, intra })
    }
    /// Latest wins: overwrites any pending target. Returns the request id.
    pub fn request(&mut self, frame: i64) -> u64 {
        self.next_id += 1;
        *self.shared.pending.lock().unwrap() = Some(Request { id: self.next_id, frame: frame.clamp(0, (self.frames - 1).max(0)), t_issued: Instant::now() });
        self.shared.cv.notify_one();
        self.next_id
    }
}
impl Drop for ScrubDecoder { fn drop(&mut self) { *self.shared.quit.lock().unwrap() = true; self.shared.cv.notify_all(); } }
