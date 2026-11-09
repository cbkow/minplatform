//! The STREAM card of the Output section (canon row 239, ruling 2; QCView `LeftRail.qml` 2318–2605), in the port's card: a
//! status dot, "Stream" and one word of state, an on / off switch (QCView's Start / Stop), one quiet line (the rate and the
//! counters while streaming; the state while connecting; the error or the settings' summary when off), then the server URL
//! and the key — always in view, the key hidden behind an eye — and QCView's six settings behind a disclosure, with its
//! defaults, ranges, option order and help texts. Everything but the switch and the eye is locked while a session runs.
//! Start commits a field still in edit (ruling 9).
use crate::output_panel::Cmd;
use crate::settings::k;
use kurbo::Rect;
use min_theme::tokens as t;
use min_ui::{paint, ActionButton, Event, Family, FieldResponse, Fonts, KeyEvent, NumberField, Picker, Response, Switch, TextField};
use std::time::Instant;
use vello::Scene;

fn tr(s: &str) -> &str { min_i18n::trc("LeftRail", s) }

pub const RESOLUTIONS: [&str; 4] = ["1080p", "720p", "540p", "2160p"];   // QCView's order
pub const FPS_MODES: [&str; 4] = ["auto", "24", "25", "30"];
pub const PROFILES: [&str; 2] = ["baseline", "main"];
pub const AUDIO_KBPS: [i64; 6] = [0, 128, 160, 192, 256, 320];

/// What the card shows (gathered by `stream::view`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct StreamView { pub available: bool, pub running: bool, /// 0 off · 1 streaming · 2 starting / connecting · 3 failed
    pub dot: u8, pub word: String, pub line: String, pub url: String, pub key: String, pub bitrate: i64, pub resolution: usize, pub fps: usize, pub profile: usize, pub audio: usize, pub latency: i64 }

const LABEL_W: f64 = 104.0;
const CONTROL_BG: min_theme::Color = t::GRAY_75;
const HELP: [(&str, &str); 6] = [
    ("Bitrate (kbps)", "Constant-rate target. Louper suggests 2500–8000 at 1080p."),
    ("Resolution", "The stream's raster; the viewport is letterboxed into it like any output."),
    ("Frame rate", "Auto takes the source's rate when the stream starts (30 when it has none) and keeps it for the session. A 23.976 source into a 30 fps clock repeats six frames a second."),
    ("H.264 profile", "Baseline is what Louper's recipe asks for; Main gives better quality per bit where the service accepts it. Never B-frames."),
    ("Audio (kbps)", "AAC stereo at 48 kHz from the mixed audio, before Listen, mute and volume. Off sends video only."),
    ("SRT latency (ms)", "The SRT retransmission window, both ends negotiate the larger. 120 ms suits a LAN or a good link; raise it on a lossy one."),
];
const CARD_HELP: &str = "Send the viewport and the mixed audio to a streaming server (Louper, or any SRT or RTMP server) as H.264 + AAC, encoded on the GPU. srt://host:port with the key as the stream id, or rtmp://host/app with the key as the stream name. A dropped connection reconnects on its own. The picture is the same Rec.709 SDR picture the output bus serves; the frame rate is fixed for the session (a paused viewport repeats its last frame).";

pub struct StreamCard {
    pub v: StreamView, on: Switch, url: TextField, key: TextField, reveal: bool, eye: ActionButton, more: ActionButton, expanded: bool,
    bitrate: NumberField, latency: NumberField, /// resolution · frame rate · profile · audio
    pickers: [Picker; 4],
    pub tile: Rect, dot: Rect, header: Rect, line: Rect, more_label: Rect, rows: Vec<(Rect, usize)>, scale: f64,
}

fn small(icon: &'static str, tip: &str) -> ActionButton { let mut b = ActionButton::new(icon, tip).size_s(); b.icon_size = Some(t::ICON_SIZE_SMALL); b }

impl StreamCard {
    pub fn new() -> StreamCard {
        let mut url = TextField::new(Family::Mono, t::CODE_SIZE_S); url.placeholder = "srt://host:port".into();
        let mut key = TextField::new(Family::Mono, t::CODE_SIZE_S).secret(); key.placeholder = tr("Stream key (SRT stream id)").into();
        let mut pickers = [
            Picker::new(RESOLUTIONS.iter().map(|s| s.to_string()).collect(), 0),
            Picker::new(vec![tr("Auto").into(), "24".into(), "25".into(), "30".into()], 0),
            Picker::new(vec![tr("Baseline").into(), tr("Main").into()], 0),
            Picker::new(AUDIO_KBPS.iter().map(|k| if *k == 0 { tr("Off").to_string() } else { k.to_string() }).collect(), 2),
        ];
        for p in &mut pickers { p.bg = Some(CONTROL_BG); }
        let (mut bitrate, mut latency) = (NumberField::new(6000, 500, 50000, 500), NumberField::new(120, 20, 5000, 20));
        bitrate.bg = Some(CONTROL_BG); latency.bg = Some(CONTROL_BG);
        StreamCard { v: StreamView::default(), on: Switch::new(false), url, key, reveal: false, eye: small("eye", tr("Show the key")), more: small("caret-right", ""), expanded: false, bitrate, latency, pickers,
            tile: Rect::ZERO, dot: Rect::ZERO, header: Rect::ZERO, line: Rect::ZERO, more_label: Rect::ZERO, rows: vec![], scale: 1.0 }
    }
    /// The language changed: the labels again, the state kept.
    pub fn relabel(&mut self, fonts: &mut Fonts) { let (v, reveal, expanded) = (self.v.clone(), self.reveal, self.expanded); *self = StreamCard::new(); self.expanded = expanded; if reveal { self.set_reveal(true, fonts); } self.sync(v, Instant::now(), fonts); }
    fn set_reveal(&mut self, on: bool, fonts: &mut Fonts) {
        if on == self.reveal { return }
        let text = self.key.text().to_string(); let focused = self.key.focused; let rect = self.key.rect;
        self.reveal = on;
        self.key = if on { TextField::new(Family::Mono, t::CODE_SIZE_S) } else { TextField::new(Family::Mono, t::CODE_SIZE_S).secret() };
        self.key.placeholder = tr("Stream key (SRT stream id)").into();
        if on { self.key.set_text_end(&text, fonts); } else { self.key.set_secret(&text); }
        self.key.focused = focused; self.key.rect = rect;
        self.eye.icon = if on { "eye-slash" } else { "eye" }; self.eye.tooltip = tr(if on { "Hide the key" } else { "Show the key" }).into();
    }
    pub fn sync(&mut self, v: StreamView, now: Instant, fonts: &mut Fonts) -> bool {
        let changed = v != self.v;
        self.on.set(v.running, now); self.on.enabled = v.available || v.running;
        if !self.url.focused && self.url.text() != v.url { self.url.set_text_end(&v.url, fonts); }
        if !self.key.focused && self.key.text() != v.key { if self.reveal { self.key.set_text_end(&v.key, fonts); } else { self.key.set_secret(&v.key); } }
        self.bitrate.set(v.bitrate); self.latency.set(v.latency);
        for (p, i) in self.pickers.iter_mut().zip([v.resolution, v.fps, v.profile, v.audio]) { p.selected = i; p.enabled = !v.running; }
        self.bitrate.enabled = !v.running; self.latency.enabled = !v.running;
        self.v = v;
        changed
    }
    pub fn menu_open(&self) -> bool { self.pickers.iter().any(|p| p.is_open()) }
    pub fn has_focus(&self) -> bool { self.url.focused || self.key.focused || self.bitrate.focused() || self.latency.focused() }
    pub fn field_hit(&self, x: f64, y: f64) -> bool { self.url.hit(x, y) || self.key.hit(x, y) || self.bitrate.hit(x, y) || self.latency.hit(x, y) }
    /// Commit whatever is in edit (a press elsewhere, Start, the rail losing focus).
    pub fn blur(&mut self) -> Vec<Cmd> {
        let mut cmds = vec![];
        if self.url.focused { self.url.focused = false; cmds.push(Cmd::StreamStr(k::STREAM_URL, self.url.text().trim().to_string())); }
        if self.key.focused { self.key.focused = false; cmds.push(Cmd::StreamStr(k::STREAM_KEY, self.key.text().trim().to_string())); }
        if let Some(v) = self.bitrate.blur() { cmds.push(Cmd::StreamInt(k::STREAM_BITRATE, v)); }
        if let Some(v) = self.latency.blur() { cmds.push(Cmd::StreamInt(k::STREAM_SRT_LATENCY, v)); }
        cmds
    }
    pub fn key(&mut self, e: &KeyEvent, fonts: &mut Fonts) -> Vec<Cmd> {
        let mut cmds = vec![];
        for (f, name) in [(&mut self.url, k::STREAM_URL), (&mut self.key, k::STREAM_KEY)] {
            if !f.focused { continue }
            match f.key(e, fonts) {
                FieldResponse::Accepted => { f.focused = false; cmds.push(Cmd::StreamStr(name, f.text().trim().to_string())); }
                FieldResponse::Cancelled => { f.focused = false; }   // the next sync puts the saved text back
                _ => {}
            }
        }
        if self.bitrate.focused() && let Some(v) = self.bitrate.key(e, fonts) { cmds.push(Cmd::StreamInt(k::STREAM_BITRATE, v)); }
        if self.latency.focused() && let Some(v) = self.latency.key(e, fonts) { cmds.push(Cmd::StreamInt(k::STREAM_SRT_LATENCY, v)); }
        cmds
    }
    pub fn ime(&mut self, e: &min_ui::ImeEvent, fonts: &mut Fonts) { for f in [&mut self.url, &mut self.key] { if f.focused { f.ime(e, fonts); } } if self.bitrate.focused() { self.bitrate.field.ime(e, fonts); } if self.latency.focused() { self.latency.field.ime(e, fonts); } }
    /// Lay the card out from `cy`; returns the y below it.
    pub fn layout(&mut self, fonts: &mut Fonts, x0: f64, x1: f64, mut cy: f64, scale: f64) -> f64 {
        self.scale = scale;
        let pad = t::SPACING_100 as f64 * scale; let gap = t::RAIL_ROW_GAP as f64 * scale; let row = t::RAIL_ROW_HEIGHT as f64 * scale; let ins = t::SPACING_200 as f64 * scale;
        let fh = t::FIELD_HEIGHT_S as f64 * scale;
        let top = cy; let (cx0, cx1) = (x0 + ins, x1 - ins); cy += ins * 0.75;
        let (sw, sh) = Switch::size(scale);
        self.on.rect = Rect::new(cx1 - sw, cy + (row - sh) / 2.0, cx1, cy + (row + sh) / 2.0);
        let d = 8.0 * scale; self.dot = Rect::new(cx0, cy + (row - d) / 2.0, cx0 + d, cy + (row + d) / 2.0);
        self.header = Rect::new(cx0 + d + 8.0 * scale, cy, self.on.rect.x0 - pad, cy + row); cy += row;
        self.line = if self.v.line.is_empty() { Rect::ZERO } else {
            let l = fonts.layout_width(&self.v.line, Family::Sans, t::DETAIL_SIZE_S * scale as f32, t::FONT_WEIGHT_REGULAR as f32, peniko::Color::WHITE, Some((cx1 - self.header.x0) as f32));
            let r = Rect::new(self.header.x0, cy, cx1, cy + l.height() as f64); cy += l.height() as f64 + 2.0 * scale; r };
        cy += gap;
        // the URL, then the key with its eye
        self.url.layout(fonts, Rect::new(cx0, cy + (row - fh) / 2.0, cx1, cy + (row + fh) / 2.0), scale); cy += row + gap;
        let (ew, eh) = self.eye.measure(fonts, scale);
        self.eye.rect = Rect::new(cx1 - ew, cy + (row - eh) / 2.0, cx1, cy + (row + eh) / 2.0);
        self.key.layout(fonts, Rect::new(cx0, cy + (row - fh) / 2.0, self.eye.rect.x0 - pad, cy + (row + fh) / 2.0), scale); cy += row + gap;
        // the disclosure
        self.more.icon = if self.expanded { "caret-down" } else { "caret-right" };
        let (mw, mh) = self.more.measure(fonts, scale);
        self.more.rect = Rect::new(cx0 - 4.0 * scale, cy + (row - mh) / 2.0, cx0 - 4.0 * scale + mw, cy + (row + mh) / 2.0);
        self.more_label = Rect::new(self.more.rect.x1 + 2.0 * scale, cy, cx1, cy + row); cy += row;
        self.rows.clear();
        for p in &mut self.pickers { p.rect = Rect::ZERO; }
        self.bitrate.rect = Rect::ZERO; self.latency.rect = Rect::ZERO;
        if self.expanded {
            cy += gap;
            let lw = LABEL_W * scale;
            for i in 0..6 {
                let r = Rect::new(cx0 + lw, cy, cx1, cy + row);
                match i {
                    0 => self.bitrate.layout(fonts, Rect::new(r.x0, r.y0 + (row - fh) / 2.0, r.x1, r.y0 + (row + fh) / 2.0), scale),
                    5 => self.latency.layout(fonts, Rect::new(r.x0, r.y0 + (row - fh) / 2.0, r.x1, r.y0 + (row + fh) / 2.0), scale),
                    _ => { let p = &mut self.pickers[i - 1]; let (_, ph) = p.measure(fonts, scale); p.rect = Rect::new(r.x0, r.y0 + (row - ph) / 2.0, r.x1, r.y0 + (row + ph) / 2.0); }
                }
                self.rows.push((Rect::new(cx0, cy, cx0 + lw, cy + row), i)); cy += row + gap;
            }
            cy -= gap;
        }
        cy += ins * 0.75;
        self.tile = Rect::new(x0, top, x1, cy);
        cy + gap * 2.0
    }
    /// An open picker takes every event first (the panel asks before anything else).
    pub fn menu_event(&mut self, e: &Event, now: Instant, fonts: &mut Fonts, bounds: Rect) -> Option<(Vec<Cmd>, bool)> {
        let s = self.scale;
        for (i, p) in self.pickers.iter_mut().enumerate() { if p.is_open() {
            let (pick, d, _) = p.event(e, now, fonts, bounds, s);
            return Some((pick.map(|j| pick_cmd(i, j)).into_iter().collect(), d || pick.is_some()))
        } }
        None
    }
    pub fn event(&mut self, e: &Event, now: Instant, fonts: &mut Fonts, bounds: Rect, cmds: &mut Vec<Cmd>) -> bool {
        let (mut dirty, s) = (false, self.scale);
        // a press outside a field in edit commits it
        if let Event::Down { x, y, .. } = *e { for hit in [self.url.hit(x, y), self.key.hit(x, y), self.bitrate.hit(x, y), self.latency.hit(x, y)].iter().enumerate().filter(|h| !*h.1).map(|h| h.0).collect::<Vec<_>>() {
            match hit {
                0 => if self.url.focused { self.url.focused = false; cmds.push(Cmd::StreamStr(k::STREAM_URL, self.url.text().trim().to_string())); dirty = true; },
                1 => if self.key.focused && !self.eye.hit(x, y) { self.key.focused = false; cmds.push(Cmd::StreamStr(k::STREAM_KEY, self.key.text().trim().to_string())); dirty = true; },
                2 => if let Some(v) = self.bitrate.blur() { cmds.push(Cmd::StreamInt(k::STREAM_BITRATE, v)); dirty = true; },
                _ => if let Some(v) = self.latency.blur() { cmds.push(Cmd::StreamInt(k::STREAM_SRT_LATENCY, v)); dirty = true; },
            }
        } }
        let (on, d) = self.on.event(e, now); dirty |= d;
        if let Some(on) = on { if on { cmds.extend(self.blur()); cmds.push(Cmd::StreamStart); } else { cmds.push(Cmd::StreamStop); } }
        match self.eye.event(e, now) { Response::Clicked => { let r = !self.reveal; self.set_reveal(r, fonts); self.key.layout(fonts, self.key.rect, s); dirty = true; } Response::None => {} _ => dirty = true }
        let on_label = matches!(e, Event::Down { x, y, .. } if self.more_label.contains(kurbo::Point::new(*x, *y)));
        match self.more.event(e, now) { Response::Clicked => { self.expanded = !self.expanded; dirty = true; } Response::None => if on_label { self.expanded = !self.expanded; dirty = true; }, _ => dirty = true }
        if !self.v.running {
            dirty |= self.url.pointer(e, fonts); dirty |= self.key.pointer(e, fonts);
            if self.expanded {
                for (i, p) in self.pickers.iter_mut().enumerate() { if p.rect.width() > 0.0 { let (pick, d, _) = p.event(e, now, fonts, bounds, s); dirty |= d; if let Some(j) = pick { cmds.push(pick_cmd(i, j)); } } }
                let (v, d) = self.bitrate.event(e, fonts); dirty |= d; if let Some(v) = v { cmds.push(Cmd::StreamInt(k::STREAM_BITRATE, v)); }
                let (v, d) = self.latency.event(e, fonts); dirty |= d; if let Some(v) = v { cmds.push(Cmd::StreamInt(k::STREAM_SRT_LATENCY, v)); }
            }
        }
        dirty
    }
    /// What the pointer is over, for the panel's tooltip: (key, rect, text).
    pub fn tooltip(&self, e: &Event) -> Option<(u64, Rect, String)> {
        let (x, y) = e.pos()?; let p = kurbo::Point::new(x, y);
        if self.eye.hovered() { return Some((100, self.eye.rect, self.eye.tooltip.clone())) }
        if self.dot.inflate(4.0, 4.0).contains(p) || self.header.contains(p) { return Some((101, self.dot.union(self.header), tr(CARD_HELP).to_string())) }
        self.rows.iter().find(|r| r.0.contains(p)).map(|(r, i)| (110 + *i as u64, *r, tr(HELP[*i].1).to_string()))
    }
    pub fn animating(&self, now: Instant) -> bool { self.on.animating(now) || self.eye.animating(now) || self.more.animating(now) || self.pickers.iter().any(|p| p.animating(now)) }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, now: Instant) {
        let small = t::DETAIL_SIZE_S * scale as f32; let lab = t::RAIL_LABEL_SIZE * scale as f32;
        let mid = |r: Rect, hgt: f64| r.y0 + (r.height() - hgt) / 2.0;
        paint::rounded(scene, self.tile, 6.0 * scale, min_ui::section::card_color());
        let dc = match self.v.dot { 1 => t::POSITIVE_VISUAL_COLOR, 2 => t::NOTICE_VISUAL_COLOR, 3 => t::NEGATIVE_VISUAL_COLOR, _ => t::GRAY_500 };
        paint::rounded(scene, self.dot, self.dot.width() / 2.0, dc);
        let mut x = self.header.x0;
        let l = fonts.layout(tr("Stream"), Family::Sans, lab, t::FONT_WEIGHT_BOLD as f32, paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT)); min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((x, mid(self.header, l.height() as f64)))); x += l.width() as f64 + 8.0 * scale;
        let p = fonts.layout_fit(&self.v.word, Family::Sans, small, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT), (self.header.x1 - x).max(10.0));
        min_ui::fonts::draw_layout(scene, &p, kurbo::Affine::translate((x, mid(self.header, p.height() as f64))));
        self.on.paint(scene, scale, now);
        if self.line.width() > 0.0 { let l = fonts.layout_width(&self.v.line, Family::Sans, small, t::FONT_WEIGHT_REGULAR as f32, paint::color(if self.v.dot == 3 { t::NEGATIVE_VISUAL_COLOR } else { t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT }), Some(self.line.width() as f32)); min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((self.line.x0, self.line.y0))); }
        self.url.paint(scene, fonts, scale); self.key.paint(scene, fonts, scale);
        self.eye.paint(scene, fonts, scale, now);
        self.more.paint(scene, fonts, scale, now);
        let l = fonts.layout(tr("Encoder settings"), Family::Sans, lab, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_LABEL_COLOR)); min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((self.more_label.x0, mid(self.more_label, l.height() as f64))));
        for (r, i) in &self.rows { let l = fonts.layout_fit(tr(HELP[*i].0), Family::Sans, lab, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_LABEL_COLOR), r.width() - 4.0 * scale); min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((r.x0, mid(*r, l.height() as f64)))); }
        if self.expanded {
            for p in &self.pickers { if p.rect.width() > 0.0 { p.paint(scene, fonts, scale, now); } }
            self.bitrate.paint(scene, fonts, scale); self.latency.paint(scene, fonts, scale);
        }
    }
    pub fn paint_menus(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64) { for p in &self.pickers { if p.is_open() { p.paint_menu(scene, fonts, scale); } } }
}

fn pick_cmd(picker: usize, choice: usize) -> Cmd {
    match picker {
        0 => Cmd::StreamStr(k::STREAM_RESOLUTION, RESOLUTIONS[choice.min(3)].into()),
        1 => Cmd::StreamStr(k::STREAM_FPS_MODE, FPS_MODES[choice.min(3)].into()),
        2 => Cmd::StreamStr(k::STREAM_PROFILE, PROFILES[choice.min(1)].into()),
        _ => Cmd::StreamInt(k::STREAM_AUDIO, AUDIO_KBPS[choice.min(5)]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// A picture of the card for the eye (`QCV_CARD_SNAP=<png> cargo test -p qcv stream_card -- --nocapture`), and the layout's
    /// invariants: the fields and the settings sit inside the tile, the settings only when disclosed.
    #[test]
    fn the_card_lays_out_inside_its_tile() {
        let mut fonts = Fonts::new(); let now = Instant::now();
        let mut panel = crate::output_panel::OutputPanel::new(); panel.section.expanded = true;
        let v = |running: bool, dot: u8, word: &str, line: &str| StreamView { available: true, running, dot, word: word.into(), line: line.into(), url: "srt://ingest.example:9000".into(), key: "abc123".into(), bitrate: 6000, resolution: 0, fps: 0, profile: 0, audio: 2, latency: 120 };
        panel.sync(vec![], v(false, 0, "", "6000 kbps  ·  1080p  ·  Auto fps  ·  Baseline  ·  AAC 160  ·  SRT 120 ms"), false, true, true, now, &mut fonts);
        let closed = panel.layout(&mut fonts, 0.0, 560.0, 0.0, 2.0);
        let c = &panel.stream;
        for r in [c.url.rect, c.key.rect, c.eye.rect, c.on.rect, c.more.rect] { assert!(r.width() > 0.0 && c.tile.contains(r.center()), "{r:?} in {:?}", c.tile); }
        assert!(c.rows.is_empty() && c.bitrate.rect.width() == 0.0);
        panel.stream.expanded = true;
        panel.sync(vec![], v(true, 1, "streaming", "6012 kbps  ·  23.976 fps  ·  1440 encoded, 12 repeated"), false, true, true, now, &mut fonts);
        let open = panel.layout(&mut fonts, 0.0, 560.0, 0.0, 2.0);
        let c = &panel.stream;
        assert!(open > closed && c.rows.len() == 6);
        for r in c.pickers.iter().map(|p| p.rect).chain([c.bitrate.rect, c.latency.rect]) { assert!(r.width() > 100.0 && c.tile.contains(r.center()), "{r:?}"); }
        // closed, the section's tile (its height less half the gap) is the header exactly: the hover row sits centred in it
        // (the walk 2026-11-09: the tile ended half a gap short, so the hover touched its bottom edge)
        { let mut p = crate::output_panel::OutputPanel::new(); p.section.expanded = false; let hgt = p.layout(&mut fonts, 0.0, 560.0, 0.0, 2.0); assert_eq!(hgt - t::RAIL_SECTION_GAP as f64 * 2.0 / 2.0, min_ui::Section::height(2.0)); }
        let Ok(path) = std::env::var("QCV_CARD_SNAP") else { return };
        let Ok(e) = min_present::engine::Engine::headless(wgpu::TextureFormat::Rgba16Float) else { return };
        let (w, h) = (560u32, open.ceil() as u32 + 8);
        let mut layer = min_ui::UiLayer::new(&e.device, w, h).unwrap();
        { let scene = layer.scene_mut(); scene.reset(); paint::rect(scene, Rect::new(0.0, 0.0, w as f64, h as f64), t::GRAY_50); panel.paint(scene, &mut fonts, 2.0, now, true); }
        layer.render(&e.device, &e.queue);
        let px = min_present::readback::read_rgba8(&e.device, &e.queue, layer.texture(), w, h);
        let file = std::fs::File::create(&path).unwrap();
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h); enc.set_color(png::ColorType::Rgba); enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&px).unwrap();
        // the section closed, its header hovered (the tile and the hover row must share a centre)
        panel.section.expanded = false;
        let hh = panel.layout(&mut fonts, 0.0, 560.0, 8.0, 2.0);
        let _ = panel.event(&Event::Move { x: 200.0, y: 8.0 + min_ui::Section::height(2.0) / 2.0 }, now, &mut fonts, Rect::new(0.0, 0.0, 560.0, 200.0));
        let later = now + std::time::Duration::from_secs(1);
        let (w, h) = (560u32, hh.ceil() as u32 + 16);
        layer.resize(&e.device, w, h);
        { let scene = layer.scene_mut(); scene.reset(); paint::rect(scene, Rect::new(0.0, 0.0, w as f64, h as f64), t::GRAY_50); panel.paint(scene, &mut fonts, 2.0, later, true); }
        layer.render(&e.device, &e.queue);
        let px = min_present::readback::read_rgba8(&e.device, &e.queue, layer.texture(), w, h);
        let file = std::fs::File::create(format!("{path}.closed.png")).unwrap();
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h); enc.set_color(png::ColorType::Rgba); enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&px).unwrap();
    }
}
