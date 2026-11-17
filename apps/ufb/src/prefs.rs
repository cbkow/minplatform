//! The Settings card (canon row 516; 1.4.2 had no settings window - its settings lay in dialogs here and there).
//! One card over the window, the family's as minRender's: nothing changes until Save.
//!
//! - **General**: what a pane shows from the start - hidden files, folders before files, image sequences as one
//!   entry, a grid instead of a list. The View menu's own rows change the same settings (they are remembered now).
//!   Start at sign-in is the installed application's (it comes with packaging): shown, not yet to be switched.
//! - **Sync**: on or off, the farm's folder, this machine's name in it, and - a development build's belt - the one
//!   folder whose rows take part. The same settings as the Sync entry at the sidebar's foot.
//! - Shares, project types and columns are not here: they are managed where they are used (the sidebar, the menus).
use kurbo::{Affine, Rect, RoundedRect};
use min_theme::tokens as t;
use min_ui::{fonts::{draw_layout, Fonts}, paint, Button, Event, Family, Key, KeyEvent, Named, Switch, TextButton, TextField, Variant};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use vello::Scene;

pub struct Prefs {
    hidden: Switch, folders_first: Switch, sequences: Switch, grid: Switch, sign_in: Switch,
    sync: Switch, farm: TextField, browse: TextButton, node: TextField, only: TextField,
    focus: usize, cancel: TextButton, save: TextButton, error: Option<String>, picked: Arc<Mutex<Option<String>>>,
    /// sync's settings as they were when the card opened (changed: sync is started again on Save)
    was: (bool, String, String, String),
}
#[derive(Debug, PartialEq)]
pub enum Out { None, Close, /// saved; whether sync's settings changed
    Saved { sync: bool } }

/// What a pane shows from the start, as the settings have it: (hidden files, folders first, sequences as one, a grid).
pub fn view() -> (bool, bool, bool, bool) { crate::settings::with(|s| (s.bool("view/hidden"), s.bool("view/folders_first"), s.bool("view/sequences"), s.bool("view/grid"))) }

fn field(text: &str, placeholder: &str, fonts: &mut Fonts) -> TextField { let mut f = TextField::new(Family::Sans, t::BODY_SIZE_S); f.placeholder = placeholder.into(); f.set_text_end(text, fonts); f }

impl Prefs {
    pub fn open(fonts: &mut Fonts) -> Prefs {
        let (hidden, folders_first, sequences, grid) = view();
        let (on, farm, node, only, sign_in) = crate::settings::with(|s| (s.bool("sync/on"), s.string("sync/farm"), s.string("sync/node"), s.string("sync/only"), s.bool("general/start_at_sign_in")));
        let mut sign = Switch::new(sign_in); sign.enabled = false;
        let mut me = Prefs { hidden: Switch::new(hidden), folders_first: Switch::new(folders_first), sequences: Switch::new(sequences), grid: Switch::new(grid), sign_in: sign,
            sync: Switch::new(on), farm: field(&farm, "a folder on shared storage", fonts), browse: TextButton::new("Browse", Variant::Secondary).size_s(), node: field(&node, &crate::mesh::node_name(""), fonts), only: field(&only, "a folder; empty: everything shared", fonts),
            focus: 0, cancel: TextButton::new("Cancel", Variant::Secondary), save: TextButton::new("Save", Variant::Primary), error: None, picked: Arc::new(Mutex::new(None)), was: (on, farm, node, only) };
        me.set_focus(usize::MAX); me
    }
    fn fields(&mut self) -> [&mut TextField; 3] { [&mut self.farm, &mut self.node, &mut self.only] }
    fn set_focus(&mut self, to: usize) { self.focus = to; for (i, f) in self.fields().into_iter().enumerate() { f.focused = i == to; } }
    pub fn typing(&self) -> bool { self.focus < 3 }
    /// Save: the settings are written, or what is wrong is said and nothing is.
    fn commit(&mut self) -> Out {
        let (farm, node, only) = (self.farm.text().trim().to_string(), self.node.text().trim().to_string(), self.only.text().trim().to_string());
        if self.sync.on && farm.is_empty() { self.error = Some("Sync needs the farm's folder: the one every machine shares.".into()); return Out::None }
        if self.sync.on && !std::path::Path::new(&farm).is_dir() { self.error = Some("The farm's folder is not there.".into()); return Out::None }
        if !only.is_empty() && !std::path::Path::new(&only).is_dir() { self.error = Some("\u{201c}Only under\u{201d} names a folder that is not there.".into()); return Out::None }
        let now = (self.sync.on, farm, node, only);
        crate::settings::with(|s| { s.set_bool("view/hidden", self.hidden.on); s.set_bool("view/folders_first", self.folders_first.on); s.set_bool("view/sequences", self.sequences.on); s.set_bool("view/grid", self.grid.on);
            s.set_bool("sync/on", now.0); s.set_str("sync/farm", &now.1); s.set_str("sync/node", &now.2); s.set_str("sync/only", &now.3); });
        Out::Saved { sync: now != self.was }
    }
    /// (scripted looks) set a switch or a field by its name, as a hand would.
    pub fn demo_set(&mut self, what: &str, value: &str, fonts: &mut Fonts) {
        let (on, now) = (matches!(value, "on" | "1" | "true"), Instant::now()); self.error = None;
        match what { "hidden" => self.hidden.set(on, now), "foldersfirst" => self.folders_first.set(on, now), "sequences" => self.sequences.set(on, now), "grid" => self.grid.set(on, now), "sync" => self.sync.set(on, now),
            "farm" => self.farm.set_text_end(value, fonts), "node" => self.node.set_text_end(value, fonts), "only" => self.only.set_text_end(value, fonts), _ => {} }
    }
    pub fn demo_save(&mut self) -> Out { self.commit() }
    pub fn error(&self) -> Option<&str> { self.error.as_deref() }

    pub fn pointer(&mut self, e: &Event, fonts: &mut Fonts) -> Out {
        let now = Instant::now();
        if self.cancel.event(e, now).0 { return Out::Close } if self.save.event(e, now).0 { return self.commit() }
        if self.browse.event(e, now).0 { let (picked, at) = (self.picked.clone(), self.farm.text().to_string()); std::thread::spawn(move || { let mut d = rfd::FileDialog::new().set_title("The farm's folder"); if std::path::Path::new(&at).is_dir() { d = d.set_directory(&at); } if let Some(p) = d.pick_folder() { *picked.lock().unwrap() = Some(p.to_string_lossy().into_owned()); } }); return Out::None }
        for sw in [&mut self.hidden, &mut self.folders_first, &mut self.sequences, &mut self.grid, &mut self.sync] { sw.event(e, now); }
        if let Event::Down { x, y, button: Button::Primary } = *e { let hit = self.fields().iter().position(|f| f.hit(x, y)); self.set_focus(hit.unwrap_or(usize::MAX)); }
        let focus = self.focus; if let Some(f) = self.fields().into_iter().nth(focus) { f.pointer(e, fonts); }
        Out::None
    }
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) -> Out {
        match &k.key {
            Key::Named(Named::Escape) => return Out::Close,
            Key::Named(Named::Enter) => return self.commit(),
            Key::Named(Named::Tab) => { let to = if self.focus >= 3 { 0 } else if k.shift { (self.focus + 2) % 3 } else { (self.focus + 1) % 3 }; self.set_focus(to); if let Some(f) = self.fields().into_iter().nth(to) { f.select_all(fonts); } }
            _ => { let focus = self.focus; let typed = match self.fields().into_iter().nth(focus) { Some(f) => { let _ = f.key(k, fonts); true } None => false }; if typed { self.error = None; } }
        }
        Out::None
    }
    pub fn paste(&mut self, text: &str, fonts: &mut Fonts) { let focus = self.focus; if let Some(f) = self.fields().into_iter().nth(focus) { f.paste(text.lines().next().unwrap_or(""), fonts); } }
    pub fn copy(&mut self, cut: bool, fonts: &mut Fonts) -> Option<String> { let focus = self.focus; self.fields().into_iter().nth(focus).and_then(|f| f.copy(cut, fonts)) }

    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, view: Rect, s: f64, now: Instant) {
        if let Some(p) = self.picked.lock().unwrap().take() { self.farm.set_text_end(&p, fonts); self.error = None; }
        let c = paint::color; let pad = t::DIALOG_PADDING as f64 * s; let (ink, sub) = (c(t::NEUTRAL_CONTENT_COLOR_DEFAULT), c(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT)); let (row, fh, head) = (34.0 * s, 30.0 * s, 30.0 * s);
        let body = 3.0 * head + 5.0 * row + 4.0 * (row + 4.0 * s) + 3.0 * 20.0 * s + 28.0 * s;
        let (w, h) = ((620.0 * s).min(view.width() - 48.0 * s), (pad * 2.0 + 40.0 * s + body + 56.0 * s).min(view.height() - 24.0 * s)); let (x0, y0) = (view.x0 + (view.width() - w) / 2.0, view.y0 + (view.height() - h) / 2.0); let card = Rect::new(x0, y0, x0 + w, y0 + h);
        paint::rect(scene, view, t::DIALOG_SCRIM_COLOR); let rad = t::DIALOG_CORNER_RADIUS as f64 * s;
        for sh in t::POPOVER_SHADOW { scene.draw_blurred_rounded_rect(Affine::translate((sh.x as f64 * s, sh.y as f64 * s)), card.inflate(sh.spread as f64 * s, sh.spread as f64 * s), c(sh.color), rad, (sh.blur as f64 * s / 2.0).max(0.1)); }
        scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, c(t::BACKGROUND_LAYER_1_COLOR), None, &RoundedRect::from_rect(card, rad));
        // a line of text, its middle at `cy`; its width
        let line = |scene: &mut Scene, fonts: &mut Fonts, text: &str, x: f64, cy: f64, size: f32, bold: bool, color: peniko::Color, room: f64| -> f64 { let l = fonts.layout_fit(text, Family::Sans, size * s as f32, if bold { t::FONT_WEIGHT_BOLD } else { t::FONT_WEIGHT_REGULAR } as f32, color, room.max(8.0)); draw_layout(scene, &l, Affine::translate((x, cy - l.height() as f64 / 2.0))); l.width() as f64 };
        let (lx, x1) = (x0 + pad, x0 + w - pad); let fx = lx + 190.0 * s; let mut y = y0 + pad; let (sw, shh) = (t::SWITCH_TRACK_WIDTH as f64 * s, t::SWITCH_TRACK_HEIGHT as f64 * s);
        line(scene, fonts, "Settings", lx, y + 12.0 * s, t::DIALOG_TITLE_SIZE, true, ink, w); y += 40.0 * s;
        // -- General
        line(scene, fonts, "General", lx, y + head / 2.0, t::DETAIL_SIZE_M, true, sub, 200.0 * s); y += head;
        let toggle = |scene: &mut Scene, fonts: &mut Fonts, label: &str, sw_: &mut Switch, note: &str, y: &mut f64| {
            line(scene, fonts, label, lx, *y + row / 2.0, t::BODY_SIZE_S, false, if sw_.enabled { ink } else { sub }, x1 - lx - sw - 16.0 * s);
            if !note.is_empty() { let lw = fonts.layout(label, Family::Sans, t::BODY_SIZE_S * s as f32, t::FONT_WEIGHT_REGULAR as f32, ink).width() as f64; line(scene, fonts, note, lx + lw + 12.0 * s, *y + row / 2.0, t::DETAIL_SIZE_S, false, sub, x1 - lx - lw - sw - 32.0 * s); }
            sw_.rect = Rect::new(x1 - sw, *y + (row - shh) / 2.0, x1, *y + (row + shh) / 2.0); sw_.paint(scene, s, now); *y += row; };
        toggle(scene, fonts, "Show hidden files", &mut self.hidden, "", &mut y);
        toggle(scene, fonts, "Folders before files", &mut self.folders_first, "", &mut y);
        toggle(scene, fonts, "Image sequences as one entry", &mut self.sequences, "", &mut y);
        toggle(scene, fonts, "Show folders as a grid", &mut self.grid, "", &mut y);
        toggle(scene, fonts, "Start at sign-in", &mut self.sign_in, "with the installed application", &mut y);
        // -- Sync
        y += 4.0 * s; line(scene, fonts, "Sync", lx, y + head / 2.0, t::DETAIL_SIZE_M, true, sub, 200.0 * s); y += head;
        toggle(scene, fonts, "Sync metadata with the other machines", &mut self.sync, &format!("now: {}", crate::ui::Ui::sync_words()), &mut y);
        let dim = if self.sync.on { ink } else { sub };
        line(scene, fonts, "Farm folder", lx, y + fh / 2.0, t::BODY_SIZE_S, false, dim, 180.0 * s); let bw = self.browse.measure(fonts, s).0;
        self.farm.layout(fonts, Rect::new(fx, y, x1 - bw - 8.0 * s, y + fh), s); self.farm.paint(scene, fonts, s); self.browse.rect = Rect::new(x1 - bw, y + 3.0 * s, x1, y + fh - 3.0 * s); self.browse.paint(scene, fonts, s, now); y += row + 4.0 * s;
        line(scene, fonts, "This machine's name", lx, y + fh / 2.0, t::BODY_SIZE_S, false, dim, 180.0 * s); self.node.layout(fonts, Rect::new(fx, y, x1, y + fh), s); self.node.paint(scene, fonts, s); y += row + 4.0 * s;
        line(scene, fonts, "Only under (test build)", lx, y + fh / 2.0, t::BODY_SIZE_S, false, dim, 180.0 * s); self.only.layout(fonts, Rect::new(fx, y, x1, y + fh), s); self.only.paint(scene, fonts, s); y += row + 4.0 * s;
        line(scene, fonts, "The farm's folder is the one every machine shares; each machine has a name of its own in it.", lx, y + 10.0 * s, t::DETAIL_SIZE_S, false, sub, w - 2.0 * pad); y += 20.0 * s;
        // -- About
        y += 4.0 * s; line(scene, fonts, "About", lx, y + head / 2.0, t::DETAIL_SIZE_M, true, sub, 200.0 * s); y += head;
        line(scene, fonts, concat!("UFB ", env!("CARGO_PKG_VERSION"), "  \u{b7}  GPL-3.0-or-later"), lx, y + 10.0 * s, t::BODY_SIZE_S, false, ink, w - 2.0 * pad); y += 20.0 * s;
        line(scene, fonts, "Updates and the licence notices come with the installed application.", lx, y + 10.0 * s, t::DETAIL_SIZE_S, false, sub, w - 2.0 * pad); y += 20.0 * s;
        if let Some(e) = &self.error { line(scene, fonts, e, lx, y + 14.0 * s, t::DETAIL_SIZE_M, false, c(t::NEGATIVE_VISUAL_COLOR), w - 2.0 * pad); }
        let bh = t::BUTTON_HEIGHT_M as f64 * s; let by = y0 + h - pad - bh; let (ws, _) = self.save.measure(fonts, s); let (wc, _) = self.cancel.measure(fonts, s);
        self.save.rect = Rect::new(x1 - ws, by, x1, by + bh); self.cancel.rect = Rect::new(self.save.rect.x0 - 8.0 * s - wc, by, self.save.rect.x0 - 8.0 * s, by + bh); self.cancel.paint(scene, fonts, s, now); self.save.paint(scene, fonts, s, now);
    }
    pub fn animating(&self, now: Instant) -> bool { [&self.hidden, &self.folders_first, &self.sequences, &self.grid, &self.sync].iter().any(|s| s.animating(now)) || self.cancel.animating(now) || self.save.animating(now) || self.browse.animating(now) || self.typing() }
}
