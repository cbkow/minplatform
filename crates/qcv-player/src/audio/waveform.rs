//! `WaveformProbeEngine` (QCView `audio/waveform_probe_engine.cpp`), verbatim: lazy, screen-space peak sampling for the
//! scrubber's waveform. Nothing is scanned up front — the strip asks for its visible window at column resolution, a
//! worker probes ONLY what those columns need and memoises every probe on a power-of-two STRIDE LADDER (base 5 ms ·
//! 2^rung) so coarse probes serve finer vantages and vice versa. Spans ≤ 60 s read every slot's full stride
//! (sample-exact); longer spans take a 20-ms probe per slot in ascending order (a map, not evidence). Probing suspends
//! while the transport runs; paint-side sampling falls back across nearby rungs while the exact one fills in.
use super::chunk_reader::AudioChunkReader;
use super::routing::RoutingMode;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::*};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

const BASE_SLOT_SEC: f64 = 0.005;
const MAX_RUNG: i32 = 24;               // 5 ms · 2^24 ≈ 23 h
const LINEAR_SPAN_MAX_SEC: f64 = 60.0;
const SPARSE_PROBE_FRAMES: usize = 960; // 20 ms @ 48 kHz
const MAX_SOURCES: usize = 16;   // a playlist's clips on screen at once (canon row 280; QCView's 4 served one strip per row)
const OUT_RATE: f64 = 48000.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ColumnPeak { pub mn: f32, pub mx: f32, pub valid: bool }

#[derive(Clone, Copy, Default)]
struct Request { t0: f64, t1: f64, columns: usize, generation: u64 }
#[derive(Default)]
struct SourceState { rungs: HashMap<i32, HashMap<i64, (f32, f32)>>, touched: u64 }
#[derive(Default)]
struct State { pending: HashMap<String, Request>, sources: HashMap<String, SourceState>, gen_counter: u64, touch_counter: u64 }

struct Inner { st: Mutex<State>, cv: Condvar, paused: AtomicBool, quit: AtomicBool, data_gen: AtomicU64 }

pub struct WaveformProbeEngine { inner: Arc<Inner>, thread: Option<std::thread::JoinHandle<()>> }

fn rung_for(column_span: f64) -> i32 { if column_span <= BASE_SLOT_SEC { 0 } else { ((column_span / BASE_SLOT_SEC).log2().ceil() as i32).clamp(0, MAX_RUNG) } }
fn stride_of(rung: i32) -> f64 { BASE_SLOT_SEC * 2f64.powi(rung) }

impl Default for WaveformProbeEngine { fn default() -> Self { Self::new() } }
impl WaveformProbeEngine {
    pub fn new() -> WaveformProbeEngine {
        let inner = Arc::new(Inner { st: Mutex::new(State::default()), cv: Condvar::new(), paused: AtomicBool::new(false), quit: AtomicBool::new(false), data_gen: AtomicU64::new(0) });
        let i2 = inner.clone();
        let thread = std::thread::Builder::new().name("qcv-waveform-probe".into()).spawn(move || worker(i2)).expect("waveform worker");
        WaveformProbeEngine { inner, thread: Some(thread) }
    }
    /// Cover `[t0, t1]` source seconds at `columns` resolution; a newer request for the same path replaces the pending one.
    pub fn request_window(&self, path: &str, t0: f64, t1: f64, columns: usize) {
        if path.is_empty() || columns == 0 || t1 <= t0 { return }
        { let mut st = self.inner.st.lock().unwrap(); st.gen_counter += 1; let g = st.gen_counter; st.pending.insert(path.to_string(), Request { t0, t1, columns, generation: g }); }
        self.inner.cv.notify_all();
    }
    /// True while the transport runs — the worker suspends until cleared.
    pub fn set_playback_active(&self, active: bool) { self.inner.paused.store(active, Relaxed); if !active { self.inner.cv.notify_all(); } }
    /// Bumps whenever new peaks landed (the strip repaints when it changes).
    pub fn data_generation(&self) -> u64 { self.inner.data_gen.load(Relaxed) }
    /// Paint-side sampling: for each column the exact rung's slot, else nearby coarser rungs (a progressive estimate), then finer.
    pub fn sample_columns(&self, path: &str, t0: f64, t1: f64, columns: usize) -> Vec<ColumnPeak> {
        let mut out = vec![ColumnPeak::default(); columns];
        if columns == 0 || t1 <= t0 { return out }
        let st = self.inner.st.lock().unwrap();
        let Some(src) = st.sources.get(path) else { return out };
        let column_span = (t1 - t0) / columns as f64;
        let rung = rung_for(column_span);
        for (x, o) in out.iter_mut().enumerate() {
            let tc = t0 + (x as f64 + 0.5) * column_span;
            if tc < 0.0 { continue }
            for dr in [0, 1, 2, 3, 4, 5, 6, -1, -2] {
                let r = rung + dr; if !(0..=MAX_RUNG).contains(&r) { continue }
                let Some(m) = src.rungs.get(&r) else { continue };
                if let Some(&(mn, mx)) = m.get(&((tc / stride_of(r)).floor() as i64)) { *o = ColumnPeak { mn, mx, valid: true }; break }
            }
        }
        out
    }
}
impl Drop for WaveformProbeEngine { fn drop(&mut self) { self.inner.quit.store(true, Relaxed); self.inner.cv.notify_all(); if let Some(t) = self.thread.take() { let _ = t.join(); } } }

fn fold_peaks(samples: &[f32], mn: &mut f32, mx: &mut f32) { for &s in samples { if s < *mn { *mn = s; } if s > *mx { *mx = s; } } }

fn worker(inner: Arc<Inner>) {
    struct OpenReader { reader: AudioChunkReader, touched: u64 }
    let mut readers: HashMap<String, OpenReader> = HashMap::new();
    let mut reader_touch = 0u64;
    let mut buf: Vec<f32> = vec![];
    let mut emit_timer = Instant::now();
    loop {
        let (path, req) = {
            let mut st = inner.st.lock().unwrap();
            loop {
                if inner.quit.load(Relaxed) { return }
                if !st.pending.is_empty() && !inner.paused.load(Relaxed) { break }
                st = inner.cv.wait_timeout(st, Duration::from_millis(100)).unwrap().0;
            }
            let path = st.pending.keys().next().cloned().unwrap();
            let req = st.pending.remove(&path).unwrap();
            st.touch_counter += 1; let tc = st.touch_counter;
            st.sources.entry(path.clone()).or_default().touched = tc;
            while st.sources.len() > MAX_SOURCES { let oldest = st.sources.iter().min_by_key(|(_, s)| s.touched).map(|(k, _)| k.clone()).unwrap(); st.sources.remove(&oldest); }
            (path, req)
        };
        reader_touch += 1;
        if !readers.contains_key(&path) {
            while readers.len() >= 2 { let oldest = readers.iter().min_by_key(|(_, r)| r.touched).map(|(k, _)| k.clone()).unwrap(); readers.remove(&oldest); }
            match AudioChunkReader::open(&path, RoutingMode::Auto) { Ok(Some(r)) => { readers.insert(path.clone(), OpenReader { reader: r, touched: reader_touch }); } Ok(None) => { log::info!("waveform: no decodable audio in the file"); continue } Err(e) => { log::warn!("waveform: {e}"); continue } }
        }
        let entry = readers.get_mut(&path).unwrap(); entry.touched = reader_touch;
        let reader = &mut entry.reader;
        let column_span = (req.t1 - req.t0) / req.columns as f64;
        let rung = rung_for(column_span); let stride = stride_of(rung);
        let linear = (req.t1 - req.t0) <= LINEAR_SPAN_MAX_SEC;
        let src_duration = reader.duration();
        let (i0, i1) = ((req.t0.max(0.0) / stride).floor() as i64, (req.t1.max(0.0) / stride).floor() as i64);
        let mut uncovered = vec![]; let mut refine = vec![]; let mut synthesized = false;
        {
            let mut st = inner.st.lock().unwrap();
            let rungs = &mut st.sources.entry(path.clone()).or_default().rungs;
            for i in i0..=i1 {
                if src_duration > 0.0 && i as f64 * stride >= src_duration { break }
                if rungs.get(&rung).is_some_and(|m| m.contains_key(&i)) { continue }
                let mut made = false;
                for dr in 1..=3 {
                    if made { break }
                    let Some(finer) = rungs.get(&(rung - dr)) else { continue };
                    let kids = 1i64 << dr; let first = i * kids;
                    let (mut mn, mut mx, mut found) = (0.0f32, 0.0f32, 0i64);
                    for k in 0..kids { if let Some(&(a, b)) = finer.get(&(first + k)) { mn = mn.min(a); mx = mx.max(b); found += 1; } }
                    if found * 2 >= kids { rungs.entry(rung).or_default().insert(i, (mn, mx)); made = true; synthesized = true; }
                }
                if made { continue }
                let covered = (1..=6).any(|dr| rungs.get(&(rung + dr)).is_some_and(|m| m.contains_key(&(i >> dr))));
                if covered { refine.push(i) } else { uncovered.push(i) }
            }
        }
        if synthesized { inner.data_gen.fetch_add(1, Relaxed); }
        if uncovered.is_empty() && refine.is_empty() { inner.data_gen.fetch_add(1, Relaxed); continue }
        let stride_frames = ((stride * OUT_RATE).round() as usize).max(1);
        let probe_frames = if linear { stride_frames } else { stride_frames.min(SPARSE_PROBE_FRAMES) };
        buf.resize(probe_frames * 2, 0.0);
        let plan: Vec<i64> = uncovered.into_iter().chain(refine).collect();
        let mut aborted = false;
        for slot in plan {
            {
                let st = inner.st.lock().unwrap();
                if inner.quit.load(Relaxed) { return }
                if st.pending.get(&path).is_some_and(|p| p.generation != req.generation) || inner.paused.load(Relaxed) { aborted = true; break }
            }
            let t = slot as f64 * stride;
            let got = reader.read_at(t, &mut buf, probe_frames);
            let (mut mn, mut mx) = (0.0f32, 0.0f32);
            if got > 0 { fold_peaks(&buf[..got * 2], &mut mn, &mut mx); }
            inner.st.lock().unwrap().sources.entry(path.clone()).or_default().rungs.entry(rung).or_default().insert(slot, (mn, mx));
            if emit_timer.elapsed() > Duration::from_millis(80) { emit_timer = Instant::now(); inner.data_gen.fetch_add(1, Relaxed); }
        }
        inner.data_gen.fetch_add(1, Relaxed);
        if aborted && inner.paused.load(Relaxed) { let mut st = inner.st.lock().unwrap(); st.pending.entry(path).or_insert(req); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rung_ladder() { assert_eq!(rung_for(0.004), 0); assert_eq!(rung_for(0.005), 0); assert_eq!(rung_for(0.011), 2); assert_eq!(rung_for(1.0e9), MAX_RUNG); assert!((stride_of(3) - 0.04).abs() < 1e-12); }
}
