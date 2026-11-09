//! The Blender input (QCView `decode/blender_card.{h,cpp}` + WindowManager's `connectBlender` / `onBlenderCard` /
//! `onBlenderCardRemoved`, QCBridge 0.4; canon row 227 step 2b). No discovery on the network: the QCBridge HOST add-on writes a
//! card (`blender.json`) on this machine when a session starts; QCView, switched on (File ▸ Connect to Blender, or Settings ▸
//! Blender sessions — OFF at every launch, never stored), watches for it, opens an SRT LISTENER at the card's URL and the
//! replica's Blender calls in. The card going away ends the session (the item stays in the bin).
//!
//! Fixes over QCView (row 227 2b): ONE Blender item per session, updated in place when the card's URL changes (QCView deduped by
//! exact URL: a new port / passphrase orphaned the old item); a card's name renames the item only while the user hasn't renamed
//! it; a listener item opened while the input is off says why (QCView: a silent blank viewport); the card's status / rows
//! changing alone re-runs nothing (QCView's digest re-ran the colour chain and its toasts).
use crate::trw;
use serde_json::{Map, Value};
use std::path::PathBuf;
use std::time::{Duration, Instant};

const POLL: Duration = Duration::from_secs(2);   // QCView's 2-s poll (its directory watcher is the fast path; review machines' folders are local)

/// What the card says (QCView `BlenderCard`): the stream to listen on and the host's colour block for step 2c.
#[derive(Clone, Debug, PartialEq)]
pub struct Card { pub name: String, pub host: String, pub url: String, pub port: i64, pub status: String,
    /// `[level, key, msg]` rows: "error" ones show in red under the switch
    pub mismatches: Vec<(String, String, String)>, pub colour: Map<String, Value>, pub path: PathBuf }

/// QCView's card folders, first existing file wins: `$QCV_INPUTS_DIR`; macOS the machine's then the user's Application Support;
/// Windows `%ProgramData%\QCView\Inputs` then the user's `AppData\Roaming\QCView\QCView\Inputs`.
pub fn card_dirs() -> Vec<PathBuf> {
    let mut v = vec![];
    if let Some(d) = std::env::var_os("QCV_INPUTS_DIR") { v.push(PathBuf::from(d)); }
    #[cfg(target_os = "macos")] {
        v.push(PathBuf::from("/Library/Application Support/QCView/Inputs"));
        if let Some(h) = std::env::var_os("HOME") { v.push(PathBuf::from(h).join("Library/Application Support/QCView/Inputs")); }
    }
    #[cfg(windows)] {
        v.push(PathBuf::from(std::env::var_os("ProgramData").unwrap_or_else(|| "C:\\ProgramData".into())).join("QCView").join("Inputs"));
        if let Some(a) = std::env::var_os("APPDATA") { v.push(PathBuf::from(a).join("QCView").join("QCView").join("Inputs")); }
    }
    v
}

/// QCView `BlenderCard::poll`'s read: a JSON object with `kind == "blender"` and a non-empty `url` (`v` is not checked).
pub fn read_card() -> Result<Option<Card>, String> {
    let Some(path) = card_dirs().into_iter().map(|d| d.join("blender.json")).find(|p| p.is_file()) else { return Ok(None) };
    let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let v: Value = serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let o = v.as_object().ok_or_else(|| format!("{}: not an object", path.display()))?;
    if o.get("kind").and_then(Value::as_str) != Some("blender") { return Err(format!("{}: kind is not \"blender\"", path.display())) }
    let s = |k: &str| o.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let url = s("url"); if url.is_empty() { return Err(format!("{}: no url", path.display())) }
    let mismatches = o.get("mismatches").and_then(Value::as_array).map(|a| a.iter().filter_map(|r| { let r = r.as_array()?; Some((r.first()?.as_str()?.to_string(), r.get(1)?.as_str().unwrap_or("").to_string(), r.get(2)?.as_str().unwrap_or("").to_string())) }).collect()).unwrap_or_default();
    Ok(Some(Card { name: s("name"), host: s("host"), url, port: o.get("port").and_then(Value::as_i64).unwrap_or(0), status: s("status"), mismatches, colour: o.get("colour").and_then(Value::as_object).cloned().unwrap_or_default(), path }))
}

/// The input's session state (QCView `BlenderCard` + WindowManager's m_blender* fields).
pub struct BlenderInput {
    pub enabled: bool, pub card: Option<Card>, pub ended: bool,
    /// this session's Blender item (QCView matched by URL only — fix: one item, its URL updated in place)
    pub item: Option<String>, /// the name the card last gave the item (a user rename since then wins)
    card_name: Option<String>, /// a project replaced while on: the next card keeps the item in the bin (QCView `m_blenderCarry`)
    pub carry: bool,
    pub last_poll: Option<Instant>, logged_error: Option<String>,
    /// the colour block last applied, by item (re-applied only when IT changes — QCView re-ran on any card change)
    applied: Option<(String, String)>, /// the live row's Host chip (QCView "Host display %1 · %2"): (display, view, what was not reproduced — empty = all of it)
    pub chip: Option<(String, String, String)>,
}
pub enum Event { Arrived, Changed, Removed }

impl Default for BlenderInput { fn default() -> Self { Self::new() } }
impl BlenderInput {
    pub fn new() -> BlenderInput { BlenderInput { enabled: false, card: None, ended: false, item: None, card_name: None, carry: false, last_poll: None, logged_error: None, applied: None, chip: None } }
    /// The 2-s poll (an immediate one right after switching on).
    pub fn poll(&mut self, now: Instant) -> Option<Event> {
        if !self.enabled || self.last_poll.is_some_and(|t| now.duration_since(t) < POLL) { return None }
        self.last_poll = Some(now);
        match read_card() {
            Ok(Some(c)) => {
                self.logged_error = None;
                let ev = match &self.card { None => Some(Event::Arrived), Some(old) if *old != c => Some(Event::Changed), _ => None };
                self.card = Some(c); ev
            }
            Ok(None) => { if self.card.take().is_some() { Some(Event::Removed) } else { None } }
            Err(e) => { if self.logged_error.as_deref() != Some(&e) { log::warn!("blender card: {e}"); self.logged_error = Some(e); } None }
        }
    }
}

impl crate::App {
    /// File ▸ Connect to Blender / the Settings switch (QCView `connectBlender`): on → watch (and poll at once); on again → off.
    pub(crate) fn connect_blender(&mut self) {
        if self.blender.enabled { self.blender_disconnect(); self.toast(trw("Blender: not listening"), min_ui::ToastKind::Notice); return }
        self.blender.enabled = true; self.blender.ended = false; self.blender.carry = false;
        log::info!("blenderInput: watching {:?}", card_dirs());
        self.blender_tick(Instant::now());
        if self.blender.card.is_none() { self.toast(trw("Listening for a Blender session on this machine: start one from the QCBridge host panel"), min_ui::ToastKind::Notice); }
        if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; }
    }
    /// The one way out (QCView `blenderDisconnect`: the menu, the switch, deleting the clip): the Blender item closes if it is
    /// on screen, the watcher stops, the item leaves the project, the card is forgotten.
    pub(crate) fn blender_disconnect(&mut self) {
        let item = self.blender.item.take();
        self.blender.enabled = false; self.blender.card = None; self.blender.ended = false; self.blender.card_name = None; self.blender.applied = None; self.blender.chip = None;
        if let Some(id) = item {
            if self.loaded_item == id { self.stop_live(); self.loaded_item.clear(); if let Some(g) = self.gpu.as_mut() { g.presenter.engine.clear_frame(); } }
            self.project.remove_media_item(&id);
        }
        if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; }
    }
    /// A project replaced while on (QCView `mediaPoolLoaded` + `m_blenderCarry`): the card is announced again into the new project,
    /// the Blender item added without taking the screen from the clip the project restored.
    pub(crate) fn blender_project_replaced(&mut self) {
        if !self.blender.enabled { return }
        self.blender.item = None; self.blender.card_name = None; self.blender.card = None; self.blender.carry = true; self.blender.last_poll = None;
    }
    /// Per tick: the card's comings and goings; the Blender item deleted from the bin = disconnect (QCView's bin hook).
    pub(crate) fn blender_tick(&mut self, now: Instant) {
        if self.blender.enabled && self.blender.item.as_ref().is_some_and(|id| self.project.item(id).is_none()) {
            self.blender.item = None; self.blender_disconnect(); self.settings.set_bool(crate::settings::k::BLENDER_SESSIONS, false);
            self.toast(trw("Blender: not listening"), min_ui::ToastKind::Notice); return
        }
        match self.blender.poll(now) {
            Some(Event::Arrived) | Some(Event::Changed) => self.on_blender_card(),
            Some(Event::Removed) => self.on_blender_card_removed(),
            None => {}
        }
    }
    /// QCView `onBlenderCard`: the item (this session's, else the one with the card's URL, else a new one) carries the card's
    /// URL and name and becomes active — unless a project replace carried the input over while the project restored another clip.
    fn on_blender_card(&mut self) {
        let Some(c) = self.blender.card.clone() else { return };
        self.blender.ended = false;
        let name = if c.name.is_empty() { trw("Blender").to_string() } else { c.name.clone() };
        let known = self.blender.item.clone().filter(|id| self.project.item(id).is_some())
            .or_else(|| self.project.pool().iter().find(|i| i.media_type == qcv_project::MediaType::LiveStream && i.path == c.url).map(|i| i.id.clone()));
        let id = match known {
            Some(id) => { if self.project.item(&id).is_some_and(|i| i.path != c.url) { self.project.set_live_stream_url(&id, &c.url); if self.loaded_item == id { self.stop_live(); self.loaded_item.clear(); } } id }
            None => match self.project.add_live_stream(&c.url, &name) { Some(id) => id, None => return },
        };
        // the card's name, unless the user renamed the item since the card last named it
        let cur = self.project.item(&id).map(|i| i.name.clone()).unwrap_or_default();
        if cur != name && self.blender.card_name.as_deref().is_none_or(|n| n == cur) { self.project.rename_media_item(&id, &name); }
        self.blender.card_name = Some(name);
        self.blender.item = Some(id.clone());
        self.apply_stream_chain(&id, &c.colour);
        let carry = std::mem::take(&mut self.blender.carry);
        let streaming = self.live.as_ref().is_some_and(|l| l.item == id);
        if self.loaded_item != id { if !carry { log::info!("blenderInput: listening for {} on port {} ({})", c.host, c.port, qcv_player::stream_feed::redact(&c.url)); self.load_item(&id); } }
        else if !streaming { self.load_item(&id); }   // Stop → Start in Blender: listen again
        if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; }
    }
    /// QCView `onBlenderCardRemoved`: the session ended — the stream stops, the item stays in the bin.
    fn on_blender_card_removed(&mut self) {
        self.blender.ended = true;
        if self.blender.item.as_ref().is_some_and(|id| self.live.as_ref().is_some_and(|l| &l.item == id)) { self.stop_live(); }
        self.toast(trw("Blender session ended"), min_ui::ToastKind::Notice);
        if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; }
    }
    /// The Settings switch's status lines (QCView LeftRail): the state line (green when live), then the card's error rows in red.
    pub(crate) fn blender_status(&self) -> Vec<(String, u8)> {
        if !self.blender.enabled { return vec![] }
        let tr = |s: &str| min_i18n::trc("LeftRail", s).to_string();
        let live_here = self.blender.item.as_ref().and_then(|id| self.live.as_ref().filter(|l| &l.item == id));
        let mut v = vec![match (&self.blender.card, live_here) {
            (Some(c), Some(l)) if l.feed.facts().status == qcv_player::host_feed::Status::Live => (min_i18n::fill(&tr("Live from %1 on port %2"), &[&c.host, &c.port]), 1),
            (Some(c), Some(_)) => (min_i18n::fill(&tr("Listening for %1 on port %2"), &[&c.host, &c.port]), 0),
            (Some(c), None) => (min_i18n::fill(&tr("%1 is streaming on port %2 — select the Blender clip"), &[&c.host, &c.port]), 0),
            (None, _) if self.blender.ended => (tr("Blender session ended"), 0),
            (None, _) => (tr("No Blender session on this machine yet"), 0),
        }];
        if let Some(c) = &self.blender.card { for (lvl, _, msg) in &c.mismatches { if lvl == "error" { v.push((msg.clone(), 2)); } } }
        v
    }

    /// QCView `applyStreamChainFromLink` (the 2026-10-27 colour rule, by step), made automatic for the port (the user 2026-11-09:
    /// "since we know for this … we can do it automatically"): the wire is QCBridge's `pq-bt2020-100` — the scene in the host's
    /// working space BEFORE its view, PQ-packed with 1.0 = 100 nits. Per clip (the port's chain lives on the clip, its OCIO config
    /// too: no global config switch, no prompt): minColor → input Rec.2020 / PQ (100 nits = 1.0) + AgX at its Blender defaults on
    /// the Rec.2020 target (the user: "2020 is where we want to land"); a host view minColor can't render (Filmic, Standard,
    /// Khronos…) or an OCIO default → the bundled Blender 5.2 config, input `Rec.2100-PQ Scene`, the host's view and look. Colour
    /// management turns on; the host's exposure / gamma join while the viewer's are at their defaults; a chain the user gave the
    /// clip is never overwritten. The host's DISPLAY is not emulated (the port's one display, canon §18.6): a non-sRGB host
    /// display is said, with QCView's other facts, in one toast and the Host chip.
    pub(crate) fn apply_stream_chain(&mut self, id: &str, colour: &Map<String, Value>) {
        use crate::color_model::{ClipChain, OcioChain, Stored};
        let digest = serde_json::to_string(colour).unwrap_or_default();
        if self.blender.applied.as_ref().is_some_and(|(i, d)| i == id && *d == digest) { return }
        self.blender.applied = Some((id.to_string(), digest));
        let item = |k: &str| colour.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
        let wtr = |s: &str| min_i18n::trc("qcv::WindowManager", s).to_string();
        let wire = item("wire");
        if wire.is_empty() { return }
        if wire != "pq-bt2020-100" {
            log::warn!("blender: stream wire '{wire}' is not one QCView knows; chain left as is");
            self.toast(&min_i18n::fill(&wtr("The stream names a wire this QCView does not know (%1); set the Input yourself"), &[&wire]), min_ui::ToastKind::Notice);
            self.blender.chip = None; return
        }
        let (view, look, display) = (item("view"), item("look"), item("display"));
        let look = if look == "None" { String::new() } else { look };
        let mut notes: Vec<String> = vec![];
        let (_, _, own) = self.chain_of(id);
        if own { log::info!("blender: the clip carries its own chain; the card leaves it"); }
        else {
            let default_is_mc = !self.color_default().chain.is_ocio();
            let agx = view.is_empty() || view == "AgX";
            let st = if default_is_mc && agx && look.is_empty() {
                let mut c = qcv_color::Chain::default();
                c.in_gamut = qcv_color::ids::IN_REC2020; c.in_transfer = 14 /* DRT_OETF_PQ_100: ST 2084, 100 nits = 1.0 */; c.in_limited = 0;
                c.agx_on = 1; c.agx_target = 0 /* Rec.2020 */;
                Stored { chain: ClipChain::MinColor(c), preset: "Blender stream".into() }
            } else {
                if default_is_mc { notes.push(min_i18n::fill(&wtr("host view %1: shown through the Blender config (OCIO)"), &[&view])); }
                let mut o = OcioChain { config: "Blender5.2".into(), input: "Rec.2100-PQ Scene".into(), look: look.clone(), view: view.clone(), scene_lut: String::new(), knee: Default::default() };
                if let Some(cfg) = self.ocio_config("Blender5.2") {
                    if !cfg.inputs().contains(&o.input) { notes.push(min_i18n::fill(&wtr("no %1 input in this config"), &[&o.input])); }
                    if !o.view.is_empty() && !cfg.views().contains(&o.view) { notes.push(min_i18n::fill(&wtr("view %1 not on %2"), &[&o.view, &"sRGB"])); o.view.clear(); }
                    if !o.look.is_empty() && !cfg.looks().contains(&o.look) { notes.push(min_i18n::fill(&wtr("look %1 not in this config"), &[&o.look])); o.look.clear(); }
                }
                Stored { chain: ClipChain::Ocio(o), preset: "Blender stream".into() }
            };
            if let Some(m) = self.project.item(id).cloned() { let (oc, mc) = st.to_clip_maps(&m.ocio_clip, &m.min_color_clip); self.project.set_color_clip(id, oc, mc); }
        }
        if !self.settings.bool(crate::settings::k::COLOR_ENABLED) {
            self.settings.set_bool(crate::settings::k::COLOR_ENABLED, true);
            log::info!("blender: colour management turned on for the stream");
            self.toast(&wtr("Colour management turned on for the Blender stream"), min_ui::ToastKind::Positive);
        }
        // the host's exposure / gamma, while the viewer's are at their defaults (QCView)
        if let Some(g) = self.gpu.as_mut() {
            let mut a = g.drawer.aids; let mut changed = false;
            if let Ok(e) = item("exposure").parse::<f32>() && a.exposure.abs() < 1e-4 && e.abs() > 1e-4 { a.exposure = e; changed = true; }
            if let Ok(gm) = item("gamma").parse::<f32>() && (a.gamma - 1.0).abs() < 1e-4 && gm > 0.0 && (gm - 1.0).abs() > 1e-4 { a.gamma = gm; changed = true; }
            if changed { g.drawer.set_aids(a); g.presenter.engine.set_viewer_aids(a); }
        }
        // what the host does that QCView can't, one plain sentence each
        if !display.is_empty() && !display.to_lowercase().contains("srgb") { notes.push(min_i18n::fill(&wtr("host display %1 not emulated"), &[&display])); }
        if item("wb") == "1" { notes.push(wtr("host white balance not reproduced")); }
        if item("curves") == "1" { notes.push(wtr("host curves not reproduced")); }
        if item("hdr") == "1" { notes.push(wtr("host HDR view: shown SDR")); }
        if item("ws_assumed") == "1" { notes.push(wtr("working space assumed Linear Rec.709")); }
        if item("cm_ok") == "0" { notes.push(min_i18n::fill(&wtr("host and replica colour setups differ (%1)"), &[&item("cm_n")])); }
        log::info!("blender: stream colour — view {view}, look {look}, display {display}, notes [{}]", notes.join(" | "));
        if !notes.is_empty() { self.toast(&min_i18n::fill(&wtr("Blender stream: %1"), &[&notes.join(" · ")]), min_ui::ToastKind::Notice); }
        let shown_view = if view.is_empty() { "AgX".to_string() } else { view };
        self.blender.chip = Some((if display.is_empty() { "sRGB".to_string() } else { display }, shown_view, notes.join("\n")));
        self.apply_color();
        if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; }
    }
}
