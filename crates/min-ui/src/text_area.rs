//! TextArea — the family's multi-line text box on the same parley `PlainEditor` as `TextField` (QCView's note `TextArea`,
//! `NotesPanel.qml`): the text wraps to the box and scrolls inside it (the caret kept in view; the wheel scrolls); QCView's
//! recessed-slot styling — the base fill at rest, lifting on hover, a step up with an accent rule along the bottom while
//! focused, no border. Enter inserts a newline, ⌘Enter accepts, Escape cancels (the owner restores); focus loss is the
//! owner's commit. Unfocused it paints the whole wrapped text, so a long note reads in place.
use crate::{input::{Button, Event, ImeEvent, Key, KeyEvent, Named}, paint, scrollbar::ScrollBar, text_field::FieldResponse, Family, Fonts};
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use parley::{FontFamily, FontFamilyName, PlainEditor, StyleProperty};
use peniko::Brush;
use std::borrow::Cow;
use vello::Scene;

/// (the vertical scroll lives in `bar.offset` — the rails' overlay scrollbar, draggable)
pub struct TextArea { editor: PlainEditor<Brush>, pub placeholder: String, pub rect: Rect, pub focused: bool, hovered: bool, family: Family, size: f32, scale: f32, bar: ScrollBar, styled: bool, drag: bool, clicks: Clicks }

impl TextArea {
    pub fn new(family: Family, size: f32) -> TextArea { TextArea { editor: PlainEditor::new(size), placeholder: String::new(), rect: Rect::ZERO, focused: false, hovered: false, family, size, scale: 1.0, bar: ScrollBar::default(), styled: false, drag: false, clicks: Clicks::default() } }
    pub fn text(&self) -> &str { self.editor.raw_text() }
    pub fn set_text(&mut self, s: &str) { if self.editor.raw_text() != s { self.editor.set_text(s); } }
    pub fn hit(&self, x: f64, y: f64) -> bool { self.rect.contains(kurbo::Point::new(x, y)) }
    fn pad(&self) -> f64 { t::SPACING_100 as f64 * self.scale as f64 }
    fn style(&mut self, fonts: &mut Fonts) {
        let name = fonts.family_name(self.family).to_string();
        let st = self.editor.edit_styles();
        st.insert(StyleProperty::FontFamily(FontFamily::Single(FontFamilyName::Named(Cow::Owned(name)))));
        st.insert(StyleProperty::FontSize(self.size));
        st.insert(StyleProperty::LineHeight(parley::LineHeight::FontSizeRelative(t::UI_LINE_HEIGHT as f32)));
        st.insert(StyleProperty::Brush(Brush::Solid(paint::color(t::FIELD_CONTENT_COLOR))));
        self.styled = true;
    }
    pub fn layout(&mut self, fonts: &mut Fonts, rect: Rect, scale: f64) {
        self.rect = rect; self.scale = scale as f32;
        self.style(fonts);
        self.editor.set_scale(self.scale);
        self.editor.set_width(Some((rect.width() - 2.0 * self.pad()) as f32));
        self.clamp_scroll(fonts);
    }
    fn content_h(&mut self, fonts: &mut Fonts) -> f64 { self.editor.layout(&mut fonts.font_cx, &mut fonts.layout_cx).height() as f64 }
    fn clamp_scroll(&mut self, fonts: &mut Fonts) { let c = self.content_h(fonts) + 2.0 * self.pad(); self.bar.set(self.rect, c, self.scale as f64); }
    /// Keep the caret in view (after an edit or a caret move).
    fn follow_caret(&mut self, fonts: &mut Fonts) {
        let _ = self.content_h(fonts);
        if let Some(cb) = self.editor.cursor_geometry(1.0) { let (top, bot) = (cb.y0 as f64, cb.y1 as f64); let view = self.rect.height() - 2.0 * self.pad();
            if top < self.bar.offset { self.bar.offset = top; } else if bot > self.bar.offset + view { self.bar.offset = bot - view; } }
        self.clamp_scroll(fonts);
    }
    /// A drag-select is in progress (the owner keeps feeding Move / Up wherever the pointer goes).
    pub fn dragging(&self) -> bool { self.drag }
    fn origin(&self) -> (f64, f64) { (self.rect.x0 + self.pad(), self.rect.y0 + self.pad() - self.bar.offset) }
    pub fn animating(&self, now: std::time::Instant) -> bool { self.bar.animating(now) }
    pub fn wheel(&mut self, x: f64, y: f64, dy: f64, fonts: &mut Fonts) -> bool { if !self.hit(x, y) { return false } let _ = fonts; self.bar.wheel(dy, std::time::Instant::now()) }
    /// The text overflows the box (the owner may show a fade / a hint).
    pub fn overflows(&mut self, fonts: &mut Fonts) -> bool { self.content_h(fonts) + 2.0 * self.pad() > self.rect.height() + 0.5 }
    pub fn pointer(&mut self, e: &Event, fonts: &mut Fonts) -> bool {
        // the scrollbar first: a drag on its thumb is never a caret move
        let (bd, mine) = self.bar.event(e, std::time::Instant::now()); if mine { return true }
        let inside = e.pos().is_some_and(|(x, y)| self.hit(x, y));
        let _ = bd;
        let was = (self.hovered, self.focused);
        match *e {
            Event::Move { x, y } => {
                self.hovered = inside;
                if self.drag {
                    let (ox, oy) = self.origin();
                    { let mut d = self.editor.driver(&mut fonts.font_cx, &mut fonts.layout_cx); d.extend_selection_to_point((x - ox) as f32, (y - oy) as f32); }
                    self.follow_caret(fonts);
                    return true;
                }
            }
            Event::Down { x, y, button: Button::Primary } if inside => {
                if !self.focused { self.focused = true; }
                self.drag = true;
                let n = self.clicks.down(x, y);
                let (ox, oy) = self.origin();
                let (px, py) = ((x - ox) as f32, (y - oy) as f32);
                let mut d = self.editor.driver(&mut fonts.font_cx, &mut fonts.layout_cx);
                match n { 1 => d.move_to_point(px, py), 2 => d.select_word_at_point(px, py), _ => d.select_line_at_point(px, py) }
            }
            Event::Up { button: Button::Primary, .. } => { if self.drag { self.drag = false; return true } }
            Event::Leave => self.hovered = false,
            _ => {}
        }
        was != (self.hovered, self.focused) || matches!(e, Event::Down { .. })
    }
    pub fn ime(&mut self, e: &ImeEvent, fonts: &mut Fonts) {
        { let mut d = self.editor.driver(&mut fonts.font_cx, &mut fonts.layout_cx);
          match e { ImeEvent::Preedit(text, cursor) => if text.is_empty() { d.clear_compose() } else { d.set_compose(text, *cursor) }, ImeEvent::Commit(text) => d.insert_or_replace_selection(text) } }
        self.follow_caret(fonts);
    }
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) -> FieldResponse {
        if !self.focused { return FieldResponse::None }
        let before = self.editor.raw_text().to_string();
        {
            let mut d = self.editor.driver(&mut fonts.font_cx, &mut fonts.layout_cx);
            match &k.key {
                Key::Named(Named::Enter) if k.cmd => return FieldResponse::Accepted,
                Key::Named(Named::Enter) => d.insert_or_replace_selection("\n"),
                Key::Named(Named::Escape) => return FieldResponse::Cancelled,
                Key::Named(Named::Left) => if k.shift { d.select_left() } else { d.move_left() },
                Key::Named(Named::Right) => if k.shift { d.select_right() } else { d.move_right() },
                Key::Named(Named::Up) => if k.shift { d.select_up() } else { d.move_up() },
                Key::Named(Named::Down) => if k.shift { d.select_down() } else { d.move_down() },
                Key::Named(Named::Home) => if k.shift { d.select_to_line_start() } else { d.move_to_line_start() },
                Key::Named(Named::End) => if k.shift { d.select_to_line_end() } else { d.move_to_line_end() },
                Key::Named(Named::Backspace) => d.backdelete(),
                Key::Named(Named::Delete) => d.delete(),
                Key::Named(Named::Tab) => {}
                Key::Char(c) if k.cmd => match c.as_str() { "a" => d.select_all(), _ => {} },
                _ => if let Some(txt) = &k.text { let clean: String = txt.chars().filter(|ch| !ch.is_control() || *ch == '\n').collect(); if !clean.is_empty() { d.insert_or_replace_selection(&clean); } },
            }
        }
        self.follow_caret(fonts);
        if self.editor.raw_text() != before { FieldResponse::Changed } else { FieldResponse::None }
    }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, scale: f64) {
        let s = scale; let r = t::FIELD_CORNER_RADIUS_S as f64 * s;
        // QCView's recessed slot: the base fill, the hover lift, a step up + an accent rule while focused
        // active = gray-200: a clear step up from a gray-100 card (the walk 2026-11-08: the focused box blended into its tile)
        let bg = if self.focused { t::GRAY_200 } else if self.hovered { t::GRAY_75 } else { t::FIELD_BACKGROUND_COLOR };
        paint::rounded(scene, self.rect, r, bg);
        if self.focused { paint::rect(scene, Rect::new(self.rect.x0 + r, self.rect.y1 - s.max(1.0), self.rect.x1 - r, self.rect.y1), t::FIELD_BORDER_COLOR_FOCUS); }
        if !self.styled { self.style(fonts); }
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &self.rect.inset(-1.0));
        let (ox, oy) = self.origin(); let tr = Affine::translate((ox, oy));
        if self.editor.raw_text().is_empty() && !self.editor.is_composing() {
            let l = fonts.layout(&self.placeholder, self.family, self.size * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::FIELD_PLACEHOLDER_COLOR));
            crate::fonts::draw_layout(scene, &l, tr);
        }
        if self.focused {
            self.editor.selection_geometry_with(|bb, _| scene.fill(peniko::Fill::NonZero, tr, paint::color(t::FIELD_SELECTION_COLOR), None, &Rect::new(bb.x0 as f64, bb.y0 as f64, bb.x1 as f64, bb.y1 as f64)));
            if let Some(cb) = self.editor.cursor_geometry(1.5 * s as f32) { scene.fill(peniko::Fill::NonZero, tr, paint::color(t::FIELD_CARET_COLOR), None, &Rect::new(cb.x0 as f64, cb.y0 as f64, cb.x1 as f64, cb.y1 as f64)); }
        }
        let layout = self.editor.layout(&mut fonts.font_cx, &mut fonts.layout_cx);
        crate::fonts::draw_layout(scene, layout, tr);
        scene.pop_layer();
        self.bar.paint(scene, std::time::Instant::now());
    }
}

/// Click counting for the text nodes: a press within 400 ms and 4 px of the last one counts up (1 caret, 2 word, 3 all / line).
#[derive(Default, Clone, Copy, Debug)]
pub struct Clicks { last: Option<(std::time::Instant, f64, f64)>, n: u8 }
impl Clicks {
    pub fn down(&mut self, x: f64, y: f64) -> u8 {
        let now = std::time::Instant::now();
        let again = self.last.is_some_and(|(t, lx, ly)| now.duration_since(t).as_millis() < 400 && (x - lx).abs() < 4.0 && (y - ly).abs() < 4.0);
        self.n = if again { (self.n % 3) + 1 } else { 1 };
        self.last = Some((now, x, y));
        self.n
    }
}
