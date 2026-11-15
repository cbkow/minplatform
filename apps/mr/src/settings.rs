//! The Settings card (the spec's 1.11): the sync root, the tags, the network, the farm's secret as seen from here,
//! staging, the path mappings. Nothing changes until Save (row 389); Save writes the file whole and the application
//! starts itself again with the new settings - the one way they all take effect alike.
use crate::node::Node;
use kurbo::{Affine, Rect, RoundedRect};
use min_theme::tokens as t;
use min_ui::{fonts::{draw_layout, Fonts}, paint, Button, Event, Family, Key, KeyEvent, Named, Switch, TextButton, TextField, Variant};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use vello::Scene;

struct Mapping { label: TextField, win: TextField, mac: TextField, on: Switch, remove: TextButton, /// what the file holds for Linux (not shown: no Linux build yet), kept as it is
    lin: String }
pub struct Settings {
    root: TextField, browse: TextButton, tags: TextField, port: TextField, ip: TextField, udp: Switch, udp_port: TextField, staging: Switch, maps: Vec<Mapping>, add: TextButton,
    focus: usize, cancel: TextButton, save: TextButton, error: Option<String>, secret: (String, usize), picked: Arc<Mutex<Option<String>>>,
}
pub enum Out { None, Close, /// saved: the application is to start itself again
    Saved }
fn field(text: &str, placeholder: &str, fonts: &mut Fonts) -> TextField { let mut f = TextField::new(Family::Sans, t::BODY_SIZE_S); f.placeholder = placeholder.into(); f.set_text_end(text, fonts); f }
fn mapping(m: &Value, fonts: &mut Fonts) -> Mapping { let s = |k: &str| m[k].as_str().unwrap_or("").to_string(); Mapping { label: field(&s("label"), "Name", fonts), win: field(&s("win"), "P:\\ or \\\\server\\share", fonts), mac: field(&s("mac"), "/Volumes/Projects", fonts), on: Switch::new(m["enabled"].as_bool().unwrap_or(true)), remove: TextButton::new("Remove", Variant::Secondary).size_s(), lin: s("lin") } }

impl Settings {
    pub fn open(node: &Node, fonts: &mut Fonts) -> Settings {
        let c = node.settings(); let s = |k: &str| c[k].as_str().unwrap_or("").to_string(); let n = |k: &str, d: u64| c[k].as_u64().unwrap_or(d).to_string();
        let tags = c["tags"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default();
        let mut me = Settings { root: field(&s("sync_root"), "e.g. \\\\server\\share\\minRender", fonts), browse: TextButton::new("Browse", Variant::Secondary).size_s(), tags: field(&tags, "gpu, gpu-fast, workstation", fonts), port: field(&n("http_port", 8420), "", fonts), ip: field(&s("ip_override"), "auto-detect", fonts),
            udp: Switch::new(c["udp_enabled"].as_bool().unwrap_or(true)), udp_port: field(&n("udp_port", 4243), "", fonts), staging: Switch::new(c["staging_enabled"].as_bool().unwrap_or(false)), maps: c["path_mappings"].as_array().map(|a| a.iter().map(|m| mapping(m, fonts)).collect()).unwrap_or_default(),
            add: TextButton::new("Add Mapping", Variant::Secondary).size_s(), focus: 0, cancel: TextButton::new("Cancel", Variant::Secondary), save: TextButton::new("Save", Variant::Primary), error: None, secret: node.secret_state(), picked: Arc::new(Mutex::new(None)) };
        me.set_focus(0); me
    }
    fn fields(&mut self) -> Vec<&mut TextField> { let mut v: Vec<&mut TextField> = vec![&mut self.root, &mut self.tags, &mut self.port, &mut self.ip, &mut self.udp_port]; for m in self.maps.iter_mut() { v.push(&mut m.label); v.push(&mut m.win); v.push(&mut m.mac); } v }
    fn set_focus(&mut self, to: usize) { self.focus = to; for (i, f) in self.fields().into_iter().enumerate() { f.focused = i == to; } }
    /// What the card holds, as the file's keys - or what is wrong with it.
    fn changes(&self) -> Result<Value, String> {
        let port = |f: &TextField, what: &str| f.text().trim().parse::<u16>().ok().filter(|p| *p > 0).ok_or(format!("{what} must be a number from 1 to 65535"));
        let tags: Vec<String> = self.tags.text().split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
        let maps: Vec<Value> = self.maps.iter().map(|m| json!({ "label": m.label.text().trim(), "win": m.win.text().trim(), "mac": m.mac.text().trim(), "lin": m.lin, "enabled": m.on.on })).filter(|m| !(m["win"] == "" && m["mac"] == "" && m["lin"] == "")).collect();
        Ok(json!({ "sync_root": self.root.text().trim(), "tags": tags, "http_port": port(&self.port, "HTTP port")?, "ip_override": self.ip.text().trim(), "udp_enabled": self.udp.on, "udp_port": port(&self.udp_port, "UDP port")?, "staging_enabled": self.staging.on, "path_mappings": maps }))
    }
    /// (scripted looks) Save, as a click on it.
    pub fn demo_save(&mut self, node: &Node) -> Out { self.staging.on = true; self.commit(node) }
    fn commit(&mut self, node: &Node) -> Out { match self.changes().and_then(|c| node.save_settings(&c)) { Ok(()) => Out::Saved, Err(e) => { self.error = Some(e); Out::None } } }
    pub fn pointer(&mut self, e: &Event, fonts: &mut Fonts, node: &Arc<Node>) -> Out {
        let now = Instant::now();
        if self.cancel.event(e, now).0 { return Out::Close } if self.save.event(e, now).0 { return self.commit(node) }
        if self.browse.event(e, now).0 { let (picked, at) = (self.picked.clone(), self.root.text().to_string()); std::thread::spawn(move || { let mut d = rfd::FileDialog::new().set_title("Select sync root"); if std::path::Path::new(&at).is_dir() { d = d.set_directory(&at); } if let Some(p) = d.pick_folder() { *picked.lock().unwrap() = Some(p.to_string_lossy().into_owned()); } }); return Out::None }
        if self.add.event(e, now).0 { self.maps.push(mapping(&json!({}), fonts)); let n = self.fields().len(); self.set_focus(n - 3); return Out::None }
        self.udp.event(e, now); self.staging.event(e, now);
        let mut gone = None; for (i, m) in self.maps.iter_mut().enumerate() { m.on.event(e, now); if m.remove.event(e, now).0 { gone = Some(i); } } if let Some(i) = gone { self.maps.remove(i); self.set_focus(0); return Out::None }
        if let Event::Down { x, y, button: Button::Primary } = *e { let hit = self.fields().iter().position(|f| f.hit(x, y)); if let Some(i) = hit { self.set_focus(i); } }
        let focus = self.focus; if let Some(f) = self.fields().into_iter().nth(focus) { f.pointer(e, fonts); }
        Out::None
    }
    pub fn key(&mut self, k: &KeyEvent, pasted: Option<String>, fonts: &mut Fonts) {
        let n = self.fields().len();
        match &k.key { Key::Named(Named::Tab) => { let to = if k.shift { (self.focus + n - 1) % n } else { (self.focus + 1) % n }; self.set_focus(to); if let Some(f) = self.fields().into_iter().nth(to) { f.select_all(fonts); } }
            Key::Named(Named::Enter) | Key::Named(Named::Escape) => {}
            _ => { let focus = self.focus; if let Some(f) = self.fields().into_iter().nth(focus) { match pasted { Some(text) => { f.paste(text.lines().next().unwrap_or(""), fonts); } None => { let _ = f.key(k, fonts); } } } } }
    }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, view: Rect, s: f64, now: Instant) {
        if let Some(p) = self.picked.lock().unwrap().take() { self.root.set_text_end(&p, fonts); }
        let c = paint::color; let pad = t::DIALOG_PADDING as f64 * s; let (ink, sub) = (c(t::NEUTRAL_CONTENT_COLOR_DEFAULT), c(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT)); let (row, fh) = (38.0 * s, 30.0 * s);
        let body = 6.0 * 26.0 * s + 5.0 * row + 3.0 * 22.0 * s + self.maps.len() as f64 * row + 34.0 * s + 24.0 * s + if self.error.is_some() { 22.0 * s } else { 0.0 };
        let (w, h) = ((680.0 * s).min(view.width() - 48.0 * s), (pad * 2.0 + 36.0 * s + body + 56.0 * s).min(view.height() - 24.0 * s)); let (x0, y0) = (view.x0 + (view.width() - w) / 2.0, view.y0 + (view.height() - h) / 2.0); let card = Rect::new(x0, y0, x0 + w, y0 + h);
        paint::rect(scene, view, t::DIALOG_SCRIM_COLOR); let rad = t::DIALOG_CORNER_RADIUS as f64 * s;
        for sh in t::POPOVER_SHADOW { scene.draw_blurred_rounded_rect(Affine::translate((sh.x as f64 * s, sh.y as f64 * s)), card.inflate(sh.spread as f64 * s, sh.spread as f64 * s), c(sh.color), rad, (sh.blur as f64 * s / 2.0).max(0.1)); }
        scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, c(t::BACKGROUND_LAYER_1_COLOR), None, &RoundedRect::from_rect(card, rad));
        let line = |scene: &mut Scene, fonts: &mut Fonts, text: &str, x: f64, cy: f64, size: f32, bold: bool, color: peniko::Color, room: f64| -> f64 { let l = fonts.layout_fit(text, Family::Sans, size * s as f32, if bold { t::FONT_WEIGHT_BOLD } else { t::FONT_WEIGHT_REGULAR } as f32, color, room.max(8.0)); draw_layout(scene, &l, Affine::translate((x, cy - l.height() as f64 / 2.0))); l.width() as f64 };
        let (lx, x1) = (x0 + pad, x0 + w - pad); let fx = lx + 148.0 * s; let mut y = y0 + pad; let (sw, sh) = (t::SWITCH_TRACK_WIDTH as f64 * s, t::SWITCH_TRACK_HEIGHT as f64 * s);
        line(scene, fonts, "Settings", lx, y + 10.0 * s, t::DIALOG_TITLE_SIZE, true, ink, w); y += 36.0 * s;
        // the sync root, and whether it is a folder that is there
        let hw = line(scene, fonts, "Sync root", lx, y + 10.0 * s, t::DETAIL_SIZE_M, true, sub, 200.0 * s);
        let root = self.root.text().trim().to_string(); if !root.is_empty() { let ok = std::path::Path::new(&root).is_dir(); line(scene, fonts, if ok { "path OK" } else { "path not reachable" }, lx + hw + 12.0 * s, y + 10.0 * s, t::DETAIL_SIZE_M, false, c(if ok { t::POSITIVE_VISUAL_COLOR } else { t::NOTICE_VISUAL_COLOR }), 200.0 * s); }
        y += 26.0 * s; let bw = self.browse.measure(fonts, s).0; self.root.layout(fonts, Rect::new(lx, y, x1 - bw - 8.0 * s, y + fh), s); self.root.paint(scene, fonts, s); self.browse.rect = Rect::new(x1 - bw, y + 3.0 * s, x1, y + fh - 3.0 * s); self.browse.paint(scene, fonts, s, now); y += row;
        line(scene, fonts, "Tags (comma-separated)", lx, y + 10.0 * s, t::DETAIL_SIZE_M, true, sub, 300.0 * s); y += 26.0 * s; self.tags.layout(fonts, Rect::new(lx, y, x1, y + fh), s); self.tags.paint(scene, fonts, s); y += row;
        line(scene, fonts, "Networking", lx, y + 10.0 * s, t::DETAIL_SIZE_M, true, sub, 200.0 * s); y += 26.0 * s;
        line(scene, fonts, "HTTP port", lx, y + fh / 2.0, t::DETAIL_SIZE_M, false, ink, 140.0 * s); self.port.layout(fonts, Rect::new(fx, y, fx + 110.0 * s, y + fh), s); self.port.paint(scene, fonts, s);
        let ix = fx + 130.0 * s; let iw = line(scene, fonts, "IP override", ix, y + fh / 2.0, t::DETAIL_SIZE_M, false, ink, 100.0 * s); self.ip.layout(fonts, Rect::new(ix + iw + 10.0 * s, y, x1, y + fh), s); self.ip.paint(scene, fonts, s); y += row;
        line(scene, fonts, "UDP multicast heartbeat", lx, y + fh / 2.0, t::DETAIL_SIZE_M, false, ink, 200.0 * s); self.udp.rect = Rect::new(fx + 60.0 * s, y + (fh - sh) / 2.0, fx + 60.0 * s + sw, y + (fh + sh) / 2.0); self.udp.paint(scene, s, now);
        let ux = fx + 60.0 * s + sw + 24.0 * s; let uw = line(scene, fonts, "UDP port", ux, y + fh / 2.0, t::DETAIL_SIZE_M, false, if self.udp.on { ink } else { sub }, 100.0 * s); self.udp_port.layout(fonts, Rect::new(ux + uw + 10.0 * s, y, ux + uw + 120.0 * s, y + fh), s); self.udp_port.paint(scene, fonts, s); y += row;
        // the farm's secret, as it looks from this node
        line(scene, fonts, "Farm security", lx, y + 10.0 * s, t::DETAIL_SIZE_M, true, sub, 200.0 * s); y += 26.0 * s;
        let fw = line(scene, fonts, "Secret fingerprint", lx, y + 8.0 * s, t::DETAIL_SIZE_M, false, ink, 160.0 * s); let (print, other) = (&self.secret.0, self.secret.1);
        let l = fonts.layout_fit(if print.is_empty() { "not loaded" } else { print }, Family::Mono, t::DETAIL_SIZE_M * s as f32, t::FONT_WEIGHT_REGULAR as f32, if print.is_empty() { c(t::NOTICE_VISUAL_COLOR) } else { ink }, 200.0 * s); draw_layout(scene, &l, Affine::translate((lx + fw + 12.0 * s, y + 8.0 * s - l.height() as f64 / 2.0))); y += 22.0 * s;
        let note = if print.is_empty() { "No farm secret loaded \u{2014} this node will reject all API calls. Check that farm.json is readable.".to_string() } else if other > 0 { format!("{other} node(s) on this farm report a different secret and cannot exchange work with this one.") } else { "Every node reading the same farm.json shares this secret. Anyone with read access to the sync root can control the farm.".to_string() };
        line(scene, fonts, &note, lx, y + 8.0 * s, t::DETAIL_SIZE_S, false, sub, w - 2.0 * pad); y += 22.0 * s + 6.0 * s;
        line(scene, fonts, "Rendering", lx, y + 10.0 * s, t::DETAIL_SIZE_M, true, sub, 200.0 * s); y += 26.0 * s;
        line(scene, fonts, "Enable staging", lx, y + fh / 2.0, t::DETAIL_SIZE_M, false, ink, 200.0 * s); self.staging.rect = Rect::new(fx + 60.0 * s, y + (fh - sh) / 2.0, fx + 60.0 * s + sw, y + (fh + sh) / 2.0); self.staging.paint(scene, s, now); y += row;
        // the path mappings: a name, the root on Windows, the root on macOS
        line(scene, fonts, "Path mappings", lx, y + 10.0 * s, t::DETAIL_SIZE_M, true, sub, 200.0 * s); y += 26.0 * s;
        let rw = self.maps.first().map(|m| m.remove.measure(fonts, s).0).unwrap_or(70.0 * s); let cols = x1 - lx - rw - sw - 32.0 * s; let (c0, c1) = (cols * 0.2, cols * 0.4);
        for m in self.maps.iter_mut() { m.label.layout(fonts, Rect::new(lx, y, lx + c0, y + fh), s); m.win.layout(fonts, Rect::new(lx + c0 + 8.0 * s, y, lx + c0 + 8.0 * s + c1, y + fh), s); m.mac.layout(fonts, Rect::new(lx + c0 + c1 + 16.0 * s, y, lx + c0 + 2.0 * c1 + 16.0 * s, y + fh), s);
            m.label.paint(scene, fonts, s); m.win.paint(scene, fonts, s); m.mac.paint(scene, fonts, s); m.on.rect = Rect::new(x1 - rw - sw - 8.0 * s, y + (fh - sh) / 2.0, x1 - rw - 8.0 * s, y + (fh + sh) / 2.0); m.on.paint(scene, s, now); m.remove.rect = Rect::new(x1 - rw, y + 3.0 * s, x1, y + fh - 3.0 * s); m.remove.paint(scene, fonts, s, now); y += row; }
        let aw = self.add.measure(fonts, s).0; self.add.rect = Rect::new(lx, y, lx + aw, y + 24.0 * s); self.add.paint(scene, fonts, s, now); y += 34.0 * s;
        if let Some(e) = &self.error { line(scene, fonts, e, lx, y + 8.0 * s, t::DETAIL_SIZE_M, false, c(t::NEGATIVE_VISUAL_COLOR), w - 2.0 * pad); }
        let bh = t::BUTTON_HEIGHT_M as f64 * s; let by = y0 + h - pad - bh; let (ws, _) = self.save.measure(fonts, s); let (wc, _) = self.cancel.measure(fonts, s);
        line(scene, fonts, "Save restarts minRender; a render on this node is ended.", lx, by + bh / 2.0, t::DETAIL_SIZE_S, false, sub, x1 - lx - ws - wc - 32.0 * s);
        self.save.rect = Rect::new(x1 - ws, by, x1, by + bh); self.cancel.rect = Rect::new(self.save.rect.x0 - 8.0 * s - wc, by, self.save.rect.x0 - 8.0 * s, by + bh); self.cancel.paint(scene, fonts, s, now); self.save.paint(scene, fonts, s, now);
    }
    pub fn animating(&self, now: Instant) -> bool { self.udp.animating(now) || self.staging.animating(now) || self.cancel.animating(now) || self.save.animating(now) || self.maps.iter().any(|m| m.on.animating(now)) }
}
