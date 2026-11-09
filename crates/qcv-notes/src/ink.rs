//! Strokes as they are drawn (canon row 214): our own smoother in place of Google's ink-stroke-modeler, and the geometry every
//! stroke draws as — on screen and in the exports alike (one path, so they match).
//!
//! THE SMOOTHER works in QCView's units (normalized × 1000): a one-euro filter on the pointer (heavy smoothing when it moves
//! slowly — no wobble — light when it moves fast — no lag), feeding the modeler's spring-mass-damper position model with
//! QCView's constants (`spring_mass` 11 / 32400, `drag` 72), sampled at ≥ 300 Hz (QCView `min_output_rate`); at the release it
//! settles onto the release point (stopping distance 1 unit, ≤ 20 steps). Its output IS the stored freehand stroke
//! (`is_modeled: true` — QCView's contract stores the modeled polyline).
//! THE GEOMETRY maps normalized display-space points onto the picture's rect: freehand = the polyline fitted to Béziers (kurbo,
//! 0.25 px), rect = corners 0 and 2, oval = centre + radii, line / arrow = start → end (the arrowhead: QCView's 25° filled
//! triangle, `12 + 2 × width` long, the shaft stopping at its base — in SOURCE pixels scaled with the picture, so a screenshot
//! matches the screen; QCView's live head was a constant 12 screen px). Widths are source pixels × the picture's scale.
use crate::strokes::{Stroke, Tool};
use kurbo::{BezPath, Ellipse, Line, PathEl, Point, Rect, Shape, Vec2};

const UNITS: f64 = 1000.0;
const SPRING_MASS: f64 = 11.0 / 32400.0;
const DRAG: f64 = 72.0;
const MIN_RATE_HZ: f64 = 300.0;
const STOP_DISTANCE: f64 = 1.0;   // QCView: 0.001 × 1000
const STOP_STEPS: usize = 20;
/// the one-euro filter's tuning (units / s): ~3 Hz at rest, opening up with speed (QCView's wobble range was 31 – 620 units / s)
const MIN_CUTOFF: f64 = 3.0;
const BETA: f64 = 0.02;
/// the speed estimate's cutoff: the textbook 1 Hz lagged a sudden flick by ~3 % of the stroke beyond QCView's spring; 5 Hz keeps it ~1 %
const D_CUTOFF: f64 = 5.0;

fn alpha(cutoff: f64, dt: f64) -> f64 { let tau = 1.0 / (2.0 * std::f64::consts::PI * cutoff); 1.0 / (1.0 + tau / dt) }

#[derive(Clone, Copy)]
struct OneEuro { x: Vec2, dx: Vec2 }

pub struct Smoother { filt: Option<OneEuro>, pos: Vec2, vel: Vec2, anchor: Vec2, t: f64, out: Vec<[f64; 2]> }

impl Smoother {
    /// A stroke begins at `p` (normalized) at `t` seconds.
    pub fn begin(p: [f64; 2], t: f64) -> Smoother {
        let v = Vec2::new(p[0] * UNITS, p[1] * UNITS);
        Smoother { filt: Some(OneEuro { x: v, dx: Vec2::ZERO }), pos: v, vel: Vec2::ZERO, anchor: v, t, out: vec![p] }
    }
    fn step(&mut self, dt: f64, anchor: Vec2) {
        let acc = (anchor - self.pos) / SPRING_MASS - self.vel * DRAG;
        self.vel += acc * dt; self.pos += self.vel * dt;
        self.out.push([self.pos.x / UNITS, self.pos.y / UNITS]);
    }
    /// A pointer sample: filtered, then the model advanced to `t` in ≤ 1 / 300 s steps (the anchor interpolated across them).
    pub fn push(&mut self, p: [f64; 2], t: f64) {
        let dt = (t - self.t).max(1e-4); let raw = Vec2::new(p[0] * UNITS, p[1] * UNITS);
        let f = self.filt.get_or_insert(OneEuro { x: raw, dx: Vec2::ZERO });
        let dx = (raw - f.x) / dt; let ad = alpha(D_CUTOFF, dt); f.dx = f.dx + (dx - f.dx) * ad;
        let a = alpha(MIN_CUTOFF + BETA * f.dx.hypot(), dt); f.x = f.x + (raw - f.x) * a;
        let target = f.x; let from = self.anchor;
        let n = (dt * MIN_RATE_HZ).ceil().max(1.0) as usize;
        for i in 1..=n { let k = i as f64 / n as f64; self.step(dt / n as f64, from + (target - from) * k); }
        self.anchor = target; self.t = t;
    }
    /// The release at `p`: settle onto it (the line ends at the pointer). Returns the modeled polyline (normalized).
    pub fn end(mut self, p: [f64; 2], t: f64) -> Vec<[f64; 2]> {
        self.push(p, t.max(self.t + 1e-3));
        let goal = Vec2::new(p[0] * UNITS, p[1] * UNITS);
        for _ in 0..STOP_STEPS { if (goal - self.pos).hypot() < STOP_DISTANCE { break } self.step(1.0 / MIN_RATE_HZ, goal); }
        self.out
    }
    /// The polyline so far (the live stroke).
    pub fn points(&self) -> &[[f64; 2]] { &self.out }
}

/// What a stroke draws as, in target pixels: outlines to stroke at `width` (round caps / joins), and a filled arrowhead.
pub struct Geometry { pub paths: Vec<BezPath>, pub head: Option<BezPath>, pub width: f64 }

/// `rect` = the displayed picture (x, y, w, h, post-rotation); `px_per_src` = target pixels per source pixel (the picture's scale).
pub fn geometry(s: &Stroke, rect: [f64; 4], px_per_src: f64) -> Geometry {
    let map = |p: [f64; 2]| Point::new(rect[0] + p[0] * rect[2], rect[1] + p[1] * rect[3]);
    let width = (s.width * px_per_src).max(0.5);
    let mut g = Geometry { paths: vec![], head: None, width };
    let pts = &s.points;
    match s.tool {
        Tool::Freehand => {
            if pts.len() == 1 { let c = map(pts[0]); let mut b = BezPath::new(); b.move_to(c); b.line_to(c); g.paths.push(b); return g }   // a dot (round caps)
            let poly: Vec<PathEl> = pts.iter().enumerate().map(|(i, p)| if i == 0 { PathEl::MoveTo(map(*p)) } else { PathEl::LineTo(map(*p)) }).collect();
            g.paths.push(kurbo::simplify::simplify_bezpath(poly, 0.25, &Default::default()));
        }
        Tool::Rectangle if pts.len() >= 3 => { g.paths.push(Rect::from_points(map(pts[0]), map(pts[2])).to_path(0.1)); }
        Tool::Oval if pts.len() >= 2 => { let c = map(pts[0]); g.paths.push(Ellipse::new(c, (pts[1][0] * rect[2], pts[1][1] * rect[3]), 0.0).to_path(0.1)); }
        Tool::Line if pts.len() >= 2 => { g.paths.push(Line::new(map(pts[0]), map(pts[1])).to_path(0.1)); }
        Tool::Arrow if pts.len() >= 2 => {
            let (a, b) = (map(pts[0]), map(pts[1])); let d = b - a; let len = d.hypot();
            if len < 1e-6 { return g }   // QCView saved a one-point arrow that never drew; this one simply has no length
            let u = d / len; let size = (12.0 + 2.0 * s.width) * px_per_src; let half = 25f64.to_radians();
            let base = b - u * (size * half.cos());
            let n = Vec2::new(-u.y, u.x) * (size * half.sin());
            let mut head = BezPath::new(); head.move_to(b); head.line_to(base + n); head.line_to(base - n); head.close_path();
            g.head = Some(head);
            g.paths.push(Line::new(a, if len > size * half.cos() { base } else { a }).to_path(0.1));
        }
        _ => {}
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_smoother_irons_out_wobble_and_ends_on_the_pointer() {
        // a slow horizontal stroke with ±0.004 jitter in y, sampled at 120 Hz over 1 s
        let mut s = Smoother::begin([0.1, 0.5], 0.0);
        let mut raw_dev = 0.0f64;
        for i in 1..=120 { let t = i as f64 / 120.0; let jit: f64 = if i % 2 == 0 { 0.004 } else { -0.004 }; raw_dev = raw_dev.max(jit.abs()); s.push([0.1 + 0.4 * t, 0.5 + jit], t); }
        let out = s.end([0.5, 0.5], 1.01);
        let dev = out.iter().skip(20).map(|p| (p[1] - 0.5).abs()).fold(0.0, f64::max);
        assert!(dev < raw_dev * 0.35, "wobble cut: {dev} vs raw {raw_dev}");
        let last = out.last().unwrap(); assert!((last[0] - 0.5).abs() < 0.0015 && (last[1] - 0.5).abs() < 0.0015, "ends on the pointer: {last:?}");
        assert!(out.len() >= 300, "≥ 300 Hz output: {}", out.len());
    }
    #[test]
    fn a_fast_stroke_keeps_up() {
        // a quick flick: 0.6 across in 100 ms. QCView's spring alone reaches 0.669 by the last sample (its own inherent lag, ~30 ms);
        // the one-euro filter in front may add at most ~2 % of the stroke on top
        let mut s = Smoother::begin([0.2, 0.2], 0.0);
        for i in 1..=12 { let t = i as f64 / 120.0; s.push([0.2 + 0.6 * (t / 0.1).min(1.0), 0.2], t); }
        let x = s.points().last().unwrap()[0]; assert!(x > 0.669 - 0.012, "lag at speed beyond QCView's spring: {x}");
    }
    #[test]
    fn shapes_land_on_the_picture() {
        let st = |tool, pts: Vec<[f64; 2]>| Stroke { id: "x".into(), tool, color: [1.0, 0.0, 0.0, 1.0], width: 6.0, filled: false, is_modeled: true, points: pts };
        let rect = [100.0, 50.0, 960.0, 540.0];
        let g = geometry(&st(Tool::Rectangle, vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]), rect, 0.5);
        assert_eq!(g.paths[0].bounding_box(), Rect::new(100.0, 50.0, 1060.0, 590.0)); assert_eq!(g.width, 3.0);
        let g = geometry(&st(Tool::Arrow, vec![[0.0, 0.5], [1.0, 0.5]]), rect, 0.5);
        let h = g.head.unwrap().bounding_box(); assert!((h.x1 - 1060.0).abs() < 1e-9 && (h.width() - 12.0 * 25f64.to_radians().cos()).abs() < 1e-6, "head (12 + 2·6) × 0.5 = 12 px long: {h:?}");
        let g = geometry(&st(Tool::Arrow, vec![[0.3, 0.3]]), rect, 1.0); assert!(g.paths.is_empty() && g.head.is_none(), "a one-point arrow draws nothing (and is not saved by the app)");
    }
}
