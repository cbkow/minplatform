//! The page's footer (canon row 299): one row under the page. Left, a readout of where the collection stands (saved,
//! notes changed, saving, merged, a save that failed); right, what acts on the note and the collection as wholes —
//! find in the note, undo, redo, Save. The toolbar above the page holds only what edits at the caret.
use std::time::Instant;
use kurbo::Rect;
use min_theme::tokens as t;
use min_ui::{action_button::{ActionButton, Response}, action_group::ActionGroup, fonts::draw_layout, paint, tooltip::Tooltips, Event, Family, Fonts};
use vello::Scene;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Do { Find, Undo, Redo, Save, Sidebar, /// the readout was pressed while it stands for something that can be shown
    Readout }
const ORDER: [Do; 4] = [Do::Find, Do::Undo, Do::Redo, Do::Save];

pub struct Shown<'a> { pub text: &'a str, /// the readout is a warning (a save that failed)
    pub warn: bool, /// the readout can be pressed (a copy under way whose window is put away)
    pub link: bool, /// the find bar is open
    pub finding: bool, pub has_note: bool, pub has_file: bool, pub sidebar_open: bool, pub can_undo: bool, pub can_redo: bool, pub unsaved: bool, pub saving: bool }

pub struct Footer { /// the sidebar's button, alone at the left end: there whether the sidebar is open or folded (row 300)
    side: ActionGroup, group: ActionGroup, pub rect: Rect, tooltips: Tooltips, pub hidden: bool, scale: f64, text: String, warn: bool, link: bool }

impl Footer {
    pub fn new() -> Footer {
        let key = |k: &str| if cfg!(target_os = "macos") { format!("{}\u{2318}{}", if k.starts_with("Shift+") { "\u{21e7}" } else { "" }, k.trim_start_matches("Shift+")) } else { format!("Ctrl+{k}") };
        let b = |icon: &'static str, tip: String, on: bool| ActionButton::new(icon, &tip).enabled(on);
        let group = ActionGroup::new(vec![
            b("magnifying-glass", format!("Find in this note ({})", key("F")), true),
            b("arrow-counter-clockwise", format!("Undo ({})", key("Z")), true), b("arrow-clockwise", format!("Redo ({})", key("Shift+Z")), true),
            b("floppy-disk", format!("Save ({})", key("S")), true),
        ]).compact();
        let side = ActionGroup::new(vec![ActionButton::new("sidebar-simple", &format!("Sidebar ({})", key("1")))]).compact();
        Footer { side, group, rect: Rect::ZERO, tooltips: Tooltips::default(), hidden: false, scale: 1.0, text: String::new(), warn: false, link: false }
    }
    /// Places the row from `x0` to `x1` with its bottom at `bottom`; returns its height (0 when hidden).
    pub fn layout(&mut self, fonts: &mut Fonts, x0: f64, x1: f64, bottom: f64, scale: f64) -> f64 {
        self.scale = scale;
        if self.hidden { self.rect = Rect::new(x0, bottom, x1, bottom); for b in self.group.buttons.iter_mut().chain(self.side.buttons.iter_mut()) { b.rect = Rect::ZERO; } self.group.rect = Rect::ZERO; self.side.rect = Rect::ZERO; return 0.0 }
        let (row, pad) = (t::TRANSPORT_BAR_HEIGHT as f64 * scale, t::SPACING_100 as f64 * scale);
        let w = self.group.layout(fonts, 0.0, 0.0, scale); let bh = self.group.rect.height();
        self.group.layout(fonts, x1 - pad - w, bottom - row + (row - bh) / 2.0, scale);
        self.side.layout(fonts, x0 + pad, bottom - row + (row - bh) / 2.0, scale);
        self.rect = Rect::new(x0, bottom - row, x1, bottom);
        row
    }
    pub fn sync(&mut self, s: &Shown) {
        self.text = s.text.to_string(); self.warn = s.warn; self.link = s.link;
        if let Some(b) = self.side.buttons.first_mut() { b.enabled = s.has_file; b.selected = s.has_file && s.sidebar_open; }
        for (b, what) in self.group.buttons.iter_mut().zip(ORDER) {
            let (enabled, selected) = match what { Do::Sidebar | Do::Readout => (false, false), Do::Find => (s.has_note, s.finding), Do::Undo => (s.has_note && s.can_undo, false), Do::Redo => (s.has_note && s.can_redo, false),
                // lit while something is not in the document yet
                Do::Save => (s.has_file && !s.saving, s.unsaved) };
            b.enabled = enabled; b.selected = selected && enabled;
        }
    }
    pub fn hit(&self, x: f64, y: f64) -> bool { !self.hidden && self.rect.contains(kurbo::Point::new(x, y)) }
    /// A pointer event: what was pressed, and whether the event was the footer's.
    pub fn event(&mut self, e: &Event, now: Instant) -> (Vec<Do>, bool) {
        if self.hidden { return (vec![], false) }
        let mut out: Vec<Do> = self.group.event(e, now).into_iter().filter(|(_, r)| *r == Response::Clicked).map(|(i, _)| ORDER[i]).collect();
        if self.side.event(e, now).into_iter().any(|(_, r)| r == Response::Clicked) { out.push(Do::Sidebar); }
        if let (true, Event::Down { x, y, .. }) = (self.link, e) { if Rect::new(self.side.rect.x1, self.rect.y0, self.group.rect.x0, self.rect.y1).contains(kurbo::Point::new(*x, *y)) { out.push(Do::Readout); } }
        if matches!(e, Event::Down { .. }) { self.tooltips.pressed(now); }
        let over = self.group.buttons.iter().chain(self.side.buttons.iter()).enumerate().find(|(_, b)| b.hovered()).map(|(i, b)| (i as u64, b.rect, b.tooltip.clone()));
        self.tooltips.hover(over.as_ref().map(|(k, r, s)| (*k, *r, s.as_str())), now);
        (out, e.pos().is_some_and(|(x, y)| self.hit(x, y)))
    }
    pub fn tick(&mut self, now: Instant) -> bool { self.tooltips.tick(now) || self.group.animating(now) || self.side.animating(now) }
    pub fn next_change(&self, now: Instant) -> Option<Instant> { self.tooltips.next_change(now) }
    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, now: Instant) {
        if self.hidden { return }
        let s = self.scale;
        paint::rect(scene, self.rect, t::BAND_BACKGROUND_COLOR);
        paint::hline(scene, self.rect.x0, self.rect.x1, self.rect.y0, t::DIVIDER_THICKNESS as f64 * s, t::DIVIDER_COLOR);
        let pad = t::SPACING_200 as f64 * s;
        let x = self.side.rect.x1 + pad; let room = (self.group.rect.x0 - x - pad).max(10.0);
        let color = if self.warn || self.link { t::NEUTRAL_CONTENT_COLOR_HOVER } else { t::RAIL_LABEL_COLOR };
        let l = fonts.layout_fit(&self.text, Family::Sans, t::RAIL_LABEL_SIZE * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(color), room);
        draw_layout(scene, &l, kurbo::Affine::translate((x, self.rect.y0 + (self.rect.height() - l.height() as f64) / 2.0)));
        self.group.paint(scene, fonts, s, now); self.side.paint(scene, fonts, s, now);
    }
    pub fn paint_tooltip(&self, scene: &mut Scene, fonts: &mut Fonts, bounds: Rect, now: Instant) { self.tooltips.paint(scene, fonts, self.scale, bounds, now); }
}
