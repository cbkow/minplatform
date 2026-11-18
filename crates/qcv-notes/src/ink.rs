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
use kurbo::{BezPath, Ellipse, Line, Point, Rect, Shape, Vec2};

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
        // the spring is under-damped and still has the pen's speed at a lift: left to itself it ran PAST the release point (2.4 %
        // of the picture's width after a 40 ms flick) and stopped there. A step that would cross the release point ends on it.
        for _ in 0..STOP_STEPS {
            let before = goal - self.pos; if before.hypot() < STOP_DISTANCE { break }
            self.step(1.0 / MIN_RATE_HZ, goal);
            if (goal - self.pos).dot(before) <= 0.0 { self.out.pop(); break }
        }
        if self.out.last() != Some(&p) { self.out.push(p); }
        self.out
    }
    /// The polyline so far (the live stroke).
    pub fn points(&self) -> &[[f64; 2]] { &self.out }
}

/// A smooth curve THROUGH the points (Catmull-Rom as cubic Béziers): every stored point is on it and each piece stays by its
/// own two points, so the same stroke has the same shape at every size. (kurbo's `simplify_bezpath` stood here: its fit
/// depends on the target's pixels, and on a fast scribble it now and then left the line by 7 % of the picture's width at
/// one size and not at another — a stroke clean on the screen overshot in an export, and the other way round.)
fn through(p: &[Point]) -> BezPath {
    let mut b = BezPath::new(); let n = p.len(); if n == 0 { return b }
    b.move_to(p[0]);
    for i in 0..n - 1 {
        let (a, z) = (p[i], p[i + 1]); let chord = (z - a).hypot();
        // a tangent is the neighbours' chord, a third of it each way, and never longer than a third of the piece it shapes (a
        // long step beside a short one would swing wide of it otherwise)
        let lim = |v: Vec2| { let l = v.hypot(); if l > chord / 3.0 && l > 0.0 { v * (chord / 3.0 / l) } else { v } };
        let t0 = lim((z - p[i.saturating_sub(1)]) / 6.0); let t1 = lim((p[(i + 2).min(n - 1)] - a) / 6.0);
        b.curve_to(a + t0, z - t1, z);
    }
    b
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
            g.paths.push(through(&pts.iter().map(|p| map(*p)).collect::<Vec<_>>()));
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
    fn a_stroke_lifted_at_speed_ends_on_the_pen() {
        // flicks across two thirds of the picture, 200 Hz samples, the pen lifted while it still moves
        for ms in [40.0, 80.0, 160.0] {
            let n = (ms / 5.0) as usize; let mut s = Smoother::begin([0.30, 0.5], 0.0);
            for i in 1..=n { s.push([0.30 + 0.67 * i as f64 / n as f64, 0.5], i as f64 * 0.005); }
            let out = s.end([0.97, 0.5], n as f64 * 0.005);
            let far = out.iter().map(|p| p[0]).fold(0.0, f64::max);
            assert!(far <= 0.97 + 1e-9, "{ms} ms flick: the line went past the pen, to {far}");
            assert_eq!(out.last(), Some(&[0.97, 0.5]), "{ms} ms flick: the line ends where the pen lifted");
        }
    }
    #[test]
    fn a_scribble_draws_the_same_at_every_size() {
        // fast zigzags (the 14 Hz one left kurbo's fit by 7 % of the width at 700 and 1400 px wide, and not at 1920)
        use kurbo::PathEl;
        fn off(p: Point, a: Point, b: Point) -> f64 { let ab = b - a; let t = if ab.hypot2() < 1e-18 { 0.0 } else { ((p - a).dot(ab) / ab.hypot2()).clamp(0.0, 1.0) }; (p - (a + ab * t)).hypot() }
        for (hz, amp) in [(5.0, 0.25), (9.0, 0.2), (14.0, 0.12)] {
            let mut s = Smoother::begin([0.3, 0.5], 0.0); let mut last = [0.3, 0.5];
            for i in 1..240 { let t = i as f64 * 0.005; let ph: f64 = (t * hz) % 1.0; let tri = if ph < 0.5 { ph * 4.0 - 1.0 } else { 3.0 - ph * 4.0 }; last = [0.3 + 0.25 * t, 0.5 + amp * tri]; s.push(last, t); }
            let pts = s.end(last, 1.2);
            for (w, h) in [(700.0, 394.0), (1400.0, 788.0), (1920.0, 1080.0), (3840.0, 2160.0), (5040.0, 3780.0)] {
                let g = geometry(&Stroke { id: "x".into(), tool: Tool::Freehand, color: [1.0, 0.0, 0.0, 1.0], width: 6.0, filled: false, is_modeled: true, points: pts.clone() }, [0.0, 0.0, w, h], w / 1920.0);
                let poly: Vec<Point> = pts.iter().map(|p| Point::new(p[0] * w, p[1] * h)).collect();
                let mut flat = vec![]; kurbo::flatten(g.paths[0].elements().iter().copied(), 0.05, |el| if let PathEl::MoveTo(p) | PathEl::LineTo(p) = el { flat.push(p) });
                let worst = flat.iter().map(|p| poly.windows(2).map(|s| off(*p, s[0], s[1])).fold(f64::MAX, f64::min)).fold(0.0, f64::max);
                assert!(worst / w < 0.0005, "{hz} Hz at {w} px wide: the curve leaves the drawn line by {worst} px ({} % of the width)", 100.0 * worst / w);
            }
        }
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
