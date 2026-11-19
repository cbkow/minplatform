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
    rtt_ms: Option<f64>, /// since when its polls have been answered by a node of another id (row 401)
    other_since: Option<Instant> }
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

/// A job as the window lists it (2.6).
pub struct JobRow { pub id: String, pub state: String, pub priority: i64, pub total: i64, pub done: i64, pub stopped: i64, pub first_ms: i64, pub last_ms: i64, pub created_ms: i64, /// where the job writes, and its output's name with the padding token - as manifests carry them (Windows form)
    pub output_dir: String, pub output_stem: String }
/// The picture the window shows of a job's output: the frame found, decoded and made small here - never on the
/// window's thread. `note` is what to say where there is no picture.
pub struct Preview { /// the EXR layer shown ("" = the file's own picture)
    pub layer: String, pub job: String, pub file: String, pub pin: Option<(i64, i64)>, pub size: (u32, u32), pub rgba: Arc<Vec<u8>>, pub note: Option<&'static str>, /// changes when the pixels do
    pub stamp: u64 }
/// One layer of an EXR for the layers card: its name and its picture made small (no size: it could not be read).
pub struct LayerTile { pub name: String, pub size: (u32, u32), pub rgba: Vec<u8> }
/// A chunk as the window shows it. `done` holds the finished frames only while the chunk is not `completed` (the
/// frame grid needs them then; a completed chunk is all done, and a hundred thousand lists would be kept for nothing).
pub struct ChunkRow { pub id: i64, pub a: i64, pub b: i64, pub state: String, pub node: String, pub assigned_ms: i64, pub completed_ms: i64, pub retries: i64, pub done: Vec<i64>, pub done_count: i64 }
/// One job with its chunks, in the order of their first frames.
pub struct Detail { pub id: String, pub state: String, pub priority: i64, pub created_ms: i64, pub first: i64, pub last: i64, pub chunks: Vec<ChunkRow> }
/// What the Logs tab asks for: a node's log, or a job's render output (with the file chosen there, if one is).
#[derive(Clone, PartialEq)]
pub enum LogWant { Node(String), Task(String, Option<String>) }
/// What the Logs tab shows: the lines, and for a job's output the files there are - (file name, the label shown).
#[derive(Default)]
pub struct LogData { pub lines: Vec<String>, pub files: Vec<(String, String)>, /// the lines are the end of a longer file
    pub cut: bool }
/// A peer as the window lists it.
pub struct PeerRow { /// benched by the leader after this many failures
    pub suspended: Option<i64>, pub id: String, pub host: String, pub alive: bool, pub leader: bool, pub stopped: bool, pub job: String, pub health_ok: bool }
/// What the window shows of the farm, as of now. The job list is made off the window's thread every two seconds and
/// handed over whole; the window never asks the leader or the database for anything itself.
pub struct View { /// why there is no farm (row 425)
    pub no_farm: Option<String>, /// this node is benched by the leader, after this many failures
    pub suspended: Option<i64>, pub me: Value, pub leads: bool, pub peers: Vec<PeerRow>, pub jobs: Arc<Vec<JobRow>>, /// false: the list is the last one the leader gave, and the leader does not answer now
    pub leader_answers: bool }
pub struct Node {
    /// why this node has no farm (row 425), and the sync root its settings name
    pub no_farm: Option<String>, wanted_root: String,
    pub id: String, pub data: PathBuf, farm: PathBuf, secret: String, port: u16, ip: String, tags: Vec<String>, hostname: String, cores: usize, ram_mb: u64,
    stopped: AtomicBool, pub stop: Arc<AtomicBool>, /// another node, or this one's window, asked for the application to start again
    pub restart_asked: AtomicBool, /// the graphics card's name, once found; the nodes the leader has benched: (id, failures); lines for the window's toast
    /// the jobs whose log folder this node, leading, has made on the share (row 424)
    job_dirs: Mutex<std::collections::BTreeSet<String>>, /// the shared folder's small duties (row 429): when they were last looked at; the content is settled
    share_looked: Mutex<Instant>, content_settled: AtomicBool, /// since when this node has been trying to take the leader's place (row 430)
    takeover_since: Mutex<Option<Instant>>, gpu: Mutex<String>, suspended: Mutex<Vec<(String, i64)>>, pub notes: Mutex<Vec<String>>, peers: Mutex<BTreeMap<String, Peer>>, work: Mutex<Work>, /// reports to the leader, in order: (path, body)
    reports: Mutex<VecDeque<(String, String)>>, wrote_endpoint: Mutex<Option<Instant>>, /// the database and the queue, while this node leads
    lead: Mutex<Option<Lead>>, /// the job list for the window, and whether the leader answered the last time it was asked
    jobs_view: Mutex<(Arc<Vec<JobRow>>, bool)>, /// the job the window shows in detail, and that detail as last fetched
    detail_of: Mutex<Option<String>>, detail_view: Mutex<Option<Arc<Detail>>>, /// the job (and the frames pinned) the window wants a picture of; the picture
    /// the file the preview shows, on this machine; the EXR layer chosen for a job (kept until the application ends)
    preview_path: Mutex<String>, layer_of: Mutex<BTreeMap<String, String>>,
    preview_of: Mutex<Option<(String, Option<(i64, i64)>)>>, preview_view: Mutex<Option<Arc<Preview>>>, /// what the Logs tab wants, and what was read for it
    log_of: Mutex<Option<LogWant>>, log_view: Mutex<Arc<LogData>>, /// renders go to a folder of this machine first (4.11; read at the farm's start)
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
    // (pictures for the manual are taken on a demo farm whose machines have made-up names)
    if let Some(h) = std::env::var("MR_DEMO_HOST").ok().filter(|h| !h.is_empty()) { return h }
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
        // NO FARM (row 425; 0.5.x's "No Farm"): no settings yet, no sync root, or a sync root that is not there now (a
        // share not mounted at sign-in). The application does not leave: the node runs ALONE, on the loopback address,
        // on an empty farm of its own inside the data folder - so the window, its Settings and the tray are all there -
        // and it says so. It takes no submission and sends no heartbeat while it is so, and it looks for the sync root
        // every few seconds: when the folder is there, the application starts itself again on the real farm.
        let _ = std::fs::create_dir_all(data);
        let (cfg, mut no_farm): (Value, Option<String>) = match std::fs::read_to_string(data.join("config.json")) { Err(_) => (json!({}), Some("The sync root is not set.".to_string())),
            Ok(t) => match serde_json::from_str(&t) { Ok(v) => (v, None), Err(e) => (json!({}), Some(format!("config.json could not be read ({e}); it is left as it is."))) } };
        let wanted = cfg["sync_root"].as_str().unwrap_or("").to_string();
        if no_farm.is_none() { if wanted.is_empty() { no_farm = Some("The sync root is not set.".to_string()); } else if !Path::new(&wanted).is_dir() { no_farm = Some(format!("The sync root is not reachable: {wanted}")); } }
        let park = data.join("no-farm"); if no_farm.is_some() { std::fs::create_dir_all(&park).map_err(|e| format!("the data folder cannot be written: {e}"))?; }
        let root_owned = if no_farm.is_some() { park.to_string_lossy().into_owned() } else { wanted.clone() }; let root = root_owned.as_str();
        let farm = Path::new(root).join("MinRender-v2"); let (ip, port, hostname) = (if no_farm.is_some() { "127.0.0.1".to_string() } else { advertised_ip(cfg["ip_override"].as_str().unwrap_or(""), root) }, cfg["http_port"].as_u64().unwrap_or(8420) as u16, host_name());
        // The node's id (1.1): twelve characters in node_id.txt, made once. A machine that has LOST the file (its data
        // folder emptied, the application put on anew) does not take a new id if the farm's phonebook still has an
        // entry at this machine's own address and port: it comes back as that node (row 401). A new id there leaves the
        // old entry for every node to take for alive - the newcomer answers its polls - and chunks given to it are lost.
        let idf = data.join("node_id.txt"); let mut adopted = false;
        let id = match std::fs::read_to_string(&idf).ok().map(|t| t.lines().next().unwrap_or("").to_string()).filter(|t| t.len() == 12) { Some(id) => id,
            // (with no farm an id is made for this run and NOT kept: the real farm's phonebook may hold this machine's own, to be taken up again)
            None if no_farm.is_some() => crate::share::hex(&crate::share::random(6)?),
            None => { let id = match former_self(&farm, &ip, port, &hostname) { Some(id) => { adopted = true; id } None => crate::share::hex(&crate::share::random(6)?) }; std::fs::write(&idf, &id).map_err(|e| e.to_string())?; id } };
        let secret = crate::share::start_farm(&farm, &id, now_ms())?;
        crate::share::log_to(farm.join("nodes").join(&id)); let _ = std::fs::create_dir_all(farm.join("nodes").join(&id));
        if adopted { crate::share::log("WARN", "farm", &format!("this machine had lost its node id; the phonebook has an entry at its address, and it returns as that node ({id})")); }
        let tags: Vec<String> = cfg["tags"].as_array().map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect()).unwrap_or_default();
        if let Some(why) = &no_farm { crate::share::log("WARN", "farm", &format!("NO FARM: {why} The node runs alone until the sync root is there.")); }
        Ok(Arc::new(Node { udp: if no_farm.is_none() && cfg["udp_enabled"].as_bool().unwrap_or(true) { let port = cfg["udp_port"].as_u64().unwrap_or(4243) as u16; match heartbeat_socket(port) { Ok(s) => Some((s, port)), Err(e) => { eprintln!("heartbeats are off (the socket: {e}); peers are found by the share and polled"); None } } } else { None },
            no_farm, wanted_root: wanted, id, data: data.to_path_buf(), farm, secret, port, ip, tags, hostname,
            job_dirs: Mutex::new(std::collections::BTreeSet::new()), share_looked: Mutex::new(Instant::now()), content_settled: AtomicBool::new(false), takeover_since: Mutex::new(None), gpu: Mutex::new(String::new()), suspended: Mutex::new(Vec::new()), notes: Mutex::new(Vec::new()), cores: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1), ram_mb: ram_mb(), stopped: AtomicBool::new(cfg["node_stopped"].as_bool().unwrap_or(false)), stop: Arc::new(AtomicBool::new(false)), restart_asked: AtomicBool::new(false),
            preview_path: Mutex::new(String::new()), layer_of: Mutex::new(BTreeMap::new()), peers: Mutex::new(BTreeMap::new()), work: Mutex::new(Work::default()), reports: Mutex::new(VecDeque::new()), wrote_endpoint: Mutex::new(None), lead: Mutex::new(None), jobs_view: Mutex::new((Arc::new(Vec::new()), true)), detail_of: Mutex::new(None), detail_view: Mutex::new(None), preview_of: Mutex::new(None), preview_view: Mutex::new(None), log_of: Mutex::new(None), log_view: Mutex::new(Arc::new(LogData::default())), staging: cfg["staging_enabled"].as_bool().unwrap_or(false),
            submitted: Mutex::new(Default::default()),
            mappings: cfg["path_mappings"].as_array().map(|a| a.iter().map(|m| { let s = |k: &str| m[k].as_str().unwrap_or("").to_string(); min_paths::map::Mapping { id: String::new(), label: s("label"), win: s("win"), mac: s("mac"), lin: s("lin"), enabled: m["enabled"].as_bool().unwrap_or(true) } }).collect()).unwrap_or_default() }))
    }
    fn endpoint_file(&self) -> PathBuf { self.farm.join("nodes").join(&self.id).join("endpoint.json") }
    /// The phonebook entry (1.2): a temporary file, then a rename; every write carries a new time.
    fn write_endpoint(&self) { let f = self.endpoint_file(); if let Some(d) = f.parent() { let _ = std::fs::create_dir_all(d); }
        let text = serde_json::to_string_pretty(&json!({ "node_id": self.id, "ip": self.ip, "port": self.port, "timestamp_ms": now_ms(), "hostname": self.hostname })).unwrap();
        match write_atomically(&f, &text) { Ok(()) => *self.wrote_endpoint.lock().unwrap() = Some(Instant::now()), Err(e) => log(format!("the phonebook entry could not be written: {e}")) } }
    /// The record this node gives of itself (2.4).
    fn record(&self) -> Value {
        let w = self.work.lock().unwrap(); let r = w.running.as_ref();
        json!({ "node_id": self.id, "hostname": self.hostname, "os": OS, "app_version": env!("CARGO_PKG_VERSION"), "gpu_name": *self.gpu.lock().unwrap(), "cpu_cores": self.cores, "ram_mb": self.ram_mb,
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
            // (the application starts itself again - on macOS too, where 0.5.x answered 500: it needs no helper)
            ("POST", "/api/node/restart") => { self.restart_asked.store(true, Ordering::Relaxed); Response::ok() }
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
            // Farm Cleanup's scan and its acts (2.5 rows 34, 35), as 0.5.x answers them
            ("POST", "/api/farm/scan-cleanup") => Response::json(200, self.scan_cleanup()),
            ("POST", "/api/farm/cleanup") => { let v = match body() { Ok(v) => v, Err(r) => return r }; let (Some(action), Some(ids)) = (v["action"].as_str(), v["ids"].as_array()) else { return Response::error(400, "key 'action' or 'ids' not found") };
                let n = self.cleanup(action, &ids.iter().filter_map(|x| x.as_str().map(str::to_string)).collect::<Vec<_>>()); Response::json(200, json!({ "status": "ok", "count": n })) }
            ("GET", "/api/config") => { let mut v = std::fs::read_to_string(self.data.join("config.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).unwrap_or(json!({}));
                for (k, x) in [("node_id", json!(self.id)), ("hostname", json!(self.hostname)), ("cpu_cores", json!(self.cores)), ("ram_mb", json!(self.ram_mb)), ("gpu_name", json!(*self.gpu.lock().unwrap())), ("farm_running", json!(true)), ("farm_path", json!(self.farm.to_string_lossy())), ("farm_error", json!("")), ("is_leader", json!(self.lead.lock().unwrap().is_some()))] { v[k] = x; }
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
                // (row 424) the leader makes the job's log folder ONCE, before the first chunk is given out: each node then adds only
                // its own folder under it, and no two machines make the same folder at the same moment (storage that
                // answers that with a second, "conflicted" folder lost a node's render output)
                if let Some(job) = manifest["job_id"].as_str().filter(|j| safe(j)) { if self.job_dirs.lock().unwrap().insert(job.to_string()) { let _ = std::fs::create_dir_all(self.farm.join("jobs").join(job).join("stdout")); } }
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
    /// The job whose chunks the window wants (or none). The detail is fetched on the node's own thread.
    pub fn show_detail(&self, id: Option<String>) { let mut d = self.detail_of.lock().unwrap(); if *d != id { *d = id; *self.detail_view.lock().unwrap() = None; } }
    pub fn detail(&self) -> Option<Arc<Detail>> { self.detail_view.lock().unwrap().clone() }
    /// A job's chunks for the window, fetched here: from this node's database when it leads, else from its leader. A
    /// fetch that fails leaves the last detail standing (0.5.x blanked the table). A job that is gone clears it.
    fn refresh_detail(self: &Arc<Self>) {
        let Some(id) = self.detail_of.lock().unwrap().clone() else { return }; let path = format!("/api/jobs/{id}");
        let got = if self.lead.lock().unwrap().is_some() { self.lead_answer("GET", &path, b"").map(|r| (r.status, r.body)) } else { self.leader().and_then(|l| http::call(&l, "GET", &path, Some(&self.secret), None, Duration::from_millis(500), Duration::from_secs(10))) };
        let Some((status, body)) = got else { return };
        if status == 404 { if self.detail_of.lock().unwrap().as_deref() == Some(id.as_str()) { *self.detail_view.lock().unwrap() = None; } return }
        let Some(v) = (status == 200).then(|| serde_json::from_slice::<Value>(&body).ok()).flatten() else { return }; let n = |j: &Value, k: &str| j[k].as_i64().unwrap_or(0);
        let chunks = v["chunks"].as_array().map(|a| a.iter().map(|c| { let state = c["state"].as_str().unwrap_or("").to_string(); let frames: Vec<i64> = c["completed_frames"].as_array().map(|f| f.iter().filter_map(Value::as_i64).collect()).unwrap_or_default();
            ChunkRow { id: n(c, "id"), a: n(c, "frame_start"), b: n(c, "frame_end"), node: c["assigned_to"].as_str().unwrap_or("").to_string(), assigned_ms: n(c, "assigned_at_ms"), completed_ms: n(c, "completed_at_ms"), retries: n(c, "retry_count"), done_count: frames.len() as i64, done: if state == "completed" { Vec::new() } else { frames }, state } }).collect()).unwrap_or_default();
        let d = Detail { id: id.clone(), state: v["current_state"].as_str().unwrap_or("").to_string(), priority: n(&v, "priority"), created_ms: n(&v, "submitted_at_ms"), first: n(&v["manifest"], "frame_start"), last: n(&v["manifest"], "frame_end"), chunks };
        if self.detail_of.lock().unwrap().as_deref() == Some(id.as_str()) { *self.detail_view.lock().unwrap() = Some(Arc::new(d)); }
    }
    /// What the Logs tab wants read (none while the tab is not in front: nothing is read then).
    pub fn show_log(&self, want: Option<LogWant>) { let mut w = self.log_of.lock().unwrap(); if *w != want { *w = want; *self.log_view.lock().unwrap() = Arc::new(LogData::default()); } }
    pub fn log(&self) -> Arc<LogData> { self.log_view.lock().unwrap().clone() }
    /// The Logs tab's reading, here and never on the window's thread: the last 500 lines of a node's log on the share
    /// (the files this machine's yesterday and today name), or a job's chunk logs listed - in the order of their frames,
    /// then their times - and the END of the one chosen (a render's output can be very long: the last 256 KB).
    fn refresh_log(self: &Arc<Self>) {
        let Some(want) = self.log_of.lock().unwrap().clone() else { return };
        let data = match &want {
            LogWant::Node(id) => LogData { lines: if safe(id) { crate::share::node_log(&self.farm, id, 500) } else { Vec::new() }, files: Vec::new(), cut: false },
            LogWant::Task(job, file) => { let mut found: Vec<(String, i64, String, String)> = Vec::new();   // (range, time, node, file name)
                if safe(job) { if let Some(a) = crate::share::task_output(&self.farm, job).as_array() { for f in a { let path = f["path"].as_str().unwrap_or(""); found.push((f["range"].as_str().unwrap_or("").to_string(), f["timestamp_ms"].as_i64().unwrap_or(0), f["node_id"].as_str().unwrap_or("").to_string(), path.rsplit(['/', '\\']).next().unwrap_or("").to_string())); } } }
                found.sort();
                let host = |id: &str| -> String { if id == self.id { return self.hostname.clone() } self.peers.lock().unwrap().get(id).and_then(|p| p.record.as_ref()).and_then(|r| r["hostname"].as_str().map(str::to_string)).filter(|h| !h.is_empty()).unwrap_or_else(|| id.to_string()) };
                // (the label: the frames as the chunk table writes them, the time, the node's name - row 389)
                let files: Vec<(String, String)> = found.iter().map(|(range, ms, node, name)| { let plain = range.split('-').map(|x| x.trim_start_matches('0')).map(|x| if x.is_empty() { "0" } else { x }).collect::<Vec<_>>(); let frames = if plain.len() == 2 && plain[0] == plain[1] { plain[0].to_string() } else { plain.join("-") };
                    (format!("{node}/{name}"), format!("{frames}  \u{b7}  {}  \u{b7}  {}", crate::share::local_stamp(*ms).get(11..).unwrap_or(""), host(node))) }).collect();
                let (mut lines, mut cut) = (Vec::new(), false);
                if let Some((node, name)) = file.as_ref().and_then(|f| f.split_once('/')) { if safe(node) && safe(name) { if let Ok(mut fh) = std::fs::File::open(self.farm.join("jobs").join(job).join("stdout").join(node).join(name)) { use std::io::{Read, Seek, SeekFrom};
                    let len = fh.metadata().map(|m| m.len()).unwrap_or(0); let keep = 256 * 1024; if len > keep { cut = true; let _ = fh.seek(SeekFrom::Start(len - keep)); } let mut b = Vec::new(); let _ = fh.read_to_end(&mut b);
                    lines = String::from_utf8_lossy(&b).lines().skip(if cut { 1 } else { 0 }).map(str::to_string).collect(); } } }
                LogData { lines, files, cut } } };
        if *self.log_of.lock().unwrap() == Some(want) { *self.log_view.lock().unwrap() = Arc::new(data); }
    }
    /// The job the window wants a picture of, and the frames it is pinned to (none: the latest).
    pub fn show_preview(&self, want: Option<(String, Option<(i64, i64)>)>) { let mut p = self.preview_of.lock().unwrap(); if *p != want { if p.as_ref().map(|x| &x.0) != want.as_ref().map(|x| &x.0) { *self.preview_view.lock().unwrap() = None; } *p = want; } }
    pub fn preview(&self) -> Option<Arc<Preview>> { self.preview_view.lock().unwrap().clone() }
    /// The picture for the window (the spec's 3.2-3.4), made here. The job's output folder, through this machine's
    /// mappings, is listed once - its own files only; the file whose name carries the HIGHEST frame number wins (inside
    /// the pin, when there is one); it is decoded small. A file written again under the same name is decoded again
    /// (0.5.x kept the old picture); one that cannot be read yet says so and is tried at the next turn (row 389).
    fn refresh_preview(self: &Arc<Self>, last: &mut Option<(PathBuf, u64, u64)>) {
        let Some((job, pin)) = self.preview_of.lock().unwrap().clone() else { *last = None; return };
        let Some((dir, stem)) = self.jobs_view.lock().unwrap().0.iter().find(|j| j.id == job).map(|j| (j.output_dir.clone(), j.output_stem.clone())) else { return };
        let layer = self.layer_of.lock().unwrap().get(&job).cloned().unwrap_or_default();
        let put = |file: String, size: (u32, u32), rgba: Vec<u8>, note: Option<&'static str>| { if self.preview_of.lock().unwrap().as_ref().map(|x| x.0.as_str()) == Some(job.as_str()) { *self.preview_view.lock().unwrap() = Some(Arc::new(Preview { layer: layer.clone(), job: job.clone(), file, pin, size, rgba: Arc::new(rgba), note, stamp: now_ms() })); } };
        let nothing = if pin.is_some() { "Not rendered yet" } else { "No frames rendered yet" };
        let here = PathBuf::from(min_paths::map::from_canonical_windows_form(&dir, &self.mappings)); if dir.is_empty() || !here.is_dir() { *last = None; put(String::new(), (0, 0), Vec::new(), Some(nothing)); return }
        // the output's name as a pattern: each run of `#` is the frame's number
        let name = stem.rsplit(['/', '\\']).next().unwrap_or(""); let pattern = { let mut p = String::from("(?i)^"); let mut hashes = false; let mut any = false;
            for ch in name.replace("[#", "#").replace("#]", "#").chars() { if ch == '#' { if !hashes { p.push_str("(\\d+)"); hashes = true; any = true; } } else { hashes = false; p.push_str(&regex::escape(&ch.to_string())); } }
            if !any { p.push_str("(\\d+)\\.\\w+"); } else { p.push_str("(\\.\\w+)?"); } p.push('$'); if name.is_empty() { None } else { regex::Regex::new(&p).ok() } };
        let tail = regex::Regex::new("(\\d+)\\.\\w+$").unwrap();
        let mut best: Option<(i64, PathBuf)> = None; let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
        if let Ok(rd) = std::fs::read_dir(&here) { for e in rd.flatten() { let p = e.path(); let fname = e.file_name().to_string_lossy().into_owned();
            if !p.extension().and_then(|x| x.to_str()).is_some_and(|x| ["exr", "png", "jpg", "jpeg", "tif", "tiff", "bmp", "tga"].contains(&x.to_ascii_lowercase().as_str())) || !p.is_file() { continue }
            let frame = pattern.as_ref().and_then(|r| r.captures(&fname)).or_else(|| if pattern.is_none() { tail.captures(&fname) } else { None }).and_then(|c| c.get(1)).and_then(|m| m.as_str().parse::<i64>().ok());
            match frame { Some(f) => if pin.is_none_or(|(a, b)| a <= f && f <= b) && best.as_ref().is_none_or(|(bf, _)| f > *bf) { best = Some((f, p)); },
                None => if let Ok(t) = e.metadata().and_then(|m| m.modified()) { if newest.as_ref().is_none_or(|(nt, _)| t > *nt) { newest = Some((t, p)); } } } } }
        let Some(path) = best.map(|(_, p)| p).or(if pin.is_none() { newest.map(|(_, p)| p) } else { None }) else { *last = None; put(String::new(), (0, 0), Vec::new(), Some(nothing)); return };
        let meta = std::fs::metadata(&path).ok(); let key = (path.clone(), meta.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0), meta.map(|m| m.len()).unwrap_or(0));
        let fname = path.file_name().unwrap_or_default().to_string_lossy().into_owned(); *self.preview_path.lock().unwrap() = path.to_string_lossy().into_owned();
        if last.as_ref() == Some(&key) && self.preview().is_none_or(|p| p.layer == layer) { if self.preview_view.lock().unwrap().as_ref().is_some_and(|p| p.pin != pin) { if let Some(old) = self.preview() { put(fname, old.size, old.rgba.as_ref().clone(), old.note); } } return }
        // (a layer that this file does not have - another frame, another pass set - shows the file's own picture)
        match min_image::load_thumbnail(&path.to_string_lossy(), &layer, 960).or_else(|e| if layer.is_empty() { Err(e) } else { min_image::load_thumbnail(&path.to_string_lossy(), "", 960) }) { Ok(img) => { let (rgba, w, h) = opaque_rgba8(&img); *last = Some(key); put(fname, (w, h), rgba, None); }
            Err(_) => { *last = None; put(fname, (0, 0), Vec::new(), Some("This frame cannot be shown yet")); } }
    }
    /// The EXR layer the preview shows for a job ("" = the file's own picture), and the choice of one.
    pub fn layer(&self, job: &str) -> String { self.layer_of.lock().unwrap().get(job).cloned().unwrap_or_default() }
    pub fn set_layer(&self, job: &str, layer: &str) { let mut m = self.layer_of.lock().unwrap(); if layer.is_empty() || layer == "default" { m.remove(job); } else { m.insert(job.to_string(), layer.to_string()); } }
    /// The layers of the file the preview shows, for the layers card: listed, then each made small, one after the
    /// other, off the window's thread. The flag says the list is whole.
    pub fn layers_for_window(self: &Arc<Self>, into: Arc<Mutex<(Vec<LayerTile>, bool)>>) {
        let path = self.preview_path.lock().unwrap().clone(); std::thread::spawn(move || {
            for name in min_image::exr::layers(&path).unwrap_or_default() { if Arc::strong_count(&into) < 2 { return }   // the card was closed
                let tile = match min_image::load_thumbnail(&path, if name == "default" { "" } else { &name }, 240) { Ok(img) => { let (rgba, w, h) = opaque_rgba8(&img); LayerTile { name, size: (w, h), rgba } } Err(_) => LayerTile { name, size: (0, 0), rgba: Vec::new() } };
                into.lock().unwrap().0.push(tile); }
            into.lock().unwrap().1 = true; });
    }
    /// What Farm Cleanup could clear (2.5 row 34, 6.7), as this node sees it: finished and archived jobs (a leader
    /// alone knows these), folders under `jobs/` no job owns, peers that do not answer, this machine's staging folders.
    pub fn scan_cleanup(self: &Arc<Self>) -> Value {
        let item = |id: String, label: String, detail: String| json!({ "id": id, "label": label, "detail": detail });
        let leading = self.lead.lock().unwrap().is_some(); let (mut finished, mut archived, mut known) = (Vec::new(), Vec::new(), std::collections::BTreeSet::new());
        if let Some(l) = self.lead.lock().unwrap().as_ref() { for (id, state, total) in l.all_jobs() { known.insert(id.clone()); if state == "completed" || state == "cancelled" { finished.push(item(id.clone(), id, format!("{state} | {total} chunks"))); } else if state == "archived" { archived.push(item(id.clone(), id, "archived".into())); } } }
        // (a node that does not lead knows the listed jobs only, not the archived ones: it names no folder an orphan - 0.5.x did,
        // and offered an archived job's folder for deletion)
        let mut orphans = Vec::new(); if leading { if let Ok(rd) = std::fs::read_dir(self.farm.join("jobs")) { for e in rd.flatten().filter(|e| e.path().is_dir()) { let name = e.file_name().to_string_lossy().into_owned(); if !known.contains(&name) { orphans.push(item(e.path().to_string_lossy().into_owned(), name, "no matching job".into())); } } } }
        let peers: Vec<Value> = self.peers.lock().unwrap().iter().filter(|(_, p)| !p.alive).map(|(id, p)| item(id.clone(), format!("{} ({})", p.record.as_ref().and_then(|r| r["hostname"].as_str()).unwrap_or(""), &id[..id.len().min(8)]), format!("last seen: {}", p.last_seen_ms))).collect();
        let (mut empty, mut full) = (Vec::new(), Vec::new());
        if let Ok(rd) = std::fs::read_dir(self.data.join("staging")) { for e in rd.flatten().filter(|e| e.path().is_dir()) { let (mut files, mut bytes) = (0u64, 0u64); let mut stack = vec![e.path()];
            while let Some(d) = stack.pop() { if let Ok(rd) = std::fs::read_dir(&d) { for f in rd.flatten() { let p = f.path(); if p.is_dir() { stack.push(p); } else { files += 1; bytes += f.metadata().map(|m| m.len()).unwrap_or(0); } } } }
            let (id, name) = (e.path().to_string_lossy().into_owned(), e.file_name().to_string_lossy().into_owned());
            if files > 0 { full.push(item(id, name, format!("{files} files ({})", if bytes < 1 << 20 { format!("{} KB", bytes >> 10) } else if bytes < 1 << 30 { format!("{} MB", bytes >> 20) } else { format!("{} GB", bytes >> 30) }))); } else { empty.push(item(id, name, "empty".into())); } } }
        json!({ "finished_jobs": finished, "archived_jobs": archived, "orphaned_dirs": orphans, "stale_peers": peers, "stale_staging_dirs": empty, "failed_staging_copies": full, "is_leader": leading })
    }
    /// One act of Farm Cleanup (row 35) on this node; how many it did. Folders are removed only under the farm's `jobs/`
    /// and this machine's staging folder, whatever the request names.
    pub fn cleanup(self: &Arc<Self>, action: &str, ids: &[String]) -> usize {
        let mut n = 0;
        for id in ids { match action {
            "archive" | "delete_jobs" => if safe(id) { let (m, p) = if action == "archive" { ("POST", format!("/api/jobs/{id}/archive")) } else { ("DELETE", format!("/api/jobs/{id}")) }; if self.lead_answer(m, &p, b"").is_some_and(|r| r.status == 200) { n += 1; } },
            "delete_dirs" => { let t = PathBuf::from(id); let within = |root: PathBuf| t.is_absolute() && !id.contains("..") && t.parent().is_some_and(|p| p == root); if (within(self.farm.join("jobs")) || within(self.data.join("staging"))) && std::fs::remove_dir_all(&t).is_ok() { n += 1; } }
            "remove_peers" => if safe(id) && *id != self.id { if std::fs::remove_dir_all(self.farm.join("nodes").join(id)).is_ok() { n += 1; } self.peers.lock().unwrap().remove(id); },
            _ => {} } }
        n
    }
    /// Farm Cleanup for the window, off its thread. The jobs a leader alone knows are asked of the leader; the rest is this machine's.
    pub fn cleanup_scan_for_window(self: &Arc<Self>, into: Arc<Mutex<Option<Value>>>) {
        let me = self.clone(); std::thread::spawn(move || { let mut v = me.scan_cleanup();
            if me.lead.lock().unwrap().is_none() { if let Some(lv) = me.to_leader("/api/farm/scan-cleanup", "").filter(|(s, _)| *s == 200).and_then(|(_, b)| serde_json::from_slice::<Value>(&b).ok()) { for k in ["finished_jobs", "archived_jobs", "orphaned_dirs"] { v[k] = lv[k].clone(); } v["leader_answered"] = json!(true); } else { v["leader_answered"] = json!(false); } }
            *into.lock().unwrap() = Some(v); });
    }
    /// One act for the window, off its thread: jobs and their folders are the leader's to clear, peers and staging this node's; then a new scan.
    pub fn cleanup_for_window(self: &Arc<Self>, action: &'static str, ids: Vec<String>, into: Arc<Mutex<Option<Value>>>) {
        let me = self.clone(); std::thread::spawn(move || { let at_leader = matches!(action, "archive" | "delete_jobs") || action == "delete_dirs" && ids.iter().all(|i| !i.contains("staging"));
            if at_leader && me.lead.lock().unwrap().is_none() { let _ = me.to_leader("/api/farm/cleanup", &json!({ "action": action, "ids": ids }).to_string()); } else { me.cleanup(action, &ids); }
            me.refresh_jobs_view(); me.cleanup_scan_for_window(into); });
    }
    /// This node's settings as they are kept (`config.json`), for the Settings card.
    pub fn settings(&self) -> Value { std::fs::read_to_string(self.data.join("config.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(json!({})) }
    /// The Settings card's Save (row 389): the keys given are laid over the file as it is - what the file holds besides
    /// stays - and the file is written whole, then moved into place. A file that is there and cannot be read is not replaced.
    pub fn save_settings(&self, changes: &Value) -> Result<(), String> {
        let f = self.data.join("config.json"); let text = std::fs::read_to_string(&f).unwrap_or_else(|_| "{}".to_string());   // (a first run has no file yet)
        let mut v: Value = serde_json::from_str(&text).map_err(|e| format!("config.json could not be read as JSON ({e}); it is left as it is"))?;
        for (k, x) in changes.as_object().into_iter().flatten() { v[k] = x.clone(); }
        write_atomically(&f, &serde_json::to_string_pretty(&v).unwrap()).map_err(|e| format!("config.json could not be written: {e}"))
    }
    /// The secret's fingerprint as this node holds it, and how many peers that answer show another one.
    pub fn secret_state(&self) -> (String, usize) { let mine = fingerprint(&self.secret); let other = self.peers.lock().unwrap().values().filter(|p| p.alive && p.record.as_ref().and_then(|r| r["secret_fingerprint"].as_str()).is_some_and(|f| !f.is_empty() && f != mine)).count(); (mine, other) }
    /// The farm's folder on the share (the form reads the templates there).
    pub fn farm(&self) -> &Path { &self.farm }
    pub fn mappings(&self) -> &[min_paths::map::Mapping] { &self.mappings }
    /// A job from the New Job form (8.1): checked, given its id, baked, put in the form manifests travel in, handed to
    /// the leader - on a thread of its own. `done` gets the new job's id, or every reason it was not submitted.
    pub fn submit_form(self: &Arc<Self>, template: Value, values: Vec<String>, name: String, a: i64, b: i64, chunk: i64, priority: i64, done: Arc<Mutex<Option<Result<String, Vec<String>>>>>) {
        let me = self.clone(); std::thread::spawn(move || {
            let exe = template["cmd"][OS].as_str().unwrap_or("").to_string(); let bad = crate::submit::refusals(&template, &values, &exe, &name, a, b, chunk);
            let r = if !bad.is_empty() { Err(bad) } else { match crate::submit::slug(&name, &me.farm) { None => Err(vec!["Job name produces an empty slug".to_string()]), Some(id) => {
                let mut m = crate::submit::bake(&template, &values, &exe, OS, &id, a, b, chunk, &me.id, now_ms()); if cfg!(target_os = "macos") { m = crate::submit::canonical(m, &me.mappings); }
                match me.to_leader("/api/jobs", &json!({ "manifest": m, "priority": priority }).to_string()) { Some((200, _)) => { me.refresh_jobs_view(); Ok(id) } Some((_, body)) => Err(vec![String::from_utf8_lossy(&body).into_owned()]), None => Err(vec!["Leader did not respond".to_string()]) } } } };
            *done.lock().unwrap() = Some(r); });
    }
    /// A job's manifest for the editor, off the window's thread: asked of the leader, its paths turned into this system's.
    pub fn fetch_manifest(self: &Arc<Self>, id: String, into: Arc<Mutex<Option<Result<Value, String>>>>) {
        let me = self.clone(); std::thread::spawn(move || { let path = format!("/api/jobs/{id}/manifest");
            let got = if me.lead.lock().unwrap().is_some() { me.lead_answer("GET", &path, b"").map(|r| (r.status, r.body)) } else { me.leader().and_then(|l| http::call(&l, "GET", &path, Some(&me.secret), None, Duration::from_secs(3), Duration::from_secs(10))) };
            let r = match got { None => Err("Leader unreachable \u{2014} check the farm connection".to_string()), Some((404, _)) => Err("The job no longer exists on the leader".to_string()),
                Some((200, b)) => serde_json::from_slice::<Value>(&b).ok().and_then(|v| v.get("manifest").filter(|m| m.is_object()).cloned()).map(|m| me.translated(m, OS)).ok_or("The stored job manifest is invalid".to_string()),
                Some((s, b)) => Err(format!("Couldn't fetch the job manifest: {s} {}", String::from_utf8_lossy(&b))) };
            *into.lock().unwrap() = Some(r); });
    }
    /// An edited manifest goes to the leader (2.5 row 22) in one of its three modes. A leader that does not answer at all:
    /// the edit is left in the share's command box (6.6), where a leader applies it within 15 minutes.
    pub fn apply_edit(self: &Arc<Self>, mut m: Value, mode: &'static str, priority: i64, done: Arc<Mutex<Option<Result<String, Vec<String>>>>>) {
        let me = self.clone(); std::thread::spawn(move || { if cfg!(target_os = "macos") { m = crate::submit::canonical(m, &me.mappings); } let id = m["job_id"].as_str().unwrap_or("").to_string();
            let r = match me.to_leader(&format!("/api/jobs/{id}/edit"), &json!({ "manifest": m, "mode": mode, "priority": priority }).to_string()) {
                Some((200, _)) => { me.refresh_jobs_view(); Ok(id) }
                Some((404, b)) => Err(if String::from_utf8_lossy(&b).contains("job_not_found") { "The job no longer exists on the leader".to_string() } else { "The leader is running an older version without job editing \u{2014} update the leader node".to_string() }),
                Some((503, _)) => Err("The leader's database isn't ready \u{2014} try again shortly".to_string()),
                Some((400, b)) => Err(format!("Edit rejected: {}", serde_json::from_slice::<Value>(&b).ok().and_then(|v| v["detail"].as_str().map(str::to_string)).unwrap_or_default())),
                Some((s, _)) => Err(format!("Leader error: {s}")),
                None => { let at = now_ms(); let stem = format!("{at}_edit_{id}_{}", me.id); let dir = me.farm.join("commands"); let _ = std::fs::create_dir_all(&dir);
                    let text = serde_json::to_string_pretty(&json!({ "type": "job_edit", "command_id": stem, "job_id": id, "manifest": m, "mode": mode, "priority": priority, "issued_at_ms": at, "issued_by": me.id })).unwrap_or_default();
                    let tmp = dir.join(format!("{stem}.tmp")); if std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, dir.join(format!("{stem}.json"))).is_ok() { Err("Leader unreachable \u{2014} edit queued to the farm and applies when the leader reconnects".to_string()) } else { Err("No response from the leader \u{2014} check the farm connection and try again".to_string()) } } };
            *done.lock().unwrap() = Some(r.map_err(|e| vec![e])); });
    }
    /// A job made from another's manifest (a chunk's frames as a job of their own): a new id from `name`, this node as
    /// its submitter, now as its time - then to the leader as any submission.
    pub fn submit_manifest(self: &Arc<Self>, mut m: Value, name: String, priority: i64, done: Arc<Mutex<Option<Result<String, Vec<String>>>>>) {
        let me = self.clone(); std::thread::spawn(move || {
            let r = match crate::submit::slug(&name, &me.farm) { None => Err(vec!["Job name produces an empty slug".to_string()]), Some(id) => {
                m["job_id"] = json!(id); m["submitted_by"] = json!(me.id); m["submitted_at_ms"] = json!(now_ms()); if cfg!(target_os = "macos") { m = crate::submit::canonical(m, &me.mappings); }
                match me.to_leader("/api/jobs", &json!({ "manifest": m, "priority": priority }).to_string()) { Some((200, _)) => { me.refresh_jobs_view(); Ok(id) } Some((_, body)) => Err(vec![String::from_utf8_lossy(&body).into_owned()]), None => Err(vec!["Leader did not respond".to_string()]) } } };
            *done.lock().unwrap() = Some(r); });
    }
    /// What the window asks of another node (the peers' menu): "stop", "start", "restart" - said to that node; "unsuspend" -
    /// said to the leader; "signal" - the restart file put in the node's folder on the share, for a node that does not answer.
    pub fn peer_command(self: &Arc<Self>, ids: Vec<String>, what: &'static str) {
        let me = self.clone(); std::thread::spawn(move || for id in ids { match what {
            "signal" => { let d = me.farm.join("nodes").join(&id); if d.is_dir() { let _ = std::fs::write(d.join("restart"), b""); } }
            "unsuspend" => { let _ = me.to_leader(&format!("/api/nodes/{id}/unsuspend"), ""); }
            _ => { let to = me.peers.lock().unwrap().get(&id).map(|p| p.endpoint.clone()); if let Some(to) = to { if http::call(&to, "POST", &format!("/api/node/{what}"), Some(&me.secret), Some(""), Duration::from_secs(3), Duration::from_secs(10)).is_none_or(|(s, _)| s != 200) { crate::share::log("WARN", "peer", &format!("{what} of node {id}: it did not take it")); } } } } });
    }
    pub fn toggle_stopped(&self) { let on = !self.stopped.load(Ordering::Relaxed); self.set_stopped(on); }
    /// What the window asks of chunks: "reassign" (back to the queue) or "stop" - said to the leader off the window's thread.
    pub fn chunk_command(self: &Arc<Self>, ids: Vec<i64>, what: &'static str) {
        let me = self.clone(); std::thread::spawn(move || { for id in ids { let body = json!({ "chunk_id": id }).to_string(); if me.to_leader(&format!("/api/chunks/{what}"), &body).is_none_or(|(s, _)| s != 200) { crate::share::log("WARN", "job", &format!("{what} of chunk {id}: the leader did not take it")); } } me.refresh_detail(); me.refresh_jobs_view(); });
    }
    /// What the window asks of the jobs it has chosen (the spec's 1.10): pause, resume, cancel, archive, requeue,
    /// retry-failed, delete - each said to the leader, on a thread of its own, never on the window's. As in 0.5.x a
    /// failure is a line in the log and nothing more. The list is fetched again at once afterwards.
    pub fn job_command(self: &Arc<Self>, ids: Vec<String>, what: &'static str) {
        let me = self.clone(); std::thread::spawn(move || { for id in ids {
            let (method, path) = if what == "delete" { ("DELETE", format!("/api/jobs/{id}")) } else { ("POST", format!("/api/jobs/{id}/{what}")) };
            let ok = if me.lead.lock().unwrap().is_some() { me.lead_answer(method, &path, b"").is_some_and(|r| r.status == 200) } else { me.leader().and_then(|l| http::call(&l, method, &path, Some(&me.secret), Some(""), Duration::from_secs(3), Duration::from_secs(10))).is_some_and(|(s, _)| s == 200) };
            // (a job deleted from a node that does not lead: that node takes the job's folder off the share itself as well)
            if what == "delete" && me.lead.lock().unwrap().is_none() && safe(&id) { let d = me.farm.join("jobs").join(&id); if d.is_dir() && std::fs::remove_dir_all(&d).is_err() { std::thread::sleep(Duration::from_millis(250)); let _ = std::fs::remove_dir_all(&d); } me.cut(Some(&id), Cut::Fail); }
            if what == "cancel" { me.cut(Some(&id), Cut::Fail); }
            if !ok { crate::share::log("WARN", "job", &format!("{what} of {id}: the leader did not take it")); } }
            me.refresh_jobs_view(); });
    }
    /// The plug-ins' drop box (8.2): each `*.json` in `submissions/` becomes a job - the template found by its id, one
    /// value a flag, the id from the name, the manifest baked and put in its travelling form, handed to the leader.
    /// A file is moved to `processed/` once the leader has taken or refused it, or when it cannot be a job at all,
    /// and the reason is logged. Unlike 0.5.x (ruled 2026-11-14, row 389): a submission is checked as the form
    /// checks one; a file the leader could not be told of STAYS and is tried again; no file is submitted twice.
    fn drop_box(self: &Arc<Self>) {
        if self.no_farm.is_some() { return }   // a submission waits for the real farm
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
            // (row 418) a path under no mapping is submitted as it is, and said: in the log, and by the window
            for v in values.iter().filter(|v| crate::submit::outside(v, &self.mappings)) { log(format!("{id}: a path outside every path mapping - only machines that have this exact path can render it: {v}")); self.notes.lock().unwrap().push(format!("{id}: {}", crate::submit::OUTSIDE)); }
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
    /// What the window shows (a copy; nothing here waits on another node).
    pub fn view(&self) -> View {
        let benched = self.suspended.lock().unwrap().clone();
        let peers = self.peers.lock().unwrap().iter().map(|(id, p)| { let r = p.record.as_ref(); let s = |k: &str| r.and_then(|r| r[k].as_str()).unwrap_or("").to_string();
            PeerRow { suspended: benched.iter().find(|(n, _)| n == id).map(|x| x.1), id: id.clone(), host: s("hostname"), alive: p.alive, leader: p.leader, stopped: s("node_state") == "stopped", job: if s("render_state") == "rendering" { s("active_job") } else { String::new() }, health_ok: r.is_none_or(|r| r.get("agent_health").is_none_or(|h| h == &json!("ok"))) } }).collect();
        let (jobs, leader_answers) = { let v = self.jobs_view.lock().unwrap(); (v.0.clone(), v.1) };
        View { no_farm: self.no_farm.clone(), suspended: benched.iter().find(|(n, _)| *n == self.id).map(|x| x.1), me: self.record(), leads: self.lead.lock().unwrap().is_some(), peers, jobs, leader_answers }
    }
    /// The job list for the window, fetched here - never on the window's thread: from this node's own queue when it
    /// leads, else from its leader. A leader that does not answer leaves the last list standing, marked (row 389).
    fn refresh_jobs_view(self: &Arc<Self>) {
        // (with no leader known yet - the first seconds after a start - there is nobody to be unreachable)
        if self.lead.lock().unwrap().is_none() && self.leader().is_none() { return }
        let body = if self.lead.lock().unwrap().is_some() { self.lead_answer("GET", "/api/jobs", b"").map(|r| r.body) } else { self.leader().and_then(|l| http::call(&l, "GET", "/api/jobs", Some(&self.secret), None, Duration::from_millis(500), Duration::from_secs(1))).filter(|(s, _)| *s == 200).map(|(_, b)| b) };
        let rows = body.and_then(|b| if b.is_empty() { Some(Vec::new()) } else { serde_json::from_slice::<Vec<Value>>(&b).ok().map(|a| a.iter().map(|j| { let n = |k: &str| j[k].as_i64().unwrap_or(0);
            JobRow { id: j["job_id"].as_str().unwrap_or("").to_string(), state: j["current_state"].as_str().unwrap_or("").to_string(), priority: n("priority"), total: n("total_chunks"), done: n("completed_chunks"), stopped: n("stopped_chunks"), first_ms: n("first_assigned_ms"), last_ms: n("last_completed_ms"), created_ms: n("submitted_at_ms"), output_dir: j["output_dir"].as_str().unwrap_or("").to_string(), output_stem: j["output_stem"].as_str().unwrap_or("").to_string() } }).collect()) });
        let mut v = self.jobs_view.lock().unwrap(); match rows { Some(r) => *v = (Arc::new(r), true), None => v.1 = false }
    }
    /// The nodes the leader has benched (2.5 row 27). No answer: the last list stands.
    fn refresh_suspended(self: &Arc<Self>) {
        let body = if self.lead.lock().unwrap().is_some() { self.lead_answer("GET", "/api/nodes/suspended", b"").map(|r| r.body) } else { self.leader().and_then(|l| http::call(&l, "GET", "/api/nodes/suspended", Some(&self.secret), None, Duration::from_millis(500), Duration::from_secs(1))).filter(|(s, _)| *s == 200).map(|(_, b)| b) };
        if let Some(list) = body.and_then(|b| serde_json::from_slice::<Vec<Value>>(&b).ok()) { *self.suspended.lock().unwrap() = list.iter().filter_map(|x| Some((x["node_id"].as_str()?.to_string(), x["failure_count"].as_i64().unwrap_or(0)))).collect(); }
    }
    /// A job is put before or after another in the leader's order (2.5: `POST …/move`); across priorities it takes the other's.
    pub fn move_job(self: &Arc<Self>, id: String, target: String, before: bool) { let me = self.clone(); std::thread::spawn(move || { let _ = me.to_leader(&format!("/api/jobs/{id}/move"), &json!({ "target": target, "position": if before { "before" } else { "after" } }).to_string()); me.refresh_jobs_view(); }); }
    /// A job's priority, set from its detail (2.5: `POST …/priority`).
    pub fn set_priority(self: &Arc<Self>, id: String, priority: i64) { let me = self.clone(); std::thread::spawn(move || { let _ = me.to_leader(&format!("/api/jobs/{id}/priority"), &json!({ "priority": priority }).to_string()); me.refresh_jobs_view(); }); }
    /// The folder a job writes to, shown in the system's file manager: its `output_dir` through the path mappings. A
    /// folder that is not there on this machine is said, not opened.
    pub fn open_output(self: &Arc<Self>, dir: String) { let me = self.clone(); std::thread::spawn(move || {
        let here = PathBuf::from(min_paths::map::from_canonical_windows_form(&dir, &me.mappings));
        if dir.is_empty() { me.notes.lock().unwrap().push("This job names no output folder".into()); return }
        if !here.is_dir() { crate::share::log("WARN", "farm", &format!("open output: not a folder on this machine: {}", here.display())); me.notes.lock().unwrap().push(format!("The output folder is not there on this machine: {}", here.display())); return }
        let _ = std::process::Command::new(if cfg!(windows) { "explorer" } else { "open" }).arg(&here).spawn(); }); }
    /// What the tray and the window say of this node: (rendering which job, stopped).
    pub fn state(&self) -> (Option<String>, bool) { (self.work.lock().unwrap().running.as_ref().map(|r| r.job.clone()), self.stopped.load(Ordering::Relaxed)) }
    pub fn set_stopped(&self, on: bool) {
        let was = self.stopped.swap(on, Ordering::Relaxed); if was != on { log(if !on { "the node is started again" } else if self.work.lock().unwrap().running.is_some() { "the node is stopped: the render is killed" } else { "the node is stopped" }); } if on { self.cut(None, Cut::Fail); }
        let f = self.data.join("config.json"); if let Some(mut v) = std::fs::read_to_string(&f).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) { v["node_stopped"] = json!(on); let _ = write_atomically(&f, &serde_json::to_string_pretty(&v).unwrap()); }
    }
    /// The render under way is killed (4.10) - of `job` only, when one is named. There is no gentle step.
    fn cut(&self, job: Option<&str>, how: Cut) {
        let mut w = self.work.lock().unwrap();
        if let Some(r) = w.running.as_mut().filter(|r| job.is_none_or(|j| j == r.job)) { if r.cut.is_none() { r.cut = Some(how); } r.leash_in = None; #[cfg(unix)] if let Some(g) = r.group { crate::leash::kill_group(g); } }
    }

    // --------------------------------------------------------------------------------------------- peers and the leader
    /// One cycle (1): the phonebook entry, the other entries, a poll of every peer, the election.
    /// Two small duties on the shared folder, every ten seconds (row 429) - each so that no two machines write one file
    /// at the same moment, which storage that syncs answers with a "conflicted" second copy:
    /// - THE CONTENT (templates, the add-on, the stamp) is written by ONE node: among the nodes that answer, those of
    ///   the newest version; of them the leader, or - when the leader is an older build - the one with the smallest id.
    ///   (Before, every node newer than the stamp wrote it at its start: after an upgrade, several at once.)
    /// - THE SECRET: when a node that answers shows another secret than this one holds, the farm's file is read again;
    ///   if it holds another secret now (two nodes each made one at a first start; the storage kept one), this node
    ///   starts itself again on that one. (Before, it refused everyone until someone restarted it.)
    fn share_duties(&self) {
        if self.no_farm.is_some() { return } { let mut at = self.share_looked.lock().unwrap(); if at.elapsed() < Duration::from_secs(10) { return } *at = Instant::now(); }
        let mine = env!("CARGO_PKG_VERSION"); let my_print = fingerprint(&self.secret);
        let (others, other_secret): (Vec<(String, String, bool)>, bool) = { let p = self.peers.lock().unwrap(); (p.iter().filter(|(_, x)| x.alive).filter_map(|(id, x)| x.record.as_ref().map(|r| (id.clone(), r["app_version"].as_str().unwrap_or("").to_string(), x.leader))).collect(),
            p.values().any(|x| x.alive && x.record.as_ref().and_then(|r| r["secret_fingerprint"].as_str()).is_some_and(|f| !f.is_empty() && f != my_print))) };
        if other_secret { if let Some(now) = crate::share::secret_now(&self.farm) { if now != self.secret { crate::share::log("WARN", "farm", "the farm's file holds another secret than this node started with: the application starts again on it"); self.restart_asked.store(true, Ordering::Relaxed); return } } }
        if self.content_settled.load(Ordering::Relaxed) { return }
        if crate::share::content_stamp(&self.farm).is_some_and(|s| !crate::share::newer(mine, &s)) { self.content_settled.store(true, Ordering::Relaxed); return }
        if others.iter().any(|(_, v, _)| crate::share::newer(v, mine)) { return }   // a newer build is on the farm: the content is its to write
        let same: Vec<&(String, String, bool)> = others.iter().filter(|(_, v, _)| v == mine).collect();
        let i_write = if self.lead.lock().unwrap().is_some() { true } else if same.iter().any(|(_, _, leads)| *leads) { false } else { same.iter().all(|(id, _, _)| self.id < *id) };
        if i_write && crate::share::put_content_if_newer(&self.farm) { self.content_settled.store(true, Ordering::Relaxed); }
    }
    /// Whether the leader this node follows makes a job's log folder before it gives out the job's first chunk (1.0 and later do).
    fn leader_makes_folders(&self) -> bool { self.peers.lock().unwrap().values().find(|p| p.leader && p.alive).and_then(|p| p.record.as_ref()).and_then(|r| r["app_version"].as_str()).is_some_and(|v| v.split('.').next().and_then(|m| m.parse::<u32>().ok()).is_some_and(|m| m >= 1)) }
    fn cycle(&self) {
        self.share_duties();
        // a restart asked for by another node (1.3): the file is taken away; one more than 60 s old is ignored
        let sig = self.farm.join("nodes").join(&self.id).join("restart");
        if let Ok(m) = std::fs::metadata(&sig) { let fresh = m.modified().ok().and_then(|t| t.elapsed().ok()).is_none_or(|d| d.as_secs() <= 60); let _ = std::fs::remove_file(&sig);
            if fresh { crate::share::log("INFO", "farm", "a restart was asked for by another node"); self.restart_asked.store(true, Ordering::Relaxed); } }
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
                None => { peers.insert(id, Peer { endpoint, record: None, alive: true, failed: 0, last_seen_ms: 0, leader: false, stamp, written_ms: written, same: 0, wait_until: None, heard: None, rtt_ms: None, other_since: None }); } }
        } }
        // the poll (1.5): no token, a new connection, one peer after the other
        // (a peer that is heard by heartbeat, is known by name, and was polled less than 9 s ago is not polled now)
        let targets: Vec<(String, String, u64)> = self.peers.lock().unwrap().iter().filter(|(_, p)| !(p.udp() && p.record.as_ref().is_some_and(|r| !r["hostname"].as_str().unwrap_or("").is_empty()) && now_ms().saturating_sub(p.last_seen_ms) < 9000))
            .map(|(id, p)| (id.clone(), p.endpoint.clone(), p.rtt_ms.map(|r| (4.0 * r) as u64).unwrap_or(2000).clamp(2000, 8000))).collect();
        for (id, endpoint, wait) in targets {
            let t = Instant::now(); let answer = http::call(&endpoint, "GET", "/api/status", None, None, Duration::from_millis(wait), Duration::from_millis(wait + 1000)).filter(|(s, _)| *s == 200).and_then(|(_, b)| serde_json::from_slice::<Value>(&b).ok()).filter(peer_record_is_sound);
            let other = answer.as_ref().and_then(|r| r["node_id"].as_str()).is_some_and(|n| n != id);
            let got = answer
                // An answer from ANOTHER node is no answer from this one. (A machine whose data folder was emptied comes
                // back under a new id at the old address, and its old entry stays in the phonebook. 0.5.x takes the
                // newcomer's answer for the old node's: the old id looks alive, is given chunks, and every report for
                // them is refused because it comes from the new id. Seen 2026-11-14.)
                .filter(|r| r["node_id"].as_str().is_none_or(|n| n == id));
            let mut peers = self.peers.lock().unwrap(); let Some(p) = peers.get_mut(&id) else { continue };
            // An entry whose address has been another node's for a full minute is an old one left behind: it is taken
            // out of the phonebook (its folder and its logs stay). 0.5.x could not do this safely - it never knew who
            // had answered - and left it to a hand on the share.
            if other { let since = *p.other_since.get_or_insert_with(Instant::now); if since.elapsed() >= Duration::from_secs(60) { let f = self.farm.join("nodes").join(&id).join("endpoint.json");
                if std::fs::remove_file(&f).is_ok() { crate::share::log("WARN", "peer", &format!("the phonebook's entry of node {id} is an old one - its address has answered as another node for a minute - and was removed")); seen.retain(|s| *s != id); } } } else if answer_was_its_own(&got) { p.other_since = None; }
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
        if lead && l.is_none() { // (row 430) a snapshot that is not whole yet is waited for - twenty seconds from the first try, a turn at a time - before the one before it is taken
            let since = *self.takeover_since.lock().unwrap().get_or_insert_with(Instant::now); let give_up = since.elapsed() > Duration::from_secs(20);
            match Lead::take_over(&if self.no_farm.is_some() { self.data.join("no-farm") } else { self.data.clone() }, &self.farm, &self.id, give_up) {
                Ok(mut x) => { if let Some(n) = x.note.take() { self.notes.lock().unwrap().push(n); } *l = Some(x); *self.takeover_since.lock().unwrap() = None; log("this node leads now"); }
                Err(e) if e == crate::lead::WAIT => { if since.elapsed() < Duration::from_secs(3) { log("this node should lead; the snapshot on the share is not whole yet - waiting for the rest of it".to_string()); } }
                Err(e) => log(format!("this node should lead, and its database could not be opened: {e}")) } }
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
        // (row 429) the job's log folder is the LEADER's to make (row 424). On storage that syncs it may not have arrived here
        // yet: this node waits a little for it rather than making a second one - six seconds, then it makes it, as before.
        { let shared = self.farm.join("jobs").join(&job).join("stdout"); if !shared.is_dir() && self.lead.lock().unwrap().is_none() && self.leader_makes_folders() { let t = Instant::now(); while !shared.is_dir() && t.elapsed() < Duration::from_secs(6) && !self.stop.load(Ordering::Relaxed) { std::thread::sleep(Duration::from_millis(250)); } } }
        if let Some(d) = logf.parent() { let _ = std::fs::create_dir_all(d); }
        // (the output's folder is made before the render, as well as may be; a leader never makes it. With local
        // staging it is NOT made here: the render writes on this machine, and the folder is made when the files are
        // delivered - as 0.5.x does - so nodes starting chunks of one job at the same moment do not all make it at once)
        if staging.is_none() { if let Some(d) = m["output_dir"].as_str().filter(|d| !d.is_empty()) { let _ = std::fs::create_dir_all(d); } }
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
        // with no farm: the sync root is looked for every five seconds; there, the application starts itself again on it
        if self.no_farm.is_some() && !self.wanted_root.is_empty() { let me = self.clone(); std::thread::spawn(move || while !me.stop.load(Ordering::Relaxed) { for _ in 0..50 { if me.stop.load(Ordering::Relaxed) { return } std::thread::sleep(Duration::from_millis(100)); }
            if Path::new(&me.wanted_root).is_dir() { crate::share::log("INFO", "farm", "the sync root is there now: the application starts again on the farm"); me.restart_asked.store(true, Ordering::Relaxed); return } }); }
        // the graphics card's name: asked of the system once, off every other thread (macOS takes its time to say)
        { let me = self.clone(); std::thread::spawn(move || { let g = gpu_name(); *me.gpu.lock().unwrap() = g; }); }
        // the window's job list, and whom the leader has benched, every 2 s
        { let me = self.clone(); std::thread::spawn(move || while !me.stop.load(Ordering::Relaxed) { me.refresh_jobs_view(); me.refresh_suspended(); for _ in 0..20 { if me.stop.load(Ordering::Relaxed) { break } std::thread::sleep(Duration::from_millis(100)); } }); }
        // the window's job detail: at once when another job is asked for, then every 3 s
        { let me = self.clone(); std::thread::spawn(move || { let (mut last, mut at) = (None::<String>, Instant::now()); while !me.stop.load(Ordering::Relaxed) { let want = me.detail_of.lock().unwrap().clone();
            if want.is_some() && (want != last || at.elapsed() >= Duration::from_secs(3)) { me.refresh_detail(); at = Instant::now(); } last = want; std::thread::sleep(Duration::from_millis(100)); } }); }
        // the window's picture of a job's output: at once when another job or pin is asked for, then every 3 s
        { let me = self.clone(); std::thread::spawn(move || { let (mut asked, mut at, mut last) = (None, Instant::now(), None); while !me.stop.load(Ordering::Relaxed) { let want = me.preview_of.lock().unwrap().clone();
            if want.is_some() && (want != asked || at.elapsed() >= Duration::from_secs(3)) { me.refresh_preview(&mut last); at = Instant::now(); } asked = want; std::thread::sleep(Duration::from_millis(100)); } }); }
        // the Logs tab's reading: at once when something else is asked for, then every 3 s
        { let me = self.clone(); std::thread::spawn(move || { let (mut asked, mut at) = (None, Instant::now()); while !me.stop.load(Ordering::Relaxed) { let want = me.log_of.lock().unwrap().clone();
            if want.is_some() && (want != asked || at.elapsed() >= Duration::from_secs(3)) { me.refresh_log(); at = Instant::now(); } asked = want; std::thread::sleep(Duration::from_millis(100)); } }); }
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
fn answer_was_its_own(got: &Option<Value>) -> bool { got.is_some() }
/// The id this machine had before it lost its id file, if the phonebook still knows it: the entry at this address and
/// port (and of this host name, where the entry gives one) - the latest written, if there are several.
fn former_self(farm: &Path, ip: &str, port: u16, hostname: &str) -> Option<String> {
    let mut best: Option<(i64, String)> = None;
    for d in std::fs::read_dir(farm.join("nodes")).ok()?.flatten() { let id = d.file_name().to_string_lossy().into_owned(); if id.len() != 12 { continue }
        let Some(v) = std::fs::read_to_string(d.path().join("endpoint.json")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) else { continue };
        if v["node_id"].as_str() != Some(&id) || v["ip"].as_str() != Some(ip) || v["port"].as_u64() != Some(port as u64) || v["hostname"].as_str().is_some_and(|h| h != hostname) { continue }
        let at = v["timestamp_ms"].as_i64().unwrap_or(0); if best.as_ref().is_none_or(|(b, _)| at > *b) { best = Some((at, id)); } }
    best.map(|(_, id)| id)
}
/// A decoded picture as the preview shows it (the spec's 3.4, carried): opaque, the colours as stored; half-float
/// through the sRGB curve and cut at 1.0. No exposure, no tone mapping.
fn opaque_rgba8(img: &min_image::Image) -> (Vec<u8>, u32, u32) {
    let (w, h, p) = (img.width as usize, img.height as usize, img.pitch as usize); let mut out = vec![255u8; w * h * 4];
    let curve = |v: f32| { let v = if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }; ((if v <= 0.003_130_8 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }) * 255.0 + 0.5) as u8 };
    for y in 0..h { let (g, b, r) = (&img.planes[0][y * p..], &img.planes[1][y * p..], &img.planes[2][y * p..]); let row = &mut out[y * w * 4..(y + 1) * w * 4];
        match img.sample { min_image::Sample::U8 => for x in 0..w { row[x * 4] = r[x]; row[x * 4 + 1] = g[x]; row[x * 4 + 2] = b[x]; },
            min_image::Sample::U16 => for x in 0..w { row[x * 4] = r[x * 2 + 1]; row[x * 4 + 1] = g[x * 2 + 1]; row[x * 4 + 2] = b[x * 2 + 1]; },
            _ => for x in 0..w { let f = |pl: &[u8]| curve(half::f16::from_bits(u16::from_le_bytes([pl[x * 2], pl[x * 2 + 1]])).to_f32()); row[x * 4] = f(r); row[x * 4 + 1] = f(g); row[x * 4 + 2] = f(b); } } }
    (out, w as u32, h as u32)
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

/// The graphics card's name as 0.5.x reports it: the first DXGI adapter on Windows, the "Chipset Model" on macOS.
#[cfg(target_os = "macos")]
fn gpu_name() -> String { std::process::Command::new("/usr/sbin/system_profiler").arg("SPDisplaysDataType").output().ok().and_then(|o| String::from_utf8_lossy(&o.stdout).lines().find_map(|l| l.trim().strip_prefix("Chipset Model:").map(|v| v.trim().to_string()))).unwrap_or_default() }
#[cfg(windows)]
fn gpu_name() -> String { use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1};
    unsafe { let Ok(f) = CreateDXGIFactory1::<IDXGIFactory1>() else { return String::new() }; let Ok(a) = f.EnumAdapters1(0) else { return String::new() }; let Ok(d) = a.GetDesc1() else { return String::new() };
        String::from_utf16_lossy(&d.Description).trim_end_matches('\0').to_string() } }
#[cfg(not(any(windows, target_os = "macos")))]
fn gpu_name() -> String { String::new() }
