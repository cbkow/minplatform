//! QCView's LiveStrip (`ui/qml/LiveStrip.qml`, canon row 228): while a live feed is the single view (and the window is not
//! fullscreen / compact) it REPLACES the status strip, the transport row and the scrubber with one row. Left to right: the
//! dot (red, pulsing, live; amber connecting / paused), the badge (LIVE · PAUSED · WAITING · OFFLINE), the item's name, the
//! status detail (amber, when not live) or the facts ("W×H · Mercury Transmit · shared memory · RGBA16F"), "inf/NaN in
//! frame", fps · elapsed (live), "%n restart(s)"; at the right the Transfer chip and the Color / Notes / screenshot buttons.
//! Fix 5 (row 228): the screenshot buttons only with a frame on screen.
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use min_ui::{paint, ActionButton, ActionGroup, Event, Family, Fonts, Menu, MenuItem, Response, TextButton, Tooltips, Variant};
use qcv_player::host_feed::{Facts, Status};
use qcv_project::TransferOverride;
use std::time::Instant;
use vello::Scene;

fn tr(s: &str) -> &str { min_i18n::trc("LiveStrip", s) }

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cmd { Transfer(TransferOverride), Listen, Color, Notes, Shot(crate::capture::Dest) }

/// The Auto label for a QCBridge host feed (the user 2026-11-09, diverging from QCView's "Auto · SDR, untagged"): the Transmit
/// device asks the host for its WORKING colour space and never names it (the ring's ICC region stays empty), so "SDR"
/// was a guess — a 32-bpc linear comp is not SDR. QCView's tag rule (LiveStrip.qml autoLabel) returns with SRT (step 2).
pub const AUTO: &str = "Auto · working space, untagged";
const AUTO_TIP: &str = "The host sends its working color space without naming it; choose the transfer it uses (Transfer override)";
const TRANSFERS: [(TransferOverride, &str); 6] = [(TransferOverride::Auto, "Auto"), (TransferOverride::SdrRec709, "SDR 709"), (TransferOverride::Pq2020, "PQ 2020"), (TransferOverride::PqP3D65, "PQ P3"), (TransferOverride::Hlg, "HLG"), (TransferOverride::Linear, "Linear")];

pub struct LiveStrip {
    pub rect: Rect,
    transfer: TextButton, buttons: ActionGroup, menu: Menu, tips: Tooltips,
    facts: Option<Facts>, name: String, app: String, /// QCView's REVIEW: the freeze's label ("HH:mm:ss")
    review: Option<String>, override_: TransferOverride, live_seconds: f64,
    /// QCView's 1-s timer: fps = frames taken per second
    fps: (u64, Instant, f64),
    t0: Instant, scale: f64,
}

impl LiveStrip {
    pub fn new() -> LiveStrip {
        let b = |icon: &'static str, tip: &str| ActionButton::new(icon, tip);
        let mac = cfg!(target_os = "macos");
        // Listen (the user 2026-11-09): the live row owns it — one place, shown only while a feed is live; off by default (the host
        // already plays the audio); the Settings row and QCView's Inspector switch are retired
        let buttons = ActionGroup::new(vec![b("speaker-x", min_i18n::trc("InspectorPanel", "Listen in QCView")), b("palette", tr("Color panel (⌃3)")), b("note-pencil", tr("Notes panel (⌃4)")),
            b("file-image", tr(if mac { "Screenshot to clipboard (⌘T)" } else { "Screenshot to clipboard (Ctrl+T)" })), b("file-arrow-down", tr("Screenshot to Desktop (T)"))]).compact();
        let now = Instant::now();
        LiveStrip { rect: Rect::ZERO, transfer: TextButton::new("", Variant::Secondary).size_s(), buttons, menu: Menu::new(), tips: Tooltips::default(), facts: None, name: String::new(), app: String::new(), review: None, override_: TransferOverride::Auto, live_seconds: 0.0, fps: (0, now, 0.0), t0: now, scale: 1.0 }
    }
    /// A language switch: the buttons' words.
    pub fn relabel(&mut self) { let f = Self::new(); for (b, n) in self.buttons.buttons.iter_mut().zip(f.buttons.buttons) { b.tooltip = n.tooltip; } }
    /// Per tick: the feed's facts, the item's name and Transfer choice. True when what is shown changed.
    pub fn sync(&mut self, facts: Facts, name: &str, app: &str, transfer: TransferOverride, live_seconds: f64, listen: bool, review: Option<&str>, now: Instant) -> bool {
        let review_changed = self.review.as_deref() != review; self.review = review.map(str::to_string);
        let lb = &mut self.buttons.buttons[0];
        let listen_changed = lb.selected != listen;
        lb.selected = listen; lb.icon = if listen { "speaker-high" } else { "speaker-x" };
        if now.duration_since(self.fps.1).as_secs_f64() >= 1.0 { let dt = now.duration_since(self.fps.1).as_secs_f64(); self.fps = (facts.frames, now, (facts.frames.saturating_sub(self.fps.0)) as f64 / dt); }
        let label = if transfer == TransferOverride::Auto { tr(AUTO).to_string() } else { tr(TRANSFERS.iter().find(|(o, _)| *o == transfer).map(|(_, l)| *l).unwrap_or("Auto")).to_string() };
        let changed = review_changed || listen_changed || self.facts.as_ref() != Some(&facts) || self.name != name || self.override_ != transfer || self.transfer.label != label || (self.live_seconds as u64) != (live_seconds as u64);
        self.app = app.to_string(); self.transfer.label = label; self.override_ = transfer; self.name = name.to_string(); self.live_seconds = live_seconds;
        let shots = facts.has_frame;
        for i in [3, 4] { self.buttons.buttons[i].enabled = shots; }   // fix 5: nothing to capture before a frame

        self.facts = Some(facts);
        changed
    }
    pub fn layout(&mut self, fonts: &mut Fonts, rect: Rect, scale: f64) {
        self.rect = rect; self.scale = scale;
        let pad = t::SPACING_200 as f64 * scale; let bh = t::ACTION_BUTTON_HEIGHT_M as f64 * scale;
        let by = rect.y0 + (rect.height() - bh) / 2.0;
        let gw = { self.buttons.layout(fonts, 0.0, by, scale) };
        let gx = rect.x1 - pad - gw; self.buttons.layout(fonts, gx, by, scale);
        let (tw, th) = self.transfer.measure(fonts, scale);
        let ty = rect.y0 + (rect.height() - th) / 2.0;
        self.transfer.rect = Rect::new(gx - t::SPACING_300 as f64 * scale - tw, ty, gx - t::SPACING_300 as f64 * scale, ty + th);
    }
    /// The status row's animation: the live dot pulses (QCView: 1 → 0.35 over 900 ms each way).
    pub fn animating(&self) -> bool { (self.review.is_none() && self.facts.as_ref().is_some_and(|f| f.status == Status::Live)) || self.tips.next_change(Instant::now()).is_some() }
    pub fn event(&mut self, e: &Event, now: Instant, fonts: &mut Fonts, bounds: Rect) -> (Vec<Cmd>, bool, bool) {
        let mut cmds = vec![]; let mut dirty = false;
        if self.menu.is_open() {
            let (pick, d, consumed) = self.menu.event(e); dirty |= d;
            if let Some(i) = pick && let Some((o, _)) = TRANSFERS.get(i) { cmds.push(Cmd::Transfer(*o)); }
            return (cmds, dirty, consumed)
        }
        let (clicked, d) = self.transfer.event(e, now); dirty |= d;
        if clicked {
            let items: Vec<MenuItem> = TRANSFERS.iter().map(|(o, l)| { let label = if *o == TransferOverride::Auto { tr(AUTO).to_string() } else { tr(l).to_string() }; MenuItem::Action { label, enabled: true, shortcut: String::new() } }).collect();
            let r = self.transfer.rect; self.menu.open_at(items, fonts, r.x0, r.y0, bounds, self.scale); dirty = true;
        }
        for (i, resp) in self.buttons.event(e, now) {
            dirty = true;
            if resp == Response::Clicked { cmds.push(match i { 0 => Cmd::Listen, 1 => Cmd::Color, 2 => Cmd::Notes, 3 => Cmd::Shot(crate::capture::Dest::Clipboard), _ => Cmd::Shot(crate::capture::Dest::File) }); }
        }
        if matches!(e, Event::Down { .. }) { self.tips.pressed(now); }
        let over = if self.transfer.rect.contains(e.pos().map(|(x, y)| kurbo::Point::new(x, y)).unwrap_or_default()) && !matches!(e, Event::Leave) { Some((99u64, self.transfer.rect, tr(AUTO_TIP).to_string())) }
            else { self.buttons.hovered().map(|i| (i as u64, self.buttons.buttons[i].rect, self.buttons.buttons[i].tooltip.clone())) };
        if self.tips.hover(over.as_ref().map(|(k, r, s)| (*k, *r, s.as_str())), now) { dirty = true; }
        let inside = e.pos().is_some_and(|(x, y)| self.rect.contains(kurbo::Point::new(x, y)));
        (cmds, dirty, inside)
    }
    pub fn tick(&mut self, now: Instant) -> bool { self.tips.tick(now) }
    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, now: Instant) {
        let s = self.scale; let r = self.rect;
        paint::rect(scene, r, t::BAND_BACKGROUND_COLOR);
        paint::hline(scene, 0.0, r.x1, r.y0, t::DIVIDER_THICKNESS as f64 * s, t::DIVIDER_COLOR);
        let Some(f) = self.facts.as_ref() else { return };
        let amber = min_theme::Color::rgba(0xe6 as f32 / 255.0, 0xa2 as f32 / 255.0, 0x3c as f32 / 255.0, 1.0);
        let red = min_theme::Color::rgba(0xe5 as f32 / 255.0, 0x48 as f32 / 255.0, 0x4d as f32 / 255.0, 1.0);
        let mid = |h: f64| r.y0 + (r.height() - h) / 2.0;
        let mut x = r.x0 + t::SPACING_300 as f64 * s;
        // the dot: red pulsing live; amber connecting / reconnecting / paused
        // Review: amber, steady (QCView: the pulse runs only live and not in Review)
        let (dot, alpha) = if self.review.is_some() { (amber, 1.0) } else { match f.status { Status::Live => { let ph = (now.duration_since(self.t0).as_secs_f64() % 1.8) / 0.9; let k = if ph < 1.0 { ph } else { 2.0 - ph }; (red, (1.0 - 0.65 * k) as f32) } _ => (amber, 1.0) } };
        let d = 8.0 * s; paint::rounded(scene, Rect::new(x, mid(d), x + d, mid(d) + d), d / 2.0, dot.with_alpha(alpha)); x += d + t::SPACING_200 as f64 * s;
        // the badge (shared memory: never CONNECTING / RECONNECTING — WAITING)
        let review_badge = self.review.as_ref().map(|l| min_i18n::fill(tr("REVIEW · %1"), &[l]));
        let badge = if let Some(b) = review_badge.as_deref() { b } else { match f.status { Status::Live => tr("LIVE"), Status::Paused => tr("PAUSED"), Status::Connecting | Status::Reconnecting => tr("WAITING") } };
        let bl = fonts.detail(badge, t::RAIL_GROUP_HEADER_SIZE * s as f32, paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT));
        min_ui::fonts::draw_layout(scene, &bl, Affine::translate((x, mid(bl.height() as f64)))); x += bl.width() as f64 + t::SPACING_300 as f64 * s;
        let size = t::RAIL_LABEL_SIZE * s as f32;
        let nl = fonts.layout_fit(&self.name, Family::Sans, size, t::FONT_WEIGHT_BOLD as f32, paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT), 220.0 * s);
        min_ui::fonts::draw_layout(scene, &nl, Affine::translate((x, mid(nl.height() as f64)))); x += nl.width() as f64 + t::SPACING_300 as f64 * s;
        let right = self.transfer.rect.x0 - t::SPACING_300 as f64 * s;
        // the detail (amber) when not live, else the facts and the rates
        let mut parts: Vec<(String, min_theme::Color)> = vec![];
        match (&f.detail, f.status) {
            (Some((key, arg)), st) if st != Status::Live => parts.push((detail_text(key, arg, &self.app), amber)),
            _ => {
                if f.width > 0 { parts.push((format!("{}×{}  ·  Mercury Transmit  ·  {}  ·  RGBA16F", f.width, f.height, tr("shared memory")), t::RAIL_LABEL_COLOR)); }
                if f.status == Status::Live { let el = self.live_seconds as u64; let elapsed = if el >= 3600 { format!("{}:{:02}:{:02}", el / 3600, el / 60 % 60, el % 60) } else { format!("{}:{:02}", el / 60, el % 60) }; parts.push((format!("{:.0} fps  ·  {elapsed}", self.fps.2), t::RAIL_LABEL_COLOR)); }
                if let Some((key, arg)) = &f.detail { parts.push((detail_text(key, arg, &self.app), amber)); }   // an unaccepted frame while live (fix 4)
            }
        }
        if f.non_finite && matches!(f.status, Status::Live | Status::Paused) { parts.push((tr("inf/NaN in frame").to_string(), red)); }
        if f.restarts > 0 { parts.push((min_i18n::trnc(Some("LiveStrip"), "%n restart(s)", f.restarts as i64), amber)); }
        for (text, c) in parts {
            if x >= right { break }
            let l = fonts.layout_fit(&text, Family::Sans, size, t::FONT_WEIGHT_REGULAR as f32, paint::color(c), right - x);
            min_ui::fonts::draw_layout(scene, &l, Affine::translate((x, mid(l.height() as f64)))); x += l.width() as f64 + t::SPACING_400 as f64 * s;
        }
        self.transfer.paint(scene, fonts, s, now);
        self.buttons.paint(scene, fonts, s, now);
    }
    pub fn paint_overlays(&self, scene: &mut Scene, fonts: &mut Fonts, bounds: Rect, now: Instant) { self.menu.paint(scene, fonts, self.scale); self.tips.paint(scene, fonts, self.scale, bounds, now); }
}

/// A feed detail key (host_feed's English) in the language, its argument filled (the host app's name is a proper noun;
/// UNSUPPORTED carries the frame's description as %2).
pub fn detail_text(key: &str, arg: &str, app: &str) -> String {
    let t = min_i18n::trc("qcv::HostBridgeSource", key);
    if key == qcv_player::host_feed::UNSUPPORTED { min_i18n::fill(t, &[&app, &arg]) } else { min_i18n::fill(t, &[&arg]) }
}
