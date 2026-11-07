//! `min-settings` — the family's settings service (canon §24 row 142, RULED 2026-11-07): ONE implementation of what
//! QCView, ufb and minNotes each do with `QSettings` today. A typed key [`Schema`] with defaults and clamps; values that
//! fall back to the default when unset; a real `reset_all`; one JSON file per app in the platform's config directory,
//! written atomically; `<APP>_SETTINGS_DIR` (QCView's `QCV_SETTINGS_DIR` dev aid) redirects the whole store for tests
//! and side-by-side runs; a change generation the UI binds to instead of per-key signals. Keys another build wrote but
//! this schema does not know are kept and written back (QSettings' behaviour — a downgrade loses nothing).
use std::{collections::BTreeMap, io, path::{Path, PathBuf}};

#[derive(Clone, Debug, PartialEq)]
pub enum Value { Bool(bool), Int(i64), Float(f64), Str(String) }

impl Value {
    pub fn as_bool(&self) -> Option<bool> { if let Value::Bool(b) = self { Some(*b) } else { None } }
    pub fn as_int(&self) -> Option<i64> { match self { Value::Int(i) => Some(*i), Value::Float(f) => Some(*f as i64), _ => None } }
    pub fn as_float(&self) -> Option<f64> { match self { Value::Float(f) => Some(*f), Value::Int(i) => Some(*i as f64), _ => None } }
    pub fn as_str(&self) -> Option<&str> { if let Value::Str(s) = self { Some(s) } else { None } }
    fn kind(&self) -> &'static str { match self { Value::Bool(_) => "bool", Value::Int(_) => "int", Value::Float(_) => "float", Value::Str(_) => "string" } }
    fn to_json(&self) -> serde_json::Value {
        match self { Value::Bool(b) => (*b).into(), Value::Int(i) => (*i).into(), Value::Float(f) => serde_json::Number::from_f64(*f).map(serde_json::Value::Number).unwrap_or(serde_json::Value::Null), Value::Str(s) => s.clone().into() }
    }
    fn from_json(v: &serde_json::Value) -> Option<Value> {
        match v { serde_json::Value::Bool(b) => Some(Value::Bool(*b)), serde_json::Value::Number(n) => if let Some(i) = n.as_i64() { Some(Value::Int(i)) } else { n.as_f64().map(Value::Float) }, serde_json::Value::String(s) => Some(Value::Str(s.clone())), _ => None }
    }
}

/// A key's default — a const table cannot own a `String`, so string defaults are `&'static str`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Default { Bool(bool), Int(i64), Float(f64), Str(&'static str) }
impl Default {
    pub fn value(self) -> Value { match self { Default::Bool(b) => Value::Bool(b), Default::Int(i) => Value::Int(i), Default::Float(f) => Value::Float(f), Default::Str(s) => Value::Str(s.to_string()) } }
    fn kind(self) -> &'static str { match self { Default::Bool(_) => "bool", Default::Int(_) => "int", Default::Float(_) => "float", Default::Str(_) => "string" } }
}

/// One key: its name (`group/name`, QCView's spelling), default, and for numbers an inclusive clamp.
#[derive(Clone, Copy, Debug)]
pub struct Key { pub name: &'static str, pub default: Default, pub range: Option<(f64, f64)> }

impl Key {
    pub const fn bool(name: &'static str, default: bool) -> Key { Key { name, default: Default::Bool(default), range: None } }
    pub const fn int(name: &'static str, default: i64, min: i64, max: i64) -> Key { Key { name, default: Default::Int(default), range: Some((min as f64, max as f64)) } }
    pub const fn float(name: &'static str, default: f64, min: f64, max: f64) -> Key { Key { name, default: Default::Float(default), range: Some((min, max)) } }
    pub const fn str(name: &'static str, default: &'static str) -> Key { Key { name, default: Default::Str(default), range: None } }
}

/// The schema: the app's keys.
pub struct Schema { pub keys: &'static [Key] }

impl Schema {
    pub fn key(&self, name: &str) -> Option<&Key> { self.keys.iter().find(|k| k.name == name) }
    pub fn default_of(&self, name: &str) -> Option<Value> { self.key(name).map(|k| k.default.value()) }
}

pub struct Settings {
    schema: &'static Schema,
    values: BTreeMap<String, Value>,
    /// keys this schema does not know (another build's) — kept and written back
    extra: BTreeMap<String, serde_json::Value>,
    path: Option<PathBuf>,
    generation: u64,
    dirty: bool,
}

impl Settings {
    /// The store's directory for `app`: `<APP>_SETTINGS_DIR` (e.g. `QCV_SETTINGS_DIR`) when set, else the platform's
    /// per-user config directory (macOS `~/Library/Application Support/<app>`, Windows `%APPDATA%\<app>`, else
    /// `$XDG_CONFIG_HOME/<app>` or `~/.config/<app>`).
    pub fn dir_for(app: &str, env_prefix: &str) -> Option<PathBuf> {
        if let Some(d) = std::env::var_os(format!("{env_prefix}_SETTINGS_DIR")) { if !d.is_empty() { return Some(PathBuf::from(d)) } }
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
        if cfg!(target_os = "macos") { return home.map(|h| h.join("Library").join("Application Support").join(app)) }
        if cfg!(windows) { return std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join(app)).or_else(|| home.map(|h| h.join("AppData").join("Roaming").join(app))) }
        std::env::var_os("XDG_CONFIG_HOME").filter(|x| !x.is_empty()).map(|x| PathBuf::from(x).join(app)).or_else(|| home.map(|h| h.join(".config").join(app)))
    }
    /// Open (and load) the app's store. `env_prefix` names the isolation variable (`QCV` → `QCV_SETTINGS_DIR`).
    pub fn open(app: &str, env_prefix: &str, schema: &'static Schema) -> Settings {
        let path = Self::dir_for(app, env_prefix).map(|d| d.join("settings.json"));
        let mut s = Settings { schema, values: BTreeMap::new(), extra: BTreeMap::new(), path, generation: 0, dirty: false };
        s.load();
        s
    }
    /// A store with no file (tests, the harness).
    pub fn in_memory(schema: &'static Schema) -> Settings { Settings { schema, values: BTreeMap::new(), extra: BTreeMap::new(), path: None, generation: 0, dirty: false } }
    pub fn path(&self) -> Option<&Path> { self.path.as_deref() }
    pub fn schema(&self) -> &'static Schema { self.schema }
    /// Bumps on every change that took; the UI compares it instead of listening per key.
    pub fn generation(&self) -> u64 { self.generation }

    /// The value, or the schema's default when unset. Panics on a key the schema does not have (a programming error).
    pub fn get(&self, name: &str) -> Value {
        self.values.get(name).cloned().or_else(|| self.schema.default_of(name)).unwrap_or_else(|| panic!("settings: `{name}` is not in the schema"))
    }
    pub fn bool(&self, name: &str) -> bool { self.get(name).as_bool().unwrap_or(false) }
    pub fn int(&self, name: &str) -> i64 { self.get(name).as_int().unwrap_or(0) }
    pub fn float(&self, name: &str) -> f64 { self.get(name).as_float().unwrap_or(0.0) }
    pub fn string(&self, name: &str) -> String { self.get(name).as_str().unwrap_or("").to_string() }
    pub fn is_default(&self, name: &str) -> bool { self.values.get(name).is_none_or(|v| Some(v) == self.schema.default_of(name).as_ref()) }
    pub fn is_set(&self, name: &str) -> bool { self.values.contains_key(name) }

    /// Set a value: clamped to the key's range, coerced between int and float, refused (false, logged) when the kind
    /// is wrong or the key unknown. Returns whether the stored value changed.
    pub fn set(&mut self, name: &str, v: Value) -> bool {
        let Some(k) = self.schema.key(name) else { log::warn!("settings: `{name}` is not in the schema; ignored"); return false };
        let v = match (k.default, v) {
            (Default::Int(_), Value::Int(i)) => Value::Int(clamp(i as f64, k.range) as i64),
            (Default::Int(_), Value::Float(f)) => Value::Int(clamp(f.round(), k.range) as i64),
            (Default::Float(_), Value::Float(f)) => Value::Float(clamp(f, k.range)),
            (Default::Float(_), Value::Int(i)) => Value::Float(clamp(i as f64, k.range)),
            (Default::Bool(_), v @ Value::Bool(_)) | (Default::Str(_), v @ Value::Str(_)) => v,
            (d, v) => { log::warn!("settings: `{name}` takes a {}, not a {}; ignored", d.kind(), v.kind()); return false }
        };
        if self.values.get(name) == Some(&v) { return false }
        if Some(&v) == self.schema.default_of(name).as_ref() && !self.values.contains_key(name) { return false }
        self.values.insert(name.to_string(), v);
        self.generation += 1; self.dirty = true;
        true
    }
    pub fn set_bool(&mut self, name: &str, b: bool) -> bool { self.set(name, Value::Bool(b)) }
    pub fn set_int(&mut self, name: &str, i: i64) -> bool { self.set(name, Value::Int(i)) }
    pub fn set_float(&mut self, name: &str, f: f64) -> bool { self.set(name, Value::Float(f)) }
    pub fn set_str(&mut self, name: &str, s: &str) -> bool { self.set(name, Value::Str(s.to_string())) }
    /// Back to the default (the key is removed from the store).
    pub fn reset(&mut self, name: &str) -> bool { let took = self.values.remove(name).is_some(); if took { self.generation += 1; self.dirty = true; } took }
    /// QCView's "Reset all preferences?": every known key back to its default; unknown keys are dropped too.
    pub fn reset_all(&mut self) { if !self.values.is_empty() || !self.extra.is_empty() { self.values.clear(); self.extra.clear(); self.generation += 1; self.dirty = true; } }
    /// Keys the store holds a non-default value for.
    pub fn set_keys(&self) -> impl Iterator<Item = &str> { self.values.keys().map(String::as_str) }

    fn load(&mut self) {
        let Some(p) = &self.path else { return };
        let Ok(text) = std::fs::read_to_string(p) else { return };
        let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(&text) else { log::warn!("settings: {} is not a JSON object; starting from defaults", p.display()); return };
        for (k, v) in map {
            if self.schema.key(&k).is_some() { if let Some(val) = Value::from_json(&v) { let _ = self.set(&k, val); } } else { self.extra.insert(k, v); }
        }
        self.dirty = false; self.generation = 0;
    }
    /// Write the store (atomically: a sibling temp file renamed over the old one). A no-op without a path.
    pub fn save(&mut self) -> io::Result<()> {
        let Some(p) = &self.path else { self.dirty = false; return Ok(()) };
        if let Some(dir) = p.parent() { std::fs::create_dir_all(dir)?; }
        let mut map = serde_json::Map::new();
        for (k, v) in &self.extra { map.insert(k.clone(), v.clone()); }
        for (k, v) in &self.values { map.insert(k.clone(), v.to_json()); }
        let text = serde_json::to_string_pretty(&serde_json::Value::Object(map))?;
        let tmp = p.with_extension(format!("json.{}.tmp", std::process::id()));
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, p)?;
        self.dirty = false;
        Ok(())
    }
    pub fn save_if_dirty(&mut self) -> io::Result<()> { if self.dirty { self.save() } else { Ok(()) } }
    pub fn is_dirty(&self) -> bool { self.dirty }
}

fn clamp(v: f64, range: Option<(f64, f64)>) -> f64 { match range { Some((lo, hi)) => v.clamp(lo, hi), None => v } }

#[cfg(test)]
mod tests {
    use super::*;
    static KEYS: &[Key] = &[Key::bool("ui/thumbs", true), Key::int("ui/width", 280, 200, 600), Key::float("audio/volume", 1.0, 0.0, 1.0), Key::str("export/format", "png")];
    static SCHEMA: Schema = Schema { keys: KEYS };

    #[test]
    fn defaults_clamps_kinds_and_generations() {
        let mut s = Settings::in_memory(&SCHEMA);
        assert!(s.bool("ui/thumbs") && s.int("ui/width") == 280 && s.float("audio/volume") == 1.0 && s.string("export/format") == "png");
        assert_eq!(s.generation(), 0);
        assert!(s.set_int("ui/width", 1000)); assert_eq!(s.int("ui/width"), 600, "clamped to the key's range");
        assert!(!s.set_int("ui/width", 700), "the same clamped value is not a change");
        assert!(!s.set_str("ui/width", "x"), "a wrong kind is refused");
        assert!(!s.set_bool("nope/key", true), "an unknown key is refused");
        assert!(s.set_float("audio/volume", 0.25) && s.set_bool("ui/thumbs", false));
        assert!(s.set("ui/width", Value::Float(300.4)) && s.int("ui/width") == 300, "floats coerce to int keys");
        assert_eq!(s.generation(), 4);
        assert!(!s.set_bool("ui/thumbs", true) || s.is_default("ui/thumbs"));
        s.reset_all();
        assert!(s.is_default("ui/width") && s.is_default("audio/volume") && s.set_keys().count() == 0);
    }

    #[test]
    fn saves_atomically_reloads_and_keeps_unknown_keys() {
        let dir = std::env::temp_dir().join(format!("min-settings-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // SAFETY: tests in this module run with no other thread reading this variable
        unsafe { std::env::set_var("MST_SETTINGS_DIR", &dir) };
        assert_eq!(Settings::dir_for("mst", "MST").unwrap(), dir, "the env var redirects the store");
        let mut s = Settings::open("mst", "MST", &SCHEMA);
        assert!(s.path().unwrap().ends_with("settings.json"));
        s.set_int("ui/width", 420); s.set_str("export/format", "jpeg");
        s.save().unwrap();
        // another build wrote a key this schema does not know
        let p = s.path().unwrap().to_path_buf();
        let mut j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        j["future/key"] = serde_json::json!(7); std::fs::write(&p, serde_json::to_string(&j).unwrap()).unwrap();
        let mut s2 = Settings::open("mst", "MST", &SCHEMA);
        assert_eq!(s2.int("ui/width"), 420); assert_eq!(s2.string("export/format"), "jpeg"); assert_eq!(s2.generation(), 0, "loading is not a change");
        s2.set_bool("ui/thumbs", false); s2.save().unwrap();
        let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(j["future/key"], 7, "the unknown key survived a save"); assert_eq!(j["ui/thumbs"], false);
        assert!(std::fs::read_dir(&dir).unwrap().filter_map(Result::ok).all(|e| !e.file_name().to_string_lossy().ends_with(".tmp")), "no temp file left behind");
        s2.reset_all(); s2.save().unwrap();
        let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(j.as_object().unwrap().len(), 0, "reset all empties the file");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
