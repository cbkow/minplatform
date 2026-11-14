//! The node: its identity, its place in the farm's phonebook, what it answers, whom it believes alive and leading,
//! and the one chunk it renders at a time. Every rule here is a line of docs/phase4/step0-contracts.md (section
//! numbers in the comments) or something `tools/mr-conform` saw a 0.5.11 node do (step2-conformance.md).
//!
//! This first slice FOLLOWS: it joins, answers, polls, renders what a leader assigns and reports it. It does not lead
//! (no database yet), so `main` insists on the tag `noleader`.
use crate::http::{self, Request, Response};
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
fn log(msg: impl AsRef<str>) { eprintln!("[{}] {}", now_ms() / 1000 % 100000, msg.as_ref()); }

/// A peer as this node knows it (1.2, 1.5).
struct Peer { endpoint: String, record: Option<Value>, alive: bool, failed: u32, last_seen_ms: u64, leader: bool, /// (modified, size) of its endpoint.json at the last good read
    stamp: Option<(u128, u64)> }
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
    reports: Mutex<VecDeque<(String, String)>>, wrote_endpoint: Mutex<Option<Instant>>, i_lead: AtomicBool,
}

fn fingerprint(secret: &str) -> String { use sha2::Digest; if secret.is_empty() { return String::new() } sha2::Sha256::digest(secret.as_bytes()).iter().take(4).map(|b| format!("{b:02x}")).collect() }
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
fn advertised_ip(over: &str) -> String {
    if !over.is_empty() { return over.to_string() }
    std::net::UdpSocket::bind("0.0.0.0:0").ok().and_then(|s| { s.connect("192.0.2.1:445").ok()?; s.local_addr().ok() }).map(|a| a.ip().to_string()).filter(|ip| ip != "0.0.0.0").unwrap_or_else(|| "127.0.0.1".into())
}
fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> { let tmp = PathBuf::from(format!("{}.tmp", path.display())); std::fs::write(&tmp, text)?; std::fs::rename(&tmp, path).or_else(|_| std::fs::write(path, text)) }

impl Node {
    /// Reads the data folder and the farm. Nothing on the share is created by this slice: a farm must exist.
    pub fn open(data: &Path) -> Result<Arc<Node>, String> {
        let cfg: Value = serde_json::from_str(&std::fs::read_to_string(data.join("config.json")).map_err(|e| format!("config.json: {e}"))?).map_err(|e| format!("config.json could not be read as JSON ({e}); it is left as it is"))?;
        // the node's id (1.1): twelve characters in node_id.txt, made once
        let idf = data.join("node_id.txt");
        let id = match std::fs::read_to_string(&idf).ok().map(|t| t.lines().next().unwrap_or("").to_string()).filter(|t| t.len() == 12) { Some(id) => id,
            None => { let mut b = [0u8; 6]; std::fs::File::open("/dev/urandom").and_then(|mut f| std::io::Read::read_exact(&mut f, &mut b)).map_err(|e| format!("no random bytes for a node id: {e}"))?;
                let id: String = b.iter().map(|x| format!("{x:02x}")).collect(); std::fs::write(&idf, &id).map_err(|e| e.to_string())?; id } };
        let root = cfg["sync_root"].as_str().unwrap_or(""); if root.is_empty() || !Path::new(root).is_dir() { return Err("the sync root is not set, or is not a folder that is there".into()) }
        let farm = Path::new(root).join("MinRender-v2");
        let secret = std::fs::read_to_string(farm.join("farm.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v["api_secret"].as_str().map(str::to_string)).filter(|s| !s.is_empty()).ok_or("there is no farm at the sync root yet (this build joins a farm; it does not make one)")?;
        let tags: Vec<String> = cfg["tags"].as_array().map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect()).unwrap_or_default();
        Ok(Arc::new(Node { id, data: data.to_path_buf(), farm, secret, port: cfg["http_port"].as_u64().unwrap_or(8420) as u16, ip: advertised_ip(cfg["ip_override"].as_str().unwrap_or("")), tags, hostname: host_name(),
            cores: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1), ram_mb: ram_mb(), stopped: AtomicBool::new(cfg["node_stopped"].as_bool().unwrap_or(false)), stop: Arc::new(AtomicBool::new(false)),
            peers: Mutex::new(BTreeMap::new()), work: Mutex::new(Work::default()), reports: Mutex::new(VecDeque::new()), wrote_endpoint: Mutex::new(None), i_lead: AtomicBool::new(false) }))
    }
    pub fn tags(&self) -> &[String] { &self.tags }
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
                for (k, v) in [("is_local", json!(false)), ("is_alive", json!(p.alive)), ("is_leader", json!(p.leader)), ("last_seen_ms", json!(p.last_seen_ms)), ("has_udp_contact", json!(false)), ("failed_polls", json!(p.failed)), ("endpoint", json!(p.endpoint))] { r[k] = v; } r }).collect())),
            ("POST", "/api/node/stop") => { self.set_stopped(true); Response::ok() }
            ("POST", "/api/node/start") => { self.set_stopped(false); Response::ok() }
            ("POST", "/api/node/restart") => Response::error(500, "restart_failed"),
            // (there is no separate agent to restart: the leash is started anew for every chunk)
            ("POST", "/api/agent/restart") => Response::ok(),
            // (a busy or stopped node says so before it reads the body: seen of 0.5.11, which answers `busy` even to a body that is not JSON)
            ("POST", "/api/dispatch/assign") => { if self.work.lock().unwrap().running.is_some() { return Response::error(409, "busy") } if self.stopped.load(Ordering::Relaxed) { return Response::error(409, "stopped") }
                let v = match body() { Ok(v) => v, Err(r) => return r };
                let (Some(m), Some(a), Some(b)) = (v.get("manifest").filter(|m| m.is_object()), v["frame_start"].as_i64(), v["frame_end"].as_i64()) else { return Response::error(400, "the assignment lacks a manifest or its frames") };
                let mut w = self.work.lock().unwrap();
                if w.running.is_some() { return Response::error(409, "busy") }
                if !plain_segment(m["job_id"].as_str().unwrap_or("")) { return Response::error(400, "invalid_job_id") }
                w.queue.push_back(Assignment { manifest: m.clone(), start: a, end: b, epoch: v["edit_epoch"].as_i64() }); w.busy = true; Response::ok() }
            ("POST", "/api/render/abort") => { let v = match body() { Ok(v) => v, Err(r) => return r }; let Some(job) = v["job_id"].as_str() else { return Response::error(400, "the request names no job") };
                log(format!("abort asked for a job ({})", v["reason"].as_str().unwrap_or("job edited"))); self.cut(Some(job), Cut::Abandon); self.work.lock().unwrap().queue.retain(|a| a.manifest["job_id"].as_str() != Some(job)); Response::ok() }
            // what only a leader answers (2.5), and this node is not one
            (_, p) if p == "/api/jobs" || p.starts_with("/api/jobs/") && !p.contains("/task-output") || p.starts_with("/api/chunks/") || p == "/api/nodes/suspended" || p.starts_with("/api/nodes/") && p.ends_with("/unsuspend")
                || matches!(p, "/api/dispatch/frame-complete" | "/api/dispatch/complete" | "/api/dispatch/failed") => not_leader(),
            _ => Response::empty(404),
        }
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
            if peers.get(&id).is_some_and(|p| p.stamp.is_some() && p.stamp == stamp) { continue }
            let Some(v) = std::fs::read_to_string(e.path()).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) else { continue };
            if v["node_id"].as_str() != Some(&id) { continue }
            let endpoint = format!("{}:{}", v["ip"].as_str().unwrap_or(""), v["port"].as_u64().unwrap_or(8420));
            match peers.get_mut(&id) { Some(p) => { if p.endpoint != endpoint { p.endpoint = endpoint; p.failed = 0; } p.stamp = stamp; }
                None => { peers.insert(id, Peer { endpoint, record: None, alive: true, failed: 0, last_seen_ms: 0, leader: false, stamp }); } }
        } }
        // the poll (1.5): no token, a new connection, one peer after the other
        let targets: Vec<(String, String)> = self.peers.lock().unwrap().iter().map(|(id, p)| (id.clone(), p.endpoint.clone())).collect();
        for (id, endpoint) in targets {
            let got = http::call(&endpoint, "GET", "/api/status", None, None, Duration::from_secs(2), Duration::from_secs(3)).filter(|(s, _)| *s == 200).and_then(|(_, b)| serde_json::from_slice::<Value>(&b).ok()).filter(peer_record_is_sound);
            let mut peers = self.peers.lock().unwrap(); let Some(p) = peers.get_mut(&id) else { continue };
            match got { Some(mut r) => { if let Some(e) = r["endpoint"].as_str() { p.endpoint = e.to_string(); } r["is_local"] = json!(false); p.record = Some(r); p.alive = true; p.failed = 0; p.last_seen_ms = now_ms(); }
                None => { p.failed += 1; if p.failed >= 3 { p.alive = false; } } }
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
        let lead = best.1.is_none(); if lead && !self.i_lead.swap(lead, Ordering::Relaxed) { log("this node would lead now, and this build cannot: it holds no jobs and answers as a follower"); } else { self.i_lead.store(lead, Ordering::Relaxed); }
    }

    // ---------------------------------------------------------------------------------------------------- the render
    /// One chunk, from the leash's start to the report (4.10).
    fn render(self: &Arc<Self>, a: Assignment) {
        let m = &a.manifest; let job = m["job_id"].as_str().unwrap_or("").to_string();
        let tokens = |s: &str| s.replace("{frame}", &a.start.to_string()).replace("{chunk_start}", &a.start.to_string()).replace("{chunk_end}", &a.end.to_string());
        let exe = m["cmd"][OS].as_str().unwrap_or("").to_string();
        let mut args: Vec<String> = Vec::new();
        for f in m["flags"].as_array().map(|a| a.as_slice()).unwrap_or(&[]) { if let Some(n) = f["flag"].as_str().filter(|n| !n.is_empty()) { args.push(tokens(n)); } if let Some(v) = f["value"].as_str() { args.push(tokens(v)); } }
        let report = |kind: &str, extra: Value| { let mut v = json!({ "node_id": self.id, "job_id": job, "frame_start": a.start, "frame_end": a.end }); if let Some(e) = a.epoch { v["edit_epoch"] = json!(e); }
            for (k, x) in extra.as_object().unwrap() { v[k] = x.clone(); } self.reports.lock().unwrap().push_back((format!("/api/dispatch/{kind}"), v.to_string())); };
        let failed = || report("failed", json!({ "error": "" }));
        // the chunk's own log on the share (6.5): one file for each attempt
        let logf = self.farm.join("jobs").join(&job).join("stdout").join(&self.id).join(format!("{:06}-{:06}_{}.log", a.start, a.end, now_ms()));
        if let Some(d) = logf.parent() { let _ = std::fs::create_dir_all(d); }
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
        let send_frames = |frames: &mut Vec<i64>| -> bool { if frames.is_empty() { return true } let Some(leader) = self.leader() else { return true };
            let mut v = json!({ "node_id": self.id, "job_id": job, "frames": frames }); if let Some(e) = a.epoch { v["edit_epoch"] = json!(e); }
            match http::call(&leader, "POST", "/api/dispatch/frame-complete", Some(&self.secret), Some(&v.to_string()), Duration::from_secs(2), Duration::from_secs(5)) {
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
            (None, Some(0)) => { let _ = send_frames(&mut frames); report("complete", json!({ "elapsed_ms": 0, "exit_code": 0 })); log("chunk complete"); }
            (None, code) => { log(format!("the renderer ended with {code:?}: failed")); failed(); }
        }
        self.idle();
    }
    fn idle(&self) { let mut w = self.work.lock().unwrap(); w.running = None; w.busy = !w.queue.is_empty(); }

    /// Runs the node until `stop` is set, then leaves the farm cleanly.
    pub fn run(self: &Arc<Self>) -> Result<(), String> {
        let listener = std::net::TcpListener::bind(("0.0.0.0", self.port)).map_err(|e| format!("port {}: {e}", self.port))?;
        self.write_endpoint();
        { let (me, stop) = (self.clone(), self.stop.clone()); std::thread::spawn(move || http::serve(listener, stop, move |q| me.answer(q))); }
        // the peers, every 3 s
        { let me = self.clone(); std::thread::spawn(move || while !me.stop.load(Ordering::Relaxed) { me.cycle(); for _ in 0..30 { if me.stop.load(Ordering::Relaxed) { break } std::thread::sleep(Duration::from_millis(100)); } }); }
        // reports to the leader: in order, kept until a 200 comes, five seconds after a failure (2.3)
        { let me = self.clone(); std::thread::spawn(move || while !me.stop.load(Ordering::Relaxed) {
            let next = me.reports.lock().unwrap().front().cloned();
            let Some((path, body)) = next else { std::thread::sleep(Duration::from_millis(100)); continue };
            let ok = me.leader().and_then(|l| http::call(&l, "POST", &path, Some(&me.secret), Some(&body), Duration::from_secs(2), Duration::from_secs(5))).is_some_and(|(s, _)| s == 200);
            if ok { me.reports.lock().unwrap().pop_front(); } else { for _ in 0..50 { if me.stop.load(Ordering::Relaxed) { break } std::thread::sleep(Duration::from_millis(100)); } } }); }
        // the jobs list, while rendering (2.6): a job that is gone from it, or cancelled, is killed at once and reported failed
        { let me = self.clone(); std::thread::spawn(move || while !me.stop.load(Ordering::Relaxed) { std::thread::sleep(Duration::from_secs(2));
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
        if let Some(l) = self.leader() { for (path, body) in self.reports.lock().unwrap().drain(..) { let _ = http::call(&l, "POST", &path, Some(&self.secret), Some(&body), Duration::from_secs(1), Duration::from_secs(2)); } }
        let _ = std::fs::remove_file(self.endpoint_file());
        Ok(())
    }
}
/// A peer's record whose read keys have the right types (2.4): anything else counts as a failed poll.
fn peer_record_is_sound(r: &Value) -> bool {
    let s = |k: &str| r.get(k).is_none_or(Value::is_string); let b = |k: &str| r.get(k).is_none_or(Value::is_boolean);
    r.is_object() && ["node_id", "hostname", "os", "app_version", "gpu_name", "node_state", "render_state", "active_job", "active_chunk", "endpoint", "secret_fingerprint", "agent_health", "alert_reason"].iter().all(|k| s(k))
        && r.get("tags").is_none_or(|t| t.as_array().is_some_and(|a| a.iter().all(Value::is_string))) && r.get("ram_mb").is_none_or(Value::is_u64) && r.get("cpu_cores").is_none_or(|v| v.is_i64() || v.is_u64()) && b("ready_for_work")
}
