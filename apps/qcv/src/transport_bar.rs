//! The transport bar as `min-ui` nodes (canon §25 row 169; QCView's `TransportBar.qml` order and tooltips carried):
//! open · [prev-clip, start, rewind (hold), step back, play / pause, step forward, fast-forward (hold), end, next-clip]
//! as a COMPACT fused group, and so are in / out / clear · loop / fullscreen / compact · screenshots · guides /
//! background / hover-thumbs / colour / notes · mute (the volume slider joins it with audio) — the user 2026-11-07:
//! "bring our other buttons into groups as well" · … · the pin flush right. Buttons the Transport backs emit its `Action` (the
//! same path as the keys); the rest are drawn disabled until their step lands.
use kurbo::Rect;
use min_theme::tokens as t;
use min_ui::{ActionButton, ActionGroup, Event, Fonts, Response, Tooltips};
use qcv_player::{Action, Transport};
use std::time::Instant;
use vello::Scene;

#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(dead_code)]   // Pin arrives with the window-state step
pub enum Cmd { Open, Act(Action), Pin }

/// Which button does what; `None` = not backed yet (drawn disabled).
#[derive(Clone, Copy)]
enum Bind { Open, Act(Action), Hold(i32), Pin, Later }

pub struct TransportBar {
    groups: Vec<ActionGroup>,
    binds: Vec<Vec<Bind>>,
    pin: ActionGroup,
    pub tooltips: Tooltips,
    pub rect: Rect,
}

impl TransportBar {
    pub fn new() -> TransportBar {
        let b = |icon, tip| ActionButton::new(icon, tip);
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
                (b("arrows-in-simple", "Compact Mode (Ctrl+Shift+C)"), Bind::Later),
            ]),
            (true, vec![
                (b("file-image", "Screenshot to clipboard (⌥T)"), Bind::Later),
                (b("file-arrow-down", "Screenshot to Desktop (T)"), Bind::Later),
            ]),
            (true, vec![
                (b("rectangle-dashed", "Safety guides"), Bind::Later),
                (b("checkerboard", "Background"), Bind::Later),
                (b("image-square", "Timeline hover thumbnails"), Bind::Later),
                (b("palette", "Color panel (⌃3)"), Bind::Later),
                (b("note-pencil", "Notes panel (⌃4)"), Bind::Later),
            ]),
            // mute alone for now; the volume slider joins this group when audio lands (step 2b)
            (true, vec![(b("speaker-high", "Mute (M)"), Bind::Later)]),
        ];
        let mut gs = vec![]; let mut binds = vec![];
        for (compact, items) in groups {
            let (buttons, bs): (Vec<_>, Vec<_>) = items.into_iter().unzip();
            let g = ActionGroup::new(buttons); gs.push(if compact { g.compact() } else { g }); binds.push(bs);
        }
        let pin = ActionGroup::new(vec![b("push-pin", "Always on Top (Ctrl+Alt+T)")]);
        TransportBar { groups: gs, binds, pin, tooltips: Tooltips::default(), rect: Rect::ZERO }
    }

    /// Lay the row out across `[0, w)` at `y` with the bar's height; the pin flush right.
    pub fn layout(&mut self, fonts: &mut Fonts, w: f64, y: f64, scale: f64) {
        let h = t::TRANSPORT_BAR_HEIGHT as f64 * scale;
        let pad = t::SPACING_100 as f64 * scale; let cluster = t::ACTION_GROUP_CLUSTER_GAP as f64 * scale;
        let by = y + (h - t::ACTION_BUTTON_HEIGHT_M as f64 * scale) / 2.0;
        let mut x = pad;
        for g in self.groups.iter_mut() { x += g.layout(fonts, x, by, scale) + cluster; }
        let pw = self.pin.layout(fonts, 0.0, by, scale);
        self.pin.layout(fonts, w - pad - pw, by, scale);
        self.rect = Rect::new(0.0, y, w, y + h);
    }

    /// Mirror the Transport's state into the buttons (QCView's bindings): enabled when media is open, play ↔ pause,
    /// in / out selected while set, clear only while either is set, loop selected while on.
    pub fn sync(&mut self, p: Option<&Transport>, now: Instant) {
        let has = p.is_some();
        for (g, binds) in self.groups.iter_mut().zip(&self.binds) {
            for (b, bind) in g.buttons.iter_mut().zip(binds) {
                let enabled = match bind { Bind::Open => true, Bind::Later | Bind::Pin => false, Bind::Act(Action::ToggleFullscreen) => true, Bind::Act(Action::ToggleLoop) => true, Bind::Act(_) | Bind::Hold(_) => has };
                if b.enabled != enabled { b.enabled = enabled; if !enabled { b.reset(now); } }
                if let (Some(p), Bind::Act(a)) = (p, bind) {
                    match a {
                        Action::TogglePlayback => { b.icon = if p.is_playing() { "pause" } else { "play" }; b.tooltip = if p.is_playing() { "Pause (Space)".into() } else { "Play (Space)".into() }; }
                        Action::SetInPoint => b.selected = p.in_point() >= 0,
                        Action::SetOutPoint => b.selected = p.out_point() >= 0,
                        Action::ClearInOut => { let on = p.in_point() >= 0 || p.out_point() >= 0; if b.enabled != on { b.enabled = on; if !on { b.reset(now); } } }
                        Action::ToggleLoop => b.selected = p.loop_enabled(),
                        _ => {}
                    }
                } else if let Bind::Act(Action::ToggleLoop) = bind { b.selected = true; }   // loop defaults ON before media opens (QCView)
            }
        }
        self.pin.buttons[0].enabled = false;
    }

    /// Route a pointer event; the commands it produced. Also feeds the tooltip with what the pointer rests on.
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
                    (Bind::Pin, Response::Clicked) => cmds.push(Cmd::Pin),
                    _ => {}
                }
            }
        }
        for (_, r) in self.pin.event(e, now) { dirty = true; if r == Response::Clicked { cmds.push(Cmd::Pin); } }
        if matches!(e, Event::Move { .. } | Event::Leave) { dirty = true; }   // hover ramps start on a move
        if matches!(e, Event::Down { .. }) { self.tooltips.pressed(now); dirty = true; }
        let over = self.hovered().map(|(k, r, s)| (k, r, s.to_string()));
        if self.tooltips.hover(over.as_ref().map(|(k, r, s)| (*k, *r, s.as_str())), now) { dirty = true; }
        (cmds, dirty)
    }
    fn hovered(&self) -> Option<(u64, Rect, &str)> {
        for (gi, g) in self.groups.iter().enumerate() { if let Some(i) = g.hovered() { let b = &g.buttons[i]; return Some(((gi as u64) << 8 | i as u64, b.rect, &b.tooltip)) } }
        self.pin.hovered().map(|i| { let b = &self.pin.buttons[i]; (1 << 20, b.rect, b.tooltip.as_str()) })
    }
    /// Per-frame: tooltip timing; true when a redraw is needed now.
    pub fn tick(&mut self, now: Instant) -> bool { self.tooltips.tick(now) || self.groups.iter().any(|g| g.animating(now)) || self.pin.animating(now) }
    #[allow(dead_code)]   // the app redraws every frame today; the on-demand scheduler uses these
    pub fn next_change(&self, now: Instant) -> Option<Instant> { self.tooltips.next_change(now) }
    #[allow(dead_code)]
    pub fn hit(&self, x: f64, y: f64) -> bool { self.rect.contains(kurbo::Point::new(x, y)) }
    /// A button's rect by its icon slug (the harness drives hover / press by name).
    pub fn rect_of(&self, icon: &str) -> Option<Rect> { self.groups.iter().flat_map(|g| g.buttons.iter()).chain(self.pin.buttons.iter()).find(|b| b.icon == icon).map(|b| b.rect) }

    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, now: Instant) {
        for g in &self.groups { g.paint(scene, fonts, scale, now); }
        self.pin.paint(scene, fonts, scale, now);
    }
    /// The tooltip, drawn last so it sits over every band.
    pub fn paint_tooltip(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, bounds: Rect, now: Instant) { self.tooltips.paint(scene, fonts, scale, bounds, now); }
}
