//! The Notes drawer (canon row 215, QCView `NotesPanel.qml` carried into our drawer slot — it swaps with Color, one open at a
//! time; Ctrl+4 and the transport's note button): the header (NOTES · the clip · the count), "Add note here" (captures the
//! frame at the playhead), the note cards in a sideways-scrolling row (the thumbnail — annotated when there are strokes — the
//! timecode, VIEWER / OCIO badges when the note carries them, the text "Note…" edited in place, Mark addressed, Delete note;
//! a click on the thumbnail jumps to the note's frame), QCView's seven tools, and the colour + "Width:" column.
use crate::annotate::Tool;
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use min_ui::{anim::Ramp, paint, ActionButton, ColorPicker, Event, Family, FieldResponse, Fonts, ImeEvent, KeyEvent, Response, Slider, TextField, Tooltips};
use std::{collections::HashMap, path::PathBuf, time::{Instant, SystemTime}};
use vello::Scene;

fn tr(s: &str) -> &str { min_i18n::trc("NotesPanel", s) }

/// The drawer's height (logical px): QCView's band was 270; ours carries the header row.
pub const HEIGHT: f64 = 300.0;
const CARD_W: f64 = 240.0; const THUMB_H: f64 = 135.0; const ADD_W: f64 = 120.0; const TOOLS_W: f64 = 32.0; const PICK_W: f64 = 186.0;
pub const TOOLS: [(Tool, &str, &str); 7] = [
    (Tool::Select, "cursor", "Select (drag to move, corners to scale, Del to remove; click again to exit)"), (Tool::Freehand, "pencil-simple", "Freehand"),
    (Tool::Rectangle, "rectangle", "Rectangle"), (Tool::Oval, "circle", "Oval"), (Tool::Arrow, "arrow-right", "Arrow"), (Tool::Line, "line-segment", "Line"),
    (Tool::Eraser, "eraser", "Erase stroke (click strokes to remove)")];

#[derive(Clone, Debug, PartialEq)]
pub struct Card { pub key: String, pub frame: i64, pub text: String, pub addressed: bool, pub thumb: Option<PathBuf>, pub viewer: bool, pub ocio: bool }
#[derive(Clone, Debug, PartialEq)]
pub enum Cmd { AddNote, Seek(i64), Text(String, String), Addressed(String, bool), Delete(String), Tool(Option<Tool>), Color(String), Width(f32) }

struct CardRects { thumb: Rect, text: Rect, addressed: Rect, delete: Rect }

pub struct NotesDrawer {
    pub open: bool, open_ramp: Ramp, pub rect: Rect, scale: f64,
    header: Rect, add: Rect, cards_area: Rect, scroll: f64, max_scroll: f64, /// the layer-2 panels behind the columns (the colour drawer's treatment)
    panels: [Rect; 4],
    cards: Vec<Card>, rects: Vec<CardRects>, current: Option<String>, clip: String,
    tools: Vec<ActionButton>, tool: Option<Tool>, picker: ColorPicker, width: Slider, width_v: f32, width_label: Rect,
    edit: Option<(String, TextField)>, thumbs: HashMap<PathBuf, (SystemTime, peniko::ImageBrush)>,
    hover: Option<(usize, u8)>, add_hover: bool, pub tips: Tooltips,
}

impl NotesDrawer {
    pub fn new() -> NotesDrawer {
        NotesDrawer { open: false, open_ramp: Ramp::new(0.0, t::RAIL_COLLAPSE_DURATION), rect: Rect::ZERO, scale: 1.0, header: Rect::ZERO, add: Rect::ZERO, cards_area: Rect::ZERO, scroll: 0.0, max_scroll: 0.0, panels: [Rect::ZERO; 4],
            cards: vec![], rects: vec![], current: None, clip: String::new(),
            tools: TOOLS.iter().map(|(_, icon, tip)| ActionButton::new(icon, tr(tip)).size_s()).collect(), tool: Some(Tool::Freehand), picker: ColorPicker::new("#ff0000"), width: Slider::new(5.0 / 23.0), width_v: 6.0, width_label: Rect::ZERO,
            edit: None, thumbs: HashMap::new(), hover: None, add_hover: false, tips: Tooltips::default() }
    }
    pub fn set_open(&mut self, on: bool, now: Instant) { if on != self.open { self.open = on; self.open_ramp.set_target(if on { 1.0 } else { 0.0 }, now); if !on { self.edit = None; } } }
    pub fn visible_height(&self, now: Instant, scale: f64) -> f64 { (HEIGHT * scale * self.open_ramp.value(now) as f64).round() }
    pub fn animating(&self, now: Instant) -> bool { self.open_ramp.animating(now) || self.tools.iter().any(|b| b.animating(now)) || self.width.animating(now) }
    pub fn tick(&mut self, now: Instant) -> bool { self.tips.tick(now) }
    pub fn has_focus(&self) -> bool { self.edit.is_some() || self.picker.focused() }
    /// The state to show; true when it changed.
    pub fn sync(&mut self, clip: &str, cards: Vec<Card>, current: Option<String>, tool: Option<Tool>, color: &str, width: f32) -> bool {
        let changed = self.cards != cards || self.current != current || self.tool != tool || self.clip != clip || (self.width_v - width).abs() > 1e-3;
        self.cards = cards; self.current = current; self.tool = tool; self.clip = clip.to_string(); self.width_v = width;
        for (i, b) in self.tools.iter_mut().enumerate() { b.selected = Some(TOOLS[i].0) == tool; }
        if !self.width.dragging() { self.width.value = ((width - 1.0) / 23.0).clamp(0.0, 1.0); }
        if !self.picker.dragging() { self.picker.set_hex(color); }
        if let Some((k, _)) = &self.edit { if !self.cards.iter().any(|c| &c.key == k) { self.edit = None; } }
        changed
    }
    pub fn layout(&mut self, fonts: &mut Fonts, x0: f64, y0: f64, x1: f64, scale: f64, now: Instant) {
        self.scale = scale; let s = scale;
        let h = self.visible_height(now, scale); self.rect = Rect::new(x0, y0, x1, y0 + h);
        if h <= 1.0 { return }
        let pad = t::SPACING_200 as f64 * s; let row = t::COMPONENT_HEIGHT_100 as f64 * s;
        let full = Rect::new(x0, y0, x1, y0 + HEIGHT * s);
        self.header = Rect::new(full.x0 + pad, full.y0, full.x1 - pad, full.y0 + row);
        let body = Rect::new(full.x0 + pad, self.header.y1, full.x1 - pad, full.y1 - pad);
        let ins = t::SPACING_100 as f64 * s; let gap_p = t::SPACING_100 as f64 * s;   // inside a panel; between panels
        // right to left: the colour + width panel, the tools panel, then the add tile and the cards panel
        let pick_panel = Rect::new(body.x1 - PICK_W * s - 2.0 * ins, body.y0, body.x1, body.y1);
        let tools_panel = Rect::new(pick_panel.x0 - gap_p - TOOLS_W * s - 2.0 * ins, body.y0, pick_panel.x0 - gap_p, body.y1);
        let add_panel = Rect::new(body.x0, body.y0, body.x0 + ADD_W * s, body.y1);
        let cards_panel = Rect::new(add_panel.x1 + gap_p, body.y0, tools_panel.x0 - gap_p, body.y1);
        self.panels = [add_panel, cards_panel, tools_panel, pick_panel];
        let pc = pick_panel.inset(-ins);
        let ph = self.picker.layout(fonts, pc.x0, pc.x1, pc.y0, s);
        let wy = pc.y0 + ph + t::SPACING_100 as f64 * s; let fh = t::FIELD_HEIGHT_S as f64 * s;
        self.width_label = Rect::new(pc.x0, wy, pc.x0 + 48.0 * s, wy + fh); self.width.rect = Rect::new(pc.x0 + 48.0 * s, wy, pc.x1 - 28.0 * s, wy + fh);
        let tx = tools_panel.x0 + ins; let bh = t::ACTION_BUTTON_HEIGHT_S as f64 * s; let gap = 4.0 * s;
        for (i, b) in self.tools.iter_mut().enumerate() { let y = tools_panel.y0 + ins + i as f64 * (bh + gap); b.rect = Rect::new(tx, y, tx + TOOLS_W * s, y + bh); }
        // the add tile, then the cards
        self.add = add_panel;
        self.cards_area = cards_panel.inset(-ins);
        let cw = CARD_W * s; let cg = t::SPACING_200 as f64 * s;
        self.max_scroll = ((self.cards.len() as f64) * (cw + cg) - cg - self.cards_area.width()).max(0.0);
        self.scroll = self.scroll.clamp(0.0, self.max_scroll);
        self.rects = (0..self.cards.len()).map(|i| {
            let ci = 6.0 * s;   // the thumbnail's inset inside its tile
            let x = self.cards_area.x0 + i as f64 * (cw + cg) - self.scroll + ci; let y = self.cards_area.y0 + ci; let cw = cw - 2.0 * ci;
            let thumb = Rect::new(x, y, x + cw, y + (THUMB_H - 12.0) * s);
            let text = Rect::new(x, thumb.y1 + 22.0 * s, x + cw, thumb.y1 + 22.0 * s + fh);
            let by = text.y1 + 4.0 * s; let ib = 24.0 * s;
            CardRects { thumb, text, addressed: Rect::new(x, by, x + ib, by + ib), delete: Rect::new(x + cw - ib, by, x + cw, by + ib) }
        }).collect();
        if let Some((k, f)) = &mut self.edit { if let Some(i) = self.cards.iter().position(|c| &c.key == k) { f.layout(fonts, self.rects[i].text, s); } }
    }
    /// The current note's card scrolled into view (the playhead moved onto it).
    pub fn reveal_current(&mut self) {
        let Some(k) = &self.current else { return }; let Some(i) = self.cards.iter().position(|c| &c.key == k) else { return };
        let s = self.scale; let x = i as f64 * (CARD_W * s + t::SPACING_200 as f64 * s);
        if x < self.scroll { self.scroll = x; } else if x + CARD_W * s > self.scroll + self.cards_area.width() { self.scroll = x + CARD_W * s - self.cards_area.width(); }
    }
    pub fn wheel(&mut self, x: f64, y: f64, d: f64) -> bool { if !self.cards_area.contains(kurbo::Point::new(x, y)) || self.max_scroll <= 0.0 { return false } self.scroll = (self.scroll + d).clamp(0.0, self.max_scroll); true }
    fn commit_edit(&mut self) -> Option<Cmd> { let (k, f) = self.edit.take()?; Some(Cmd::Text(k, f.text().to_string())) }
    /// Returns (commands, redraw, consumed).
    pub fn event(&mut self, e: &Event, now: Instant, fonts: &mut Fonts) -> (Vec<Cmd>, bool, bool) {
        let mut cmds = vec![]; let mut dirty = false;
        if self.rect.height() <= 1.0 { return (cmds, false, false) }
        let inside = e.pos().is_some_and(|(x, y)| self.rect.contains(kurbo::Point::new(x, y))) && !matches!(e, Event::Leave);
        if let Some((_, f)) = &mut self.edit { if f.pointer(e, fonts) { dirty = true; } }
        // a press outside the text being edited commits it
        if let Event::Down { x, y, .. } = *e { if let Some((_, f)) = &self.edit { if !f.hit(x, y) { if let Some(c) = self.commit_edit() { cmds.push(c); } dirty = true; } } }
        for (i, b) in self.tools.iter_mut().enumerate() { if let Response::Clicked = b.event(e, now) { let tl = TOOLS[i].0; cmds.push(Cmd::Tool(if self.tool == Some(tl) { None } else { Some(tl) })); dirty = true; } }
        let (c, d) = self.picker.event(e, fonts); dirty |= d; if let Some(c) = c { cmds.push(Cmd::Color(c)); }
        let (v, d) = self.width.event(e, now, self.scale); dirty |= d; if let Some(v) = v { cmds.push(Cmd::Width((1.0 + v * 23.0).round())); }
        // the cards and the add tile
        let pt = e.pos().map(|(x, y)| kurbo::Point::new(x, y));
        let over_cards = pt.is_some_and(|p| self.cards_area.contains(p));
        let hover = pt.filter(|_| inside && over_cards).and_then(|p| self.rects.iter().enumerate().find_map(|(i, r)| if r.addressed.contains(p) { Some((i, 1)) } else if r.delete.contains(p) { Some((i, 2)) } else if r.thumb.contains(p) { Some((i, 0)) } else if r.text.contains(p) { Some((i, 3)) } else { None }));
        if hover != self.hover { self.hover = hover; dirty = true; }
        let add_h = inside && pt.is_some_and(|p| self.add.contains(p)); if add_h != self.add_hover { self.add_hover = add_h; dirty = true; }
        if let Event::Down { button: min_ui::Button::Primary, .. } = *e {
            if add_h { cmds.push(Cmd::AddNote); dirty = true; }
            if let Some((i, part)) = hover { let c = &self.cards[i]; match part {
                0 => cmds.push(Cmd::Seek(c.frame)),
                1 => cmds.push(Cmd::Addressed(c.key.clone(), !c.addressed)),
                2 => cmds.push(Cmd::Delete(c.key.clone())),
                _ => { if self.edit.as_ref().map(|x| &x.0) != Some(&c.key) { let mut f = TextField::new(Family::Sans, t::RAIL_LABEL_SIZE); f.placeholder = tr("Note…").to_string(); f.set_text_end(&c.text, fonts); f.focused = true; f.layout(fonts, self.rects[i].text, self.scale); self.edit = Some((c.key.clone(), f)); } }
            } dirty = true; }
        }
        // tooltips: the tools, the card buttons
        let tip = pt.filter(|_| inside).and_then(|p| {
            for (i, b) in self.tools.iter().enumerate() { if b.rect.contains(p) { return Some((100 + i as u64, b.rect, tr(TOOLS[i].2).to_string())) } }
            match hover { Some((i, 1)) => Some((200 + i as u64 * 2, self.rects[i].addressed, tr("Mark addressed").to_string())), Some((i, 2)) => Some((201 + i as u64 * 2, self.rects[i].delete, tr("Delete note").to_string())), _ => None }
        });
        if self.tips.hover(tip.as_ref().map(|(k, r, s)| (*k, *r, s.as_str())), now) { dirty = true; }
        if matches!(e, Event::Down { .. }) && inside { self.tips.pressed(now); }
        (cmds, dirty, inside || self.picker.dragging() || self.width.dragging())
    }
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) -> Vec<Cmd> {
        if self.picker.focused() { return self.picker.key(k, fonts).map(Cmd::Color).into_iter().collect() }
        let Some((_, f)) = &mut self.edit else { return vec![] };
        match f.key(k, fonts) { FieldResponse::Accepted => self.commit_edit().into_iter().collect(), FieldResponse::Cancelled => { self.edit = None; vec![] } _ => vec![] }
    }
    pub fn ime(&mut self, e: &ImeEvent, fonts: &mut Fonts) { if let Some((_, f)) = &mut self.edit { f.ime(e, fonts); } else { self.picker.ime(e, fonts); } }
    pub fn blur(&mut self) -> Vec<Cmd> { let mut v: Vec<Cmd> = self.commit_edit().into_iter().collect(); if let Some(c) = self.picker.blur() { v.push(Cmd::Color(c)); } v }
    fn thumb(&mut self, p: &PathBuf) -> Option<peniko::ImageBrush> {
        let stamp = std::fs::metadata(p).and_then(|m| m.modified()).ok()?;
        if let Some((s, img)) = self.thumbs.get(p) { if *s == stamp { return Some(img.clone()) } }
        let (px, (w, h)) = crate::image_rgba8(p).ok()?;
        let img = paint::image_rgba8(w, h, px); self.thumbs.insert(p.clone(), (stamp, img.clone())); Some(img)
    }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, now: Instant) {
        if self.rect.height() <= 1.0 { return }
        let s = self.scale;
        paint::rect(scene, self.rect, t::RAIL_BACKGROUND_COLOR);
        paint::rect(scene, Rect::new(self.rect.x0, self.rect.y0, self.rect.x1, self.rect.y0 + s.max(1.0)), t::DIVIDER_COLOR);
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &self.rect);
        fn mid<B: parley::Brush>(l: &parley::Layout<B>, r: Rect) -> f64 { r.y0 + (r.height() - l.height() as f64) / 2.0 }
        let h = fonts.detail(min_i18n::trc("Main", "Notes"), t::RAIL_GROUP_HEADER_SIZE * s as f32, paint::color(t::RAIL_GROUP_HEADER_COLOR));
        let mut x = self.header.x0; min_ui::fonts::draw_layout(scene, &h, Affine::translate((x, mid(&h, self.header)))); x += h.width() as f64 + t::SPACING_200 as f64 * s;
        let cl = fonts.layout(&format!("{}  ·  {}", self.clip, self.cards.len()), Family::Sans, t::RAIL_LABEL_SIZE * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_LABEL_COLOR));
        min_ui::fonts::draw_layout(scene, &cl, Affine::translate((x, mid(&cl, self.header))));
        // the layer-2 panels (the colour drawer's treatment), the add tile one of them, rising on hover
        let r = t::CORNER_RADIUS_500 as f64 * s;
        for (i, p) in self.panels.iter().enumerate() { paint::rounded(scene, *p, r, if i == 0 && self.add_hover { t::HOVER_BACKGROUND_COLOR } else { t::BACKGROUND_LAYER_2_COLOR }); }
        if let Some(g) = fonts.icon("plus", 22.0 * s as f32, paint::color(t::RAIL_LABEL_COLOR)) { min_ui::fonts::draw_layout(scene, &g, Affine::translate((self.add.x0 + (self.add.width() - g.width() as f64) / 2.0, self.add.y0 + self.add.height() * 0.32))); }
        for (i, txt) in [tr("Add note here"), tr("captures current frame")].iter().enumerate() {
            let l = fonts.layout_width(txt, Family::Sans, (if i == 0 { t::RAIL_LABEL_SIZE } else { t::DETAIL_SIZE_S }) * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(if i == 0 { t::RAIL_LABEL_COLOR } else { t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT }), Some((self.add.width() - 12.0 * s) as f32));
            min_ui::fonts::draw_layout(scene, &l, Affine::translate((self.add.x0 + (self.add.width() - l.width() as f64) / 2.0, self.add.y0 + self.add.height() * 0.55 + i as f64 * 20.0 * s)));
        }
        // the cards
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &self.cards_area);
        let cards = self.cards.clone();
        for (i, c) in cards.iter().enumerate() {
            let Some(rc) = self.rects.get(i) else { break }; let (thumb, text, addr, del) = (rc.thumb, rc.text, rc.addressed, rc.delete);
            if thumb.x1 < self.cards_area.x0 || thumb.x0 > self.cards_area.x1 { continue }
            let current = self.current.as_deref() == Some(&c.key);
            // the card's tile: one step up from its panel, so each note reads as a unit
            let tile = Rect::new(thumb.x0 - 6.0 * s, thumb.y0 - 6.0 * s, thumb.x1 + 6.0 * s, del.y1 + 6.0 * s);
            paint::rounded(scene, tile, 6.0 * s, min_ui::section::card_color());
            paint::rounded(scene, thumb, 4.0 * s, t::GRAY_25);
            match c.thumb.as_ref().and_then(|p| self.thumb(p)) {
                Some(img) => {
                    let (iw, ih) = (img.image.width as f64, img.image.height as f64); let k = (thumb.width() / iw).min(thumb.height() / ih);
                    let (w, hh) = (iw * k, ih * k); let ir = Rect::new(thumb.x0 + (thumb.width() - w) / 2.0, thumb.y0 + (thumb.height() - hh) / 2.0, thumb.x0 + (thumb.width() + w) / 2.0, thumb.y0 + (thumb.height() + hh) / 2.0);
                    paint::image(scene, &img, ir);
                }
                None => { let l = fonts.layout(tr("(no thumb)"), Family::Sans, t::DETAIL_SIZE_S * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT)); min_ui::fonts::draw_layout(scene, &l, Affine::translate((thumb.x0 + (thumb.width() - l.width() as f64) / 2.0, mid(&l, thumb)))); }
            }
            if current || self.hover.is_some_and(|h| h.0 == i && h.1 == 0) { scene.stroke(&kurbo::Stroke::new(if current { 2.0 * s } else { s }), Affine::IDENTITY, paint::color(if current { t::FOCUS_INDICATOR_COLOR } else { t::GRAY_500 }), None, &kurbo::RoundedRect::from_rect(thumb, 4.0 * s)); }
            // the timecode line + badges
            let tl = fonts.layout(&c.key, Family::Mono, t::CODE_SIZE_S * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(if current { t::NEUTRAL_CONTENT_COLOR_DEFAULT } else { t::RAIL_LABEL_COLOR }));
            let ly = thumb.y1 + 3.0 * s; min_ui::fonts::draw_layout(scene, &tl, Affine::translate((thumb.x0, ly)));
            let mut bx = thumb.x0 + tl.width() as f64 + 8.0 * s;
            for (on, b) in [(c.viewer, tr("VIEWER")), (c.ocio, tr("OCIO"))] { if on { let bl = fonts.detail(b, t::DETAIL_SIZE_S * s as f32, paint::color(t::GRAY_50)); let br = Rect::new(bx, ly + 1.0 * s, bx + bl.width() as f64 + 8.0 * s, ly + 15.0 * s); paint::rounded(scene, br, 3.0 * s, t::GRAY_600); min_ui::fonts::draw_layout(scene, &bl, Affine::translate((bx + 4.0 * s, mid(&bl, br)))); bx = br.x1 + 4.0 * s; } }
            // the text (the field while editing)
            match &mut self.edit { Some((k, f)) if *k == c.key => { paint::rounded(scene, text, 4.0 * s, t::FIELD_BACKGROUND_COLOR); f.paint(scene, fonts, s); }
                _ => {
                    if self.hover.is_some_and(|h| h.0 == i && h.1 == 3) { paint::rounded(scene, text, 4.0 * s, t::HOVER_BACKGROUND_COLOR); }
                    let (txt, col) = if c.text.is_empty() { (tr("Note…").to_string(), t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT) } else { (c.text.lines().next().unwrap_or("").to_string(), t::NEUTRAL_CONTENT_COLOR_DEFAULT) };
                    let l = crate::project_panel::elided(fonts, &txt, t::FONT_WEIGHT_REGULAR as f32, col, text.width() - 12.0 * s, s);
                    min_ui::fonts::draw_layout(scene, &l, Affine::translate((text.x0 + 6.0 * s, mid(&l, text))));
                } }
            // Mark addressed · Delete note
            for (k, (rr, icon, col)) in [(addr, if c.addressed { "check-circle" } else { "circle" }, if c.addressed { t::POSITIVE_VISUAL_COLOR } else { t::RAIL_LABEL_COLOR }), (del, "trash", t::RAIL_LABEL_COLOR)].into_iter().enumerate() {
                if self.hover.is_some_and(|h| h.0 == i && h.1 == k as u8 + 1) { paint::rounded(scene, rr, 4.0 * s, t::HOVER_BACKGROUND_COLOR); }
                if let Some(g) = fonts.icon(icon, 16.0 * s as f32, paint::color(col)) { min_ui::fonts::draw_layout(scene, &g, Affine::translate((rr.x0 + (rr.width() - g.width() as f64) / 2.0, mid(&g, rr)))); }
            }
        }
        scene.pop_layer();
        for b in &self.tools { b.paint(scene, fonts, s, now); }
        self.picker.paint(scene, fonts, s);
        let wl = fonts.layout(tr("Width:"), Family::Sans, t::RAIL_LABEL_SIZE * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_LABEL_COLOR));
        min_ui::fonts::draw_layout(scene, &wl, Affine::translate((self.width_label.x0, mid(&wl, self.width_label))));
        self.width.paint(scene, s, now);
        let wv = fonts.layout(&format!("{}", self.width_v.round() as i32), Family::Mono, t::CODE_SIZE_S * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT));
        min_ui::fonts::draw_layout(scene, &wv, Affine::translate((self.width.rect.x1 + 6.0 * s, mid(&wv, self.width.rect))));
        scene.pop_layer();
    }
    pub fn paint_overlays(&self, scene: &mut Scene, fonts: &mut Fonts, bounds: Rect, now: Instant) { self.tips.paint(scene, fonts, self.scale, bounds, now); }
}
