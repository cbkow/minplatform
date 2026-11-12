//! A choice cell's picker (canon row 317, stage 3): a list under the cell - a field that filters the options and adds a
//! new one, the options, Clear, "Edit options…". Its second face edits the column's options on a DRAFT (rename,
//! recolour, reorder, remove, add); Done hands the whole set back as one change, Cancel drops it.
use kurbo::{Affine, BezPath, Circle, Line, Rect, Stroke};
use min_theme::tokens as t;
use min_ui::{fonts::draw_layout, Event, Family, FieldResponse, Fonts, Key, KeyEvent, Named, Placement, Popover, TextField};
use mn_doc::{BlockId, ChoiceOption};
use mn_edit::typed::PALETTE;
use peniko::{Color, Fill};
use vello::Scene;

pub enum Out { None, Redraw, Close, Pick(Option<String>), Add(String), Options(Vec<ChoiceOption>) }
#[derive(Clone, Copy, PartialEq, Debug)]
enum Hit { Option(usize), Add, Clear, Edit, Swatch(usize), Label(usize), Up(usize), Down(usize), Remove(usize), New, Done, Cancel }

pub struct ChoicePicker {
    /// the cell it stands under
    pub cell: BlockId,
    pop: Popover, field: TextField, options: Vec<ChoiceOption>, chosen: Option<String>,
    hover: Option<Hit>, at: usize, edit: bool, renaming: Option<usize>, rows: Vec<(Rect, Hit)>, scale: f64,
}
const W: f64 = 248.0; const ROW: f64 = 28.0; const FIELD: f64 = 30.0;
fn c(k: min_theme::Color) -> Color { min_ui::paint::color(k) }
fn hex(s: &str) -> Color { mn_layout::hex(s).unwrap_or(c(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT)) }

impl ChoicePicker {
    /// `typed`: what was typed to open it (it goes into the field, and the list is filtered by it).
    pub fn new(cell: BlockId, options: Vec<ChoiceOption>, chosen: Option<String>, typed: &str, fonts: &mut Fonts) -> ChoicePicker {
        let mut field = TextField::new(Family::Sans, 13.0); field.placeholder = "Find or add\u{2026}".into(); field.set_text_end(typed, fonts); field.focused = true;
        ChoicePicker { cell, pop: Popover::new(""), field, options, chosen, hover: None, at: 0, edit: false, renaming: None, rows: vec![], scale: 1.0 }
    }
    /// Opened on its options' editor.
    pub fn editing(mut self) -> ChoicePicker { self.edit = true; self.field.clear(); self.field.placeholder.clear(); self.field.focused = false; self }
    pub fn hit(&self, x: f64, y: f64) -> bool { self.pop.hit(x, y) }
    fn filter(&self) -> String { self.field.text().trim().to_lowercase() }
    /// The rows of the list, in order (without their places).
    fn list(&self) -> Vec<Hit> {
        if self.edit { let mut v: Vec<Hit> = (0..self.options.len()).map(Hit::Label).collect(); v.push(Hit::New); v.push(Hit::Done); return v }
        let f = self.filter();
        let mut v: Vec<Hit> = self.options.iter().enumerate().filter(|(_, o)| f.is_empty() || o.label.to_lowercase().contains(&f)).map(|(i, _)| Hit::Option(i)).collect();
        if !f.is_empty() && !self.options.iter().any(|o| o.label.trim().to_lowercase() == f) { v.push(Hit::Add); }
        if self.chosen.is_some() && f.is_empty() { v.push(Hit::Clear); }
        if f.is_empty() { v.push(Hit::Edit); }
        v
    }
    /// Placed under `anchor` (the cell, window px) within `bounds`.
    pub fn layout(&mut self, fonts: &mut Fonts, anchor: Rect, bounds: Rect, s: f64) {
        self.scale = s; let list = self.list(); let top = if self.edit { 0.0 } else { FIELD + 4.0 };
        self.pop.open_at_prefer(fonts, anchor, W, top + list.len() as f64 * ROW, bounds, s, Placement::Below);
        let k = self.pop.content; self.rows.clear(); let mut y = k.y0 + top * s;
        for h in list { let r = Rect::new(k.x0, y, k.x1, y + ROW * s); y += ROW * s;
            match h {
                Hit::Label(i) => { let b = 22.0 * s; self.rows.push((Rect::new(r.x0, r.y0, r.x0 + 26.0 * s, r.y1), Hit::Swatch(i)));
                    self.rows.push((Rect::new(r.x0 + 26.0 * s, r.y0, r.x1 - 3.0 * b, r.y1), Hit::Label(i)));
                    self.rows.push((Rect::new(r.x1 - 3.0 * b, r.y0, r.x1 - 2.0 * b, r.y1), Hit::Up(i))); self.rows.push((Rect::new(r.x1 - 2.0 * b, r.y0, r.x1 - b, r.y1), Hit::Down(i))); self.rows.push((Rect::new(r.x1 - b, r.y0, r.x1, r.y1), Hit::Remove(i))); }
                Hit::Done => { let m = r.center().x; self.rows.push((Rect::new(r.x0, r.y0, m, r.y1), Hit::Cancel)); self.rows.push((Rect::new(m, r.y0, r.x1, r.y1), Hit::Done)); }
                h => self.rows.push((r, h)),
            }
        }
        if self.edit { if let Some(i) = self.renaming { if let Some((r, _)) = self.rows.iter().find(|(_, h)| *h == Hit::Label(i)) { self.field.layout(fonts, r.inset((0.0, -2.0 * s)), s); } } }
        else { self.field.layout(fonts, Rect::new(k.x0, k.y0, k.x1, k.y0 + FIELD * s), s); }
        if !self.edit { let n = self.list().len(); self.at = self.at.min(n.saturating_sub(1)); }
    }
    fn at_point(&self, x: f64, y: f64) -> Option<Hit> { self.rows.iter().find(|(r, _)| r.contains(kurbo::Point::new(x, y))).map(|(_, h)| *h) }
    fn rename_end(&mut self, keep: bool) {
        if let Some(i) = self.renaming.take() { let l = mn_edit::typed::clean_label(self.field.text()); if keep && !l.is_empty() { if let Some(o) = self.options.get_mut(i) { o.label = l; } } else if self.options.get(i).is_some_and(|o| o.label.is_empty()) { self.options.remove(i); } }
    }
    fn act(&mut self, h: Hit, fonts: &mut Fonts) -> Out {
        match h {
            Hit::Option(i) => Out::Pick(self.options.get(i).map(|o| o.id.clone())),
            Hit::Add => Out::Add(self.field.text().trim().to_string()),
            Hit::Clear => Out::Pick(None),
            Hit::Edit => { self.edit = true; self.field.clear(); self.field.placeholder.clear(); self.field.focused = false; Out::Redraw }
            Hit::Swatch(i) => { self.rename_end(true); if let Some(o) = self.options.get_mut(i) { let k = PALETTE.iter().position(|p| p.eq_ignore_ascii_case(&o.color)).map(|k| k + 1).unwrap_or(0); o.color = PALETTE[k % PALETTE.len()].to_string(); } Out::Redraw }
            Hit::Label(i) => { self.rename_end(true); if let Some(o) = self.options.get(i) { self.field.set_text_end(&o.label, fonts); self.field.select_all(fonts); self.field.focused = true; self.renaming = Some(i); } Out::Redraw }
            Hit::Up(i) => { self.rename_end(true); if i > 0 && i < self.options.len() { self.options.swap(i, i - 1); } Out::Redraw }
            Hit::Down(i) => { self.rename_end(true); if i + 1 < self.options.len() { self.options.swap(i, i + 1); } Out::Redraw }
            Hit::Remove(i) => { self.rename_end(true); if i < self.options.len() { self.options.remove(i); } Out::Redraw }
            Hit::New => { self.rename_end(true); let n = self.options.len(); self.options.push(ChoiceOption { id: BlockId::new().to_string(), label: String::new(), color: PALETTE[n % PALETTE.len()].to_string() });
                self.field.clear(); self.field.focused = true; self.renaming = Some(n); Out::Redraw }
            Hit::Done => { self.rename_end(true); Out::Options(self.options.clone()) }
            Hit::Cancel => Out::Close,
        }
    }
    /// A pointer event. A press outside closes it (and is not taken: the page gets it).
    pub fn pointer(&mut self, e: &Event, fonts: &mut Fonts) -> (Out, bool) {
        match *e {
            Event::Move { x, y } => { let h = self.at_point(x, y); let ch = h != self.hover; self.hover = h; if self.field.focused { self.field.pointer(e, fonts); } (if ch { Out::Redraw } else { Out::None }, self.hit(x, y)) }
            Event::Down { x, y, .. } => { if !self.hit(x, y) { return (if self.edit { self.rename_end(true); Out::Options(self.options.clone()) } else { Out::Close }, false) }
                if self.field.focused && self.field.hit(x, y) { self.field.pointer(e, fonts); return (Out::Redraw, true) }
                match self.at_point(x, y) { Some(h) => (self.act(h, fonts), true), None => (Out::None, true) } }
            Event::Up { .. } => { if self.field.focused { self.field.pointer(e, fonts); } (Out::None, true) }
            _ => (Out::None, false),
        }
    }
    pub fn ime(&mut self, e: &min_ui::ImeEvent, fonts: &mut Fonts) { if self.field.focused { self.field.ime(e, fonts); self.at = 0; } }
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) -> Out {
        if self.edit {
            if self.renaming.is_some() { return match self.field.key(k, fonts) { FieldResponse::Accepted => { self.rename_end(true); self.field.focused = false; Out::Redraw } FieldResponse::Cancelled => { self.rename_end(false); self.field.focused = false; Out::Redraw } _ => Out::Redraw } }
            return match k.key { Key::Named(Named::Escape) => Out::Close, Key::Named(Named::Enter) => Out::Options(self.options.clone()), _ => Out::None }
        }
        let n = self.list().len();
        match k.key {
            Key::Named(Named::Escape) => Out::Close,
            Key::Named(Named::Down) => { self.at = (self.at + 1).min(n.saturating_sub(1)); Out::Redraw }
            Key::Named(Named::Up) => { self.at = self.at.saturating_sub(1); Out::Redraw }
            Key::Named(Named::Enter) | Key::Named(Named::Tab) => match self.list().get(self.at).copied() { Some(h) => self.act(h, fonts), None => Out::Close },
            _ => { if self.field.key(k, fonts) == FieldResponse::Changed { self.at = 0; } Out::Redraw }
        }
    }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts) {
        let s = self.scale; self.pop.paint(scene, fonts, s);
        let (text, dim) = (c(t::NEUTRAL_CONTENT_COLOR_DEFAULT), c(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT));
        let list = self.list(); let current = if self.edit { None } else { list.get(self.at).copied() };
        let label = |scene: &mut Scene, fonts: &mut Fonts, r: Rect, x: f64, w: &str, col: Color| { let g = fonts.layout_fit(w, Family::Sans, (13.0 * s) as f32, 400.0, col, (r.x1 - x - 4.0 * s).max(8.0)); draw_layout(scene, &g, Affine::translate((x, r.center().y - g.height() as f64 / 2.0))); };
        let glyph = |scene: &mut Scene, r: Rect, kind: u8, col: Color| { let (m, a) = (r.center(), 4.0 * s); let mut p = BezPath::new();
            match kind { 0 => { p.move_to((m.x - a, m.y + a / 2.0)); p.line_to((m.x, m.y - a / 2.0)); p.line_to((m.x + a, m.y + a / 2.0)); }
                1 => { p.move_to((m.x - a, m.y - a / 2.0)); p.line_to((m.x, m.y + a / 2.0)); p.line_to((m.x + a, m.y - a / 2.0)); }
                _ => { p.move_to((m.x - a, m.y - a)); p.line_to((m.x + a, m.y + a)); p.move_to((m.x + a, m.y - a)); p.line_to((m.x - a, m.y + a)); } }
            scene.stroke(&Stroke::new(1.4 * s), Affine::IDENTITY, col, None, &p); };
        for (r, h) in self.rows.clone() {
            let lit = self.hover == Some(h) || current == Some(h);
            if lit && !matches!(h, Hit::Label(i) if self.renaming == Some(i)) { min_ui::paint::rounded(scene, r.inset((-1.0 * s, -1.0 * s)), 4.0 * s, t::BACKGROUND_LAYER_2_COLOR); }
            match h {
                Hit::Option(i) => { let o = &self.options[i]; scene.fill(Fill::NonZero, Affine::IDENTITY, hex(&o.color), None, &Circle::new((r.x0 + 13.0 * s, r.center().y), 5.0 * s)); label(scene, fonts, r, r.x0 + 26.0 * s, &o.label, text);
                    if self.chosen.as_deref() == Some(o.id.as_str()) { let (m, a) = ((r.x1 - 14.0 * s, r.center().y), 4.0 * s); let mut p = BezPath::new(); p.move_to((m.0 - a, m.1)); p.line_to((m.0 - a / 3.0, m.1 + a * 0.8)); p.line_to((m.0 + a, m.1 - a * 0.8)); scene.stroke(&Stroke::new(1.6 * s), Affine::IDENTITY, text, None, &p); } }
                Hit::Add => label(scene, fonts, r, r.x0 + 8.0 * s, &format!("Add \u{201c}{}\u{201d}", self.field.text().trim()), text),
                Hit::Clear => label(scene, fonts, r, r.x0 + 8.0 * s, "Clear", dim),
                Hit::Edit => { scene.stroke(&Stroke::new(s.max(1.0)), Affine::IDENTITY, c(t::DIVIDER_COLOR), None, &Line::new((r.x0, r.y0), (r.x1, r.y0))); label(scene, fonts, r, r.x0 + 8.0 * s, "Edit options\u{2026}", dim) }
                Hit::Swatch(i) => scene.fill(Fill::NonZero, Affine::IDENTITY, hex(&self.options[i].color), None, &Circle::new((r.x0 + 13.0 * s, r.center().y), 6.0 * s)),
                Hit::Label(i) => if self.renaming != Some(i) { label(scene, fonts, r, r.x0, &self.options[i].label, text) },
                Hit::Up(i) => glyph(scene, r, 0, if i > 0 { dim } else { c(t::DIVIDER_COLOR) }),
                Hit::Down(i) => glyph(scene, r, 1, if i + 1 < self.options.len() { dim } else { c(t::DIVIDER_COLOR) }),
                Hit::Remove(_) => glyph(scene, r, 2, dim),
                Hit::New => label(scene, fonts, r, r.x0 + 8.0 * s, "Add option", dim),
                Hit::Cancel => { scene.stroke(&Stroke::new(s.max(1.0)), Affine::IDENTITY, c(t::DIVIDER_COLOR), None, &Line::new((r.x0, r.y0), (r.x1 + r.width(), r.y0))); label(scene, fonts, r, r.x0 + 8.0 * s, "Cancel", dim) }
                Hit::Done => label(scene, fonts, r, r.x0 + 8.0 * s, "Done", text),
            }
        }
        if !self.edit || self.renaming.is_some() { self.field.paint(scene, fonts, s); }
    }
}
