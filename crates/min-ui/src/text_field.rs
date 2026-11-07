//! TextField — the family's single-line field on S3's editor (parley `PlainEditor`, canon row 79 / S3's GO): a recessed
//! well (`field-background` base) with a 1-px border that steps up on hover and turns accent on focus, negative on
//! error; the text vertically centred in whatever height the caller gives (QCView's `FlatTextField` rule); Enter
//! accepts, Escape cancels, newlines never enter. Selection and caret geometry come from the editor; IME composition is
//! routed through (`ime`) so dead keys and press-and-hold compose when the window layer delivers them.
use crate::{input::{Button, Event, ImeEvent, Key, KeyEvent, Named}, paint, Family, Fonts};
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use parley::{FontFamily, FontFamilyName, PlainEditor, StyleProperty};
use peniko::Brush;
use std::borrow::Cow;
use vello::Scene;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldResponse { None, Changed, Accepted, Cancelled }

pub struct TextField {
    editor: PlainEditor<Brush>,
    pub placeholder: String,
    pub rect: Rect,
    pub focused: bool,
    pub error: bool,
    /// Paint the text alone: the owner draws the well (a NumberField's field sits inside one box with its steppers).
    pub bare: bool,
    hovered: bool,
    family: Family,
    size: f32,
    scale: f32,
    styled: bool,
}

impl TextField {
    /// `size` in logical px (the editor scales it by the window's factor at layout).
    pub fn new(family: Family, size: f32) -> TextField {
        TextField { editor: PlainEditor::new(size), placeholder: String::new(), rect: Rect::ZERO, focused: false, error: false, bare: false, hovered: false, family, size, scale: 1.0, styled: false }
    }
    pub fn text(&self) -> &str { self.editor.raw_text() }
    pub fn set_text(&mut self, s: &str) { self.editor.set_text(s); }
    /// Set the text with the caret at its end (a staged or restored entry).
    pub fn set_text_end(&mut self, s: &str, fonts: &mut Fonts) { self.editor.set_text(s); let mut d = self.editor.driver(&mut fonts.font_cx, &mut fonts.layout_cx); d.move_to_text_end(); }
    pub fn clear(&mut self) { self.editor.set_text(""); }
    pub fn select_all(&mut self, fonts: &mut Fonts) { let mut d = self.editor.driver(&mut fonts.font_cx, &mut fonts.layout_cx); d.select_all(); }
    pub fn hovered(&self) -> bool { self.hovered }
    pub fn hit(&self, x: f64, y: f64) -> bool { self.rect.contains(kurbo::Point::new(x, y)) }
    fn style(&mut self, fonts: &mut Fonts, color: peniko::Color) {
        let name = fonts.family_name(self.family).to_string();
        let st = self.editor.edit_styles();
        st.insert(StyleProperty::FontFamily(FontFamily::Single(FontFamilyName::Named(Cow::Owned(name)))));
        st.insert(StyleProperty::FontSize(self.size));
        st.insert(StyleProperty::LineHeight(parley::LineHeight::FontSizeRelative(t::UI_LINE_HEIGHT as f32)));
        st.insert(StyleProperty::Brush(Brush::Solid(color)));
        self.styled = true;
    }
    /// Place the field; `scale` is the window's factor.
    pub fn layout(&mut self, fonts: &mut Fonts, rect: Rect, scale: f64) {
        self.rect = rect; self.scale = scale as f32;
        let color = paint::color(if self.error { t::FIELD_ERROR_CONTENT_COLOR } else { t::FIELD_CONTENT_COLOR });
        self.style(fonts, color);
        self.editor.set_scale(self.scale);
        self.editor.set_width(Some((rect.width() - 2.0 * t::FIELD_EDGE_TO_TEXT as f64 * scale) as f32));
    }
    /// The caret's rect in window px (for the IME candidate window).
    pub fn caret_rect(&self, fonts: &mut Fonts) -> Option<Rect> {
        let _ = self.editor.try_layout()?;
        let _ = fonts;
        let cb = self.editor.cursor_geometry(1.0)?;
        let (ox, oy) = self.text_origin();
        Some(Rect::new(ox + cb.x0 as f64, oy + cb.y0 as f64, ox + cb.x1 as f64, oy + cb.y1 as f64))
    }
    fn text_origin(&self) -> (f64, f64) {
        let lh = self.editor.try_layout().map(|l| l.height() as f64).unwrap_or(self.size as f64 * self.scale as f64 * t::UI_LINE_HEIGHT);
        (self.rect.x0 + t::FIELD_EDGE_TO_TEXT as f64 * self.scale as f64, self.rect.y0 + (self.rect.height() - lh) / 2.0)
    }
    pub fn pointer(&mut self, e: &Event, fonts: &mut Fonts) -> bool {
        let inside = e.pos().is_some_and(|(x, y)| self.hit(x, y));
        let was = (self.hovered, self.focused);
        match *e {
            Event::Move { .. } => self.hovered = inside,
            Event::Down { x, y, button: Button::Primary } if inside => {
                self.focused = true;
                let (ox, oy) = self.text_origin();
                let mut d = self.editor.driver(&mut fonts.font_cx, &mut fonts.layout_cx);
                d.move_to_point((x - ox) as f32, (y - oy) as f32);
            }
            Event::Leave => self.hovered = false,
            _ => {}
        }
        was != (self.hovered, self.focused) || matches!(e, Event::Down { .. })
    }
    pub fn ime(&mut self, e: &ImeEvent, fonts: &mut Fonts) {
        let mut d = self.editor.driver(&mut fonts.font_cx, &mut fonts.layout_cx);
        match e { ImeEvent::Preedit(text, cursor) => if text.is_empty() { d.clear_compose() } else { d.set_compose(text, *cursor) }, ImeEvent::Commit(text) => d.insert_or_replace_selection(&text.replace(['\n', '\r'], "")) }
    }
    /// S3's key table for one line: arrows / Home / End with Shift selection, Backspace / Delete, ⌘A, typed text; Enter and
    /// Escape are reported, not consumed by the text.
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) -> FieldResponse {
        if !self.focused { return FieldResponse::None }
        let before = self.editor.raw_text().to_string();
        let mut d = self.editor.driver(&mut fonts.font_cx, &mut fonts.layout_cx);
        match &k.key {
            Key::Named(Named::Enter) => return FieldResponse::Accepted,
            Key::Named(Named::Escape) => return FieldResponse::Cancelled,
            Key::Named(Named::Left) => if k.shift { d.select_left() } else { d.move_left() },
            Key::Named(Named::Right) => if k.shift { d.select_right() } else { d.move_right() },
            Key::Named(Named::Home) | Key::Named(Named::Up) => if k.shift { d.select_to_text_start() } else { d.move_to_text_start() },
            Key::Named(Named::End) | Key::Named(Named::Down) => if k.shift { d.select_to_text_end() } else { d.move_to_text_end() },
            Key::Named(Named::Backspace) => d.backdelete(),
            Key::Named(Named::Delete) => d.delete(),
            Key::Named(Named::Tab) => {}
            Key::Char(c) if k.cmd => match c.as_str() { "a" => d.select_all(), _ => {} },
            _ => if let Some(txt) = &k.text { let clean: String = txt.chars().filter(|ch| !ch.is_control()).collect(); if !clean.is_empty() { d.insert_or_replace_selection(&clean); } },
        }
        if self.editor.raw_text() != before { FieldResponse::Changed } else { FieldResponse::None }
    }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, scale: f64) {
        let s = scale;
        let border = if self.error { t::FIELD_BORDER_COLOR_ERROR } else if self.focused { t::FIELD_BORDER_COLOR_FOCUS } else if self.hovered { t::FIELD_BORDER_COLOR_HOVER } else { t::FIELD_BORDER_COLOR };
        if !self.bare {
            let r = t::FIELD_CORNER_RADIUS_S as f64 * s;
            paint::rounded(scene, self.rect, r, border);
            let bw = t::BORDER_WIDTH_100 as f64 * s;
            paint::rounded(scene, self.rect.inset(-bw), (r - bw).max(0.0), t::FIELD_BACKGROUND_COLOR);
        }
        let _ = border;
        if !self.styled { let c = paint::color(t::FIELD_CONTENT_COLOR); self.style(fonts, c); }
        let (ox, oy) = self.text_origin();
        let tr = Affine::translate((ox, oy));
        if self.editor.raw_text().is_empty() && !self.editor.is_composing() {
            let l = fonts.layout(&self.placeholder, self.family, self.size * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::FIELD_PLACEHOLDER_COLOR));
            crate::fonts::draw_layout(scene, &l, Affine::translate((ox, self.rect.y0 + (self.rect.height() - l.height() as f64) / 2.0)));
        }
        if self.focused {
            self.editor.selection_geometry_with(|bb, _| scene.fill(peniko::Fill::NonZero, tr, paint::color(t::FIELD_SELECTION_COLOR), None, &Rect::new(bb.x0 as f64, bb.y0 as f64, bb.x1 as f64, bb.y1 as f64)));
            if let Some(cb) = self.editor.cursor_geometry(1.5 * s as f32) { scene.fill(peniko::Fill::NonZero, tr, paint::color(t::FIELD_CARET_COLOR), None, &Rect::new(cb.x0 as f64, cb.y0 as f64, cb.x1 as f64, cb.y1 as f64)); }
        }
        let layout = self.editor.layout(&mut fonts.font_cx, &mut fonts.layout_cx);
        crate::fonts::draw_layout(scene, layout, tr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(k: Key, text: Option<&str>) -> KeyEvent { KeyEvent { key: k, text: text.map(str::to_string), shift: false, cmd: false } }
    #[test]
    fn typing_editing_and_the_two_reports() {
        let mut fonts = Fonts::new();
        let mut f = TextField::new(Family::Mono, 12.0);
        f.layout(&mut fonts, Rect::new(0.0, 0.0, 200.0, 24.0), 1.0);
        assert_eq!(f.key(&key(Key::Char("1".into()), Some("1")), &mut fonts), FieldResponse::None, "unfocused: ignored");
        f.focused = true;
        for ch in ["1", "0", "0", "0"] { assert_eq!(f.key(&key(Key::Char(ch.into()), Some(ch)), &mut fonts), FieldResponse::Changed); }
        assert_eq!(f.text(), "1000");
        assert_eq!(f.key(&key(Key::Named(Named::Backspace), None), &mut fonts), FieldResponse::Changed);
        assert_eq!(f.text(), "100");
        assert_eq!(f.key(&key(Key::Char("\n".into()), Some("\n")), &mut fonts), FieldResponse::None, "a newline never enters");
        assert_eq!(f.key(&key(Key::Named(Named::Enter), None), &mut fonts), FieldResponse::Accepted);
        assert_eq!(f.key(&key(Key::Named(Named::Escape), None), &mut fonts), FieldResponse::Cancelled);
        assert_eq!(f.text(), "100");
        f.ime(&ImeEvent::Commit("é\n".into()), &mut fonts);
        assert_eq!(f.text(), "100é");
        assert!(f.caret_rect(&mut fonts).is_some());
    }
}
