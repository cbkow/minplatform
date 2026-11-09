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

#[derive(Clone, Debug, PartialEq)]
pub struct Stroke { pub id: String, pub tool: Tool, pub color: [f64; 4], pub width: f64, pub filled: bool, pub is_modeled: bool, pub points: Vec<[f64; 2]> }

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
        Value::Object(o)
    }
    fn from_json(o: &Map<String, Value>, v2: bool, idx: usize) -> Option<Stroke> {
        let tool = Tool::parse(o.get("type")?.as_str()?)?;
        let color = o.get("color").and_then(Value::as_array).filter(|c| c.len() >= 4).map(|c| [0, 1, 2, 3].map(|i| c[i].as_f64().unwrap_or(0.0))).unwrap_or([1.0, 0.0, 0.0, 1.0]);
        let width = o.get("stroke_width").and_then(Value::as_f64).unwrap_or(2.5);
        let points: Vec<[f64; 2]> = o.get("points")?.as_array()?.iter().filter_map(|p| { let a = p.as_array()?; if a.len() < 2 { return None } Some([a[0].as_f64()?, a[1].as_f64()?]) }).collect();
        if points.is_empty() { return None }
        let id = o.get("id").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| format!("stroke-loaded-{idx}"));
        Some(Stroke { id, tool, color, width, filled: o.get("filled").and_then(Value::as_bool).unwrap_or(false), is_modeled: v2 && o.get("is_modeled").and_then(Value::as_bool).unwrap_or(false), points })
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
pub fn new_id() -> String {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let r = 1000 + (ms as u64).wrapping_mul(6364136223846793005).wrapping_add(std::process::id() as u64) % 9000;
    format!("stroke-{ms}-{r}")
}
