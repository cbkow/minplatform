//! Chips (canon row 490): the options of a dropdown or priority column as coloured chips - one look in the cell, in
//! the popover that picks a cell's value, and in the column's editor - made from min-ui's parts (the popover, the
//! fields, the buttons, the colour picker). What is stored does not change: an option is a name and a `#rrggbb`
//! colour, a cell holds the option's NAME (UFB 1.4.2's `column_options` and blobs).
//!
//! [`ChipPicker`]: under a cell, every option as its chip; typing narrows them, and a text that is no option yet can be
//! made one on the spot. [`ColumnEditor`]: a column's name and, for a dropdown or a priority, its options as rows - a
//! swatch (twelve colours to choose from, or any through the colour picker), the name, up, down, remove.
use kurbo::{Affine, Rect, RoundedRect};
use min_theme::tokens as t;
use min_ui::{action_button::Response, color_picker::ColorPicker, fonts::draw_layout, paint, text_button::Variant, ActionButton, Button, Event, Family, FieldResponse, Fonts, Key, KeyEvent, Named, Placement, Popover, TextButton, TextField};
use std::path::PathBuf;
use std::time::Instant;
use vello::Scene;

/// The colours offered for an option (and given to new ones in turn): twelve that read as chips on the dark surface.
pub const PRESETS: [&str; 12] = ["#4a90e2", "#2bb8a6", "#5fbf3f", "#a3c93a", "#e6b422", "#f08a24", "#e5484d", "#e667a8", "#9b6cf0", "#6673e5", "#9b9b9b", "#b08968"];
/// The colour a new option gets among `taken`: the first of the twelve not in use, else they go round.
pub fn next_color(taken: &[Option<String>]) -> String { PRESETS.iter().find(|p| !taken.iter().any(|c| c.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(p)))).unwrap_or(&PRESETS[taken.len() % PRESETS.len()]).to_string() }
pub fn hex(c: &str) -> Option<min_theme::Color> {
    let h = c.trim().strip_prefix('#')?; if h.len() != 6 { return None } let n = u32::from_str_radix(h, 16).ok()?;
    Some(min_theme::Color::rgba((n >> 16 & 255) as f32 / 255.0, (n >> 8 & 255) as f32 / 255.0, (n & 255) as f32 / 255.0, 1.0))
}
const GREY: min_theme::Color = min_theme::Color::rgba(0.5, 0.5, 0.5, 1.0);

/// A chip: the colour as a soft fill with a solid dot, the name in the content colour. Its left edge at `x`, centred on
/// `cy`, no wider than `max_w`. None colour (a value that is no option): grey. Returns its place.
pub fn paint_chip(scene: &mut Scene, fonts: &mut Fonts, text: &str, color: Option<&str>, x: f64, cy: f64, max_w: f64, s: f64) -> Rect {
    let fill = color.and_then(hex).unwrap_or(GREY); let h = 20.0 * s;
    let l = fonts.layout_fit(text, Family::Sans, t::BODY_SIZE_S * s as f32, t::FONT_WEIGHT_BOLD as f32, paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT), (max_w - 26.0 * s).max(4.0));
    let r = Rect::new(x, cy - h / 2.0, x + l.width() as f64 + 26.0 * s, cy + h / 2.0);
    paint::rounded(scene, r, h / 2.0, min_theme::Color::rgba(fill.r, fill.g, fill.b, 0.32)); paint::rounded(scene, Rect::new(r.x0 + 7.0 * s, cy - 3.0 * s, r.x0 + 13.0 * s, cy + 3.0 * s), 3.0 * s, fill);
    draw_layout(scene, &l, Affine::translate((r.x0 + 18.0 * s, cy - l.height() as f64 / 2.0)));
    r
}

// ------------------------------------------------------------------ the popover that picks a cell's value
/// What was picked: an option; a text made an option and set; nothing (the cell is cleared).
#[derive(Clone, Debug, PartialEq)]
pub enum Pick { Set(String), Create(String), Clear }
#[derive(Clone, Debug, PartialEq)]
enum Row { Option(usize), Create, Clear }
pub struct ChipPicker { pub item: PathBuf, pub col: usize, options: Vec<(String, Option<String>)>, current: String, filter: String, pop: Popover, rows: Vec<(Row, Rect)>, at: usize, anchor: Rect, bounds: Rect }
const PICK_W: f64 = 230.0; const PICK_ROW: f64 = 30.0; const PICK_HINT: f64 = 24.0;
impl ChipPicker {
    pub fn new(item: PathBuf, col: usize, options: Vec<(String, Option<String>)>, current: String) -> ChipPicker { ChipPicker { item, col, options, current, filter: String::new(), pop: Popover::new(""), rows: vec![], at: 0, anchor: Rect::ZERO, bounds: Rect::ZERO } }
    /// The rows as the typed text leaves them: the options that hold it, then "Create" when it is no option's name, then Clear.
    fn build(&self) -> Vec<Row> {
        let f = self.filter.trim().to_lowercase();
        let mut rows: Vec<Row> = self.options.iter().enumerate().filter(|(_, o)| f.is_empty() || o.0.to_lowercase().contains(&f)).map(|(i, _)| Row::Option(i)).collect();
        if !f.is_empty() && !self.options.iter().any(|o| o.0.to_lowercase() == f) { rows.push(Row::Create); }
        if f.is_empty() { rows.push(Row::Clear); }
        rows
    }
    pub fn open(&mut self, fonts: &mut Fonts, anchor: Rect, bounds: Rect, s: f64) {
        self.anchor = anchor; self.bounds = bounds; let rows = self.build();
        self.pop.open_at_prefer(fonts, anchor, PICK_W, PICK_HINT + rows.len().max(1) as f64 * PICK_ROW, bounds, s, Placement::Below);
        let c = self.pop.content; self.rows = rows.into_iter().enumerate().map(|(k, r)| (r, Rect::new(c.x0, c.y0 + PICK_HINT * s + k as f64 * PICK_ROW * s, c.x1, c.y0 + PICK_HINT * s + (k + 1) as f64 * PICK_ROW * s))).collect();
        self.at = self.at.min(self.rows.len().saturating_sub(1));
    }
    fn pick(&self, k: usize) -> Option<Pick> { match self.rows.get(k)?.0 { Row::Option(i) => Some(Pick::Set(self.options[i].0.clone())), Row::Create => Some(Pick::Create(self.filter.trim().to_string())), Row::Clear => Some(Pick::Clear) } }
    /// The pointer: (what was picked, the popover is to close).
    pub fn event(&mut self, e: &Event) -> (Option<Pick>, bool) {
        if let Some((x, y)) = e.pos() { if let Some(k) = self.rows.iter().position(|(_, r)| r.contains(kurbo::Point::new(x, y))) { self.at = k; } }
        match *e { Event::Down { x, y, button: Button::Primary } => { if !self.pop.hit(x, y) { return (None, true) } match self.rows.iter().position(|(_, r)| r.contains(kurbo::Point::new(x, y))) { Some(k) => (self.pick(k), true), None => (None, false) } }
            Event::Down { x, y, .. } => (None, !self.pop.hit(x, y)), _ => (None, false) }
    }
    /// A key: typing narrows, the arrows move, Return picks, Escape closes.
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts, s: f64) -> (Option<Pick>, bool) {
        match &k.key {
            Key::Named(Named::Escape) => return (None, true),
            Key::Named(Named::Enter) => return (self.pick(self.at), true),
            Key::Named(Named::Down) => self.at = (self.at + 1).min(self.rows.len().saturating_sub(1)),
            Key::Named(Named::Up) => self.at = self.at.saturating_sub(1),
            Key::Named(Named::Backspace) => { self.filter.pop(); self.at = 0; let (a, b) = (self.anchor, self.bounds); self.open(fonts, a, b, s); }
            _ => if let Some(text) = k.text.as_ref().filter(|t| !k.cmd && !t.chars().any(char::is_control)) { self.filter.push_str(text); self.at = 0; let (a, b) = (self.anchor, self.bounds); self.open(fonts, a, b, s); },
        }
        (None, false)
    }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, s: f64) {
        self.pop.paint(scene, fonts, s); let c = self.pop.content; let sub = paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT); let small = t::BODY_SIZE_S * s as f32;
        let hint = if self.filter.is_empty() { "Type to narrow, or to add an option".to_string() } else { self.filter.clone() };
        let l = fonts.layout_fit(&hint, Family::Sans, small, t::FONT_WEIGHT_REGULAR as f32, if self.filter.is_empty() { sub } else { paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT) }, c.width() - 12.0 * s);
        draw_layout(scene, &l, Affine::translate((c.x0 + 6.0 * s, c.y0 + (PICK_HINT * s - l.height() as f64) / 2.0 - 2.0 * s)));
        for (k, (row, r)) in self.rows.iter().enumerate() {
            if k == self.at { paint::rounded(scene, *r, t::CORNER_RADIUS_SMALL_DEFAULT as f64 * s, t::HOVER_BACKGROUND_COLOR); }
            let cy = (r.y0 + r.y1) / 2.0;
            match row {
                Row::Option(i) => { let (name, color) = &self.options[*i]; paint_chip(scene, fonts, name, Some(color.as_deref().unwrap_or("")), r.x0 + 6.0 * s, cy, r.width() - 40.0 * s, s);
                    if *name == self.current { if let Some(g) = fonts.icon("check", 13.0 * s as f32, paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT)) { draw_layout(scene, &g, Affine::translate((r.x1 - 8.0 * s - g.width() as f64, cy - g.height() as f64 / 2.0))); } } }
                Row::Create => { let l = fonts.layout("Create", Family::Sans, small, t::FONT_WEIGHT_REGULAR as f32, sub); draw_layout(scene, &l, Affine::translate((r.x0 + 8.0 * s, cy - l.height() as f64 / 2.0)));
                    let taken: Vec<Option<String>> = self.options.iter().map(|o| o.1.clone()).collect(); paint_chip(scene, fonts, self.filter.trim(), Some(&next_color(&taken)), r.x0 + 14.0 * s + l.width() as f64, cy, r.width() - 30.0 * s - l.width() as f64, s); }
                Row::Clear => { let l = fonts.layout("Clear", Family::Sans, small, t::FONT_WEIGHT_REGULAR as f32, sub); draw_layout(scene, &l, Affine::translate((r.x0 + 8.0 * s, cy - l.height() as f64 / 2.0))); }
            }
        }
    }
}

// ------------------------------------------------------------------ a column's editor
struct OptRow { name: TextField, color: String, /// the name it had when the editor opened (a rename takes the items' values along)
    orig: Option<String>, swatch: Rect, up: ActionButton, down: ActionButton, remove: ActionButton }
fn opt_row(name: &str, color: &str, orig: Option<String>) -> OptRow {
    let mut f = TextField::new(Family::Sans, t::DIALOG_BODY_SIZE); f.set_text(name); f.placeholder = "Option".into();
    OptRow { name: f, color: color.to_string(), orig, swatch: Rect::ZERO, up: ActionButton::new("arrow-up", "Move up").size_s(), down: ActionButton::new("arrow-down", "Move down").size_s(), remove: ActionButton::new("x", "Remove this option").size_s() }
}
/// The colours of an option: the twelve, and "Custom" - the colour picker.
struct Swatches { row: usize, pop: Popover, cells: Vec<Rect>, custom: Rect, picker: Option<ColorPicker> }
/// How the editor was left.
#[derive(Clone, Debug, PartialEq)]
pub enum Done { Cancel, /// the name, the options, and the options that were renamed: (the old name, the new)
    Save { name: String, options: Vec<(String, Option<String>)>, renames: Vec<(String, String)> } }
pub struct ColumnEditor { pub id: Option<i64>, pub kind: String, title: String, name: TextField, rows: Vec<OptRow>, with_options: bool, add: ActionButton, cancel: TextButton, save: TextButton, rect: Rect, swatches: Option<Swatches>, scale: f64 }
impl ColumnEditor {
    pub fn new(id: Option<i64>, kind: &str, name: &str, options: &[(String, Option<String>)], fonts: &mut Fonts) -> ColumnEditor {
        let k = crate::db::kind_of(kind); let with_options = matches!(k, "dropdown" | "priority"); let what = format!("{}{}", &k[..1].to_uppercase(), &k[1..]);
        let mut f = TextField::new(Family::Sans, t::DIALOG_BODY_SIZE); f.set_text_end(name, fonts); f.select_all(fonts); f.focused = true; f.placeholder = "Name".into();
        let mut add = ActionButton::new("plus", "").size_s(); add.label = Some("Add option".into());
        ColumnEditor { id, kind: kind.to_string(), title: if id.is_some() { format!("Edit {what} Column") } else { format!("New {what} Column") }, name: f, with_options,
            rows: options.iter().map(|(n, c)| opt_row(n, c.as_deref().unwrap_or("#9b9b9b"), Some(n.clone()))).collect(), add, cancel: TextButton::new("Cancel", Variant::Secondary), save: TextButton::new(if id.is_some() { "Save" } else { "Add" }, Variant::Primary), rect: Rect::ZERO, swatches: None, scale: 1.0 }
    }
    fn fields(&mut self) -> Vec<&mut TextField> { std::iter::once(&mut self.name).chain(self.rows.iter_mut().map(|r| &mut r.name)).collect() }
    fn focused(&mut self) -> Option<&mut TextField> { self.fields().into_iter().find(|f| f.focused) }
    fn focus(&mut self, k: usize, fonts: &mut Fonts) { for (i, f) in self.fields().into_iter().enumerate() { f.focused = i == k; if i == k { f.select_all(fonts); } } }
    fn add_row(&mut self, fonts: &mut Fonts) { let taken: Vec<Option<String>> = self.rows.iter().map(|r| Some(r.color.clone())).collect(); self.rows.push(opt_row("", &next_color(&taken), None)); let k = self.rows.len(); self.focus(k, fonts); }
    /// What Save gives: the options with a name (none twice), and the renames.
    fn result(&self) -> Done {
        let mut options: Vec<(String, Option<String>)> = vec![]; let mut renames = vec![];
        for r in &self.rows { let n = r.name.text().trim().to_string(); if n.is_empty() || options.iter().any(|o| o.0.eq_ignore_ascii_case(&n)) { continue }
            if let Some(o) = r.orig.as_ref().filter(|o| **o != n) { renames.push((o.clone(), n.clone())); } options.push((n, Some(r.color.clone()))); }
        Done::Save { name: self.name.text().trim().to_string(), options, renames }
    }
    /// The colours of option `row` are shown (a scripted look; a press on its swatch does the same). `custom`: with the colour picker open.
    pub fn show_swatches(&mut self, row: usize, custom: bool) { if let Some(r) = self.rows.get(row) { self.swatches = Some(Swatches { row, pop: Popover::new(""), cells: vec![], custom: Rect::ZERO, picker: custom.then(|| ColorPicker::new(&r.color)) }); } }
    fn usable(&self) -> bool { !self.name.text().trim().is_empty() }
    pub fn paste(&mut self, text: &str, fonts: &mut Fonts) { if let Some(f) = self.focused() { f.paste(text, fonts); } }
    pub fn copy(&mut self, cut: bool, fonts: &mut Fonts) -> Option<String> { self.focused().and_then(|f| f.copy(cut, fonts)) }
    pub fn animating(&self, now: Instant) -> bool { self.cancel.animating(now) || self.save.animating(now) || self.add.animating(now) || self.rows.iter().any(|r| r.up.animating(now) || r.down.animating(now) || r.remove.animating(now)) || true }

    pub fn layout(&mut self, fonts: &mut Fonts, bounds: Rect, s: f64) {
        self.scale = s; let pad = t::DIALOG_PADDING as f64 * s; let gap = t::SPACING_200 as f64 * s; let fh = t::FIELD_HEIGHT_M as f64 * s; let bh = t::BUTTON_HEIGHT_M as f64 * s; let sb = t::ACTION_BUTTON_HEIGHT_S as f64 * s;
        let w = (t::DIALOG_WIDTH_S as f64 * s).max(420.0 * s); let title_h = fonts.layout(&self.title, Family::Sans, t::DIALOG_TITLE_SIZE * s as f32, t::FONT_WEIGHT_BOLD as f32, peniko::Color::WHITE).height() as f64;
        let row_h = fh + 6.0 * s; let opts_h = if self.with_options { gap + 20.0 * s + self.rows.len() as f64 * row_h + sb + 6.0 * s } else { 0.0 };
        let h = pad + title_h + gap + fh + opts_h + gap + bh + pad;
        let x0 = bounds.x0 + (bounds.width() - w) / 2.0; let y0 = (bounds.y0 + (bounds.height() - h) / 2.0).max(bounds.y0 + 8.0 * s); self.rect = Rect::new(x0, y0, x0 + w, y0 + h);
        let mut y = y0 + pad + title_h + gap; self.name.layout(fonts, Rect::new(x0 + pad, y, x0 + w - pad, y + fh), s); y += fh;
        if self.with_options { y += gap + 20.0 * s;
            for r in self.rows.iter_mut() {
                let cy = y + fh / 2.0; r.swatch = Rect::new(x0 + pad, cy - 11.0 * s, x0 + pad + 22.0 * s, cy + 11.0 * s);
                let right = x0 + w - pad; r.remove.rect = Rect::new(right - sb, cy - sb / 2.0, right, cy + sb / 2.0); r.down.rect = Rect::new(right - 2.0 * sb - 2.0 * s, cy - sb / 2.0, right - sb - 2.0 * s, cy + sb / 2.0); r.up.rect = Rect::new(right - 3.0 * sb - 4.0 * s, cy - sb / 2.0, right - 2.0 * sb - 4.0 * s, cy + sb / 2.0);
                r.name.layout(fonts, Rect::new(r.swatch.x1 + 8.0 * s, y, r.up.rect.x0 - 8.0 * s, y + fh), s); y += row_h;
            }
            let aw = self.add.measure(fonts, s).0; self.add.rect = Rect::new(x0 + pad, y, x0 + pad + aw, y + sb);
        } else { self.add.rect = Rect::ZERO; }
        let by = self.rect.y1 - pad - bh; let (sw, cw) = (self.save.measure(fonts, s).0, self.cancel.measure(fonts, s).0); let bgap = t::DIALOG_BUTTON_GAP as f64 * s;
        self.save.rect = Rect::new(self.rect.x1 - pad - sw, by, self.rect.x1 - pad, by + bh); self.cancel.rect = Rect::new(self.save.rect.x0 - bgap - cw, by, self.save.rect.x0 - bgap, by + bh); self.save.enabled = self.usable();
        // the colours' popover lies under its swatch
        if let Some(sw) = self.swatches.as_mut() { if let Some(r) = self.rows.get(sw.row) {
            let (cell, cols) = (26.0, 6.0); let grid_h = (PRESETS.len() as f64 / cols).ceil() * cell; let picker_h = if sw.picker.is_some() { 196.0 } else { 0.0 };
            sw.pop.open_at_prefer(fonts, r.swatch, cols * cell, grid_h + 30.0 + picker_h, bounds, s, Placement::Below); let c = sw.pop.content;
            sw.cells = (0..PRESETS.len()).map(|i| { let (cx, cy) = (c.x0 + (i % cols as usize) as f64 * cell * s, c.y0 + (i / cols as usize) as f64 * cell * s); Rect::new(cx + 2.0 * s, cy + 2.0 * s, cx + (cell - 2.0) * s, cy + (cell - 2.0) * s) }).collect();
            sw.custom = Rect::new(c.x0, c.y0 + grid_h * s + 2.0 * s, c.x1, c.y0 + grid_h * s + 28.0 * s);
            if let Some(p) = sw.picker.as_mut() { p.layout(fonts, c.x0, c.x1, sw.custom.y1 + 6.0 * s, s); }
        } }
    }
    /// The pointer; `Some`: the editor is done.
    pub fn event(&mut self, e: &Event, fonts: &mut Fonts, now: Instant) -> Option<Done> {
        // the colours' popover takes the pointer while it is open
        if let Some(sw) = self.swatches.as_mut() {
            if let Some(p) = sw.picker.as_mut() { let (hexv, _) = p.event(e, fonts); if let (Some(h), Some(r)) = (hexv, self.rows.get_mut(sw.row)) { r.color = h; } if p.dragging() { return None } }
            if let Event::Down { x, y, .. } = *e {
                if !sw.pop.hit(x, y) { self.swatches = None; return None }
                if let Some(i) = sw.cells.iter().position(|c| c.contains(kurbo::Point::new(x, y))) { if let Some(r) = self.rows.get_mut(sw.row) { r.color = PRESETS[i].to_string(); } self.swatches = None; return None }
                if sw.custom.contains(kurbo::Point::new(x, y)) && sw.picker.is_none() { let cur = self.rows.get(sw.row).map(|r| r.color.clone()).unwrap_or_default(); sw.picker = Some(ColorPicker::new(&cur)); }
            }
            return None
        }
        if self.cancel.event(e, now).0 { return Some(Done::Cancel) }
        if self.save.event(e, now).0 && self.usable() { return Some(self.result()) }
        if self.with_options {
            if self.add.event(e, now) == Response::Clicked { self.add_row(fonts); return None }
            let mut act: Option<(usize, u8)> = None;
            for (i, r) in self.rows.iter_mut().enumerate() { if r.up.event(e, now) == Response::Clicked { act = Some((i, 0)); } if r.down.event(e, now) == Response::Clicked { act = Some((i, 1)); } if r.remove.event(e, now) == Response::Clicked { act = Some((i, 2)); }
                if let Event::Down { x, y, button: Button::Primary } = *e { if r.swatch.contains(kurbo::Point::new(x, y)) { act = Some((i, 3)); } } }
            match act { Some((i, 0)) if i > 0 => self.rows.swap(i, i - 1), Some((i, 1)) if i + 1 < self.rows.len() => self.rows.swap(i, i + 1), Some((i, 2)) => { self.rows.remove(i); }
                Some((i, 3)) => { self.swatches = Some(Swatches { row: i, pop: Popover::new(""), cells: vec![], custom: Rect::ZERO, picker: None }); return None } _ => {} }
            if act.is_some() { return None }
        }
        // the fields: a press on one gives it the keys
        let mut hit = None; for (i, f) in self.fields().into_iter().enumerate() { f.pointer(e, fonts); if let Event::Down { x, y, .. } = *e { if f.hit(x, y) { hit = Some(i); } } }
        if let Some(k) = hit { for (i, f) in self.fields().into_iter().enumerate() { f.focused = i == k; } }
        None
    }
    /// A key: Tab goes through the fields, Return saves (in the last option's field with a name in it: another option), Escape leaves.
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) -> Option<Done> {
        if let Some(sw) = self.swatches.as_mut() { if matches!(k.key, Key::Named(Named::Escape)) { self.swatches = None; } else if let Some(p) = sw.picker.as_mut() { if let (Some(h), Some(r)) = (p.key(k, fonts), self.rows.get_mut(sw.row)) { r.color = h; } } return None }
        let n = 1 + self.rows.len(); let at = self.fields().iter().position(|f| f.focused).unwrap_or(0);
        if matches!(k.key, Key::Named(Named::Tab)) { self.focus(if k.shift { (at + n - 1) % n } else { (at + 1) % n }, fonts); return None }
        let Some(f) = self.focused() else { return None };
        match f.key(k, fonts) {
            FieldResponse::Accepted => { if self.with_options && at == n - 1 && at > 0 && !self.rows[at - 1].name.text().trim().is_empty() && !k.cmd { self.add_row(fonts); None } else if self.usable() { Some(self.result()) } else { None } }
            FieldResponse::Cancelled => Some(Done::Cancel),
            _ => None,
        }
    }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, bounds: Rect, now: Instant) {
        let s = self.scale; let pad = t::DIALOG_PADDING as f64 * s; let r = t::DIALOG_CORNER_RADIUS as f64 * s;
        paint::rect(scene, bounds, t::DIALOG_SCRIM_COLOR);
        for sh in t::POPOVER_SHADOW { scene.draw_blurred_rounded_rect(Affine::translate((sh.x as f64 * s, sh.y as f64 * s)), self.rect.inflate(sh.spread as f64 * s, sh.spread as f64 * s), paint::color(sh.color), r, (sh.blur as f64 * s / 2.0).max(0.1)); }
        scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, paint::color(t::DIALOG_BACKGROUND_COLOR), None, &RoundedRect::from_rect(self.rect, r));
        let title = fonts.layout(&self.title, Family::Sans, t::DIALOG_TITLE_SIZE * s as f32, t::FONT_WEIGHT_BOLD as f32, paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT)); draw_layout(scene, &title, Affine::translate((self.rect.x0 + pad, self.rect.y0 + pad)));
        self.name.paint(scene, fonts, s);
        if self.with_options {
            let l = fonts.layout("Options", Family::Sans, t::DIALOG_BODY_SIZE * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::DIALOG_BODY_COLOR)); draw_layout(scene, &l, Affine::translate((self.rect.x0 + pad, self.name.rect.y1 + t::SPACING_200 as f64 * s)));
            let corner = Corners::all(t::ACTION_BUTTON_CORNER_RADIUS_S as f64 * s); let last = self.rows.len().saturating_sub(1);
            for (i, row) in self.rows.iter_mut().enumerate() {
                let c = hex(&row.color).unwrap_or(GREY); paint::rounded(scene, row.swatch, 6.0 * s, min_theme::Color::rgba(c.r, c.g, c.b, 0.32)); paint::rounded(scene, row.swatch.inset(-6.0 * s), 5.0 * s, c);
                row.name.paint(scene, fonts, s);
                row.up.enabled = i > 0; row.down.enabled = i < last; for b in [&mut row.up, &mut row.down, &mut row.remove] { b.corners = corner; b.paint(scene, fonts, s, now); }
            }
            self.add.corners = corner; self.add.paint(scene, fonts, s, now);
        }
        self.cancel.paint(scene, fonts, s, now); self.save.paint(scene, fonts, s, now);
        if let Some(sw) = self.swatches.as_mut() {
            sw.pop.paint(scene, fonts, s); let cur = self.rows.get(sw.row).map(|r| r.color.to_lowercase()).unwrap_or_default();
            for (i, cell) in sw.cells.iter().enumerate() { let c = hex(PRESETS[i]).unwrap_or(GREY); if PRESETS[i] == cur { paint::rounded(scene, cell.inflate(2.0 * s, 2.0 * s), 7.0 * s, t::NEUTRAL_CONTENT_COLOR_DEFAULT); } paint::rounded(scene, *cell, 5.0 * s, c); }
            let l = fonts.layout("Custom\u{2026}", Family::Sans, t::BODY_SIZE_S * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(if sw.picker.is_some() { t::NEUTRAL_CONTENT_COLOR_DEFAULT } else { t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT })); draw_layout(scene, &l, Affine::translate((sw.custom.x0 + 4.0 * s, sw.custom.y0 + (sw.custom.height() - l.height() as f64) / 2.0)));
            if let Some(p) = sw.picker.as_mut() { p.paint(scene, fonts, s); }
        }
    }
}
use min_ui::Corners;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_new_option_gets_the_first_colour_not_in_use() {
        assert_eq!(next_color(&[]), PRESETS[0]); assert_eq!(next_color(&[Some(PRESETS[0].to_uppercase()), Some(PRESETS[2].into())]), PRESETS[1]);
        let all: Vec<Option<String>> = PRESETS.iter().map(|p| Some(p.to_string())).collect(); assert_eq!(next_color(&all), PRESETS[0], "all twelve in use: they go round");
        assert!(PRESETS.iter().all(|p| hex(p).is_some()));
    }
    #[test]
    fn typing_narrows_the_options_and_offers_a_new_one() {
        let opts = vec![("WIP".to_string(), Some("#e6b422".to_string())), ("Review".to_string(), None), ("Final".to_string(), None)];
        let mut p = ChipPicker::new(PathBuf::from("/j/3d/sh010"), 0, opts, "WIP".into());
        assert_eq!(p.build(), [Row::Option(0), Row::Option(1), Row::Option(2), Row::Clear]);
        p.filter = "re".into(); assert_eq!(p.build(), [Row::Option(1), Row::Create], "a text that is no option's name can be made one");
        p.filter = "review".into(); assert_eq!(p.build(), [Row::Option(1)], "an option's own name (letters' case apart): nothing to create");
        p.filter = "Blocked".into(); assert_eq!(p.build(), [Row::Create]);
    }
}
