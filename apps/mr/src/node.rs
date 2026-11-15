//! The node: its identity, its place in the farm's phonebook, what it answers, whom it believes alive and leading,
//! and the one chunk it renders at a time. Every rule here is a line of docs/phase4/step0-contracts.md (section
//! numbers in the comments) or something `tools/mr-conform` saw a 0.5.11 node do (step2-conformance.md).
//!
//! It follows or leads as the election says: what only a leader holds and does is `lead.rs`.
use crate::http::{self, Request, Response};
use crate::lead::{Candidate, Effect, Lead};
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const OS: &str = if cfg!(windows) { "windows" } else if cfg!(target_os = "macos") { "macos" } else { "linux" };
/// What this build says it can do (2.4): the same five words 0.5.11 sends - every one is honoured below or is the leader's.
const CAPABILITIES: [&str; 5] = ["job_edit", "edit_result", "edit_epoch", "edit_commands", "chunk_stop"];
pub fn now_ms() -> u64 { SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0) }
fn log(msg: impl AsRef<str>) { crate::share::log("INFO", "farm", msg.as_ref()); }

/// A peer as this node knows it (1.2, 1.5).
struct Peer { endpoint: String, record: Option<Value>, alive: bool, failed: u32, last_seen_ms: u64, leader: bool, /// (modified, size) of its endpoint.json at the last good read
    stamp: Option<(u128, u64)>, /// the `timestamp_ms` of that read, and how often the listing changed since with the same one (a cached listing: 1.2)
    written_ms: i64, same: u8, /// its entry is not looked at again before this
    wait_until: Option<Instant>, /// the last heartbeat heard from it (1.6)
    heard: Option<Instant>, /// the round trip of its polls, smoothed (1.5)
    rtt_ms: Option<f64> }
impl Peer { fn udp(&self) -> bool { self.heard.is_some_and(|t| t.elapsed() < Duration::from_secs(15)) } }
/// A chunk a leader gave this node (2.5 row 7).
struct Assignment { manifest: Value, start: i64, end: i64, epoch: Option<i64> }
/// How a render that was cut short is told to the leader (4.10): not at all, or as a failure.
#[derive(Clone, Copy, PartialEq)]
enum Cut { Abandon, Fail }
struct Running { job: String, start: i64, end: i64, cut: Option<Cut>, /// closing it tells the leash to kill
    leash_in: Option<std::process::ChildStdin>, group: Option<u32> }
#[derive(Default)]
struct Work { queue: VecDeque<Assignment>, running: Option<Running>, /// from an accepted dispatch until the node is idle again
    busy: bool }

pub struct Node {
    pub id: String, data: PathBuf, farm: PathBuf, secret: String, port: u16, ip: String, tags: Vec<String>, hostname: String, cores: usize, ram_mb: u64,
    stopped: AtomicBool, pub stop: Arc<AtomicBool>, peers: Mutex<BTreeMap<String, Peer>>, work: Mutex<Work>, /// reports to the leader, in order: (path, body)
    reports: Mutex<VecDeque<(String, String)>>, wrote_endpoint: Mutex<Option<Instant>>, /// the database and the queue, while this node leads
    lead: Mutex<Option<Lead>>, /// renders go to a folder of this machine first (4.11; read at the farm's start)
    staging: bool, /// the heartbeat's socket and port, when heartbeats are on (1.6)
    udp: Option<(std::net::UdpSocket, u16)>, /// submission files that were dealt with and could not be moved away
    submitted: Mutex<std::collections::BTreeSet<String>>, /// this machine's table of shared roots (5.3)
    mappings: Vec<min_paths::map::Mapping>,
}

fn fingerprint(secret: &str) -> String { use sha2::Digest; if secret.is_empty() { return String::new() } sha2::Sha256::digest(secret.as_bytes()).iter().take(4).map(|b| format!("{b:02x}")).collect() }
/// A name that may be one step of a path on the share (the rule 0.5.x applies to every id that becomes one).
fn safe(s: &str) -> bool { !s.is_empty() && s != "." && s != ".." && !s.contains(['/', '\\', ':']) }
/// An id that is one plain path segment (it names a folder on the share).
fn plain_segment(s: &str) -> bool { !s.is_empty() && s != "." && s != ".." && !s.contains(['/', '\\', '\0']) && !s.contains("..") }
fn host_name() -> String {
    #[cfg(unix)] { let mut b = [0u8; 256]; if unsafe { libc::gethostname(b.as_mut_ptr() as *mut _, b.len()) } == 0 { let n = b.iter().position(|c| *c == 0).unwrap_or(b.len()); return String::from_utf8_lossy(&b[..n]).into_owned() } }
    std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_default()
}
fn ram_mb() -> u64 {
    #[cfg(target_os = "macos")] { let mut v: u64 = 0; let mut n = std::mem::size_of::<u64>(); if unsafe { libc::sysctlbyname(c"hw.memsize".as_ptr(), &mut v as *mut _ as *mut _, &mut n, std::ptr::null_mut(), 0) } == 0 { return v >> 20 } }
    #[cfg(windows)] { use windows::Win32::System::SystemInformation::*; let mut m = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() }; if unsafe { GlobalMemoryStatusEx(&mut m) }.is_ok() { return m.ullTotalPhys >> 20 } }
    0
}
/// The address this node gives out (1.4): the override, else the address the system would use to leave the machine.
fn advertised_ip(over: &str, sync_root: &str) -> String {
    if !over.is_empty() { return over.to_string() }
    // the address this machine uses to reach the share's host is the one the farm's other machines can reach it by
    let towards = |host: &str| { use std::net::ToSocketAddrs; let to = (host, 445).to_socket_addrs().ok()?.find(|a| a.is_ipv4())?; let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?; s.connect(to).ok()?; Some(s.local_addr().ok()?.ip().to_string()).filter(|ip| ip != "0.0.0.0" && ip != "127.0.0.1") };
    min_paths::classify::share_host_for_path(sync_root).and_then(|h| towards(&h)).or_else(|| towards("192.0.2.1")).unwrap_or_else(|| "127.0.0.1".into())
}
fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> { let tmp = PathBuf::from(format!("{}.tmp", path.display())); std::fs::write(&tmp, text)?; std::fs::rename(&tmp, path).or_else(|_| std::fs::write(path, text)) }

impl Node {
    /// Reads the data folder and starts the farm at the sync root - making it, with its secret, if it is not there.
    pub fn open(data: &Path) -> Result<Arc<Node>, String> {
        let cfg: Value = serde_json::from_str(&std::fs::read_to_string(data.join("config.json")).map_err(|e| format!("config.json: {e}"))?).map_err(|e| format!("config.json could not be read as JSON ({e}); it is left as it is"))?;
        // the node's id (1.1): twelve characters in node_id.txt, made once
        let idf = data.join("node_id.txt");
        let id = match std::fs::read_to_string(&idf).ok().map(|t| t.lines().next().unwrap_or("").to_string()).filter(|t| t.len() == 12) { Some(id) => id,
            None => { let id = crate::share::hex(&crate::share::random(6)?); std::fs::write(&idf, &id).map_err(|e| e.to_string())?; id } };
        let root = cfg["sync_root"].as_str().unwrap_or(""); if root.is_empty() || !Path::new(root).is_dir() { return Err("the sync root is not set, or is not a folder that is there".into()) }
        let farm = Path::new(root).join("MinRender-v2");
        let secret = crate::share::start_farm(&farm, &id, now_ms())?;
        crate::share::log_to(farm.join("nodes").join(&id)); let _ = std::fs::create_dir_all(farm.join("nodes").join(&id));
        let tags: Vec<String> = cfg["tags"].as_array().map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect()).unwrap_or_default();
        Ok(Arc::new(Node { id, data: data.to_path_buf(), farm, secret, port: cfg["http_port"].as_u64().unwrap_or(8420) as u16, ip: advertised_ip(cfg["ip_override"].as_str().unwrap_or(""), root), tags, hostname: host_name(),
            cores: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1), ram_mb: ram_mb(), stopped: AtomicBool::new(cfg["node_stopped"].as_bool().unwrap_or(false)), stop: Arc::new(AtomicBool::new(false)),
            peers: Mutex::new(BTreeMap::new()), work: Mutex::new(Work::default()), reports: Mutex::new(VecDeque::new()), wrote_endpoint: Mutex::new(None), lead: Mutex::new(None), staging: cfg["staging_enabled"].as_bool().unwrap_or(false),
            udp: if cfg["udp_enabled"].as_bool().unwrap_or(true) { let port = cfg["udp_port"].as_u64().unwrap_or(4243) as u16; match heartbeat_socket(port) { Ok(s) => Some((s, port)), Err(e) => { eprintln!("heartbeats are off (the socket: {e}); peers are found by the share and polled"); None } } } else { None }, submitted: Mutex::new(Default::default()),
            mappings: cfg["path_mappings"].as_array().map(|a| a.iter().map(|m| { let s = |k: &str| m[k].as_str().unwrap_or("").to_string(); min_paths::map::Mapping { id: String::new(), label: s("label"), win: s("win"), mac: s("mac"), lin: s("lin"), enabled: m["enabled"].as_bool().unwrap_or(true) } }).collect()).unwrap_or_default() }))
    }
    fn endpoint_file(&self) -> PathBuf { self.farm.join("nodes").join(&self.id).join("endpoint.json") }
    /// The phonebook entry (1.2): a temporary file, then a rename; every write carries a new time.
    fn write_endpoint(&self) { let f = self.endpoint_file(); if let Some(d) = f.parent() { let _ = std::fs::create_dir_all(d); }
        let text = serde_json::to_string_pretty(&json!({ "node_id": self.id, "ip": self.ip, "port": self.port, "timestamp_ms": now_ms() })).unwrap();
        match write_atomically(&f, &text) { Ok(()) => *self.wrote_endpoint.lock().unwrap() = Some(Instant::now()), Err(e) => log(format!("the phonebook entry could not be written: {e}")) } }
    /// The record this node gives of itself (2.4).
    fn record(&self) -> Value {
        let w = self.work.lock().unwrap(); let r = w.running.as_ref();
        json!({ "node_id": self.id, "hostname": self.hostname, "os": OS, "app_version": env!("CARGO_PKG_VERSION"), "gpu_name": "", "cpu_cores": self.cores, "ram_mb": self.ram_mb,
            "node_state": if self.stopped.load(Ordering::Relaxed) { "stopped" } else { "active" }, "render_state": if r.is_some() { "rendering" } else { "idle" },
            "active_job": r.map(|r| r.job.clone()).unwrap_or_default(), "active_chunk": r.map(|r| if r.start == r.end { format!("f{}", r.start) } else { format!("f{}-{}", r.start, r.end) }).unwrap_or_default(),
            "priority": 100, "tags": self.tags, "capabilities": CAPABILITIES, "endpoint": format!("{}:{}", self.ip, self.port), "secret_fingerprint": fingerprint(&self.secret),
            "agent_health": "ok", "alert_reason": "", "ready_for_work": !w.busy, "is_local": true, "is_alive": true, "is_leader": false, "last_seen_ms": 0, "has_udp_contact": false, "failed_polls": 0 })
    }
    /// A heartbeat (1.6): what this node is doing, to every node that listens. The types are exactly 0.5.x's: a
    /// 0.5.x node reads them without a guard.
    fn beat(&self) {
        let Some((s, port)) = &self.udp else { return }; let r = self.record(); let mut v = json!({ "t": "hb", "from": self.id, "n": self.id, "ip": self.ip, "port": self.port, "st": r["node_state"], "rs": r["render_state"], "ah": "ok", "rfw": r["ready_for_work"] });
        if r["render_state"] == json!("rendering") { v["job"] = r["active_job"].clone(); v["chunk"] = r["active_chunk"].clone(); }
        let text = v.to_string(); if text.len() <= 1400 { let _ = s.send_to(text.as_bytes(), (HEARTBEAT_GROUP, *port)); }
    }
    /// What a heartbeat of another node changes (1.6): only a peer the phonebook already gave; never an address.
    fn heard(&self, v: &Value) {
        let (Some(t), Some(n)) = (v["t"].as_str(), v["n"].as_str()) else { return }; if n.is_empty() || n == self.id || v["from"].as_str() == Some(&self.id) { return }
        let mut peers = self.peers.lock().unwrap(); let Some(p) = peers.get_mut(n) else { return };
        if t == "bye" { p.alive = false; p.heard = None; return } if t != "hb" { return }
        let r = p.record.get_or_insert_with(|| json!({ "node_id": n }));
        r["node_state"] = json!(v["st"].as_str().unwrap_or("active")); r["render_state"] = json!(v["rs"].as_str().unwrap_or("idle")); r["active_job"] = json!(v["job"].as_str().unwrap_or("")); r["active_chunk"] = json!(v["chunk"].as_str().unwrap_or(""));
        r["agent_health"] = json!(v["ah"].as_str().unwrap_or("ok")); r["alert_reason"] = json!(v["ar"].as_str().unwrap_or("")); r["ready_for_work"] = json!(v["rfw"].as_bool().unwrap_or(true));
        p.alive = true; p.failed = 0; p.heard = Some(Instant::now());
    }
    /// The leader's address as this node believes it (2.3): the first peer it holds alive and leading.
    fn leader(&self) -> Option<String> { self.peers.lock().unwrap().values().find(|p| p.leader && p.alive).map(|p| p.endpoint.clone()) }

    // ------------------------------------------------------------------------------------------------- what it answers
    pub fn answer(self: &Arc<Self>, q: Request) -> Response {
        let (raw, _query) = q.path.split_once('?').unwrap_or((&q.path, "")); let path = http::decode(raw);
        // the token is asked for before a route is looked for (2.2)
        if path.starts_with("/api/") && !(q.method == "GET" && path == "/api/status") {
            if self.secret.is_empty() { return Response::error(503, "auth_unavailable") }
            let want = format!("Bearer {}", self.secret); let got = q.authorization.as_deref().unwrap_or("");
            let same = got.len() == want.len() && got.bytes().zip(want.bytes()).fold(0u8, |d, (a, b)| d | (a ^ b)) == 0;
            if !same { return Response::error(401, "unauthorized") }
        }
        let not_leader = || { let mut v = json!({ "error": "not_leader" }); if let Some(e) = self.leader() { v["leader_endpoint"] = json!(e); } Response::json(503, v) };
        let body = || serde_json::from_slice::<Value>(&q.body).map_err(|e| Response::error(400, &e.to_string()));
        match (q.method.as_str(), path.as_str()) {
            ("GET", "/api/status") => Response::json(200, self.record()),
            ("GET", "/api/peers") => Response::json(200, Value::Array(self.peers.lock().unwrap().iter().map(|(id, p)| {
                let mut r = p.record.clone().unwrap_or_else(|| json!({ "node_id": id, "endpoint": p.endpoint }));
                for (k, v) in [("is_local", json!(false)), ("is_alive", json!(p.alive)), ("is_leader", json!(p.leader)), ("last_seen_ms", json!(p.last_seen_ms)), ("has_udp_contact", json!(p.udp())), ("failed_polls", json!(p.failed)), ("endpoint", json!(p.endpoint))] { r[k] = v; } r }).collect())),
            ("POST", "/api/node/stop") => { self.set_stopped(true); Response::ok() }
            ("POST", "/api/node/start") => { self.set_stopped(false); Response::ok() }
            ("POST", "/api/node/restart") => Response::error(500, "restart_failed"),
            // (there is no separate agent to restart: the leash is started anew for every chunk)
            ("POST", "/api/agent/restart") => Response::ok(),
            // (a busy or stopped node says so before it reads the body: seen of 0.5.11, which answers `busy` even to a body that is not JSON)
            // a job's chunk logs, a node's log, this node's settings (2.5 rows 31-33, 36): any node answers these
            ("GET", p) if p.starts_with("/api/jobs/") && p.contains("/task-output") => { let seg: Vec<&str> = p["/api/jobs/".len()..].split('/').collect();
                match seg.as_slice() { [job, "task-output"] => if !safe(job) { Response::error(400, "invalid_path") } else { Response::json(200, crate::share::task_output(&self.farm, job)) },
                    [job, "task-output", node, file] => if !safe(job) || !safe(node) || !safe(file) { Response::error(400, "invalid_path") } else { match std::fs::read(self.farm.join("jobs").join(job).join("stdout").join(node).join(file)) { Ok(b) => Response { status: 200, content_type: Some("text/plain"), body: b }, Err(_) => Response::error(404, "not_found") } },
                    _ => Response::empty(404) } }
            ("GET", p) if p.starts_with("/api/nodes/") && p.ends_with("/log") => { let node = &p["/api/nodes/".len()..p.len() - "/log".len()];
                if !safe(node) { Response::error(400, "invalid_path") } else { let max = _query.split('&').find_map(|kv| kv.strip_prefix("max_lines=")).and_then(|v| v.parse().ok()).unwrap_or(500); Response::json(200, json!(crate::share::node_log(&self.farm, node, max))) } }
            ("GET", "/api/config") => { let mut v = std::fs::read_to_string(self.data.join("config.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).unwrap_or(json!({}));
                for (k, x) in [("node_id", json!(self.id)), ("hostname", json!(self.hostname)), ("cpu_cores", json!(self.cores)), ("ram_mb", json!(self.ram_mb)), ("gpu_name", json!("")), ("farm_running", json!(true)), ("farm_path", json!(self.farm.to_string_lossy())), ("farm_error", json!("")), ("is_leader", json!(self.lead.lock().unwrap().is_some()))] { v[k] = x; }
                Response::json(200, v) }
            ("POST", "/api/dispatch/assign") => { if self.work.lock().unwrap().running.is_some() { return Response::error(409, "busy") } if self.stopped.load(Ordering::Relaxed) { return Response::error(409, "stopped") }
                let v = match body() { Ok(v) => v, Err(r) => return r };
                let (Some(m), Some(a), Some(b)) = (v.get("manifest").filter(|m| m.is_object()), v["frame_start"].as_i64(), v["frame_end"].as_i64()) else { return Response::error(400, "the assignment lacks a manifest or its frames") };
                let mut w = self.work.lock().unwrap();
                if w.running.is_some() { return Response::error(409, "busy") }
                if !plain_segment(m["job_id"].as_str().unwrap_or("")) { return Response::error(400, "invalid_job_id") }
                w.queue.push_back(Assignment { manifest: m.clone(), start: a, end: b, epoch: v["edit_epoch"].as_i64() }); w.busy = true; Response::ok() }
            ("POST", "/api/render/abort") => { let v = match body() { Ok(v) => v, Err(r) => return r }; let Some(job) = v["job_id"].as_str() else { return Response::error(400, "the request names no job") };
                log(format!("abort asked for a job ({})", v["reason"].as_str().unwrap_or("job edited"))); self.cut(Some(job), Cut::Abandon); self.work.lock().unwrap().queue.retain(|a| a.manifest["job_id"].as_str() != Some(job)); Response::ok() }
            // what only a leader answers (2.5): by the leader's own routine while this node leads, with a refusal otherwise
            (m, p) if self.lead.lock().unwrap().is_some() && leader_route(p) => match self.lead_answer(m, p, &q.body) { Some(r) => r, None => Response::empty(404) },
            (_, p) if leader_route(p) => not_leader(),
            _ => Response::empty(404),
        }
    }
    /// A leader's route, answered here (this node leads). What the answer set off is done after the lock is let go.
    fn lead_answer(self: &Arc<Self>, method: &str, path: &str, body: &[u8]) -> Option<Response> {
        let (r, fx) = self.lead.lock().unwrap().as_mut()?.handle(method, path, body)?; self.effects(fx); Some(r)
    }
    fn effects(self: &Arc<Self>, fx: Vec<Effect>) {
        for e in fx { match e {
            Effect::CutOwn(job, fail) => self.cut(Some(&job), if fail { Cut::Fail } else { Cut::Abandon }),
            Effect::Abort(nodes, job, reason) => { let targets: Vec<String> = { let p = self.peers.lock().unwrap(); nodes.iter().filter_map(|n| p.get(n).map(|p| p.endpoint.clone())).collect() };
                for t in targets { let (secret, body) = (self.secret.clone(), json!({ "job_id": job, "reason": reason }).to_string()); std::thread::spawn(move || { let _ = http::call(&t, "POST", "/api/render/abort", Some(&secret), Some(&body), Duration::from_secs(3), Duration::from_secs(10)); }); } }
            Effect::RemoveJobDir(id) => { let d = self.farm.join("jobs").join(&id); std::thread::spawn(move || { if d.is_dir() && std::fs::remove_dir_all(&d).is_err() { std::thread::sleep(Duration::from_millis(250)); let _ = std::fs::remove_dir_all(&d); } }); }
            Effect::Dispatch { node, manifest, start, end, epoch } => {
                // (0.5.x translates nothing when the leader gives itself a chunk, so a leader on another system than the
                // submitter's runs the job with the wrong paths; here its own chunks are translated like any node's)
                if node == self.id { let manifest = self.translated(manifest, OS); let mut w = self.work.lock().unwrap(); w.queue.push_back(Assignment { manifest, start, end, epoch: Some(epoch) }); w.busy = true; continue }
                let Some((to, os)) = self.peers.lock().unwrap().get(&node).map(|p| (p.endpoint.clone(), p.record.as_ref().and_then(|r| r["os"].as_str()).unwrap_or("").to_string())) else { continue }; let me = self.clone();
                let manifest = self.translated(manifest, &os);
                std::thread::spawn(move || { let job = manifest["job_id"].as_str().unwrap_or("").to_string(); let body = json!({ "manifest": manifest, "frame_start": start, "frame_end": end, "edit_epoch": epoch }).to_string();
                    let ok = http::call(&to, "POST", "/api/dispatch/assign", Some(&me.secret), Some(&body), Duration::from_millis(500), Duration::from_secs(2)).is_some_and(|(s, _)| s == 200);
                    if !ok { if let Some(l) = me.lead.lock().unwrap().as_mut() { l.revert(job, start, end); } } }); }
        } }
    }
    fn translated(&self, m: Value, worker_os: &str) -> Value { translate_manifest(m, worker_os, &self.mappings) }
    /// Something for the leader: handed over directly when this node leads, sent otherwise. `None`: no answer.
    fn to_leader(self: &Arc<Self>, path: &str, body: &str) -> Option<(u16, Vec<u8>)> {
        if self.lead.lock().unwrap().is_some() { return self.lead_answer("POST", path, body.as_bytes()).map(|r| (r.status, r.body)) }
        let l = self.leader()?; http::call(&l, "POST", path, Some(&self.secret), Some(body), Duration::from_secs(2), Duration::from_secs(5))
    }
    /// The plug-ins' drop box (8.2): each `*.json` in `submissions/` becomes a job - the template found by its id, one
    /// value a flag, the id from the name, the manifest baked and put in its travelling form, handed to the leader.
    /// A file is moved to `processed/` once the leader has taken or refused it, or when it cannot be a job at all,
    /// and the reason is logged. Unlike 0.5.x (ruled 2026-11-14, row 389): a submission is checked as the form
    /// checks one; a file the leader could not be told of STAYS and is tried again; no file is submitted twice.
    fn drop_box(self: &Arc<Self>) {
        let dir = self.data.join("submissions"); let Ok(rd) = std::fs::read_dir(&dir) else { return };
        let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "json")).collect(); files.sort();
        for f in files {
            let name = f.file_name().unwrap_or_default().to_string_lossy().into_owned(); if self.submitted.lock().unwrap().contains(&name) { continue }
            let done = |why: Option<String>| { if let Some(w) = &why { log(format!("the submission {name} is not a job: {w}")); } let to = dir.join("processed"); let _ = std::fs::create_dir_all(&to);
                if std::fs::rename(&f, to.join(&name)).is_err() { self.submitted.lock().unwrap().insert(name.clone()); } };
            let Some(v) = std::fs::read_to_string(&f).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(Value::is_object) else {
                // (a file still being written is not yet JSON: it is given ten seconds before it is called bad)
                if f.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).is_some_and(|d| d.as_secs() >= 10) { done(Some("it is not JSON".into())); } continue };
            let (Some(tid), Some(job_name)) = (v["template_id"].as_str(), v["job_name"].as_str()) else { done(Some("it names no template or no job".into())); continue };
            let Some(t) = crate::submit::templates(&self.farm).into_iter().find(|t| t["template_id"].as_str() == Some(tid)) else { done(Some(format!("the template {tid} is not on the share"))); continue };
            let values: Vec<String> = t["flags"].as_array().map(|a| a.iter().map(|fl| fl["id"].as_str().and_then(|id| v["overrides"][id].as_str()).or(fl["value"].as_str()).unwrap_or("").to_string()).collect()).unwrap_or_default();
            let int = |k: &str, d: i64| v[k].as_i64().unwrap_or(d); let (a, b, chunk) = (int("frame_start", 1), int("frame_end", 250), int("chunk_size", 10));
            let exe = t["cmd"][OS].as_str().unwrap_or("").to_string();
            if let Some(why) = crate::submit::refusal(&t, &values, &exe, a, b, chunk) { done(Some(why)); continue }
            let Some(id) = crate::submit::slug(job_name, &self.farm) else { done(Some("its name gives no id".into())); continue };
            let mut m = crate::submit::bake(&t, &values, &exe, OS, &id, a, b, chunk, &self.id, now_ms());
            if cfg!(target_os = "macos") { m = crate::submit::canonical(m, &self.mappings); }
            match self.to_leader("/api/jobs", &json!({ "manifest": m, "priority": int("priority", 50) }).to_string()) {
                Some((200, _)) => { log(format!("submitted {id}")); done(None) }
                Some((s, b)) => done(Some(format!("the leader refused it ({s} {})", String::from_utf8_lossy(&b)))),
                None => log(format!("the submission {name} waits: no leader answers")),
            }
        }
    }
    /// The leader's cycle (4.6), when this node leads.
    fn lead_cycle(self: &Arc<Self>) {
        let mut nodes: Vec<Candidate> = self.peers.lock().unwrap().iter().map(|(id, p)| { let r = p.record.as_ref();
            Candidate { id: id.clone(), alive: p.alive, can_work: r.is_some_and(|r| r["node_state"] != json!("stopped") && r["ready_for_work"] != json!(false)), rendering: r.is_some_and(|r| r["render_state"] == json!("rendering")), needs_attention: r.is_some_and(|r| r.get("agent_health").is_some_and(|h| h != &json!("ok"))),
                tags: r.and_then(|r| r["tags"].as_array()).map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect()).unwrap_or_default(),
                active: r.filter(|r| r["render_state"] == json!("rendering")).map(|r| (r["active_job"].as_str().unwrap_or("").to_string(), r["active_chunk"].as_str().unwrap_or("").to_string())) } }).collect();
        { let w = self.work.lock().unwrap(); nodes.push(Candidate { id: self.id.clone(), alive: true, can_work: !self.stopped.load(Ordering::Relaxed) && !w.busy, rendering: w.running.is_some(), needs_attention: false, tags: self.tags.clone(),
            active: w.running.as_ref().map(|r| (r.job.clone(), if r.start == r.end { format!("f{}", r.start) } else { format!("f{}-{}", r.start, r.end) })) }); }
        let t = Instant::now(); let fx = match self.lead.lock().unwrap().as_mut() { Some(l) => l.cycle(&nodes, &self.farm), None => return };
        if t.elapsed() > Duration::from_millis(500) { crate::share::log("WARN", "dispatch", &format!("the leader's cycle took {} ms", t.elapsed().as_millis())); }
        self.effects(fx); self.commands();
    }
    /// The node is stopped or started (4.12): kept in the config; a render under way is ended and reported as failed.
    fn set_stopped(&self, on: bool) {
        self.stopped.store(on, Ordering::Relaxed); if on { self.cut(None, Cut::Fail); }
        let f = self.data.join("config.json"); if let Some(mut v) = std::fs::read_to_string(&f).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) { v["node_stopped"] = json!(on); let _ = write_atomically(&f, &serde_json::to_string_pretty(&v).unwrap()); }
    }
    /// The render under way is killed (4.10) - of `job` only, when one is named. There is no gentle step.
    fn cut(&self, job: Option<&str>, how: Cut) {
        let mut w = self.work.lock().unwrap();
        if let Some(r) = w.running.as_mut().filter(|r| job.is_none_or(|j| j == r.job)) { if r.cut.is_none() { r.cut = Some(how); } r.leash_in = None; #[cfg(unix)] if let Some(g) = r.group { crate::leash::kill_group(g); } }
    }

    // --------------------------------------------------------------------------------------------- peers and the leader
    /// One cycle (1): the phonebook entry, the other entries, a poll of every peer, the election.
    fn cycle(&self) {
        // a restart asked for by another node (1.3): the file is taken away; one more than 60 s old is ignored
        let sig = self.farm.join("nodes").join(&self.id).join("restart");
        if let Ok(m) = std::fs::metadata(&sig) { let fresh = m.modified().ok().and_then(|t| t.elapsed().ok()).is_none_or(|d| d.as_secs() <= 60); let _ = std::fs::remove_file(&sig);
            if fresh { crate::share::log("WARN", "farm", "a restart was asked for by another node; this build cannot start itself again yet"); } }
        // the node's own entry: rewritten when it is gone from the listing, not within 15 s of its last write
        let mine = self.endpoint_file(); let listed = mine.parent().and_then(|d| std::fs::read_dir(d).ok()).is_some_and(|rd| rd.flatten().any(|e| e.file_name() == "endpoint.json"));
        if !listed && self.wrote_endpoint.lock().unwrap().is_none_or(|t| t.elapsed() > Duration::from_secs(15)) { self.write_endpoint(); }
        // the others, found by LISTING each folder (never by asking for the file by its path: 1.2)
        let mut seen = Vec::new();
        if let Ok(rd) = std::fs::read_dir(self.farm.join("nodes")) { for d in rd.flatten() {
            let id = d.file_name().to_string_lossy().into_owned(); if id == self.id { continue }
            let Some(e) = std::fs::read_dir(d.path()).ok().and_then(|rd| rd.flatten().find(|e| e.file_name() == "endpoint.json")) else { continue };
            seen.push(id.clone());
            let stamp = e.metadata().ok().map(|m| (m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis()).unwrap_or(0), m.len()));
            let mut peers = self.peers.lock().unwrap();
            if peers.get(&id).is_some_and(|p| p.stamp.is_some() && p.stamp == stamp || p.wait_until.is_some_and(|t| Instant::now() < t)) { continue }
            // an entry that cannot be read is left alone for 15 s
            let Some(v) = std::fs::read_to_string(e.path()).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) else { if let Some(p) = peers.get_mut(&id) { p.wait_until = Some(Instant::now() + Duration::from_secs(15)); } continue };
            if v["node_id"].as_str() != Some(&id) { continue }
            let endpoint = format!("{}:{}", v["ip"].as_str().unwrap_or(""), v["port"].as_u64().unwrap_or(8420)); let written = v["timestamp_ms"].as_i64().unwrap_or(0);
            match peers.get_mut(&id) {
                Some(p) => {
                    // The listing says the file changed and the file says it was written when the last one was: a
                    // Windows share's cached view of a file another machine renamed over. It is believed at the third
                    // such read, after waits of 20 and 40 s (every true write carries a new time: 1.2).
                    if p.stamp.is_some() && written == p.written_ms && p.same < 2 { p.same += 1; p.wait_until = Some(Instant::now() + Duration::from_secs(20 * p.same as u64)); continue }
                    if p.endpoint != endpoint { p.endpoint = endpoint; p.failed = 0; } p.stamp = stamp; p.written_ms = written; p.same = 0; p.wait_until = None; }
                None => { peers.insert(id, Peer { endpoint, record: None, alive: true, failed: 0, last_seen_ms: 0, leader: false, stamp, written_ms: written, same: 0, wait_until: None, heard: None, rtt_ms: None }); } }
        } }
        // the poll (1.5): no token, a new connection, one peer after the other
        // (a peer that is heard by heartbeat, is known by name, and was polled less than 9 s ago is not polled now)
        let targets: Vec<(String, String, u64)> = self.peers.lock().unwrap().iter().filter(|(_, p)| !(p.udp() && p.record.as_ref().is_some_and(|r| !r["hostname"].as_str().unwrap_or("").is_empty()) && now_ms().saturating_sub(p.last_seen_ms) < 9000))
            .map(|(id, p)| (id.clone(), p.endpoint.clone(), p.rtt_ms.map(|r| (4.0 * r) as u64).unwrap_or(2000).clamp(2000, 8000))).collect();
        for (id, endpoint, wait) in targets {
            let t = Instant::now(); let got = http::call(&endpoint, "GET", "/api/status", None, None, Duration::from_millis(wait), Duration::from_millis(wait + 1000)).filter(|(s, _)| *s == 200).and_then(|(_, b)| serde_json::from_slice::<Value>(&b).ok()).filter(peer_record_is_sound)
                // An answer from ANOTHER node is no answer from this one. (A machine whose data folder was emptied comes
                // back under a new id at the old address, and its old entry stays in the phonebook. 0.5.x takes the
                // newcomer's answer for the old node's: the old id looks alive, is given chunks, and every report for
                // them is refused because it comes from the new id. Seen 2026-11-14.)
                .filter(|r| r["node_id"].as_str().is_none_or(|n| n == id));
            let mut peers = self.peers.lock().unwrap(); let Some(p) = peers.get_mut(&id) else { continue };
            match got { Some(mut r) => { if let Some(e) = r["endpoint"].as_str() { p.endpoint = e.to_string(); } r["is_local"] = json!(false);
                    // what a heartbeat said most lately stands over the poll's answer for readiness, and for health while the poll says "ok"
                    if p.udp() { if let Some(old) = &p.record { r["ready_for_work"] = old["ready_for_work"].clone(); if r["agent_health"] == json!("ok") { r["agent_health"] = old["agent_health"].clone(); r["alert_reason"] = old["alert_reason"].clone(); } } }
                    let ms = t.elapsed().as_secs_f64() * 1000.0; p.rtt_ms = Some(p.rtt_ms.map(|o| 0.7 * o + 0.3 * ms).unwrap_or(ms));
                    p.record = Some(r); p.alive = true; p.failed = 0; p.last_seen_ms = now_ms(); }
                // dead at three failed polls - unless a heartbeat was heard in the last ten seconds
                None => { p.failed += 1; if p.failed >= 3 && !p.heard.is_some_and(|t| t.elapsed() < Duration::from_secs(10)) { p.alive = false; } } }
            // a peer leaves only when it is dead AND its entry is gone from the listing
            if !p.alive && !seen.contains(&id) { peers.remove(&id); }
        }
        // the election (3.1): the tag `leader` first, then those without `noleader`, then the id
        let mut peers = self.peers.lock().unwrap();
        let key = |tags: &[String], id: &str| (!tags.iter().any(|t| t == "leader"), tags.iter().any(|t| t == "noleader"), id.to_string());
        let tags_of = |p: &Peer| p.record.as_ref().and_then(|r| r["tags"].as_array()).map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect::<Vec<_>>()).unwrap_or_default();
        let mut best = (key(&self.tags, &self.id), None::<String>);
        for (id, p) in peers.iter().filter(|(_, p)| p.alive) { let k = key(&tags_of(p), id); if k < best.0 { best = (k, Some(id.clone())); } }
        for (id, p) in peers.iter_mut() { p.leader = best.1.as_deref() == Some(id.as_str()); }
        let lead = best.1.is_none(); drop(peers);
        // taking over, or giving way (3.4): the database is opened only while this node leads
        let mut l = self.lead.lock().unwrap();
        if lead && l.is_none() { match Lead::take_over(&self.data, &self.farm, &self.id) { Ok(x) => { *l = Some(x); log("this node leads now"); } Err(e) => log(format!("this node should lead, and its database could not be opened: {e}")) } }
        else if !lead && l.is_some() { *l = None; log("another node leads now"); }
    }

    // ---------------------------------------------------------------------------------------------------- the render
    /// One chunk, from the leash's start to the report (4.10).
    fn render(self: &Arc<Self>, a: Assignment) {
        let m = &a.manifest; let job = m["job_id"].as_str().unwrap_or("").to_string();
        let tokens = |s: &str| s.replace("{frame}", &a.start.to_string()).replace("{chunk_start}", &a.start.to_string()).replace("{chunk_end}", &a.end.to_string());
        let exe = m["cmd"][OS].as_str().unwrap_or("").to_string();
        // Local staging (4.11), when this node has it on and the job names an output folder: the render writes to a folder
        // of this machine, and the files go to the output folder when the chunk is done.
        let out_dir = m["output_dir"].as_str().filter(|d| !d.is_empty()).map(PathBuf::from);
        let staging = out_dir.as_ref().filter(|_| self.staging).map(|_| self.data.join("staging").join(&job).join(format!("{:06}-{:06}", a.start, a.end))).filter(|d| std::fs::create_dir_all(d).is_ok());
        let mut args: Vec<String> = Vec::new();
        for f in m["flags"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) { if let Some(n) = f["flag"].as_str().filter(|n| !n.is_empty()) { args.push(tokens(n)); }
            if let Some(v) = f["value"].as_str() { match (&staging, f["is_output"] == json!(true)) { (Some(st), true) => args.push(st.join(v.rsplit(['/', '\\']).next().unwrap_or(v)).to_string_lossy().into_owned()), _ => args.push(tokens(v)) } } }
        let report = |kind: &str, extra: Value| { let mut v = json!({ "node_id": self.id, "job_id": job, "frame_start": a.start, "frame_end": a.end }); if let Some(e) = a.epoch { v["edit_epoch"] = json!(e); }
            for (k, x) in extra.as_object().unwrap() { v[k] = x.clone(); } self.reports.lock().unwrap().push_back((format!("/api/dispatch/{kind}"), v.to_string())); };
        let failed = || report("failed", json!({ "error": "" }));
        // the chunk's own log on the share (6.5): one file for each attempt
        let logf = self.farm.join("jobs").join(&job).join("stdout").join(&self.id).join(format!("{:06}-{:06}_{}.log", a.start, a.end, now_ms()));
        if let Some(d) = logf.parent() { let _ = std::fs::create_dir_all(d); }
        // (the output's folder is made before the render, as well as may be; a leader never makes it)
        if let Some(d) = m["output_dir"].as_str().filter(|d| !d.is_empty()) { let _ = std::fs::create_dir_all(d); }
        let pattern = m["progress"]["completion_pattern"]["regex"].as_str().and_then(|r| regex::Regex::new(r).ok());
        let mut c = std::process::Command::new(std::env::current_exe().unwrap_or_else(|_| "minRender".into()));
        c.arg("--leash").arg("--").arg(&exe).args(&args).env("PYTHONUNBUFFERED", "1").stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        if let Some(e) = m["environment"].as_object() { for (k, v) in e { if let Some(v) = v.as_str() { c.env(k, v); } } }
        if let Some(d) = m["process"]["working_dir"].as_str().map(tokens).filter(|d| !d.is_empty()) { c.current_dir(d); }
        #[cfg(windows)] { use std::os::windows::process::CommandExt; c.creation_flags(0x0800_0000); }
        let mut child = match c.spawn() { Ok(c) => c, Err(e) => { log(format!("the leash could not be started: {e}")); failed(); self.idle(); return } };
        let (tx, rx) = std::sync::mpsc::channel::<(bool, String)>();
        let out = std::io::BufReader::new(child.stdout.take().unwrap()); let err = std::io::BufReader::new(child.stderr.take().unwrap());
        // (the group's id is the first line where there are process groups; it never reaches the log)
        let group = Arc::new(Mutex::new(None::<u32>));
        { let (tx, group) = (tx.clone(), group.clone()); std::thread::spawn(move || { for l in out.split(b'\n').flatten() { let l = String::from_utf8_lossy(&l).trim_end_matches('\r').to_string();
            if let Some(g) = l.strip_prefix(crate::leash::GROUP_LINE) { *group.lock().unwrap() = g.trim().parse().ok(); let _ = tx.send((false, String::new())); continue } if tx.send((false, l)).is_err() { break } } }); }
        std::thread::spawn(move || { for l in err.split(b'\n').flatten() { if tx.send((true, String::from_utf8_lossy(&l).trim_end_matches('\r').to_string())).is_err() { break } } });
        self.work.lock().unwrap().running = Some(Running { job: job.clone(), start: a.start, end: a.end, cut: None, leash_in: child.stdin.take(), group: None });
        log(format!("rendering frames {}-{}", a.start, a.end));
        let started = Instant::now(); let limit = m["timeout_seconds"].as_u64().filter(|s| *s > 0).map(Duration::from_secs);
        let (mut lines, mut flushed, mut matches, mut frames, mut sent) = (Vec::<String>::new(), Instant::now(), 0i64, Vec::<i64>::new(), Instant::now());
        let flush = |lines: &mut Vec<String>| { if lines.is_empty() { return } if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&logf) { for l in lines.iter() { let _ = writeln!(f, "{l}"); } } lines.clear(); };
        // frames done, in batches (2.3): an answer that says "stale" ends the render without a report
        let send_frames = |frames: &mut Vec<i64>| -> bool { if frames.is_empty() { return true }
            let mut v = json!({ "node_id": self.id, "job_id": job, "frames": frames }); if let Some(e) = a.epoch { v["edit_epoch"] = json!(e); }
            match self.to_leader("/api/dispatch/frame-complete", &v.to_string()) {
                Some((200, b)) => { frames.clear(); !String::from_utf8_lossy(&b).contains("\"stale\"") } _ => true } };
        loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok((is_err, l)) => { if let Some(g) = *group.lock().unwrap() { if let Some(r) = self.work.lock().unwrap().running.as_mut() { if r.group.is_none() { r.group = Some(g); } } } if l.is_empty() && !is_err { continue }
                    if pattern.as_ref().is_some_and(|p| p.is_match(&l)) { let f = a.start + matches; matches += 1; if f <= a.end { frames.push(f); } }
                    lines.push(if is_err { format!("[stderr] {l}") } else { l }); }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,   // both streams have ended: the tree is gone
            }
            if lines.len() >= 50 || flushed.elapsed() > Duration::from_secs(5) { flush(&mut lines); flushed = Instant::now(); }
            if sent.elapsed() > Duration::from_secs(2) { sent = Instant::now(); if !send_frames(&mut frames) { log("the leader calls this render stale: abandoned"); self.cut(None, Cut::Abandon); } }
            if limit.is_some_and(|l| started.elapsed() > l) { log("the chunk ran past its time: killed"); self.cut(None, Cut::Fail); }
        }
        let status = child.wait().ok(); flush(&mut lines);
        // the leash itself was killed (nothing told it to): its tree is this process's to kill
        #[cfg(unix)] { use std::os::unix::process::ExitStatusExt; if status.as_ref().is_some_and(|s| s.signal().is_some()) { if let Some(g) = *group.lock().unwrap() { crate::leash::kill_group(g); } } }
        let cut = self.work.lock().unwrap().running.as_ref().and_then(|r| r.cut);
        match (cut, status.and_then(|s| s.code())) {
            (Some(Cut::Abandon), _) => log("abandoned: nothing is reported"),
            (Some(Cut::Fail), _) => failed(),
            (None, Some(0)) => { let _ = send_frames(&mut frames);
                match (staging, out_dir) {
                    // staged files are delivered before the chunk is called complete; a delivery that fails is tried again
                    // every 30 s for ten minutes with the node free for other work, then the chunk is reported failed and
                    // the files stay; a job that is gone meanwhile ends the tries without a report
                    (Some(st), Some(out)) => if deliver(&st, &out) { report("complete", json!({ "elapsed_ms": 0, "exit_code": 0 })); log("chunk complete (its files delivered)"); } else {
                        crate::share::log("WARN", "render", "the chunk's files could not be delivered to the output folder: they are kept and the delivery is tried again");
                        let me = self.clone(); let (job, mut v) = (job.clone(), json!({ "node_id": self.id, "job_id": job, "frame_start": a.start, "frame_end": a.end })); if let Some(e) = a.epoch { v["edit_epoch"] = json!(e); }
                        std::thread::spawn(move || { let t = Instant::now(); loop { for _ in 0..300 { if me.stop.load(Ordering::Relaxed) { return } std::thread::sleep(Duration::from_millis(100)); }
                            if me.job_is_gone(&job) { return }
                            if deliver(&st, &out) { v["elapsed_ms"] = json!(0); v["exit_code"] = json!(0); me.reports.lock().unwrap().push_back(("/api/dispatch/complete".into(), v.to_string())); return }
                            if t.elapsed() > Duration::from_secs(600) { v["error"] = json!(""); me.reports.lock().unwrap().push_back(("/api/dispatch/failed".into(), v.to_string())); return } } }); },
                    _ => { report("complete", json!({ "elapsed_ms": 0, "exit_code": 0 })); log("chunk complete"); } } }
            (None, code) => { log(format!("the renderer ended with {code:?}: failed")); failed(); }
        }
        self.idle();
    }
    /// The leader's list no longer has the job (asked of the leader, or of this node's own queue when it leads).
    fn job_is_gone(self: &Arc<Self>, job: &str) -> bool {
        let list = if self.lead.lock().unwrap().is_some() { self.lead_answer("GET", "/api/jobs", b"").map(|r| r.body) } else { self.leader().and_then(|l| http::call(&l, "GET", "/api/jobs", Some(&self.secret), None, Duration::from_millis(500), Duration::from_secs(1))).filter(|(s, _)| *s == 200).map(|(_, b)| b) };
        list.and_then(|b| serde_json::from_slice::<Value>(&b).ok()).and_then(|v| v.as_array().map(|a| !a.iter().any(|j| j["job_id"].as_str() == Some(job)))).unwrap_or(false)
    }
    /// The leader's side of the command drop box (6.6): job edits a node could not get to its leader, left on the share.
    /// Each is applied in the order of its name (its time), or set aside when it is not an edit or is more than fifteen
    /// minutes old; either way it goes to `processed/`, where files are kept seven days.
    fn commands(self: &Arc<Self>) {
        let dir = self.farm.join("commands"); let Ok(rd) = std::fs::read_dir(&dir) else { return }; let done = dir.join("processed");
        let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "json")).collect(); files.sort();
        for f in files {
            let v = std::fs::read_to_string(&f).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).unwrap_or(Value::Null); let at = v["issued_at_ms"].as_i64().unwrap_or(0);
            if v["type"] == json!("job_edit") && at > 0 && (now_ms() as i64 - at) <= 15 * 60 * 1000 { if let Some(job) = v["job_id"].as_str() {
                let body = json!({ "manifest": v["manifest"], "mode": v["mode"].as_str().unwrap_or("continue"), "priority": v["priority"].as_i64().unwrap_or(50) }).to_string();
                match self.lead_answer("POST", &format!("/api/jobs/{job}/edit"), body.as_bytes()) { Some(r) => log(format!("an edit left on the share was {} ({})", if r.status == 200 { "applied" } else { "refused" }, r.status)), None => return } } }
            let _ = std::fs::create_dir_all(&done); if std::fs::rename(&f, done.join(f.file_name().unwrap_or_default())).is_err() { let _ = std::fs::remove_file(&f); }
        }
        if let Ok(rd) = std::fs::read_dir(&done) { for e in rd.flatten() { if e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).is_some_and(|d| d.as_secs() > 7 * 86400) { let _ = std::fs::remove_file(e.path()); } } }
    }
    fn idle(&self) { let mut w = self.work.lock().unwrap(); w.running = None; w.busy = !w.queue.is_empty(); }

    /// Runs the node until `stop` is set, then leaves the farm cleanly.
    pub fn run(self: &Arc<Self>) -> Result<(), String> {
        let listener = std::net::TcpListener::bind(("0.0.0.0", self.port)).map_err(|e| format!("port {}: {e}", self.port))?;
        self.write_endpoint();
        { let (me, stop) = (self.clone(), self.stop.clone()); std::thread::spawn(move || http::serve(listener, stop, move |q| me.answer(q))); }
        // the peers, every 3 s
        { let me = self.clone(); std::thread::spawn(move || while !me.stop.load(Ordering::Relaxed) { me.cycle(); for _ in 0..30 { if me.stop.load(Ordering::Relaxed) { break } std::thread::sleep(Duration::from_millis(100)); } }); }
        // the leader's cycle, every 2 s, while this node leads
        { let me = self.clone(); std::thread::spawn(move || while !me.stop.load(Ordering::Relaxed) { me.lead_cycle(); for _ in 0..20 { if me.stop.load(Ordering::Relaxed) { break } std::thread::sleep(Duration::from_millis(100)); } }); }
        // heartbeats: one at the start and every 3 s; those of others, as they come
        if let Some((s, _)) = &self.udp { self.beat();
            { let me = self.clone(); std::thread::spawn(move || while !me.stop.load(Ordering::Relaxed) { for _ in 0..30 { if me.stop.load(Ordering::Relaxed) { return } std::thread::sleep(Duration::from_millis(100)); } me.beat(); }); }
            if let Ok(s) = s.try_clone() { let me = self.clone(); std::thread::spawn(move || { let mut b = [0u8; 1500]; while !me.stop.load(Ordering::Relaxed) { if let Ok((n, _)) = s.recv_from(&mut b) { if let Ok(v) = serde_json::from_slice::<Value>(&b[..n]) { if v.is_object() { me.heard(&v); } } } } }); } }
        // the plug-ins' drop box, every 3 s
        { let me = self.clone(); std::thread::spawn(move || while !me.stop.load(Ordering::Relaxed) { me.drop_box(); for _ in 0..30 { if me.stop.load(Ordering::Relaxed) { break } std::thread::sleep(Duration::from_millis(100)); } }); }
        // reports to the leader: in order, kept until a 200 comes, five seconds after a failure (2.3)
        { let me = self.clone(); std::thread::spawn(move || while !me.stop.load(Ordering::Relaxed) {
            let next = me.reports.lock().unwrap().front().cloned();
            let Some((path, body)) = next else { std::thread::sleep(Duration::from_millis(100)); continue };
            let ok = me.to_leader(&path, &body).is_some_and(|(s, _)| s == 200);
            if ok { me.reports.lock().unwrap().pop_front(); } else { for _ in 0..50 { if me.stop.load(Ordering::Relaxed) { break } std::thread::sleep(Duration::from_millis(100)); } } }); }
        // the jobs list, while rendering (2.6): a job that is gone from it, or cancelled, is killed at once and reported failed
        { let me = self.clone(); std::thread::spawn(move || while !me.stop.load(Ordering::Relaxed) { std::thread::sleep(Duration::from_secs(2));
            // (a leader ends its own render of a cancelled or deleted job as it takes the command)
            if me.lead.lock().unwrap().is_some() { continue }
            let Some(job) = me.work.lock().unwrap().running.as_ref().map(|r| r.job.clone()) else { continue }; let Some(l) = me.leader() else { continue };
            let Some(list) = http::call(&l, "GET", "/api/jobs", Some(&me.secret), None, Duration::from_millis(500), Duration::from_secs(1)).filter(|(s, _)| *s == 200).and_then(|(_, b)| serde_json::from_slice::<Value>(&b).ok()).and_then(|v| v.as_array().cloned()) else { continue };
            if !list.iter().any(|j| j["job_id"].as_str() == Some(&job) && j["current_state"] != json!("cancelled")) { log("the job is cancelled or gone: the render is killed"); me.cut(Some(&job), Cut::Fail); } }); }
        log(format!("node {} is in the farm as {}:{}", self.id, self.ip, self.port));
        // the chunks, one at a time
        while !self.stop.load(Ordering::Relaxed) {
            let next = self.work.lock().unwrap().queue.pop_front();
            match next { Some(a) => if self.stopped.load(Ordering::Relaxed) { log("stopped: the chunk is abandoned"); self.idle(); } else { self.render(a) }, None => std::thread::sleep(Duration::from_millis(50)) }
        }
        // leaving: a render is killed, the leader is told once if it answers, the phonebook entry is taken away
        self.cut(None, Cut::Fail); std::thread::sleep(Duration::from_millis(150));
        let last: Vec<(String, String)> = self.reports.lock().unwrap().drain(..).collect(); for (path, body) in last { let _ = self.to_leader(&path, &body); }
        if let Some((s, port)) = &self.udp { let _ = s.send_to(json!({ "from": self.id, "n": self.id, "t": "bye" }).to_string().as_bytes(), (HEARTBEAT_GROUP, *port)); }
        let _ = std::fs::remove_file(self.endpoint_file()); http::wake(self.port);
        Ok(())
    }
}
/// A manifest's paths for a node on another system than the submitter's (7.6): every flag value that is text, and the
/// output folder, through the LEADER's mappings - the first that matches wins, and separators are made the target's in
/// every value, path or not. Nothing else is touched. With no mappings, or the same system: as it is.
fn translate_manifest(mut m: Value, worker_os: &str, mappings: &[min_paths::map::Mapping]) -> Value {
    use min_paths::{map::translate, Os}; let tag = |o: &str| match o { "windows" => Some(Os::Win), "macos" => Some(Os::Mac), "linux" => Some(Os::Lin), _ => None };
    let from = m["submitted_os"].as_str().unwrap_or("").to_string(); if from == worker_os || mappings.is_empty() { return m }
    let (Some(src), Some(dst)) = (tag(&from), tag(worker_os)) else { return m };
    if let Some(flags) = m["flags"].as_array_mut() { for f in flags { if let Some(v) = f["value"].as_str() { f["value"] = json!(translate(src, dst, v, mappings)); } } }
    if let Some(d) = m["output_dir"].as_str() { m["output_dir"] = json!(translate(src, dst, d, mappings)); }
    m
}
#[cfg(test)]
mod tests {
    use super::*;
    fn maps() -> Vec<min_paths::map::Mapping> { vec![min_paths::map::Mapping { id: String::new(), label: "Projects".into(), win: "P:\\jobs".into(), mac: "/Volumes/projects/jobs".into(), lin: String::new(), enabled: true }] }
    #[test] fn a_windows_manifest_for_a_mac_worker() {
        let m = json!({ "submitted_os": "windows", "cmd": { "windows": "C:\\blender.exe" }, "flags": [{ "flag": "-b", "value": null }, { "flag": "", "value": "P:\\jobs\\show\\scene.blend" }, { "flag": "-F", "value": "OPEN_EXR_MULTILAYER" }, { "flag": "", "value": "{chunk_start}" }], "output_dir": "p:\\JOBS\\show\\out", "process": { "working_dir": "P:\\jobs" } });
        let t = translate_manifest(m, "macos", &maps());
        assert_eq!(t["flags"][1]["value"], json!("/Volumes/projects/jobs/show/scene.blend")); assert_eq!(t["flags"][0]["value"], Value::Null);
        assert_eq!(t["flags"][2]["value"], json!("OPEN_EXR_MULTILAYER")); assert_eq!(t["flags"][3]["value"], json!("{chunk_start}"));
        assert_eq!(t["output_dir"], json!("/Volumes/projects/jobs/show/out"));   // a Windows root is matched whatever its case
        assert_eq!(t["cmd"]["windows"], json!("C:\\blender.exe")); assert_eq!(t["process"]["working_dir"], json!("P:\\jobs"));   // never translated
    }
    #[test] fn a_root_ends_on_a_folder_boundary() { let m = json!({ "submitted_os": "windows", "flags": [{ "flag": "", "value": "P:\\jobs-old\\a.blend" }] }); assert_eq!(translate_manifest(m, "macos", &maps())["flags"][0]["value"], json!("P:/jobs-old/a.blend")); }
    #[test] fn the_same_system_and_no_mappings_change_nothing() { let m = json!({ "submitted_os": "windows", "flags": [{ "flag": "", "value": "a/b" }] });
        assert_eq!(translate_manifest(m.clone(), "windows", &maps()), m); assert_eq!(translate_manifest(m.clone(), "macos", &[]), m); }
}
/// A staged chunk's files go to the output folder (4.11): every file directly in the staging folder, written over what
/// is there; when all have gone the staging folder is removed. False: the folder could not be made or a copy failed.
fn deliver(staging: &Path, out: &Path) -> bool {
    if std::fs::create_dir_all(out).is_err() { return false } let Ok(rd) = std::fs::read_dir(staging) else { return false };
    for e in rd.flatten().filter(|e| e.path().is_file()) { if std::fs::copy(e.path(), out.join(e.file_name())).is_err() { return false } }
    let _ = std::fs::remove_dir_all(staging); if let Some(job) = staging.parent() { let _ = std::fs::remove_dir(job); } true
}
/// The group every node's heartbeats go to (1.6); the port is a setting.
const HEARTBEAT_GROUP: std::net::Ipv4Addr = std::net::Ipv4Addr::new(239, 42, 0, 1);
/// The heartbeat's socket: shared with any other node on this machine, in the group, heard one hop away.
fn heartbeat_socket(port: u16) -> std::io::Result<std::net::UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type}; let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?; s.set_reuse_address(true)?; #[cfg(unix)] s.set_reuse_port(true)?;
    s.bind(&std::net::SocketAddr::from(([0, 0, 0, 0], port)).into())?; s.join_multicast_v4(&HEARTBEAT_GROUP, &std::net::Ipv4Addr::UNSPECIFIED)?; s.set_multicast_ttl_v4(1)?; s.set_read_timeout(Some(Duration::from_millis(500)))?;
    Ok(s.into())
}
/// The routes only a leader answers (2.5, rows 9-30).
fn leader_route(p: &str) -> bool {
    p == "/api/jobs" || p.starts_with("/api/jobs/") && !p.contains("/task-output") || p.starts_with("/api/chunks/") || p == "/api/nodes/suspended" || p.starts_with("/api/nodes/") && p.ends_with("/unsuspend") || matches!(p, "/api/dispatch/frame-complete" | "/api/dispatch/complete" | "/api/dispatch/failed")
}
/// A peer's record whose read keys have the right types (2.4): anything else counts as a failed poll.
fn peer_record_is_sound(r: &Value) -> bool {
    let s = |k: &str| r.get(k).is_none_or(Value::is_string); let b = |k: &str| r.get(k).is_none_or(Value::is_boolean);
    r.is_object() && ["node_id", "hostname", "os", "app_version", "gpu_name", "node_state", "render_state", "active_job", "active_chunk", "endpoint", "secret_fingerprint", "agent_health", "alert_reason"].iter().all(|k| s(k))
        && r.get("tags").is_none_or(|t| t.as_array().is_some_and(|a| a.iter().all(Value::is_string))) && r.get("ram_mb").is_none_or(Value::is_u64) && r.get("cpu_cores").is_none_or(|v| v.is_i64() || v.is_u64()) && b("ready_for_work")
}
