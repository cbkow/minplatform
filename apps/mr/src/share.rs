//! What the node keeps on the share besides its phonebook entry (contract 6.2-6.4): the farm's own file with the
//! secret, the Blender template and add-on it brings, and its log, which other nodes read from there.
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The content this build brings to a share (row 390: Blender only), with where each file goes under the farm's folder.
const CONTENT: [(&str, &str); 3] = [("templates/examples/blender-5.2-studio.json", include_str!("../resources/templates/blender-5.2-studio.json")), ("templates/plugins/blender-5.2-plugin.json", include_str!("../resources/templates/plugins/blender-5.2-plugin.json")), ("plugins/blender/MinRender.py", include_str!("../resources/plugins/blender/MinRender.py"))];

/// `n` bytes from the system's source of randomness.
pub fn random(n: usize) -> Result<Vec<u8>, String> {
    let mut b = vec![0u8; n];
    #[cfg(unix)] std::fs::File::open("/dev/urandom").and_then(|mut f| std::io::Read::read_exact(&mut f, &mut b)).map_err(|e| format!("no random bytes: {e}"))?;
    #[cfg(windows)] { use windows::Win32::Security::Cryptography::*; if unsafe { BCryptGenRandom(None, &mut b, BCRYPT_USE_SYSTEM_PREFERRED_RNG) }.is_err() { return Err("no random bytes".into()) } }
    Ok(b)
}
pub fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }
/// "0.5.11" against "0.6.0", part by part as numbers (a part that is missing or no number counts as 0).
fn newer(mine: &str, theirs: &str) -> bool { let p = |s: &str| -> Vec<u64> { s.split('.').map(|x| x.parse().unwrap_or(0)).collect() }; let (a, b) = (p(mine), p(theirs));
    for i in 0..a.len().max(b.len()) { let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0)); if x != y { return x > y } } false }
/// This build's files are put on the share - written over what is there of the same name; nothing else is touched or
/// removed (0.5.x also deleted every template it did not ship). True: every file went.
fn put_content(farm: &Path) -> bool { let mut ok = true; for (rel, text) in CONTENT { let f = farm.join(rel); if let Some(d) = f.parent() { let _ = std::fs::create_dir_all(d); } if std::fs::write(&f, text).is_err() { ok = false; } } ok }

/// The farm's folder at the start of a node (6.2): made if it is not there; its file read, a secret made if it has
/// none; this build's content put there if this build is newer than the one that last did so. The secret.
pub fn start_farm(farm: &Path, node: &str, now_ms: u64) -> Result<String, String> {
    for d in ["nodes", "jobs", "state", "templates", "templates/plugins", "templates/examples", "plugins"] { std::fs::create_dir_all(farm.join(d)).map_err(|e| format!("the farm's folder could not be made: {e}"))?; }
    let file = farm.join("farm.json"); let version = env!("CARGO_PKG_VERSION");
    let write = |v: &Value| std::fs::write(&file, serde_json::to_string_pretty(v).unwrap()).map_err(|e| format!("farm.json could not be written: {e}"));
    if !file.exists() {
        let secret = hex(&random(32)?); let stamp = if put_content(farm) { version } else { "" };
        write(&json!({ "_version": 1, "protocol_version": 2, "created_by": node, "created_at_ms": now_ms, "last_example_update": stamp, "api_secret": secret }))?;
        return Ok(secret)
    }
    // (a file that is there and cannot be read is never replaced: the node starts without a secret, as 0.5.x does)
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&file).map_err(|e| format!("farm.json: {e}"))?).map_err(|e| format!("farm.json is there and is not JSON ({e}); it is left as it is"))?;
    let mut changed = false;
    if v["api_secret"].as_str().is_none_or(str::is_empty) { v["api_secret"] = json!(hex(&random(32)?)); changed = true; }
    if newer(version, v["last_example_update"].as_str().unwrap_or("")) && put_content(farm) { v["last_example_update"] = json!(version); changed = true; }
    if changed { write(&v)?; }
    Ok(v["api_secret"].as_str().unwrap_or("").to_string())
}

// ------------------------------------------------------------------------------------------------------------ the log
/// (year, month, day, hour, minute, second, millisecond) of this machine's local time, `days` days from now.
fn local(days: i64) -> (i32, u32, u32, u32, u32, u32, u32) {
    #[cfg(unix)] { let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default(); let t = now.as_secs() as libc::time_t + (days * 86400) as libc::time_t; let mut tm: libc::tm = unsafe { std::mem::zeroed() }; unsafe { libc::localtime_r(&t, &mut tm); }
        return (tm.tm_year + 1900, tm.tm_mon as u32 + 1, tm.tm_mday as u32, tm.tm_hour as u32, tm.tm_min as u32, tm.tm_sec as u32, now.subsec_millis()) }
    #[cfg(windows)] { let s = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        // the day is moved by the calendar's own arithmetic (days since 1970 and back)
        let (y, m) = if s.wMonth <= 2 { (s.wYear as i64 - 1, s.wMonth as i64 + 9) } else { (s.wYear as i64, s.wMonth as i64 - 3) }; let era = y.div_euclid(400); let yoe = y - era * 400; let doy = (153 * m + 2) / 5 + s.wDay as i64 - 1; let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let z = era * 146097 + doe + days; let era = z.div_euclid(146097); let doe = z - era * 146097; let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); let mp = (5 * doy + 2) / 153; let d = doy - (153 * mp + 2) / 5 + 1; let m = if mp < 10 { mp + 3 } else { mp - 9 }; let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
        return (y as i32, m as u32, d as u32, s.wHour as u32, s.wMinute as u32, s.wSecond as u32, s.wMilliseconds as u32) }
}
fn day_name(days: i64) -> String { let (y, m, d, ..) = local(days); format!("monitor-{y:04}-{m:02}-{d:02}.log") }
static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();
/// Where this node's log goes on the share (`nodes/<id>`); files more than seven days old are removed now, not
/// only when the date turns while the node runs (ruled, row 389).
pub fn log_to(dir: PathBuf) {
    let keep: Vec<String> = (-7..=0).map(day_name).collect();
    if let Ok(rd) = std::fs::read_dir(&dir) { for e in rd.flatten() { let n = e.file_name().to_string_lossy().into_owned(); if n.starts_with("monitor-") && n.ends_with(".log") && !keep.contains(&n) { let _ = std::fs::remove_file(e.path()); } } }
    let _ = LOG_DIR.set(dir);
}
/// One event: to the standard error, and to the node's log on the share as other nodes read it (6.4):
/// `HH:MM:SS.mmm LEVEL  [category] message`, local time, the file opened for each line.
pub fn log(level: &str, category: &str, msg: &str) {
    let (_, _, _, h, mi, s, ms) = local(0); let line = format!("{h:02}:{mi:02}:{s:02}.{ms:03} {level}  [{category}] {msg}");
    eprintln!("{line}");
    if let Some(d) = LOG_DIR.get() { if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(d.join(day_name(0))) { let _ = writeln!(f, "{line}"); } }
}
/// The last `max` lines of a node's log as THIS machine's yesterday and today name the files (2.5 row 33).
pub fn node_log(farm: &Path, node: &str, max: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for days in [-1, 0] { if let Ok(t) = std::fs::read_to_string(farm.join("nodes").join(node).join(day_name(days))) { lines.extend(t.lines().map(str::to_string)); } }
    let from = lines.len().saturating_sub(max); lines.split_off(from)
}
/// The chunk logs of a job on the share (2.5 row 31): each `*.log` under `jobs/<id>/stdout/<node>/` whose name ends in `_<number>`.
pub fn task_output(farm: &Path, job: &str) -> Value {
    let mut out = Vec::new();
    if let Ok(nodes) = std::fs::read_dir(farm.join("jobs").join(job).join("stdout")) { for n in nodes.flatten().filter(|n| n.path().is_dir()) { let node = n.file_name().to_string_lossy().into_owned();
        if let Ok(files) = std::fs::read_dir(n.path()) { for f in files.flatten().filter(|f| f.path().is_file()) { let name = f.file_name().to_string_lossy().into_owned();
            let Some(stem) = name.strip_suffix(".log").filter(|s| !s.is_empty()) else { continue }; let Some((range, ms)) = stem.rsplit_once('_') else { continue }; let Ok(ms) = ms.parse::<i64>() else { continue };
            out.push(json!({ "node_id": node, "range": range, "timestamp_ms": ms, "path": f.path().to_string_lossy() })); } } } }
    Value::Array(out)
}
