//! Compact mode's controls (canon row 617, the owner 2026-11-26: the Quick Look preview's glass bar taken for compact
//! mode; before it row 172's opaque band under the picture, QCView's `CompactStrip.qml`). The picture has the whole
//! window. Over its foot a glass bar: the timecode, rewind, play / pause, fast-forward (the hold buttons are row 615's),
//! the scrubber in its line state, the pin (Always on Top) and the exit. The bar shows while the pointer moves over the
//! window, rests on the bar, scrubs or holds a button, and fades when the pointer leaves or has rested two seconds.
//! While it is away the timecode stays, alone on a small plate where the bar has it; a click on the bar's timecode
//! turns that off and on. The window is moved by the picture (the timecode was its handle on the old band).
use kurbo::Rect;
use min_theme::tokens as t;
use min_ui::{anim::Ramp, paint, ActionButton, Event, Family, Fonts, Response, Tooltips};
use std::time::Duration;
use qcv_player::Action;
use std::time::Instant;
use vello::Scene;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd { Act(Action), Pin, Exit, KeepTimecode(bool) }

pub struct CompactStrip {
    rew: ActionButton, play: ActionButton, ff: ActionButton, pin: ActionButton, exit: ActionButton,
    tc: String, tc_rect: Rect, tc_hover: bool, tc_press: bool, live: bool,
    pub tooltips: Tooltips,
    /// the glass bar
    pub rect: Rect,
    /// where the scrubber goes (between the left and right controls)
    pub scrub_rect: Rect,
    /// the timecode stays on the picture while the bar is away (`ui/compactTimecode`)
    pub keep_tc: bool,
    bar: Ramp, drawn: f32, inside: bool, moved: Instant, pointer: (f64, f64),
}

/// The bar leaves after the pointer has rested this long anywhere but on it (the Quick Look preview's time).
const REST: Duration = Duration::from_millis(2000);

/// The live red (the LiveStrip's and the bin's dot, #e5484d): compact LIVE uses it too (the user 2026-11-09; QCView used the accent).
const LIVE_RED: min_theme::Color = min_theme::Color::rgba(0xe5 as f32 / 255.0, 0x48 as f32 / 255.0, 0x4d as f32 / 255.0, 1.0);
/// QCView's catalog context (canon row 195).
fn tr(s: &str) -> String { min_i18n::trc_hint("CompactStrip", s) }
/// The hold buttons' tooltips are the transport bar's own strings (their translations exist there).
fn tr_bar(s: &str) -> String { min_i18n::trc_hint("TransportBar", s) }

impl CompactStrip {
    /// A language switch (canon row 197): the labels built at construction, rebuilt; the state stays.
    pub fn relabel(&mut self) { let f = Self::new(); self.rew.tooltip = f.rew.tooltip; self.ff.tooltip = f.ff.tooltip; self.play.tooltip = f.play.tooltip; self.pin.tooltip = f.pin.tooltip; self.exit.tooltip = f.exit.tooltip; }
    pub fn new() -> CompactStrip {
        let now = Instant::now();
        CompactStrip { rew: ActionButton::new("rewind", &tr_bar("Rewind (hold; J)")).size_s().hold().glass(), ff: ActionButton::new("fast-forward", &tr_bar("Fast-forward (hold; L)")).size_s().hold().glass(), play: ActionButton::new("play", &tr("Play (Space)")).size_s().glass(), pin: ActionButton::new("push-pin", &tr("Always on Top (Ctrl+Alt+T)")).size_s().glass(), exit: ActionButton::new("arrows-out-simple", &tr("Leave Compact Mode (Ctrl+Shift+C, Esc)")).size_s().glass(),
            live: false, tc: String::new(), tc_rect: Rect::ZERO, tc_hover: false, tc_press: false, tooltips: Tooltips::default().glass(), rect: Rect::ZERO, scrub_rect: Rect::ZERO,
            keep_tc: true, bar: Ramp::new(0.0, 160), drawn: 0.0, inside: false, moved: now.checked_sub(REST).unwrap_or(now), pointer: (-1.0, -1.0) }
    }
    fn wanted(&self, scrubbing: bool, now: Instant) -> bool {
        scrubbing || self.rew.is_pressed() || self.ff.is_pressed() || (self.inside && (now.duration_since(self.moved) < REST || self.rect.contains(kurbo::Point::new(self.pointer.0, self.pointer.1))))
    }
    /// How much of the bar is there (0 = away, 1 = shown).
    pub fn alpha(&self, now: Instant) -> f64 { self.bar.value(now) as f64 }
    /// The bar across the window's foot (`w` x `h` the window). `scrubbing`: the scrubber is held. Returns whether the
    /// bar's state changed (a redraw is owed).
    pub fn layout(&mut self, fonts: &mut Fonts, w: f64, h: f64, scale: f64, scrubbing: bool, now: Instant) -> bool {
        let k = scale; let m = 12.0 * k; let gap = 10.0 * k; let tight = t::SPACING_50 as f64 * k;
        self.rect = Rect::new(m, h - m - 44.0 * k, (w - m).max(m + 1.0), h - m);
        let was = self.bar.target(); self.bar.set_target(if self.wanted(scrubbing, now) { 1.0 } else { 0.0 }, now);
        let a = self.bar.value(now); let changed = (was - self.bar.target()).abs() > 0.5 || a != self.drawn; self.drawn = a;   // the ramp's last step is drawn too
        let cy = self.rect.center().y; let (bw, bh) = self.play.measure(fonts, k);
        // the timecode first: it stands at this place whether the bar is there or not
        let l = fonts.layout(&self.tc, Family::Mono, t::TIMECODE_WELL_FONT_SIZE * k as f32, t::FONT_WEIGHT_REGULAR as f32, peniko::Color::WHITE);
        let mut x = self.rect.x0 + 12.0 * k;
        self.tc_rect = Rect::new(x, cy - bh / 2.0, x + l.width() as f64, cy + bh / 2.0); x += l.width() as f64 + gap;
        if self.alpha(now) <= 0.01 { for b in [&mut self.rew, &mut self.play, &mut self.ff, &mut self.pin, &mut self.exit] { b.rect = Rect::ZERO; } self.scrub_rect = Rect::ZERO; return changed }
        for b in [&mut self.rew, &mut self.play, &mut self.ff, &mut self.pin, &mut self.exit] { b.corners = min_ui::action_button::Corners::all(6.0 * k); }
        for b in [&mut self.rew, &mut self.play, &mut self.ff] { b.rect = Rect::new(x, cy - bh / 2.0, x + bw, cy + bh / 2.0); x += bw + tight; }
        x += gap - tight;
        let mut rx = self.rect.x1 - 10.0 * k;
        self.exit.rect = Rect::new(rx - bw, cy - bh / 2.0, rx, cy + bh / 2.0); rx -= bw + tight;
        self.pin.rect = Rect::new(rx - bw, cy - bh / 2.0, rx, cy + bh / 2.0); rx -= bw + gap;
        let sh = (t::SCRUBBER_HEIGHT as f64 * k).min(self.rect.height());
        // a window too narrow for a line to scrub gets none
        self.scrub_rect = if rx - x > 40.0 * k { Rect::new(x, cy - sh / 2.0, rx, cy + sh / 2.0) } else { Rect::ZERO };
        changed
    }
    /// A live feed (QCView CompactStrip: the LIVE badge in the accent, a live dot at the track's start).
    pub fn set_live(&mut self, on: bool) { self.live = on; if on { self.tc = tr("LIVE"); } }
    pub fn sync(&mut self, p: Option<&dyn qcv_player::Playback>, tc: &str, pinned: bool, now: Instant) {
        let has = p.is_some();
        if self.play.enabled != has { self.play.enabled = has; if !has { self.play.reset(now); } }
        // a live feed has nothing to shuttle through
        let shuttle = has && !self.live; for b in [&mut self.rew, &mut self.ff] { if b.enabled != shuttle { b.enabled = shuttle; if !shuttle { b.reset(now); } } }
        if let Some(p) = p { self.play.icon = if p.is_playing() { "pause" } else { "play" }; self.play.tooltip = tr(if p.is_playing() { "Pause (Space)" } else { "Play (Space)" }); }
        self.tc = if self.live { tr("LIVE") } else { tc.to_string() };
        self.pin.selected = pinned;
    }
    /// Returns (commands, redraw needed, consumed).
    pub fn event(&mut self, e: &Event, now: Instant) -> (Vec<Cmd>, bool, bool) {
        let mut cmds = vec![]; let mut dirty = false;
        match e.pos() { Some(p) if !matches!(e, Event::Leave) => { self.inside = true; self.pointer = p; self.moved = now; } _ => self.inside = false }
        for (b, cmd) in [(&mut self.play, Cmd::Act(Action::TogglePlayback)), (&mut self.pin, Cmd::Pin), (&mut self.exit, Cmd::Exit)] {
            match b.event(e, now) { Response::None => {} Response::Clicked => { cmds.push(cmd); dirty = true; } _ => dirty = true }
        }
        for (b, dir) in [(&mut self.rew, -1), (&mut self.ff, 1)] {
            match b.event(e, now) { Response::None => {} Response::PressBegan => { cmds.push(Cmd::Act(Action::ShuttleBegin(dir))); dirty = true; } Response::PressEnded => { cmds.push(Cmd::Act(Action::ShuttleEnd)); dirty = true; } Response::Clicked => dirty = true }
        }
        let shown = self.bar.target() > 0.5;
        let over = |r: Rect| e.pos().is_some_and(|(x, y)| r.contains(kurbo::Point::new(x, y))) && !matches!(e, Event::Leave);
        // the timecode: a click turns its staying on the picture off and on (only while the bar is there - away, it is the picture's)
        let over_tc = shown && over(self.tc_rect.inflate(6.0, 0.0));
        if over_tc != self.tc_hover { self.tc_hover = over_tc; dirty = true; }
        match e { Event::Down { button: min_ui::Button::Primary, .. } => self.tc_press = over_tc, Event::Up { button: min_ui::Button::Primary, .. } => { if self.tc_press && over_tc { self.keep_tc = !self.keep_tc; cmds.push(Cmd::KeepTimecode(self.keep_tc)); dirty = true; } self.tc_press = false; } Event::Leave => self.tc_press = false, _ => {} }
        if matches!(e, Event::Down { .. }) { self.tooltips.pressed(now); }
        let tip: Option<(u64, Rect, String)> = if self.play.hovered() { Some((1, self.play.rect, self.play.tooltip.clone())) } else if self.rew.hovered() { Some((5, self.rew.rect, self.rew.tooltip.clone())) } else if self.ff.hovered() { Some((6, self.ff.rect, self.ff.tooltip.clone())) } else if self.pin.hovered() { Some((2, self.pin.rect, self.pin.tooltip.clone())) } else if self.exit.hovered() { Some((3, self.exit.rect, self.exit.tooltip.clone())) }
            else if self.tc_hover { Some((if self.keep_tc { 4 } else { 7 }, self.tc_rect, tr(if self.keep_tc { "Timecode stays on the picture. Click to hide it with the controls." } else { "Timecode hides with the controls. Click to keep it on the picture." }))) } else { None };
        if self.tooltips.hover(tip.as_ref().map(|(k, r, s)| (*k, *r, s.as_str())), now) { dirty = true; }
        // the bar's own ground is nobody's handle: a press on it (off the scrubber) neither scrubs nor moves the window
        let consumed = !cmds.is_empty() || (shown && over(self.rect) && !over(self.scrub_rect));
        (cmds, dirty, consumed)
    }
    /// A button of the bar is held.
    #[allow(dead_code)] pub fn pressed(&self) -> bool { [&self.rew, &self.play, &self.ff, &self.pin, &self.exit].iter().any(|b| b.is_pressed()) }
    pub fn tick(&mut self, now: Instant) -> bool { self.bar.animating(now) || self.tooltips.tick(now) || self.rew.animating(now) || self.ff.animating(now) || self.play.animating(now) || self.pin.animating(now) || self.exit.animating(now) }
    /// The bar with everything on it, at its present strength; `scrub` paints the scrubber's line into it. Under it,
    /// while it is away, the timecode on its plate.
    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, bounds: Rect, now: Instant, scrub: impl FnOnce(&mut Scene, &mut Fonts)) {
        let k = scale; let a = self.alpha(now) as f32; let glass = t::BACKGROUND_LAYER_1_COLOR.with_alpha(0.9);
        let c = if self.live { LIVE_RED } else if self.tc_hover { t::NEUTRAL_CONTENT_COLOR_HOVER } else { t::NEUTRAL_CONTENT_COLOR_DEFAULT };
        let l = fonts.layout(&self.tc, Family::Mono, t::TIMECODE_WELL_FONT_SIZE * k as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(c));
        let at = kurbo::Affine::translate((self.tc_rect.x0, self.tc_rect.y0 + (self.tc_rect.height() - l.height() as f64) / 2.0));
        let layer = |scene: &mut Scene, alpha: f32, b: &Scene| { scene.push_layer(peniko::Fill::NonZero, peniko::Mix::Normal, alpha, kurbo::Affine::IDENTITY, &bounds); scene.append(b, None); scene.pop_layer(); };
        if self.keep_tc && a < 0.99 && !self.tc.is_empty() {
            let mut b = Scene::new();
            paint::rounded(&mut b, Rect::new(self.tc_rect.x0 - 8.0 * k, self.tc_rect.center().y - l.height() as f64 / 2.0 - 5.0 * k, self.tc_rect.x1 + 8.0 * k, self.tc_rect.center().y + l.height() as f64 / 2.0 + 5.0 * k), 7.0 * k, glass);
            min_ui::fonts::draw_layout(&mut b, &l, at);
            layer(scene, 1.0 - a, &b);
        }
        if a <= 0.01 { return }
        let mut b = Scene::new();
        paint::rounded(&mut b, self.rect, 10.0 * k, glass);
        min_ui::fonts::draw_layout(&mut b, &l, at);
        if self.scrub_rect.width() > 0.0 { scrub(&mut b, fonts); }
        self.rew.paint(&mut b, fonts, k, now); self.ff.paint(&mut b, fonts, k, now); self.play.paint(&mut b, fonts, k, now); self.pin.paint(&mut b, fonts, k, now); self.exit.paint(&mut b, fonts, k, now);
        if self.live && self.scrub_rect.width() > 0.0 { let d = 8.0 * k; let y = self.scrub_rect.y0 + (self.scrub_rect.height() - d) / 2.0; paint::rounded(&mut b, Rect::new(self.scrub_rect.x0, y, self.scrub_rect.x0 + d, y + d), d / 2.0, LIVE_RED); }
        layer(scene, a, &b);
    }
    pub fn paint_tooltip(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, bounds: Rect, now: Instant) { self.tooltips.paint(scene, fonts, scale, bounds, now); }
}
