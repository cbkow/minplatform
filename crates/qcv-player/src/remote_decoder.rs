//! A remote sequence (canon row 624): a clip QCView cannot decode itself, pulled frame by frame from a PROVIDER -
//! another program, found through its description file (`min_framelink::described`) - into shared memory this
//! process owns. Behind the same calls the transport makes of a [`crate::sequence_decoder::SequenceDecoder`]:
//! frames by number, a window around a playhead that follows the wall clock, the last good picture held on a miss.
//!
//! The shared slots ARE the cache. A few frames are in flight at a time and the next most wanted is chosen each time
//! one comes back, so a jump waits for at most those few and nothing has to be cancelled. One provider process
//! serves every clip of its kind; a provider that ends takes its clips to `Errored`, and the next open starts it anew.
use crate::{ffmpeg::SessionInfo, frame_handle::FrameHandle, frame_index::FrameIndex, video_decoder::State, Error};
use min_framelink::described::Described;
use min_framelink::host::Provider;
use min_framelink::msg::*;
use min_present::{Chroma, ColorMeta, Frame, Matrix, Pixels, Plane, PlanarLayout, Primaries, Range, Transfer};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering::*};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[cfg(unix)]
use min_framelink::shm::Block;
/// Where shared blocks are not written yet (Windows) nothing can be opened; the type keeps the rest compiling.
#[cfg(not(unix))]
pub struct Block;
#[cfg(not(unix))]
impl Block {
    fn create(_: &str, _: u32, _: usize) -> std::io::Result<Block> { Err(std::io::Error::other("shared blocks are not available on this system yet")) }
    fn unlink(&self) {}
    fn slot(&self, _: u32) -> *mut u8 { std::ptr::null_mut() }
}

/// Frames asked of the provider and not yet answered.
const IN_FLIGHT: usize = 4;
/// The window's bound in bytes, whatever the read-ahead setting says.
const BUDGET: u64 = 4 << 30;
/// A provider that does not answer an open within this is taken for hung.
const PATIENCE: Duration = Duration::from_secs(20);

/// The providers described on this machine, read once.
pub fn providers() -> &'static [Described] {
    static ALL: OnceLock<Vec<Described>> = OnceLock::new();
    ALL.get_or_init(|| if cfg!(unix) { min_framelink::described::installed() } else { vec![] })
}
/// The extensions some provider opens (lower case, no dot). Empty where none is installed: nothing changes then.
pub fn extensions() -> Vec<String> { let mut all: Vec<String> = providers().iter().flat_map(|p| p.extensions.iter().cloned()).collect(); all.sort(); all.dedup(); all }
pub fn provider_for(path: &str) -> Option<&'static Described> {
    let ext = Path::new(path).extension()?.to_string_lossy().to_ascii_lowercase();
    providers().iter().find(|p| p.extensions.iter().any(|e| *e == ext))
}
/// Whether a provider opens this path.
pub fn reads(path: &str) -> bool { provider_for(path).is_some() }

type Reply = Box<dyn FnOnce(ToHost) + Send>;
/// One running provider: requests go out with a number, and each answer finds its asker by it.
struct Hub { provider: Provider, waiting: Mutex<HashMap<u64, Reply>>, alive: AtomicBool, page: u64, blocks: AtomicU32 }

fn id_of(m: &ToHost) -> Option<u64> {
    match m { ToHost::Clip(c) => Some(c.id), ToHost::Attached { id, .. } | ToHost::Detached { id, .. } | ToHost::FrameDone { id, .. } | ToHost::SoundDone { id, .. } | ToHost::Closed { id, .. } | ToHost::Controls { id, .. } | ToHost::SettingsShown { id, .. } => Some(*id),
        ToHost::Error { id, .. } => *id, ToHost::Hello { .. } | ToHost::SettingsChanged { .. } => None }
}

impl Hub {
    fn of(d: &Described) -> Result<Arc<Hub>, Error> {
        static HUBS: Mutex<Vec<(PathBuf, Arc<Hub>)>> = Mutex::new(Vec::new());
        let mut hubs = HUBS.lock().unwrap();
        hubs.retain(|(_, h)| h.alive.load(Acquire));
        if let Some((_, h)) = hubs.iter().find(|(exe, _)| *exe == d.executable) { return Ok(h.clone()) }
        let (provider, answers) = Provider::start(&d.executable, &d.args, "QCView").map_err(|e| Error::Open(format!("{}: {e}", d.name)))?;
        let page = match answers.recv_timeout(PATIENCE) { Ok((_, Some(ToHost::Hello { version: VERSION, page, .. }))) => page, Ok((_, Some(ToHost::Error { message, .. }))) => return Err(Error::Open(format!("{}: {message}", d.name))), _ => return Err(Error::Open(format!("{} did not answer", d.name))) };
        let hub = Arc::new(Hub { provider, waiting: Mutex::new(HashMap::new()), alive: AtomicBool::new(true), page, blocks: AtomicU32::new(0) });
        let h = hub.clone();
        std::thread::Builder::new().name("qcv-remote-in".into()).spawn(move || {
            for (_, m) in answers {
                let Some(m) = m else { break };
                if let Some(id) = id_of(&m) && let Some(reply) = h.waiting.lock().unwrap().remove(&id) { reply(m) }
            }
            // the provider is gone: nobody waits for an answer any more
            h.alive.store(false, Release);
            h.waiting.lock().unwrap().clear();
        }).map_err(|e| Error::Open(e.to_string()))?;
        hubs.push((d.executable.clone(), hub.clone()));
        Ok(hub)
    }
    /// Send what `make` builds for a fresh number; `reply` is called with its answer, on the hub's thread.
    fn ask(&self, make: impl FnOnce(u64) -> ToProvider, reply: Reply) -> bool {
        let id = self.provider.id();
        self.waiting.lock().unwrap().insert(id, reply);
        if self.alive.load(Acquire) && self.provider.send(&make(id)).is_ok() { return true }
        self.waiting.lock().unwrap().remove(&id);
        false
    }
    /// Ask and wait for the answer; None when the provider is gone or silent.
    fn call(&self, make: impl FnOnce(u64) -> ToProvider, patience: Duration) -> Option<ToHost> {
        let (tx, rx) = channel();
        if !self.ask(make, Box::new(move |m| { let _ = tx.send(m); })) { return None }
        let until = Instant::now() + patience;
        loop { match rx.recv_timeout(Duration::from_millis(100)) { Ok(m) => return Some(m), Err(RecvTimeoutError::Timeout) if self.alive.load(Acquire) && Instant::now() < until => continue, Err(_) => return None } }
    }
}

/// A slot's bytes read in place by the presenter. While one of these lives its slot is not given to another frame.
struct SlotBytes { block: Arc<Block>, slot: u32, len: usize, _pin: Arc<()> }
unsafe impl Send for SlotBytes {}
unsafe impl Sync for SlotBytes {}
impl AsRef<[u8]> for SlotBytes { fn as_ref(&self) -> &[u8] { unsafe { std::slice::from_raw_parts(self.block.slot(self.slot), self.len) } } }

enum Ev { Answer(ToHost, i32, u32), Wake }
#[derive(Default)]
struct Window {
    playhead: i32,
    /// frame -> (slot, width, height)
    made: HashMap<i32, (u32, u32, u32)>,
    failed: Vec<i32>,
    block: Option<Arc<Block>>,
    pins: Vec<Arc<()>>,
}
struct Shared { window: Mutex<Window>, wake: Mutex<Sender<Ev>>, stop: AtomicBool, dead: AtomicBool, why: Mutex<String> }

pub struct RemoteDecoder {
    pub info: SessionInfo, index: Arc<FrameIndex>, fps: f64, frames: i32,
    shared: Arc<Shared>, worker: Option<std::thread::JoinHandle<()>>,
    playing: AtomicBool, seeks: AtomicU32, published: AtomicU64, current: AtomicI32,
    /// a user seek's target, still to land
    want: Mutex<Option<i32>>,
    /// the frame `fetch_due` handed out last (each frame comes back once)
    last: AtomicI32,
    shown: Mutex<Option<FrameParts>>,
    /// The provider's name, and the clip's facts as it stated them (the Inspector's).
    pub provider: String, pub facts: ClipFacts,
}
#[derive(Clone)]
struct FrameParts { f: i32, bytes: Arc<SlotBytes>, w: u32, h: u32 }

fn meta() -> ColorMeta {
    ColorMeta { matrix: Matrix::Identity, range: Range::Full, transfer: Transfer::Linear, primaries: Primaries::Bt2020, bit_depth: 16, chroma: Chroma::Rgb, range_forced: false, rotation: 0, sar: (1, 1), mastering: None, raw: None }
}

impl RemoteDecoder {
    /// `ahead` / `behind`: the read-ahead setting's window, in frames (bounded here by [`BUDGET`]).
    pub fn open(path: &str, ahead: i32, behind: i32) -> Result<RemoteDecoder, Error> {
        let described = provider_for(path).ok_or_else(|| Error::Open("no provider for this kind of file is installed".into()))?;
        let hub = Hub::of(described)?;
        let facts = match hub.call(|id| ToProvider::Open { id, path: path.to_string(), gamut: Some("rec2020".into()) }, PATIENCE) {
            Some(ToHost::Clip(c)) => c, Some(ToHost::Error { message, .. }) => return Err(Error::Open(message)), _ => return Err(Error::Open(format!("{} did not answer", described.name))) };
        if facts.frames == 0 || facts.picture.sizes.is_empty() { return Err(Error::Open("the clip has no frames".into())) }
        // the smallest size that still covers a full-HD picture, until a viewer says what it shows
        let size = facts.picture.sizes.iter().filter(|s| s.width >= 1920).min_by_key(|s| s.width).or_else(|| facts.picture.sizes.iter().max_by_key(|s| s.width)).cloned().unwrap();
        let slots = ((ahead + behind + 1).max(8) as u64).min(BUDGET / size.bytes.max(1)).max(8) as u32;
        let number = hub.blocks.fetch_add(1, AcqRel) + 1;
        let name = format!("/qcv-{}-{number}", std::process::id());
        let bytes = (size.bytes.div_ceil(hub.page.max(1)) * hub.page.max(1)) as usize;
        let block = Arc::new(Block::create(&name, slots, bytes).map_err(|e| Error::Open(format!("{slots} frames of shared memory: {e}")))?);
        let attached = hub.call(|id| ToProvider::Attach { id, block: number, name: name.clone(), slots, slot_bytes: bytes as u64, purpose: Purpose::Frames }, PATIENCE);
        block.unlink();
        if !matches!(attached, Some(ToHost::Attached { .. })) { return Err(Error::Open(match attached { Some(ToHost::Error { message, .. }) => message, _ => format!("{} did not take the shared memory", described.name) })) }

        let (frames, rate) = (facts.frames.min(i32::MAX as u64) as i32, (facts.rate.0.max(1) as i32, facts.rate.1.max(1) as i32));
        let fps = rate.0 as f64 / rate.1 as f64;
        let info = SessionInfo { width: facts.width, height: facts.height, frame_count: frames, codec_name: described.name.clone(), codec_short: "remote".into(), pix_fmt: "rgbaf16le".into(), hw_accel: String::new(),
            source_timecode: (!facts.timecode.is_empty()).then(|| facts.timecode.clone()), drop_frame: facts.drop_frame, time_base: (rate.1, rate.0), frame_rate: rate, intra_only: true, meta: meta(), duration_secs: frames as f64 / fps, tags: Default::default() };
        let index = Arc::new(FrameIndex::new(rate.1, rate.0, rate.0, rate.1, frames, true));
        let (tx, rx) = channel();
        let shared = Arc::new(Shared { window: Mutex::new(Window { block: Some(block.clone()), pins: (0..slots).map(|_| Arc::new(())).collect(), ..Default::default() }), wake: Mutex::new(tx.clone()), stop: AtomicBool::new(false), dead: AtomicBool::new(false), why: Mutex::new(String::new()) });
        let job = Worker { hub, clip: facts.clip, block: number, scale: size.scale, frames, slots, behind: behind.clamp(0, slots as i32 / 4), shared: shared.clone(), tx, rx, flying: HashMap::new(), free: (0..slots).rev().collect() };
        let worker = std::thread::Builder::new().name("qcv-remote".into()).spawn(move || job.run()).map_err(|e| Error::Open(e.to_string()))?;
        Ok(RemoteDecoder { info, index, fps, frames, shared, worker: Some(worker), playing: AtomicBool::new(false), seeks: AtomicU32::new(0), published: AtomicU64::new(0), current: AtomicI32::new(-1),
            want: Mutex::new(Some(0)), last: AtomicI32::new(-1), shown: Mutex::new(None), provider: described.name.clone(), facts })
    }
    pub fn frame_index(&self) -> &Arc<FrameIndex> { &self.index }
    pub fn play(&self) { self.playing.store(true, Release) }
    pub fn pause(&self) { self.playing.store(false, Release) }
    pub fn is_playing(&self) -> bool { self.playing.load(Acquire) }
    pub fn state(&self) -> State { if self.shared.dead.load(Acquire) { State::Errored } else { State::Decoding } }
    pub fn seeks_performed(&self) -> u32 { self.seeks.load(Acquire) }
    pub fn published_seq(&self) -> u64 { self.published.load(Acquire) }
    pub fn current_frame(&self) -> i32 { self.current.load(Acquire) }
    /// Why the source stopped (the provider ended, or did not answer); empty while it runs.
    pub fn last_error(&self) -> String { self.shared.why.lock().unwrap().clone() }
    pub fn failed_count(&self) -> usize { self.shared.window.lock().unwrap().failed.len() }
    /// Frames ready before and after the playhead without a gap, as fractions of the clip (the scrubber's band).
    pub fn buffered(&self) -> (i32, i32) {
        let w = self.shared.window.lock().unwrap();
        ((1..=w.playhead).take_while(|d| w.made.contains_key(&(w.playhead - d))).count() as i32, (1..self.frames - w.playhead).take_while(|d| w.made.contains_key(&(w.playhead + d))).count() as i32)
    }

    fn move_to(&self, f: i32) {
        let f = f.clamp(0, self.frames - 1);
        let mut w = self.shared.window.lock().unwrap();
        if w.playhead != f { w.playhead = f; drop(w); let _ = self.shared.wake.lock().unwrap().send(Ev::Wake); }
    }
    pub fn seek_to_frame(&self, f: i32) {
        self.move_to(f);
        *self.want.lock().unwrap() = Some(f.clamp(0, self.frames - 1));
        self.last.store(-1, Release);
        self.seeks.fetch_add(1, AcqRel);
    }
    /// A drag move: the playhead follows.
    pub fn scrub_to(&self, f: i32) { self.move_to(f); self.last.store(-1, Release); }

    fn parts(&self, f: i32) -> Option<FrameParts> {
        let w = self.shared.window.lock().unwrap();
        let &(slot, width, height) = w.made.get(&f)?;
        let block = w.block.clone()?;
        Some(FrameParts { f, bytes: Arc::new(SlotBytes { block, slot, len: width as usize * height as usize * 8, _pin: w.pins[slot as usize].clone() }), w: width, h: height })
    }
    fn handle(&self, p: &FrameParts) -> FrameHandle {
        // rows of a multiple of 256 bytes are read in place; any other width is copied once into padded rows
        let row = p.w as usize * 8;
        let plane = if row % 256 == 0 { Plane { data: p.bytes.clone(), width: p.w, height: p.h, pitch: row as u32 } } else {
            let pitch = row.div_ceil(256) * 256;
            let (src, mut padded) = ((*p.bytes).as_ref(), vec![0u8; pitch * p.h as usize]);
            for y in 0..p.h as usize { padded[y * pitch..y * pitch + row].copy_from_slice(&src[y * row..(y + 1) * row]) }
            Plane::owned(padded, p.w, p.h, pitch as u32)
        };
        let frame = Frame { width: p.w, height: p.h, pixels: Pixels::Planar { planes: vec![plane], layout: PlanarLayout::RgbaF16 { premultiplied: true } }, meta: meta() };
        FrameHandle { frame, pts: p.f as i64, pts_us: if self.fps > 0.0 { (p.f as f64 / self.fps * 1e6) as i64 } else { 0 }, frame_no: p.f, keyframe: true, decode_ms: 0.0 }
    }
    fn landed(&self, p: FrameParts) -> Option<FrameHandle> {
        self.current.store(p.f, Release);
        self.published.fetch_add(1, AcqRel);
        let h = self.handle(&p);
        *self.shown.lock().unwrap() = Some(p);
        Some(h)
    }
    /// The seek target once it is there. A target that failed lands without a picture: the old one holds.
    pub fn fetch_front(&self) -> Option<FrameHandle> {
        let mut want = self.want.lock().unwrap();
        let f = (*want)?;
        if let Some(p) = self.parts(f) { *want = None; return self.landed(p) }
        if self.shared.dead.load(Acquire) || self.shared.window.lock().unwrap().failed.contains(&f) { *want = None; self.current.store(f, Release); self.published.fetch_add(1, AcqRel); }
        None
    }
    /// The playhead is at `f` (`i32::MAX` = wherever a gesture left it): the frame, once, when it is there.
    pub fn fetch_due(&self, f: i32) -> (Option<FrameHandle>, u32) {
        if self.want.lock().unwrap().is_some() { return (self.fetch_front(), 0) }
        let f = if f == i32::MAX { self.shared.window.lock().unwrap().playhead } else { self.move_to(f); f.clamp(0, self.frames - 1) };
        if self.last.load(Acquire) == f { return (None, 0) }
        let Some(p) = self.parts(f) else { return (None, 0) };
        self.last.store(f, Release);
        (self.landed(p), 0)
    }
    /// The frame shown last.
    pub fn fetch_latest(&self) -> Option<FrameHandle> { self.shown.lock().unwrap().clone().map(|p| self.handle(&p)) }
}
impl Drop for RemoteDecoder {
    fn drop(&mut self) {
        self.shared.stop.store(true, Release);
        let _ = self.shared.wake.lock().unwrap().send(Ev::Wake);
        if let Some(w) = self.worker.take() { let _ = w.join(); }
    }
}

struct Worker { hub: Arc<Hub>, clip: u32, block: u32, scale: u32, frames: i32, slots: u32, behind: i32, shared: Arc<Shared>, tx: Sender<Ev>, rx: Receiver<Ev>, flying: HashMap<i32, u32>, free: Vec<u32> }
impl Worker {
    fn die(&self, why: &str) { *self.shared.why.lock().unwrap() = why.to_string(); self.shared.dead.store(true, Release); }
    fn run(mut self) {
        loop {
            if self.shared.stop.load(Acquire) { break }
            if !self.hub.alive.load(Acquire) { self.die("The provider ended."); break }
            self.feed();
            match self.rx.recv_timeout(Duration::from_millis(200)) {
                Ok(Ev::Answer(m, frame, slot)) => self.answered(m, frame, slot),
                Ok(Ev::Wake) | Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            while let Ok(e) = self.rx.try_recv() { if let Ev::Answer(m, frame, slot) = e { self.answered(m, frame, slot) } }
        }
        // The clip and the block are given back. A provider that is alive answers `detached` once nothing of ours is
        // under way; only then (or once it is gone) may the memory be let go - and it stays mapped for as long as a
        // frame still on screen reads it.
        let block = self.shared.window.lock().unwrap().block.take();
        if self.hub.alive.load(Acquire) {
            let detached = self.hub.call(|id| ToProvider::Detach { id, block: self.block }, Duration::from_secs(5));
            let _ = self.hub.call(|id| ToProvider::Close { id, clip: self.clip }, Duration::from_secs(5));
            // no answer from a provider still running: it may yet write - the mapping is kept rather than pulled from under it
            if detached.is_none() && self.hub.alive.load(Acquire) && let Some(b) = block { std::mem::forget(b) }
        }
    }
    fn answered(&mut self, m: ToHost, frame: i32, slot: u32) {
        self.flying.remove(&frame);
        let mut w = self.shared.window.lock().unwrap();
        match m {
            ToHost::FrameDone { result: FrameResult::Made, width, height, .. } => { w.made.insert(frame, (slot, width, height)); }
            ToHost::FrameDone { result: FrameResult::Failed, message, .. } => { log::warn!("remote sequence: frame {frame}: {message}"); if !w.failed.contains(&frame) { w.failed.push(frame) } self.free.push(slot) }
            _ => self.free.push(slot),
        }
    }
    /// Ask for the frames wanted most, as far as flight and slots allow: the playhead's, then on from it (round the
    /// clip's end), and what lies just behind is kept.
    fn feed(&mut self) {
        if self.flying.len() >= IN_FLIGHT { return }
        let mut w = self.shared.window.lock().unwrap();
        let (c, n) = (w.playhead, self.frames);
        let ahead = (self.slots as i32 - IN_FLIGHT as i32 - self.behind).max(1).min(n);
        let wanted: Vec<i32> = (0..ahead).map(|d| (c + d) % n).collect();
        let kept = |f: i32| wanted.contains(&f) || (f < c && c - f <= self.behind);
        for &frame in &wanted {
            if self.flying.len() >= IN_FLIGHT { break }
            if w.made.contains_key(&frame) || self.flying.contains_key(&frame) || w.failed.contains(&frame) { continue }
            let slot = match self.free.pop() { Some(s) => s, None => {
                // give up the frame farthest from the playhead that is neither wanted nor still read by the presenter
                let far = |f: i32| if f >= c { f - c } else { (c - f) * 3 };
                let victim = w.made.iter().filter(|(f, (slot, ..))| !kept(**f) && Arc::strong_count(&w.pins[*slot as usize]) == 1).max_by_key(|(f, _)| far(**f)).map(|(f, _)| *f);
                let Some(victim) = victim else { break };
                w.made.remove(&victim).unwrap().0
            } };
            let tx = self.tx.clone();
            let (clip, scale, block) = (self.clip, self.scale, self.block);
            if !self.hub.ask(|id| ToProvider::Frame { id, clip, frame: frame as u64, scale, generation: 0, block, slot }, Box::new(move |m| { let _ = tx.send(Ev::Answer(m, frame, slot)); })) { self.free.push(slot); break }
            self.flying.insert(frame, slot);
        }
    }
}
