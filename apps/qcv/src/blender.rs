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
    /// listener URLs the USER opened this session (Open Stream, a launch argument, a recent item, a link): they listen
    /// without the switch (canon row 243) — the gate is for the sessions the Blender card brings
    pub user_listeners: std::collections::HashSet<String>,
}
pub enum Event { Arrived, Changed, Removed }

impl Default for BlenderInput { fn default() -> Self { Self::new() } }
impl BlenderInput {
    pub fn new() -> BlenderInput { BlenderInput { enabled: false, card: None, ended: false, item: None, card_name: None, carry: false, last_poll: None, logged_error: None, user_listeners: Default::default() } }
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
        self.blender.enabled = false; self.blender.card = None; self.blender.ended = false; self.blender.card_name = None;
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
        let carry = std::mem::take(&mut self.blender.carry);
        // the session's feed, in the single view or on a dual side (row 258, R10): a side that is already listening stays put
        let streaming = self.live_feed_of(&id).is_some();
        if streaming && self.loaded_item != id { /* it is dual's B: nothing to load */ }
        else if self.loaded_item != id { if !carry { log::info!("blenderInput: listening for {} on port {} ({})", c.host, c.port, qcv_player::stream_feed::redact(&c.url)); self.load_item(&id); } }
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
        let live_here = self.blender.item.as_ref().and_then(|id| self.live_feed_of(id));   // the single view's, or a dual side's
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
}
