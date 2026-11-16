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
//! NOT here yet: the snapshot file (what a node that was away, or is new, catches up from), the merge by cell (row
//! 440), the shared secret (row 442), the columns' shared templates.
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
pub const EPOCH: u32 = 6; pub const CTL_PORT: u16 = 49221; pub const DATA_PORT: u16 = 49222; pub const UDP_PORT: u16 = 4265;
const GROUP: Ipv4Addr = Ipv4Addr::new(239, 42, 6, 3); const PASS: Duration = Duration::from_millis(3000); const FRESH_MS: i64 = 15_000; const DEAD_AFTER: u8 = 3;
/// The epoch a development build's mesh runs at: no installed UFB is of it.
pub const TEST_EPOCH: u32 = 9006;

#[derive(Clone, Debug)]
pub struct Config { pub farm: PathBuf, pub node: String, pub epoch: u32, pub ctl: u16, pub data: u16, pub udp: u16, pub tags: Vec<String>, /// only rows under this identity are sent and taken (a test's playground)
    pub only: Option<String> }
#[derive(Clone, Debug, PartialEq)]
struct Peer { ip: String, port: u16, data_port: u16, tags: Vec<String>, alive: bool, fails: u8, stamp: i64 }
/// What the window says of the mesh.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status { pub node: String, pub leader: Option<String>, pub leading: bool, pub peers: usize, pub pending: usize }

struct Shared { cfg: Config, peers: Mutex<HashMap<String, Peer>>, leader: Mutex<Option<String>>, pending: AtomicUsize, rev: AtomicU64, stop: Arc<AtomicBool>, wakers: Mutex<Vec<Arc<dyn Fn() + Send + Sync>>>, ip: Mutex<String> }
static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();
fn now_ms() -> i64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0) }
fn say(s: &Shared, what: &str) { if std::env::var_os("UFB_MESH_LOG").is_some() { eprintln!("UFB_MESH[{}]: {what}", s.cfg.node); } }

/// The mesh's state, for the window; none when the mesh is not running.
pub fn status() -> Option<Status> { let s = SHARED.get()?; let leader = s.leader.lock().unwrap().clone(); Some(Status { node: s.cfg.node.clone(), leading: leader.as_deref() == Some(s.cfg.node.as_str()), leader, peers: s.peers.lock().unwrap().values().filter(|p| p.alive).count(), pending: s.pending.load(Ordering::Relaxed) }) }
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
pub fn start(cfg: Config) {
    let s = Arc::new(Shared { cfg, peers: Default::default(), leader: Default::default(), pending: AtomicUsize::new(0), rev: AtomicU64::new(0), stop: Arc::new(AtomicBool::new(false)), wakers: Default::default(), ip: Mutex::new(local_ip()) });
    if SHARED.set(s.clone()).is_err() { return }
    say(&s, &format!("epoch {} farm {} ports {}/{}/{} only {:?}", s.cfg.epoch, s.cfg.farm.display(), s.cfg.ctl, s.cfg.data, s.cfg.udp, s.cfg.only));
    // the two servers: the status on the control port, everything else on the data port
    for (port, data) in [(s.cfg.ctl, false), (s.cfg.data, true)] {
        let sock = socket2::Socket::new(socket2::Domain::IPV4, socket2::Type::STREAM, None).and_then(|k| { k.set_reuse_address(true)?; k.bind(&SocketAddr::from(([0, 0, 0, 0], port)).into())?; k.listen(1024)?; Ok(k) });
        match sock { Ok(k) => { let (st, stop) = (s.clone(), s.stop.clone()); let _ = std::thread::Builder::new().name("ufb-mesh-http".into()).spawn(move || http::serve(k.into(), stop, move |q| if data { serve_data(&st, q) } else { serve_ctl(&st, q) })); }
            Err(e) => eprintln!("UFB: the mesh's port {port} could not be opened: {e}") }
    }
    { let st = s.clone(); let _ = std::thread::Builder::new().name("ufb-mesh-peers".into()).spawn(move || peers_loop(&st)); }
    { let st = s.clone(); let _ = std::thread::Builder::new().name("ufb-mesh-send".into()).spawn(move || send_loop(&st)); }
    { let st = s.clone(); let _ = std::thread::Builder::new().name("ufb-mesh-udp".into()).spawn(move || udp_listen(&st)); }
}

fn serve_ctl(s: &Shared, q: Request) -> Response {
    if q.method != "GET" || q.path.split('?').next() != Some("/api/status") { return Response::empty(404) }
    let st = status().unwrap_or_default();
    Response::json(200, json!({ "node_id": s.cfg.node, "is_leader": st.leading, "peer_count": st.peers, "last_snapshot_time": 0, "enabled": true, "tags": s.cfg.tags, "mesh_epoch": s.cfg.epoch }))
}
fn serve_data(s: &Arc<Shared>, q: Request) -> Response {
    let path = q.path.split('?').next().unwrap_or("").to_string(); if q.method != "POST" { return Response::empty(if path.starts_with("/api/") { 405 } else { 404 }) }
    if path == "/api/snapshot/notify" { return Response::ok() }   // (no snapshot yet: accepted, as 1.4.2's leader accepts and ignores it)
    if path != "/api/metadata/update" && path != "/api/table/update" { return Response::empty(404) }
    let Ok(v) = serde_json::from_slice::<Value>(&q.body) else { return Response::empty(400) };
    // another epoch's node is not of this mesh
    if v.get("mesh_epoch").and_then(|x| x.as_u64()).unwrap_or(0) != s.cfg.epoch as u64 { return Response::json(409, json!({ "error": "epoch" })) }
    let meta = path == "/api/metadata/update";
    if !in_scope(&s.cfg, &scope_of(&v)) { say(s, &format!("left out (not in what this node shares): {}", scope_of(&v))); return Response::ok() }
    let r = crate::db::with(|d| if meta { d.apply_meta(&v) } else { d.apply_table(&v) });
    match r {
        Some(Ok(took)) => { say(s, &format!("in {path}: {}", if took { "taken" } else { "not newer" })); if took { changed(s); }
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
fn post(at: &str, path: &str, body: &str) -> bool { http::call(at, "POST", path, None, Some(body), Duration::from_secs(3), Duration::from_secs(5)).is_some_and(|(code, _)| (200..300).contains(&code)) }

/// What waits in the database is sent: by a follower to the leader, by the leader to every node that is alive. What
/// was accepted leaves the queue; what was not stays, and is tried again at the next pass - for as long as it takes.
fn send_loop(s: &Shared) {
    while !s.stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(400));
        let pending = crate::db::with(|d| d.pending()).unwrap_or_default(); s.pending.store(crate::db::with(|d| d.pending_count()).unwrap_or(0), Ordering::Relaxed);
        if pending.is_empty() { continue }
        let (leader, me) = (s.leader.lock().unwrap().clone(), s.cfg.node.clone()); let Some(leader) = leader else { continue };
        let target = if leader == me { None } else { s.peers.lock().unwrap().get(&leader).filter(|p| p.alive).map(|p| format!("{}:{}", p.ip, p.data_port)) };
        // (this node leads and is alone: its changes wait for somebody to give them to)
        if leader == me && !s.peers.lock().unwrap().values().any(|p| p.alive) { continue }
        if leader != me && target.is_none() { continue }
        for (id, kind, key, payload) in pending {
            let msg: Option<Value> = match (&payload, kind.as_str()) { (Some(p), _) => serde_json::from_str(p).ok(), (None, "meta") => crate::db::with(|d| d.meta_wire(&key)).flatten(), (None, _) => crate::db::with(|d| d.col_wire(&key)).flatten() };
            // (the row is gone, or it is not of what this node shares: nothing to send)
            let Some(mut msg) = msg.filter(|m| in_scope(&s.cfg, &scope_of(m))) else { crate::db::with(|d| d.pending_done(id)); continue };
            msg["mesh_epoch"] = s.cfg.epoch.into(); let (path, body) = (if kind == "meta" { "/api/metadata/update" } else { "/api/table/update" }, msg.to_string());
            let ok = match &target { Some(at) => post(at, path, &body), None => relay(s, path, &body) };
            say(s, &format!("out {path} {key}: {}", if ok { "accepted" } else { "not yet" }));
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
                // (a fresh entry starts alive, a stale one waits for an answer)
                None => { say(s, &format!("found {id} at {ipv}:{port}")); peers.insert(id, Peer { ip: ipv, port, data_port: data, tags, alive: now - stamp < FRESH_MS, fails: 0, stamp }); } }
        } }
        // a heartbeat to the local network
        if let Some(u) = &udp { let hb = json!({ "t": "hb", "from": s.cfg.node, "n": s.cfg.node, "ip": ip, "port": s.cfg.ctl, "data_port": s.cfg.data, "epoch": s.cfg.epoch, "tags": s.cfg.tags }).to_string(); let _ = u.send_to(hb.as_bytes(), SocketAddr::from((GROUP, s.cfg.udp))); }
        // a look at each: alive when it answers, of this epoch (1.4.2: dead after three looks without an answer)
        let known: Vec<(String, String)> = s.peers.lock().unwrap().iter().map(|(id, p)| (id.clone(), format!("{}:{}", p.ip, p.port))).collect();
        for (id, at) in known {
            let got = http::call(&at, "GET", "/api/status", None, None, Duration::from_secs(3), Duration::from_secs(3)).filter(|(c, _)| *c == 200).and_then(|(_, b)| serde_json::from_slice::<Value>(&b).ok());
            let mut peers = s.peers.lock().unwrap(); let Some(p) = peers.get_mut(&id) else { continue };
            match got { Some(v) if v["mesh_epoch"].as_u64() == Some(s.cfg.epoch as u64) => { if !p.alive { say(s, &format!("{id} is alive")); } p.alive = true; p.fails = 0; p.tags = v["tags"].as_array().map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect()).unwrap_or_default(); }
                Some(_) => { peers.remove(&id); }
                None => if p.alive { p.fails += 1; if p.fails >= DEAD_AFTER { p.alive = false; say(s, &format!("{id} does not answer")); } } }
        }
        // who leads: this node and every node that is alive (or whose entry is fresh)
        let mut nodes = vec![(s.cfg.node.clone(), s.cfg.tags.clone())]; nodes.extend(s.peers.lock().unwrap().iter().filter(|(_, p)| p.alive || now - p.stamp < FRESH_MS).map(|(id, p)| (id.clone(), p.tags.clone())));
        let leader = elect(nodes); let was = s.leader.lock().unwrap().clone(); if leader != was { say(s, &format!("the leader is {leader:?}")); *s.leader.lock().unwrap() = leader; changed(s); }
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
                let p = Peer { ip: v["ip"].as_str().unwrap_or("").to_string(), port: v["port"].as_u64().unwrap_or(0) as u16, data_port: data, tags: v["tags"].as_array().map(|a| a.iter().filter_map(|t| t.as_str().map(str::to_string)).collect()).unwrap_or_default(), alive: true, fails: 0, stamp: now_ms() };
                let mut peers = s.peers.lock().unwrap(); if !peers.get(&id).is_some_and(|o| o.alive) { say(s, &format!("heard {id} at {}:{}", p.ip, p.port)); } peers.insert(id, p); }
            Some("bye") => { if let Some(p) = s.peers.lock().unwrap().get_mut(&id) { p.alive = false; } }
            _ => {}
        }
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
    fn only_what_is_shared_and_in_the_playground_goes_through() {
        let cfg = |only: Option<&str>| Config { farm: PathBuf::new(), node: "a".into(), epoch: TEST_EPOCH, ctl: 1, data: 2, udp: 3, tags: vec![], only: only.map(str::to_string) };
        assert!(in_scope(&cfg(None), "vol:1/jobs/x")); assert!(!in_scope(&cfg(None), "native:mac//Users/a"), "this machine's own is never shared");
        assert!(in_scope(&cfg(Some("vol:1/jobs/0001_Test")), "vol:1/jobs/0001_Test")); assert!(in_scope(&cfg(Some("vol:1/jobs/0001_Test")), "vol:1/jobs/0001_Test/3d/sh010")); assert!(!in_scope(&cfg(Some("vol:1/jobs/0001_Test")), "vol:1/jobs/0001_Test2")); assert!(!in_scope(&cfg(Some("vol:1/jobs/0001_Test")), "vol:1/jobs/other"));
        assert_eq!(scope_of(&json!({ "job_path": "vol:1/j" })), "vol:1/j"); assert_eq!(scope_of(&json!({ "action": "col_add", "def": "{\"jobPath\":\"vol:1/j\"}" })), "vol:1/j");
    }
}
