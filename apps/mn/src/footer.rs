//! The page's footer (canon row 299): one row under the page. Left, a readout of where the collection stands (saved,
//! notes changed, saving, merged, a save that failed); right, what acts on the note and the collection as wholes —
//! find in the note, undo, redo, Save. The toolbar above the page holds only what edits at the caret.
use std::time::Instant;
use kurbo::Rect;
use min_theme::tokens as t;
use min_ui::{action_button::{ActionButton, Response}, action_group::ActionGroup, fonts::draw_layout, paint, tooltip::Tooltips, Event, Family, Fonts};
use vello::Scene;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Do { Find, Undo, Redo, Save }
const ORDER: [Do; 4] = [Do::Find, Do::Undo, Do::Redo, Do::Save];

pub struct Shown<'a> { pub text: &'a str, /// the readout is a warning (a save that failed)
    pub warn: bool, pub has_note: bool, pub has_file: bool, pub can_undo: bool, pub can_redo: bool, pub unsaved: bool, pub saving: bool }

pub struct Footer { group: ActionGroup, pub rect: Rect, tooltips: Tooltips, pub hidden: bool, scale: f64, text: String, warn: bool }

impl Footer {
    pub fn new() -> Footer {
        let key = |k: &str| if cfg!(target_os = "macos") { format!("{}\u{2318}{}", if k.starts_with("Shift+") { "\u{21e7}" } else { "" }, k.trim_start_matches("Shift+")) } else { format!("Ctrl+{k}") };
        let b = |icon: &'static str, tip: String, on: bool| ActionButton::new(icon, &tip).enabled(on);
        let group = ActionGroup::new(vec![
            b("magnifying-glass", format!("Find in this note ({}) \u{2014} not yet", key("F")), false),
            b("arrow-counter-clockwise", format!("Undo ({})", key("Z")), true), b("arrow-clockwise", format!("Redo ({})", key("Shift+Z")), true),
            b("floppy-disk", format!("Save ({})", key("S")), true),
        ]).compact();
        Footer { group, rect: Rect::ZERO, tooltips: Tooltips::default(), hidden: false, scale: 1.0, text: String::new(), warn: false }
    }
    /// Places the row from `x0` to `x1` with its bottom at `bottom`; returns its height (0 when hidden).
    pub fn layout(&mut self, fonts: &mut Fonts, x0: f64, x1: f64, bottom: f64, scale: f64) -> f64 {
        self.scale = scale;
        if self.hidden { self.rect = Rect::new(x0, bottom, x1, bottom); for b in &mut self.group.buttons { b.rect = Rect::ZERO; } self.group.rect = Rect::ZERO; return 0.0 }
        let (row, pad) = (t::TRANSPORT_BAR_HEIGHT as f64 * scale, t::SPACING_100 as f64 * scale);
        let w = self.group.layout(fonts, 0.0, 0.0, scale); let bh = self.group.rect.height();
        self.group.layout(fonts, x1 - pad - w, bottom - row + (row - bh) / 2.0, scale);
        self.rect = Rect::new(x0, bottom - row, x1, bottom);
        row
    }
    pub fn sync(&mut self, s: &Shown) {
        self.text = s.text.to_string(); self.warn = s.warn;
        for (b, what) in self.group.buttons.iter_mut().zip(ORDER) {
            let (enabled, selected) = match what { Do::Find => (false, false), Do::Undo => (s.has_note && s.can_undo, false), Do::Redo => (s.has_note && s.can_redo, false),
                // lit while something is not in the document yet
                Do::Save => (s.has_file && !s.saving, s.unsaved) };
            b.enabled = enabled; b.selected = selected && enabled;
        }
    }
    pub fn hit(&self, x: f64, y: f64) -> bool { !self.hidden && self.rect.contains(kurbo::Point::new(x, y)) }
    /// A pointer event: what was pressed, and whether the event was the footer's.
    pub fn event(&mut self, e: &Event, now: Instant) -> (Vec<Do>, bool) {
        if self.hidden { return (vec![], false) }
        let out = self.group.event(e, now).into_iter().filter(|(_, r)| *r == Response::Clicked).map(|(i, _)| ORDER[i]).collect();
        if matches!(e, Event::Down { .. }) { self.tooltips.pressed(now); }
        let over = self.group.buttons.iter().enumerate().find(|(_, b)| b.hovered()).map(|(i, b)| (i as u64, b.rect, b.tooltip.clone()));
        self.tooltips.hover(over.as_ref().map(|(k, r, s)| (*k, *r, s.as_str())), now);
        (out, e.pos().is_some_and(|(x, y)| self.hit(x, y)))
    }
    pub fn tick(&mut self, now: Instant) -> bool { self.tooltips.tick(now) || self.group.animating(now) }
    pub fn next_change(&self, now: Instant) -> Option<Instant> { self.tooltips.next_change(now) }
    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, now: Instant) {
        if self.hidden { return }
        let s = self.scale;
        paint::rect(scene, self.rect, t::BAND_BACKGROUND_COLOR);
        paint::hline(scene, self.rect.x0, self.rect.x1, self.rect.y0, t::DIVIDER_THICKNESS as f64 * s, t::DIVIDER_COLOR);
        let pad = t::SPACING_200 as f64 * s;
        let room = (self.group.rect.x0 - self.rect.x0 - 2.0 * pad).max(10.0);
        let color = if self.warn { t::NEUTRAL_CONTENT_COLOR_HOVER } else { t::RAIL_LABEL_COLOR };
        let l = fonts.layout_fit(&self.text, Family::Sans, t::RAIL_LABEL_SIZE * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(color), room);
        draw_layout(scene, &l, kurbo::Affine::translate((self.rect.x0 + pad, self.rect.y0 + (self.rect.height() - l.height() as f64) / 2.0)));
        self.group.paint(scene, fonts, s, now);
    }
    pub fn paint_tooltip(&self, scene: &mut Scene, fonts: &mut Fonts, bounds: Rect, now: Instant) { self.tooltips.paint(scene, fonts, self.scale, bounds, now); }
}
