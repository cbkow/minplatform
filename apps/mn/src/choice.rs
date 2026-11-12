//! A choice cell's picker (canon row 317, stage 3): a list under the cell - a field that filters the options and adds a
//! new one, the options, Clear, "Edit options…". Its second face edits the column's options on a DRAFT (rename,
//! recolour, reorder, remove, add); Done hands the whole set back as one change, Cancel drops it.
//! Everything in it is min-ui's (the user 2026-11-12): a `Popover`, a `TextField`, the menu's rows, ticks and
//! separators, `ActionButton`s for a row's tools, `TextButton`s for Cancel / Done. Only the colour dot is drawn here.
use kurbo::{Affine, Circle, Rect};
use min_theme::tokens as t;
use min_ui::{action_button::{ActionButton, Response}, fonts::draw_layout, menu, Event, Family, FieldResponse, Fonts, Key, KeyEvent, Named, Placement, Popover, TextButton, TextField, Variant};
use mn_doc::{BlockId, ChoiceOption};
use mn_edit::typed::PALETTE;
use peniko::{Color, Fill};
use std::time::Instant;
use vello::Scene;

pub enum Out { None, Redraw, Close, Pick(Option<String>), Add(String), Options(Vec<ChoiceOption>) }
#[derive(Clone, Copy, PartialEq, Debug)]
enum Hit { Option(usize), Add, Clear, Sep, Edit, Swatch(usize), Label(usize) }

pub struct ChoicePicker {
    /// the cell it stands under
    pub cell: BlockId,
    pop: Popover, field: TextField, options: Vec<ChoiceOption>, chosen: Option<String>,
    hover: Option<Hit>, at: usize, edit: bool, renaming: Option<usize>, rows: Vec<(Rect, Hit)>, scale: f64,
    /// the editor's tools: per option up, down, remove; then Add option, Cancel, Done
    tools: Vec<[ActionButton; 3]>, add: ActionButton, cancel: TextButton, done: TextButton,
}
const W: f64 = 256.0; const FIELD: f64 = 32.0;
fn row_h() -> f64 { t::MENU_ITEM_HEIGHT as f64 }
fn hex(s: &str) -> Color { mn_layout::hex(s).unwrap_or(min_ui::paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT)) }
fn small(icon: &'static str, tip: &str) -> ActionButton { let mut b = ActionButton::new(icon, tip).size_s(); b.icon_size = Some(t::ICON_SIZE_SMALL); b }

impl ChoicePicker {
    /// `typed`: what was typed to open it (it goes into the field, and the list is filtered by it).
    pub fn new(cell: BlockId, options: Vec<ChoiceOption>, chosen: Option<String>, typed: &str, fonts: &mut Fonts) -> ChoicePicker {
        let mut field = TextField::new(Family::Sans, t::BODY_SIZE_M); field.placeholder = "Find or add\u{2026}".into(); field.set_text_end(typed, fonts); field.focused = true;
        let mut add = ActionButton::new("plus", "").size_s(); add.label = Some("Add option".into()); add.icon_size = Some(t::ICON_SIZE_SMALL);
        ChoicePicker { cell, pop: Popover::new(""), field, options, chosen, hover: None, at: 0, edit: false, renaming: None, rows: vec![], scale: 1.0,
            tools: vec![], add, cancel: TextButton::new("Cancel", Variant::Secondary).size_s(), done: TextButton::new("Done", Variant::Primary).size_s() }
    }
    /// Opened on its options' editor.
    pub fn editing(mut self) -> ChoicePicker { self.to_edit(); self }
    fn to_edit(&mut self) { self.edit = true; self.field.clear(); self.field.placeholder.clear(); self.field.focused = false; self.hover = None; }
    pub fn hit(&self, x: f64, y: f64) -> bool { self.pop.hit(x, y) }
    pub fn animating(&self, now: Instant) -> bool { self.edit && (self.add.animating(now) || self.cancel.animating(now) || self.done.animating(now) || self.tools.iter().flatten().any(|b| b.animating(now))) }
    fn filter(&self) -> String { self.field.text().trim().to_lowercase() }
    /// The rows of the list, in order (the picker's face).
    fn list(&self) -> Vec<Hit> {
        let f = self.filter();
        let mut v: Vec<Hit> = self.options.iter().enumerate().filter(|(_, o)| f.is_empty() || o.label.to_lowercase().contains(&f)).map(|(i, _)| Hit::Option(i)).collect();
        if !f.is_empty() && !self.options.iter().any(|o| o.label.trim().to_lowercase() == f) { v.push(Hit::Add); }
        if f.is_empty() { if !v.is_empty() { v.push(Hit::Sep); } if self.chosen.is_some() { v.push(Hit::Clear); } v.push(Hit::Edit); }
        v
    }
    /// The rows the keys walk (no separator).
    fn stops(&self) -> Vec<Hit> { self.list().into_iter().filter(|h| *h != Hit::Sep).collect() }
    /// Placed under `anchor` (the cell, window px) within `bounds`.
    pub fn layout(&mut self, fonts: &mut Fonts, anchor: Rect, bounds: Rect, s: f64) {
        self.scale = s; self.rows.clear(); let rh = row_h(); let sep = t::SPACING_100 as f64;
        if self.edit {
            if self.tools.len() != self.options.len() { self.tools = (0..self.options.len()).map(|_| [small("caret-up", "Move up"), small("caret-down", "Move down"), small("x", "Remove")]).collect(); }
            let foot = t::COMPONENT_HEIGHT_75 as f64 + sep * 2.0;
            self.pop.open_at_prefer(fonts, anchor, W, self.options.len() as f64 * rh + foot, bounds, s, Placement::Below);
            let k = self.pop.content; let mut y = k.y0; let b = t::ACTION_BUTTON_HEIGHT_S as f64 * s; let n = self.options.len();
            for i in 0..n { let r = Rect::new(k.x0, y, k.x1, y + rh * s); y += rh * s; let by = r.center().y - b / 2.0;
                self.rows.push((Rect::new(r.x0, r.y0, r.x0 + 28.0 * s, r.y1), Hit::Swatch(i))); self.rows.push((Rect::new(r.x0 + 28.0 * s, r.y0, r.x1 - 3.0 * b - 4.0 * s, r.y1), Hit::Label(i)));
                for (j, t) in self.tools[i].iter_mut().enumerate() { let x = r.x1 - (3 - j) as f64 * b; t.rect = Rect::new(x, by, x + b, by + b); }
                self.tools[i][0].enabled = i > 0; self.tools[i][1].enabled = i + 1 < n; }
            let fy = y + sep * s; let fh = t::COMPONENT_HEIGHT_75 as f64 * s;
            let (aw, _) = self.add.measure(fonts, s); self.add.rect = Rect::new(k.x0, fy, k.x0 + aw, fy + fh);
            let (dw, _) = self.done.measure(fonts, s); let (cw, _) = self.cancel.measure(fonts, s);
            self.done.rect = Rect::new(k.x1 - dw, fy, k.x1, fy + fh); self.cancel.rect = Rect::new(k.x1 - dw - 8.0 * s - cw, fy, k.x1 - dw - 8.0 * s, fy + fh);
            if let Some(i) = self.renaming { if let Some((r, _)) = self.rows.iter().find(|(_, h)| *h == Hit::Label(i)) { self.field.layout(fonts, r.inset((0.0, -3.0 * s)), s); } }
            return
        }
        let list = self.list(); let top = FIELD + sep;
        let h: f64 = list.iter().map(|x| if *x == Hit::Sep { sep } else { rh }).sum();
        self.pop.open_at_prefer(fonts, anchor, W, top + h, bounds, s, Placement::Below);
        let k = self.pop.content; let mut y = k.y0 + top * s;
        for x in list { let hh = if x == Hit::Sep { sep } else { rh } * s; self.rows.push((Rect::new(k.x0, y, k.x1, y + hh), x)); y += hh; }
        self.field.layout(fonts, Rect::new(k.x0, k.y0, k.x1, k.y0 + FIELD * s), s);
        self.at = self.at.min(self.stops().len().saturating_sub(1));
    }
    fn at_point(&self, x: f64, y: f64) -> Option<Hit> { self.rows.iter().find(|(r, h)| *h != Hit::Sep && r.contains(kurbo::Point::new(x, y))).map(|(_, h)| *h) }
    fn rename_end(&mut self, keep: bool) {
        if let Some(i) = self.renaming.take() { let l = mn_edit::typed::clean_label(self.field.text()); if keep && !l.is_empty() { if let Some(o) = self.options.get_mut(i) { o.label = l; } } if self.options.get(i).is_some_and(|o| o.label.is_empty()) { self.options.remove(i); } self.field.focused = false; }
    }
    fn rename(&mut self, i: usize, fonts: &mut Fonts) { if let Some(o) = self.options.get(i) { self.field.set_text_end(&o.label, fonts); self.field.select_all(fonts); self.field.focused = true; self.renaming = Some(i); } }
    fn act(&mut self, h: Hit, fonts: &mut Fonts) -> Out {
        match h {
            Hit::Option(i) => Out::Pick(self.options.get(i).map(|o| o.id.clone())),
            Hit::Add => Out::Add(self.field.text().trim().to_string()),
            Hit::Clear => Out::Pick(None),
            Hit::Edit => { self.to_edit(); Out::Redraw }
            Hit::Swatch(i) => { self.rename_end(true); if let Some(o) = self.options.get_mut(i) { let k = PALETTE.iter().position(|p| p.eq_ignore_ascii_case(&o.color)).map(|k| k + 1).unwrap_or(0); o.color = PALETTE[k % PALETTE.len()].to_string(); } Out::Redraw }
            Hit::Label(i) => { self.rename_end(true); self.rename(i, fonts); Out::Redraw }
            Hit::Sep => Out::None,
        }
    }
    /// The editor's buttons take an event; what came of it.
    fn tools_event(&mut self, e: &Event, fonts: &mut Fonts) -> Option<Out> {
        let now = Instant::now(); let mut did = None;
        for i in 0..self.tools.len() { for j in 0..3 { if self.tools[i][j].event(e, now) == Response::Clicked { did = Some((i, j)); } } }
        if let Some((i, j)) = did { self.rename_end(true); let n = self.options.len();
            match j { 0 if i > 0 && i < n => self.options.swap(i, i - 1), 1 if i + 1 < n => self.options.swap(i, i + 1), 2 if i < n => { self.options.remove(i); } _ => {} }
            for t in self.tools.iter_mut().flatten() { t.reset(now); } return Some(Out::Redraw) }
        if self.add.event(e, now) == Response::Clicked { self.rename_end(true); let n = self.options.len();
            self.options.push(ChoiceOption { id: BlockId::new().to_string(), label: String::new(), color: PALETTE[n % PALETTE.len()].to_string() }); self.field.clear(); self.field.focused = true; self.renaming = Some(n); let _ = fonts; return Some(Out::Redraw) }
        if self.cancel.event(e, now).0 { return Some(Out::Close) }
        if self.done.event(e, now).0 { self.rename_end(true); return Some(Out::Options(self.options.clone())) }
        None
    }
    /// A pointer event. A press outside closes it (and is not taken: the page gets it).
    pub fn pointer(&mut self, e: &Event, fonts: &mut Fonts) -> (Out, bool) {
        if self.edit { if let Some(out) = self.tools_event(e, fonts) { return (out, true) } }
        match *e {
            Event::Move { x, y } => { self.hover = self.at_point(x, y); if self.field.focused { self.field.pointer(e, fonts); } (Out::Redraw, self.hit(x, y)) }
            Event::Down { x, y, .. } => { if !self.hit(x, y) { return (if self.edit { self.rename_end(true); Out::Options(self.options.clone()) } else { Out::Close }, false) }
                if self.field.focused && self.field.hit(x, y) { self.field.pointer(e, fonts); return (Out::Redraw, true) }
                match self.at_point(x, y) { Some(h) => (self.act(h, fonts), true), None => { if self.edit { self.rename_end(true); } (Out::Redraw, true) } } }
            Event::Up { .. } => { if self.field.focused { self.field.pointer(e, fonts); } (Out::Redraw, true) }
            _ => (Out::None, false),
        }
    }
    pub fn ime(&mut self, e: &min_ui::ImeEvent, fonts: &mut Fonts) { if self.field.focused { self.field.ime(e, fonts); self.at = 0; } }
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) -> Out {
        if self.edit {
            if self.renaming.is_some() { return match self.field.key(k, fonts) { FieldResponse::Accepted => { self.rename_end(true); Out::Redraw } FieldResponse::Cancelled => { self.rename_end(false); Out::Redraw } _ => Out::Redraw } }
            return match k.key { Key::Named(Named::Escape) => Out::Close, Key::Named(Named::Enter) => Out::Options(self.options.clone()), _ => Out::None }
        }
        let stops = self.stops();
        match k.key {
            Key::Named(Named::Escape) => Out::Close,
            Key::Named(Named::Down) => { self.at = (self.at + 1).min(stops.len().saturating_sub(1)); Out::Redraw }
            Key::Named(Named::Up) => { self.at = self.at.saturating_sub(1); Out::Redraw }
            Key::Named(Named::Enter) | Key::Named(Named::Tab) => match stops.get(self.at).copied() { Some(h) => self.act(h, fonts), None => Out::Close },
            _ => { if self.field.key(k, fonts) == FieldResponse::Changed { self.at = 0; } Out::Redraw }
        }
    }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts) {
        let s = self.scale; let now = Instant::now(); self.pop.paint(scene, fonts, s);
        let dot = |scene: &mut Scene, x: f64, y: f64, c: &str| scene.fill(Fill::NonZero, Affine::IDENTITY, hex(c), None, &Circle::new((x, y), 5.0 * s));
        if self.edit {
            for (r, h) in self.rows.clone() { match h {
                Hit::Swatch(i) => { if self.hover == Some(h) { min_ui::paint::rounded(scene, Rect::from_center_size(r.center(), (22.0 * s, 22.0 * s)), t::PICKER_CORNER_RADIUS as f64 * s, t::MENU_ITEM_HOVER_COLOR); } dot(scene, r.center().x, r.center().y, &self.options[i].color) }
                Hit::Label(i) if self.renaming != Some(i) => { if self.hover == Some(h) { min_ui::paint::rounded(scene, r, t::PICKER_CORNER_RADIUS as f64 * s, t::MENU_ITEM_HOVER_COLOR); }
                    let g = fonts.layout_fit(&self.options[i].label, Family::Sans, t::BODY_SIZE_M * s as f32, t::FONT_WEIGHT_REGULAR as f32, min_ui::paint::color(t::MENU_ITEM_CONTENT_COLOR), (r.width() - 8.0 * s).max(8.0));
                    draw_layout(scene, &g, Affine::translate((r.x0 + 4.0 * s, r.center().y - g.height() as f64 / 2.0))); }
                _ => {} } }
            for t in self.tools.iter().flatten() { t.paint(scene, fonts, s, now); }
            if let Some(y) = self.rows.last().map(|(r, _)| r.y1) { let k = self.pop.content; menu::separator(scene, Rect::new(k.x0, y, k.x1, y + t::SPACING_100 as f64 * s), s); }
            self.add.paint(scene, fonts, s, now); self.cancel.paint(scene, fonts, s, now); self.done.paint(scene, fonts, s, now);
            if self.renaming.is_some() { self.field.paint(scene, fonts, s); }
            return
        }
        let current = self.stops().get(self.at).copied();
        for (r, h) in self.rows.clone() {
            let lit = self.hover == Some(h) || (self.hover.is_none() && current == Some(h));
            match h {
                Hit::Option(i) => { let o = &self.options[i]; menu::row(scene, fonts, r, &o.label, true, lit, Some(self.chosen.as_deref() == Some(o.id.as_str())), true, s); dot(scene, r.x1 - 14.0 * s, r.center().y, &o.color) }
                Hit::Add => menu::row(scene, fonts, r, &format!("Add \u{201c}{}\u{201d}", self.field.text().trim()), true, lit, None, true, s),
                Hit::Clear => menu::row(scene, fonts, r, "Clear", true, lit, None, true, s),
                Hit::Edit => menu::row(scene, fonts, r, "Edit options\u{2026}", true, lit, None, true, s),
                Hit::Sep => menu::separator(scene, r, s),
                _ => {}
            }
        }
        self.field.paint(scene, fonts, s);
    }
}
