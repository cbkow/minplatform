//! Dialog — Spectrum 2's dialog as a node: a scrim over the window, an elevated card (radius 16, 24-px padding) with a
//! heading, a body in the subdued colour and a right-aligned row of pill buttons; the last button is the default. Modal:
//! presses outside do nothing (the owner closes it on Escape via `cancel`). QCView's "Reset all preferences?" lives here.
//! `with_field` adds a labelled text field under the body (Save as Dual View's name, canon row 173): it takes the keys while
//! open, Enter = the default (last) button, Escape = cancel, and the default button is off while the FIRST field is empty.
//! More `with_field` / `with_secret_field` calls stack more fields (QCView's Open Stream: URL, name, passphrase); Tab moves
//! between them.
use crate::{input::{Event, ImeEvent, KeyEvent}, paint, text_button::{TextButton, Variant}, text_field::{FieldResponse, TextField}, Family, Fonts};
use kurbo::{Affine, Rect, RoundedRect};
use min_theme::tokens as t;
use std::time::Instant;
use vello::Scene;

pub struct Dialog { pub title: String, pub body: String, pub buttons: Vec<TextButton>, pub open: bool, pub rect: Rect, /// (label, field), top to bottom; `focus` = the one taking keys
    pub fields: Vec<(String, TextField)>, focus: usize, /// a tick box above the buttons ("Apply to all"): its label, whether it is ticked
    pub check: Option<(String, bool)>, check_rect: Rect, check_hover: bool }

impl Dialog {
    pub fn new(title: &str, body: &str, buttons: Vec<(&str, Variant)>) -> Dialog { Dialog { title: title.to_string(), body: body.to_string(), buttons: buttons.into_iter().map(|(l, v)| TextButton::new(l, v)).collect(), open: false, rect: Rect::ZERO, fields: vec![], focus: 0, check: None, check_rect: Rect::ZERO, check_hover: false } }
    /// A tick box above the buttons (a file browser's "Apply to all").
    pub fn with_check(mut self, label: &str) -> Dialog { self.check = Some((label.to_string(), false)); self }
    pub fn checked(&self) -> bool { self.check.as_ref().is_some_and(|c| c.1) }
    pub fn with_field(self, label: &str, placeholder: &str) -> Dialog { self.push_field(label, placeholder, false) }
    /// A field whose text is never shown (a passphrase: QCView's password echo).
    pub fn with_secret_field(self, label: &str, placeholder: &str) -> Dialog { self.push_field(label, placeholder, true) }
    fn push_field(mut self, label: &str, placeholder: &str, secret: bool) -> Dialog { let mut f = TextField::new(Family::Sans, t::DIALOG_BODY_SIZE); if secret { f = f.secret(); } f.placeholder = placeholder.to_string(); self.fields.push((label.to_string(), f)); self }
    /// Open with the field holding `text`, all of it selected (QCView's save field: the auto-name, selected, focused).
    pub fn show_with(&mut self, text: &str, fonts: &mut Fonts) {
        self.open = true; self.focus = 0;
        for (i, (_, f)) in self.fields.iter_mut().enumerate() { if i == 0 { f.set_text_end(text, fonts); f.select_all(fonts); f.focused = true; } else { f.clear(); f.focused = false; } }
        self.sync_default();
    }
    /// The field's text, trimmed.
    pub fn value(&self) -> String { self.value_at(0) }
    /// Field `i`'s text, trimmed.
    pub fn value_at(&self, i: usize) -> String { self.fields.get(i).map(|(_, f)| f.text().trim().to_string()).unwrap_or_default() }
    fn sync_default(&mut self) { if !self.fields.is_empty() { let on = !self.value().is_empty(); if let Some(b) = self.buttons.last_mut() { b.enabled = on; } } }
    /// A key while open (the field's): Some(button index) when it closes the dialog — Enter = the last button, Escape = the first.
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) -> Option<usize> {
        if self.fields.is_empty() { return None }
        if k.key == crate::input::Key::Named(crate::input::Named::Tab) && self.fields.len() > 1 {
            let n = self.fields.len(); self.fields[self.focus].1.focused = false;
            self.focus = if k.shift { (self.focus + n - 1) % n } else { (self.focus + 1) % n };
            let f = &mut self.fields[self.focus].1; f.focused = true; f.select_all(fonts); return None
        }
        let f = &mut self.fields[self.focus].1;
        match f.key(k, fonts) {
            FieldResponse::Accepted => { if self.value().is_empty() { return None } self.open = false; Some(self.buttons.len().saturating_sub(1)) }
            FieldResponse::Cancelled => { self.open = false; Some(0) }
            FieldResponse::Changed => { self.sync_default(); None }
            FieldResponse::None => None,
        }
    }
    /// The field's pointer handling (focus, caret placement); call before `event` while open.
    pub fn pointer(&mut self, e: &Event, fonts: &mut Fonts) -> bool {
        if !self.open { return false }
        let mut d = false;
        for (i, (_, f)) in self.fields.iter_mut().enumerate() { d |= f.pointer(e, fonts); if f.focused && i != self.focus { self.focus = i; } }
        for (i, (_, f)) in self.fields.iter_mut().enumerate() { if i != self.focus { f.focused = false; } }
        d
    }
    /// The clipboard's text into the field taking keys; the chosen text out of it (`cut` removes it). The default
    /// button follows the field as it does for typed text.
    pub fn paste(&mut self, text: &str, fonts: &mut Fonts) { if let Some((_, f)) = self.fields.get_mut(self.focus) { f.paste(text, fonts); self.sync_default(); } }
    pub fn copy(&mut self, cut: bool, fonts: &mut Fonts) -> Option<String> { let r = self.fields.get_mut(self.focus).and_then(|(_, f)| f.copy(cut, fonts)); if cut { self.sync_default(); } r }
    pub fn ime(&mut self, e: &ImeEvent, fonts: &mut Fonts) { if let Some((_, f)) = self.fields.get_mut(self.focus) { f.ime(e, fonts); self.sync_default(); } }
    pub fn caret_rect(&self, fonts: &mut Fonts) -> Option<Rect> { self.fields.get(self.focus).filter(|_| self.open).and_then(|(_, f)| f.caret_rect(fonts)) }
    pub fn show(&mut self) { self.open = true; }
    pub fn cancel(&mut self) { self.open = false; }
    /// Centre in `bounds`; the body wraps to the dialog's width.
    pub fn layout(&mut self, fonts: &mut Fonts, bounds: Rect, scale: f64) {
        let pad = t::DIALOG_PADDING as f64 * scale; let gap = t::SPACING_200 as f64 * scale;
        // the width is the dialog's own - or what its row of buttons needs (a file browser's question has four)
        let row: f64 = self.buttons.iter().map(|b| b.measure(fonts, scale).0).sum::<f64>() + self.buttons.len().saturating_sub(1) as f64 * t::DIALOG_BUTTON_GAP as f64 * scale;
        let w = (t::DIALOG_WIDTH_S as f64 * scale).max(row + 2.0 * pad);
        // the title wraps to the dialog's width like the body (a document's name in it can be any length - minNotes, 2026-11-11)
        let title = fonts.layout_width(&self.title, Family::Sans, t::DIALOG_TITLE_SIZE * scale as f32, t::FONT_WEIGHT_BOLD as f32, peniko::Color::WHITE, Some((w - 2.0 * pad) as f32));
        let body = fonts.layout_width(&self.body, Family::Sans, t::DIALOG_BODY_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, peniko::Color::WHITE, Some((w - 2.0 * pad) as f32));
        let bh = t::BUTTON_HEIGHT_M as f64 * scale;   // the dialog's buttons stay size M
        let has_body = !self.body.is_empty();
        let fh = t::FIELD_HEIGHT_M as f64 * scale;
        let label_h = fonts.layout("Ag", Family::Sans, t::DIALOG_BODY_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, peniko::Color::WHITE).height() as f64 + t::SPACING_75 as f64 * scale;
        let check_h = if self.check.is_some() { crate::checkbox::size(scale) + gap } else { 0.0 };
        let field_block = self.fields.len() as f64 * (label_h + fh + gap) + check_h;
        let h = pad + title.height() as f64 + gap + if has_body { body.height() as f64 + gap } else { 0.0 } + field_block + gap * 0.5 + bh + pad;
        let x0 = bounds.x0 + (bounds.width() - w) / 2.0; let y0 = bounds.y0 + (bounds.height() - h) / 2.0;
        self.rect = Rect::new(x0, y0, x0 + w, y0 + h);
        let top = y0 + pad + title.height() as f64 + gap + if has_body { body.height() as f64 + gap } else { 0.0 };
        for (i, (_, f)) in self.fields.iter_mut().enumerate() { let fy = top + i as f64 * (label_h + fh + gap) + label_h; f.layout(fonts, Rect::new(x0 + pad, fy, x0 + w - pad, fy + fh), scale); }
        let mut x = self.rect.x1 - pad; let by = self.rect.y1 - pad - bh; let bgap = t::DIALOG_BUTTON_GAP as f64 * scale;
        if let Some((label, _)) = &self.check { let side = crate::checkbox::size(scale); let cy = by - gap * 0.5 - check_h + (check_h - side) / 2.0;
            let lw = fonts.layout(label, Family::Sans, t::DIALOG_BODY_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, peniko::Color::WHITE).width() as f64;
            self.check_rect = Rect::new(x0 + pad, cy, x0 + pad + side + t::SPACING_100 as f64 * scale + lw, cy + side); }
        for b in self.buttons.iter_mut().rev() { let (bw, _) = b.measure(fonts, scale); b.rect = Rect::new(x - bw, by, x, by + bh); x -= bw + bgap; }
    }
    /// Returns (the index of the button clicked, redraw needed). Every event is consumed while open.
    pub fn event(&mut self, e: &Event, now: Instant) -> (Option<usize>, bool) {
        if !self.open { return (None, false) }
        let mut clicked = None; let mut dirty = false;
        for (i, b) in self.buttons.iter_mut().enumerate() { let (c, d) = b.event(e, now); if c { clicked = Some(i); } dirty |= d; }
        if let Some((_, on)) = self.check.as_mut() { let over = e.pos().is_some_and(|(x, y)| self.check_rect.contains(kurbo::Point::new(x, y)));
            if over != self.check_hover { self.check_hover = over; dirty = true; }
            if over && matches!(e, Event::Down { button: crate::input::Button::Primary, .. }) { *on = !*on; dirty = true; } }
        if clicked.is_some() { self.open = false; dirty = true; }
        (clicked, dirty)
    }
    pub fn animating(&self, now: Instant) -> bool { self.open && self.buttons.iter().any(|b| b.animating(now)) }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, bounds: Rect, now: Instant) {
        if !self.open { return }
        paint::rect(scene, bounds, t::DIALOG_SCRIM_COLOR);
        let r = t::DIALOG_CORNER_RADIUS as f64 * scale;
        for sh in t::POPOVER_SHADOW { scene.draw_blurred_rounded_rect(Affine::translate((sh.x as f64 * scale, sh.y as f64 * scale)), self.rect.inflate(sh.spread as f64 * scale, sh.spread as f64 * scale), paint::color(sh.color), r, (sh.blur as f64 * scale / 2.0).max(0.1)); }
        scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, paint::color(t::DIALOG_BACKGROUND_COLOR), None, &RoundedRect::from_rect(self.rect, r));
        let pad = t::DIALOG_PADDING as f64 * scale; let gap = t::SPACING_200 as f64 * scale;
        let title = fonts.layout_width(&self.title, Family::Sans, t::DIALOG_TITLE_SIZE * scale as f32, t::FONT_WEIGHT_BOLD as f32, paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT), Some((self.rect.width() - 2.0 * pad) as f32));
        crate::fonts::draw_layout(scene, &title, Affine::translate((self.rect.x0 + pad, self.rect.y0 + pad)));
        let body = fonts.layout_width(&self.body, Family::Sans, t::DIALOG_BODY_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::DIALOG_BODY_COLOR), Some((self.rect.width() - 2.0 * pad) as f32));
        crate::fonts::draw_layout(scene, &body, Affine::translate((self.rect.x0 + pad, self.rect.y0 + pad + title.height() as f64 + gap)));
        if let Some((label, on)) = &self.check { let side = crate::checkbox::size(scale); let r = self.check_rect;
            crate::checkbox::paint_box(scene, fonts, Rect::new(r.x0, r.y0, r.x0 + side, r.y0 + side), Some(*on), true, self.check_hover, scale);
            let l = fonts.layout(label, Family::Sans, t::DIALOG_BODY_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT));
            crate::fonts::draw_layout(scene, &l, Affine::translate((r.x0 + side + t::SPACING_100 as f64 * scale, r.y0 + (side - l.height() as f64) / 2.0))); }
        for (l, f) in self.fields.iter_mut() {
            let label = fonts.layout(l, Family::Sans, t::DIALOG_BODY_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::DIALOG_BODY_COLOR));
            let ly = f.rect.y0 - t::SPACING_75 as f64 * scale - label.height() as f64;
            crate::fonts::draw_layout(scene, &label, Affine::translate((self.rect.x0 + pad, ly)));
            f.paint(scene, fonts, scale);
        }
        for b in &self.buttons { b.paint(scene, fonts, scale, now); }
    }
}
