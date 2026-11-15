//! How a job is made (contract 7.1, 7.2, 7.5, 7.6, 4.2, 8.2): the templates on the share, the id from the job's name,
//! the manifest baked from a template and the values given, the paths put in the form manifests carry. These are
//! the rules of the SUBMITTING node; a worker never reads a template.
use min_paths::map::Mapping;
use serde_json::{json, Map, Value};
use std::path::Path;

/// The templates on the share that can be used: plug-in ones, examples, and the users' own - which win over a shipped
/// one of the same id. A file that is not a template (no id, no name, no executable) is left out.
pub fn templates(farm: &Path) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for dir in ["templates/examples", "templates/plugins", "templates"] {
        let Ok(rd) = std::fs::read_dir(farm.join(dir)) else { continue }; let mut files: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json") && p.file_name().is_some_and(|n| n != "farm.json")).collect(); files.sort();
        for f in files { let Some(t) = std::fs::read_to_string(&f).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) else { continue };
            let id = t["template_id"].as_str().unwrap_or(""); if id.is_empty() || t["name"].as_str().unwrap_or("").is_empty() || !["windows", "linux", "macos"].iter().any(|o| !t["cmd"][o].as_str().unwrap_or("").is_empty()) { continue }
            if dir == "templates" { out.retain(|x| x["template_id"].as_str() != Some(id)); }
            out.push(t); }
    }
    out
}
/// The templates the New Job form offers: the examples and the users' own (the plug-ins' templates are not for the form).
pub fn form_templates(farm: &Path) -> Vec<Value> {
    let plug: Vec<String> = std::fs::read_dir(farm.join("templates/plugins")).map(|rd| rd.flatten().filter_map(|e| std::fs::read_to_string(e.path()).ok()).filter_map(|t| serde_json::from_str::<Value>(&t).ok()).filter_map(|t| t["template_id"].as_str().map(str::to_string)).collect()).unwrap_or_default();
    let users: Vec<String> = std::fs::read_dir(farm.join("templates")).map(|rd| rd.flatten().filter(|e| e.path().is_file()).filter_map(|e| std::fs::read_to_string(e.path()).ok()).filter_map(|t| serde_json::from_str::<Value>(&t).ok()).filter_map(|t| t["template_id"].as_str().map(str::to_string)).collect()).unwrap_or_default();
    templates(farm).into_iter().filter(|t| { let id = t["template_id"].as_str().unwrap_or(""); !plug.iter().any(|p| p == id) || users.iter().any(|u| u == id) }).collect()
}
/// Everything that stands in the way of a submission from the form - all of it at once, not the first alone (row 389).
pub fn refusals(template: &Value, values: &[String], exe: &str, name: &str, a: i64, b: i64, chunk: i64) -> Vec<String> {
    let mut out = Vec::new();
    if exe.is_empty() { out.push("Executable path is empty".to_string()); } if name.trim().is_empty() { out.push("Job name is empty".into()); } else if name.bytes().all(|c| !(c.is_ascii_alphanumeric() || c == b'_')) { out.push("Job name produces an empty slug".into()); }
    if a > b { out.push("Frame start must be <= frame end".into()); } if chunk < 1 { out.push("Chunk size must be >= 1".into()); }
    for (f, v) in template["flags"].as_array().map(|a| a.as_slice()).unwrap_or(&[]).iter().zip(values) { if f["editable"] == json!(true) && f["required"] == json!(true) && v.trim().is_empty() { out.push(format!("Required field is empty: {}", f["info"].as_str().unwrap_or("a value"))); } }
    out
}
/// A job's id from its name (4.2): bytes lower-cased, letters, digits and `_` kept, every other byte a `-`, runs of
/// `-` made one, none at the ends, 64 bytes at most; the first of `id`, `id-2` ... `id-99` that has no folder under
/// `jobs/` on the share. `None`: the name gives nothing, or all are taken.
pub fn slug(name: &str, farm: &Path) -> Option<String> {
    let mut s = String::new();
    for b in name.bytes() { let c = b.to_ascii_lowercase(); if c.is_ascii_alphanumeric() || c == b'_' { s.push(c as char); } else if !s.ends_with('-') { s.push('-'); } }
    let mut s = s.trim_matches('-').to_string(); s.truncate(64); if s.is_empty() { return None }
    let jobs = farm.join("jobs"); if !jobs.join(&s).exists() { return Some(s) }
    (2..=99).map(|i| format!("{s}-{i}")).find(|c| !jobs.join(c).exists())
}
/// What stands in the way of a submission (7.5), or nothing.
pub fn refusal(template: &Value, values: &[String], exe: &str, a: i64, b: i64, chunk: i64) -> Option<String> {
    if exe.is_empty() { return Some("the template names no program for this system".into()) }
    if a > b { return Some("the first frame is after the last".into()) } if chunk < 1 { return Some("the chunk size is less than 1".into()) }
    for (f, v) in template["flags"].as_array().map(|a| a.as_slice()).unwrap_or(&[]).iter().zip(values) { if f["editable"] == json!(true) && f["required"] == json!(true) && v.is_empty() { return Some(format!("\"{}\" is required", f["info"].as_str().unwrap_or("a value"))) } }
    None
}
/// The manifest of a job (7.5): the template with one value for each of its flags.
pub fn bake(template: &Value, values: &[String], exe: &str, os: &str, id: &str, a: i64, b: i64, chunk: i64, node: &str, now_ms: u64) -> Value {
    let mut cmd = Map::new(); for o in ["windows", "linux", "macos"] { if let Some(c) = template["cmd"][o].as_str().filter(|c| !c.is_empty()) { cmd.insert(o.into(), json!(c)); } } cmd.insert(os.into(), json!(exe));
    let none = Vec::new(); let flags = template["flags"].as_array().unwrap_or(&none);
    let fact = |i: usize| { let f = &flags[i]; (f["flag"].as_str().unwrap_or("").to_string(), f["value"].is_null(), f["editable"] == json!(true), f["required"] == json!(true)) };
    // an entry that may be left out: editable, not required, with no flag of its own, and nothing given for it
    let optional_and_empty = |i: usize| { let (flag, _, editable, required) = fact(i); editable && !required && flag.is_empty() && values.get(i).is_none_or(|v| v.is_empty()) };
    let (mut out, mut i, mut output_dir) = (Vec::new(), 0, None::<String>);
    while i < flags.len() {
        let (flag, null, editable, _) = fact(i);
        // a bare switch whose value was left out goes with it
        if !flag.is_empty() && null && !editable && i + 1 < flags.len() && optional_and_empty(i + 1) { i += 2; continue }
        if optional_and_empty(i) { i += 1; continue }
        let value = if editable { json!(values.get(i).cloned().unwrap_or_default()) } else { flags[i]["value"].clone() };
        let mut e = json!({ "flag": flag, "value": value });
        if flags[i]["type"] == json!("output") { e["is_output"] = json!(true); if output_dir.is_none() { output_dir = value.as_str().filter(|v| !v.is_empty()).map(parent); } }
        out.push(e); i += 1;
    }
    let d = &template["job_defaults"];
    json!({ "_version": 1, "job_id": id, "template_id": template["template_id"], "submitted_by": node, "submitted_os": os, "submitted_at_ms": now_ms, "cmd": cmd, "flags": out, "frame_start": a, "frame_end": b, "chunk_size": chunk,
        "max_retries": d["max_retries"].as_i64().unwrap_or(3), "timeout_seconds": d.get("timeout_seconds").cloned().unwrap_or(Value::Null), "output_dir": output_dir,
        "progress": progress(&template["progress"]), "output_detection": { "stdout_regex": template["output_detection"].get("stdout_regex").cloned().unwrap_or(Value::Null), "path_group": template["output_detection"]["path_group"].as_i64().unwrap_or(1),
            "validation": template["output_detection"]["validation"].as_str().unwrap_or("exit_code_only"), "info": template["output_detection"]["info"].as_str().unwrap_or("") },
        "process": { "working_dir": template["process"].get("working_dir").cloned().unwrap_or(Value::Null) }, "environment": template.get("environment").cloned().unwrap_or(json!({})), "tags_required": template.get("tags_required").cloned().unwrap_or(json!([])) })
}
fn progress(p: &Value) -> Value { json!({ "patterns": p.get("patterns").cloned().unwrap_or(json!([])), "error_patterns": p.get("error_patterns").cloned().unwrap_or(json!([])), "completion_pattern": p.get("completion_pattern").cloned().unwrap_or(Value::Null) }) }
/// A full path of this system's that lies under none of the farm's path mappings (and there are mappings): only a
/// machine that has this exact path can use it. (Blender's "//" paths, relative to the scene, are not full paths.)
pub fn outside(value: &str, mappings: &[Mapping]) -> bool {
    let b = value.as_bytes(); let full = if cfg!(windows) { (b.len() > 2 && b[1] == b':') || value.starts_with("\\\\") } else { value.starts_with('/') && !value.starts_with("//") };
    full && mappings.iter().any(|m| m.enabled) && !matches!(min_paths::map::to_ref(value, min_paths::Os::current(), mappings), min_paths::map::Ref::Vol { .. })
}
pub const OUTSIDE: &str = "Outside every path mapping \u{2014} only machines that have this exact path can render it.";
/// The folder a path is in, as the system this runs on reads paths.
pub fn parent(p: &str) -> String { let cut = if cfg!(windows) { p.rfind(['/', '\\']) } else { p.rfind('/') }; match cut { Some(0) => p[..1].to_string(), Some(i) => p[..i].to_string(), None => String::new() } }
/// A manifest made on macOS is put in the form manifests travel in (7.6): every flag value that is not empty, and the
/// output folder, from macOS to Windows through this node's mappings; it then says it was submitted on Windows.
pub fn canonical(mut m: Value, mappings: &[Mapping]) -> Value {
    use min_paths::{map::translate, Os};
    // (row 418) only what lies under a mapping is translated: a path under none is left exactly as given - turned
    // "to Windows" it came out with its slashes flipped, a path no machine has, and a Mac that did have it wrote elsewhere
    let mapped = |v: &str| matches!(min_paths::map::to_ref(v, Os::Mac, mappings), min_paths::map::Ref::Vol { .. });
    if let Some(flags) = m["flags"].as_array_mut() { for f in flags { if let Some(v) = f["value"].as_str().filter(|v| !v.is_empty() && mapped(v)) { f["value"] = json!(translate(Os::Mac, Os::Win, v, mappings)); } } }
    if let Some(d) = m["output_dir"].as_str().filter(|d| !d.is_empty() && mapped(d)) { m["output_dir"] = json!(translate(Os::Mac, Os::Win, d, mappings)); }
    m["submitted_os"] = json!("windows"); m
}

#[cfg(test)]
mod path_tests {
    use super::*;
    fn table() -> Vec<Mapping> { vec![Mapping { id: String::new(), label: "jobs".into(), win: "Z:\\jobs".into(), mac: "/Volumes/jobs".into(), lin: String::new(), enabled: true }] }
    /// Row 418: what lies under a mapping goes to Windows form; a path under none, and a value that is no path, stay as given.
    #[test] fn canonical_leaves_what_no_mapping_holds() {
        let m = json!({ "flags": [{ "flag": "", "value": "/Volumes/jobs/a/s.blend" }, { "flag": "", "value": "/Users/me/local/s_####" }, { "flag": "-F", "value": "OPEN_EXR" }, { "flag": "-b", "value": null }], "output_dir": "/Users/me/local", "submitted_os": "macos" });
        let c = canonical(m, &table());
        assert_eq!(c["flags"][0]["value"], "Z:\\jobs\\a\\s.blend"); assert_eq!(c["flags"][1]["value"], "/Users/me/local/s_####"); assert_eq!(c["flags"][2]["value"], "OPEN_EXR"); assert!(c["flags"][3]["value"].is_null());
        assert_eq!(c["output_dir"], "/Users/me/local"); assert_eq!(c["submitted_os"], "windows");
    }
    #[cfg(not(windows))] #[test] fn outside_is_a_full_path_under_no_mapping() {
        assert!(outside("/Users/me/x", &table())); assert!(!outside("/Volumes/jobs/x", &table())); assert!(!outside("//relative/to/scene", &table())); assert!(!outside("OPEN_EXR", &table())); assert!(!outside("/Users/me/x", &[]));
    }
}
