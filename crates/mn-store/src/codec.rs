//! A block's kind, attributes and formatting as the file's columns: a kind name, an attrs JSON object (NULL when
//! empty) and the text. Formatting offsets are UTF-16 units in the file (row 283, ruling 3; the 2.0 span form).
use serde_json::{json, Map, Value};
use mn_doc::{Block, ChoiceOption, ColumnKind, ColumnSpec, Kind, Spans};

fn s(m: &Map<String, Value>, k: &str) -> String { m.get(k).and_then(Value::as_str).unwrap_or("").to_string() }
fn n(m: &Map<String, Value>, k: &str) -> i64 { m.get(k).and_then(Value::as_i64).unwrap_or(0) }
fn put(m: &mut Map<String, Value>, k: &str, v: &str) { if !v.is_empty() { m.insert(k.into(), json!(v)); } }

/// (kind name, attrs JSON or None, the text column).
pub fn encode(b: &Block) -> (&'static str, Option<String>, String) {
    let mut a = Map::new(); let mut text = b.text.clone();
    let name = match &b.kind {
        Kind::Paragraph => "paragraph",
        Kind::Heading { level } => { a.insert("level".into(), json!(level)); "heading" }
        Kind::Quote => "quote",
        Kind::Code { lang } => { put(&mut a, "lang", lang); "code" }
        Kind::Bullet { depth } => { a.insert("depth".into(), json!(depth)); "bullet" }
        Kind::Numbered { depth } => { a.insert("depth".into(), json!(depth)); "numbered" }
        Kind::Task { depth, state } => { a.insert("depth".into(), json!(depth)); a.insert("state".into(), json!(state)); "task" }
        Kind::Divider => "divider",
        Kind::Media { descriptor } => { text = descriptor.clone(); "media" }
        Kind::Columns => "columns",
        Kind::Column { width } => { a.insert("width".into(), json!((*width as f64 * 10000.0).round() / 10000.0)); "column" }
        Kind::Table { header_rows, columns } => {
            a.insert("header".into(), json!(header_rows));
            a.insert("cols".into(), Value::Array(columns.iter().map(|c| {
                let mut o = Map::new();
                if let Some(w) = c.width { o.insert("w".into(), json!(w)); }
                if c.align > 0 { o.insert("a".into(), json!(c.align)); }
                put(&mut o, "bg", &c.bg); put(&mut o, "fg", &c.fg);
                match &c.kind {
                    ColumnKind::Text => {}
                    ColumnKind::Choice { options } => { o.insert("k".into(), json!(1)); o.insert("o".into(), Value::Array(options.iter().map(|x| json!({"id": x.id, "l": x.label, "c": x.color})).collect())); }
                    ColumnKind::Check => { o.insert("k".into(), json!(2)); }
                    ColumnKind::Timecode { fps } => { o.insert("k".into(), json!(3)); o.insert("fps".into(), json!(fps)); }
                }
                Value::Object(o)
            }).collect()));
            "table"
        }
        Kind::Row { bg, fg } => { put(&mut a, "bg", bg); put(&mut a, "fg", fg); "row" }
        Kind::Cell { bg, fg } => { put(&mut a, "bg", bg); put(&mut a, "fg", fg); "cell" }
    };
    if !b.spans.is_empty() {
        a.insert("spans".into(), Value::Array(b.spans.to_utf16(&b.text).into_iter().map(|(start, end, k, u)| {
            let mut o = Map::new(); o.insert("s".into(), json!(start)); o.insert("e".into(), json!(end)); o.insert("k".into(), json!(k));
            if let Some(u) = u { o.insert("u".into(), json!(u)); }
            Value::Object(o)
        }).collect()));
    }
    (name, (!a.is_empty()).then(|| Value::Object(a).to_string()), text)
}

/// The inverse; an unknown kind name reads as a paragraph (a file from a later build still opens what it can).
pub fn decode(name: &str, attrs: Option<&str>, text: String) -> (Kind, String, Spans) {
    let a = attrs.and_then(|x| serde_json::from_str::<Value>(x).ok()).and_then(|v| match v { Value::Object(m) => Some(m), _ => None }).unwrap_or_default();
    let depth = n(&a, "depth").clamp(0, 8) as u8;
    let kind = match name {
        "heading" => Kind::Heading { level: n(&a, "level").clamp(1, 6) as u8 },
        "quote" => Kind::Quote,
        "code" => Kind::Code { lang: s(&a, "lang") },
        "bullet" => Kind::Bullet { depth },
        "numbered" => Kind::Numbered { depth },
        "task" => Kind::Task { depth, state: n(&a, "state").clamp(0, 2) as u8 },
        "divider" => Kind::Divider,
        "media" => Kind::Media { descriptor: text.clone() },
        "columns" => Kind::Columns,
        "column" => Kind::Column { width: a.get("width").and_then(Value::as_f64).unwrap_or(0.0) as f32 },
        "table" => Kind::Table { header_rows: n(&a, "header").clamp(0, 255) as u8, columns: a.get("cols").and_then(Value::as_array).map(|v| v.iter().map(|c| {
            let Some(o) = c.as_object() else { return ColumnSpec::default() };
            ColumnSpec { width: o.get("w").and_then(Value::as_u64).map(|w| w as u32), align: n(o, "a").clamp(0, 2) as u8, bg: s(o, "bg"), fg: s(o, "fg"), kind: match n(o, "k") {
                1 => ColumnKind::Choice { options: o.get("o").and_then(Value::as_array).map(|v| v.iter().filter_map(Value::as_object).map(|x| ChoiceOption { id: s(x, "id"), label: s(x, "l"), color: s(x, "c") }).collect()).unwrap_or_default() },
                2 => ColumnKind::Check, 3 => ColumnKind::Timecode { fps: o.get("fps").and_then(Value::as_f64).unwrap_or(24.0) }, _ => ColumnKind::Text } }
        }).collect()).unwrap_or_default() },
        "row" => Kind::Row { bg: s(&a, "bg"), fg: s(&a, "fg") },
        "cell" => Kind::Cell { bg: s(&a, "bg"), fg: s(&a, "fg") },
        _ => Kind::Paragraph,
    };
    if !kind.is_text() { return (kind, String::new(), Spans::new()) }
    let raw: Vec<(usize, usize, &str, &str)> = a.get("spans").and_then(Value::as_array).map(|v| v.iter().filter_map(|x| {
        let o = x.as_object()?; Some((o.get("s")?.as_u64()? as usize, o.get("e")?.as_u64()? as usize, o.get("k")?.as_str()?, o.get("u").and_then(Value::as_str).unwrap_or("")))
    }).collect()).unwrap_or_default();
    let spans = if matches!(kind, Kind::Code { .. }) { Spans::new() } else { Spans::from_utf16(&text, raw) };
    (kind, text, spans)
}
