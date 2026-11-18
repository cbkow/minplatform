//! QCView `AnnotationSerializer`: `annotation_data` v2.0 — `{"version":"2.0","coordinate_system":"normalized","shapes":[…]}`, each
//! shape `{id, type, color [r,g,b,a], stroke_width, filled, is_modeled, points [[x,y],…]}`, points normalized to the displayed
//! picture (post-rotation; outside 0..1 allowed). v1.0 loads with `is_modeled = false`; an unknown version or coordinate system
//! loads empty; missing colour = red, missing width = 2.5 (QCView's defaults). Ids are KEPT (row 215; QCView reminted them).
use crate::note::num;
use serde_json::{json, Map, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool { Freehand, Rectangle, Oval, Arrow, Line }
impl Tool {
    pub fn name(self) -> &'static str { match self { Tool::Freehand => "freehand", Tool::Rectangle => "rect", Tool::Oval => "oval", Tool::Arrow => "arrow", Tool::Line => "line" } }
    pub fn parse(s: &str) -> Option<Tool> { Some(match s { "freehand" => Tool::Freehand, "rect" => Tool::Rectangle, "oval" => Tool::Oval, "arrow" => Tool::Arrow, "line" => Tool::Line, _ => return None }) }
}

/// One anchor of a pen path: the point, and the two handles as POINTS (the one the curve arrives by, the one it leaves by);
/// a handle on its anchor = a corner on that side. Normalized, as every point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor { pub p: [f64; 2], pub hin: [f64; 2], pub hout: [f64; 2] }
impl Anchor { pub fn corner(p: [f64; 2]) -> Anchor { Anchor { p, hin: p, hout: p } } }
/// A path drawn with the pen (the user 2026-11-18, canon row 566): cubic pieces from anchor to anchor, open or closed.
/// It rides on a `freehand` shape - `points` hold the curve already flattened, so a QCView or minNotes that does not know
/// `path` still draws the same line (and, saving the note again, keeps it as that plain line) - under the added key
/// `"path": {"closed": bool, "anchors": [[x, y, in_x, in_y, out_x, out_y], …]}`.
#[derive(Clone, Debug, PartialEq)]
pub struct PenPath { pub anchors: Vec<Anchor>, pub closed: bool }
impl PenPath {
    /// The curve through `map` (the picture's place on a target): the same shape at every size.
    pub fn bez(&self, map: impl Fn([f64; 2]) -> kurbo::Point) -> kurbo::BezPath {
        let mut b = kurbo::BezPath::new(); let a = &self.anchors; if a.is_empty() { return b }
        b.move_to(map(a[0].p));
        for w in a.windows(2) { b.curve_to(map(w[0].hout), map(w[1].hin), map(w[1].p)); }
        if self.closed && a.len() > 1 { let (l, f) = (a[a.len() - 1], a[0]); b.curve_to(map(l.hout), map(f.hin), map(f.p)); b.close_path(); }
        b
    }
    /// The curve as the plain line an older reader draws: flattened in a 4000-unit picture to a tenth of a unit.
    pub fn flatten(&self) -> Vec<[f64; 2]> {
        const U: f64 = 4000.0; let mut out = vec![];
        kurbo::flatten(self.bez(|p| kurbo::Point::new(p[0] * U, p[1] * U)), 0.1, |el| if let kurbo::PathEl::MoveTo(p) | kurbo::PathEl::LineTo(p) = el { out.push([p.x / U, p.y / U]) });
        if out.len() == 1 { out.push(out[0]); }
        out
    }
    /// Every point and handle through `f` (a move, a scale).
    pub fn map(&mut self, f: impl Fn([f64; 2]) -> [f64; 2]) { for a in &mut self.anchors { a.p = f(a.p); a.hin = f(a.hin); a.hout = f(a.hout); } }
    fn to_json(&self) -> Value { json!({ "closed": self.closed, "anchors": self.anchors.iter().map(|a| Value::Array([a.p[0], a.p[1], a.hin[0], a.hin[1], a.hout[0], a.hout[1]].iter().map(|v| num(*v)).collect())).collect::<Vec<_>>() }) }
    fn from_json(v: &Value) -> Option<PenPath> {
        let o = v.as_object()?;
        let anchors: Vec<Anchor> = o.get("anchors")?.as_array()?.iter().filter_map(|a| { let a = a.as_array()?; if a.len() < 6 { return None } let n: Vec<f64> = a.iter().take(6).filter_map(Value::as_f64).collect(); (n.len() == 6).then(|| Anchor { p: [n[0], n[1]], hin: [n[2], n[3]], hout: [n[4], n[5]] }) }).collect();
        (anchors.len() >= 2).then(|| PenPath { anchors, closed: o.get("closed").and_then(Value::as_bool).unwrap_or(false) })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stroke { pub id: String, pub tool: Tool, pub color: [f64; 4], pub width: f64, pub filled: bool, pub is_modeled: bool, pub points: Vec<[f64; 2]>, /// a pen path's anchors (a `freehand` whose `points` are its curve flattened); None = every other stroke
    pub path: Option<PenPath> }

impl Stroke {
    pub fn to_json(&self) -> Value {
        let mut o = Map::new();
        o.insert("id".into(), Value::from(self.id.clone()));
        o.insert("type".into(), Value::from(self.tool.name()));
        o.insert("color".into(), Value::Array(self.color.iter().map(|c| num(*c)).collect()));
        o.insert("stroke_width".into(), num(self.width));
        o.insert("filled".into(), Value::from(self.filled));
        o.insert("is_modeled".into(), Value::from(self.is_modeled));
        o.insert("points".into(), Value::Array(self.points.iter().map(|p| Value::Array(vec![num(p[0]), num(p[1])])).collect()));
        if let Some(p) = &self.path { o.insert("path".into(), p.to_json()); }   // (absent for every other stroke: their JSON is QCView's, byte for byte)
        Value::Object(o)
    }
    fn from_json(o: &Map<String, Value>, v2: bool, idx: usize) -> Option<Stroke> {
        let tool = Tool::parse(o.get("type")?.as_str()?)?;
        let color = o.get("color").and_then(Value::as_array).filter(|c| c.len() >= 4).map(|c| [0, 1, 2, 3].map(|i| c[i].as_f64().unwrap_or(0.0))).unwrap_or([1.0, 0.0, 0.0, 1.0]);
        let width = o.get("stroke_width").and_then(Value::as_f64).unwrap_or(2.5);
        let points: Vec<[f64; 2]> = o.get("points")?.as_array()?.iter().filter_map(|p| { let a = p.as_array()?; if a.len() < 2 { return None } Some([a[0].as_f64()?, a[1].as_f64()?]) }).collect();
        if points.is_empty() { return None }
        let id = o.get("id").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| format!("stroke-loaded-{idx}"));
        Some(Stroke { id, tool, color, width, filled: o.get("filled").and_then(Value::as_bool).unwrap_or(false), is_modeled: v2 && o.get("is_modeled").and_then(Value::as_bool).unwrap_or(false), points, path: if tool == Tool::Freehand { o.get("path").and_then(PenPath::from_json) } else { None } })
    }
    /// QCView's colour as QColor keeps it: 16 bits per channel, read back as a float (`redF()`), widened to the JSON double.
    pub fn qcolor(rgba: [f32; 4]) -> [f64; 4] { rgba.map(|c| { let q = (c.clamp(0.0, 1.0) * 65535.0).round() as u16; (q as f32 / 65535.0) as f64 }) }
}

/// `annotation_data` → strokes (empty on anything QCView would not load).
pub fn parse(data: Option<&Value>) -> Vec<Stroke> {
    let Some(root) = data.and_then(Value::as_object) else { return vec![] };
    let version = root.get("version").and_then(Value::as_str).unwrap_or("1.0");
    if version != "1.0" && version != "2.0" { return vec![] }
    if let Some(cs) = root.get("coordinate_system") { if cs.as_str() != Some("normalized") { return vec![] } }
    let Some(shapes) = root.get("shapes").and_then(Value::as_array) else { return vec![] };
    shapes.iter().enumerate().filter_map(|(i, s)| Stroke::from_json(s.as_object()?, version == "2.0", i)).collect()
}
/// Strokes → `annotation_data` (QCView `strokesToJsonString`: the object even when every stroke was erased; a note that never
/// had a drawing keeps `annotation: None` → `null`, the caller's choice).
pub fn to_data(strokes: &[Stroke]) -> Value {
    json!({ "version": "2.0", "coordinate_system": "normalized", "shapes": strokes.iter().map(Stroke::to_json).collect::<Vec<_>>() })
}
/// QCView `generateStrokeId`: "stroke-<ms since the epoch>-<1000..9999>".
/// (Two ids asked for in the same millisecond differ: a count of the ids given stands in for QCView's random draw - without
/// it the second stroke of a burst took the first one's id.)
pub fn new_id() -> String {
    static GIVEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let n = GIVEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let r = 1000 + (ms as u64).wrapping_mul(6364136223846793005).wrapping_add(std::process::id() as u64).wrapping_add(n) % 9000;
    format!("stroke-{ms}-{r}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_pen_path_rides_on_a_freehand_shape() {
        use super::*;
        let path = PenPath { anchors: vec![Anchor { p: [0.2, 0.5], hin: [0.2, 0.5], hout: [0.3, 0.2] }, Anchor { p: [0.6, 0.5], hin: [0.5, 0.2], hout: [0.7, 0.8] }, Anchor::corner([0.8, 0.4])], closed: false };
        let s = Stroke { id: "p".into(), tool: Tool::Freehand, color: [1.0, 0.0, 0.0, 1.0], width: 4.0, filled: false, is_modeled: true, points: path.flatten(), path: Some(path.clone()) };
        let data = to_data(std::slice::from_ref(&s));
        assert_eq!(data["shapes"][0]["type"], "freehand", "an older reader sees a freehand line");
        assert!(s.points.len() > 20 && s.points[0] == [0.2, 0.5] && (s.points.last().unwrap()[0] - 0.8).abs() < 1e-9, "and its points are the curve: {}", s.points.len());
        assert_eq!(parse(Some(&data)), vec![s.clone()], "the path comes back whole");
        // a plain stroke's JSON has no such key
        let plain = Stroke { path: None, ..s }; assert!(to_data(&[plain])["shapes"][0].get("path").is_none());
    }
    #[test]
    fn ids_given_in_one_burst_differ() { let ids: Vec<String> = (0..500).map(|_| super::new_id()).collect(); let mut u = ids.clone(); u.sort(); u.dedup(); assert_eq!(u.len(), ids.len()); assert!(ids[0].starts_with("stroke-") && ids[0].rsplit('-').next().unwrap().len() == 4); }
}
