//! The mesh (canon rows 440-442, 459, 494; UFB 1.4.2's `core/src/mesh`, read into `docs/phase5/step0-mesh-sync.md`):
//! the machines that share a farm folder keep each other's metadata. This is the NODE, as a 1.4.2 node speaks:
//!
//! - **found** through the phonebook - `<farm>/v<epoch>/nodes/<node id>/endpoint.json`, rewritten every pass -, through
//!   a heartbeat on the local network's multicast group, and kept alive by `GET /api/status`;
//! - **one leads**: a plain sort of who is alive (the tag `leader` first, `noleader` last, then the node's id) - no
//!   votes, as 1.4.2 has none;
//! - **a change is a whole row over plain HTTP**: a follower posts it to the leader, the leader takes it and passes it
//!   on to every other node (`POST /api/metadata/update`, `POST /api/table/update`).
//!
//! What is mended already (row 441): **the database is the queue**. A row changed here waits in `mesh_pending` until it
//! was accepted - by the leader, or, when this node leads, by every node that is alive. Nothing is dropped after a
//! number of tries, a quit loses nothing, and what waited is sent when a leader is found again.
//!
//! **The snapshot** (rows 442, 495) is the mesh on the shared folder alone: the node that leads writes the shared rows
//! as a SQLite file - 1.4.2's file, table for table - and every node merges it. A node that was away, or is new,
//! catches up from it; and machines that reach the folder but NOT each other (a VPN that passes the share and nothing
//! else, a firewall) each lead alone, each merge what is there before they write, and so hold the same in the end
//! with no message between them. Mended against 1.4.2 (row 442): a temporary name of the writer's own; the file
//! checked before it is merged, and waited for when it is not whole; the others told AFTER it was written; a leader
//! merges too - what is there before it publishes its own, and what another leader wrote; and a node counts as alive
//! when it ANSWERS, not when its entry in the phonebook looks fresh - so one that cannot be reached is never the leader
//! this node waits for.
//!
//! **Merged by cell** (row 440; `db::merge_row`): a row that arrives - in a message or in a snapshot - does not replace
//! ours: what its sender changed is taken, what this machine changed is kept, and when the two are not the same in the
//! end ours is sent on.
//!
//! **The shared secret** (rows 442, 497; as minRender has it - the family's systems are not to diverge): a secret in
//! the farm's folder, made by the first node that finds none, sent with every request but the status (`Authorization:
//! Bearer`), its fingerprint shown in every node's status. A request with another secret is refused; one with none is
//! a 1.4.2 node's, and is taken only while a 1.4.2 node is among those that answer. Two nodes that each made a secret
//! at a first start settle on the one the folder kept.
//!
//! NOT here yet: the columns' shared templates.
//!
//! THE DEVELOPMENT BUILD'S BELT: the epoch is a TEST epoch unless `UFB_MESH_EPOCH` says otherwise - a node of another
//! epoch is ignored entirely, by 1.4.2's own rule, so no installed UFB can see or be seen by a test node; and
//! `--mesh-only <folder>` keeps everything but that folder's rows out of the mesh, both ways.
use crate::http::{self, Request, Response};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// 1.4.2's numbers.
#[allow(dead_code)]
pub const EPOCH: u32 = 6;
// (1.4.2's ports: what the installed application will take - a development build never does, see `TEST_PORTS`)
#[allow(dead_code)] pub const CTL_PORT: u16 = 49221; #[allow(dead_code)] pub const DATA_PORT: u16 = 49222; #[allow(dead_code)] pub const UDP_PORT: u16 = 4265;
const GROUP: Ipv4Addr = Ipv4Addr::new(239, 42, 6, 3); const PASS: Duration = Duration::from_millis(3000); const DEAD_AFTER: u8 = 3;
/// The snapshot (1.4.2's numbers): not oftener than every 30 s; written again every 10 min while there is something; another node's, younger than 60 s, is not written over; looked for every 10 s.
const SNAP_EVERY: Duration = Duration::from_secs(30); const SNAP_REARM: Duration = Duration::from_secs(600); const SNAP_FENCE_MS: i64 = 60_000; const SNAP_LOOK: Duration = Duration::from_secs(10);
/// The epoch a development build's mesh runs at: no installed UFB is of it.
pub const TEST_EPOCH: u32 = 9006;

#[derive(Clone, Debug)]
pub struct Config { pub farm: PathBuf, pub node: String, pub epoch: u32, pub ctl: u16, pub data: u16, pub udp: u16, pub tags: Vec<String>, /// only rows under this identity are sent and taken (a test's playground)
    pub only: Option<String> }
#[derive(Clone, Debug, PartialEq)]
struct Peer { ip: String, port: u16, data_port: u16, tags: Vec<String>, alive: bool, fails: u8, stamp: i64, /// its secret's fingerprint, as its status says it (none: a 1.4.2 node, which has no secret)
    print: Option<String> }
/// What the window says of the mesh.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status { pub node: String, pub leader: Option<String>, pub leading: bool, pub peers: usize, pub pending: usize, /// when a snapshot was last written here, and last merged here (ms; 0: never)
    pub written: i64, pub merged: i64, /// the secret's fingerprint as this node holds it; how many nodes that answer hold another
    pub secret: String, pub strangers: usize }

struct Shared { cfg: Config, peers: Mutex<HashMap<String, Peer>>, leader: Mutex<Option<String>>, pending: AtomicUsize, rev: AtomicU64, stop: Arc<AtomicBool>, wakers: Mutex<Vec<Arc<dyn Fn() + Send + Sync>>>, ip: Mutex<String>,
    /// the snapshot: something changed since the last one was written; a look at the file is wanted now; when one was written and merged here; what this node already passed on to the others (it leaves the queue with the next snapshot)
    snap_needed: AtomicBool, snap_look: AtomicBool, written: AtomicU64, merged: AtomicU64, relayed: Mutex<std::collections::HashSet<i64>>,
    /// a test's switch (`UFB_MESH_FILES_ONLY=1`): this node neither answers nor asks - as over a VPN that passes the shared folder and nothing else
    files_only: bool,
    /// the farm's secret as this node holds it (empty: the folder gave none)
    secret: Mutex<String> }
static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();
fn now_ms() -> i64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0) }
fn say(s: &Shared, what: &str) { if std::env::var_os("UFB_MESH_LOG").is_some() { eprintln!("UFB_MESH[{}]: {what}", s.cfg.node); } }

/// The mesh's state, for the window; none when the mesh is not running.
pub fn status() -> Option<Status> { let s = SHARED.get()?; let leader = s.leader.lock().unwrap().clone();
    // (each lock taken and let go in a statement of its own: two of one lock in one expression wait for each other for good)
    let mine = fingerprint(&s.secret.lock().unwrap()); let strangers = s.peers.lock().unwrap().values().filter(|p| p.alive && p.print.as_ref().is_some_and(|f| *f != mine)).count(); let peers = s.peers.lock().unwrap().values().filter(|p| p.alive).count();
    Some(Status { node: s.cfg.node.clone(), leading: leader.as_deref() == Some(s.cfg.node.as_str()), leader, peers, pending: s.pending.load(Ordering::Relaxed), written: s.written.load(Ordering::Relaxed) as i64, merged: s.merged.load(Ordering::Relaxed) as i64, secret: mine, strangers }) }
/// A number that changes whenever something arrived from another node (the windows read their lists again).
pub fn rev() -> u64 { SHARED.get().map(|s| s.rev.load(Ordering::Relaxed)).unwrap_or(0) }
/// A window asks to be woken when something arrives.
pub fn on_change(wake: Arc<dyn Fn() + Send + Sync>) { if let Some(s) = SHARED.get() { s.wakers.lock().unwrap().push(wake); } }
fn changed(s: &Shared) { s.rev.fetch_add(1, Ordering::Relaxed); for w in s.wakers.lock().unwrap().iter() { w(); } }

/// Who leads, of these: (the node's id, its tags). 1.4.2's sort - `leader` first, `noleader` last, then the id.
pub fn elect(mut nodes: Vec<(String, Vec<String>)>) -> Option<String> {
    let rank = |t: &[String]| if t.iter().any(|x| x == "leader") { 0 } else if t.iter().any(|x| x == "noleader") { 2 } else { 1 };
    nodes.sort_by(|a, b| rank(&a.1).cmp(&rank(&b.1)).then(a.0.cmp(&b.0))); nodes.into_iter().next().map(|n| n.0)
}
/// A message's path is inside what this node shares (the test's playground, when one is set).
fn in_scope(cfg: &Config, identity: &str) -> bool { identity.starts_with("vol:") && cfg.only.as_ref().is_none_or(|o| identity == o || identity.starts_with(&format!("{o}/"))) }
fn scope_of(v: &Value) -> String { v.get("job_path").and_then(|x| x.as_str()).map(str::to_string).or_else(|| v.get("def").and_then(|d| d.as_str()).and_then(|d| serde_json::from_str::<Value>(d).ok()).and_then(|d| d.get("jobPath").and_then(|x| x.as_str()).map(str::to_string))).unwrap_or_default() }

/// This machine's address on the network the others are on (the route to a public address; nothing is sent).
fn local_ip() -> String { UdpSocket::bind("0.0.0.0:0").and_then(|s| { s.connect("192.0.2.1:9")?; s.local_addr() }).map(|a| a.ip().to_string()).unwrap_or_else(|_| "127.0.0.1".into()) }

/// Start the node. Twice: nothing the second time.
/// The ports a development build's node takes when nothing says otherwise: never 1.4.2's (an installed UFB on this
/// machine holds those).
pub const TEST_PORTS: (u16, u16, u16) = (49321, 49322, 4365);
pub fn running() -> bool { SHARED.get().is_some() }
/// This machine's name as a node's id: `given`, else the system's; only letters, digits, `-` and `_`.
pub fn node_name(given: &str) -> String {
    let name = if given.trim().is_empty() { std::env::var("COMPUTERNAME").ok().or_else(|| std::process::Command::new("hostname").output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())).unwrap_or_else(|| "node".into()) } else { given.trim().to_string() };
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect()
}
/// The node is started from what the start-up arguments or the settings say (row 499). A development build runs at
/// the TEST epoch and on the TEST ports unless `UFB_MESH_EPOCH` / `UFB_MESH_PORTS=c,d,u` say otherwise. `leader`:
/// "" | "leader" | "noleader"; `only`: a folder - nothing but its rows takes part.
pub fn start_with(farm: &std::path::Path, node: &str, leader: &str, only: Option<&std::path::Path>) {
    let ports: Vec<u16> = std::env::var("UFB_MESH_PORTS").ok().map(|p| p.split(',').filter_map(|x| x.trim().parse().ok()).collect()).unwrap_or_default();
    let mut tags: Vec<String> = std::env::var("UFB_MESH_TAGS").ok().map(|t| t.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()).unwrap_or_default();
    if !leader.is_empty() && !tags.iter().any(|t| t == leader) { tags.push(leader.to_string()); }
    let abs = |p: &std::path::Path| std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    start(Config { farm: abs(farm), node: node_name(node), epoch: std::env::var("UFB_MESH_EPOCH").ok().and_then(|e| e.parse().ok()).unwrap_or(TEST_EPOCH),
        ctl: ports.first().copied().unwrap_or(TEST_PORTS.0), data: ports.get(1).copied().unwrap_or(TEST_PORTS.1), udp: ports.get(2).copied().unwrap_or(TEST_PORTS.2), tags, only: only.map(|p| crate::db::to_identity(&abs(p))) });
}
pub fn start(cfg: Config) {
    let s = Arc::new(Shared { cfg, peers: Default::default(), leader: Default::default(), pending: AtomicUsize::new(0), rev: AtomicU64::new(0), stop: Arc::new(AtomicBool::new(false)), wakers: Default::default(), ip: Mutex::new(local_ip()), snap_needed: AtomicBool::new(true), snap_look: AtomicBool::new(true), written: AtomicU64::new(0), merged: AtomicU64::new(0), relayed: Default::default(), files_only: std::env::var_os("UFB_MESH_FILES_ONLY").is_some(), secret: Mutex::new(String::new()) });
    if SHARED.set(s.clone()).is_err() { return }
    // the farm's secret: the folder's, or one made now (a test can hold another: `UFB_MESH_SECRET`)
    *s.secret.lock().unwrap() = std::env::var("UFB_MESH_SECRET").ok().unwrap_or_else(|| farm_secret(&s, true).unwrap_or_default());
    say(&s, &format!("epoch {} farm {} ports {}/{}/{} only {:?}", s.cfg.epoch, s.cfg.farm.display(), s.cfg.ctl, s.cfg.data, s.cfg.udp, s.cfg.only));
    // the two servers: the status on the control port, everything else on the data port
    for (port, data) in [(s.cfg.ctl, false), (s.cfg.data, true)] { if s.files_only { break }
        let sock = socket2::Socket::new(socket2::Domain::IPV4, socket2::Type::STREAM, None).and_then(|k| { k.set_reuse_address(true)?; k.bind(&SocketAddr::from(([0, 0, 0, 0], port)).into())?; k.listen(1024)?; Ok(k) });
        match sock { Ok(k) => { let (st, stop) = (s.clone(), s.stop.clone()); let _ = std::thread::Builder::new().name("ufb-mesh-http".into()).spawn(move || http::serve(k.into(), stop, move |q| if data { serve_data(&st, q) } else { serve_ctl(&st, q) })); }
            Err(e) => eprintln!("UFB: the mesh's port {port} could not be opened: {e}") }
    }
    { let st = s.clone(); let _ = std::thread::Builder::new().name("ufb-mesh-peers".into()).spawn(move || peers_loop(&st)); }
    { let st = s.clone(); let _ = std::thread::Builder::new().name("ufb-mesh-send".into()).spawn(move || send_loop(&st)); }
    if !s.files_only { let st = s.clone(); let _ = std::thread::Builder::new().name("ufb-mesh-udp".into()).spawn(move || udp_listen(&st)); }
    { let st = s.clone(); let _ = std::thread::Builder::new().name("ufb-mesh-snapshot".into()).spawn(move || snapshot_loop(&st)); }
}

fn serve_ctl(s: &Shared, q: Request) -> Response {
    if q.method != "GET" || q.path.split('?').next() != Some("/api/status") { return Response::empty(404) }
    let st = status().unwrap_or_default();
    // (1.4.2's fields, and one more that it does not read: the secret's fingerprint)
    Response::json(200, json!({ "node_id": s.cfg.node, "is_leader": st.leading, "peer_count": st.peers, "last_snapshot_time": st.written, "enabled": true, "tags": s.cfg.tags, "mesh_epoch": s.cfg.epoch, "secret_fingerprint": st.secret }))
}
fn serve_data(s: &Arc<Shared>, q: Request) -> Response {
    let path = q.path.split('?').next().unwrap_or("").to_string(); if q.method != "POST" { return Response::empty(if path.starts_with("/api/") { 405 } else { 404 }) }
    // the secret: every request but the status carries it
    { let old = s.peers.lock().unwrap().values().any(|p| p.alive && p.print.is_none()); if !allowed(&s.secret.lock().unwrap(), q.authorization.as_deref(), old) { say(s, &format!("refused {path}: {}", if q.authorization.is_some() { "another secret" } else { "no secret" })); return Response::error(401, "unauthorized") } }
    // a snapshot was written: it is looked at now (1.4.2's leader ignores the notice; ours merges too - row 442)
    if path == "/api/snapshot/notify" { s.snap_look.store(true, Ordering::Relaxed); return Response::ok() }
    if path != "/api/metadata/update" && path != "/api/table/update" { return Response::empty(404) }
    let Ok(v) = serde_json::from_slice::<Value>(&q.body) else { return Response::empty(400) };
    // another epoch's node is not of this mesh
    if v.get("mesh_epoch").and_then(|x| x.as_u64()).unwrap_or(0) != s.cfg.epoch as u64 { return Response::json(409, json!({ "error": "epoch" })) }
    let meta = path == "/api/metadata/update";
    if !in_scope(&s.cfg, &scope_of(&v)) { say(s, &format!("left out (not in what this node shares): {}", scope_of(&v))); return Response::ok() }
    let r = crate::db::with(|d| if meta { d.apply_meta(&v) } else { d.apply_table(&v) });
    match r {
        Some(Ok(took)) => { say(s, &format!("in {path}: {}", if took { "taken" } else { "not newer" })); if took { changed(s); s.snap_needed.store(true, Ordering::Relaxed); }
            // the leader passes it on to every other node (1.4.2: to all, the one it came from among them)
            if status().is_some_and(|t| t.leading) { let (st, body) = (s.clone(), String::from_utf8_lossy(&q.body).into_owned()); let _ = std::thread::Builder::new().name("ufb-mesh-relay".into()).spawn(move || { relay(&st, &path, &body); }); }
            Response::ok() }
        Some(Err(e)) => Response::error(500, &e),
        None => Response::error(500, "the database could not be opened"),
    }
}
/// A message to every node that is alive: three tries each, half a second and a second apart (1.4.2). True: all took it.
fn relay(s: &Shared, path: &str, body: &str) -> bool {
    let peers: Vec<(String, String)> = s.peers.lock().unwrap().iter().filter(|(_, p)| p.alive).map(|(id, p)| (id.clone(), format!("{}:{}", p.ip, p.data_port))).collect(); let mut all = true;
    for (id, at) in peers { let mut ok = false;
        for wait in [0u64, 500, 1000] { if wait > 0 { std::thread::sleep(Duration::from_millis(wait)); } if post(&at, path, body) { ok = true; break } }
        if !ok { say(s, &format!("{id} did not take {path}")); all = false; } }
    all
}
fn post(at: &str, path: &str, body: &str) -> bool { let secret = SHARED.get().map(|s| s.secret.lock().unwrap().clone()).filter(|x| !x.is_empty()); http::call(at, "POST", path, secret.as_deref(), Some(body), Duration::from_secs(3), Duration::from_secs(5)).is_some_and(|(code, _)| (200..300).contains(&code)) }
/// The first four bytes of the secret's SHA-256, in hex (minRender's fingerprint): what a node shows of its secret.
pub fn fingerprint(secret: &str) -> String { use sha2::Digest; if secret.is_empty() { return String::new() } sha2::Sha256::digest(secret.as_bytes()).iter().take(4).map(|b| format!("{b:02x}")).collect() }
/// A request is taken: it carries this node's secret; or it carries none - a 1.4.2 node's - while a 1.4.2 node is
/// among those that answer (`old`); or this node has no secret itself (the folder gave none: as 1.4.2, open).
pub fn allowed(secret: &str, authorization: Option<&str>, old: bool) -> bool { if secret.is_empty() { return true } match authorization { Some(a) => a.strip_prefix("Bearer ").is_some_and(|x| x == secret), None => old } }
fn secret_file(s: &Shared) -> PathBuf { s.cfg.farm.join(format!("v{}", s.cfg.epoch)).join("secret.json") }
/// The farm's secret as its folder has it; `make`: one is made when there is none (under a temporary name of this
/// node's own, then renamed - and read back: on storage that syncs another node's may be the one that stays).
fn farm_secret(s: &Shared, make: bool) -> Option<String> {
    let read = || -> Option<String> { let v: Value = serde_json::from_str(&std::fs::read_to_string(secret_file(s)).ok()?).ok()?; v["api_secret"].as_str().filter(|x| !x.is_empty()).map(str::to_string) };
    if let Some(x) = read() { return Some(x) } if !make { return None }
    let file = secret_file(s); std::fs::create_dir_all(file.parent()?).ok()?;
    let secret = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    let tmp = file.with_file_name(format!("secret.json.{}.tmp", s.cfg.node)); std::fs::write(&tmp, serde_json::to_vec_pretty(&json!({ "_version": 1, "created_by": s.cfg.node, "created_at_ms": now_ms(), "api_secret": secret })).ok()?).ok()?;
    // (a file that came meanwhile is not replaced)
    if file.exists() { let _ = std::fs::remove_file(&tmp); } else if std::fs::rename(&tmp, &file).is_err() { let _ = std::fs::remove_file(&tmp); }
    read()
}

/// What waits in the database is sent: by a follower to the leader, by the leader to every node that is alive. What
/// was accepted leaves the queue; what was not stays, and is tried again at the next pass - for as long as it takes.
fn send_loop(s: &Shared) {
    while !s.stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(400));
        let pending = crate::db::with(|d| d.pending()).unwrap_or_default(); s.pending.store(crate::db::with(|d| d.pending_count()).unwrap_or(0), Ordering::Relaxed);
        if pending.is_empty() { continue }
        let (leader, me) = (s.leader.lock().unwrap().clone(), s.cfg.node.clone()); let Some(leader) = leader else { continue };
        let target = if leader == me { None } else { s.peers.lock().unwrap().get(&leader).filter(|p| p.alive).map(|p| format!("{}:{}", p.ip, p.data_port)) };
        if leader != me && target.is_none() { continue }
        for (id, kind, key, payload) in pending {
            let msg: Option<Value> = match (&payload, kind.as_str()) { (Some(p), _) => serde_json::from_str(p).ok(), (None, "meta") => crate::db::with(|d| d.meta_wire(&key)).flatten(), (None, _) => crate::db::with(|d| d.col_wire(&key)).flatten() };
            // (the row is gone, or it is not of what this node shares: nothing to send)
            let Some(mut msg) = msg.filter(|m| in_scope(&s.cfg, &scope_of(m))) else { crate::db::with(|d| d.pending_done(id)); continue };
            // this node leads: what it changed is passed on to those that answer - once - and is in the next snapshot,
            // which is what everybody else gets it from; it leaves the queue when that snapshot is written
            if target.is_none() { if s.relayed.lock().unwrap().contains(&id) { continue } s.snap_needed.store(true, Ordering::Relaxed); }
            msg["mesh_epoch"] = s.cfg.epoch.into(); let (path, body) = (if kind == "meta" { "/api/metadata/update" } else { "/api/table/update" }, msg.to_string());
            // (taken by somebody: by the leader - or, when this node leads, by at least one node that answers; passed on to nobody, it is in common with nobody)
            let reached = target.is_some() || s.peers.lock().unwrap().values().any(|p| p.alive);
            let ok = match &target { Some(at) => post(at, path, &body), None => relay(s, path, &body) };
            say(s, &format!("out {path} {key}: {}", if ok { "accepted" } else { "not yet" }));
            // (what was taken is the version this machine has in common with the others from now on - row 440)
            if ok && reached && kind == "meta" { crate::db::with(|d| d.base_from_wire(&msg)); }
            if target.is_none() { s.relayed.lock().unwrap().insert(id); continue }
            if ok { crate::db::with(|d| d.pending_done(id)); } else { break }
        }
        s.pending.store(crate::db::with(|d| d.pending_count()).unwrap_or(0), Ordering::Relaxed); changed(s);
    }
}

fn phonebook(s: &Shared) -> PathBuf { s.cfg.farm.join(format!("v{}", s.cfg.epoch)).join("nodes") }
/// One pass: this node's entry in the phonebook, the others' entries, a heartbeat, a look at each of them, who leads.
fn peers_loop(s: &Shared) {
    let udp = UdpSocket::bind("0.0.0.0:0").ok(); if let Some(u) = &udp { let _ = u.set_multicast_ttl_v4(1); }
    while !s.stop.load(Ordering::Relaxed) {
        let ip = local_ip(); *s.ip.lock().unwrap() = ip.clone(); let now = now_ms();
        // this node's entry, under a temporary name of its OWN (row 442: 1.4.2 has one fixed name for everybody)
        let dir = phonebook(s).join(&s.cfg.node); let entry = json!({ "node_id": s.cfg.node, "ip": ip, "port": s.cfg.ctl, "data_port": s.cfg.data, "mesh_epoch": s.cfg.epoch, "timestamp_ms": now, "tags": s.cfg.tags });
        if std::fs::create_dir_all(&dir).is_ok() { let tmp = dir.join(format!("endpoint.json.{}.tmp", s.cfg.node)); if std::fs::write(&tmp, serde_json::to_vec_pretty(&entry).unwrap_or_default()).is_ok() && std::fs::rename(&tmp, dir.join("endpoint.json")).is_err() { let _ = std::fs::remove_file(&tmp); } }
        // the others' entries
        if let Ok(rd) = std::fs::read_dir(phonebook(s)) { for e in rd.flatten() {
            let Ok(text) = std::fs::read_to_string(e.path().join("endpoint.json")) else { continue }; let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
            let id = v["node_id"].as_str().unwrap_or("").to_string(); let (port, data) = (v["port"].as_u64().unwrap_or(0) as u16, v["data_port"].as_u64().unwrap_or(0) as u16);
            if id.is_empty() || id == s.cfg.node || data == 0 || v["mesh_epoch"].as_u64() != Some(s.cfg.epoch as u64) { continue }
            let stamp = v["timestamp_ms"].as_i64().unwrap_or(0); let tags: Vec<String> = v["tags"].as_array().map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect()).unwrap_or_default();
            let mut peers = s.peers.lock().unwrap(); let ipv = v["ip"].as_str().unwrap_or("").to_string();
            match peers.get_mut(&id) { Some(p) => { p.stamp = stamp; if !p.alive { p.ip = ipv; p.port = port; p.data_port = data; } }
                // (it is alive when it answers - row 442; 1.4.2 takes a fresh entry's word for it)
                None => { say(s, &format!("found {id} at {ipv}:{port}")); peers.insert(id, Peer { ip: ipv, port, data_port: data, tags, alive: false, fails: 0, stamp, print: None }); } }
        } }
        // a heartbeat to the local network
        if let Some(u) = udp.as_ref().filter(|_| !s.files_only) { let hb = json!({ "t": "hb", "from": s.cfg.node, "n": s.cfg.node, "ip": ip, "port": s.cfg.ctl, "data_port": s.cfg.data, "epoch": s.cfg.epoch, "tags": s.cfg.tags }).to_string(); let _ = u.send_to(hb.as_bytes(), SocketAddr::from((GROUP, s.cfg.udp))); }
        // a look at each: alive when it answers, of this epoch (1.4.2: dead after three looks without an answer)
        let known: Vec<(String, String)> = s.peers.lock().unwrap().iter().map(|(id, p)| (id.clone(), format!("{}:{}", p.ip, p.port))).collect();
        for (id, at) in known { if s.files_only { break }
            let got = http::call(&at, "GET", "/api/status", None, None, Duration::from_secs(3), Duration::from_secs(3)).filter(|(c, _)| *c == 200).and_then(|(_, b)| serde_json::from_slice::<Value>(&b).ok());
            let mut peers = s.peers.lock().unwrap(); let Some(p) = peers.get_mut(&id) else { continue };
            match got { Some(v) if v["mesh_epoch"].as_u64() == Some(s.cfg.epoch as u64) => { if !p.alive { say(s, &format!("{id} is alive")); } p.alive = true; p.fails = 0; p.print = v["secret_fingerprint"].as_str().map(str::to_string); p.tags = v["tags"].as_array().map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect()).unwrap_or_default(); }
                Some(_) => { peers.remove(&id); }
                None => if p.alive { p.fails += 1; if p.fails >= DEAD_AFTER { p.alive = false; say(s, &format!("{id} does not answer")); } } }
        }
        // a node that answers holds ANOTHER secret: the folder's file is read again - two nodes each made one at a first
        // start and the storage kept one; this node takes the folder's (minRender's way, row 429)
        if std::env::var_os("UFB_MESH_SECRET").is_none() { let mine = fingerprint(&s.secret.lock().unwrap()); if s.peers.lock().unwrap().values().any(|p| p.alive && p.print.as_ref().is_some_and(|f| !f.is_empty() && *f != mine)) { if let Some(now) = farm_secret(s, false) { if now != *s.secret.lock().unwrap() { say(s, "the folder holds another secret than this node started with: taken"); *s.secret.lock().unwrap() = now; changed(s); } } } }
        // who leads: this node and every node that is alive (or whose entry is fresh)
        let mut nodes = vec![(s.cfg.node.clone(), s.cfg.tags.clone())]; nodes.extend(s.peers.lock().unwrap().iter().filter(|(_, p)| p.alive).map(|(id, p)| (id.clone(), p.tags.clone())));
        let leader = elect(nodes); let was = s.leader.lock().unwrap().clone(); if leader != was { say(s, &format!("the leader is {leader:?}")); let mine = leader.as_deref() == Some(s.cfg.node.as_str()); *s.leader.lock().unwrap() = leader; if mine { s.snap_needed.store(true, Ordering::Relaxed); } s.snap_look.store(true, Ordering::Relaxed); changed(s); }
        std::thread::sleep(PASS);
    }
}
/// The heartbeats of the others: a node heard is alive, at the address it says (1.4.2).
fn udp_listen(s: &Shared) {
    let sock = socket2::Socket::new(socket2::Domain::IPV4, socket2::Type::DGRAM, None).and_then(|k| { k.set_reuse_address(true)?; #[cfg(unix)] k.set_reuse_port(true)?; k.bind(&SocketAddr::from(([0, 0, 0, 0], s.cfg.udp)).into())?; k.join_multicast_v4(&GROUP, &Ipv4Addr::UNSPECIFIED)?; Ok(UdpSocket::from(k)) });
    let Ok(sock) = sock else { say(s, "no multicast on this machine: the phonebook alone finds the others"); return }; let _ = sock.set_read_timeout(Some(Duration::from_millis(500))); let mut buf = [0u8; 2048];
    while !s.stop.load(Ordering::Relaxed) {
        let Ok((n, _)) = sock.recv_from(&mut buf) else { continue }; let Ok(v) = serde_json::from_slice::<Value>(&buf[..n]) else { continue };
        let id = v["n"].as_str().or(v["from"].as_str()).unwrap_or("").to_string(); if id.is_empty() || v["from"].as_str() == Some(s.cfg.node.as_str()) { continue }
        match v["t"].as_str() {
            Some("hb") => { let data = v["data_port"].as_u64().unwrap_or(0) as u16; if data == 0 || v["epoch"].as_u64() != Some(s.cfg.epoch as u64) { continue }
                // (a node heard is KNOWN, at the address it says; whether it is alive is what its answer says - a heartbeat
                // passes where a firewall stops everything else)
                let mut peers = s.peers.lock().unwrap(); let (ip, port) = (v["ip"].as_str().unwrap_or("").to_string(), v["port"].as_u64().unwrap_or(0) as u16);
                match peers.get_mut(&id) { Some(p) => if !p.alive { p.ip = ip; p.port = port; p.data_port = data; }, None => { say(s, &format!("heard {id} at {ip}:{port}")); peers.insert(id, Peer { ip, port, data_port: data, tags: vec![], alive: false, fails: 0, stamp: now_ms(), print: None }); } } }
            Some("bye") => { if let Some(p) = s.peers.lock().unwrap().get_mut(&id) { p.alive = false; } }
            _ => {}
        }
    }
}

fn snap_file(s: &Shared) -> PathBuf { s.cfg.farm.join(format!("v{}", s.cfg.epoch)).join("snapshots").join(format!("ufb_snapshot_v{}.db", s.cfg.epoch)) }
fn snap_meta(s: &Shared) -> PathBuf { snap_file(s).with_file_name(format!("ufb_snapshot_v{}.meta.json", s.cfg.epoch)) }
fn mtime_ms(p: &std::path::Path) -> Option<u64> { std::fs::metadata(p).ok()?.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_millis() as u64) }
/// Who wrote the snapshot that is there, and when (its sidecar; 1.4.2's: `{"node_id", "written_at"}`).
fn snap_author(s: &Shared) -> Option<(String, i64)> { let v: Value = serde_json::from_str(&std::fs::read_to_string(snap_meta(s)).ok()?).ok()?; Some((v["node_id"].as_str()?.to_string(), v["written_at"].as_i64().unwrap_or(0))) }
/// The snapshot that is there is merged into this database, when it is another one than the last this node merged
/// or wrote. It is copied to this machine first and checked: a file that is not whole (still on its way) is left for
/// the next look. True: something was taken from it.
fn snap_merge(s: &Shared, seen: &mut u64) -> bool {
    let file = snap_file(s); let Some(stamp) = mtime_ms(&file) else { return false }; if stamp == *seen { return false }
    let local = std::env::temp_dir().join(format!("ufb_snapshot_restore_{}_{}.db", s.cfg.node, now_ms()));
    let r = std::fs::copy(&file, &local).map_err(|e| e.to_string()).and_then(|_| crate::db::with(|d| d.merge_snapshot(&local, s.cfg.only.as_deref())).unwrap_or_else(|| Err("the database could not be opened".into())));
    let _ = std::fs::remove_file(&local);
    match r { Ok(n) => { *seen = stamp; s.merged.store(now_ms() as u64, Ordering::Relaxed); say(s, &format!("snapshot merged: {n} row(s) taken")); if n > 0 { changed(s); } n > 0 }
        Err(e) => { say(s, &format!("snapshot not merged yet ({e})")); false } }
}
/// This node's shared rows are written as the snapshot: built on this machine, copied beside the snapshot under a
/// temporary name of this node's own, renamed over it; then the sidecar the same way; then the others are told.
fn snap_write(s: &Shared, seen: &mut u64) -> bool {
    let through = crate::db::with(|d| d.pending_last()).unwrap_or(0);
    let local = std::env::temp_dir().join(format!("ufb_snapshot_take_{}_{}.db", s.cfg.node, now_ms())); let file = snap_file(s);
    let wrote = crate::db::with(|d| d.write_snapshot(&local, s.cfg.only.as_deref())).unwrap_or_else(|| Err("the database could not be opened".into()));
    let r = match wrote { Ok(false) => { let _ = std::fs::remove_file(&local); return false } Err(e) => Err(e),
        Ok(true) => (|| -> std::io::Result<()> { if let Some(d) = file.parent() { std::fs::create_dir_all(d)?; }
            let tmp = file.with_file_name(format!("ufb_snapshot_v{}.db.{}.tmp", s.cfg.epoch, s.cfg.node)); std::fs::copy(&local, &tmp)?; std::fs::rename(&tmp, &file).inspect_err(|_| { let _ = std::fs::remove_file(&tmp); })?;
            let mtmp = snap_meta(s).with_file_name(format!("ufb_snapshot_v{}.meta.json.{}.tmp", s.cfg.epoch, s.cfg.node)); std::fs::write(&mtmp, json!({ "node_id": s.cfg.node, "written_at": now_ms() }).to_string())?; let _ = std::fs::rename(&mtmp, snap_meta(s)); Ok(()) })().map_err(|e| e.to_string()) };
    let _ = std::fs::remove_file(&local);
    match r { Ok(()) => { *seen = mtime_ms(&file).unwrap_or(0); s.written.store(now_ms() as u64, Ordering::Relaxed); say(s, "snapshot written");
            // what this node changed up to here is in it: it leaves the queue
            crate::db::with(|d| d.pending_done_through(through)); s.relayed.lock().unwrap().retain(|id| *id > through); s.pending.store(crate::db::with(|d| d.pending_count()).unwrap_or(0), Ordering::Relaxed); changed(s);
            // the others are told, now that it is there (1.4.2 tells them before)
            if !s.files_only { let peers: Vec<String> = s.peers.lock().unwrap().values().filter(|p| p.alive).map(|p| format!("{}:{}", p.ip, p.data_port)).collect(); for at in peers { post(&at, "/api/snapshot/notify", "{}"); } }
            true }
        Err(e) => { say(s, &format!("snapshot not written ({e})")); false } }
}
/// The snapshot's turn: every node looks for a newer file and merges it; the node that leads writes its own when
/// something changed - after merging what is there, and not over another node's that is younger than a minute.
fn snapshot_loop(s: &Shared) {
    let every = std::env::var("UFB_MESH_SNAPSHOT_SECS").ok().and_then(|v| v.parse().ok()).map(Duration::from_secs).unwrap_or(SNAP_EVERY);
    let (mut seen, mut looked, mut wrote, mut armed) = (0u64, std::time::Instant::now() - SNAP_LOOK, None::<std::time::Instant>, std::time::Instant::now());
    while !s.stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(700));
        if s.snap_look.swap(false, Ordering::Relaxed) || looked.elapsed() >= SNAP_LOOK.min(every) { looked = std::time::Instant::now();
            // what another node wrote brought something this node had not: when it leads, its own snapshot is due (the two sides of a split end up with one)
            if snap_merge(s, &mut seen) { s.snap_needed.store(true, Ordering::Relaxed); } }
        if armed.elapsed() >= SNAP_REARM { armed = std::time::Instant::now(); s.snap_needed.store(true, Ordering::Relaxed); }
        let leading = s.leader.lock().unwrap().as_deref() == Some(s.cfg.node.as_str());
        if !leading || !s.snap_needed.load(Ordering::Relaxed) || wrote.is_some_and(|t| t.elapsed() < every) { continue }
        // another node's snapshot, younger than a minute, is not written over (1.4.2's fence) - it is merged, and this one's turn comes
        if snap_author(s).is_some_and(|(who, at)| who != s.cfg.node && now_ms() - at < SNAP_FENCE_MS.min(every.as_millis() as i64 * 2)) { snap_merge(s, &mut seen); wrote = Some(std::time::Instant::now() - every + Duration::from_secs(2)); continue }
        snap_merge(s, &mut seen);
        s.snap_needed.store(false, Ordering::Relaxed); wrote = Some(std::time::Instant::now());
        if !snap_write(s, &mut seen) { /* nothing shared yet, or the folder is not there: at the next change */ }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_leader_is_1_4_2s_sort() {
        let n = |id: &str, tags: &[&str]| (id.to_string(), tags.iter().map(|t| t.to_string()).collect::<Vec<_>>());
        assert_eq!(elect(vec![n("mac", &[]), n("box", &[])]).as_deref(), Some("box"), "the id, in order");
        assert_eq!(elect(vec![n("box", &[]), n("mac", &["leader"])]).as_deref(), Some("mac"), "the tag leader first");
        assert_eq!(elect(vec![n("box", &["noleader"]), n("mac", &[])]).as_deref(), Some("mac"), "noleader last"); assert_eq!(elect(vec![n("box", &["noleader"])]).as_deref(), Some("box"), "alone, it leads all the same");
        assert_eq!(elect(vec![]), None);
    }
    #[test]
    fn the_secret_opens_the_door() {
        // this node's secret: taken; another: refused; none (a 1.4.2 node's request): only while a 1.4.2 node is among those that answer
        assert!(allowed("s3cret", Some("Bearer s3cret"), false)); assert!(!allowed("s3cret", Some("Bearer other"), false)); assert!(!allowed("s3cret", Some("Bearer other"), true)); assert!(!allowed("s3cret", Some("s3cret"), false));
        assert!(!allowed("s3cret", None, false)); assert!(allowed("s3cret", None, true)); assert!(allowed("", None, false), "no secret here: open, as 1.4.2 is");
        assert_eq!(fingerprint(""), ""); assert_eq!(fingerprint("s3cret").len(), 8); assert_ne!(fingerprint("s3cret"), fingerprint("other"));
    }
    #[test]
    fn only_what_is_shared_and_in_the_playground_goes_through() {
        let cfg = |only: Option<&str>| Config { farm: PathBuf::new(), node: "a".into(), epoch: TEST_EPOCH, ctl: 1, data: 2, udp: 3, tags: vec![], only: only.map(str::to_string) };
        assert!(in_scope(&cfg(None), "vol:1/jobs/x")); assert!(!in_scope(&cfg(None), "native:mac//Users/a"), "this machine's own is never shared");
        assert!(in_scope(&cfg(Some("vol:1/jobs/0001_Test")), "vol:1/jobs/0001_Test")); assert!(in_scope(&cfg(Some("vol:1/jobs/0001_Test")), "vol:1/jobs/0001_Test/3d/sh010")); assert!(!in_scope(&cfg(Some("vol:1/jobs/0001_Test")), "vol:1/jobs/0001_Test2")); assert!(!in_scope(&cfg(Some("vol:1/jobs/0001_Test")), "vol:1/jobs/other"));
        assert_eq!(scope_of(&json!({ "job_path": "vol:1/j" })), "vol:1/j"); assert_eq!(scope_of(&json!({ "action": "col_add", "def": "{\"jobPath\":\"vol:1/j\"}" })), "vol:1/j");
    }
}
