//! The colour palette (canon row 301): ONE popover for every role — Type (the text's colour), Background (behind the
//! text; a tint, so the text on it is never changed), and later Pen on canvases. It offers Default (the role's colour
//! taken away), the named colours, the collection's own colours, and "+", which opens the picker and adds what is
//! picked to the collection. A right press on one of the collection's colours removes it from the palette.
use std::time::Instant;
use kurbo::{Affine, Rect, RoundedRect, Stroke};
use min_theme::tokens as t;
use min_ui::{color_picker::ColorPicker, fonts::draw_layout, paint, text_button::{TextButton, Variant}, Button, Event, Fonts, KeyEvent, Placement, Popover};
use mn_doc::MarkKind;
use vello::Scene;

const COLS: usize = 6;
const SZ: f64 = 24.0;
const GAP: f64 = 6.0;

#[derive(Clone, Debug, PartialEq)]
enum Cell { Default, Named(&'static str), Custom(String), Plus }
#[derive(Debug, PartialEq)]
pub enum Out { None, Redraw, /// the role's colour: a name, "#rrggbb", or None for the default
    Pick(Option<String>), /// a picked colour to add to the collection and apply
    Add(String), Remove(String) }

pub struct Colours { pop: Popover, pub role: MarkKind, cells: Vec<(Rect, Cell)>, customs: Vec<String>, current: Option<String>, picker: Option<ColorPicker>, add: TextButton,
    hover: Option<usize>, pressed: Option<usize>, anchor: Rect, bounds: Rect, scale: f64 }

impl Colours {
    pub fn new() -> Colours { Colours { pop: Popover::new(""), role: MarkKind::Color, cells: vec![], customs: vec![], current: None, picker: None, add: TextButton::new("Add to Palette", Variant::Primary).size_s(), hover: None, pressed: None, anchor: Rect::ZERO, bounds: Rect::ZERO, scale: 1.0 } }
    pub fn is_open(&self) -> bool { self.pop.open }
    pub fn close(&mut self) { self.pop.close(); self.picker = None; self.hover = None; self.pressed = None; }
    /// The hex field of the picker has the keys.
    pub fn typing(&self) -> bool { self.pop.open && self.picker.as_ref().is_some_and(|p| p.focused()) }
    pub fn open(&mut self, role: MarkKind, anchor: Rect, customs: Vec<String>, current: Option<String>, fonts: &mut Fonts, bounds: Rect, scale: f64) {
        self.role = role; self.anchor = anchor; self.bounds = bounds; self.scale = scale; self.customs = customs; self.current = current; self.picker = None; self.hover = None; self.pressed = None;
        self.pop.title = match role { MarkKind::Color => "Type colour", _ => "Background colour" }.to_string();
        self.relayout(fonts);
    }
    pub fn set_customs(&mut self, customs: Vec<String>, fonts: &mut Fonts) { self.customs = customs; self.relayout(fonts); }
    fn relayout(&mut self, fonts: &mut Fonts) {
        let s = self.scale;
        let mut cells = vec![Cell::Default]; cells.extend(mn_layout::NAMED.iter().map(|(n, _)| Cell::Named(n))); cells.extend(self.customs.iter().cloned().map(Cell::Custom)); cells.push(Cell::Plus);
        let rows = cells.len().div_ceil(COLS);
        let w = COLS as f64 * SZ + (COLS - 1) as f64 * GAP; let grid_h = rows as f64 * SZ + (rows - 1) as f64 * GAP;
        let (bw, bh) = self.add.measure(fonts, s);
        // the picker's height is its own to say: measured, then the popover is opened with room for it
        let picker_h = self.picker.as_mut().map(|p| p.layout(fonts, 0.0, w * s, 0.0, s) / s);
        let h = grid_h + picker_h.map(|ph| 2.0 * GAP + ph + GAP + bh / s).unwrap_or(0.0);
        self.pop.open_at_prefer(fonts, self.anchor, w, h, self.bounds, s, Placement::Below);
        let c = self.pop.content;
        self.cells = cells.into_iter().enumerate().map(|(i, cell)| { let (col, row) = ((i % COLS) as f64, (i / COLS) as f64); let (x, y) = (c.x0 + col * (SZ + GAP) * s, c.y0 + row * (SZ + GAP) * s); (Rect::new(x, y, x + SZ * s, y + SZ * s), cell) }).collect();
        if let Some(p) = self.picker.as_mut() { let y = c.y0 + (grid_h + 2.0 * GAP) * s; let ph = p.layout(fonts, c.x0, c.x1, y, s); self.add.rect = Rect::new(c.x1 - bw, y + ph + GAP * s, c.x1, y + ph + GAP * s + bh); }
    }
    pub fn show_picker(&mut self, fonts: &mut Fonts) { if self.picker.is_none() { let start = self.current.as_deref().and_then(to_hex).unwrap_or_else(|| "#ff5768".to_string()); self.picker = Some(ColorPicker::new(&start)); self.relayout(fonts); } }
    fn at(&self, x: f64, y: f64) -> Option<usize> { self.cells.iter().position(|(r, _)| r.contains(kurbo::Point::new(x, y))) }
    /// A pointer event while the palette is open (every event is its own; a press outside closes it).
    pub fn event(&mut self, e: &Event, fonts: &mut Fonts, now: Instant) -> Out {
        if !self.pop.open { return Out::None }
        if let Event::Down { x, y, .. } = *e { if !self.pop.hit(x, y) { self.close(); return Out::Redraw } }
        if let Some(p) = self.picker.as_mut() {
            let dragging = p.dragging(); let (_, _) = p.event(e, fonts);
            if self.add.event(e, now).0 { return Out::Add(p.hex()) }
            if dragging || p.dragging() || p.focused() && !matches!(e, Event::Move { .. }) && e.pos().is_some_and(|(x, y)| p.rect.contains(kurbo::Point::new(x, y))) { return Out::Redraw }
        }
        match *e {
            Event::Move { x, y } => { self.hover = self.at(x, y); Out::Redraw }
            Event::Down { x, y, button: Button::Secondary } => match self.at(x, y).map(|i| self.cells[i].1.clone()) { Some(Cell::Custom(c)) => Out::Remove(c), _ => Out::Redraw },
            Event::Down { x, y, .. } => { self.pressed = self.at(x, y); Out::Redraw }
            Event::Up { x, y, .. } => {
                let hit = self.at(x, y).filter(|i| self.pressed == Some(*i)); self.pressed = None;
                match hit.map(|i| self.cells[i].1.clone()) {
                    Some(Cell::Default) => Out::Pick(None), Some(Cell::Named(n)) => Out::Pick(Some(n.to_string())), Some(Cell::Custom(c)) => Out::Pick(Some(c)),
                    // "+": the picker opens under the swatches, starting from the colour in effect
                    Some(Cell::Plus) => { self.show_picker(fonts); Out::Redraw }
                    None => Out::Redraw,
                }
            }
            Event::Leave => { self.hover = None; Out::Redraw }
        }
    }
    /// A key for the picker's hex field.
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) { if let Some(p) = self.picker.as_mut() { let _ = p.key(k, fonts); } }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, page: peniko::Color, now: Instant) {
        if !self.pop.open { return }
        let s = self.scale; self.pop.paint(scene, fonts, s);
        let r = t::CORNER_RADIUS_SMALL_DEFAULT as f64 * s;
        for (i, (rect, cell)) in self.cells.iter().enumerate() {
            let rr = RoundedRect::from_rect(*rect, r);
            let ring = |scene: &mut Scene, c: min_theme::Color, w: f64| scene.stroke(&Stroke::new(w * s), Affine::IDENTITY, paint::color(c), None, &RoundedRect::from_rect(rect.inflate(2.0 * s, 2.0 * s), r + 2.0 * s));
            match cell {
                // the role's default: the page itself, struck through
                Cell::Default => { scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, page, None, &rr); scene.stroke(&Stroke::new(1.0 * s), Affine::IDENTITY, paint::color(t::DIVIDER_COLOR), None, &rr);
                    scene.stroke(&Stroke::new(1.5 * s), Affine::IDENTITY, paint::color(t::NEGATIVE_VISUAL_COLOR), None, &kurbo::Line::new((rect.x0 + 4.0 * s, rect.y1 - 4.0 * s), (rect.x1 - 4.0 * s, rect.y0 + 4.0 * s))); }
                Cell::Named(n) => { if let Some(c) = mn_layout::colour(n) { scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, c, None, &rr); } }
                Cell::Custom(h) => { if let Some(c) = mn_layout::colour(h) { scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, c, None, &rr); } }
                Cell::Plus => { scene.stroke(&Stroke::new(1.0 * s), Affine::IDENTITY, paint::color(t::DIVIDER_COLOR), None, &rr);
                    if let Some(g) = fonts.icon("plus", t::ICON_SIZE_SMALL * s as f32, paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT)) { draw_layout(scene, &g, Affine::translate((rect.x0 + (rect.width() - g.width() as f64) / 2.0, rect.y0 + (rect.height() - g.height() as f64) / 2.0))); } }
            }
            let is_current = match cell { Cell::Default => self.current.is_none(), Cell::Named(n) => self.current.as_deref() == Some(*n), Cell::Custom(h) => self.current.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(h)), Cell::Plus => false };
            if is_current { ring(scene, t::NEUTRAL_CONTENT_COLOR_DEFAULT, 1.5); } else if self.hover == Some(i) { ring(scene, t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT, 1.0); }
        }
        if let Some(p) = self.picker.as_mut() { p.paint(scene, fonts, s); self.add.paint(scene, fonts, s, now); }
    }
}

/// A colour of either spelling as "#rrggbb" (the picker's start).
fn to_hex(c: &str) -> Option<String> { mn_layout::NAMED.iter().find(|(n, _)| *n == c).map(|(_, h)| h.to_lowercase()).or_else(|| c.starts_with('#').then(|| c.to_string())) }
