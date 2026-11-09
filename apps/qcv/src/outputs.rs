//! The OUTPUTS (QCView `window/output_bus.cpp`, canon row 227 step 3): QCView links no NDI / AJA SDK — each output is a SINK
//! app (QCBridgeN, QCBridgeA) registered by a manifest JSON in `…/QCView/Outputs`; Start creates the output's QCVO frame ring
//! + QCAA audio ring and launches the sink; the sink attaches, asks for a raster / format / transfer; QCView answers (accept or
//! refuse, QCView's rules and words) and publishes the composite at that raster whenever the picture changes (the sinks repeat
//! the last frame); the mixed audio before Listen / mute / volume goes out beside it. Stop retires the rings (the sink's cue)
//! and stops a sink it launched.
//!
//! Fixes over QCView (row 227 step 3, the user's go): Start does not launch a second sink when one is already attached; a sink
//! that fails to start or keeps crashing says so (QCView showed "starting" forever); a B side that isn't there is refused (QCView
//! accepted it and sent nothing); frames carry the timecode and the Paused flag (AJA's RP188, canon row 107); no guide / stroke
//! layers (row 225).
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
pub struct Spec { pub w: u32, pub h: u32, pub fmt: min_present::wire::WireFormat, pub transfer: u32, pub content: u32 }

pub struct Output {
    pub m: Manifest, pub enabled: bool,
    ring: Option<OutputRing>, audio: Option<qcv_ring::audio::AudioRing>,
    child: Option<Child>, launched: bool, launch_due: Option<Instant>, restarts: Vec<Instant>, dying: Option<(Child, Instant)>,
    /// the sink process's state (fix 2): "", "running", "failed to start: …", "restarting (n)", "exited; not restarting …", "attached"
    pub plugin: String,
    pub spec: Option<Spec>, pub spec_gen: u64, pub present: bool, pub status: String, pub shape: String,
    req: Option<OutputRequest>, req_gen: u64, auto_class: Option<bool>, published: Option<u64>, pub_tag: u64,
    pub pinned_transfer: bool, /// a setting changed: answer the standing request again (QCView `setOutputSetting`)
    pub reanswer: bool,
}

#[derive(Default)]
pub struct OutputBus { pub outputs: Vec<Output>, last_poll: Option<Instant>, restored: bool, enc: Option<min_present::wire::WireEncoder>, tap: Vec<f32>, audio_t: i64 }

impl OutputBus {
    pub fn new() -> OutputBus { OutputBus { outputs: scan().into_iter().map(|m| Output { m, enabled: false, ring: None, audio: None, child: None, launched: false, launch_due: None, restarts: vec![], dying: None, plugin: String::new(), spec: None, spec_gen: 0, present: false, status: String::new(), shape: String::new(), req: None, req_gen: 0, auto_class: None, published: None, pub_tag: 0, pinned_transfer: false, reanswer: false }).collect(), ..Default::default() } }
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
        o.ring = None; o.audio = None; o.enabled = false; o.present = false; o.spec = None; o.launch_due = None; o.plugin.clear(); o.status.clear(); o.shape.clear();
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
        self.outputs_publish();
        if self.outputs.last_poll.is_some_and(|t| now.duration_since(t) < POLL) { return }
        self.outputs.last_poll = Some(now);
        self.output_tap_sync();   // the player / live feed may have changed since
        let src = self.output_source_size();
        let wide = src.0 > 1920 || src.1 > 1080;
        let (dual, has_b) = (self.dual.is_some(), self.dual.as_ref().is_some_and(|d| d.side(qcv_player::Side::B).is_some()));
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
                let (reply, spec, hdr) = answer_request(&r, cfg, src, dual, has_b);
                ring.write_reply(reply, o.req_gen);
                o.pinned_transfer = r.transfer != 0;
                let msg = String::from_utf8_lossy(&reply.message[..reply.message.iter().position(|&c| c == 0).unwrap_or(reply.message.len())]).into_owned();
                log::info!("OutputBus: {id}: request {} ({}x{} fmt {} transfer {} content {}) -> {} \"{msg}\"", o.req_gen, r.width, r.height, r.pixel_format, r.transfer, r.content, if spec.is_some() { "accepted" } else { "refused" });
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
        match c {
            Cmd::Start(id) => self.output_start(&id),
            Cmd::Stop(id) => self.output_stop(&id),
            Cmd::FeedMute(on) => { self.settings.set_bool(k::OUTPUT_FEED_MUTED, on); }
            Cmd::Transfer(id, v) => { set(&mut self.settings, &id, "transfer", v); self.output_reanswer(&id); }
            Cmd::Raster(id, v) => { set(&mut self.settings, &id, "raster", v); self.output_reanswer(&id); }
            Cmd::Primaries(id, v) => { set(&mut self.settings, &id, "primaries", v); self.output_reanswer(&id); }
        }
        if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; }
    }
    fn output_reanswer(&mut self, id: &str) { if let Some(o) = self.outputs.outputs.iter_mut().find(|o| o.m.id == id) { o.reanswer = true; } self.outputs.last_poll = None; }
}

/// The cards (QCView LeftRail's Output section): the state word, the line, the settings.
pub(crate) fn views(bus: &OutputBus, st: &min_settings::Settings) -> Vec<crate::output_panel::CardView> {
    {
        let out_setting = |id: &str, f: &str, d: f64| setting_of(st, id, f, d);
        let tr = |s: &str| min_i18n::trc("LeftRail", s).to_string();
        bus.outputs.iter().map(|o| {
            let (transfer, raster, primaries) = (out_setting(&o.m.id, "transfer", 1.0) as usize, out_setting(&o.m.id, "raster", 0.0) as usize, out_setting(&o.m.id, "primaries", 1.0) as usize);
            let state = if !o.enabled { if o.m.installed { "" } else { "not installed" } } else if o.present && o.spec.is_some() { "live" } else if o.present && o.req.is_some() { "refused" } else { "starting" };
            let line = if o.enabled {
                if o.present && o.spec.is_some() { if o.status.is_empty() { o.shape.clone() } else { format!("{}  ·  {}", o.shape, o.status) } }
                else if o.present && !o.shape.is_empty() { o.shape.clone() }
                else if !o.plugin.is_empty() && o.plugin != "running" { o.plugin.clone() } else { tr("waiting for the sink") }
            } else {
                let tname = if transfer == 2 { "PQ Rec.2100" } else { "SDR Rec.709" };
                let r = if o.m.raster_settable { format!("{}  ·  ", [tr("Auto"), tr("1080p"), tr("UHD")][raster.min(2)]) } else { String::new() };
                format!("{r}{}", tr(tname))
            };
            crate::output_panel::CardView { id: o.m.id.clone(), label: o.m.label.clone(), product: o.m.product.clone(), state, line, enabled: o.enabled, installed: o.m.installed,
                transfer: transfer.saturating_sub(1).min(1), raster: raster.min(2), primaries: primaries.saturating_sub(1).min(1), show_transfer: !o.pinned_transfer, show_raster: o.m.raster_settable }
        }).collect()
    }
}

impl crate::App {
    fn output_source_size(&self) -> (u32, u32) {
        if let Some(l) = self.live.as_ref() { let f = l.feed.facts(); return (f.width, f.height) }
        if let Some(p) = self.player.as_ref() { let i = p.info(); return (i.width, i.height) }
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
            if let Some(d) = e.dual() { format!("{:?}", d.mode).hash(&mut h); d.seam.to_bits().hash(&mut h); }
        }
        self.color_bound.hash(&mut h); self.loaded_item.hash(&mut h);
        // the chain's SETTINGS, not only which engine is bound (a slider drag keeps the engine; QCView's stamp holds the chain)
        self.settings.bool(k::COLOR_ENABLED).hash(&mut h);
        format!("{:?}", self.chain_of(&self.slot_item(0)).1.chain).hash(&mut h);
        if self.dual.is_some() { format!("{:?}", self.chain_of(&self.slot_item(1)).1.chain).hash(&mut h); }
        self.live.as_ref().and_then(|l| l.review.as_ref()).map(|r| (&r.label, &r.note, r.stored)).hash(&mut h);
        h.finish()
    }
    /// Publish-on-change (QCView: no clock of its own): every accepted, present output whose stamp moved gets a frame; finished
    /// read-backs land in their rings.
    fn outputs_publish(&mut self) {
        let stamp = self.output_stamp();
        let (playing, frame, fps, tc) = self.output_transport();
        let Some(g) = self.gpu.as_mut() else { return };
        let dev = g.presenter.engine.device.clone(); let q = g.presenter.engine.queue.clone();
        let enc = self.outputs.enc.get_or_insert_with(|| min_present::wire::WireEncoder::new(&dev));
        // the finished frames → their rings (tag = output index << 32 | spec_gen)
        let outs = &mut self.outputs.outputs;
        enc.take(&dev, &mut |bytes, w, hh, bpr, tag| {
            let (i, sgen) = ((tag >> 32) as usize, tag & 0xffff_ffff);
            let Some(o) = outs.get_mut(i) else { return };
            let (Some(ring), Some(spec)) = (o.ring.as_mut(), o.spec) else { return };
            if o.spec_gen & 0xffff_ffff != sgen || spec.w != w || spec.h != hh { return }   // a stale shape: dropped
            let Some(dst) = ring.begin_write(bpr as u64 * hh as u64) else { return };
            unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, (bpr * hh) as usize); }
            let (fn_, fd) = fps_rational(fps);
            let mut flags = v::FRAME_HAS_A | if playing { 0 } else { v::FRAME_PAUSED };
            let tcp = tc.as_deref().and_then(pack_tc);
            if let Some((_, drop)) = tcp { flags |= v::FRAME_TIMECODE_VALID | if drop { v::FRAME_DROP_FRAME } else { 0 }; }
            ring.commit(OutputFrameDesc { width: w, height: hh, bytes_per_row: bpr, pixel_format: match spec.fmt { min_present::wire::WireFormat::Bgra8 => OutputPixelFormat::Bgra8 as u32, min_present::wire::WireFormat::Rgb10 => OutputPixelFormat::Rgb10 as u32 }, transfer: spec.transfer, content: spec.content, layers: 0, flags, presentation_ns: qcvo::now_ns(), source_frame_index: frame, source_fps_num: fn_, source_fps_den: fd, timecode: tcp.map(|t| t.0).unwrap_or(0), _pad0: 0 });
        });
        // the new frames
        for i in 0..self.outputs.outputs.len() {
            let o = &mut self.outputs.outputs[i];
            let (true, true, Some(spec)) = (o.enabled, o.present, o.spec) else { continue };
            if o.published == Some(stamp) { continue }
            let (mode, white) = if spec.transfer == v::TRANSFER_PQ { (min_present::engine::OutMode::Pq, 100.0) } else { (min_present::engine::OutMode::SdrEncode, 100.0) };
            let Ok(canvas) = g.presenter.engine.render_output(spec.w, spec.h, mode, white) else { continue };
            o.pub_tag += 1;
            if enc.submit(&dev, &q, &canvas, spec.fmt, ((i as u64) << 32) | (o.spec_gen & 0xffff_ffff)) { o.published = Some(stamp); }
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
        (false, 0, 0.0, None)
    }
    /// The mixed audio → every present output's audio ring, 1024-frame planar packets (QCView `OutputBus::tapAudio`); the master
    /// mute or the feed mute → zeros flagged muted.
    fn outputs_audio(&mut self) {
        let mut buf = std::mem::take(&mut self.outputs.tap);
        if let Some(p) = self.player.as_mut() { p.audio.drain_tap(&mut buf); }
        if let Some(au) = self.live.as_mut().and_then(|l| l.audio.as_mut()) { au.drain_tap(&mut buf); }
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

/// QCView `OutputBus::answer`'s rules and words (the protocol's English: the sink shows it). HLG and per-side content arrive with
/// the AJA pass (the port's output pass encodes SDR and PQ today).
fn answer_request(r: &OutputRequest, cfg: (u32, u32, u32, f32, f32, u32, u32), src: (u32, u32), dual: bool, has_b: bool) -> (OutputReply, Option<Spec>, Option<OutputHdrInfo>) {
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
    if transfer == v::TRANSFER_HLG { return refuse(reply, v::REFUSE_TRANSFER, "HLG output is not available yet; SDR or PQ") }
    if r.fit != 0 && r.fit != v::FIT_LETTERBOX { return refuse(reply, v::REFUSE_FORMAT, "letterbox fit only for now") }
    if r.content > v::CONTENT_SIDE_B { return refuse(reply, v::REFUSE_CONTENT, "content must be composite, A or B") }
    if r.content == v::CONTENT_SIDE_B && !(dual && has_b) { return refuse(reply, v::REFUSE_CONTENT, "no B side loaded") }   // fix 6
    if dual && matches!(r.content, v::CONTENT_SIDE_A | v::CONTENT_SIDE_B) { return refuse(reply, v::REFUSE_CONTENT, "one side alone is not available yet; composite") }
    let content = if r.content == 0 { v::CONTENT_COMPOSITE } else { r.content };
    reply.accepted = 1; reply.width = w; reply.height = h; reply.pixel_format = r.pixel_format; reply.transfer = transfer; reply.content = content; reply.layers = 0;
    let fname = if fmt == min_present::wire::WireFormat::Rgb10 { "RGB10" } else { "BGRA8" };
    let tname = if transfer == v::TRANSFER_PQ { "PQ Rec.2100" } else { "SDR Rec.709" };
    msg(&mut reply.message, &format!("{w}x{h} {fname} {tname} from QCView"));
    let hdr = if transfer == v::TRANSFER_SDR { OutputHdrInfo { transfer, ..Default::default() } } else { OutputHdrInfo { transfer, primaries, max_mastering_nits: peak, min_mastering_nits: min, max_cll: cll, max_fall: fall, _pad0: [0; 2] } };
    (reply, Some(Spec { w, h, fmt, transfer, content }), Some(hdr))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn req(w: u32, h: u32, fmt: u32, transfer: u32, content: u32) -> OutputRequest { OutputRequest { protocol_version: 1, width: w, height: h, pixel_format: fmt, transfer, fit: 1, content, ..Default::default() } }
    const CFG: (u32, u32, u32, f32, f32, u32, u32) = (0, 1, 1, 1000.0, 0.005, 1000, 400);
    #[test]
    fn the_answers_are_qcviews() {
        let ok = |r: OutputRequest, src| answer_request(&r, CFG, src, false, false);
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
