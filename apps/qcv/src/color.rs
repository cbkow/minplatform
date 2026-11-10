//! The colour engines in the app (canon §18, rows 198–204). Until the Color drawer and its presets land (row 199, the
//! preset UX talk owed), a chain is engaged only from the harness: `QCV_MINCOLOR=<name>` with one of [`TEST_CHAINS`], or
//! `QCV_OCIO=<config>|<input>|<view>[|<look>]` against a bundled config folder (`QCV_OCIO_CONFIGS`, else the app's own
//! `assets/OCIO`).
//! The minColor pipeline builds OFF the main thread at startup (515 ms cold on macOS; QCView built it ahead too) and
//! binds to the slot when ready.
use qcv_color::{ids, Chain, MinColorPass};
use std::sync::mpsc;

/// Harness chains: (name, what it is for).
pub const TEST_CHAINS: &[(&str, &str)] = &[
    ("rec709", "Rec.709 / Rec.1886 in, un-tone-mapped (QCView's minColor default)"),
    ("rec709-drt", "Rec.709 / Rec.1886 in, OpenDRT"),
    ("linear-agx", "Linear Rec.709 in, AgX (renders, EXR)"),
    ("linear-drt", "Linear Rec.709 in, OpenDRT"),
    ("knee", "Linear Rec.709 in, Knee 1000 → 100, un-tone-mapped"),
];

pub fn test_chain(name: &str) -> Option<Chain> {
    let d = Chain::default();
    let lin = ids::OETF_LINEAR;
    Some(match name {
        "rec709" => d,
        "rec709-drt" => Chain { open_drt: 1, ..d },
        "linear-agx" => Chain { in_transfer: lin, agx_on: 1, ..d },
        "linear-drt" => Chain { in_transfer: lin, open_drt: 1, ..d },
        "knee" => Chain { in_transfer: lin, knee_on: 1, ..d },
        _ => return None,
    })
}

/// The chain `QCV_MINCOLOR` asks for (a warning names the choices when it is unknown).
pub fn wanted() -> Option<Chain> {
    let name = std::env::var("QCV_MINCOLOR").ok()?;
    let c = test_chain(&name);
    if c.is_none() { log::warn!("QCV_MINCOLOR={name}: not one of {:?}", TEST_CHAINS.iter().map(|(n, _)| *n).collect::<Vec<_>>()); }
    c
}

/// Build the pass on a worker; the receiver yields it once.
pub fn build_ahead(device: &wgpu::Device, queue: &wgpu::Queue, chain: Chain) -> mpsc::Receiver<Box<dyn min_present::ColorPass + Send>> {
    let (tx, rx) = mpsc::channel();
    let (d, q) = (device.clone(), queue.clone());
    std::thread::Builder::new().name("mincolor-build".into()).spawn(move || { let _ = tx.send(Box::new(MinColorPass::new(&d, &q, chain)) as Box<dyn min_present::ColorPass + Send>); }).ok();
    rx
}

/// Whichever chain the harness asked for, built ahead.
pub fn wanted_ahead(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<mpsc::Receiver<Box<dyn min_present::ColorPass + Send>>> {
    if let Some((cfg, spec)) = ocio_wanted() { return Some(ocio_build_ahead(device, queue, cfg, spec)) }
    wanted().map(|c| build_ahead(device, queue, c))
}

/// The bundled configs' folder (Blender5.2/ ACES_1.3/ ACES_2.0/).
pub fn ocio_configs_dir() -> std::path::PathBuf {
    std::env::var_os("QCV_OCIO_CONFIGS").map(std::path::PathBuf::from).unwrap_or_else(|| {
        // the configs ship with the app (`apps/qcv/assets/OCIO`; they came over from QCView-Player 2026-11-09): beside the
        // executable in a Windows package, in the bundle's Resources on macOS, else this workspace's copy (target/<profile>/QCView)
        let exe = std::env::current_exe().unwrap_or_default();
        let dir = exe.parent().unwrap_or(std::path::Path::new(""));
        let candidates = [dir.join("assets/OCIO"), dir.join("../Resources/assets/OCIO"), exe.ancestors().nth(3).map(|r| r.join("apps/qcv/assets/OCIO")).unwrap_or_default()];
        candidates.iter().find(|p| p.is_dir()).cloned().unwrap_or_else(|| candidates[2].clone())
    })
}

/// `QCV_KNEE=<source nits>,<target nits>[,<start>]` turns the Highlight Knee on for an OCIO harness chain.
pub fn knee_wanted() -> qcv_color::Knee {
    let Ok(v) = std::env::var("QCV_KNEE") else { return qcv_color::Knee::default() };
    let n: Vec<f32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    qcv_color::Knee { enabled: n.len() >= 2, source_nits: *n.first().unwrap_or(&1000.0), target_nits: *n.get(1).unwrap_or(&1000.0), start: *n.get(2).unwrap_or(&-1.0) }
}

/// The bundled configs (row 198): folder name, label.
pub const BUNDLED: [(&str, &str); 3] = [("ACES_2.0", "ACES 2.0"), ("ACES_1.3", "ACES 1.3"), ("Blender5.2", "Blender 5.2")];
/// A config key → its file: a bundled folder name under the configs folder, else the path itself.
pub fn ocio_config_path(key: &str) -> String {
    if key.contains('/') || key.contains('\\') || key.ends_with(".ocio") { key.to_string() } else { ocio_configs_dir().join(key).join("config.ocio").to_string_lossy().into_owned() }
}

/// `QCV_OCIO=<config>|<input>|<view>[|<look>]`.
pub fn ocio_wanted() -> Option<(String, qcv_color::ocio::Spec)> {
    let v = std::env::var("QCV_OCIO").ok()?;
    let p: Vec<&str> = v.split('|').collect();
    if p.len() < 3 { log::warn!("QCV_OCIO wants <config>|<input>|<view>[|<look>], got {v:?}"); return None }
    let cfg = ocio_configs_dir().join(p[0]).join("config.ocio");
    Some((cfg.to_string_lossy().into_owned(), qcv_color::ocio::Spec { input: p[1].into(), view: p[2].into(), look: p.get(3).unwrap_or(&"").to_string(), scene_lut: String::new(), ccc_id: String::new() }))
}

/// Build an OCIO pass on a worker (OCIO + the backend compile); `None` on the channel when it failed (logged).
pub fn ocio_build_ahead(device: &wgpu::Device, queue: &wgpu::Queue, config: String, spec: qcv_color::ocio::Spec) -> mpsc::Receiver<Box<dyn min_present::ColorPass + Send>> {
    let (tx, rx) = mpsc::channel();
    let (d, q) = (device.clone(), queue.clone());
    std::thread::Builder::new().name("ocio-build".into()).spawn(move || {
        match qcv_color::ocio::Config::open(&config).and_then(|c| qcv_color::ocio::OcioPass::new(&d, &q, &c, spec, knee_wanted())) {
            Ok(p) => { let _ = tx.send(Box::new(p) as Box<dyn min_present::ColorPass + Send>); }
            Err(e) => log::error!("OCIO: {e}"),
        }
    }).ok();
    rx
}

// ---- the Color drawer's engine binding (canon rows 199 / 205–208) -------------------------------------------------------

use crate::color_drawer::{Cmd as DCmd, OcioLists, View};
use crate::color_model::{fallback_default, ClipChain, OcioChain, Preset, Stored};
use std::{cell::RefCell, rc::Rc};

/// The one minColor pass, shared between the shell (its chain changes) and the engine's slot (it runs).
pub struct SharedPass(pub Rc<RefCell<MinColorPass>>);
impl min_present::ColorPass for SharedPass {
    fn input(&self) -> min_present::Space { self.0.borrow().input() }
    fn output(&self) -> min_present::Space { self.0.borrow().output() }
    fn run(&mut self, enc: &mut wgpu::CommandEncoder, src: &wgpu::Texture, dst: &wgpu::Texture) { self.0.borrow_mut().run(enc, src, dst) }
    fn set_scope_tap(&mut self, on: bool) { self.0.borrow_mut().set_scope_tap(on) }
    fn scope_tap(&self) -> Option<(wgpu::Texture, [f32; 9])> { self.0.borrow().scope_tap() }
}

/// The harness owns the slot when it asked for a chain (`QCV_MINCOLOR` / `QCV_OCIO`).
pub fn harness_active() -> bool { std::env::var_os("QCV_MINCOLOR").is_some() || std::env::var_os("QCV_OCIO").is_some() }

/// Build the minColor pass on a worker at startup (QCView built it ahead too).
pub fn mincolor_ahead(device: &wgpu::Device, queue: &wgpu::Queue) -> mpsc::Receiver<MinColorPass> {
    let (tx, rx) = mpsc::channel();
    let (d, q) = (device.clone(), queue.clone());
    std::thread::Builder::new().name("mincolor-build".into()).spawn(move || { let _ = tx.send(MinColorPass::new(&d, &q, Chain::default())); }).ok();
    rx
}

/// The nudge (the user 2026-11-09: "it will make it easier for the user if we nudge them toward that pq 2020 input for these types
/// of streams"): a LIVE stream tagged PQ with no chain of its own starts minColor from the project default with the input set to
/// what its pixels are — Rec.2020 (P3-D65 when tagged so) / PQ (100 nits = 1.0: QCBridge's Blender wire, and minColor's only PQ).
/// Nothing is stored until the user edits (the edit keeps it); the project default and an own chain are untouched; OCIO left
/// alone (a PQ input's name is the config's).
fn stream_nudge(item: &qcv_project::MediaItem, mut d: Stored) -> Stored {
    if item.media_type != qcv_project::MediaType::LiveStream || item.video.color_transfer != "smpte2084" { return d }
    if let ClipChain::MinColor(c) = &mut d.chain {
        let p = item.video.color_primaries.to_lowercase();
        c.in_gamut = if p.contains("431") || p.contains("432") || p.contains("p3") { 3 /* P3-D65 */ } else { qcv_color::ids::IN_REC2020 };
        c.in_transfer = 14;   // DRT_OETF_PQ_100
        c.in_limited = 0;
    }
    d
}

impl crate::App {
    /// Colour starts OFF at every launch (the user 2026-11-08: "ensure color is OFF when the app opens fresh unless a project has a
    /// specific media-item with assigned color"); opening a project that has a clip with its own chain turns it on.
    pub(crate) fn color_on_for_project(&mut self) {
        let assigned = self.project.pool().iter().any(|m| Stored::from_clip_maps(&m.ocio_clip, &m.min_color_clip, &fallback_default()).is_some());
        if assigned { self.settings.set_bool(crate::settings::k::COLOR_ENABLED, true); }
    }
    pub(crate) fn color_default(&self) -> Stored { Stored::from_map(self.project.color_default()).unwrap_or_else(fallback_default) }
    /// The item a colour slot shows: slot 0 = the loaded clip (dual A), slot 1 = dual B's.
    pub(crate) fn slot_item(&self, slot: usize) -> String { if slot == 0 { self.loaded_item.clone() } else if self.dual.is_some() { self.project.b_source_media_id().to_string() } else { String::new() } }
    /// The slot the drawer edits: dual view's TARGET side (row 212 — one selection for the lanes, the keys and colour), else 0.
    pub(crate) fn edit_slot(&self) -> usize { if self.dual.is_some() && self.gpu.as_ref().is_some_and(|g| g.dual_bars.target == qcv_player::Side::B) { 1 } else { 0 } }
    /// An item's chain and whether it is its own (else the project default's).
    pub(crate) fn chain_of(&self, id: &str) -> (Option<String>, Stored, bool) {
        let d = self.color_default();
        let Some(item) = self.project.item(id) else { return (None, d, false) };
        match Stored::from_clip_maps(&item.ocio_clip, &item.min_color_clip, &d) { Some(s) => (Some(item.id.clone()), s, true), None => (Some(item.id.clone()), stream_nudge(item, d), false) }
    }
    /// The edited clip's chain and whether it is its own (else the project default's).
    pub(crate) fn color_target(&self) -> (Option<String>, Stored, bool) { self.chain_of(&self.slot_item(self.edit_slot())) }
    pub(crate) fn color_view(&self) -> View {
        let (id, stored, own) = self.color_target();
        let target = id.as_ref().and_then(|i| self.project.item(i)).map(|m| m.name.clone()).unwrap_or_else(|| min_i18n::trc("ColorPanel", "Project default").to_string());
        let custom = self.colors.find(&stored.preset).map(|p| p.stored.chain != stored.chain).unwrap_or(true);
        let file_peak = id.as_ref().and_then(|i| self.project.item(i)).map(|m| (m.video.max_cll, m.video.mastering_max_nits)).unwrap_or((0, 0.0));
        let ocio = if let ClipChain::Ocio(o) = &stored.chain { self.ocio_configs.get(&o.config).map(|c| {
            let mut configs: Vec<(String, String)> = BUNDLED.iter().map(|(k, l)| (k.to_string(), l.to_string())).collect();
            if !BUNDLED.iter().any(|(k, _)| *k == o.config) { configs.push((o.config.clone(), std::path::Path::new(&o.config).parent().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| o.config.clone()))); }
            OcioLists { configs, inputs: c.inputs(), looks: c.looks(), views: c.views() }
        }) } else { None };
        // the source is HDR when its Input says so (minColor PQ / HLG, an OCIO PQ / HLG space) or, in bypass, its tags do
        let tags_hdr = id.as_ref().and_then(|i| self.project.item(i)).is_some_and(|m| { let t = m.video.color_transfer.to_lowercase(); t.contains("2084") || t.contains("pq") || t.contains("hlg") || t.contains("arib") });
        let on = self.settings.bool(crate::settings::k::COLOR_ENABLED);
        let hdr_source = match &stored.chain { _ if !on => tags_hdr, ClipChain::MinColor(c) => c.in_transfer == 14 || c.in_transfer == 15, ClipChain::Ocio(o) => { let n = o.input.to_uppercase(); n.contains("PQ") || n.contains("2084") || n.contains("HLG") } };
        let side = self.dual.is_some().then(|| if self.edit_slot() == 1 { qcv_player::Side::B } else { qcv_player::Side::A });
        // row 222: the picture can exceed SDR white (an HDR file shown as is, or a chain whose last stage outputs HDR) on a screen
        // that has no headroom — its highlights clip; the drawer says so and points at the Highlight Knee
        let picture_hdr = picture_is_hdr(&stored, on, tags_hdr);
        let clip_warn = self.screen_hdr == Some(false) && picture_hdr && id.is_some();
        // row 246: the Video emulation acts on a chain's SDR picture — with Color off the picture is already as stored
        let video_state = if !on { 1 } else if picture_hdr { 2 } else { 0 };
        // row 257 R13: Export LUT is offered whenever the edited chain is complete, Color on or off, either engine (QCView
        // gated it on OCIO's state even under minColor — spec B5)
        let export_ok = match &stored.chain { ClipChain::MinColor(_) => true, ClipChain::Ocio(o) => !o.input.is_empty() && !o.view.is_empty() && self.ocio_configs.contains_key(&o.config) };
        // row 257 R5: the edited chain's Scene LUT did not load (whichever slot shows that chain)
        let lut_error = match &stored.chain { ClipChain::Ocio(o) => { let key = ocio_key(o); self.ocio_error.iter().flatten().find(|(k, _)| *k == key).map(|(_, why)| why.clone()).unwrap_or_default() } _ => String::new() };
        View { target, side, has_clip: id.is_some(), own, stored, custom, on, presets: self.colors.all(), file_peak, ocio, hdr_source, clip_warn, video: self.settings.bool(crate::settings::k::VIDEO_EMULATION), video_state, lut_error, export_ok }
    }
    /// The Video emulation onto the screen's engines (row 246), each tick: the setting, where a chain renders that slot's
    /// picture in SDR. Never in the difference view (its sides feed arithmetic, not a display).
    pub(crate) fn sync_video_emulation(&mut self) {
        let want = self.settings.bool(crate::settings::k::VIDEO_EMULATION) && self.settings.bool(crate::settings::k::COLOR_ENABLED);
        let dual = self.dual.is_some();
        let mut flags = [false; 2];
        for slot in 0..if dual { 2 } else { 1 } {
            if !want || self.color_bound[slot].is_none() { continue }
            let (id, stored, _) = self.chain_of(&self.slot_item(slot));
            let tags_hdr = id.as_ref().and_then(|i| self.project.item(i)).is_some_and(|m| { let t = m.video.color_transfer.to_lowercase(); t.contains("2084") || t.contains("pq") || t.contains("hlg") || t.contains("arib") });
            flags[slot] = !picture_is_hdr(&stored, true, tags_hdr);
        }
        let Some(g) = self.gpu.as_mut() else { return };
        if g.presenter.engine.dual().is_some_and(|d| d.mode == min_present::dual::CompareMode::Difference) { flags = [false; 2]; }
        let mut changed = false;
        for slot in 0..if dual { 2 } else { 1 } { if let Some(e) = slot_engine(g, slot, dual) && e.video_emulation() != flags[slot] { e.set_video_emulation(flags[slot]); changed = true; } }
        if changed { g.ui_dirty = true; }
    }
    /// Bind each visible picture's chain to its engine: the single view's (slot 0), or dual's A (slot 0) and B (slot 1), each
    /// from its own clip (row 205); off = bypass. minColor = the slot's pass (a uniform update); OCIO = a pass built on a
    /// worker (the previous chain stays on screen until it is ready).
    pub(crate) fn apply_color(&mut self) {
        if harness_active() { return }
        let on = self.settings.bool(crate::settings::k::COLOR_ENABLED);
        let slots = if self.dual.is_some() { 2 } else { 1 };
        for slot in 0..2 {
            let shown = slot < slots && (if self.dual.is_some() { self.dual.as_ref().unwrap().side(if slot == 0 { qcv_player::Side::A } else { qcv_player::Side::B }).is_some() } else { self.player.is_some() || self.live.as_ref().is_some_and(|l| !l.review.as_ref().is_some_and(|r| r.stored)) });   // a live feed (row 228) takes the chain as a file does — not a saved note's PNG in Review (display-referred: QCView's bypass)
            let (_, stored, _) = self.chain_of(&self.slot_item(slot));
            let dual = self.dual.is_some();
            let Some(g) = self.gpu.as_mut() else { return };
            let Some(engine) = slot_engine(g, slot, dual) else { self.color_bound[slot] = None; self.ocio_rx[slot] = None; self.ocio_error[slot] = None; continue };
            if !on || !shown {
                if self.color_bound[slot].is_some() { engine.set_color_pass(None); self.color_bound[slot] = None; }
                self.ocio_rx[slot] = None; self.ocio_error[slot] = None;
                continue
            }
            match stored.chain {
                ClipChain::MinColor(c) => {
                    let Some(rc) = self.mincolor[slot].clone() else { continue };   // binds when the build lands
                    rc.borrow_mut().set_chain(c);
                    if self.color_bound[slot].as_deref() != Some("minColor") { engine.set_color_pass(Some(Box::new(SharedPass(rc)))); self.color_bound[slot] = Some("minColor".into()); }
                    self.ocio_rx[slot] = None; self.ocio_error[slot] = None;
                }
                ClipChain::Ocio(o) => {
                    let key = ocio_key(&o);
                    if self.color_bound[slot].as_deref() == Some(key.as_str()) || self.ocio_rx[slot].as_ref().is_some_and(|(k, _)| *k == key) { continue }
                    // a Scene LUT that did not load: bypass stays, and the same chain is not built again and again
                    if self.ocio_error[slot].as_ref().is_some_and(|(k, _)| *k == key) { continue }
                    self.ocio_error[slot] = None;
                    let cfg = ocio_config_path(&o.config);
                    let spec = qcv_color::ocio::Spec { input: o.input, look: o.look, view: o.view, scene_lut: lut_native(&o.scene_lut), ccc_id: o.ccc_id };
                    let (d, q, knee) = (engine.device.clone(), engine.queue.clone(), o.knee);
                    let (tx, rx) = mpsc::channel();
                    std::thread::Builder::new().name("ocio-build".into()).spawn(move || {
                        match qcv_color::ocio::Config::open(&cfg).and_then(|c| qcv_color::ocio::OcioPass::new(&d, &q, &c, spec, knee)) { Ok(p) => { let _ = tx.send(Ok(Box::new(p) as Box<dyn min_present::ColorPass + Send>)); } Err(e) => { log::error!("OCIO: {e}"); let _ = tx.send(Err(e)); } }
                    }).ok();
                    self.ocio_rx[slot] = Some((key, rx));
                }
            }
        }
    }
    /// The drawer's background builds landing (called every tick).
    pub(crate) fn poll_color(&mut self) {
        for slot in 0..2 {
            if let Some(rx) = &self.mincolor_ready[slot] && let Ok(p) = rx.try_recv() { self.mincolor[slot] = Some(Rc::new(RefCell::new(p))); self.mincolor_ready[slot] = None; self.apply_color(); }
            if let Some((key, rx)) = &self.ocio_rx[slot] && let Ok(built) = rx.try_recv() {
                let key = key.clone(); self.ocio_rx[slot] = None;
                let dual = self.dual.is_some();
                match built {
                    Ok(p) => { self.ocio_error[slot] = None; if let Some(g) = self.gpu.as_mut() { if let Some(e) = slot_engine(g, slot, dual) { e.set_color_pass(Some(p)); self.color_bound[slot] = Some(key); } g.ui_dirty = true; } }
                    Err(why) => {
                        // canon row 257, R5: a Scene LUT that cannot load (missing, malformed, an unknown Correction ID) says so and
                        // the clip drops to Color BYPASS — never the previous clip's chain, never this chain minus its LUT (QCView
                        // showed the old picture and told the user nothing). A chain without a LUT keeps its old behaviour.
                        let (_, stored, _) = self.chain_of(&self.slot_item(slot));
                        let lut = match &stored.chain { ClipChain::Ocio(o) if ocio_key(o) == key && !o.scene_lut.is_empty() => Some(o.scene_lut.clone()), _ => None };
                        if let Some(lut) = lut {
                            let name = lut.rsplit(['/', '\\']).next().unwrap_or(&lut).to_string();
                            if let Some(g) = self.gpu.as_mut() {
                                if let Some(e) = slot_engine(g, slot, dual) { e.set_color_pass(None); }
                                g.toast.show(&min_i18n::fill(min_i18n::trc("ColorPanel", "Couldn't load the Scene LUT %1: %2"), &[&name, &why]), min_ui::ToastKind::Negative, std::time::Instant::now());
                                g.ui_dirty = true;
                            }
                            self.color_bound[slot] = None; self.ocio_error[slot] = Some((key, why));
                        }
                    }
                }
            }
        }
    }
    /// A config by its key (a bundled folder name, or a file path), loaded once and kept.
    pub(crate) fn ocio_config(&mut self, key: &str) -> Option<std::sync::Arc<qcv_color::ocio::Config>> {
        if let Some(c) = self.ocio_configs.get(key) { return Some(c.clone()) }
        match qcv_color::ocio::Config::open(&ocio_config_path(key)) {
            Ok(c) => { self.ocio_configs.insert(key.to_string(), c.clone()); Some(c) }
            Err(e) => { log::error!("OCIO config {key}: {e}"); if let Some(g) = self.gpu.as_mut() { g.toast.show(&min_i18n::fill(min_i18n::trc("ColorPanel", "Couldn't load the OCIO config: %1"), &[&e]), min_ui::ToastKind::Negative, std::time::Instant::now()); } None }
        }
    }
    /// An OCIO chain's names checked against its config: a missing input falls to the scene_linear role, a missing view to the
    /// first SDR view, a missing look to none (QCView keeps names across configs where they exist).
    fn fixup_ocio(&mut self, mut o: OcioChain) -> OcioChain {
        let Some(c) = self.ocio_config(&o.config) else { return o };
        let ins = c.inputs(); if !ins.contains(&o.input) { let r = c.role("scene_linear"); o.input = if ins.contains(&r) { r } else { ins.first().cloned().unwrap_or_default() }; }
        let vs = c.views(); if !vs.contains(&o.view) { o.view = vs.iter().find(|v| v.contains("SDR") || v.as_str() == "Standard").or(vs.first()).cloned().unwrap_or_default(); }
        if !o.look.is_empty() && !c.looks().contains(&o.look) { o.look.clear(); }
        o
    }
    /// Keep the target's OCIO config loaded (the drawer's lists come from it).
    pub(crate) fn ensure_color_config(&mut self) {
        let (_, st, _) = self.color_target();
        if let ClipChain::Ocio(o) = st.chain { if !self.ocio_configs.contains_key(&o.config) { let _ = self.ocio_config(&o.config); } }
    }
    fn write_target(&mut self, st: Stored) {
        match self.project.item(&self.slot_item(self.edit_slot())).cloned() {
            Some(item) => { let (oc, mc) = st.to_clip_maps(&item.ocio_clip, &item.min_color_clip); self.project.set_color_clip(&item.id, oc, mc); }
            None => { self.project.set_color_default(st.to_map()); }
        }
    }
    pub(crate) fn color_cmd(&mut self, c: DCmd) {
        use crate::settings::k;
        let now = std::time::Instant::now();
        match c {
            DCmd::Pick(mut st) | DCmd::Edit(mut st) => {
                if let ClipChain::Ocio(o) = st.chain { st.chain = ClipChain::Ocio(self.fixup_ocio(o)); }
                self.settings.set_bool(k::COLOR_ENABLED, true); self.write_target(st);
            }
            DCmd::NewOcio => {
                let o = self.fixup_ocio(OcioChain { config: BUNDLED[0].0.into(), input: String::new(), look: String::new(), view: String::new(), scene_lut: String::new(), ccc_id: String::new(), knee: qcv_color::Knee::default() });
                self.settings.set_bool(k::COLOR_ENABLED, true); self.write_target(Stored { chain: ClipChain::Ocio(o), preset: String::new() });
            }
            DCmd::ExportLut => self.export_lut(),
            DCmd::PickLut => {
                // QCView's `lutPicker`: "Choose LUT", the scene slot's two filters; a new file starts at its first correction
                let (_, st, _) = self.color_target();
                let tc = |s: &'static str| min_i18n::trc("ColorPanel", s);
                if let (ClipChain::Ocio(o), Some(p)) = (st.chain.clone(), rfd::FileDialog::new().set_title(tc("Choose LUT")).add_filter(tc("LUT / CDL files (*.cube *.3dl *.csp *.cc *.ccc *.cdl)"), &["cube", "3dl", "csp", "cc", "ccc", "cdl"]).add_filter(tc("All files (*)"), &["*"]).pick_file()) {
                    self.settings.set_bool(k::COLOR_ENABLED, true);
                    self.write_target(Stored { chain: ClipChain::Ocio(OcioChain { scene_lut: p.to_string_lossy().into_owned(), ccc_id: String::new(), ..o }), preset: st.preset });
                }
            }
            DCmd::OpenConfig => {
                let (_, st, _) = self.color_target();
                if let (ClipChain::Ocio(o), Some(p)) = (st.chain.clone(), rfd::FileDialog::new().set_title(min_i18n::trc("ColorPanel", "Open OCIO config")).add_filter("OCIO config", &["ocio"]).pick_file()) {
                    let o = self.fixup_ocio(OcioChain { config: p.to_string_lossy().into_owned(), ..o });
                    self.write_target(Stored { chain: ClipChain::Ocio(o), preset: st.preset });
                }
            }
            DCmd::ResetToDefault => { let id = self.slot_item(self.edit_slot()); if self.project.item(&id).is_some() { self.project.set_color_clip(&id, Default::default(), Default::default()); } }
            DCmd::ApplyToAll => {
                let (_, st, _) = self.color_target();
                let ids: Vec<(String, _, _)> = self.project.pool().iter().filter(|m| !matches!(m.media_type, qcv_project::MediaType::Audio | qcv_project::MediaType::LiveStream)).map(|m| (m.id.clone(), m.ocio_clip.clone(), m.min_color_clip.clone())).collect();
                for (id, oc, mc) in ids { let (oc, mc) = st.to_clip_maps(&oc, &mc); self.project.set_color_clip(&id, oc, mc); }
            }
            DCmd::MakeDefault => { let (_, st, _) = self.color_target(); self.project.set_color_default(st.to_map()); }
            DCmd::SetOn(on) => { self.settings.set_bool(k::COLOR_ENABLED, on); }
            DCmd::SavePreset(name) => {
                let (_, st, _) = self.color_target();
                let name = self.colors.unique(&name);
                self.colors.user.push(Preset { name: name.clone(), stored: Stored { chain: st.chain.clone(), preset: name.clone() }, builtin: false }); self.colors.save();
                self.write_target(Stored { chain: st.chain, preset: name });
            }
            DCmd::RenamePreset(old, new) => {
                if self.colors.user.iter().any(|p| p.name == new) && new != old { return }
                if let Some(p) = self.colors.user.iter_mut().find(|p| p.name == old) { p.name = new.clone(); p.stored.preset = new.clone(); }
                self.colors.save();
                let (_, st, _) = self.color_target(); if st.preset == old { self.write_target(Stored { preset: new, ..st }); }
            }
            DCmd::UpdatePreset(name) => {
                let (_, st, _) = self.color_target();
                if let Some(p) = self.colors.user.iter_mut().find(|p| p.name == name) { p.stored.chain = st.chain.clone(); }
                self.colors.save();
                if st.preset == name { self.write_target(st); }
            }
            DCmd::DeletePreset(name) => { self.colors.user.retain(|p| p.name != name); self.colors.save(); }
            DCmd::Waveform(hdr, peak, open) => { self.settings.set_int(k::WAVEFORM_SCALE, match hdr { None => -1, Some(false) => 0, Some(true) => 1 }); self.settings.set_int(k::WAVEFORM_PEAK, peak as i64); self.settings.set_bool(k::WAVEFORM_OPEN, open); }
            DCmd::Height(h) => { self.settings.set_int(k::COLOR_DRAWER_HEIGHT, h.round() as i64); }
            DCmd::Video(on) => { self.settings.set_bool(k::VIDEO_EMULATION, on); if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; } }
            DCmd::Aids(a) => { if let Some(g) = self.gpu.as_mut() { g.presenter.engine.set_viewer_aids(a); } }
            DCmd::Tab => { if self.gpu.as_ref().is_some_and(|g| g.notes.open) { self.close_notes(); return } return self.color_cmd(DCmd::Toggle) }
            DCmd::Toggle => { let opening = self.gpu.as_ref().is_some_and(|g| !g.drawer.open); if opening { self.close_notes(); } if let Some(g) = self.gpu.as_mut() { g.drawer.toggle(now); self.settings.set_bool(k::COLOR_DRAWER_OPEN, g.drawer.open); g.ui_dirty = true; } }   // one drawer at a time (row 215)
        }
        self.apply_color();
        if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; }
    }
}

/// A colour slot's engine: 0 = the single view's (the main engine) or dual's A side, 1 = dual's B side.
pub(crate) fn slot_engine(g: &mut crate::Gpu, slot: usize, dual: bool) -> Option<&mut min_present::engine::Engine> {
    if !dual { return (slot == 0).then_some(&mut g.presenter.engine) }
    let d = g.presenter.engine.dual_mut()?;
    Some(if slot == 0 { &mut d.a } else { &mut d.b })
}

/// Row 222: whether a minColor chain can put out more than SDR white — its LAST stage decides: AgX or OpenDRT by their target
/// peak; un-tone-mapped, the Highlight Knee's target when it is on, else the input (a PQ / HLG source passes its highlights
/// through; scene-linear / SDR inputs are SDR-referred).
/// Row 222's test: can the picture this chain hands the display exceed SDR white? With Color off, the source's own tags say.
pub(crate) fn picture_is_hdr(stored: &Stored, on: bool, tags_hdr: bool) -> bool {
    match &stored.chain { _ if !on => tags_hdr, ClipChain::MinColor(c) => mincolor_outputs_hdr(c), ClipChain::Ocio(o) => !qcv_color::ocio::view_is_sdr(&o.view) && !(o.knee.enabled && o.knee.target_nits <= 100.5) }
}
pub(crate) fn mincolor_outputs_hdr(c: &qcv_color::Chain) -> bool {
    if c.agx_on != 0 { return c.agx_peak > 100.5 }
    if c.open_drt != 0 { return c.peak_nits > 100.5 }
    if c.knee_on != 0 { return c.knee_tgt > 100.5 }
    c.in_transfer == 14 || c.in_transfer == 15
}

#[cfg(test)]
mod hdr_chain_tests {
    #[test] fn the_last_stage_decides() {
        let mut c = qcv_color::Chain::default(); assert!(!super::mincolor_outputs_hdr(&c), "Rec.1886 in, un-tone-mapped");
        c.in_transfer = 14; assert!(super::mincolor_outputs_hdr(&c), "PQ in, un-tone-mapped: the highlights pass");
        c.knee_on = 1; c.knee_tgt = 100.0; assert!(!super::mincolor_outputs_hdr(&c), "the knee to 100 nits fits them");
        c.knee_tgt = 600.0; assert!(super::mincolor_outputs_hdr(&c));
        c.agx_on = 1; c.agx_peak = 100.0; assert!(!super::mincolor_outputs_hdr(&c), "AgX to SDR");
        c.agx_on = 0; c.knee_on = 0; c.open_drt = 1; c.peak_nits = 1000.0; assert!(super::mincolor_outputs_hdr(&c), "OpenDRT at 1000 nits");
    }
}

impl crate::App {
    /// EXPORT LUT (canon row 257, spec `docs/phase2/step6-luts.md` R9–R14): the edited target's chain — the clip on screen,
    /// dual's edited side, else the project default — baked to QCView's 65³ `.cube`: Input → Look → Scene LUT → Knee → the
    /// view / the Rendering, as a screenshot sees it (the SDR twin, sRGB-encoded). Works with Color off; never the viewer
    /// aids or the Video emulation. No suggested name (a clip's or a preset's name would ride out in a file sent elsewhere).
    fn export_lut(&mut self) {
        let tc = |s: &'static str| min_i18n::trc("ColorPanel", s);
        let (_, st, _) = self.color_target();
        let Some(path) = rfd::FileDialog::new().set_title(tc("Export LUT…")).add_filter(tc("3D LUT (*.cube)"), &["cube"]).save_file() else { return };
        let path = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("cube")) { path } else { let mut p = path.into_os_string(); p.push(".cube"); std::path::PathBuf::from(p) };
        let n = qcv_color::bake::CUBE_SIZE;
        let baked: Result<(Vec<f32>, bool, &str), String> = match st.chain {
            ClipChain::MinColor(c) => { let mut sdr = c; sdr.peak_nits = 100.0; sdr.agx_peak = 100.0; Ok((qcv_color::bake::bake_mincolor(&sdr, n), false, "minColor Transform LUT")) }
            ClipChain::Ocio(o) => match self.ocio_config(&o.config) {
                None => Err(min_i18n::trc("qcv::OCIOConfigManager", "No OCIO config loaded").to_string()),
                Some(cfg) => {
                    let view = crate::capture::sdr_view(&cfg.views(), &o.view).unwrap_or_else(|| o.view.clone());
                    let spec = qcv_color::ocio::Spec { input: o.input.clone(), look: o.look.clone(), view, scene_lut: lut_native(&o.scene_lut), ccc_id: o.ccc_id.clone() };
                    qcv_color::bake::bake_ocio(&cfg, &spec, o.knee, n).map(|(rgb, skipped)| (rgb, skipped, "OCIO Transform LUT"))
                }
            },
        };
        let name = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        let (text, kind) = match baked.and_then(|(rgb, skipped, title)| qcv_color::bake::write_atomic(&path, &qcv_color::bake::cube_text(title, n, &rgb)).map(|_| skipped).map_err(|e| e.to_string())) {
            Ok(false) => (min_i18n::fill(tc("LUT exported: %1"), &[&name]), min_ui::ToastKind::Positive),
            // the chain could not be split around the Knee: QCView exported without it and only its log said so (spec B6)
            Ok(true) => (min_i18n::fill(tc("LUT exported without the Highlight Knee: %1"), &[&name]), min_ui::ToastKind::Notice),
            Err(why) => { log::warn!("Export LUT: {why}"); (min_i18n::fill(tc("Export LUT failed — %1"), &[&why]), min_ui::ToastKind::Negative) }
        };
        if let Some(g) = self.gpu.as_mut() { g.toast.show(&text, kind, std::time::Instant::now()); g.ui_dirty = true; }
    }
}

/// What makes an OCIO pass what it is (a change = a new build).
pub(crate) fn ocio_key(o: &OcioChain) -> String { format!("{}|{}|{}|{}|{}|{}|{:?}", o.config, o.input, o.look, o.view, o.scene_lut, o.ccc_id, o.knee) }
/// A stored Scene LUT path as this OS opens it. The project keeps the path VERBATIM (QCView's form, canon row 257 R7); only
/// the separators are turned here — the app has no drive / mount mapping for any path yet.
pub(crate) fn lut_native(path: &str) -> String { if path.is_empty() { String::new() } else { crate::os_actions::native(path) } }
