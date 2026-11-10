//! The OUTPUTS (QCView `window/output_bus.cpp`, canon row 227 step 3): QCView links no NDI / AJA SDK — each output is a SINK
//! app (QCBridgeN, QCBridgeA) registered by a manifest JSON in `…/QCView/Outputs`; Start creates the output's QCVO frame ring
//! + QCAA audio ring and launches the sink; the sink attaches, asks for a raster / format / transfer; QCView answers (accept or
//! refuse, QCView's rules and words) and publishes the composite at that raster whenever the picture changes (the sinks repeat
//! the last frame); the mixed audio before Listen / mute / volume goes out beside it. Stop retires the rings (the sink's cue)
//! and stops a sink it launched.
//!
//! Fixes over QCView (row 227 step 3, the user's go): Start does not launch a second sink when one is already attached; a sink
//! that fails to start or keeps crashing says so (QCView showed "starting" forever); a B side that isn't there is refused (QCView
//! accepted it and sent nothing); frames carry the timecode and the Paused flag (AJA's RP188, canon row 107).
//!
//! The LAYERS (canon row 238, reversing row 225; QCView's Guides / Notes chips): a sink gets the layers it asks for ∧ the two
//! toggles — the safety guide fitted to the picture (side by side: both halves; a wipe / difference: A's frame), the note's
//! strokes clipped to it, drawn only while the viewport draws them (the Notes panel open, single view). A vello overlay at
//! the raster, laid over the picture by the wire encoder: SDR as drawn, 203 nits on PQ = 75 % on HLG (QCView). The port's
//! rule for the guide's LINE WIDTH (the user 2026-11-09): the screen's width scaled by the output picture over the screen's
//! picture, so the wire shows the proportion the viewport shows (QCView: the same pixel count on every target).
use crate::settings::k;
use qcv_ring::layout::*;
use qcv_ring::qcvo::{self, v, OutputRing};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use qcv_player::Playback;

const POLL: Duration = Duration::from_millis(250);
/// A user-started sink attaches to a new ring within its 250-ms poll: Start waits this long before launching its own (fix 1).
const LAUNCH_GRACE: Duration = Duration::from_millis(700);

/// One registered sink (QCView `OutputBus::scan` + the manifest keys).
#[derive(Clone, Debug)]
#[allow(dead_code)]   // name / protocol: the manifest's record (one protocol today)
pub struct Manifest { pub id: String, pub name: String, pub label: String, pub product: String, pub exec: PathBuf, pub args: Vec<String>, pub protocol: u32, pub raster_settable: bool, pub installed: bool }

/// QCView's manifest folders: `$QCV_OUTPUT_MANIFEST_DIR`; macOS the machine's then the user's Application Support; Windows
/// `%ProgramData%\QCView\Outputs` then the user's `AppData\Roaming\QCView\QCView\Outputs`.
pub fn manifest_dirs() -> Vec<PathBuf> {
    let mut v = vec![];
    if let Some(d) = std::env::var_os("QCV_OUTPUT_MANIFEST_DIR") { v.push(PathBuf::from(d)); }
    #[cfg(target_os = "macos")] {
        v.push(PathBuf::from("/Library/Application Support/QCView/Outputs"));
        if let Some(h) = std::env::var_os("HOME") { v.push(PathBuf::from(h).join("Library/Application Support/QCView/Outputs")); }
    }
    #[cfg(windows)] {
        v.push(PathBuf::from(std::env::var_os("ProgramData").unwrap_or_else(|| "C:\\ProgramData".into())).join("QCView").join("Outputs"));
        if let Some(a) = std::env::var_os("APPDATA") { v.push(PathBuf::from(a).join("QCView").join("QCView").join("Outputs")); }
    }
    v
}
/// QCView `OutputBus::scan`: every `*.json` by name; the first registration of an id wins; a bad id is skipped.
pub fn scan() -> Vec<Manifest> {
    let mut out: Vec<Manifest> = vec![];
    for d in manifest_dirs() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect(); files.sort();
        for f in files {
            let Ok(v) = std::fs::read(&f).map_err(|_| ()).and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).map_err(|_| ())) else { log::warn!("outputs: {} is not JSON", f.display()); continue };
            let s = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
            let id = s("id");
            if !qcvo::id_ok(&id) { log::warn!("outputs: {}: bad id '{id}'", f.display()); continue }
            if out.iter().any(|m| m.id == id) { continue }
            let name = s("name");
            // label / product: the keys, else parsed from "Product (Label …)"
            let (mut label, mut product) = (s("label"), s("product"));
            if label.is_empty() || product.is_empty() { if let Some((p, rest)) = name.split_once(" (") { if product.is_empty() { product = p.trim().into(); } if label.is_empty() { label = rest.trim_end_matches(')').split_whitespace().next().unwrap_or("").into(); } } }
            if label.is_empty() { label = id.to_uppercase(); }
            let exec = PathBuf::from(s("exec"));
            out.push(Manifest { installed: exec.exists(), exec, args: v.get("args").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(), protocol: v.get("protocol").and_then(|x| x.as_u64()).unwrap_or(1) as u32, raster_settable: s("raster") == "auto", id, name, label, product });
        }
    }
    out
}

/// What an accepted request asked for (QCView `OutputSpec`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spec { pub w: u32, pub h: u32, pub fmt: min_present::wire::WireFormat, pub transfer: u32, pub content: u32, /// PQ's primaries: 1 Rec.2020 · 2 P3-D65
    pub primaries: u32, /// the granted `OutputLayers`
    pub layers: u32 }

pub struct Output {
    pub m: Manifest, pub enabled: bool,
    ring: Option<OutputRing>, /// the collector thread's handle on the same ring: frames are written there, the handshake here
    wire: Option<std::sync::Arc<OutWire>>, audio: Option<qcv_ring::audio::AudioRing>,
    child: Option<Child>, launched: bool, launch_due: Option<Instant>, restarts: Vec<Instant>, dying: Option<(Child, Instant)>,
    /// the sink process's state (fix 2): "", "running", "failed to start: …", "restarting (n)", "exited; not restarting …", "attached"
    pub plugin: String,
    pub spec: Option<Spec>, pub spec_gen: u64, pub present: bool, pub status: String, pub shape: String,
    req: Option<OutputRequest>, req_gen: u64, auto_class: Option<bool>, published: Option<u64>, pub_tag: u64, /// the layers drawn into the frame last submitted (stamped on its descriptor)
    drawn: u32,
    pub pinned_transfer: bool, /// a setting changed: answer the standing request again (QCView `setOutputSetting`)
    pub reanswer: bool,
}

#[derive(Default)]
pub struct OutputBus { pub outputs: Vec<Output>, last_poll: Option<Instant>, restored: bool, enc: Option<min_present::wire::WireEncoder>, tap: Vec<f32>, audio_t: i64, /// the overlay (guides / strokes): its vello layer and what it holds (size + the overlay's signature)
    layer: Option<min_ui::UiLayer>, layer_key: Option<u64>, trace: OutTrace, collector: Option<Collector> }

/// An output's frame writer: the collector's own mapping of the ring, and the spec generation that is live — a frame drawn
/// for an older answer is dropped, not committed.
struct OutWire { ring: std::sync::Mutex<OutputRing>, spec_gen: std::sync::atomic::AtomicU64 }
/// One frame on its way out: the copy to wait for, where it goes, and its descriptor AS DRAWN (QCView stamps at encode,
/// canon row 259: two frames finishing together must not share one frame index, one timecode, one timestamp).
struct Job { rb: min_present::wire::Readback, wire: std::sync::Arc<OutWire>, spec_gen: u64, desc: OutputFrameDesc, shown_at: Option<Instant>, playing: bool }
#[derive(Default)]
struct CollectStats { committed: u32, dropped: u32, readback_ms: Vec<f32>, shown_ms: Vec<f32> }
/// The COLLECTOR (canon row 261, spike S8): a thread that waits for the GPU to finish each output frame's copy and writes it
/// into the ring at once, in submit order — the tick comes round only once a refresh, and waiting for it cost one.
struct Collector { tx: std::sync::mpsc::Sender<Job>, stats: std::sync::Arc<std::sync::Mutex<CollectStats>> }
impl Collector {
    fn new(device: wgpu::Device) -> Collector {
        let (tx, rx) = std::sync::mpsc::channel::<Job>();
        let stats = std::sync::Arc::new(std::sync::Mutex::new(CollectStats::default()));
        let st = stats.clone();
        let _ = std::thread::Builder::new().name("qcv-output-collect".into()).spawn(move || {
            while let Ok(Job { rb, wire, spec_gen, mut desc, shown_at, playing }) = rx.recv() {
                let (bpr, h, at) = (rb.bpr, rb.h, rb.at);
                let mut done = false;
                let ok = rb.collect(&device, Duration::from_secs(1), &mut |bytes| {
                    if wire.spec_gen.load(std::sync::atomic::Ordering::Acquire) != spec_gen { return }   // a stale shape
                    let Ok(mut ring) = wire.ring.lock() else { return };
                    let n = bpr as usize * h as usize;
                    let Some(dst) = ring.begin_write(n as u64) else { return };
                    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, n.min(bytes.len())); }
                    desc.bytes_per_row = bpr;
                    ring.commit(desc); done = true;
                });
                if let Ok(mut s) = st.lock() {
                    if ok && done { s.committed += 1; if s.readback_ms.len() < 4096 { s.readback_ms.push(at.elapsed().as_secs_f32() * 1000.0); if let Some(t) = shown_at && playing { s.shown_ms.push(t.elapsed().as_secs_f32() * 1000.0); } } } else { s.dropped += 1; }
                }
            }
        });
        Collector { tx, stats }
    }
}

/// `QCV_TRACE_OUTPUT`: the output path's timing, a line a second — frames submitted / refused (every copy buffer busy) /
/// committed to a ring / dropped (a stale shape, a failed copy), a frame shown → its ring commit, the copy back alone, and
/// the time one publish call takes on the main thread (render + encode).
#[derive(Default)]
struct OutTrace { since: Option<Instant>, publish_ms: Vec<f32> }
fn pct(v: &mut [f32], p: f32) -> f32 { if v.is_empty() { return 0.0 } v.sort_by(|a, b| a.total_cmp(b)); v[(((v.len() - 1) as f32) * p).round() as usize] }

impl OutputBus {
    pub fn new() -> OutputBus { OutputBus { outputs: scan().into_iter().map(|m| Output { m, enabled: false, ring: None, wire: None, audio: None, child: None, launched: false, launch_due: None, restarts: vec![], dying: None, plugin: String::new(), spec: None, spec_gen: 0, present: false, status: String::new(), shape: String::new(), req: None, req_gen: 0, auto_class: None, published: None, pub_tag: 0, drawn: 0, pinned_transfer: false, reanswer: false }).collect(), ..Default::default() } }
    pub fn any_enabled(&self) -> bool { self.outputs.iter().any(|o| o.enabled) }
}

fn setting_of(st: &min_settings::Settings, id: &str, field: &str, default: f64) -> f64 { match k::output_key(id, field) { Some(key) => if matches!(field, "peakNits" | "minNits") { st.float(key) } else { st.int(key) as f64 }, None => default } }

/// QCView `fpsToRational`.
fn fps_rational(fps: f64) -> (u32, u32) {
    for (r, n) in [(23.976, 24000), (29.97, 30000), (59.94, 60000), (47.952, 48000), (119.88, 120000)] { if (fps - r).abs() < 0.01 { return (n, 1001) } }
    if fps > 0.0 && (fps - fps.round()).abs() < 1e-3 { return (fps.round() as u32, 1) }
    if fps > 0.0 { ((fps * 1000.0).round() as u32, 1000) } else { (0, 0) }
}
/// "HH:MM:SS:FF" (or ;FF) → QCView `pack_timecode`.
fn pack_tc(tc: &str) -> Option<(u32, bool)> {
    let drop = tc.contains(';'); let p: Vec<u32> = tc.split([':', ';']).filter_map(|x| x.parse().ok()).collect();
    (p.len() == 4).then(|| ((p[0] << 24) | (p[1] << 16) | (p[2] << 8) | p[3], drop))
}

impl crate::App {
    fn out_setting(&self, id: &str, field: &str, default: f64) -> f64 { setting_of(&self.settings, id, field, default) }
    /// Launch at startup the outputs that were on (QCView `restoreEnabled`).
    pub(crate) fn outputs_restore(&mut self) {
        let ids: Vec<String> = self.outputs.outputs.iter().map(|o| o.m.id.clone()).collect();
        for id in ids { if k::output_key(&id, "enabled").is_some_and(|key| self.settings.bool(key)) { self.output_start(&id); } }
    }
    /// Start (QCView `OutputBus::setEnabled(true)`): the rings, then the sink — unless one attaches by itself first (fix 1).
    pub(crate) fn output_start(&mut self, id: &str) {
        let Some(o) = self.outputs.outputs.iter_mut().find(|o| o.m.id == id) else { return };
        if o.enabled { return }
        let (Some(name), Some(aname)) = (qcvo::name_for(id), qcvo::audio_name_for(id)) else { return };
        let _ = qcv_ring::mapping::unlink(&name); let _ = qcv_ring::mapping::unlink(&aname);
        let ring = match OutputRing::create(&name, qcvo::frame_bytes(qcvo::OUTPUT_MAX_WIDTH, qcvo::OUTPUT_MAX_HEIGHT, OutputPixelFormat::Bgra8), 3, if o.m.installed { OUTPUT_LAUNCHED_BY_PRODUCER } else { 0 }) {
            Ok(r) => r, Err(e) => { log::warn!("outputs: {id}: {e}"); o.plugin = format!("ring could not be created: {e}"); return }
        };
        let audio = qcv_ring::audio::AudioRing::create(&aname, 1_000_000_000, qcv_ring::audio::DEFAULT_SLOTS, qcv_ring::audio::DEFAULT_SLOT_FRAMES, 2).ok();
        if let Some(a) = &audio { a.begin_session(AudioSession { sample_rate: 48000, channels: 2, speed: 1.0, push_frames: 1024, ..Default::default() }); }
        // the frames' writer: a second mapping of the ring, owned by the collector's jobs (QCView: the render thread commits, the GUI
        // thread shakes hands — one ring, two threads, no lock between them)
        o.wire = match OutputRing::open(&name) { Ok(r) => Some(std::sync::Arc::new(OutWire { ring: std::sync::Mutex::new(r), spec_gen: std::sync::atomic::AtomicU64::new(0) })), Err(e) => { log::warn!("outputs: {id}: the frame writer could not open the ring: {e}"); None } };
        o.ring = Some(ring); o.audio = audio; o.enabled = true; o.present = false; o.spec = None; o.req = None; o.status.clear(); o.shape.clear(); o.published = None; o.pinned_transfer = false;
        o.restarts.clear(); o.plugin.clear();
        o.launch_due = o.m.installed.then(|| Instant::now() + LAUNCH_GRACE);
        if let Some(key) = k::output_key(id, "enabled") { self.settings.set_bool(key, true); }
        log::info!("outputs: {id} started ({})", name);
        self.output_tap_sync();
    }
    /// Stop (QCView `setEnabled(false)`): the rings retired (the sink's cue), unmapped after a beat; a sink QCView launched is
    /// stopped if it is still running 3 s later.
    pub(crate) fn output_stop(&mut self, id: &str) {
        let Some(o) = self.outputs.outputs.iter_mut().find(|o| o.m.id == id) else { return };
        if !o.enabled { return }
        if let Some(r) = &o.ring { r.set_state(OutputState::Retired); }
        if let Some(a) = &o.audio { a.set_state(qcv_ring::audio::State::Retired); }
        std::thread::sleep(Duration::from_millis(30));
        o.ring = None; o.wire = None; o.audio = None; o.enabled = false; o.present = false; o.spec = None; o.launch_due = None; o.plugin.clear(); o.status.clear(); o.shape.clear();
        if let Some(c) = o.child.take() { if o.launched { o.dying = Some((c, Instant::now() + Duration::from_secs(3))); } }
        o.launched = false;
        if let Some(key) = k::output_key(id, "enabled") { self.settings.set_bool(key, false); }
        log::info!("outputs: {id} stopped");
        self.output_tap_sync();
    }
    /// The audio tap on while any output is on (the file player's or the live feed's player).
    pub(crate) fn output_tap_sync(&mut self) {
        let on = self.outputs.any_enabled();
        if let Some(p) = self.player.as_mut() { p.audio.set_output_tap(on); }
        if let Some(au) = self.live.as_mut().and_then(|l| l.audio.as_mut()) { au.set_output_tap(on); }
        // dual view (row 258 step 6): each side's player — a file's, or a live host's
        if let Some(d) = self.dual.as_mut() { d.for_each_audio(|au| au.set_output_tap(on)); }
        for l in self.side_live.iter_mut().flatten() { if let Some(au) = l.audio.as_mut() { au.set_output_tap(on); } }
    }
    fn launch(o: &mut Output) {
        let mut c = Command::new(&o.m.exec); c.args(&o.m.args).env("QCVO_ID", &o.m.id);
        match c.spawn() { Ok(ch) => { log::info!("outputs: {} launched {} (pid {})", o.m.id, o.m.exec.display(), ch.id()); o.child = Some(ch); o.launched = true; o.plugin = "running".into(); }
            Err(e) => { log::warn!("outputs: {}: launch {}: {e}", o.m.id, o.m.exec.display()); o.plugin = format!("failed to start: {e}"); } }
    }
    /// QCView `OutputBus::poll` (4 Hz) + the sinks' lifetimes + the publish.
    pub(crate) fn outputs_tick(&mut self, now: Instant) {
        // the dying sinks: a launched one still running 3 s after Stop is stopped
        for o in &mut self.outputs.outputs { if let Some((c, due)) = o.dying.as_mut() && now >= *due { let _ = c.kill(); let _ = c.wait(); o.dying = None; } else if let Some((c, _)) = o.dying.as_mut() && matches!(c.try_wait(), Ok(Some(_))) { o.dying = None; } }
        if !self.outputs.restored && self.gpu.is_some() { self.outputs.restored = true; self.outputs_restore(); }
        if !self.outputs.any_enabled() { return }
        self.outputs_audio();
        if self.outputs.last_poll.is_some_and(|t| now.duration_since(t) < POLL) { return }
        self.outputs.last_poll = Some(now);
        self.output_tap_sync();   // the player / live feed may have changed since
        let src = self.output_source_size();
        let wide = src.0 > 1920 || src.1 > 1080;
        let (dual, has_b) = (self.dual.is_some(), self.dual.as_ref().is_some_and(|d| d.present(qcv_player::Side::B)));
        let layers_on = (if self.settings.bool(k::OUTPUT_GUIDES) { v::LAYER_GUIDES } else { 0 }) | (if self.settings.bool(k::OUTPUT_NOTES) { v::LAYER_ANNOTATIONS } else { 0 });
        let ids: Vec<String> = self.outputs.outputs.iter().filter(|o| o.enabled).map(|o| o.m.id.clone()).collect();
        let mut changed = false;
        for id in ids {
            let cfg = (self.out_setting(&id, "raster", 0.0) as u32, self.out_setting(&id, "transfer", 1.0) as u32, self.out_setting(&id, "primaries", 1.0) as u32, self.out_setting(&id, "peakNits", 1000.0) as f32, self.out_setting(&id, "minNits", 0.005) as f32, self.out_setting(&id, "maxCll", 1000.0) as u32, self.out_setting(&id, "maxFall", 400.0) as u32);
            let Some(o) = self.outputs.outputs.iter_mut().find(|o| o.m.id == id) else { continue };
            // the sink's process: launch when due (unless one attached by itself), restart on a crash (QCView: 500 ms × n, 3 a minute)
            let qnow = qcvo::now_ns();
            let present = o.ring.as_ref().is_some_and(|r| r.consumer_present(qnow));
            if let Some(due) = o.launch_due && now >= due { o.launch_due = None; if present { o.plugin = "attached".into(); log::info!("outputs: {id}: a sink is already attached; not launching another"); } else { Self::launch(o); } }
            if let Some(c) = o.child.as_mut() && let Ok(Some(st)) = c.try_wait() {
                o.child = None; o.restarts.retain(|t| now.duration_since(*t) < Duration::from_secs(60));
                if o.restarts.len() >= 3 { o.plugin = "exited; not restarting (three in a minute)".into(); log::warn!("outputs: {id}: sink exited ({st}); not restarting"); }
                else { o.restarts.push(now); let n = o.restarts.len() as u32; o.launch_due = Some(now + Duration::from_millis(500) * n); o.plugin = format!("restarting ({n})"); log::warn!("outputs: {id}: sink exited ({st}); restarting ({n})"); }
            }
            let Some(ring) = o.ring.as_mut() else { continue };
            // a new request → the answer; an auto raster re-answered when the source changes class
            let mut answer = false;
            if let Some(r) = ring.read_request() { o.req = Some(r); o.req_gen = ring.request_generation(); answer = true; }
            if let Some(r) = o.req && r.width == 0 && r.height == 0 && o.spec.is_some() && o.auto_class != Some(wide) { answer = true; }
            if std::mem::take(&mut o.reanswer) && o.req.is_some() { answer = true; }
            if answer && let Some(r) = o.req {
                o.auto_class = Some(wide);
                let (reply, spec, hdr) = answer_request(&r, cfg, src, dual, has_b, layers_on);
                ring.write_reply(reply, o.req_gen);
                o.pinned_transfer = r.transfer != 0;
                let msg = String::from_utf8_lossy(&reply.message[..reply.message.iter().position(|&c| c == 0).unwrap_or(reply.message.len())]).into_owned();
                log::info!("OutputBus: {id}: request {} ({}x{} fmt {} transfer {} content {} layers {:#x}) -> {} \"{msg}\" (layers {:#x})", o.req_gen, r.width, r.height, r.pixel_format, r.transfer, r.content, r.layers, if spec.is_some() { "accepted" } else { "refused" }, reply.layers);
                if let Some(h) = hdr { ring.write_hdr_info(h); }
                o.shape = msg; o.spec = spec; o.spec_gen += 1; o.published = None; changed = true;
            }
            let was = o.present; o.present = present;
            if present != was { changed = true; o.published = None; if present && o.plugin.is_empty() { o.plugin = "attached".into(); } }
            if let Some(s) = ring.read_status() { if s != o.status { o.status = s; changed = true; } }
        }
        if changed && let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; }
    }
    /// Quitting: the rings retired and a sink QCView launched stopped now (QCView: the sink sees Retired and quits).
    pub(crate) fn output_stop_quitting(&mut self, id: &str) {
        self.output_stop(id);
        if let Some(key) = k::output_key(id, "enabled") { self.settings.set_bool(key, true); }   // still on at the next launch
        if let Some(o) = self.outputs.outputs.iter_mut().find(|o| o.m.id == id) && let Some((mut c, _)) = o.dying.take() { std::thread::sleep(Duration::from_millis(200)); if !matches!(c.try_wait(), Ok(Some(_))) { let _ = c.kill(); } }
    }
    /// The Output section's commands.
    pub(crate) fn output_cmd(&mut self, c: crate::output_panel::Cmd) {
        use crate::output_panel::Cmd;
        let set = |s: &mut min_settings::Settings, id: &str, f: &str, v: i64| if let Some(key) = k::output_key(id, f) { s.set_int(key, v); };
        match c.clone() {
            Cmd::Start(id) => self.output_start(&id),
            Cmd::Stop(id) => self.output_stop(&id),
            Cmd::FeedMute(on) => { self.settings.set_bool(k::OUTPUT_FEED_MUTED, on); }
            // QCView `layersChanged`: every standing request answered again with the new grant
            Cmd::Guides(on) | Cmd::Notes(on) => { self.settings.set_bool(if matches!(c, Cmd::Guides(_)) { k::OUTPUT_GUIDES } else { k::OUTPUT_NOTES }, on); for o in &mut self.outputs.outputs { o.reanswer = true; } self.outputs.last_poll = None; }
            Cmd::Transfer(id, v) => { set(&mut self.settings, &id, "transfer", v); self.output_reanswer(&id); }
            Cmd::Raster(id, v) => { set(&mut self.settings, &id, "raster", v); self.output_reanswer(&id); }
            Cmd::Primaries(id, v) => { set(&mut self.settings, &id, "primaries", v); self.output_reanswer(&id); }
            Cmd::Peak(id, v) => { if let Some(key) = k::output_key(&id, "peakNits") { self.settings.set_float(key, v as f64); } self.output_reanswer(&id); }
            Cmd::Cll(id, v) => { set(&mut self.settings, &id, "maxCll", v); self.output_reanswer(&id); }
            Cmd::Fall(id, v) => { set(&mut self.settings, &id, "maxFall", v); self.output_reanswer(&id); }
        }
        if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; }
    }
    fn output_reanswer(&mut self, id: &str) { if let Some(o) = self.outputs.outputs.iter_mut().find(|o| o.m.id == id) { o.reanswer = true; } self.outputs.last_poll = None; }
}

/// The cards (the Output section): the dot + its reason, the shape and the sink's report, the settings it lets QCView choose.
pub(crate) fn views(bus: &OutputBus, st: &min_settings::Settings) -> Vec<crate::output_panel::CardView> {
    let tr = |s: &str| min_i18n::trc("LeftRail", s).to_string();
    bus.outputs.iter().map(|o| {
        let g = |f: &str, d: f64| setting_of(st, &o.m.id, f, d);
        let (transfer, raster, primaries) = (g("transfer", 1.0) as usize, g("raster", 0.0) as usize, g("primaries", 1.0) as usize);
        let failed = o.plugin.starts_with("failed") || o.plugin.starts_with("exited") || o.plugin.starts_with("ring");
        let (dot, reason, shape, status) = if !o.enabled {
            if o.m.installed { (0, String::new(), String::new(), String::new()) } else { (0, tr("not installed"), tr("not installed"), String::new()) }
        } else if o.present && let Some(sp) = o.spec {
            let f = if sp.fmt == min_present::wire::WireFormat::Rgb10 { "RGB10" } else { "BGRA8" };
            let tn = match sp.transfer { v::TRANSFER_PQ => "PQ Rec.2100", v::TRANSFER_HLG => "HLG Rec.2100", _ => "SDR Rec.709" };
            (1, tr("live"), format!("{}×{}  ·  {f}  ·  {tn}", sp.w, sp.h), o.status.clone())
        } else if o.present && o.req.is_some() { (3, format!("{}: {}", tr("refused"), o.shape), o.shape.clone(), o.status.clone()) }
        else if failed { (3, o.plugin.clone(), o.plugin.clone(), String::new()) }
        else { let w = if o.plugin.starts_with("restarting") { o.plugin.clone() } else { tr("waiting for the sink") }; (2, w.clone(), w, String::new()) };
        crate::output_panel::CardView { id: o.m.id.clone(), label: o.m.label.clone(), product: o.m.product.clone(), dot, reason, shape, status, enabled: o.enabled, installed: o.m.installed,
            transfer: transfer.saturating_sub(1).min(2), raster: raster.min(2), primaries: primaries.saturating_sub(1).min(1),
            peak: g("peakNits", 1000.0).round() as i64, cll: g("maxCll", 1000.0) as i64, fall: g("maxFall", 400.0) as i64,
            show_transfer: !o.pinned_transfer, show_raster: o.m.raster_settable }
    }).collect()
}

impl crate::App {
    fn output_source_size(&self) -> (u32, u32) {
        if let Some(l) = self.live.as_ref() { let f = l.feed.facts(); return (f.width, f.height) }
        if let Some(p) = self.player.as_ref() { let i = p.info(); return (i.width, i.height) }
        // dual view: the larger side — a file's stream, or a live side's raster (row 258 step 6; it answered 0 × 0 in dual)
        if let Some(d) = self.dual.as_ref() {
            let mut best = (0u32, 0u32);
            for (i, side) in [qcv_player::Side::A, qcv_player::Side::B].into_iter().enumerate() {
                let s = match (d.side(side), self.side_live[i].as_ref()) { (Some(t), _) => { let inf = t.info(); (inf.width, inf.height) } (None, Some(l)) => { let f = l.feed.facts(); (f.width, f.height) } _ => (0, 0) };
                if s.0 as u64 * s.1 as u64 > best.0 as u64 * best.1 as u64 { best = s; }
            }
            return best
        }
        (0, 0)
    }
    /// The change stamp (QCView `OutputStamp`): a new frame, the colour, the aids, the geometry, Review, a dual change, a new spec.
    fn output_stamp(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        if let Some(g) = self.gpu.as_ref() {
            let e = &g.presenter.engine; e.frames_shown.hash(&mut h);
            let a = g.drawer.aids; a.exposure.to_bits().hash(&mut h); a.gamma.to_bits().hash(&mut h); a.channel.hash(&mut h);
            format!("{:?}", e.geometry()).hash(&mut h);
            // in dual the pictures are shown on the SIDE engines — the main engine's count stands still (canon row 255)
            if let Some(d) = e.dual() { d.a.frames_shown.hash(&mut h); d.b.frames_shown.hash(&mut h); format!("{:?}", d.mode).hash(&mut h); d.seam.to_bits().hash(&mut h); }
        }
        self.color_bound.hash(&mut h); self.loaded_item.hash(&mut h);
        // the chain's SETTINGS, not only which engine is bound (a slider drag keeps the engine; QCView's stamp holds the chain)
        self.settings.bool(k::COLOR_ENABLED).hash(&mut h);
        format!("{:?}", self.chain_of(&self.slot_item(0)).1.chain).hash(&mut h);
        if self.dual.is_some() { format!("{:?}", self.chain_of(&self.slot_item(1)).1.chain).hash(&mut h); }
        self.live.as_ref().and_then(|l| l.review.as_ref()).map(|r| (&r.label, &r.note, r.stored)).hash(&mut h);
        self.overlay_sig().hash(&mut h);
        h.finish()
    }
    /// The note whose strokes the viewport is drawing now (the Notes panel open, single view) — what an output may carry.
    fn overlay_note(&self) -> Option<crate::annotate::Cur> {
        if !self.annot.active || self.dual.is_some() { return None }
        self.current_note()
    }
    /// What the outputs' overlay depends on (row 238): the guide and its look, the screen's pictures (the line width's
    /// reference), the ink the viewport shows. Granted layers are per output (the spec).
    fn overlay_sig(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let st = &self.settings;
        st.bool(k::OUTPUT_GUIDES).hash(&mut h); st.bool(k::OUTPUT_NOTES).hash(&mut h);
        st.string(k::SAFETY_GUIDE).hash(&mut h); st.string(k::SAFETY_COLOR).hash(&mut h); st.float(k::SAFETY_WIDTH).to_bits().hash(&mut h); st.float(k::SAFETY_OPACITY).to_bits().hash(&mut h);
        if let Some(g) = self.gpu.as_ref() {
            g.guide.is_some().hash(&mut h); g.window.scale_factor().to_bits().hash(&mut h);
            for c in [v::CONTENT_COMPOSITE, v::CONTENT_SIDE_B] { for p in screen_places(&g.presenter.engine, c) { for x in p.0 { x.to_bits().hash(&mut h); } } }
        }
        if let Some(cur) = self.overlay_note() { self.annot.ink_sig(&cur).hash(&mut h); }
        h.finish()
    }
    /// Publish-on-change (QCView: no clock of its own): every accepted, present output whose stamp moved gets a frame, drawn
    /// and encoded here and handed to the collector. Called from the tick AFTER the frame is shown (canon row 261: at the top
    /// of the tick a frame went out one refresh after it was shown).
    pub(crate) fn outputs_publish(&mut self) {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if !*ON.get_or_init(|| std::env::var_os("QCV_TRACE_OUTPUT").is_some()) { return self.outputs_publish_now() }
        let t0 = Instant::now();
        self.outputs.trace.since.get_or_insert(t0);
        if let Some(e) = self.outputs.enc.as_mut() { e.trace = true; }
        self.outputs_publish_now();
        let tr = &mut self.outputs.trace; tr.publish_ms.push(t0.elapsed().as_secs_f32() * 1000.0);
        if tr.since.is_some_and(|s| s.elapsed() >= Duration::from_secs(1)) {
            tr.since = Some(Instant::now());
            let st = self.outputs.enc.as_mut().map(|e| std::mem::take(&mut e.stats)).unwrap_or_default();
            let mut c = self.outputs.collector.as_ref().and_then(|c| c.stats.lock().ok().map(|mut s| std::mem::take(&mut *s))).unwrap_or_default();
            let mut p = std::mem::take(&mut tr.publish_ms);
            // through the log, not a print: on Windows the windowed app's prints go to the launching console, never to a file
            log::info!("output trace: submitted {} busy {} committed {} dropped {} · shown→ring ms p50 {:.1} p95 {:.1} max {:.1} · copy back ms p50 {:.1} p95 {:.1} max {:.1} · main thread ms p50 {:.2} p95 {:.2} max {:.2}",
                st.submitted, st.busy, c.committed, c.dropped, pct(&mut c.shown_ms, 0.5), pct(&mut c.shown_ms, 0.95), pct(&mut c.shown_ms, 1.0), pct(&mut c.readback_ms, 0.5), pct(&mut c.readback_ms, 0.95), pct(&mut c.readback_ms, 1.0), pct(&mut p, 0.5), pct(&mut p, 0.95), pct(&mut p, 1.0));
        }
    }
    fn outputs_publish_now(&mut self) {
        let stamp = self.output_stamp();
        let (ov_sig, note) = (self.overlay_sig(), self.overlay_note().filter(|c| self.annot.has_ink(c)));
        let (gw, gcol) = { let st = &self.settings; let c = min_ui::color_picker::parse_hex(&st.string(k::SAFETY_COLOR)).unwrap_or([1.0, 1.0, 0.0]); (st.float(k::SAFETY_WIDTH), peniko::Color::new([c[0], c[1], c[2], (st.float(k::SAFETY_OPACITY) as f32).clamp(0.0, 1.0)])) };
        let (playing, frame, fps, tc) = self.output_transport();
        let dual_b = self.dual.as_ref().is_some_and(|d| d.present(qcv_player::Side::B));
        let Some(g) = self.gpu.as_mut() else { return };
        let dev = g.presenter.engine.device.clone(); let q = g.presenter.engine.queue.clone();
        let enc = self.outputs.enc.get_or_insert_with(|| min_present::wire::WireEncoder::new(&dev));
        let collector = self.outputs.collector.get_or_insert_with(|| Collector::new(dev.clone()));
        // the generation the collector may commit for (a frame drawn for an older answer is dropped there)
        for o in &self.outputs.outputs { if let Some(w) = &o.wire { w.spec_gen.store(o.spec_gen, std::sync::atomic::Ordering::Release); } }
        // the new frames
        for i in 0..self.outputs.outputs.len() {
            let o = &self.outputs.outputs[i];
            let (true, true, Some(spec)) = (o.enabled, o.present, o.spec) else { continue };
            if o.published == Some(stamp) { continue }
            // the canvas in linear light; the wire encoder encodes it: SDR · PQ in the output's primaries · HLG (1.0 = 100 nits)
            let encoding = match spec.transfer { v::TRANSFER_PQ => min_present::wire::WireEncoding::Pq { p3: spec.primaries == 2, white: 100.0 }, v::TRANSFER_HLG => min_present::wire::WireEncoding::Hlg { white: 100.0 }, _ => min_present::wire::WireEncoding::Sdr };
            let lin = min_present::engine::OutMode::Linear;
            // content: dual's composite, or one side alone (QCView `OutputContent::SideA / SideB`); single view: the picture
            // the screen's pictures first (the overlay's reference), then the canvas and where its pictures landed
            let screen = screen_places(&g.presenter.engine, spec.content);
            let screen_geo = crate::annot_geo(&g.presenter.engine);
            let canvas = match (spec.content, g.presenter.engine.dual_mut()) {
                (v::CONTENT_SIDE_A, Some(d)) => d.a.render_output_placed(spec.w, spec.h, lin, 100.0),
                (v::CONTENT_SIDE_B, Some(d)) => d.b.render_output_placed(spec.w, spec.h, lin, 100.0),
                _ => g.presenter.engine.render_output_placed(spec.w, spec.h, lin, 100.0),
            };
            let Ok((canvas, places)) = canvas else { continue };
            // the layers (row 238): the guide over each picture, the note's ink over the one picture
            let guide = (spec.layers & v::LAYER_GUIDES != 0).then(|| g.guide.as_ref().map(|x| &x.1)).flatten().filter(|_| !places.is_empty());
            let ink = (spec.layers & v::LAYER_ANNOTATIONS != 0).then_some(note.as_ref()).flatten().zip(screen_geo.as_ref()).filter(|_| places.len() == 1);
            let drawn = (if guide.is_some() { v::LAYER_GUIDES } else { 0 }) | (if ink.is_some() { v::LAYER_ANNOTATIONS } else { 0 });
            let mut overlay = None;
            if drawn != 0 {
                if self.outputs.layer.is_none() { self.outputs.layer = min_ui::UiLayer::new(&dev, spec.w, spec.h).ok(); self.outputs.layer_key = None; }
                if let Some(layer) = self.outputs.layer.as_mut() {
                    use std::hash::{Hash, Hasher};
                    let mut hk = std::collections::hash_map::DefaultHasher::new();
                    (ov_sig, spec.w, spec.h, drawn, spec.content).hash(&mut hk); for p in &places { for x in p.0 { x.to_bits().hash(&mut hk); } }
                    let key = hk.finish();
                    if self.outputs.layer_key != Some(key) {
                        layer.resize(&dev, spec.w, spec.h);
                        let scene = layer.scene_mut(); scene.reset();
                        if let Some((cur, sg)) = ink { let r = places[0].0; self.annot.paint_ink(scene, cur, &crate::annotate::Geo { rect: r, px_per_src: sg.px_per_src * r[2] / sg.rect[2].max(1.0) }); }
                        if let Some(gd) = guide { for (n, (rect, src, quarters)) in places.iter().enumerate() {
                            crate::guides::paint(scene, gd, crate::guides::transform(gd.view, *src, *rect, *quarters), guide_width(gw, g.window.scale_factor(), rect, screen.get(n)), gcol);
                        } }
                        layer.render(&dev, &q); self.outputs.layer_key = Some(key);
                    }
                    overlay = Some(layer.view());
                }
            }
            let o = &mut self.outputs.outputs[i];
            o.pub_tag += 1;
            let Some(wire) = o.wire.clone() else { continue };
            if enc.submit(&dev, &q, &canvas, overlay, spec.fmt, encoding, ((i as u64) << 32) | (o.spec_gen & 0xffff_ffff)) {
                o.published = Some(stamp); o.drawn = if overlay.is_some() { drawn } else { 0 };
                // the frame's descriptor as drawn, and off to the collector
                let (fn_, fd) = fps_rational(fps);
                let mut flags = (if spec.content == v::CONTENT_SIDE_B { 0 } else { v::FRAME_HAS_A }) | (if dual_b && spec.content != v::CONTENT_SIDE_A { v::FRAME_HAS_B } else { 0 }) | if playing { 0 } else { v::FRAME_PAUSED };
                let tcp = tc.as_deref().and_then(pack_tc);
                if let Some((_, drop)) = tcp { flags |= v::FRAME_TIMECODE_VALID | if drop { v::FRAME_DROP_FRAME } else { 0 }; }
                let desc = OutputFrameDesc { width: spec.w, height: spec.h, bytes_per_row: 0, pixel_format: match spec.fmt { min_present::wire::WireFormat::Bgra8 => OutputPixelFormat::Bgra8 as u32, min_present::wire::WireFormat::Rgb10 => OutputPixelFormat::Rgb10 as u32 }, transfer: spec.transfer, content: spec.content, layers: o.drawn, flags, presentation_ns: qcvo::now_ns(), source_frame_index: frame, source_fps_num: fn_, source_fps_den: fd, timecode: tcp.map(|t| t.0).unwrap_or(0), _pad0: 0 };
                if let Some(rb) = enc.detach_last() { let _ = collector.tx.send(Job { rb, wire, spec_gen: o.spec_gen, desc, shown_at: g.presenter.engine.last_shown_at, playing }); }
            }
        }
    }
    /// The frame's transport facts: playing, the source frame, its fps, the timecode (QCView fills none of these — fix 7).
    fn output_transport(&self) -> (bool, i64, f64, Option<String>) {
        if let Some(l) = self.live.as_ref() { let tc = l.feed.host_timecode(); return (l.review.is_none(), l.feed.facts().frames as i64, l.feed.host_fps().unwrap_or(0.0), (!tc.is_empty() && !tc.contains('.')).then_some(tc)) }
        if let Some(p) = self.player.as_ref() {
            let i = p.timecode_info(); let fmt = qcv_meta::TimecodeFormatter::new(i.frame_rate.0, i.frame_rate.1, i.drop_frame);
            let start = i.source_timecode.as_deref().map(|t| fmt.parse(t).max(0)).unwrap_or(0);
            let f = p.current_frame();
            return (p.is_playing(), f as i64, p.rate().as_f64(), fmt.is_valid().then(|| fmt.format(start + f)))
        }
        // dual view (row 258 step 6; it answered Paused, frame 0, no rate): the master's transport — or, with no clocked
        // side, the first live side's feed (never Paused: its frames keep coming). No timecode: two sources, two clocks.
        if let Some(d) = self.dual.as_ref() {
            if d.has_clocked_side() { return (d.is_playing(), d.current_frame() as i64, d.rate().as_f64(), None) }
            if let Some(l) = self.side_live.iter().flatten().next() { return (true, l.feed.facts().frames as i64, l.feed.host_fps().unwrap_or(0.0), None) }
        }
        (false, 0, 0.0, None)
    }
    /// The mixed audio → every present output's audio ring, 1024-frame planar packets (QCView `OutputBus::tapAudio`); the master
    /// mute or the feed mute → zeros flagged muted.
    fn outputs_audio(&mut self) {
        let mut buf = std::mem::take(&mut self.outputs.tap);
        if let Some(p) = self.player.as_mut() { p.audio.drain_tap(&mut buf); }
        if let Some(au) = self.live.as_mut().and_then(|l| l.audio.as_mut()) { au.drain_tap(&mut buf); }
        // dual view (row 258 step 6): every side's tap is drained (none may grow), and the output carries the first HEARD
        // side — A when both are unmuted. QCView's mixer summed the heard sides; two taps filled by two device callbacks do
        // not line up sample for sample here, so the sum is not attempted.
        if self.dual.is_some() {
            let mut taken = false;
            for (i, side) in [qcv_player::Side::A, qcv_player::Side::B].into_iter().enumerate() {
                let mut one = vec![];
                if let Some(t) = self.dual.as_mut().and_then(|d| d.side_mut(side)) { t.audio.drain_tap(&mut one); }
                if let Some(au) = self.side_live[i].as_mut().and_then(|l| l.audio.as_mut()) { au.drain_tap(&mut one); }
                let heard = self.dual.as_ref().is_some_and(|d| !d.side_muted(side));
                if heard && !taken && !one.is_empty() { buf.extend_from_slice(&one); taken = true; }
            }
        }
        let muted = self.settings.bool(k::MUTED) || self.settings.bool(k::OUTPUT_FEED_MUTED);
        let n = buf.len() / 2 / 1024 * 1024;
        if n == 0 { self.outputs.tap = buf; return }
        let base = qcvo::now_ns();
        for c in 0..n / 1024 {
            let chunk = &buf[c * 2048..(c + 1) * 2048];
            let (l, r): (Vec<f32>, Vec<f32>) = if muted { (vec![0.0; 1024], vec![0.0; 1024]) } else { (chunk.iter().step_by(2).copied().collect(), chunk.iter().skip(1).step_by(2).copied().collect()) };
            let t = base + (c as i64) * 1_000_000_000 * 1024 / 48000;
            for o in &mut self.outputs.outputs { if o.enabled && o.present && let Some(a) = o.audio.as_mut() { a.push(&[&l, &r], 48000, 1024, t, if muted { qcv_ring::audio::PACKET_MUTED } else { 0 }); } }
        }
        buf.drain(..n * 2);
        self.outputs.tap = buf;
        let _ = self.outputs.audio_t;
    }
}

/// The screen's pictures an output's content shows, in the order the output pass places them (A, then B side by side; one
/// side alone = that side) — the reference the overlay's line width scales from.
fn screen_places(e: &min_present::engine::Engine, content: u32) -> Vec<min_present::engine::Placement> {
    match e.dual() {
        Some(d) => match content {
            v::CONTENT_SIDE_A => d.a.picture_placement().into_iter().collect(),
            v::CONTENT_SIDE_B => d.b.picture_placement().into_iter().collect(),
            _ => { let mut p = vec![d.a.picture_placement()]; if d.mode == min_present::dual::CompareMode::SideBySide { p.push(d.b.picture_placement()); } p.into_iter().flatten().collect() }
        },
        None => e.picture_placement().into_iter().collect(),
    }
}
/// The guide's line width on a target (an output's raster, a screenshot): the screen's width (`setting` × the window's
/// scale) × the target picture's height over the screen picture's, so the line keeps the proportion the viewport shows
/// (the user 2026-11-09). With no screen picture: the setting at a 1080-line picture, scaled with the target.
pub(crate) fn guide_width(setting: f64, scale: f64, rect: &[f64; 4], screen: Option<&min_present::engine::Placement>) -> f64 {
    match screen { Some(s) if s.0[3] >= 1.0 => setting * scale * rect[3] / s.0[3], _ => setting * (rect[3] / 1080.0).max(1.0) }
}

/// QCView `OutputBus::answer`'s rules and words (the protocol's English: the sink shows it). HLG and per-side content arrive with
/// the AJA pass (the port's output pass encodes SDR and PQ today).
fn answer_request(r: &OutputRequest, cfg: (u32, u32, u32, f32, f32, u32, u32), src: (u32, u32), dual: bool, has_b: bool, layers_on: u32) -> (OutputReply, Option<Spec>, Option<OutputHdrInfo>) {
    let (raster, cfg_transfer, primaries, peak, min, cll, fall) = cfg;
    let mut reply = OutputReply { accepted: 0, refusal: 0, width: 0, height: 0, pixel_format: 0, transfer: 0, content: 0, layers: 0, message: [0; OUTPUT_MESSAGE_BYTES] };
    let msg = |m: &mut [u8; OUTPUT_MESSAGE_BYTES], s: &str| { let n = s.len().min(OUTPUT_MESSAGE_BYTES - 1); m[..n].copy_from_slice(&s.as_bytes()[..n]); };
    let refuse = |mut rep: OutputReply, why: u32, s: &str| { rep.refusal = why; msg(&mut rep.message, s); (rep, None, None) };
    let transfer = if r.transfer != 0 { r.transfer } else { cfg_transfer };
    let (w, h) = if r.width == 0 && r.height == 0 { match raster { 1 => (1920, 1080), 2 => (3840, 2160), _ => if src.0 > 1920 || src.1 > 1080 { (3840, 2160) } else { (1920, 1080) } } } else { (r.width, r.height) };
    let fmt = match r.pixel_format { 1 => Some(min_present::wire::WireFormat::Bgra8), 3 => Some(min_present::wire::WireFormat::Rgb10), _ => None };
    if r.protocol_version != 1 { return refuse(reply, v::REFUSE_VERSION, "this QCView speaks output protocol 1") }
    if w == 0 || h == 0 || w > qcvo::OUTPUT_MAX_WIDTH || h > qcvo::OUTPUT_MAX_HEIGHT { return refuse(reply, v::REFUSE_TOO_LARGE, "above the 3840x2160 canvas cap") }
    let Some(fmt) = fmt else { return refuse(reply, v::REFUSE_FORMAT, "BGRA8 or RGB10; RGBA16F comes with the HDR transfers") };
    if transfer > v::TRANSFER_HLG { return refuse(reply, v::REFUSE_TRANSFER, "unknown transfer") }
    if r.fit != 0 && r.fit != v::FIT_LETTERBOX { return refuse(reply, v::REFUSE_FORMAT, "letterbox fit only for now") }
    if r.content > v::CONTENT_SIDE_B { return refuse(reply, v::REFUSE_CONTENT, "content must be composite, A or B") }
    if r.content == v::CONTENT_SIDE_B && !(dual && has_b) { return refuse(reply, v::REFUSE_CONTENT, "no B side loaded") }   // fix 6
    let content = if r.content == 0 { v::CONTENT_COMPOSITE } else { r.content };
    reply.accepted = 1; reply.width = w; reply.height = h; reply.pixel_format = r.pixel_format; reply.transfer = transfer; reply.content = content; reply.layers = r.layers & layers_on;   // QCView: the request ∧ the two chips
    let fname = if fmt == min_present::wire::WireFormat::Rgb10 { "RGB10" } else { "BGRA8" };
    let tname = match transfer { v::TRANSFER_PQ => "PQ Rec.2100", v::TRANSFER_HLG => "HLG Rec.2100", _ => "SDR Rec.709" };
    msg(&mut reply.message, &format!("{w}x{h} {fname} {tname} from QCView"));
    let hdr = if transfer == v::TRANSFER_SDR { OutputHdrInfo { transfer, ..Default::default() } } else { OutputHdrInfo { transfer, primaries, max_mastering_nits: peak, min_mastering_nits: min, max_cll: cll, max_fall: fall, _pad0: [0; 2] } };
    (reply, Some(Spec { w, h, fmt, transfer, content, primaries: if transfer == v::TRANSFER_HLG { 1 } else { primaries }, layers: reply.layers }), Some(hdr))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn req(w: u32, h: u32, fmt: u32, transfer: u32, content: u32) -> OutputRequest { OutputRequest { protocol_version: 1, width: w, height: h, pixel_format: fmt, transfer, fit: 1, content, ..Default::default() } }
    const CFG: (u32, u32, u32, f32, f32, u32, u32) = (0, 1, 1, 1000.0, 0.005, 1000, 400);
    #[test]
    fn the_answers_are_qcviews() {
        let ok = |r: OutputRequest, src| answer_request(&r, CFG, src, false, false, 0);
        // the layers: the request ∧ the toggles (QCView), never more than asked
        let lay = |want: u32, on: u32| { let mut r = req(1920, 1080, 1, 1, 1); r.layers = want; let (rep, spec, _) = answer_request(&r, CFG, (0, 0), false, false, on); (rep.layers, spec.unwrap().layers) };
        assert_eq!(lay(3, 3), (3, 3)); assert_eq!(lay(3, 1), (1, 1)); assert_eq!(lay(0, 3), (0, 0)); assert_eq!(lay(2, 1), (0, 0));
        let (rep, spec, _) = ok(req(1920, 1080, 1, 1, 1), (3840, 2160)); assert_eq!(rep.accepted, 1); assert_eq!(spec.unwrap().w, 1920);
        assert!(String::from_utf8_lossy(&rep.message).starts_with("1920x1080 BGRA8 SDR Rec.709 from QCView"));
        let (_, spec, _) = ok(req(0, 0, 3, 0, 0), (3840, 2160)); assert_eq!((spec.unwrap().w, spec.unwrap().transfer), (3840, 1), "auto raster: UHD for a UHD source; the setting's transfer");
        let (_, spec, _) = ok(req(0, 0, 3, 0, 0), (1920, 1080)); assert_eq!(spec.unwrap().w, 1920);
        assert_eq!(ok(req(1920, 1080, 2, 1, 1), (0, 0)).0.refusal, v::REFUSE_FORMAT);
        assert_eq!(ok(req(7680, 4320, 1, 1, 1), (0, 0)).0.refusal, v::REFUSE_TOO_LARGE);
        assert_eq!(ok(req(1920, 1080, 1, 1, 3), (0, 0)).0.refusal, v::REFUSE_CONTENT, "fix 6: no B side");
        let mut r = req(1920, 1080, 1, 1, 1); r.protocol_version = 2; assert_eq!(ok(r, (0, 0)).0.refusal, v::REFUSE_VERSION);
        let (_, _, hdr) = ok(req(1920, 1080, 3, 2, 1), (0, 0)); assert_eq!((hdr.unwrap().max_cll, hdr.unwrap().primaries), (1000, 1));
    }
    #[test]
    fn fps_and_timecode_pack_as_qcview() {
        assert_eq!(fps_rational(23.976), (24000, 1001)); assert_eq!(fps_rational(25.0), (25, 1)); assert_eq!(fps_rational(12.5), (12500, 1000));
        assert_eq!(pack_tc("01:02:03:04"), Some((0x01020304, false))); assert_eq!(pack_tc("01:00:00;02").map(|t| t.1), Some(true));
    }
}
