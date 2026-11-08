//! The colour engines in the app (canon §18, rows 198–204). Until the Color drawer and its presets land (row 199, the
//! preset UX talk owed), a chain is engaged only from the harness: `QCV_MINCOLOR=<name>` with one of [`TEST_CHAINS`], or
//! `QCV_OCIO=<config>|<input>|<view>[|<look>]` against a bundled config folder (`QCV_OCIO_CONFIGS`, else QCView's
//! checkout beside this repo — where the configs ship from is a packaging question).
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
        let exe = std::env::current_exe().unwrap_or_default();
        // target/<profile>/qcv → the repo → its sibling QCView-Player
        exe.ancestors().nth(4).map(|r| r.join("QCView-Player/assets/OCIO")).unwrap_or_default()
    })
}

/// `QCV_KNEE=<source nits>,<target nits>[,<start>]` turns the Highlight Knee on for an OCIO harness chain.
pub fn knee_wanted() -> qcv_color::Knee {
    let Ok(v) = std::env::var("QCV_KNEE") else { return qcv_color::Knee::default() };
    let n: Vec<f32> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    qcv_color::Knee { enabled: n.len() >= 2, source_nits: *n.first().unwrap_or(&1000.0), target_nits: *n.get(1).unwrap_or(&1000.0), start: *n.get(2).unwrap_or(&-1.0) }
}

/// `QCV_OCIO=<config>|<input>|<view>[|<look>]`.
pub fn ocio_wanted() -> Option<(String, qcv_color::ocio::Spec)> {
    let v = std::env::var("QCV_OCIO").ok()?;
    let p: Vec<&str> = v.split('|').collect();
    if p.len() < 3 { log::warn!("QCV_OCIO wants <config>|<input>|<view>[|<look>], got {v:?}"); return None }
    let cfg = ocio_configs_dir().join(p[0]).join("config.ocio");
    Some((cfg.to_string_lossy().into_owned(), qcv_color::ocio::Spec { input: p[1].into(), view: p[2].into(), look: p.get(3).unwrap_or(&"").to_string(), scene_lut: String::new() }))
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

use crate::color_drawer::{Cmd as DCmd, View};
use crate::color_model::{fallback_default, ClipChain, Preset, Stored};
use std::{cell::RefCell, rc::Rc};

/// The one minColor pass, shared between the shell (its chain changes) and the engine's slot (it runs).
pub struct SharedPass(pub Rc<RefCell<MinColorPass>>);
impl min_present::ColorPass for SharedPass {
    fn input(&self) -> min_present::Space { self.0.borrow().input() }
    fn output(&self) -> min_present::Space { self.0.borrow().output() }
    fn run(&mut self, enc: &mut wgpu::CommandEncoder, src: &wgpu::Texture, dst: &wgpu::Texture) { self.0.borrow_mut().run(enc, src, dst) }
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

impl crate::App {
    pub(crate) fn color_default(&self) -> Stored { Stored::from_map(self.project.color_default()).unwrap_or_else(fallback_default) }
    /// The loaded clip's chain and whether it is its own (else the project default's).
    pub(crate) fn color_target(&self) -> (Option<String>, Stored, bool) {
        let d = self.color_default();
        let Some(item) = self.project.item(&self.loaded_item) else { return (None, d, false) };
        match Stored::from_clip_maps(&item.ocio_clip, &item.min_color_clip, &d) { Some(s) => (Some(item.id.clone()), s, true), None => (Some(item.id.clone()), d, false) }
    }
    pub(crate) fn color_view(&self) -> View {
        let (id, stored, own) = self.color_target();
        let target = id.as_ref().and_then(|i| self.project.item(i)).map(|m| m.name.clone()).unwrap_or_else(|| min_i18n::trc("ColorPanel", "Project default").to_string());
        let custom = self.colors.find(&stored.preset).map(|p| p.stored.chain != stored.chain).unwrap_or(true);
        View { target, has_clip: id.is_some(), own, stored, custom, on: self.settings.bool(crate::settings::k::COLOR_ENABLED), presets: self.colors.all() }
    }
    /// Bind what the drawer says to the engine's slot: off = bypass; minColor = the shared pass (a uniform update); OCIO = a
    /// pass built on a worker (the previous chain stays on screen until it is ready).
    pub(crate) fn apply_color(&mut self) {
        if harness_active() { return }
        let on = self.settings.bool(crate::settings::k::COLOR_ENABLED);
        let (_, stored, _) = self.color_target();
        let Some(g) = self.gpu.as_mut() else { return };
        if !on || self.player.is_none() {
            if self.color_bound.is_some() { g.presenter.engine.set_color_pass(None); self.color_bound = None; self.ocio_rx = None; }
            return
        }
        match stored.chain {
            ClipChain::MinColor(c) => {
                let Some(rc) = self.mincolor.clone() else { return };   // binds when the build lands
                rc.borrow_mut().set_chain(c);
                if self.color_bound.as_deref() != Some("minColor") { g.presenter.engine.set_color_pass(Some(Box::new(SharedPass(rc)))); self.color_bound = Some("minColor".into()); }
                self.ocio_rx = None;
            }
            ClipChain::Ocio(o) => {
                let key = format!("{}|{}|{}|{}|{:?}", o.config, o.input, o.look, o.view, o.knee);
                if self.color_bound.as_deref() == Some(key.as_str()) || self.ocio_rx.as_ref().is_some_and(|(k, _)| *k == key) { return }
                let cfg = if o.config.contains('/') || o.config.contains('\\') { o.config.clone() } else { ocio_configs_dir().join(&o.config).join("config.ocio").to_string_lossy().into_owned() };
                let spec = qcv_color::ocio::Spec { input: o.input, look: o.look, view: o.view, scene_lut: o.scene_lut };
                let (d, q, knee) = (g.presenter.engine.device.clone(), g.presenter.engine.queue.clone(), o.knee);
                let (tx, rx) = mpsc::channel();
                std::thread::Builder::new().name("ocio-build".into()).spawn(move || {
                    match qcv_color::ocio::Config::open(&cfg).and_then(|c| qcv_color::ocio::OcioPass::new(&d, &q, &c, spec, knee)) { Ok(p) => { let _ = tx.send(Box::new(p) as Box<dyn min_present::ColorPass + Send>); } Err(e) => log::error!("OCIO: {e}") }
                }).ok();
                self.ocio_rx = Some((key, rx));
            }
        }
    }
    /// The drawer's background builds landing (called every tick).
    pub(crate) fn poll_color(&mut self) {
        if let Some(rx) = &self.mincolor_ready && let Ok(p) = rx.try_recv() { self.mincolor = Some(Rc::new(RefCell::new(p))); self.mincolor_ready = None; self.apply_color(); }
        if let Some((key, rx)) = &self.ocio_rx && let Ok(p) = rx.try_recv() {
            let key = key.clone(); self.ocio_rx = None;
            if let Some(g) = self.gpu.as_mut() { g.presenter.engine.set_color_pass(Some(p)); self.color_bound = Some(key); g.ui_dirty = true; }
        }
    }
    fn write_target(&mut self, st: Stored) {
        match self.project.item(&self.loaded_item).cloned() {
            Some(item) => { let (oc, mc) = st.to_clip_maps(&item.ocio_clip, &item.min_color_clip); self.project.set_color_clip(&item.id, oc, mc); }
            None => { self.project.set_color_default(st.to_map()); }
        }
    }
    pub(crate) fn color_cmd(&mut self, c: DCmd) {
        use crate::settings::k;
        let now = std::time::Instant::now();
        match c {
            DCmd::Pick(st) | DCmd::Edit(st) => { self.settings.set_bool(k::COLOR_ENABLED, true); self.write_target(st); }
            DCmd::ResetToDefault => { let id = self.loaded_item.clone(); if self.project.item(&id).is_some() { self.project.set_color_clip(&id, Default::default(), Default::default()); } }
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
            DCmd::Toggle => { if let Some(g) = self.gpu.as_mut() { g.drawer.toggle(now); self.settings.set_bool(k::COLOR_DRAWER_OPEN, g.drawer.open); g.ui_dirty = true; } }
        }
        self.apply_color();
        if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; }
    }
}
