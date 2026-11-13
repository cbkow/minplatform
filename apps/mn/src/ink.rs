//! Ink on a canvas (canon rows 274, 324; `docs/phase3/step-canvas-ink-spec.md` §6 has the Qt app's). A stroke is one
//! element of kind `ink`: its record is QCView's envelope with ONE shape - `{"version":"2.0","coordinate_system":
//! "normalized","shapes":[{id, type, color, stroke_width, filled, is_modeled, points}]}` - the points fractions of the
//! element's box (so a resized element needs no new record), the width in the canvas's units. A pen's pressure rides in
//! one more key of the shape, `pressures` (0 … 1, one per point): QCView reads the shape without it and draws the line
//! at its even width.
//!
//! Here a stroke is worked on as a `Line`: the same stroke with its points in the SHEET's units (an oval: its centre,
//! then its radii). `read` and `write` go between the two; `drawn` is what it paints as; `hit` asks whether a point is
//! on the drawn line; `rub` is the eraser passing over it.
use kurbo::{BezPath, Cap, Circle, Join, Point, Rect, Shape, Vec2};
use mn_store::Element;
use qcv_notes::strokes::{self, Stroke, Tool};
use serde_json::{json, Value};

/// The lightest touch draws this share of the pen's width.
pub const LIGHT: f64 = 0.25;
/// A freehand line is kept to within this of what was drawn (the sheet's units).
pub const KEEP: f64 = 0.1;

#[derive(Clone, Debug, PartialEq)]
pub struct Line { pub s: Stroke, pub pressures: Option<Vec<f64>> }

/// The stroke an element holds, in the sheet's units.
pub fn read(e: &Element) -> Option<Line> {
    if e.kind != "ink" { return None }
    let v: Value = serde_json::from_str(&e.record).ok()?;
    let mut s = strokes::parse(Some(&v)).into_iter().next()?;
    let pressures = v.get("shapes").and_then(|x| x.get(0)).and_then(|x| x.get("pressures")).and_then(Value::as_array).map(|a| a.iter().map(|x| x.as_f64().unwrap_or(1.0)).collect::<Vec<_>>()).filter(|p| p.len() == s.points.len());
    let oval = s.tool == Tool::Oval;
    for (i, p) in s.points.iter_mut().enumerate() { *p = if oval && i == 1 { [p[0] * e.w, p[1] * e.h] } else { [e.x + p[0] * e.w, e.y + p[1] * e.h] }; }
    Some(Line { s, pressures })
}
/// How far an arrow's head reaches to either side of its shaft.
fn head(w: f64) -> f64 { (12.0 + 2.0 * w) * 25f64.to_radians().sin() }
/// The box a stroke needs: its points', grown by half its width and a little (whole units).
pub fn bounds(l: &Line) -> Rect {
    let p = &l.s.points;
    let r = if l.s.tool == Tool::Oval && p.len() >= 2 { Rect::new(p[0][0] - p[1][0].abs(), p[0][1] - p[1][1].abs(), p[0][0] + p[1][0].abs(), p[0][1] + p[1][1].abs()) }
        else { p.iter().map(|q| Rect::from_points(Point::new(q[0], q[1]), Point::new(q[0], q[1]))).reduce(|a, b| a.union(b)).unwrap_or(Rect::ZERO) };
    let grow = (l.s.width / 2.0 + 1.0).max(if l.s.tool == Tool::Arrow { head(l.s.width) + 1.0 } else { 0.0 });
    let r = r.inflate(grow, grow); Rect::new(r.x0.floor(), r.y0.floor(), r.x1.ceil(), r.y1.ceil())
}
/// The element's box and record for a stroke.
pub fn write(l: &Line) -> (Rect, String) {
    let b = bounds(l); let (w, h) = (b.width().max(1.0), b.height().max(1.0)); let cut = |x: f64| (x * 1e6).round() / 1e6;
    let mut s = l.s.clone(); let oval = s.tool == Tool::Oval;
    for (i, p) in s.points.iter_mut().enumerate() { *p = if oval && i == 1 { [cut(p[0] / w), cut(p[1] / h)] } else { [cut((p[0] - b.x0) / w), cut((p[1] - b.y0) / h)] }; }
    let mut shape = s.to_json();
    if let (Some(p), Some(o)) = (&l.pressures, shape.as_object_mut()) { o.insert("pressures".into(), Value::Array(p.iter().map(|x| json!((x.clamp(0.0, 1.0) * 1000.0).round() / 1000.0)).collect())); }
    (b, json!({ "version": "2.0", "coordinate_system": "normalized", "shapes": [shape] }).to_string())
}

/// What a stroke paints as, in the sheet's units from `origin` (its element's corner, so a moved element is drawn from
/// the same picture): lines stroked at `width` with round ends, and shapes filled (an arrow's head; a pressed line's body).
pub struct Drawn { pub lines: Vec<BezPath>, pub fills: Vec<BezPath>, pub width: f64, pub color: peniko::Color }
impl Drawn {
    pub fn paint(&self, scene: &mut vello::Scene, at: kurbo::Affine) {
        let st = kurbo::Stroke::new(self.width).with_caps(Cap::Round).with_join(Join::Round);
        for p in &self.lines { scene.stroke(&st, at, self.color, None, p); }
        for p in &self.fills { scene.fill(peniko::Fill::NonZero, at, self.color, None, p); }
    }
}
fn half(w: f64, p: f64) -> f64 { w / 2.0 * (LIGHT + (1.0 - LIGHT) * p.clamp(0.0, 1.0)) }
/// A pressed line's body: a disc at each point and the band between each two, all wound the same way (so they fill as one).
fn body(pts: &[[f64; 2]], pr: &[f64], w: f64, o: Vec2) -> BezPath {
    let mut out = BezPath::new(); let at = |i: usize| Point::new(pts[i][0], pts[i][1]) - o;
    for i in 0..pts.len() {
        let (a, ha) = (at(i), half(w, pr[i]).max(0.05)); out.extend(Circle::new(a, ha).to_path(0.02));
        if i + 1 < pts.len() { let (b, hb) = (at(i + 1), half(w, pr[i + 1]).max(0.05)); let d = b - a; let len = d.hypot(); if len < 1e-9 { continue }
            let n = Vec2::new(-d.y, d.x) / len; out.move_to(a - n * ha); out.line_to(b - n * hb); out.line_to(b + n * hb); out.line_to(a + n * ha); out.close_path(); }
    }
    out
}
pub fn drawn(l: &Line, origin: Point) -> Drawn {
    let c = l.s.color; let color = peniko::Color::new([c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32]);
    if let (Tool::Freehand, Some(pr)) = (l.s.tool, &l.pressures) { if pr.len() == l.s.points.len() { return Drawn { lines: vec![], fills: vec![body(&l.s.points, pr, l.s.width, origin.to_vec2())], width: l.s.width, color } } }
    let g = qcv_notes::ink::geometry(&l.s, [-origin.x, -origin.y, 1.0, 1.0], 1.0);
    Drawn { lines: g.paths, fills: g.head.into_iter().collect(), width: g.width, color }
}

fn seg_dist(p: Point, a: Point, b: Point) -> f64 { let d = b - a; let l2 = d.hypot2(); if l2 < 1e-18 { return (p - a).hypot() } let t = ((p - a).dot(d) / l2).clamp(0.0, 1.0); (p - (a + d * t)).hypot() }
fn pt(q: [f64; 2]) -> Point { Point::new(q[0], q[1]) }
/// A point is on the drawn line (within `slack` of its edge) - not merely in its box.
pub fn hit(l: &Line, p: Point, slack: f64) -> bool {
    let (q, reach) = (&l.s.points, l.s.width / 2.0 + slack);
    match l.s.tool {
        Tool::Freehand => if q.len() == 1 { (p - pt(q[0])).hypot() <= reach } else { q.windows(2).any(|w| seg_dist(p, pt(w[0]), pt(w[1])) <= reach) },
        Tool::Line | Tool::Arrow if q.len() >= 2 => seg_dist(p, pt(q[0]), pt(q[1])) <= reach.max(if l.s.tool == Tool::Arrow && (p - pt(q[1])).hypot() < 12.0 + 2.0 * l.s.width { head(l.s.width) } else { 0.0 }),
        Tool::Rectangle if q.len() >= 3 => { let r = Rect::from_points(pt(q[0]), pt(q[2])); if l.s.filled && r.contains(p) { return true }
            let c = [Point::new(r.x0, r.y0), Point::new(r.x1, r.y0), Point::new(r.x1, r.y1), Point::new(r.x0, r.y1)]; (0..4).any(|i| seg_dist(p, c[i], c[(i + 1) % 4]) <= reach) }
        Tool::Oval if q.len() >= 2 => { let (rx, ry) = (q[1][0].abs().max(1e-6), q[1][1].abs().max(1e-6)); let d = p - pt(q[0]); let k = (d.x / rx).hypot(d.y / ry);
            if l.s.filled && k <= 1.0 { return true } if k < 1e-9 { return rx.min(ry) <= reach } (d.hypot() * (1.0 - 1.0 / k)).abs() <= reach }
        _ => false,
    }
}

/// Fewer points for the same line: none is dropped that stands more than `tol` off the line through its neighbours
/// that stay (a pressure's change counts as a distance too, so a swelling line keeps its points).
pub fn reduce(pts: &[[f64; 2]], pr: Option<&[f64]>, w: f64, tol: f64) -> (Vec<[f64; 2]>, Option<Vec<f64>>) {
    let n = pts.len(); if n <= 2 { return (pts.to_vec(), pr.map(|p| p.to_vec())) }
    let z = |i: usize| pr.map(|p| half(w, p[i])).unwrap_or(0.0);
    let mut keep = vec![false; n]; keep[0] = true; keep[n - 1] = true; let mut todo = vec![(0usize, n - 1)];
    while let Some((a, b)) = todo.pop() { if b <= a + 1 { continue }
        let (pa, pb, za, zb) = (pt(pts[a]), pt(pts[b]), z(a), z(b)); let d = pb - pa; let l2 = d.hypot2() + (zb - za) * (zb - za);
        let (mut far, mut at) = (0.0f64, a);
        for i in a + 1..b { let v = pt(pts[i]) - pa; let vz = z(i) - za; let t = if l2 < 1e-18 { 0.0 } else { ((v.dot(d) + vz * (zb - za)) / l2).clamp(0.0, 1.0) }; let e = ((v - d * t).hypot2() + (vz - (zb - za) * t).powi(2)).sqrt(); if e > far { far = e; at = i; } }
        if far > tol { keep[at] = true; todo.push((a, at)); todo.push((at, b)); } }
    ((0..n).filter(|i| keep[*i]).map(|i| pts[i]).collect(), pr.map(|p| (0..n).filter(|i| keep[*i]).map(|i| p[i]).collect()))
}

/// The eraser, a disc of radius `r`, passes from `a` to `b`. None: it did not touch the stroke. Else what is left of it:
/// a freehand line loses the part passed over and what stays are lines of their own (new ids); a shape goes whole.
pub fn rub(l: &Line, a: Point, b: Point, r: f64) -> Option<Vec<Line>> {
    if bounds(l).inflate(r, r).intersect(Rect::from_points(a, b).inflate(r, r)).area() <= 0.0 { return None }
    if l.s.tool != Tool::Freehand { let n = ((b - a).hypot() / (r / 2.0).max(0.25)).ceil().max(1.0) as usize; return (0..=n).any(|i| hit(l, a + (b - a) * (i as f64 / n as f64), r - l.s.width / 2.0)).then(Vec::new) }
    let reach = r.max(l.s.width / 4.0); let gone = |p: Point| seg_dist(p, a, b) < reach;
    // points close enough together that no stretch of the line can cross the eraser between two of them
    let step = (reach / 2.0).max(0.25); let (src, spr) = (&l.s.points, l.pressures.as_deref());
    let mut pts: Vec<(Point, f64)> = vec![]; for i in 0..src.len() { let (p, z) = (pt(src[i]), spr.map(|x| x[i]).unwrap_or(1.0));
        if let Some(&(q, zq)) = pts.last() { let n = ((p - q).hypot() / step).ceil() as usize; for k in 1..n { let t = k as f64 / n as f64; pts.push((q + (p - q) * t, zq + (z - zq) * t)); } } pts.push((p, z)); }
    if !pts.iter().any(|(p, _)| gone(*p)) { return None }
    // where the line leaves the eraser, between a point that went and one that stays
    let edge = |inside: (Point, f64), outside: (Point, f64)| { let (mut lo, mut hi) = (0.0f64, 1.0f64); for _ in 0..10 { let m = (lo + hi) / 2.0; if gone(inside.0 + (outside.0 - inside.0) * m) { lo = m } else { hi = m } } (inside.0 + (outside.0 - inside.0) * hi, inside.1 + (outside.1 - inside.1) * hi) };
    let mut runs: Vec<Vec<(Point, f64)>> = vec![]; let mut cur: Vec<(Point, f64)> = vec![];
    for i in 0..pts.len() { if gone(pts[i].0) { if !cur.is_empty() { cur.push(edge(pts[i], pts[i - 1])); runs.push(std::mem::take(&mut cur)); } }
        else { if cur.is_empty() && i > 0 { cur.push(edge(pts[i - 1], pts[i])); } cur.push(pts[i]); } }
    if !cur.is_empty() { runs.push(cur); }
    Some(runs.into_iter().filter(|run| run.windows(2).map(|w| (w[1].0 - w[0].0).hypot()).sum::<f64>() > 0.5).map(|run| {
        let p: Vec<[f64; 2]> = run.iter().map(|(p, _)| [p.x, p.y]).collect(); let z: Vec<f64> = run.iter().map(|(_, z)| *z).collect();
        let (p, z) = reduce(&p, spr.map(|_| &z[..]), l.s.width, KEEP);
        Line { s: Stroke { id: strokes::new_id(), points: p, ..l.s.clone() }, pressures: z } }).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn line(tool: Tool, pts: Vec<[f64; 2]>, pr: Option<Vec<f64>>) -> Line { Line { s: Stroke { id: "s".into(), tool, color: [1.0, 0.0, 0.0, 1.0], width: 6.0, filled: false, is_modeled: tool == Tool::Freehand, points: pts }, pressures: pr } }
    fn el(l: &Line) -> Element { let (b, record) = write(l); Element { id: mn_doc::BlockId::new(), kind: "ink".into(), x: b.x0, y: b.y0, w: b.width(), h: b.height(), z: 1, parent: None, record } }
    fn near(a: &Line, b: &Line) -> bool { a.s.points.len() == b.s.points.len() && a.s.points.iter().zip(&b.s.points).all(|(p, q)| (p[0] - q[0]).abs() < 0.01 && (p[1] - q[1]).abs() < 0.01) }
    #[test]
    fn a_stroke_is_kept_as_qcviews_shape_and_read_back_where_it_was() {
        let free = line(Tool::Freehand, vec![[100.0, 200.0], [160.0, 240.0], [300.0, 210.0]], Some(vec![0.1, 1.0, 0.4]));
        let e = el(&free); assert_eq!((e.x, e.y, e.w, e.h), (96.0, 196.0, 208.0, 48.0), "the points' box, grown by half the width and one");
        // QCView's reader takes the record as it is: the same shape, the pressure passed over
        let v: Value = serde_json::from_str(&e.record).unwrap(); let q = strokes::parse(Some(&v)); assert_eq!(q.len(), 1); assert_eq!((q[0].tool, q[0].width, q[0].points.len()), (Tool::Freehand, 6.0, 3)); assert!(q[0].points.iter().all(|p| (0.0..=1.0).contains(&p[0]) && (0.0..=1.0).contains(&p[1])));
        let back = read(&e).unwrap(); assert!(near(&back, &free)); assert_eq!(back.pressures, Some(vec![0.1, 1.0, 0.4]));
        // an oval is its centre and its radii; a moved or resized element moves and stretches what it holds
        let oval = line(Tool::Oval, vec![[500.0, 500.0], [80.0, 40.0]], None); let mut e = el(&oval); assert!(near(&read(&e).unwrap(), &oval));
        e.x += 100.0; e.w *= 2.0; let moved = read(&e).unwrap(); assert!((moved.s.points[1][0] - 160.0).abs() < 0.01 && (moved.s.points[1][1] - 40.0).abs() < 0.01, "{:?}", moved.s.points);
        let arrow = line(Tool::Arrow, vec![[0.0, 0.0], [100.0, 0.0]], None); assert!(bounds(&arrow).y1 >= head(6.0), "an arrow's box holds its head");
    }
    #[test]
    fn a_point_is_on_the_line_not_merely_in_its_box() {
        let diag = line(Tool::Line, vec![[0.0, 0.0], [200.0, 200.0]], None);
        assert!(hit(&diag, Point::new(100.0, 101.0), 2.0)); assert!(!hit(&diag, Point::new(180.0, 20.0), 2.0), "the box's empty corner is not the line");
        let rect = line(Tool::Rectangle, vec![[0.0, 0.0], [100.0, 0.0], [100.0, 60.0], [0.0, 60.0]], None); assert!(hit(&rect, Point::new(100.0, 30.0), 2.0)); assert!(!hit(&rect, Point::new(50.0, 30.0), 2.0), "an outline's middle is empty");
        let oval = line(Tool::Oval, vec![[0.0, 0.0], [100.0, 50.0]], None); assert!(hit(&oval, Point::new(100.0, 0.0), 2.0) && hit(&oval, Point::new(0.0, -50.0), 2.0)); assert!(!hit(&oval, Point::new(0.0, 0.0), 2.0));
    }
    #[test]
    fn the_eraser_takes_the_part_it_passes_over() {
        let pts: Vec<[f64; 2]> = (0..=100).map(|i| [i as f64 * 4.0, 50.0]).collect(); let pr: Vec<f64> = (0..=100).map(|i| i as f64 / 100.0).collect();
        let l = line(Tool::Freehand, pts, Some(pr));
        assert!(rub(&l, Point::new(200.0, 300.0), Point::new(210.0, 300.0), 5.0).is_none(), "elsewhere: untouched");
        // across the middle: two lines, ending 5 either side of the crossing, each its own stroke, the pressure kept along them
        let left = rub(&l, Point::new(200.0, 0.0), Point::new(200.0, 100.0), 5.0).unwrap(); assert_eq!(left.len(), 2);
        let (a, b) = (&left[0], &left[1]); assert!((a.s.points.last().unwrap()[0] - 195.0).abs() < 0.1 && (b.s.points[0][0] - 205.0).abs() < 0.1, "{:?} {:?}", a.s.points.last(), b.s.points[0]);
        assert_eq!(a.s.points[0], [0.0, 50.0]); assert_eq!(b.s.points.last().unwrap(), &[400.0, 50.0]); assert_ne!(a.s.id, b.s.id); assert_ne!(a.s.id, l.s.id);
        let (pa, pb) = (a.pressures.as_ref().unwrap(), b.pressures.as_ref().unwrap()); assert!(pa[0] < 0.01 && (pa.last().unwrap() - 0.4875).abs() < 0.01 && (pb[0] - 0.5125).abs() < 0.01 && pb.last().unwrap() > &0.99, "{pa:?} {pb:?}");
        // a straight stretch between two far points is still cut (no point of it lies under the eraser)
        let long = line(Tool::Freehand, vec![[0.0, 0.0], [1000.0, 0.0]], None); let cut = rub(&long, Point::new(500.0, -10.0), Point::new(500.0, 10.0), 3.0).unwrap(); assert_eq!(cut.len(), 2); assert!(cut[0].s.points.len() == 2 && (cut[0].s.points[1][0] - 497.0).abs() < 0.1);
        // along the whole of it: nothing is left; a shape goes whole when its outline is touched, and only then
        assert!(rub(&long, Point::new(-5.0, 0.0), Point::new(1005.0, 0.0), 3.0).unwrap().is_empty());
        let rect = line(Tool::Rectangle, vec![[0.0, 0.0], [100.0, 0.0], [100.0, 60.0], [0.0, 60.0]], None);
        assert!(rub(&rect, Point::new(40.0, 30.0), Point::new(60.0, 30.0), 5.0).is_none()); assert_eq!(rub(&rect, Point::new(90.0, 30.0), Point::new(110.0, 30.0), 5.0), Some(vec![]));
    }
    #[test]
    fn a_line_is_kept_with_fewer_points_and_a_pressed_line_fills_as_one() {
        // a straight line sampled 300 times is its two ends; a swelling one keeps where the swell turns
        let pts: Vec<[f64; 2]> = (0..300).map(|i| [i as f64, 10.0]).collect(); assert_eq!(reduce(&pts, None, 6.0, KEEP).0.len(), 2);
        let pr: Vec<f64> = (0..300).map(|i| if i < 150 { i as f64 / 150.0 } else { 1.0 }).collect(); let (p, z) = reduce(&pts, Some(&pr), 6.0, KEEP); assert_eq!(p.len(), 3); assert_eq!(z.unwrap(), vec![0.0, 1.0, 1.0]);
        let arc: Vec<[f64; 2]> = (0..=300).map(|i| { let a = i as f64 / 300.0 * std::f64::consts::PI; [100.0 * a.cos(), 100.0 * a.sin()] }).collect(); let (p, _) = reduce(&arc, None, 6.0, KEEP); assert!(p.len() > 12 && p.len() < 100, "{}", p.len());
        // the discs and the bands of a pressed line turn the same way round: where they overlap they fill, they do not cut holes
        let b = body(&[[0.0, 0.0], [50.0, 0.0]], &[1.0, 1.0], 6.0, Vec2::ZERO); let disc = Circle::new((0.0, 0.0), 3.0).to_path(0.02).area(); let band = b.area() - 2.0 * disc;
        assert!(disc.signum() == band.signum() && (band.abs() - 300.0).abs() < 1.0, "disc {disc}, band {band}");
        let d = drawn(&line(Tool::Freehand, vec![[10.0, 10.0], [60.0, 10.0]], Some(vec![0.0, 1.0])), Point::new(10.0, 10.0)); assert!(d.lines.is_empty() && d.fills.len() == 1); let bb = d.fills[0].bounding_box(); assert!((bb.x0 + 0.75).abs() < 0.01 && (bb.x1 - 53.0).abs() < 0.01, "light at the start, full at the end, from the element's corner: {bb:?}");
    }
}
