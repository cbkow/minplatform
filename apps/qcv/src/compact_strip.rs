//! Compact mode's one band (canon row 172, RULED on the build 2026-11-07; QCView's `CompactStrip.qml` carried): play /
//! pause and the timecode at the left — the timecode doubles as the window's move handle, since the title bar is gone —
//! the scrubber in its line state through the middle, the pin (Always on Top) and the exit at the right. 32 px tall
//! (QCView's is 22: the family's band height is the written deviation; fullscreen stays on F, as QCView's note says).
use kurbo::Rect;
use min_theme::tokens as t;
use min_ui::{paint, ActionButton, Event, Family, Fonts, Response, Tooltips};
use qcv_player::Action;
use std::time::Instant;
use vello::Scene;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd { Act(Action), Pin, Exit, DragWindow }

pub struct CompactStrip {
    play: ActionButton, pin: ActionButton, exit: ActionButton,
    tc: String, tc_rect: Rect, tc_hover: bool, live: bool,
    pub tooltips: Tooltips,
    pub rect: Rect,
    /// where the scrubber goes (between the left and right controls)
    pub scrub_rect: Rect,
}

/// The live red (the LiveStrip's and the bin's dot, #e5484d): compact LIVE uses it too (the user 2026-11-09; QCView used the accent).
const LIVE_RED: min_theme::Color = min_theme::Color::rgba(0xe5 as f32 / 255.0, 0x48 as f32 / 255.0, 0x4d as f32 / 255.0, 1.0);
/// QCView's catalog context (canon row 195).
fn tr(s: &str) -> String { min_i18n::trc_hint("CompactStrip", s) }

impl CompactStrip {
    /// A language switch (canon row 197): the labels built at construction, rebuilt; the state stays.
    pub fn relabel(&mut self) { let f = Self::new(); self.play.tooltip = f.play.tooltip; self.pin.tooltip = f.pin.tooltip; self.exit.tooltip = f.exit.tooltip; }
    pub fn new() -> CompactStrip {
        CompactStrip { play: ActionButton::new("play", &tr("Play (Space)")).size_s(), pin: ActionButton::new("push-pin", &tr("Always on Top (Ctrl+Alt+T)")).size_s(), exit: ActionButton::new("arrows-out-simple", &tr("Leave Compact Mode (Ctrl+Shift+C, Esc)")).size_s(),
            live: false, tc: String::new(), tc_rect: Rect::ZERO, tc_hover: false, tooltips: Tooltips::default(), rect: Rect::ZERO, scrub_rect: Rect::ZERO }
    }
    pub fn layout(&mut self, fonts: &mut Fonts, w: f64, y: f64, scale: f64) {
        let h = t::SCRUBBER_HEIGHT as f64 * scale; let pad = t::SPACING_100 as f64 * scale; let gap = t::SPACING_75 as f64 * scale;
        let (bw, bh) = self.play.measure(fonts, scale); let by = y + (h - bh) / 2.0;
        let mut x = pad;
        self.play.rect = Rect::new(x, by, x + bw, by + bh); x += bw + gap;
        let l = fonts.layout(&self.tc, Family::Mono, t::TIMECODE_WELL_FONT_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, peniko::Color::WHITE);
        let tw = l.width() as f64 + 2.0 * t::FIELD_EDGE_TO_TEXT as f64 * scale;
        self.tc_rect = Rect::new(x, by, x + tw, by + bh); x += tw + gap;
        let mut rx = w - pad;
        self.exit.rect = Rect::new(rx - bw, by, rx, by + bh); rx -= bw + gap;
        self.pin.rect = Rect::new(rx - bw, by, rx, by + bh); rx -= bw + gap;
        self.scrub_rect = Rect::new(x, y, rx.max(x + 1.0), y + h);
        self.rect = Rect::new(0.0, y, w, y + h);
    }
    /// A live feed (QCView CompactStrip: the LIVE badge in the accent, a live dot at the track's start).
    pub fn set_live(&mut self, on: bool) { self.live = on; if on { self.tc = tr("LIVE"); } }
    pub fn sync(&mut self, p: Option<&dyn qcv_player::Playback>, tc: &str, pinned: bool, now: Instant) {
        let has = p.is_some();
        if self.play.enabled != has { self.play.enabled = has; if !has { self.play.reset(now); } }
        if let Some(p) = p { self.play.icon = if p.is_playing() { "pause" } else { "play" }; self.play.tooltip = tr(if p.is_playing() { "Pause (Space)" } else { "Play (Space)" }); }
        self.tc = if self.live { tr("LIVE") } else { tc.to_string() };
        self.pin.selected = pinned;
    }
    /// Returns (commands, redraw needed, consumed).
    pub fn event(&mut self, e: &Event, now: Instant) -> (Vec<Cmd>, bool, bool) {
        let mut cmds = vec![]; let mut dirty = false;
        for (b, cmd) in [(&mut self.play, Cmd::Act(Action::TogglePlayback)), (&mut self.pin, Cmd::Pin), (&mut self.exit, Cmd::Exit)] {
            match b.event(e, now) { Response::None => {} Response::Clicked => { cmds.push(cmd); dirty = true; } _ => dirty = true }
        }
        let over_tc = e.pos().is_some_and(|(x, y)| self.tc_rect.contains(kurbo::Point::new(x, y))) && !matches!(e, Event::Leave);
        if over_tc != self.tc_hover { self.tc_hover = over_tc; dirty = true; }
        if over_tc { if let Event::Down { button: min_ui::Button::Primary, .. } = e { cmds.push(Cmd::DragWindow); } }
        if matches!(e, Event::Down { .. }) { self.tooltips.pressed(now); }
        let over: Option<(u64, Rect, String)> = if self.play.hovered() { Some((1, self.play.rect, self.play.tooltip.clone())) } else if self.pin.hovered() { Some((2, self.pin.rect, self.pin.tooltip.clone())) } else if self.exit.hovered() { Some((3, self.exit.rect, self.exit.tooltip.clone())) } else if self.tc_hover { Some((4, self.tc_rect, "Drag to move the window".into())) } else { None };
        if self.tooltips.hover(over.as_ref().map(|(k, r, s)| (*k, *r, s.as_str())), now) { dirty = true; }
        let consumed = !cmds.is_empty() || e.pos().is_some_and(|(x, y)| self.play.hit(x, y) || self.pin.hit(x, y) || self.exit.hit(x, y) || self.tc_rect.contains(kurbo::Point::new(x, y)));
        (cmds, dirty, consumed)
    }
    pub fn tick(&mut self, now: Instant) -> bool { self.tooltips.tick(now) || self.play.animating(now) || self.pin.animating(now) || self.exit.animating(now) }
    /// The controls over the scrubber's band (the scrubber paints the band itself).
    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, now: Instant) {
        // the controls' own backdrop so the scrub line never runs under them
        paint::rect(scene, Rect::new(self.rect.x0, self.rect.y0 + t::HAIRLINE as f64 * scale, self.scrub_rect.x0, self.rect.y1), t::SCRUBBER_BACKGROUND_COLOR);
        paint::rect(scene, Rect::new(self.scrub_rect.x1, self.rect.y0 + t::HAIRLINE as f64 * scale, self.rect.x1, self.rect.y1), t::SCRUBBER_BACKGROUND_COLOR);
        self.play.paint(scene, fonts, scale, now); self.pin.paint(scene, fonts, scale, now); self.exit.paint(scene, fonts, scale, now);
        if self.live { let d = 8.0 * scale; let y = self.scrub_rect.y0 + (self.scrub_rect.height() - d) / 2.0; paint::rounded(scene, Rect::new(self.scrub_rect.x0, y, self.scrub_rect.x0 + d, y + d), d / 2.0, LIVE_RED); }
        let c = if self.live { LIVE_RED } else if self.tc_hover { t::NEUTRAL_CONTENT_COLOR_HOVER } else { t::NEUTRAL_CONTENT_COLOR_DEFAULT };
        let l = fonts.layout(&self.tc, Family::Mono, t::TIMECODE_WELL_FONT_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(c));
        min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((self.tc_rect.x0 + t::FIELD_EDGE_TO_TEXT as f64 * scale, self.tc_rect.y0 + (self.tc_rect.height() - l.height() as f64) / 2.0)));
    }
    pub fn paint_tooltip(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, bounds: Rect, now: Instant) { self.tooltips.paint(scene, fonts, scale, bounds, now); }
}
