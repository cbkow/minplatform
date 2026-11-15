//! mr-conform - the minRender port's conformance harness, part 1: listen and check.
//!
//!   mr-conform record --config <config.json> --out <dir> [--max-jobs N]
//!       Asks the node on THIS machine (and, for what only a leader answers, the leader) what it answers, and copies
//!       the small files of the share that are part of the contract. GET requests only; the share is only read.
//!       The farm's secret is read from the share, used, and NEVER written: `farm.json` is saved with it redacted.
//!       A recording holds job and machine names: `--out` belongs under `fixtures/` (ignored), never in the tree.
//!   mr-conform check <dir>
//!       Checks a recording against the contract (docs/phase4/step0-contracts.md, sections 1.2, 2.2, 2.4-2.7, 6.2).
//!       Prints one line per finding, naming keys and types only - never a value - so its output can be quoted.
//!       Exit code 1 when something contradicts the contract.
use serde_json::{json, Map, Value};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn arg(args: &[String], k: &str) -> Option<String> { args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned()) }

// --------------------------------------------------------------------------------------------------- a plain client
/// One answer: the status, the headers as sent (names lower-cased), the body.
struct Answer { status: u16, headers: Vec<(String, String)>, body: Vec<u8>, ms: u128 }
/// One `GET` over a new connection, as a 0.5.x node makes its calls (HTTP/1.1, no TLS). `auth` is the whole value of
/// the Authorization header, when there is one.
fn get(endpoint: &str, path: &str, auth: Option<&str>) -> Result<Answer, String> { http(endpoint, "GET", path, auth, None) }
/// Any method; a body is sent as JSON text, as the application sends its own.
fn http(endpoint: &str, method: &str, path: &str, auth: Option<&str>, body: Option<&str>) -> Result<Answer, String> {
    let t = Instant::now();
    let addr = endpoint.to_socket_addrs().map_err(|e| e.to_string())?.next().ok_or("no address")?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(3)).map_err(|e| e.to_string())?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(10)));
    let mut req = format!("{method} {path} HTTP/1.1\r\nHost: {endpoint}\r\nAccept: */*\r\nConnection: close\r\n");
    if let Some(a) = auth { req.push_str(&format!("Authorization: {a}\r\n")); }
    if method != "GET" && method != "DELETE" { let b = body.unwrap_or(""); req.push_str(&format!("Content-Type: application/json\r\nContent-Length: {}\r\n", b.len())); }
    req.push_str("\r\n"); if method != "GET" && method != "DELETE" { req.push_str(body.unwrap_or("")); }
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut raw = Vec::new(); s.read_to_end(&mut raw).map_err(|e| e.to_string())?;
    let cut = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or("no header end")?;
    let head = String::from_utf8_lossy(&raw[..cut]).into_owned(); let mut lines = head.split("\r\n");
    let status: u16 = lines.next().and_then(|l| l.split(' ').nth(1)).and_then(|c| c.parse().ok()).ok_or("no status line")?;
    let headers: Vec<(String, String)> = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
    let mut body = raw[cut + 4..].to_vec();
    if headers.iter().any(|(k, v)| k == "transfer-encoding" && v.eq_ignore_ascii_case("chunked")) { body = dechunk(&body); }
    Ok(Answer { status, headers, body, ms: t.elapsed().as_millis() })
}
fn dechunk(b: &[u8]) -> Vec<u8> {
    let (mut out, mut i) = (Vec::new(), 0);
    while let Some(e) = b[i..].windows(2).position(|w| w == b"\r\n") {
        let n = usize::from_str_radix(String::from_utf8_lossy(&b[i..i + e]).split(';').next().unwrap_or("").trim(), 16).unwrap_or(0);
        if n == 0 || i + e + 2 + n > b.len() { break } out.extend_from_slice(&b[i + e + 2..i + e + 2 + n]); i += e + 2 + n + 2;
    }
    out
}

// ------------------------------------------------------------------------------------------------------- recording
struct Rec { out: PathBuf, n: usize, secret: String }
impl Rec {
    /// Sends one request and files the answer. `auth`: none | good | wrong | lowercase | twospaces | basic.
    fn ask(&mut self, name: &str, target: &str, endpoint: &str, path: &str, auth: &str) -> Option<(u16, Vec<u8>)> { self.send(name, target, endpoint, "GET", path, auth, None) }
    fn send(&mut self, name: &str, target: &str, endpoint: &str, method: &str, path: &str, auth: &str, body: Option<&str>) -> Option<(u16, Vec<u8>)> {
        let header = match auth { "good" => Some(format!("Bearer {}", self.secret)), "wrong" => Some(format!("Bearer {}", "0".repeat(self.secret.len().max(1)))), "lowercase" => Some(format!("bearer {}", self.secret)),
            "twospaces" => Some(format!("Bearer  {}", self.secret)), "basic" => Some(format!("Basic {}", self.secret)), _ => None };
        self.n += 1; let file = self.out.join(format!("{:03}-{name}.json", self.n));
        let (v, r) = match http(endpoint, method, path, header.as_deref(), body) {
            Ok(a) => { let keep = &a.body[..a.body.len().min(2 << 20)];
                (json!({ "name": name, "target": target, "method": method, "request_body": body, "path": path, "auth": auth, "status": a.status, "headers": a.headers.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>(),
                    "body_len": a.body.len(), "body": String::from_utf8_lossy(keep), "ms": a.ms as u64 }), Some((a.status, a.body))) }
            Err(e) => (json!({ "name": name, "target": target, "method": method, "request_body": body, "path": path, "auth": auth, "error": e }), None) };
        // (the secret can only be in a body if a node echoed it; it is taken out whatever happens)
        let text = serde_json::to_string_pretty(&v).unwrap().replace(&self.secret, "<redacted>");
        let _ = std::fs::write(&file, text);
        println!("{:>3} {name}: {}", self.n, r.as_ref().map(|(s, b)| format!("{s}, {} bytes", b.len())).unwrap_or_else(|| "no answer".into()));
        r
    }
}
fn seg(s: &str) -> String { s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect() }

fn record(args: &[String]) -> Result<(), String> {
    let config = PathBuf::from(arg(args, "--config").ok_or("--config <config.json>")?); let out = PathBuf::from(arg(args, "--out").ok_or("--out <dir>")?);
    let max_jobs: usize = arg(args, "--max-jobs").and_then(|x| x.parse().ok()).unwrap_or(5);
    let cfg: Value = serde_json::from_str(&std::fs::read_to_string(&config).map_err(|e| format!("{}: {e}", config.display()))?).map_err(|e| e.to_string())?;
    let port = cfg["http_port"].as_u64().unwrap_or(8420); let farm = Path::new(cfg["sync_root"].as_str().ok_or("the config has no sync_root")?).join("MinRender-v2");
    let farm_json: Value = serde_json::from_str(&std::fs::read_to_string(farm.join("farm.json")).map_err(|e| format!("farm.json: {e}"))?).map_err(|e| e.to_string())?;
    let secret = farm_json["api_secret"].as_str().unwrap_or("").to_string(); if secret.is_empty() { return Err("farm.json holds no secret".into()) }
    std::fs::create_dir_all(out.join("share")).map_err(|e| e.to_string())?;
    // -- the share: the small files of the contract, the secret taken out
    let mut fj = farm_json.clone(); fj["api_secret"] = json!(format!("<redacted: {} characters, lowercase hex: {}>", secret.len(), secret.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))));
    let _ = std::fs::write(out.join("share/farm.json"), serde_json::to_string_pretty(&fj).unwrap());
    let mut nodes = Vec::new();   // (node id, endpoint)
    for d in std::fs::read_dir(farm.join("nodes")).map_err(|e| e.to_string())?.flatten() {
        let id = d.file_name().to_string_lossy().into_owned(); let Ok(t) = std::fs::read_to_string(d.path().join("endpoint.json")) else { continue };
        let _ = std::fs::write(out.join(format!("share/endpoint-{id}.json")), &t);
        if let Ok(v) = serde_json::from_str::<Value>(&t) { if let (Some(ip), Some(p)) = (v["ip"].as_str(), v["port"].as_u64()) { nodes.push((id, format!("{ip}:{p}"))); } }
    }
    // the names in the farm's folder, three levels down (no file is opened)
    fn tree(p: &Path, depth: usize) -> Value { let mut m = Map::new(); if let Ok(rd) = std::fs::read_dir(p) { for e in rd.flatten().take(400) { let n = e.file_name().to_string_lossy().into_owned();
        let md = e.metadata().ok(); if md.as_ref().is_some_and(|m| m.is_dir()) { m.insert(n + "/", if depth > 0 { tree(&e.path(), depth - 1) } else { json!("...") }); } else { m.insert(n, json!(md.map(|m| m.len()).unwrap_or(0))); } } } Value::Object(m) }
    let _ = std::fs::write(out.join("share/tree.json"), serde_json::to_string_pretty(&tree(&farm, 3)).unwrap());

    let me = format!("127.0.0.1:{port}"); let mut r = Rec { out: out.clone(), n: 0, secret };
    // -- what any node answers
    let status = r.ask("status", "self", &me, "/api/status", "none");
    let my_id = status.as_ref().and_then(|(_, b)| serde_json::from_slice::<Value>(b).ok()).and_then(|v| v["node_id"].as_str().map(str::to_string)).unwrap_or_default();
    r.ask("status-with-a-wrong-token", "self", &me, "/api/status", "wrong");
    let peers = r.ask("peers", "self", &me, "/api/peers", "good");
    for a in ["none", "wrong", "lowercase", "twospaces", "basic"] { r.ask(&format!("peers-auth-{a}"), "self", &me, "/api/peers", a); }
    r.ask("unknown-api-path-with-token", "self", &me, "/api/no-such-route", "good");
    r.ask("unknown-api-path-without-token", "self", &me, "/api/no-such-route", "none");
    r.ask("unknown-path-outside-api", "self", &me, "/no-such-route", "none");
    r.ask("root", "self", &me, "/", "none");
    r.ask("config", "self", &me, "/api/config", "good");
    if !my_id.is_empty() { r.ask("node-log", "self", &me, &format!("/api/nodes/{}/log?max_lines=20", seg(&my_id)), "good"); }
    r.ask("task-output-of-a-path-that-climbs", "self", &me, "/api/jobs/%2E%2E/task-output", "good");
    // -- what only the leader answers: asked here first (a follower's refusal is part of the contract), then of the leader
    let leader = peers.as_ref().and_then(|(_, b)| serde_json::from_slice::<Value>(b).ok()).and_then(|v| v.as_array().and_then(|a| a.iter().find(|p| p["is_leader"] == json!(true) && p["is_alive"] == json!(true)).and_then(|p| p["endpoint"].as_str().map(str::to_string))));
    let jobs_here = r.ask("jobs", "self", &me, "/api/jobs", "good");
    let i_lead = jobs_here.as_ref().is_some_and(|(s, _)| *s == 200);
    let (who, at) = if i_lead { ("self".to_string(), me.clone()) } else { match leader { Some(e) => { r.ask("jobs", "leader", &e, "/api/jobs", "good"); ("leader".to_string(), e) } None => ("none".into(), String::new()) } };
    if !i_lead { r.ask("suspended-on-a-follower", "self", &me, "/api/nodes/suspended", "good"); r.ask("job-detail-on-a-follower", "self", &me, "/api/jobs/no-such-job", "good"); }
    if who != "none" {
        r.ask("suspended", &who, &at, "/api/nodes/suspended", "good");
        r.ask("job-that-does-not-exist", &who, &at, "/api/jobs/no-such-job-zz", "good");
        r.ask("manifest-of-a-job-that-does-not-exist", &who, &at, "/api/jobs/no-such-job-zz/manifest", "good");
        let list = get(&at, "/api/jobs", Some(&format!("Bearer {}", r.secret))).ok().and_then(|a| serde_json::from_slice::<Value>(&a.body).ok());
        let ids: Vec<String> = list.as_ref().and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|j| j["job_id"].as_str().map(str::to_string)).collect()).unwrap_or_default();
        // a spread of states where there is one, not just the first few
        let mut pick: Vec<String> = Vec::new();
        if let Some(a) = list.as_ref().and_then(|v| v.as_array()) { for st in ["active", "paused", "completed", "failed", "cancelled"] { if let Some(j) = a.iter().find(|j| j["current_state"] == json!(st)) { if let Some(id) = j["job_id"].as_str() { pick.push(id.to_string()); } } } }
        for id in ids { if pick.len() >= max_jobs { break } if !pick.contains(&id) { pick.push(id); } }
        for (i, id) in pick.iter().take(max_jobs).enumerate() {
            r.ask(&format!("job-{i}-detail"), &who, &at, &format!("/api/jobs/{}", seg(id)), "good");
            r.ask(&format!("job-{i}-manifest"), &who, &at, &format!("/api/jobs/{}/manifest", seg(id)), "good");
            let logs = r.ask(&format!("job-{i}-task-output"), "self", &me, &format!("/api/jobs/{}/task-output", seg(id)), "good");
            if let Some(first) = logs.and_then(|(_, b)| serde_json::from_slice::<Value>(&b).ok()).and_then(|v| v.as_array().and_then(|a| a.first().cloned())) {
                if let (Some(node), Some(p)) = (first["node_id"].as_str(), first["path"].as_str()) { let file = p.rsplit(['/', '\\']).next().unwrap_or("");
                    r.ask(&format!("job-{i}-task-output-file"), "self", &me, &format!("/api/jobs/{}/task-output/{}/{}", seg(id), seg(node), seg(file)), "good"); } }
        }
    }
    // -- every other node's own record
    for (id, e) in nodes.iter().filter(|(id, _)| *id != my_id) { r.ask(&format!("status-of-peer-{}", &id[..id.len().min(4)]), &format!("peer:{id}"), e, "/api/status", "none"); }
    let _ = std::fs::write(out.join("about.json"), serde_json::to_string_pretty(&json!({ "recorded_by": "mr-conform record", "requests": r.n, "this_node_leads": i_lead, "nodes_in_the_phonebook": nodes.len() })).unwrap());
    println!("{} requests recorded (GET only) in {}", r.n, out.display());
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------- driving
/// Part 2: what a leader does with commands, on a PRIVATE farm only (tools/mr-conform/farm.sh). It submits synthetic
/// jobs whose "renderer" is a shell script, sends every job command, and files each answer and each change of state.
/// It refuses any farm that is not on the loopback address under a folder named `mr-farm`.
fn drive(args: &[String]) -> Result<(), String> {
    let config = PathBuf::from(arg(args, "--config").ok_or("--config <config.json of a private farm's node>")?); let out = PathBuf::from(arg(args, "--out").ok_or("--out <dir>")?);
    let cfg: Value = serde_json::from_str(&std::fs::read_to_string(&config).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let root = cfg["sync_root"].as_str().unwrap_or("").to_string();
    if cfg["ip_override"] != json!("127.0.0.1") || !root.replace('\\', "/").contains("/mr-farm/") { return Err("this is not a private farm (loopback address, a share under a folder named mr-farm): refused".into()) }
    let farm = Path::new(&root).join("MinRender-v2"); let port = cfg["http_port"].as_u64().unwrap_or(0);
    let secret = serde_json::from_str::<Value>(&std::fs::read_to_string(farm.join("farm.json")).map_err(|e| e.to_string())?).ok().and_then(|v| v["api_secret"].as_str().map(str::to_string)).ok_or("no secret")?;
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let me = format!("127.0.0.1:{port}"); let bearer = format!("Bearer {secret}"); let mut r = Rec { out: out.clone(), n: 0, secret };
    // the leader: this node, or the one its refusal names
    let first = get(&me, "/api/jobs", Some(&bearer))?; let at = if first.status == 200 { me.clone() } else { serde_json::from_slice::<Value>(&first.body).ok().and_then(|v| v["leader_endpoint"].as_str().map(str::to_string)).ok_or("no leader known yet")? };
    if !at.starts_with("127.0.0.1:") { return Err("the leader is not on the loopback address: refused".into()) }
    let my_id = get(&me, "/api/status", None).ok().and_then(|a| serde_json::from_slice::<Value>(&a.body).ok()).and_then(|v| v["node_id"].as_str().map(str::to_string)).unwrap_or_default();
    // the stand-in renderer is this program itself (`fake-render`): no shell, the same on every system
    let work = Path::new(&root).parent().unwrap_or(Path::new(".")).join("render"); let _ = std::fs::create_dir_all(&work); let exe = std::env::current_exe().map_err(|e| e.to_string())?.to_string_lossy().into_owned();
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0); let this_os = if cfg!(windows) { "windows" } else if cfg!(target_os = "macos") { "macos" } else { "linux" };
    let manifest = |id: &str, a: i64, b: i64, chunk: i64, mode: &str, nap: &str, retries: i64| json!({
        "_version": 1, "job_id": id, "template_id": "conformance", "submitted_by": my_id, "submitted_os": this_os, "submitted_at_ms": stamp, "cmd": { "macos": exe, "windows": exe, "linux": exe },
        "flags": [{ "flag": "fake-render", "value": "{chunk_start}" }, { "flag": "", "value": "{chunk_end}" }, { "flag": "", "value": mode }, { "flag": "", "value": nap }, { "flag": "", "value": work.join(id).to_string_lossy() }],
        "frame_start": a, "frame_end": b, "chunk_size": chunk, "max_retries": retries, "timeout_seconds": null, "output_dir": work.join(id).to_string_lossy(),
        "progress": { "patterns": [], "error_patterns": [], "completion_pattern": { "info": "one line per frame", "regex": "Saved: '?(.+)'?" } },
        "output_detection": { "stdout_regex": "Saved: '?([^']+)'?", "path_group": 1, "validation": "exists_nonzero", "info": "" }, "process": { "working_dir": null }, "environment": {}, "tags_required": [] });
    // a job's detail is filed each time what it shows changes: the job's state, each chunk's state, how many frames are done
    fn watch(r: &mut Rec, at: &str, bearer: &str, label: &str, id: &str, secs: u64, done: &dyn Fn(&Value) -> bool) -> Option<Value> {
        let (t, mut last, mut n, mut seen) = (Instant::now(), String::new(), 0, None);
        while t.elapsed() < Duration::from_secs(secs) {
            if let Some(v) = get(at, &format!("/api/jobs/{id}"), Some(bearer)).ok().filter(|a| a.status == 200).and_then(|a| serde_json::from_slice::<Value>(&a.body).ok()) {
                let digest = format!("{}|{}", v["current_state"], v["chunks"].as_array().map(|c| c.iter().map(|x| format!("{}:{}:{}", x["state"].as_str().unwrap_or(""), x["completed_frames"].as_array().map(|f| f.len()).unwrap_or(0), x["retry_count"])).collect::<Vec<_>>().join(",")).unwrap_or_default());
                if digest != last { last = digest; n += 1; r.ask(&format!("{label}-{n:02}-detail"), "leader", at, &format!("/api/jobs/{id}"), "good"); }
                let stop = done(&v); seen = Some(v); if stop { break }
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        seen
    }
    let state = |v: &Value| v["current_state"].as_str().unwrap_or("").to_string();
    let chunk_in = |v: &Value, st: &str| v["chunks"].as_array().and_then(|c| c.iter().find(|x| x["state"] == json!(st)).and_then(|x| x["id"].as_i64()));
    let cmd = |r: &mut Rec, name: &str, method: &str, path: &str, body: Option<String>| r.send(name, "leader", &at, method, path, "good", body.as_deref());
    // -- what a submission may not be
    r.send("submit-no-token", "leader", &at, "POST", "/api/jobs", "none", Some(&json!({ "manifest": manifest("conf-x", 1, 2, 1, "ok", "0.1", 3) }).to_string()));
    cmd(&mut r, "submit-not-json", "POST", "/api/jobs", Some("{ this is not json".into()));
    cmd(&mut r, "submit-without-a-manifest", "POST", "/api/jobs", Some("{}".into()));
    cmd(&mut r, "submit-bad-id", "POST", "/api/jobs", Some(json!({ "manifest": manifest("../climb", 1, 2, 1, "ok", "0.1", 3) }).to_string()));
    // -- A: a job that just renders - 12 frames in chunks of 3
    let a = format!("conf-a-{stamp}");
    cmd(&mut r, "submit-a", "POST", "/api/jobs", Some(json!({ "manifest": manifest(&a, 1, 12, 3, "ok", "0.3", 3), "priority": 50 }).to_string()));
    cmd(&mut r, "submit-a-again", "POST", "/api/jobs", Some(json!({ "manifest": manifest(&a, 1, 12, 3, "ok", "0.3", 3), "priority": 50 }).to_string()));
    watch(&mut r, &at, &bearer, "a", &a, 90, &|v| state(v) == "completed");
    r.ask("jobs-after-a", "leader", &at, "/api/jobs", "good"); r.ask("a-manifest", "leader", &at, &format!("/api/jobs/{a}/manifest"), "good"); r.ask("a-task-output", "leader", &at, &format!("/api/jobs/{a}/task-output"), "good");
    // -- B: a slow job, and every command sent to it - 40 frames in chunks of 5, a second a frame
    let b = format!("conf-b-{stamp}");
    cmd(&mut r, "submit-b", "POST", "/api/jobs", Some(json!({ "manifest": manifest(&b, 1, 40, 5, "ok", "1", 3) }).to_string()));
    watch(&mut r, &at, &bearer, "b-start", &b, 30, &|v| chunk_in(v, "assigned").is_some());
    cmd(&mut r, "cmd-pause", "POST", &format!("/api/jobs/{b}/pause"), Some(String::new()));
    watch(&mut r, &at, &bearer, "b-paused", &b, 12, &|_| false);
    cmd(&mut r, "cmd-resume", "POST", &format!("/api/jobs/{b}/resume"), Some(String::new()));
    let v = watch(&mut r, &at, &bearer, "b-resumed", &b, 20, &|v| chunk_in(v, "assigned").is_some());
    if let Some(id) = v.as_ref().and_then(|v| chunk_in(v, "assigned")) { cmd(&mut r, "cmd-chunk-stop", "POST", "/api/chunks/stop", Some(json!({ "chunk_id": id }).to_string()));
        watch(&mut r, &at, &bearer, "b-chunk-stopped", &b, 6, &|_| false);
        cmd(&mut r, "cmd-chunk-reassign", "POST", "/api/chunks/reassign", Some(json!({ "chunk_id": id }).to_string())); watch(&mut r, &at, &bearer, "b-chunk-requeued", &b, 4, &|_| false); }
    cmd(&mut r, "cmd-chunk-stop-unknown", "POST", "/api/chunks/stop", Some(json!({ "chunk_id": 99999999 }).to_string()));
    cmd(&mut r, "cmd-chunk-stop-no-body", "POST", "/api/chunks/stop", Some(String::new()));
    cmd(&mut r, "cmd-priority", "POST", &format!("/api/jobs/{b}/priority"), Some(json!({ "priority": 10 }).to_string()));
    cmd(&mut r, "cmd-move", "POST", &format!("/api/jobs/{b}/move"), Some(json!({ "target": a, "position": "before" }).to_string()));
    cmd(&mut r, "cmd-move-onto-itself", "POST", &format!("/api/jobs/{b}/move"), Some(json!({ "target": b }).to_string()));
    r.ask("jobs-after-move", "leader", &at, "/api/jobs", "good");
    cmd(&mut r, "cmd-cancel", "POST", &format!("/api/jobs/{b}/cancel"), Some(String::new()));
    watch(&mut r, &at, &bearer, "b-cancelled", &b, 8, &|_| false);
    cmd(&mut r, "cmd-requeue", "POST", &format!("/api/jobs/{b}/requeue"), Some(String::new()));
    watch(&mut r, &at, &bearer, "b-requeued", &b, 8, &|_| false);
    cmd(&mut r, "cmd-cancel-again", "POST", &format!("/api/jobs/{b}/cancel"), Some(String::new()));
    cmd(&mut r, "cmd-archive", "POST", &format!("/api/jobs/{b}/archive"), Some(String::new()));
    r.ask("jobs-after-archive", "leader", &at, "/api/jobs", "good"); r.ask("b-archived-detail", "leader", &at, &format!("/api/jobs/{b}"), "good");
    cmd(&mut r, "cmd-delete", "DELETE", &format!("/api/jobs/{b}"), None);
    r.ask("job-that-does-not-exist", "leader", &at, &format!("/api/jobs/{b}"), "good");
    cmd(&mut r, "cmd-pause-a-job-that-is-gone", "POST", &format!("/api/jobs/{b}/pause"), Some(String::new()));
    // -- C: a job whose renderer fails every time - 4 frames in chunks of 2, two tries
    let c = format!("conf-c-{stamp}");
    cmd(&mut r, "submit-c", "POST", "/api/jobs", Some(json!({ "manifest": manifest(&c, 1, 4, 2, "fail", "0", 2) }).to_string()));
    watch(&mut r, &at, &bearer, "c", &c, 90, &|v| state(v) == "failed" || v["chunks"].as_array().is_some_and(|c| !c.is_empty() && c.iter().all(|x| x["state"] == json!("failed"))));
    r.ask("suspended-after-c", "leader", &at, "/api/nodes/suspended", "good"); r.ask("jobs-after-c", "leader", &at, "/api/jobs", "good");
    cmd(&mut r, "cmd-retry-failed", "POST", &format!("/api/jobs/{c}/retry-failed"), Some(String::new()));
    watch(&mut r, &at, &bearer, "c-retried", &c, 6, &|_| false);
    cmd(&mut r, "cmd-delete-c", "DELETE", &format!("/api/jobs/{c}"), None); cmd(&mut r, "cmd-delete-a", "DELETE", &format!("/api/jobs/{a}"), None);
    r.ask("jobs-at-the-end", "leader", &at, "/api/jobs", "good");
    println!("{} requests recorded on the private farm in {}", r.n, out.display());
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------- leading
/// A stand-in renderer with no shell in it, so that it is the same on every system:
///   mr-conform fake-render <first> <last> <ok|fail> <seconds a frame> <folder>
/// It writes `<folder>/<pid>.renderer`, starts one helper (which writes `<pid>.helper` and sleeps), prints one
/// `Saved:` line a frame, and ends with 0 - or, for `fail`, prints a complaint and ends with 3.
fn fake_render(args: &[String]) -> ! {
    let n = |i: usize| args.get(i).cloned().unwrap_or_default(); let dir = n(5);
    if n(0) == "fake-helper" { let _ = std::fs::write(Path::new(&n(1)).join(format!("{}.helper", std::process::id())), b""); std::thread::sleep(Duration::from_secs(300)); std::process::exit(0) }
    let _ = std::fs::create_dir_all(&dir); let _ = std::fs::write(Path::new(&dir).join(format!("{}.renderer", std::process::id())), b"");
    let _ = std::process::Command::new(std::env::current_exe().unwrap()).args(["fake-helper", &dir]).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn();
    if n(3) == "fail" { eprintln!("Unable to open the scene"); std::process::exit(3) }
    let (a, b, nap) = (n(1).parse::<i64>().unwrap_or(1), n(2).parse::<i64>().unwrap_or(1), n(4).parse::<f64>().unwrap_or(0.5));
    for f in a..=b { std::thread::sleep(Duration::from_secs_f64(nap)); println!("Saved: '{dir}/f_{f:04}.exr'"); let _ = std::io::stdout().flush(); }
    std::process::exit(0)
}
fn alive(pid: u32) -> bool {
    if cfg!(windows) { std::process::Command::new("tasklist").args(["/FI", &format!("PID eq {pid}"), "/NH"]).output().map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string())).unwrap_or(false) }
    else { std::process::Command::new("ps").args(["-o", "stat=", "-p", &pid.to_string()]).output().map(|o| { let t = String::from_utf8_lossy(&o.stdout); let t = t.trim(); !t.is_empty() && !t.starts_with('Z') }).unwrap_or(false) }
}
/// Part 3: the harness LEADS a private farm of one worker, to see the wire as a worker sends it. It writes itself into
/// the phonebook with the tag `leader`, answers the routes a worker calls, files every request that reaches it, and
/// puts the worker through seven scenarios. Refuses any share that is not under a folder named `mr-farm`.
///   mr-conform lead --share <sync root> --port <N> --out <dir>
fn lead(args: &[String]) -> Result<(), String> {
    use std::sync::{Arc, Mutex};
    let share = arg(args, "--share").ok_or("--share <sync root of a private farm>")?; let port: u16 = arg(args, "--port").and_then(|p| p.parse().ok()).ok_or("--port <N>")?; let out = PathBuf::from(arg(args, "--out").ok_or("--out <dir>")?);
    if !share.replace('\\', "/").contains("/mr-farm/") { return Err("this is not a private farm (a share under a folder named mr-farm): refused".into()) }
    let farm = Path::new(&share).join("MinRender-v2"); let secret = serde_json::from_str::<Value>(&std::fs::read_to_string(farm.join("farm.json")).map_err(|e| format!("farm.json: {e}"))?).ok().and_then(|v| v["api_secret"].as_str().map(str::to_string)).ok_or("no secret")?;
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?; let me_id = "000000lead00".to_string(); let bearer = format!("Bearer {secret}");
    let my_dir = farm.join("nodes").join(&me_id); std::fs::create_dir_all(&my_dir).map_err(|e| e.to_string())?;
    let stamp = || std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    std::fs::write(my_dir.join("endpoint.json"), serde_json::to_string_pretty(&json!({ "node_id": me_id, "ip": "127.0.0.1", "port": port, "timestamp_ms": stamp() })).unwrap()).map_err(|e| e.to_string())?;
    // what the stand-in leader holds: the jobs it lists, whether frame reports are called stale, everything it was sent
    struct Lead { jobs: Vec<Value>, stale: bool, got: Vec<(String, String, Value)>, n: usize }
    let st = Arc::new(Mutex::new(Lead { jobs: Vec::new(), stale: false, got: Vec::new(), n: 0 }));
    let listener = std::net::TcpListener::bind(("127.0.0.1", port)).map_err(|e| format!("port {port}: {e}"))?;
    { let (st, out, bearer, me_id) = (st.clone(), out.clone(), bearer.clone(), me_id.clone()); std::thread::spawn(move || for mut s in listener.incoming().flatten() {
        let _ = s.set_read_timeout(Some(Duration::from_secs(5))); let (mut buf, mut tmp) = (Vec::new(), [0u8; 8192]);
        let cut = loop { if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") { break Some(i) } match s.read(&mut tmp) { Ok(0) | Err(_) => break None, Ok(n) => buf.extend_from_slice(&tmp[..n]) } };
        let Some(cut) = cut else { continue }; let head = String::from_utf8_lossy(&buf[..cut]).into_owned(); let mut lines = head.split("\r\n");
        let mut first = lines.next().unwrap_or("").split(' '); let (method, path) = (first.next().unwrap_or("").to_string(), first.next().unwrap_or("").to_string());
        let headers: Vec<(String, String)> = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
        let len: usize = headers.iter().find(|(k, _)| k == "content-length").and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
        let mut body = buf[cut + 4..].to_vec(); while body.len() < len { match s.read(&mut tmp) { Ok(0) | Err(_) => break, Ok(n) => body.extend_from_slice(&tmp[..n]) } }
        let auth = match headers.iter().find(|(k, _)| k == "authorization") { None => "none", Some((_, v)) if *v == bearer => "good", Some(_) => "wrong" };
        let text = String::from_utf8_lossy(&body).into_owned(); let parsed: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        let mut g = st.lock().unwrap();
        let reply = match (method.as_str(), path.as_str()) {
            ("GET", "/api/status") => json!({ "node_id": me_id, "hostname": "conformance", "os": if cfg!(windows) { "windows" } else { "macos" }, "app_version": "0.5.11", "gpu_name": "", "cpu_cores": 1, "ram_mb": 1024, "node_state": "stopped", "render_state": "idle", "active_job": "", "active_chunk": "", "priority": 100,
                "tags": ["leader"], "capabilities": ["job_edit", "edit_result", "edit_epoch", "edit_commands", "chunk_stop"], "endpoint": format!("127.0.0.1:{port}"), "secret_fingerprint": "", "agent_health": "ok", "alert_reason": "", "ready_for_work": false,
                "is_local": true, "is_alive": true, "is_leader": false, "last_seen_ms": 0, "has_udp_contact": false, "failed_polls": 0 }).to_string(),
            ("GET", "/api/jobs") => Value::Array(g.jobs.clone()).to_string(),
            ("GET", "/api/nodes/suspended") => "[]".to_string(),
            ("POST", "/api/dispatch/frame-complete") if g.stale => json!({ "status": "stale", "stale": true }).to_string(),
            _ => json!({ "status": "ok" }).to_string() };
        // the polls are counted, not filed one by one; everything else is filed
        if method == "GET" { let k = format!("GET {path}"); if !g.got.iter().any(|(m, _, _)| *m == k) { g.got.push((k, auth.to_string(), json!({ "headers": headers.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>() }))); } }
        else { g.n += 1; let _ = std::fs::write(out.join(format!("in-{:03}{}.json", g.n, path.replace('/', "-"))), serde_json::to_string_pretty(&json!({ "method": method, "path": path, "auth": auth, "headers": headers.iter().filter(|(k, _)| k != "authorization").map(|(k, v)| json!([k, v])).collect::<Vec<_>>(), "body": parsed.clone() })).unwrap());
            g.got.push((format!("{method} {path}"), auth.to_string(), parsed)); }
        drop(g);
        let _ = s.write_all(format!("HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{reply}", reply.len()).as_bytes()); let _ = s.flush();
    }); }
    // the worker: the one other node in the phonebook; wait until it holds this stand-in for the leader
    let worker = (0..120).find_map(|_| { std::thread::sleep(Duration::from_millis(500));
        std::fs::read_dir(farm.join("nodes")).ok()?.flatten().filter(|d| d.file_name() != me_id.as_str()).find_map(|d| { let v: Value = serde_json::from_str(&std::fs::read_to_string(d.path().join("endpoint.json")).ok()?).ok()?; let e = format!("{}:{}", v["ip"].as_str()?, v["port"].as_u64()?);
            let peers: Value = serde_json::from_slice(&http(&e, "GET", "/api/peers", Some(&bearer), None).ok()?.body).ok()?;
            peers.as_array()?.iter().any(|p| p["node_id"] == json!(me_id) && p["is_leader"] == json!(true) && p["is_alive"] == json!(true)).then_some(e) }) }).ok_or("no worker took this stand-in for its leader within a minute")?;
    println!("the worker at {worker} follows the stand-in leader");
    let exe = std::env::current_exe().map_err(|e| e.to_string())?.to_string_lossy().into_owned(); let pids = Path::new(&share).parent().unwrap_or(Path::new(".")).join("pids-lead"); let _ = std::fs::remove_dir_all(&pids);
    let manifest = |id: &str, mode: &str, nap: &str| json!({ "_version": 1, "job_id": id, "template_id": "conformance", "submitted_by": me_id, "submitted_os": if cfg!(windows) { "windows" } else { "macos" }, "submitted_at_ms": stamp(),
        "cmd": { "macos": exe, "windows": exe, "linux": exe }, "flags": [{ "flag": "fake-render", "value": "{chunk_start}" }, { "flag": "", "value": "{chunk_end}" }, { "flag": "", "value": mode }, { "flag": "", "value": nap }, { "flag": "", "value": pids.to_string_lossy() }],
        "frame_start": 1, "frame_end": 1000, "chunk_size": 10, "max_retries": 3, "timeout_seconds": null, "output_dir": null, "progress": { "patterns": [], "error_patterns": [], "completion_pattern": { "regex": "Saved: '?(.+)'?" } },
        "output_detection": { "stdout_regex": null, "path_group": 1, "validation": "exists_nonzero", "info": "" }, "process": { "working_dir": null }, "environment": {}, "tags_required": [] });
    let summary = |id: &str, state: &str| json!({ "job_id": id, "template_id": "conformance", "current_state": state, "priority": 50, "submitted_at_ms": 0, "submitted_by": me_id, "frame_start": 1, "frame_end": 1000, "chunk_size": 10, "total_chunks": 100,
        "completed_chunks": 0, "failed_chunks": 0, "rendering_chunks": 1, "pending_chunks": 99, "stopped_chunks": 0, "first_assigned_ms": 0, "last_completed_ms": 0 });
    let send = |name: &str, path: &str, body: Value| -> u16 { let a = http(&worker, "POST", path, Some(&bearer), Some(&body.to_string())); let (s, b) = a.map(|a| (a.status, String::from_utf8_lossy(&a.body).into_owned())).unwrap_or((0, String::new()));
        println!("  -> {name}: {s} {b}"); let _ = std::fs::write(out.join(format!("out-{name}.json")), serde_json::to_string_pretty(&json!({ "path": path, "status": s, "answer": b })).unwrap()); s };
    let state_of = || serde_json::from_slice::<Value>(&http(&worker, "GET", "/api/status", None, None).map(|a| a.body).unwrap_or_default()).unwrap_or(Value::Null);
    let wait = |what: &dyn Fn() -> bool, secs: u64| -> Option<f64> { let t = Instant::now(); while t.elapsed() < Duration::from_secs(secs) { if what() { return Some(t.elapsed().as_secs_f64()) } std::thread::sleep(Duration::from_millis(100)); } None };
    let rendering = || state_of()["render_state"] == json!("rendering"); let idle = || { let s = state_of(); s["render_state"] == json!("idle") && s["ready_for_work"] == json!(true) };
    let left = || std::fs::read_dir(&pids).map(|d| d.flatten().filter_map(|e| e.file_name().to_string_lossy().split_once('.').and_then(|(p, _)| p.parse::<u32>().ok())).filter(|p| alive(*p)).count()).unwrap_or(0);
    let mut report = Vec::<String>::new();
    let mut scenario = |name: &str, mode: &str, nap: &str, first: i64, last: i64, epoch: Option<i64>, act: &dyn Fn(&str)| {
        let id = format!("lead-{name}"); let mark = st.lock().unwrap().got.len(); { let mut g = st.lock().unwrap(); g.jobs = vec![summary(&id, "active")]; g.stale = false; }
        println!("{name}"); let mut body = json!({ "manifest": manifest(&id, mode, nap), "frame_start": first, "frame_end": last }); if let Some(e) = epoch { body["edit_epoch"] = json!(e); }
        send(&format!("{name}-assign"), "/api/dispatch/assign", body);
        let began = wait(&rendering, 15); act(&id); let ended = wait(&idle, 40); std::thread::sleep(Duration::from_millis(2500));
        let g = st.lock().unwrap(); let sent: Vec<String> = g.got[mark..].iter().filter(|(m, _, _)| m.starts_with("POST")).map(|(m, auth, b)| { let mut keys: Vec<String> = b.as_object().map(|o| o.iter().map(|(k, v)| format!("{k}:{}", match v { Value::Array(a) => format!("[{}]", a.len()), Value::String(s) if k == "error" => format!("\"{s}\""), Value::Number(n) if k != "frame_start" && k != "frame_end" => n.to_string(), v => kind(v).to_string() })).collect()).unwrap_or_default(); keys.sort();
            format!("{} ({auth}) {{{}}}", m.trim_start_matches("POST /api/dispatch/"), keys.join(", ")) }).collect();
        drop(g);
        let line = format!("| {name} | {} | {} | {} | {} |", began.map(|t| format!("{t:.1} s")).unwrap_or("never".into()), ended.map(|t| format!("{t:.1} s")).unwrap_or("never".into()), if sent.is_empty() { "nothing".to_string() } else { sent.join("<br>") }, left());
        println!("  {line}"); report.push(line);
    };
    let pause = |s: f64| std::thread::sleep(Duration::from_secs_f64(s));
    scenario("renders", "ok", "0.4", 1, 4, Some(0), &|_| {});
    scenario("renders-no-epoch", "ok", "0.4", 5, 6, None, &|_| {});
    scenario("fails", "fail", "0", 1, 2, Some(0), &|_| {});
    scenario("aborted", "ok", "1", 1, 30, Some(0), &|id| { pause(3.2); send("aborted-abort", "/api/render/abort", json!({ "job_id": id, "reason": "chunk stopped" })); });
    scenario("cancelled-in-the-list", "ok", "1", 1, 30, Some(0), &|id| { pause(3.2); st.lock().unwrap().jobs = vec![summary(id, "cancelled")]; });
    scenario("gone-from-the-list", "ok", "1", 1, 30, Some(0), &|_| { pause(3.2); st.lock().unwrap().jobs.clear(); });
    scenario("called-stale", "ok", "1", 1, 30, Some(4), &|_| { pause(1.0); st.lock().unwrap().stale = true; });
    scenario("busy-and-bad", "ok", "1", 1, 6, Some(0), &|id| { pause(1.5); send("busy-second-assign", "/api/dispatch/assign", json!({ "manifest": manifest(id, "ok", "1"), "frame_start": 7, "frame_end": 8 }));
        send("bad-id-assign", "/api/dispatch/assign", json!({ "manifest": manifest("../climb", "ok", "1"), "frame_start": 1, "frame_end": 1 })); send("not-json-assign", "/api/dispatch/assign", json!("nothing")); send("abort-other-job", "/api/render/abort", json!({ "job_id": "another" })); });
    scenario("node-stopped", "ok", "1", 1, 30, Some(0), &|_| { pause(3.2); send("node-stop", "/api/node/stop", json!({})); });
    send("node-start", "/api/node/start", json!({}));
    let g = st.lock().unwrap(); let polls: Vec<String> = g.got.iter().filter(|(m, _, _)| m.starts_with("GET")).map(|(m, a, _)| format!("{m} ({a})")).collect(); drop(g);
    let text = format!("| scenario | rendering after | idle after | what the worker sent the leader | processes left |\n|---|---|---|---|---|\n{}\n\nThe worker's polls of its leader: {}\n", report.join("\n"), polls.join(", "));
    let _ = std::fs::write(out.join("table.md"), &text); println!("\n{text}");
    let _ = std::fs::remove_file(my_dir.join("endpoint.json"));
    Ok(())
}

// --------------------------------------------------------------------------------------------------------- checking
/// A key's type in the contract. `Opt`: may be absent.
#[derive(Clone, Copy)]
enum T { Str, Int, UInt, Bool, StrArr, IntArr, Obj, Arr }
fn is(v: &Value, t: T) -> bool { match t { T::Str => v.is_string(), T::Int => v.is_i64() || v.is_u64(), T::UInt => v.is_u64(), T::Bool => v.is_boolean(),
    T::StrArr => v.as_array().is_some_and(|a| a.iter().all(Value::is_string)), T::IntArr => v.as_array().is_some_and(|a| a.iter().all(|x| x.is_i64() || x.is_u64())), T::Obj => v.is_object(), T::Arr => v.is_array() } }
fn kind(v: &Value) -> &'static str { match v { Value::Null => "null", Value::Bool(_) => "boolean", Value::Number(n) => if n.is_f64() { "float" } else { "integer" }, Value::String(_) => "string", Value::Array(_) => "array", Value::Object(_) => "object" } }
struct Out { bad: usize, notes: usize }
impl Out {
    fn bad(&mut self, what: &str, msg: String) { self.bad += 1; println!("CONTRADICTS  {what}: {msg}"); }
    fn note(&mut self, what: &str, msg: String) { self.notes += 1; println!("note         {what}: {msg}"); }
    /// An object against a table of (key, type, required). A key the contract does not know is a note, not a fault.
    fn object(&mut self, what: &str, v: &Value, table: &[(&str, T, bool)]) {
        let Some(o) = v.as_object() else { self.bad(what, format!("is a {}, not an object", kind(v))); return };
        for (k, t, req) in table { match o.get(*k) { None => if *req { self.bad(what, format!("key `{k}` is missing")) }, Some(x) => if !is(x, *t) { self.bad(what, format!("key `{k}` is a {}", kind(x))) } } }
        for k in o.keys() { if !table.iter().any(|(n, _, _)| n == k) { self.note(what, format!("key `{k}` ({}) is not in the contract", kind(&o[k]))); } }
    }
}
const PEER: &[(&str, T, bool)] = &[("node_id", T::Str, true), ("hostname", T::Str, true), ("os", T::Str, true), ("app_version", T::Str, true), ("gpu_name", T::Str, true), ("cpu_cores", T::Int, true), ("ram_mb", T::UInt, true),
    ("node_state", T::Str, true), ("render_state", T::Str, true), ("active_job", T::Str, true), ("active_chunk", T::Str, true), ("priority", T::Int, true), ("tags", T::StrArr, true), ("capabilities", T::StrArr, false),
    ("endpoint", T::Str, true), ("secret_fingerprint", T::Str, true), ("agent_health", T::Str, true), ("alert_reason", T::Str, true), ("ready_for_work", T::Bool, true), ("is_local", T::Bool, true), ("is_alive", T::Bool, true),
    ("is_leader", T::Bool, true), ("last_seen_ms", T::Int, true), ("has_udp_contact", T::Bool, true), ("failed_polls", T::Int, true)];
const SUMMARY: &[(&str, T, bool)] = &[("job_id", T::Str, true), ("template_id", T::Str, true), ("current_state", T::Str, true), ("priority", T::Int, true), ("submitted_at_ms", T::Int, true), ("submitted_by", T::Str, true),
    ("frame_start", T::Int, true), ("frame_end", T::Int, true), ("chunk_size", T::Int, true), ("total_chunks", T::Int, true), ("completed_chunks", T::Int, true), ("failed_chunks", T::Int, true), ("rendering_chunks", T::Int, true),
    ("pending_chunks", T::Int, true), ("stopped_chunks", T::Int, true), ("first_assigned_ms", T::Int, true), ("last_completed_ms", T::Int, true), ("output_dir", T::Str, false), ("output_stem", T::Str, false)];
const DETAIL: &[(&str, T, bool)] = &[("job_id", T::Str, true), ("current_state", T::Str, true), ("priority", T::Int, true), ("submitted_at_ms", T::Int, true), ("manifest", T::Obj, true), ("chunks", T::Arr, true)];
const CHUNK: &[(&str, T, bool)] = &[("id", T::Int, true), ("frame_start", T::Int, true), ("frame_end", T::Int, true), ("state", T::Str, true), ("assigned_to", T::Str, true), ("assigned_at_ms", T::Int, true), ("completed_at_ms", T::Int, true),
    ("retry_count", T::Int, true), ("completed_frames", T::IntArr, true), ("failed_on", T::StrArr, true)];
const SUSPENDED: &[(&str, T, bool)] = &[("node_id", T::Str, true), ("failure_count", T::Int, true), ("first_failure_ms", T::Int, true), ("last_failure_ms", T::Int, true)];
const TASK_OUTPUT: &[(&str, T, bool)] = &[("node_id", T::Str, true), ("range", T::Str, true), ("timestamp_ms", T::Int, true), ("path", T::Str, true)];
const ENDPOINT: &[(&str, T, bool)] = &[("node_id", T::Str, true), ("ip", T::Str, true), ("port", T::Int, true), ("timestamp_ms", T::Int, true)];
const FARM: &[(&str, T, bool)] = &[("_version", T::Int, true), ("protocol_version", T::Int, true), ("created_by", T::Str, true), ("created_at_ms", T::Int, true), ("last_example_update", T::Str, true), ("api_secret", T::Str, true)];

fn check(dir: &Path) -> Result<bool, String> {
    let mut o = Out { bad: 0, notes: 0 }; let mut seen = 0;
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json") && p.file_name().is_some_and(|n| n != "about.json")).collect(); files.sort();
    let one_of = |o: &mut Out, what: &str, v: &Value, key: &str, allowed: &[&str]| { if let Some(s) = v[key].as_str() { if !allowed.contains(&s) { o.bad(what, format!("`{key}` has a value outside the contract's set {allowed:?}")); } } };
    for f in files {
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&f).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?; seen += 1;
        let name = v["name"].as_str().unwrap_or("?"); let what = format!("{} [{}]", name, v["target"].as_str().unwrap_or("?").split(':').next().unwrap_or("?"));
        if v.get("error").is_some() { o.note(&what, "no answer".into()); continue }
        let status = v["status"].as_u64().unwrap_or(0); let text = v["body"].as_str().unwrap_or(""); let body: Option<Value> = serde_json::from_str(text).ok();
        let ctype = v["headers"].as_array().and_then(|h| h.iter().find(|p| p[0] == json!("content-type")).and_then(|p| p[1].as_str().map(str::to_string))).unwrap_or_default();
        let want = |o: &mut Out, s: u64| { if status != s { o.bad(&what, format!("status {status}, the contract says {s}")); } status == s };
        let err = |o: &mut Out, s: u64, e: &str| { if status == s { if body.as_ref().map(|b| b["error"] == json!(e)) != Some(true) { o.bad(&what, format!("status {s} without {{\"error\":\"{e}\"}}")); } if !ctype.starts_with("application/json") { o.bad(&what, format!("content type `{ctype}`")); } true } else { false } };
        let not_leader = |o: &mut Out| { if err(o, 503, "not_leader") { if let Some(b) = &body { if let Some(x) = b.get("leader_endpoint") { if !x.is_string() { o.bad(&what, "`leader_endpoint` is not a string".into()); } } else { o.note(&what, "a follower's refusal names no leader".into()); } } true } else { false } };
        let json_ok = |o: &mut Out| { if !ctype.starts_with("application/json") { o.bad(&what, format!("content type `{ctype}`")); } if body.is_none() && !text.is_empty() { o.bad(&what, "the body is not JSON".into()); } };
        match name {
            "status" | "status-with-a-wrong-token" => if want(&mut o, 200) { json_ok(&mut o); if let Some(b) = &body { o.object(&what, b, PEER); one_of(&mut o, &what, b, "os", &["windows", "macos", "linux"]); one_of(&mut o, &what, b, "node_state", &["active", "stopped"]); one_of(&mut o, &what, b, "render_state", &["idle", "rendering"]); one_of(&mut o, &what, b, "agent_health", &["ok", "needs_attention"]);
                if b["is_leader"] != json!(false) { o.bad(&what, "`is_leader` is not false in a node's own record".into()); } if b["node_id"].as_str().is_some_and(|s| s.len() != 12) { o.bad(&what, "`node_id` is not 12 characters".into()); }
                if b["secret_fingerprint"].as_str().is_some_and(|s| !(s.len() == 8 || s.is_empty())) { o.bad(&what, "`secret_fingerprint` is not 8 characters".into()); } } },
            n if n.starts_with("status-of-peer-") => if status == 200 { if let Some(b) = &body { o.object(&what, b, PEER); } },
            "peers" => if want(&mut o, 200) { json_ok(&mut o); match body.as_ref().and_then(|b| b.as_array()) { Some(a) => for p in a { o.object(&what, p, PEER); }, None => o.bad(&what, "not an array".into()) } },
            "peers-auth-none" | "peers-auth-wrong" | "peers-auth-lowercase" | "peers-auth-twospaces" | "peers-auth-basic" | "unknown-api-path-without-token" => { if !err(&mut o, 401, "unauthorized") { o.bad(&what, format!("status {status}, the contract says 401")); } },
            "unknown-api-path-with-token" | "unknown-path-outside-api" => if want(&mut o, 404) && !text.is_empty() { o.bad(&what, "a 404 for an unknown path has a body".into()); },
            "root" => o.note(&what, format!("status {status}, {} bytes (the contract does not say)", text.len())),
            "config" => if want(&mut o, 200) { json_ok(&mut o); if let Some(b) = &body { for k in ["sync_root", "http_port", "node_id", "hostname", "cpu_cores", "ram_mb", "gpu_name", "farm_running", "farm_path", "farm_error", "is_leader"] { if b.get(k).is_none() { o.bad(&what, format!("key `{k}` is missing")); } } } },
            "node-log" => if want(&mut o, 200) { json_ok(&mut o); if !body.as_ref().is_some_and(|b| is(b, T::StrArr)) { o.bad(&what, "not an array of strings".into()); } },
            "task-output-of-a-path-that-climbs" => { if !err(&mut o, 400, "invalid_path") { o.bad(&what, format!("status {status}, the contract says 400 invalid_path")); } },
            "submit-no-token" => { if !err(&mut o, 401, "unauthorized") { o.bad(&what, format!("status {status}, the contract says 401")); } },
            "submit-bad-id" => { if !err(&mut o, 400, "invalid_job_id") { o.bad(&what, format!("status {status}, the contract says 400 invalid_job_id")); } },
            "submit-not-json" | "submit-without-a-manifest" | "cmd-chunk-stop-no-body" => { if status != 400 || body.as_ref().map(|b| b["error"].is_string()) != Some(true) { o.bad(&what, format!("status {status}, the contract says 400 with an error text")); } },
            "cmd-move-onto-itself" => { if !err(&mut o, 409, "move_rejected") { o.bad(&what, format!("status {status}, the contract says 409 move_rejected")); } },
            n if n.starts_with("submit-") || n.starts_with("cmd-") => if want(&mut o, 200) { json_ok(&mut o); if body.as_ref().map(|b| b["status"] == json!("ok")) != Some(true) { o.bad(&what, "the answer is not {\"status\":\"ok\"}".into()); } },
            n if n.starts_with("suspended") && n != "suspended-on-a-follower" => if want(&mut o, 200) { json_ok(&mut o); match body.as_ref().and_then(|b| b.as_array()) { Some(a) => for s in a { o.object(&what, s, SUSPENDED); }, None => o.bad(&what, "not an array".into()) } },
            n if n == "jobs" || n.starts_with("jobs-") => { if !not_leader(&mut o) && want(&mut o, 200) { if text.is_empty() { o.note(&what, "an empty body (the leader had not refreshed yet)".into()); } else { json_ok(&mut o); match body.as_ref().and_then(|b| b.as_array()) { Some(a) => for j in a { o.object(&what, j, SUMMARY); one_of(&mut o, &what, j, "current_state", &["active", "paused", "cancelled", "completed", "failed"]); }, None => o.bad(&what, "not an array".into()) } } } },
            "suspended-on-a-follower" | "job-detail-on-a-follower" => { if !not_leader(&mut o) { o.bad(&what, format!("status {status}, the contract says 503 not_leader")); } },
            "job-that-does-not-exist" | "manifest-of-a-job-that-does-not-exist" => { if !err(&mut o, 404, "not_found") { o.bad(&what, format!("status {status}, the contract says 404 not_found")); } },
            n if n.ends_with("-detail") => if want(&mut o, 200) { json_ok(&mut o); if let Some(b) = &body { o.object(&what, b, DETAIL); if let Some(cs) = b["chunks"].as_array() { for (i, c) in cs.iter().enumerate() { if i < 3 || i + 1 == cs.len() { o.object(&what, c, CHUNK); } one_of(&mut o, &what, c, "state", &["pending", "assigned", "completed", "failed", "stopped", "partial"]); }
                if !cs.windows(2).all(|w| w[0]["frame_start"].as_i64() <= w[1]["frame_start"].as_i64()) { o.bad(&what, "chunks are not ordered by frame_start".into()); } } } },
            n if n.ends_with("-manifest") => if want(&mut o, 200) { json_ok(&mut o); if !body.as_ref().is_some_and(|b| b["manifest"].is_object()) { o.bad(&what, "no `manifest` object".into()); } else if let Some(m) = body.as_ref().and_then(|b| b["manifest"].as_object()) { o.note(&what, format!("manifest keys: {}", m.keys().cloned().collect::<Vec<_>>().join(", "))); } },
            n if n.ends_with("-task-output") => if want(&mut o, 200) { json_ok(&mut o); match body.as_ref().and_then(|b| b.as_array()) { Some(a) => for t in a.iter().take(3) { o.object(&what, t, TASK_OUTPUT); }, None => o.bad(&what, "not an array".into()) } },
            n if n.ends_with("-task-output-file") => if want(&mut o, 200) && !ctype.starts_with("text/plain") { o.bad(&what, format!("content type `{ctype}`, the contract says text/plain")); },
            _ => o.note(&what, format!("status {status}: no rule for this request")),
        }
        // every answer: the headers a peer could come to depend on
        let hs: Vec<String> = v["headers"].as_array().map(|h| h.iter().filter_map(|p| p[0].as_str().map(str::to_string)).collect()).unwrap_or_default();
        if name == "status" { o.note(&what, format!("response headers: {}", hs.join(", "))); }
    }
    // the share's files
    if let Ok(t) = std::fs::read_to_string(dir.join("share/farm.json")) { if let Ok(v) = serde_json::from_str::<Value>(&t) { seen += 1; o.object("share farm.json", &v, FARM);
        if v["_version"] != json!(1) { o.bad("share farm.json", "`_version` is not 1".into()); } if v["protocol_version"] != json!(2) { o.bad("share farm.json", "`protocol_version` is not 2".into()); }
        if !v["api_secret"].as_str().is_some_and(|s| s.contains("64 characters, lowercase hex: true")) { o.note("share farm.json", "the secret is not 64 lowercase hex characters".into()); } } }
    if let Ok(rd) = std::fs::read_dir(dir.join("share")) { for e in rd.flatten() { let n = e.file_name().to_string_lossy().into_owned(); if let Some(id) = n.strip_prefix("endpoint-").and_then(|x| x.strip_suffix(".json")) {
        if let Ok(v) = std::fs::read_to_string(e.path()).map_err(|_| ()).and_then(|t| serde_json::from_str::<Value>(&t).map_err(|_| ())) { seen += 1; o.object("share endpoint.json", &v, ENDPOINT); if v["node_id"].as_str() != Some(id) { o.bad("share endpoint.json", "`node_id` is not the folder's name".into()); } } } } }
    println!("{seen} answers and files checked: {} contradict the contract, {} notes", o.bad, o.notes);
    Ok(o.bad == 0)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.first().map(|s| s.as_str()) {
        Some("record") => record(&args).map(|_| true),
        Some("fake-render") | Some("fake-helper") => fake_render(&args),
        Some("lead") => lead(&args).map(|_| true),
        // a private farm's folder with nothing in it but a secret, for a machine that has no 0.5.x build to make one
        Some("make-farm") => (|| { let share = arg(&args, "--share").ok_or("--share <dir>")?; if !share.replace('\\', "/").contains("/mr-farm/") { return Err("not under a folder named mr-farm: refused".to_string()) }
            let farm = Path::new(&share).join("MinRender-v2"); std::fs::create_dir_all(farm.join("nodes")).map_err(|e| e.to_string())?;
            let seed = format!("{:?}{}", std::time::SystemTime::now(), std::process::id()); let mut h: u64 = 0xcbf29ce484222325; let mut secret = String::new();
            for i in 0..4u64 { for b in seed.bytes().chain(i.to_le_bytes()) { h = (h ^ b as u64).wrapping_mul(0x100000001b3); } secret.push_str(&format!("{h:016x}")); }
            std::fs::write(farm.join("farm.json"), serde_json::to_string_pretty(&json!({ "_version": 1, "protocol_version": 2, "created_by": "conformance", "created_at_ms": 0, "last_example_update": "", "api_secret": secret })).unwrap()).map_err(|e| e.to_string())?; Ok(true) })(),
        Some("drive") => drive(&args).map(|_| true),
        Some("check") => match args.get(1) { Some(d) => check(Path::new(d)), None => Err("check <dir>".into()) },
        _ => Err("mr-conform record --config <config.json> --out <dir> [--max-jobs N] | drive --config <a private farm node's config.json> --out <dir> | check <dir>".into()) };
    match r { Ok(true) => {} Ok(false) => std::process::exit(1), Err(e) => { eprintln!("mr-conform: {e}"); std::process::exit(2) } }
}
