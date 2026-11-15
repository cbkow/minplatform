//! The EXR layers card (the spec's 3.7): the layers of the file the preview shows, each as a small picture with its
//! name, two to a row. A press on one makes it the preview's layer for that job (until the application ends) and
//! closes the card; a press outside closes it. The list and the pictures are made off the window's thread and come in
//! one by one (0.5.x read the header on the UI thread).
use crate::node::{LayerTile, Node};
use kurbo::{Affine, Rect, RoundedRect};
use min_theme::tokens as t;
use min_ui::{fonts::{draw_layout, Fonts}, paint, Button, Event, Family, ScrollBar};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use vello::Scene;

pub struct Layers { pub job: String, data: Arc<Mutex<(Vec<LayerTile>, bool)>>, tiles: Vec<(String, (u32, u32), Option<peniko::ImageBrush>)>, done: bool, chosen: String, bar: ScrollBar, card: Rect, cells: Vec<(Rect, String)> }
pub enum Out { None, Close, Pick(String) }

impl Layers {
    pub fn open(node: &Arc<Node>, job: &str) -> Layers { let data = Arc::new(Mutex::new((Vec::new(), false))); node.layers_for_window(data.clone());
        let chosen = node.layer(job); Layers { job: job.to_string(), data, tiles: Vec::new(), done: false, chosen: if chosen.is_empty() { "default".into() } else { chosen }, bar: ScrollBar::default(), card: Rect::ZERO, cells: Vec::new() } }
    pub fn wheel(&mut self, dy: f64) { self.bar.wheel(dy, Instant::now()); }
    pub fn pointer(&mut self, e: &Event) -> Out {
        if self.bar.event(e, Instant::now()).1 { return Out::None }
        if let Event::Down { x, y, button: Button::Primary } = *e { let p = kurbo::Point::new(x, y); if !self.card.contains(p) { return Out::Close }
            if let Some((_, name)) = self.cells.iter().find(|(r, _)| r.contains(p)) { return Out::Pick(name.clone()) } }
        Out::None
    }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, view: Rect, s: f64, now: Instant) {
        { let mut d = self.data.lock().unwrap(); for tile in d.0.drain(..) { let brush = (tile.size.0 > 0 && tile.size.1 > 0).then(|| paint::image_rgba8(tile.size.0, tile.size.1, tile.rgba)); self.tiles.push((tile.name, tile.size, brush)); } self.done = d.1; }
        let c = paint::color; let pad = 12.0 * s; let (ink, sub) = (c(t::NEUTRAL_CONTENT_COLOR_DEFAULT), c(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT));
        let need = 12.0 * s * 2.0 + 36.0 * s + (self.tiles.len().div_ceil(2).max(1) as f64) * (168.0 * s + 12.0 * s);   // as tall as its rows, up to 0.5.x's 620
        let (w, h) = ((560.0 * s).min(view.width() - 48.0 * s), (620.0 * s).min(need).min(view.height() - 48.0 * s)); let (x0, y0) = (view.x0 + (view.width() - w) / 2.0, view.y0 + (view.height() - h) / 2.0); self.card = Rect::new(x0, y0, x0 + w, y0 + h);
        paint::rect(scene, view, t::DIALOG_SCRIM_COLOR); let rad = t::DIALOG_CORNER_RADIUS as f64 * s;
        for sh in t::POPOVER_SHADOW { scene.draw_blurred_rounded_rect(Affine::translate((sh.x as f64 * s, sh.y as f64 * s)), self.card.inflate(sh.spread as f64 * s, sh.spread as f64 * s), c(sh.color), rad, (sh.blur as f64 * s / 2.0).max(0.1)); }
        scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, c(t::BACKGROUND_LAYER_1_COLOR), None, &RoundedRect::from_rect(self.card, rad));
        let title = fonts.layout_fit("EXR layers", Family::Sans, t::DIALOG_TITLE_SIZE * s as f32, t::FONT_WEIGHT_BOLD as f32, ink, w - 2.0 * pad); draw_layout(scene, &title, Affine::translate((x0 + pad + 4.0 * s, y0 + pad + 2.0 * s)));
        let list = Rect::new(x0 + pad, y0 + pad + 36.0 * s, x0 + w - pad, y0 + h - pad); let cell_h = 168.0 * s; let cw = (list.width() - 10.0 * s - pad) / 2.0;
        let rows = self.tiles.len().div_ceil(2); self.bar.set(list, rows as f64 * (cell_h + pad), s); self.cells.clear();
        if self.tiles.is_empty() { let l = fonts.layout_fit(if self.done { "No layer could be read from this file." } else { "Reading the layers\u{2026}" }, Family::Sans, t::BODY_SIZE_S * s as f32, t::FONT_WEIGHT_REGULAR as f32, sub, list.width()); draw_layout(scene, &l, Affine::translate((list.x0 + 4.0 * s, list.y0 + 8.0 * s))); }
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &list);
        for (i, (name, size, brush)) in self.tiles.iter().enumerate() { let (col, row) = (i % 2, i / 2); let y = list.y0 + row as f64 * (cell_h + pad) - self.bar.offset; if y + cell_h < list.y0 || y > list.y1 { continue }
            let cell = Rect::new(list.x0 + col as f64 * (cw + pad), y, list.x0 + col as f64 * (cw + pad) + cw, y + cell_h); let tile = Rect::new(cell.x0, cell.y0, cell.x1, cell.y1 - 24.0 * s); self.cells.push((cell, name.clone()));
            paint::rounded(scene, tile, t::CORNER_RADIUS_500 as f64 * s, t::BACKGROUND_BASE_COLOR);
            if let Some(b) = brush { let inner = tile.inset(-4.0 * s); let k = (inner.width() / size.0 as f64).min(inner.height() / size.1 as f64); let (dw, dh) = (size.0 as f64 * k, size.1 as f64 * k); paint::image(scene, b, Rect::new(inner.center().x - dw / 2.0, inner.center().y - dh / 2.0, inner.center().x + dw / 2.0, inner.center().y + dh / 2.0)); }
            let on = *name == self.chosen; scene.stroke(&kurbo::Stroke::new(if on { 2.0 * s } else { s }), Affine::IDENTITY, c(if on { t::ACCENT_VISUAL_COLOR } else { t::DIVIDER_COLOR_MEDIUM }), None, &RoundedRect::from_rect(tile, t::CORNER_RADIUS_500 as f64 * s));
            let l = fonts.layout_fit(name, Family::Sans, t::DETAIL_SIZE_M * s as f32, t::FONT_WEIGHT_REGULAR as f32, if on { ink } else { sub }, cw - 8.0 * s); draw_layout(scene, &l, Affine::translate((cell.center().x - l.width() as f64 / 2.0, cell.y1 - 12.0 * s - l.height() as f64 / 2.0))); }
        scene.pop_layer(); self.bar.paint(scene, now);
    }
    pub fn animating(&self, now: Instant) -> bool { !self.done || self.bar.animating(now) }
}
