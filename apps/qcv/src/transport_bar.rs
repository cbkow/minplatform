//! The transport bar as `min-ui` nodes (canon §25 row 169; QCView's `TransportBar.qml` order and tooltips carried):
//! open · [prev-clip, start, rewind (hold), step back, play / pause, step forward, fast-forward (hold), end, next-clip]
//! as a COMPACT fused group, and so are in / out / clear · loop / fullscreen / compact · screenshots · guides /
//! background / hover-thumbs / colour / notes · mute (the volume slider joins it with audio) — the user 2026-11-07:
//! "bring our other buttons into groups as well" · … · the pin flush right. Buttons the Transport backs emit its `Action` (the
//! same path as the keys); the rest are drawn disabled until their step lands.
use kurbo::Rect;
use min_theme::tokens as t;
use min_ui::{anim::{mix, Ramp}, paint, ActionButton, ActionGroup, Button, Event, Family, Fonts, Response, Slider, Tooltips};
use qcv_player::Action;
use std::time::Instant;
use vello::Scene;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cmd { Open, Act(Action), Pin, Volume(f32), /// QCView's Tools ▸ Background: open the left rail on the Background section
    RevealBackground, ToggleCompact, /// the Color drawer (row 199)
    ToggleColor }

/// QCView's review-speed chip: "1×" at rest, cycles the ring on click (R), snaps to 1× on Shift-click (⇧R).
struct SpeedChip { rect: Rect, text: String, off_unity: bool, hovered: bool, hover: Ramp, enabled: bool }

/// Which button does what; `None` = not backed yet (drawn disabled).
#[derive(Clone, Copy)]
enum Bind { Open, Act(Action), Hold(i32), RevealBackground, ToggleCompact, ToggleColor, Later }

/// The window scale the bar was laid out at (its height is 32 logical px).
fn scale_of(bar: &Rect, _: Rect) -> f64 { (bar.height() / t::TRANSPORT_BAR_HEIGHT as f64).max(0.5) }

pub struct TransportBar {
    groups: Vec<ActionGroup>,
    binds: Vec<Vec<Bind>>,
    pin: ActionGroup,
    /// QCView's 90-px volume slider, in the mute group's cluster (the user 2026-11-07: "mute … in its own group with the eventual volume slider").
    volume: Slider,
    speed: SpeedChip, shift: bool,
    pub tooltips: Tooltips,
    pub rect: Rect,
}

/// QCView's catalog context (canon row 195); our tooltips add the key in parentheses, kept as written when only the head is in the catalog.
fn tr(s: &str) -> String { min_i18n::trc_hint("TransportBar", s) }

impl TransportBar {
    pub fn new() -> TransportBar {
        let b = |icon, tip| ActionButton::new(icon, &tr(tip));
        let groups: Vec<(bool, Vec<(ActionButton, Bind)>)> = vec![
            (false, vec![(b("folder-simple", "Open media…"), Bind::Open)]),
            (true, vec![
                (b("arrow-line-left", "Previous clip"), Bind::Later),
                (b("skip-back", "Skip to start (Home)"), Bind::Act(Action::SeekToStart)),
                (b("rewind", "Rewind (hold; J)").hold(), Bind::Hold(-1)),
                (b("caret-left", "Step back 1 frame (←)"), Bind::Act(Action::Step(-1))),
                (b("play", "Play (Space)"), Bind::Act(Action::TogglePlayback)),
                (b("caret-right", "Step forward 1 frame (→)"), Bind::Act(Action::Step(1))),
                (b("fast-forward", "Fast-forward (hold; L)").hold(), Bind::Hold(1)),
                (b("skip-forward", "Skip to end (End)"), Bind::Act(Action::SeekToEnd)),
                (b("arrow-line-right", "Next clip"), Bind::Later),
            ]),
            (true, vec![
                (b("sign-out", "Set in point (I)"), Bind::Act(Action::SetInPoint)),
                (b("sign-in", "Set out point (O)"), Bind::Act(Action::SetOutPoint)),
                (b("x-circle", "Clear in/out points (Shift+I)"), Bind::Act(Action::ClearInOut)),
            ]),
            (true, vec![
                (b("repeat", "Loop (V)"), Bind::Act(Action::ToggleLoop)),
                (b("frame-corners", "Fullscreen (F)"), Bind::Act(Action::ToggleFullscreen)),
                (b("arrows-in-simple", "Compact Mode (Ctrl+Shift+C)"), Bind::ToggleCompact),
            ]),
            (true, vec![
                (b("file-image", "Screenshot to clipboard (⌥T)"), Bind::Later),
                (b("file-arrow-down", "Screenshot to Desktop (T)"), Bind::Later),
            ]),
            (true, vec![
                (b("rectangle-dashed", "Safety guides"), Bind::Later),
                (b("checkerboard", "Background"), Bind::RevealBackground),
                (b("image-square", "Timeline hover thumbnails"), Bind::Later),
                (b("palette", "Color panel (⌃3)"), Bind::ToggleColor),
                (b("note-pencil", "Notes panel (⌃4)"), Bind::Later),
            ]),
            // mute alone for now; the volume slider joins this group when audio lands (step 2b)
            (true, vec![(b("speaker-high", "Mute (M)"), Bind::Act(Action::ToggleMute))]),
        ];
        let mut gs = vec![]; let mut binds = vec![];
        for (compact, items) in groups {
            let (buttons, bs): (Vec<_>, Vec<_>) = items.into_iter().unzip();
            let g = ActionGroup::new(buttons); gs.push(if compact { g.compact() } else { g }); binds.push(bs);
        }
        let pin = ActionGroup::new(vec![b("push-pin", "Always on Top (Ctrl+Alt+T)")]);
        let mut volume = Slider::new(1.0); volume.tooltip = tr("Volume");
        TransportBar { groups: gs, binds, pin, volume, speed: SpeedChip { rect: Rect::ZERO, text: "1×".into(), off_unity: false, hovered: false, hover: Ramp::new(0.0, t::ANIMATION_DURATION_HOVER), enabled: false }, shift: false, tooltips: Tooltips::default(), rect: Rect::ZERO }
    }

    /// Lay the row out across `[0, w)` at `y` with the bar's height; the pin flush right.
    pub fn layout(&mut self, fonts: &mut Fonts, w: f64, y: f64, scale: f64) {
        let h = t::TRANSPORT_BAR_HEIGHT as f64 * scale;
        let pad = t::SPACING_100 as f64 * scale; let cluster = t::ACTION_GROUP_CLUSTER_GAP as f64 * scale;
        let by = y + (h - t::ACTION_BUTTON_HEIGHT_M as f64 * scale) / 2.0;
        let mut x = pad;
        let last = self.groups.len() - 1;
        for (i, g) in self.groups.iter_mut().enumerate() {
            x += g.layout(fonts, x, by, scale);
            if i == 1 {
                // the review-speed chip sits with the transport cluster (QCView: right after skip-forward)
                x += t::SPACING_75 as f64 * scale;
                let l = fonts.layout(&self.speed.text, Family::Mono, t::REVIEW_SPEED_CHIP_FONT_SIZE * scale as f32, t::FONT_WEIGHT_BOLD as f32, peniko::Color::WHITE);
                let w = l.width() as f64 + 2.0 * t::FIELD_EDGE_TO_TEXT as f64 * scale; let h = t::REVIEW_SPEED_CHIP_HEIGHT as f64 * scale;
                let cy = by + (t::ACTION_BUTTON_HEIGHT_M as f64 * scale - h) / 2.0;
                self.speed.rect = Rect::new(x, cy, x + w, cy + h); x += w;
            }
            if i == last { x += t::SPACING_75 as f64 * scale; let w = 90.0 * scale; self.volume.rect = Rect::new(x, by, x + w, by + t::ACTION_BUTTON_HEIGHT_M as f64 * scale); x += w; }
            x += cluster;
        }
        let pw = self.pin.layout(fonts, 0.0, by, scale);
        self.pin.layout(fonts, w - pad - pw, by, scale);
        self.rect = Rect::new(0.0, y, w, y + h);
    }

    /// Mirror the Transport's state into the buttons (QCView's bindings): enabled when media is open, play ↔ pause,
    /// in / out selected while set, clear only while either is set, loop selected while on.
    pub fn sync(&mut self, p: Option<&dyn qcv_player::Playback>, now: Instant) {
        let has = p.is_some();
        for (g, binds) in self.groups.iter_mut().zip(&self.binds) {
            for (b, bind) in g.buttons.iter_mut().zip(binds) {
                let enabled = match bind { Bind::Open | Bind::RevealBackground | Bind::ToggleCompact | Bind::ToggleColor => true, Bind::Later => false, Bind::Act(Action::ToggleFullscreen) => true, Bind::Act(Action::ToggleLoop) => true, Bind::Act(Action::ToggleMute) => p.is_some_and(|p| p.has_audio()), Bind::Act(_) | Bind::Hold(_) => has };
                if b.enabled != enabled { b.enabled = enabled; if !enabled { b.reset(now); } }
                if let (Some(p), Bind::Act(a)) = (p, bind) {
                    match a {
                        Action::TogglePlayback => { b.icon = if p.is_playing() { "pause" } else { "play" }; b.tooltip = tr(if p.is_playing() { "Pause (Space)" } else { "Play (Space)" }); }
                        Action::SetInPoint => b.selected = p.in_point() >= 0,
                        Action::SetOutPoint => b.selected = p.out_point() >= 0,
                        Action::ClearInOut => { let on = p.in_point() >= 0 || p.out_point() >= 0; if b.enabled != on { b.enabled = on; if !on { b.reset(now); } } }
                        Action::ToggleLoop => b.selected = p.loop_enabled(),
                        Action::ToggleMute => { let has = p.has_audio(); if b.enabled != has { b.enabled = has; if !has { b.reset(now); } } let m = p.is_muted(); b.icon = if m { "speaker-x" } else { "speaker-high" }; b.tooltip = tr(if m { "Unmute (M)" } else { "Mute (M)" }); b.selected = m; }
                        _ => {}
                    }
                } else if let Bind::Act(Action::ToggleLoop) = bind { b.selected = true; }   // loop defaults ON before media opens (QCView)
            }
        }
        self.pin.buttons[0].enabled = true;
        if let Some(p) = p { let sp = p.review_speed(); let mut txt = format!("{sp:.2}"); while txt.ends_with('0') { txt.pop(); } if txt.ends_with('.') { txt.pop(); } self.speed.text = format!("{txt}×"); self.speed.off_unity = (sp - 1.0).abs() > 0.001; self.speed.enabled = true; } else { self.speed.enabled = false; }
        if let Some(p) = p { let on = p.has_audio() && !p.is_muted(); self.volume.enabled = on; if !self.volume.dragging() { self.volume.value = p.volume(); } self.volume.tooltip = min_i18n::fill(&tr("Volume — %1%"), &[&((p.volume() * 100.0).round() as i32)]); } else { self.volume.enabled = false; }
    }

    /// Route a pointer event; the commands it produced. Also feeds the tooltip with what the pointer rests on.
    /// Some node of the bar holds the pointer (a held shuttle button, an unreleased click, the volume slider's drag): the
    /// shell must route Move / Up here before any hit-tested panel, or a release over a rail is lost and a hold runs on.
    pub fn is_pressed(&self) -> bool { self.groups.iter().chain(std::iter::once(&self.pin)).any(|g| g.buttons.iter().any(|b| b.is_pressed())) || self.volume.dragging() }
    pub fn event(&mut self, e: &Event, now: Instant) -> (Vec<Cmd>, bool) {
        let mut cmds = vec![]; let mut dirty = false;
        for (gi, g) in self.groups.iter_mut().enumerate() {
            for (i, r) in g.event(e, now) {
                dirty = true;
                match (self.binds[gi][i], r) {
                    (Bind::Open, Response::Clicked) => cmds.push(Cmd::Open),
                    (Bind::Act(a), Response::Clicked) => cmds.push(Cmd::Act(a)),
                    (Bind::Hold(d), Response::PressBegan) => cmds.push(Cmd::Act(Action::ShuttleBegin(d))),
                    (Bind::Hold(_), Response::PressEnded) => cmds.push(Cmd::Act(Action::ShuttleEnd)),
                    (Bind::RevealBackground, Response::Clicked) => cmds.push(Cmd::RevealBackground),
                    (Bind::ToggleColor, Response::Clicked) => cmds.push(Cmd::ToggleColor),
                    (Bind::ToggleCompact, Response::Clicked) => cmds.push(Cmd::ToggleCompact),
                    _ => {}
                }
            }
        }
        for (_, r) in self.pin.event(e, now) { dirty = true; if r == Response::Clicked { cmds.push(Cmd::Pin); } }
        let (v, d) = self.volume.event(e, now, scale_of(&self.rect, self.volume.rect)); if d { dirty = true; } if let Some(v) = v { cmds.push(Cmd::Volume(v)); }
        {
            let inside = e.pos().is_some_and(|(x, y)| self.speed.rect.contains(kurbo::Point::new(x, y))) && self.speed.enabled;
            let was = self.speed.hovered; self.speed.hovered = inside && !matches!(e, Event::Leave); if was != self.speed.hovered { dirty = true; }
            self.speed.hover.set_target(if self.speed.hovered { 1.0 } else { 0.0 }, now);
            if let Event::Down { button: Button::Primary, .. } = e { if inside { cmds.push(Cmd::Act(if self.shift { Action::ResetReviewSpeed } else { Action::CycleReviewSpeed })); dirty = true; } }
        }
        if matches!(e, Event::Move { .. } | Event::Leave) { dirty = true; }   // hover ramps start on a move
        if matches!(e, Event::Down { .. }) { self.tooltips.pressed(now); dirty = true; }
        let over = self.hovered().map(|(k, r, s)| (k, r, s.to_string()));
        if self.tooltips.hover(over.as_ref().map(|(k, r, s)| (*k, *r, s.as_str())), now) { dirty = true; }
        (cmds, dirty)
    }
    /// Always on Top's state (the pin reads selected while the window floats).
    pub fn set_pinned(&mut self, on: bool) { self.pin.buttons[0].selected = on; }
    /// The shell's modifier state (Shift-click on the chip snaps to 1×).
    pub fn set_shift(&mut self, on: bool) { self.shift = on; }
    fn hovered(&self) -> Option<(u64, Rect, &str)> {
        if self.speed.hovered { return Some((1 << 22, self.speed.rect, min_i18n::trc("TransportBar", "Review speed — click to cycle (R), Shift+click for 1× (Shift+R)"))) }
        if self.volume.hovered() || self.volume.dragging() { return Some((1 << 21, self.volume.rect, &self.volume.tooltip)) }
        for (gi, g) in self.groups.iter().enumerate() { if let Some(i) = g.hovered() { let b = &g.buttons[i]; return Some(((gi as u64) << 8 | i as u64, b.rect, &b.tooltip)) } }
        self.pin.hovered().map(|i| { let b = &self.pin.buttons[i]; (1 << 20, b.rect, b.tooltip.as_str()) })
    }
    /// Per-frame: tooltip timing; true when a redraw is needed now.
    pub fn tick(&mut self, now: Instant) -> bool { self.tooltips.tick(now) || self.groups.iter().any(|g| g.animating(now)) || self.pin.animating(now) || self.volume.animating(now) || self.speed.hover.animating(now) }
    #[allow(dead_code)]   // the app redraws every frame today; the on-demand scheduler uses these
    pub fn next_change(&self, now: Instant) -> Option<Instant> { self.tooltips.next_change(now) }
    #[allow(dead_code)]
    pub fn hit(&self, x: f64, y: f64) -> bool { self.rect.contains(kurbo::Point::new(x, y)) }
    /// A button's rect by its icon slug (the harness drives hover / press by name).
    pub fn rect_of(&self, icon: &str) -> Option<Rect> { self.groups.iter().flat_map(|g| g.buttons.iter()).chain(self.pin.buttons.iter()).find(|b| b.icon == icon).map(|b| b.rect) }

    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, now: Instant) {
        for g in &self.groups { g.paint(scene, fonts, scale, now); }
        self.pin.paint(scene, fonts, scale, now);
        self.volume.paint(scene, scale, now);
        // the chip: the wells' recessed look, border stepping up on hover, bold content colour off 1×
        let c = &self.speed; let r = t::REVIEW_SPEED_CHIP_CORNER_RADIUS as f64 * scale; let bw = t::BORDER_WIDTH_100 as f64 * scale;
        paint::rounded(scene, c.rect, r, mix(t::FIELD_BORDER_COLOR, t::FIELD_BORDER_COLOR_HOVER, c.hover.value(now)));
        paint::rounded(scene, c.rect.inset(-bw), (r - bw).max(0.0), t::FIELD_BACKGROUND_COLOR);
        let color = if !c.enabled { t::DISABLED_CONTENT_COLOR } else if c.off_unity { t::REVIEW_SPEED_CHIP_OFF_UNITY_COLOR } else { t::REVIEW_SPEED_CHIP_COLOR };
        let l = fonts.layout(&c.text, Family::Mono, t::REVIEW_SPEED_CHIP_FONT_SIZE * scale as f32, if c.off_unity { t::FONT_WEIGHT_BOLD } else { t::FONT_WEIGHT_REGULAR } as f32, paint::color(color));
        min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((c.rect.x0 + (c.rect.width() - l.width() as f64) / 2.0, c.rect.y0 + (c.rect.height() - l.height() as f64) / 2.0)));
    }
    /// The tooltip, drawn last so it sits over every band.
    pub fn paint_tooltip(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, bounds: Rect, now: Instant) { self.tooltips.paint(scene, fonts, scale, bounds, now); }
}
