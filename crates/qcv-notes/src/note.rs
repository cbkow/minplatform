//! QCView `AnnotationNote` + `noteToJson` / `noteFromJson`: one note per key (SMPTE "HH:MM:SS:FF", or a live capture stamp).
use serde_json::{Map, Value};

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Note {
    pub timecode: String, pub timestamp_seconds: f64, pub frame: i64,
    /// relative, "images/note_HH_MM_SS_FF.png"
    pub image: String,
    pub viewer: String, pub ocio: String, pub captured_at: String, pub host_timecode: String, pub stream: String,
    /// `annotation_data` as stored (an object, an array, or None = null) — kept as JSON so a load / save round-trips exactly
    pub annotation: Option<Value>,
    pub text: String, pub addressed: bool,
}

impl Note {
    pub fn to_json(&self) -> Value {
        let mut o = Map::new();
        o.insert("timecode".into(), Value::from(self.timecode.clone()));
        o.insert("timestamp_seconds".into(), num(self.timestamp_seconds));
        o.insert("frame".into(), Value::from(self.frame));
        o.insert("image".into(), Value::from(self.image.clone()));
        for (k, v) in [("viewer", &self.viewer), ("ocio", &self.ocio), ("captured_at", &self.captured_at), ("host_timecode", &self.host_timecode), ("stream", &self.stream)] { if !v.is_empty() { o.insert(k.into(), Value::from(v.clone())); } }
        o.insert("annotation_data".into(), match &self.annotation { Some(v @ (Value::Object(_) | Value::Array(_))) => v.clone(), _ => Value::Null });
        o.insert("text".into(), Value::from(self.text.clone()));
        o.insert("addressed".into(), Value::from(self.addressed));
        Value::Object(o)
    }
    /// QCView's required four: timecode, timestamp_seconds, frame, image.
    pub fn from_json(o: &Map<String, Value>) -> Option<Note> {
        for k in ["timecode", "timestamp_seconds", "frame", "image"] { if !o.contains_key(k) { return None } }
        let s = |k: &str| o.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        Some(Note {
            timecode: s("timecode"), timestamp_seconds: o.get("timestamp_seconds").and_then(Value::as_f64).unwrap_or(0.0),
            frame: o.get("frame").and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))).unwrap_or(0),
            image: s("image"), viewer: s("viewer"), ocio: s("ocio"), captured_at: s("captured_at"), host_timecode: s("host_timecode"), stream: s("stream"),
            annotation: match o.get("annotation_data") { Some(v @ (Value::Object(_) | Value::Array(_))) => Some(v.clone()), _ => None },
            text: s("text"), addressed: o.get("addressed").and_then(Value::as_bool).unwrap_or(false),
        })
    }
    /// The note carries strokes (QCView `hasStrokes`).
    pub fn has_strokes(&self) -> bool { self.annotation.as_ref().and_then(|a| a.get("shapes")).and_then(Value::as_array).is_some_and(|s| !s.is_empty()) }
}
/// A double as QJsonValue holds it (written in Qt's form by `qt_json`; an integral value prints as an integer either way).
pub(crate) fn num(v: f64) -> Value { serde_json::Number::from_f64(v).map(Value::Number).unwrap_or(Value::Null) }
