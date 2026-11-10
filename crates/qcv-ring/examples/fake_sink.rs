//! A STAND-IN output sink (canon row 227 step 3): behaves like QCBridgeA (or, `--ndi`, QCBridgeN) on the QCVO bus so QCView's
//! side of an output can be checked without the card or the NDI runtime — the O13 lifetime (poll the ring every 250 ms, attach,
//! request, heartbeat, quit when a ring that launched it retires), AJA's request (RGB10, auto raster, transfer = QCView's
//! setting, letterbox, composite), the HDR block, every frame's desc checked, a status line like the real sinks', a report
//! every second (fps, the shape, the centre pixel's 10-bit code, timecode, Paused, audio packets).
//! `--cadence`: the frame-rate check — the ring polled every millisecond; each new frame's arrival interval and its step in
//! `source_frame_index` (+1 = the next frame, 0 = the same one again, > 1 = frames never seen, < 0 = an older frame after a
//! newer one), and a stand-in CARD that takes the newest frame once per source-frame period, as QCBridgeA's does (+1 = a
//! new picture, 0 = a repeat, > 1 = a skip). A line a second while playing, the totals when the ring retires. `--card-ppm N`
//! runs that card's clock N parts per million slow (a real card's clock is not the computer's: the phase between the two
//! slides, and the check should see every phase). `--card-queue N`: the card takes the OLDEST of up to N frames queued as they
//! arrive (a sink that reads the ring as frames land, not once per card frame) — a repeat only when the queue is empty, a
//! skip only when it overflows.
//!   register it: a manifest {"id":"ajatest","name":"Fake AJA (test)","label":"AJA test","product":"fake_sink",
//!   "exec":"<this binary>","args":["--id","ajatest"],"raster":"auto"} in $QCV_OUTPUT_MANIFEST_DIR
use qcv_ring::layout::*;
use qcv_ring::qcvo::{self, v, OutputRing};
use std::time::{Duration, Instant};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let id = std::env::var("QCVO_ID").ok().or_else(|| a.iter().position(|x| x == "--id").and_then(|i| a.get(i + 1).cloned())).unwrap_or_else(|| "ajatest".into());
    let ndi = a.iter().any(|x| x == "--ndi");
    let cadence = a.iter().any(|x| x == "--cadence");
    let mut cad = Cadence { queue_cap: a.iter().position(|x| x == "--card-queue").and_then(|i| a.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(0), ppm: a.iter().position(|x| x == "--card-ppm").and_then(|i| a.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(0.0), ..Default::default() };
    // the layers asked for: both, as the real sinks do by default (`--no-layers` = the clean picture)
    let layers = if a.iter().any(|x| x == "--no-layers") { 0 } else { v::LAYER_GUIDES | v::LAYER_ANNOTATIONS };
    let name = qcvo::name_for(&id).expect("a valid id"); let aname = qcvo::audio_name_for(&id).unwrap();
    println!("fake_sink {id}: waiting for {name}");
    loop {
        let Ok(mut r) = OutputRing::open(&name) else { std::thread::sleep(Duration::from_millis(250)); continue };
        if r.header().state.load(std::sync::atomic::Ordering::Acquire) == OutputState::Retired as u32 || !r.producer_alive() { drop(r); std::thread::sleep(Duration::from_millis(500)); continue }
        let launched = r.header().header_flags & OUTPUT_LAUNCHED_BY_PRODUCER != 0;
        r.attach(qcvo::now_ns());
        // AJA: RGB10, auto raster, QCView's transfer; NDI: 1080p BGRA8 SDR
        r.write_request(if ndi { OutputRequest { protocol_version: 1, width: 1920, height: 1080, pixel_format: OutputPixelFormat::Bgra8 as u32, transfer: v::TRANSFER_SDR, fit: v::FIT_LETTERBOX, content: v::CONTENT_COMPOSITE, layers, ..Default::default() } }
            else { OutputRequest { protocol_version: 1, width: 0, height: 0, pixel_format: OutputPixelFormat::Rgb10 as u32, transfer: 0, fit: v::FIT_LETTERBOX, content: v::CONTENT_COMPOSITE, layers, ..Default::default() } });
        let audio = qcv_ring::audio::AudioRing::open(&aname).ok(); let mut alast = 0u64; if let Some(au) = &audio { au.skip_to_latest(&mut alast); }
        let (mut hb, mut report, mut frames, mut apk, mut hdr_known) = (Instant::now(), Instant::now(), 0u64, 0u64, 0u64);
        let mut edge = 0u32;
        let mut last: Option<(OutputFrameDesc, u32)> = None; let mut shape = String::from("attached");
        loop {
            let st = r.header().state.load(std::sync::atomic::Ordering::Acquire);
            if st == OutputState::Retired as u32 || !r.producer_alive() { println!("fake_sink {id}: the ring retired"); break }
            if hb.elapsed() >= Duration::from_millis(250) { r.heartbeat(qcvo::now_ns()); hb = Instant::now(); }
            if let Some((rep, _)) = r.read_reply() {
                let msg = String::from_utf8_lossy(&rep.message[..rep.message.iter().position(|&c| c == 0).unwrap_or(rep.message.len())]).into_owned();
                println!("fake_sink {id}: reply {} {}x{} fmt {} transfer {} layers {:#x} — \"{msg}\"", if rep.accepted != 0 { "ACCEPTED" } else { "REFUSED" }, rep.width, rep.height, rep.pixel_format, rep.transfer, rep.layers);
                shape = msg;
            }
            if let Some(h) = r.read_hdr_info(&mut hdr_known) { println!("fake_sink {id}: HDR info: transfer {} primaries {} peak {} min {} MaxCLL {} MaxFALL {}", h.transfer, h.primaries, h.max_mastering_nits, h.min_mastering_nits, h.max_cll, h.max_fall); }
            if let Some((d, px)) = r.acquire_latest() {
                assert!(d.bytes_per_row >= d.width * 4 && d.bytes_per_row % 256 == 0, "the row pitch");
                let c = unsafe { std::ptr::read_unaligned(px.add(((d.height / 2) * d.bytes_per_row + (d.width / 2) * 4) as usize) as *const u32) };
                // where the 16x9 guide's title-safe line runs (5 % in, mid height): the layers' check
                edge = unsafe { std::ptr::read_unaligned(px.add(((d.height / 2) * d.bytes_per_row + (d.width / 20) * 4) as usize) as *const u32) };
                if cadence { cad.arrived(&d); }
                frames += 1; last = Some((d, c)); r.report_seen(r.newest_seq()); r.release();
            }
            if let Some(au) = &audio { let mut desc = AudioPacketDesc::default(); let mut planes = vec![0f32; 2 * 1024]; let mut dropped = 0; while au.next_packet(&mut alast, &mut desc, &mut planes, &mut dropped) == qcv_ring::audio::Next::Packet { apk += 1; } }
            if cadence { cad.card_tick(); }
            if report.elapsed() >= Duration::from_secs(1) {
                if cadence { cad.report(&id, false); }
                let dt = report.elapsed().as_secs_f64(); report = Instant::now();
                match &last {
                    Some((d, c)) => {
                        let px = |c: u32| if d.pixel_format == OutputPixelFormat::Rgb10 as u32 { format!("R{} G{} B{}", c & 0x3ff, (c >> 10) & 0x3ff, (c >> 20) & 0x3ff) } else { format!("B{} G{} R{}", c & 0xff, (c >> 8) & 0xff, (c >> 16) & 0xff) };
                        let lay = format!(" · layers {:#x} · 5 % in {}", d.layers, px(edge));
                        let centre = if d.pixel_format == OutputPixelFormat::Rgb10 as u32 { format!("R{} G{} B{} (10-bit)", c & 0x3ff, (c >> 10) & 0x3ff, (c >> 20) & 0x3ff) } else { format!("B{} G{} R{} (8-bit)", c & 0xff, (c >> 8) & 0xff, (c >> 16) & 0xff) };
                        let tc = if d.flags & v::FRAME_TIMECODE_VALID != 0 { format!("{:02}:{:02}:{:02}{}{:02}", d.timecode >> 24, (d.timecode >> 16) & 0xff, (d.timecode >> 8) & 0xff, if d.flags & v::FRAME_DROP_FRAME != 0 { ';' } else { ':' }, d.timecode & 0xff) } else { "—".into() };
                        println!("fake_sink {id}: {:.1} fps · {}x{} transfer {} · centre {centre} · frame {} @ {}/{} · TC {tc}{} · audio {apk} packets{}", frames as f64 / dt, d.width, d.height, d.transfer, d.source_frame_index, d.source_fps_num, d.source_fps_den, if d.flags & v::FRAME_PAUSED != 0 { " · PAUSED" } else { "" }, lay);
                    }
                    None => println!("fake_sink {id}: no frame yet"),
                }
                r.write_status(&format!("fake {} {}  ·  {:.0} fps", if ndi { "NDI" } else { "AJA" }, shape.split(" from").next().unwrap_or(""), frames as f64 / dt));
                frames = 0; apk = 0;
            }
            std::thread::sleep(Duration::from_millis(if cadence { 1 } else { 4 }));
        }
        if cadence { cad.report(&id, true); }
        if launched { println!("fake_sink {id}: launched by QCView — quitting"); return }
    }
}

/// `--cadence` (see the module's note). Steps are counted while the frames say Playing; a jump of more than a second of frames
/// either way is a seek or a loop, counted apart.
#[derive(Default)]
struct Cadence { ppm: f64, queue_cap: usize, queue: std::collections::VecDeque<i64>, primed: bool, last: Option<(Instant, i64)>, period: Option<Duration>, next_card: Option<Instant>, card_last: Option<i64>, newest: Option<(i64, bool, i64)>, sec: Tally, all: Tally }
#[derive(Default, Clone)]
struct Tally { new: u32, next: u32, same: u32, skipped: u32, back: u32, jumps: u32, arrival_ms: Vec<f32>, card_new: u32, card_repeat: u32, card_skip: u32, card_back: u32 }
impl Tally {
    fn line(&mut self) -> String {
        let v = &mut self.arrival_ms; v.sort_by(|a, b| a.total_cmp(b));
        let p = |q: f32| if v.is_empty() { 0.0 } else { v[(((v.len() - 1) as f32) * q).round() as usize] };
        format!("new {} · steps +1 {} / same {} / skipped {} / back {} / jumps {} · arrival ms p5 {:.1} p50 {:.1} p95 {:.1} max {:.1} · card: new {} repeat {} skip {} back {}", self.new, self.next, self.same, self.skipped, self.back, self.jumps, p(0.05), p(0.5), p(0.95), p(1.0), self.card_new, self.card_repeat, self.card_skip, self.card_back)
    }
}
impl Cadence {
    fn arrived(&mut self, d: &OutputFrameDesc) {
        let (now, idx, playing) = (Instant::now(), d.source_frame_index, d.flags & v::FRAME_PAUSED == 0);
        let sec_frames = if d.source_fps_den > 0 { (d.source_fps_num / d.source_fps_den).max(1) as i64 } else { 24 };
        if d.source_fps_num > 0 && d.source_fps_den > 0 { self.period = Some(Duration::from_secs_f64(d.source_fps_den as f64 / d.source_fps_num as f64 * (1.0 + self.ppm * 1e-6))); }
        self.newest = Some((idx, playing, sec_frames));
        if self.queue_cap > 0 { if self.queue.len() >= self.queue_cap { self.queue.pop_front(); } self.queue.push_back(idx); }
        if let Some((t, li)) = self.last && playing {
            let step = idx - li;
            for s in [&mut self.sec, &mut self.all] {
                s.new += 1;
                if step.abs() > sec_frames { s.jumps += 1; continue }
                s.arrival_ms.push(now.duration_since(t).as_secs_f32() * 1000.0);
                match step { 1 => s.next += 1, 0 => s.same += 1, x if x > 1 => s.skipped += (x - 1) as u32, _ => s.back += 1 }
            }
        }
        self.last = Some((now, idx));
    }
    fn card_tick(&mut self) {
        let (Some(period), Some((mut idx, playing, sec_frames))) = (self.period, self.newest) else { return };
        let now = Instant::now();
        let due = *self.next_card.get_or_insert(now + period);
        if now < due { return }
        self.next_card = Some(if now.duration_since(due) > period * 4 { now + period } else { due + period });
        if self.queue_cap > 0 {
            // primed with two (one frame of cushion against the arrival jitter); an empty queue = the last picture again
            if !self.primed { if self.queue.len() < 2.min(self.queue_cap) { return } self.primed = true; }
            idx = match self.queue.pop_front() { Some(i) => i, None => match self.card_last { Some(l) => l, None => return } };
        }
        if let Some(l) = self.card_last && playing {
            let step = idx - l;
            if step.abs() <= sec_frames { for s in [&mut self.sec, &mut self.all] { match step { 1 => s.card_new += 1, 0 => s.card_repeat += 1, x if x > 1 => s.card_skip += 1, _ => s.card_back += 1 } } }
        }
        self.card_last = Some(idx);
    }
    fn report(&mut self, id: &str, total: bool) {
        if total { println!("fake_sink {id}: cadence TOTAL: {}", self.all.line()); }
        else if self.sec.new > 0 { println!("fake_sink {id}: cadence: {}", self.sec.line()); }
        self.sec = Tally::default();
    }
}
