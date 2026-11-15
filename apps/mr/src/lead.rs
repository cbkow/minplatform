//! What a LEADER holds and does (contract 2.5 rows 9-30, 3.3, 3.4, 4.3-4.9, 5.1): the job database, the queue, the
//! dispatch cycle, the reports, the commands, the snapshot. A node has one of these only while it leads.
//!
//! The database is 0.5.x's, statement for statement: a 0.5.x node that takes over opens this node's snapshot, and
//! this node opens a 0.5.x leader's.
use crate::http::Response;
use crate::node::now_ms;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// What has to happen outside the database after a request or a cycle (the node does these, with no lock held).
pub enum Effect { /// this node's own render of a job is ended; true: reported as a failure, false: abandoned
    CutOwn(String, bool), /// `POST /api/render/abort` to these nodes
    Abort(Vec<String>, String, &'static str), /// the job's folder on the share is removed
    RemoveJobDir(String), /// a chunk is handed to a node (this one included)
    Dispatch { node: String, manifest: Value, start: i64, end: i64, epoch: i64 } }
/// A node as the dispatch sees it.
pub struct Candidate { pub id: String, pub alive: bool, pub can_work: bool, pub rendering: bool, pub needs_attention: bool, pub tags: Vec<String>, /// what it says it renders now: (job, chunk label), when it renders
    pub active: Option<(String, String)> }
struct Failures { count: u32, first_ms: u64, last_ms: u64, suspended: bool }
struct Report { node: String, job: String, start: i64, end: i64, epoch: i64 }

pub struct Lead {
    db: Connection, me: String, submissions: Vec<(Value, i64)>, completes: Vec<Report>, fails: Vec<Report>, reverts: Vec<(String, i64, i64)>, frames: Vec<(String, i64, Vec<i64>)>,
    tracker: BTreeMap<String, Failures>, dispatched: BTreeMap<String, Instant>, /// the list as `GET /api/jobs` gives it: made every cycle, so it trails a command by up to two seconds (seen of 0.5.11)
    list: Option<String>, last_snapshot: Instant, data: PathBuf, /// chunks whose holder is alive and not rendering them, and since when (row 396)
    orphaned: BTreeMap<i64, Instant>,
}
/// A manifest read for its tags alone.
#[derive(serde::Deserialize)]
struct TagsOnly { #[serde(default)] tags_required: Vec<String> }
fn safe_segment(s: &str) -> bool { !s.is_empty() && s != "." && s != ".." && !s.contains(['/', '\\', ':']) }
fn chunks_of(a: i64, b: i64, size: i64) -> Vec<(i64, i64)> { let mut v = Vec::new(); if size <= 0 || a > b { return v } let mut f = a; while f <= b { v.push((f, (f + size - 1).min(b))); f += size; } v }
fn strings(text: &str) -> Vec<String> { serde_json::from_str::<Vec<String>>(text).unwrap_or_default() }
fn ints(text: &str) -> Vec<i64> { serde_json::from_str::<Vec<i64>>(text).unwrap_or_default() }

impl Lead {
    /// Taking over (3.4): the share's snapshot is copied over the local file and opened; with no snapshot, the local file as it is.
    pub fn take_over(data: &Path, farm: &Path, me: &str) -> Result<Lead, String> {
        let local = data.join("minrender.db"); let snap = farm.join("state").join("snapshot.db");
        if snap.is_file() { for ext in ["-wal", "-shm"] { let _ = std::fs::remove_file(format!("{}{ext}", local.display())); } if let Err(e) = std::fs::copy(&snap, &local) { eprintln!("the snapshot could not be copied ({e}): the local database is used as it is"); } }
        let db = Connection::open(&local).map_err(|e| e.to_string())?;
        let _ = db.pragma_update(None, "journal_mode", "WAL"); let _ = db.pragma_update(None, "foreign_keys", "ON"); let _ = db.busy_timeout(std::time::Duration::from_millis(5000));
        db.execute_batch("CREATE TABLE IF NOT EXISTS jobs (
    job_id TEXT PRIMARY KEY,
    manifest_json TEXT NOT NULL,
    current_state TEXT NOT NULL DEFAULT 'active',
    priority INTEGER NOT NULL DEFAULT 50,
    submitted_at_ms INTEGER NOT NULL,
    edit_epoch INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS chunks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id TEXT NOT NULL REFERENCES jobs(job_id) ON DELETE CASCADE,
    frame_start INTEGER NOT NULL,
    frame_end INTEGER NOT NULL,
    state TEXT NOT NULL DEFAULT 'pending',
    assigned_to TEXT,
    assigned_at_ms INTEGER,
    completed_at_ms INTEGER,
    retry_count INTEGER NOT NULL DEFAULT 0,
    completed_frames TEXT NOT NULL DEFAULT '[]',
    failed_on TEXT NOT NULL DEFAULT '[]',
    edit_epoch INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_chunks_job ON chunks(job_id);
CREATE INDEX IF NOT EXISTS idx_chunks_state ON chunks(state);").map_err(|e| e.to_string())?;
        // the additive steps, each tried on every open (a database of an older leader lacks some of these)
        for m in ["ALTER TABLE chunks ADD COLUMN completed_frames TEXT NOT NULL DEFAULT '[]'", "ALTER TABLE chunks ADD COLUMN failed_on TEXT NOT NULL DEFAULT '[]'", "ALTER TABLE jobs ADD COLUMN sort_key INTEGER",
            "UPDATE jobs SET sort_key = submitted_at_ms WHERE sort_key IS NULL", "ALTER TABLE jobs ADD COLUMN edit_epoch INTEGER NOT NULL DEFAULT 0", "ALTER TABLE chunks ADD COLUMN edit_epoch INTEGER NOT NULL DEFAULT 0"] { let _ = db.execute(m, []); }
        Ok(Lead { db, me: me.to_string(), submissions: Vec::new(), completes: Vec::new(), fails: Vec::new(), reverts: Vec::new(), frames: Vec::new(), tracker: BTreeMap::new(), dispatched: BTreeMap::new(), list: None, last_snapshot: Instant::now(), data: data.to_path_buf(), orphaned: BTreeMap::new() })
    }
    fn manifest(&self, id: &str) -> Option<Value> { self.db.query_row("SELECT manifest_json FROM jobs WHERE job_id = ?", [id], |r| r.get::<_, String>(0)).optional().ok().flatten().and_then(|t| serde_json::from_str(&t).ok()) }
    fn exists(&self, id: &str) -> bool { self.db.query_row("SELECT 1 FROM jobs WHERE job_id = ?", [id], |_| Ok(())).optional().ok().flatten().is_some() }
    fn epoch(&self, id: &str) -> Option<i64> { self.db.query_row("SELECT edit_epoch FROM jobs WHERE job_id = ?", [id], |r| r.get(0)).optional().ok().flatten() }
    fn insert_chunks(&self, id: &str, m: &Value) { let Ok(mut st) = self.db.prepare_cached("INSERT INTO chunks (job_id, frame_start, frame_end) VALUES (?, ?, ?)") else { return };
        for (a, b) in chunks_of(m["frame_start"].as_i64().unwrap_or(1), m["frame_end"].as_i64().unwrap_or(250), m["chunk_size"].as_i64().unwrap_or(1)) { let _ = st.execute(params![id, a, b]); } }
    /// A job enters the database: `active`, its chunks `pending`. An id that is already there is dropped without a word (2.5 row 9).
    fn insert(&self, m: &Value, priority: i64, at_ms: i64) -> bool {
        let Some(id) = m["job_id"].as_str() else { return false };
        if self.db.execute("INSERT INTO jobs (job_id, manifest_json, current_state, priority, submitted_at_ms, sort_key) VALUES (?, ?, 'active', ?, ?, ?)", params![id, m.to_string(), priority, at_ms, at_ms]).is_err() { return false }
        self.insert_chunks(id, m); true
    }
    /// The chunk a report is about, if the report is current (4.4): the job, the range, `assigned`, to that node, in that epoch.
    fn current(&self, r: &Report) -> Option<(i64, i64, String)> {
        self.db.query_row("SELECT id, retry_count, failed_on FROM chunks WHERE job_id = ? AND frame_start = ? AND frame_end = ? AND state = 'assigned' AND (?4 = '' OR assigned_to = ?4) AND (?5 < 0 OR edit_epoch = ?5)", params![r.job, r.start, r.end, r.node, r.epoch], |x| Ok((x.get(0)?, x.get(1)?, x.get(2)?))).optional().ok().flatten()
    }

    // ------------------------------------------------------------------------------------------------------ requests
    /// A request for the leader. `None`: not one of the leader's routes.
    pub fn handle(&mut self, method: &str, path: &str, body: &[u8]) -> Option<(Response, Vec<Effect>)> {
        let mut fx = Vec::new(); let seg: Vec<&str> = path.trim_start_matches("/api/").split('/').collect();
        let parsed = || serde_json::from_slice::<Value>(body).map_err(|e| Response::error(400, &e.to_string()));
        macro_rules! body { () => { match parsed() { Ok(v) => v, Err(r) => return Some((r, fx)) } } }
        let report = |v: &Value| Report { node: v["node_id"].as_str().unwrap_or("").to_string(), job: v["job_id"].as_str().unwrap_or("").to_string(), start: v["frame_start"].as_i64().unwrap_or(0), end: v["frame_end"].as_i64().unwrap_or(0), epoch: v["edit_epoch"].as_i64().unwrap_or(-1) };
        let stale = || Response::json(200, json!({ "status": "stale", "stale": true }));
        let r = match (method, seg.as_slice()) {
            ("POST", ["jobs"]) => { let v = body!(); let Some(m) = v.get("manifest") else { return Some((Response::error(400, "key 'manifest' not found"), fx)) };
                if !safe_segment(m["job_id"].as_str().unwrap_or("")) { Response::error(400, "invalid_job_id") } else { self.submissions.push((m.clone(), v["priority"].as_i64().unwrap_or(50))); Response::ok() } }
            ("GET", ["jobs"]) => Response { status: 200, content_type: Some("application/json"), body: self.list.clone().unwrap_or_default().into_bytes() },
            ("GET", ["nodes", "suspended"]) => Response::json(200, Value::Array(self.tracker.iter().filter(|(_, f)| f.suspended).map(|(n, f)| json!({ "node_id": n, "failure_count": f.count, "first_failure_ms": f.first_ms, "last_failure_ms": f.last_ms })).collect())),
            ("POST", ["nodes", node, "unsuspend"]) => { self.tracker.remove(*node); Response::ok() }
            ("POST", ["dispatch", "frame-complete"]) => { let v = body!(); let (job, epoch) = (v["job_id"].as_str().unwrap_or("").to_string(), v["edit_epoch"].as_i64().unwrap_or(-1));
                if epoch >= 0 && self.epoch(&job).is_some_and(|e| e != epoch) { stale() } else { self.frames.push((job, epoch, v["frames"].as_array().map(|a| a.iter().filter_map(Value::as_i64).collect()).unwrap_or_default())); Response::ok() } }
            ("POST", ["dispatch", kind @ ("complete" | "failed")]) => { let v = body!(); let r = report(&v); if self.current(&r).is_none() { stale() } else { if *kind == "complete" { self.completes.push(r); } else { self.fails.push(r); } Response::ok() } }
            ("POST", ["chunks", "reassign"]) => { let v = body!(); let Some(id) = v["chunk_id"].as_i64() else { return Some((Response::error(400, "key 'chunk_id' not found"), fx)) };
                match v["target_node"].as_str().filter(|t| !t.is_empty()) {
                    Some(t) => { let _ = self.db.execute("UPDATE chunks SET state = 'assigned', assigned_to = ?, assigned_at_ms = ? WHERE id = ? AND state IN ('assigned', 'failed', 'stopped')", params![t, now_ms() as i64, id]); }
                    None => { let job: Option<String> = self.db.query_row("SELECT job_id FROM chunks WHERE id = ? AND state IN ('assigned', 'failed', 'stopped')", [id], |r| r.get(0)).optional().ok().flatten();
                        if let Some(job) = job { let _ = self.db.execute("UPDATE chunks SET state = 'pending', assigned_to = NULL, assigned_at_ms = NULL WHERE id = ?", [id]); let _ = self.db.execute("UPDATE jobs SET current_state = 'active' WHERE job_id = ? AND current_state IN ('completed', 'failed')", [job]); } } }
                Response::ok() }
            ("POST", ["chunks", "stop"]) => { let v = body!(); let Some(id) = v["chunk_id"].as_i64() else { return Some((Response::error(400, "key 'chunk_id' not found"), fx)) };
                let row: Option<(String, String, Option<String>)> = self.db.query_row("SELECT job_id, state, assigned_to FROM chunks WHERE id = ? AND state IN ('pending', 'assigned')", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).optional().ok().flatten();
                if let Some((job, state, who)) = row { let _ = self.db.execute("UPDATE chunks SET state = 'stopped', assigned_to = NULL, assigned_at_ms = NULL WHERE id = ?", [id]);
                    if state == "assigned" { match who { Some(w) if w == self.me => fx.push(Effect::CutOwn(job, false)), Some(w) => fx.push(Effect::Abort(vec![w], job, "chunk stopped")), None => {} } } }
                Response::ok() }
            ("GET", ["jobs", id]) => match self.detail(id) { Some(v) => Response::json(200, v), None => Response::error(404, "not_found") },
            ("DELETE", ["jobs", id]) => { if !safe_segment(id) { Response::error(400, "invalid_path") } else { let _ = self.db.execute("UPDATE jobs SET current_state = 'cancelled' WHERE job_id = ?", [id]); let _ = self.db.execute("DELETE FROM chunks WHERE job_id = ?", [id]); let _ = self.db.execute("DELETE FROM jobs WHERE job_id = ?", [id]);
                fx.push(Effect::CutOwn(id.to_string(), true)); fx.push(Effect::RemoveJobDir(id.to_string())); Response::ok() } }
            ("GET", ["jobs", id, "manifest"]) => match self.db.query_row("SELECT manifest_json FROM jobs WHERE job_id = ?", [id], |r| r.get::<_, String>(0)).optional().ok().flatten() { None => Response::error(404, "not_found"),
                Some(t) => match serde_json::from_str::<Value>(&t) { Ok(m) => Response::json(200, json!({ "manifest": m })), Err(e) => Response::error(500, &e.to_string()) } },
            ("POST", ["jobs", id, state @ ("pause" | "resume" | "cancel" | "archive")]) => { let to = match *state { "pause" => "paused", "resume" => "active", "cancel" => "cancelled", _ => "archived" };
                let _ = self.db.execute("UPDATE jobs SET current_state = ? WHERE job_id = ?", params![to, id]); if *state == "cancel" { fx.push(Effect::CutOwn(id.to_string(), true)); } Response::ok() }
            ("POST", ["jobs", id, "priority"]) => { let v = body!(); let Some(p) = v["priority"].as_i64() else { return Some((Response::error(400, "key 'priority' not found"), fx)) };
                let _ = self.db.execute("UPDATE jobs SET priority = ?, sort_key = submitted_at_ms WHERE job_id = ?", params![p, id]); Response::ok() }
            ("POST", ["jobs", id, "move"]) => { let v = body!(); let Some(target) = v["target"].as_str() else { return Some((Response::error(400, "key 'target' not found"), fx)) };
                if self.move_job(id, target, v["position"].as_str().unwrap_or("before") == "before") { Response::ok() } else { Response::error(409, "move_rejected") } }
            // "Retry failed": failed chunks go back to the queue. The nodes barred from each are let in again (ruled
            // 2026-11-14: a person asking for another try means it; 0.5.x kept the bar, and where every node had
            // failed a chunk the retried chunk could never be given out). The automatic retries keep the bar.
            ("POST", ["jobs", id, "retry-failed"]) => { let n = self.db.execute("UPDATE chunks SET state = 'pending', retry_count = 0, completed_frames = '[]', failed_on = '[]', assigned_to = NULL, assigned_at_ms = NULL WHERE job_id = ? AND state = 'failed'", [id]).unwrap_or(0);
                if n > 0 { let _ = self.db.execute("UPDATE jobs SET current_state = 'active' WHERE job_id = ?", [id]); } Response::ok() }
            ("POST", ["jobs", id, "requeue"]) => { let _ = self.db.execute("UPDATE chunks SET state = 'pending', assigned_to = NULL, assigned_at_ms = NULL, completed_at_ms = NULL, retry_count = 0, completed_frames = '[]', failed_on = '[]' WHERE job_id = ?", [id]);
                let _ = self.db.execute("UPDATE jobs SET current_state = 'active' WHERE job_id = ?", [id]); Response::ok() }
            ("POST", ["jobs", id, "edit"]) => { let Ok(v) = serde_json::from_slice::<Value>(body) else { return Some((Response::json(400, json!({ "error": "invalid_manifest", "detail": "the body is not JSON" })), fx)) };
                let Some(m) = v.get("manifest").filter(|m| m.is_object()) else { return Some((Response::json(400, json!({ "error": "invalid_manifest", "detail": "no manifest" })), fx)) };
                if m["job_id"].as_str() != Some(id) { Response::json(400, json!({ "error": "invalid_manifest", "detail": "the manifest's job_id is not the job's" })) } else if !self.exists(id) { Response::error(404, "job_not_found") } else {
                    let _ = self.db.execute("UPDATE jobs SET priority = ?, sort_key = submitted_at_ms, manifest_json = ? WHERE job_id = ?", params![v["priority"].as_i64().unwrap_or(50), m.to_string(), id]);
                    let mode = v["mode"].as_str().unwrap_or("continue");
                    if mode == "restart" || mode == "startover" {
                        let holders: Vec<String> = self.db.prepare("SELECT DISTINCT assigned_to FROM chunks WHERE job_id = ? AND state = 'assigned' AND assigned_to IS NOT NULL").and_then(|mut s| s.query_map([id], |r| r.get(0))?.collect()).unwrap_or_default();
                        let _ = self.db.execute("UPDATE jobs SET edit_epoch = edit_epoch + 1, current_state = 'active' WHERE job_id = ?", [id]);
                        if mode == "startover" { let _ = self.db.execute("DELETE FROM chunks WHERE job_id = ?", [id]); self.insert_chunks(id, m); } else { let _ = self.db.execute("UPDATE chunks SET state = 'pending', assigned_to = NULL, assigned_at_ms = NULL WHERE job_id = ? AND state = 'assigned'", [id]); }
                        if holders.iter().any(|h| *h == self.me) { fx.push(Effect::CutOwn(id.to_string(), false)); } let others: Vec<String> = holders.into_iter().filter(|h| *h != self.me).collect(); if !others.is_empty() { fx.push(Effect::Abort(others, id.to_string(), "job edited")); }
                    }
                    Response::ok() } }
            ("POST", ["jobs", id, "resubmit"]) => { let v: Value = serde_json::from_slice(body).unwrap_or(Value::Null); match self.resubmit(id, &v) { Some(new) => Response::json(200, json!({ "status": "ok", "job_id": new })), None => Response::error(404, "resubmit_failed") } }
            _ => return None,
        };
        Some((r, fx))
    }
    /// Every job with its state and how many chunks it has - the archived ones too, which no list shows (for Farm Cleanup).
    pub fn all_jobs(&self) -> Vec<(String, String, i64)> { self.db.prepare("SELECT j.job_id, j.current_state, (SELECT COUNT(*) FROM chunks c WHERE c.job_id = j.job_id AND c.state != 'stopped') FROM jobs j ORDER BY j.job_id").and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect()).unwrap_or_default() }
    fn detail(&self, id: &str) -> Option<Value> {
        let (state, priority, at, m): (String, i64, i64, String) = self.db.query_row("SELECT current_state, priority, submitted_at_ms, manifest_json FROM jobs WHERE job_id = ?", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).optional().ok().flatten()?;
        let chunks: Vec<Value> = self.db.prepare("SELECT id, frame_start, frame_end, state, assigned_to, assigned_at_ms, completed_at_ms, retry_count, completed_frames, failed_on FROM chunks WHERE job_id = ? ORDER BY frame_start").and_then(|mut s| s.query_map([id], |r| Ok(json!({
            "id": r.get::<_, i64>(0)?, "frame_start": r.get::<_, i64>(1)?, "frame_end": r.get::<_, i64>(2)?, "state": r.get::<_, String>(3)?, "assigned_to": r.get::<_, Option<String>>(4)?.unwrap_or_default(), "assigned_at_ms": r.get::<_, Option<i64>>(5)?.unwrap_or(0),
            "completed_at_ms": r.get::<_, Option<i64>>(6)?.unwrap_or(0), "retry_count": r.get::<_, i64>(7)?, "completed_frames": ints(&r.get::<_, String>(8)?), "failed_on": strings(&r.get::<_, String>(9)?) })))?.collect()).unwrap_or_default();
        Some(json!({ "job_id": id, "current_state": state, "priority": priority, "submitted_at_ms": at, "manifest": serde_json::from_str::<Value>(&m).unwrap_or(Value::Null), "chunks": chunks }))
    }
    /// The queue's one order (4.5).
    const ORDER: &'static str = "ORDER BY priority ASC, COALESCE(sort_key, submitted_at_ms) ASC, submitted_at_ms ASC";
    /// A job is put before or after another (4.5): it takes the target's priority, and the whole group is renumbered.
    fn move_job(&self, id: &str, target: &str, before: bool) -> bool {
        if id == target || !self.exists(id) || !self.exists(target) { return false }
        let prio = |j: &str| self.db.query_row("SELECT priority FROM jobs WHERE job_id = ?", [j], |r| r.get::<_, i64>(0)).unwrap_or(50);
        let p = prio(target); if prio(id) != p { let _ = self.db.execute("UPDATE jobs SET priority = ? WHERE job_id = ?", params![p, id]); }
        let mut group: Vec<String> = self.db.prepare(&format!("SELECT job_id FROM jobs WHERE priority = ? {}", Self::ORDER)).and_then(|mut s| s.query_map([p], |r| r.get(0))?.collect()).unwrap_or_default();
        group.retain(|j| j != id); let Some(at) = group.iter().position(|j| j == target) else { return false }; group.insert(if before { at } else { at + 1 }, id.to_string());
        for (i, j) in group.iter().enumerate() { let _ = self.db.execute("UPDATE jobs SET sort_key = ? WHERE job_id = ?", params![(i as i64 + 1) * 1024, j]); }
        true
    }
    /// A job again under a new id (4.9, 4.2): the same range, another range, or only what is not done.
    fn resubmit(&self, id: &str, v: &Value) -> Option<String> {
        let mut m = self.manifest(id)?; let priority: i64 = self.db.query_row("SELECT priority FROM jobs WHERE job_id = ?", [id], |r| r.get(0)).ok()?;
        let rows: Vec<(i64, i64, String)> = self.db.prepare("SELECT frame_start, frame_end, state FROM chunks WHERE job_id = ? ORDER BY frame_start").and_then(|mut s| s.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect()).ok()?;
        if rows.is_empty() { return None } let only = v["incomplete_only"] == json!(true); if only && rows.iter().all(|(_, _, s)| s == "completed") { return None }
        let base = { let cut = id.rfind("-v").filter(|i| id[i + 2..].chars().all(|c| c.is_ascii_digit()) && id.len() > i + 2); cut.map(|i| &id[..i]).unwrap_or(id).to_string() };
        let new = (2..1000).map(|n| format!("{base}-v{n}")).find(|c| !self.exists(c))?;
        m["job_id"] = json!(new); let now = now_ms() as i64; m["submitted_at_ms"] = json!(now);
        for k in ["frame_start", "frame_end", "chunk_size"] { if let Some(x) = v[k].as_i64() { m[k] = json!(x); } }
        self.db.execute("INSERT INTO jobs (job_id, manifest_json, current_state, priority, submitted_at_ms, sort_key) VALUES (?, ?, 'active', ?, ?, ?)", params![new, m.to_string(), priority, now, now]).ok()?;
        if only { for (a, b, s) in rows { let _ = self.db.execute("INSERT INTO chunks (job_id, frame_start, frame_end, state, completed_at_ms) VALUES (?, ?, ?, ?, ?)", params![new, a, b, if s == "completed" { "completed" } else { "pending" }, if s == "completed" { Some(now) } else { None }]); } } else { self.insert_chunks(&new, &m); }
        Some(new)
    }

    // --------------------------------------------------------------------------------------------------- the cycle
    /// One dispatch cycle (4.6), every two seconds: what was queued is applied, dead workers' chunks come back,
    /// finished jobs are marked, each free node gets at most one chunk, the list is made anew.
    pub fn cycle(&mut self, nodes: &[Candidate], farm: &Path) -> Vec<Effect> {
        let mut fx = Vec::new(); let now = now_ms();
        // (one transaction for the whole cycle: a hundred thousand chunks are one write to the disk, not as many)
        let _ = self.db.execute_batch("BEGIN IMMEDIATE");
        for (m, p) in std::mem::take(&mut self.submissions) { let at = m["submitted_at_ms"].as_i64().unwrap_or(now as i64); self.insert(&m, p, at); }
        for r in std::mem::take(&mut self.completes) { if let Some((id, _, _)) = self.current(&r) { let _ = self.db.execute("UPDATE chunks SET state = 'completed', completed_at_ms = ? WHERE id = ?", params![now as i64, id]); } }
        for r in std::mem::take(&mut self.fails) { if let Some((id, retries, barred)) = self.current(&r) {
            let max = self.manifest(&r.job).and_then(|m| m["max_retries"].as_i64()).unwrap_or(3); let mut b = strings(&barred); if !r.node.is_empty() && !b.contains(&r.node) { b.push(r.node.clone()); } let b = serde_json::to_string(&b).unwrap();
            if retries + 1 < max { let _ = self.db.execute("UPDATE chunks SET state = 'pending', assigned_to = NULL, assigned_at_ms = NULL, retry_count = retry_count + 1, failed_on = ? WHERE id = ?", params![b, id]); }
            else { let _ = self.db.execute("UPDATE chunks SET state = 'failed', retry_count = retry_count + 1, failed_on = ? WHERE id = ?", params![b, id]); }
            // (the leader's own failures are not counted against it: 4.7)
            if r.node != self.me && !r.node.is_empty() { let f = self.tracker.entry(r.node.clone()).or_insert(Failures { count: 0, first_ms: now, last_ms: now, suspended: false }); if now.saturating_sub(f.first_ms) > 300_000 { f.count = 0; f.first_ms = now; } f.count += 1; f.last_ms = now; if f.count >= 5 { f.suspended = true; } } } }
        for (job, a, b) in std::mem::take(&mut self.reverts) { let _ = self.db.execute("UPDATE chunks SET state = 'pending', assigned_to = NULL, assigned_at_ms = NULL WHERE job_id = ? AND frame_start = ? AND frame_end = ?", params![job, a, b]); }
        for (job, epoch, frames) in std::mem::take(&mut self.frames) { if epoch >= 0 && self.epoch(&job).is_some_and(|e| e != epoch) { continue }
            for f in frames { if let Some((id, text)) = self.db.query_row("SELECT id, completed_frames FROM chunks WHERE job_id = ? AND frame_start <= ?2 AND frame_end >= ?2", params![job, f], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))).optional().ok().flatten() {
                let mut v = ints(&text); if !v.contains(&f) { v.push(f); v.sort(); let _ = self.db.execute("UPDATE chunks SET completed_frames = ? WHERE id = ?", params![serde_json::to_string(&v).unwrap(), id]); } } } }
        // chunks held by a node that is dead, or alive and asking for attention, come back (no retry is counted)
        let held: Vec<(i64, String, String, i64, i64, i64)> = self.db.prepare("SELECT id, assigned_to, job_id, frame_start, frame_end, COALESCE(assigned_at_ms, 0) FROM chunks WHERE state = 'assigned' AND assigned_to IS NOT NULL").and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?.collect()).unwrap_or_default();
        let mut still = BTreeMap::new();
        for (id, who, job, a, b, at) in held {
            let node = nodes.iter().find(|n| n.id == who);
            if node.is_none_or(|n| !n.alive || n.needs_attention) { let _ = self.db.execute("UPDATE chunks SET state = 'pending', assigned_to = NULL, assigned_at_ms = NULL WHERE id = ?", [id]); continue }
            // A chunk whose holder is ALIVE and is not rendering it (ruled 2026-11-14, row 396). It happens at a
            // takeover: the holder finished the chunk and told the old leader, which died before its next snapshot;
            // the holder never says it again, and under 0.5.x the chunk stays `assigned` for ever. Here: a holder that
            // has been free, or rendering something else, for twelve seconds (four of its polls) on a chunk given out
            // at least fifteen seconds ago gives the chunk back to the queue. No retry is counted.
            let n = node.unwrap(); let label = if a == b { format!("f{a}") } else { format!("f{a}-{b}") };
            let renders_it = n.active.as_ref().is_some_and(|(j, c)| *j == job && *c == label); let waiting = !n.rendering && !n.can_work && n.active.is_none();
            if renders_it || waiting && self.dispatched.contains_key(&who) || now.saturating_sub(at as u64) < 15_000 { continue }
            let since = *self.orphaned.get(&id).unwrap_or(&Instant::now()); still.insert(id, since);
            if since.elapsed().as_secs() >= 12 { eprintln!("a chunk of {job} was held by a node that is not rendering it: returned to the queue"); let _ = self.db.execute("UPDATE chunks SET state = 'pending', assigned_to = NULL, assigned_at_ms = NULL WHERE id = ?", [id]); still.remove(&id); }
        }
        self.orphaned = still;
        // active jobs with nothing left to do are completed, or failed if a chunk is
        let done: Vec<(String, i64)> = self.db.prepare_cached("SELECT j.job_id, COALESCE(SUM(c.state = 'failed'), 0) FROM jobs j LEFT JOIN chunks c ON c.job_id = j.job_id WHERE j.current_state = 'active' GROUP BY j.job_id HAVING COALESCE(SUM(c.state NOT IN ('completed', 'failed', 'stopped')), 0) = 0").and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect()).unwrap_or_default();
        for (job, failed) in done { let _ = self.db.execute("UPDATE jobs SET current_state = ? WHERE job_id = ?", params![if failed > 0 { "failed" } else { "completed" }, job]); }
        // the work: peers in the order of their ids, this node last; one chunk a node a cycle
        self.dispatched.retain(|id, t| t.elapsed().as_secs() < 15 && nodes.iter().find(|n| n.id == *id).is_some_and(|n| n.alive && !n.rendering));
        let mut order: Vec<&Candidate> = nodes.iter().filter(|n| n.id != self.me).collect(); order.sort_by(|a, b| a.id.cmp(&b.id)); order.extend(nodes.iter().filter(|n| n.id == self.me));
        let free: Vec<&Candidate> = order.into_iter().filter(|n| n.alive && n.can_work && !n.rendering && !n.needs_attention && !self.dispatched.contains_key(&n.id) && !self.tracker.get(&n.id).is_some_and(|f| f.suspended || now.saturating_sub(f.last_ms) < 30_000)).collect();
        // the jobs that have something to give, in the queue's order - read once a cycle, and only when a node is free;
        // a job's tags are read without the rest of its manifest until a node may take it
        let mut jobs: Vec<(String, String, i64, Vec<String>, bool)> = if free.is_empty() { Vec::new() } else { self.db.prepare_cached(&format!("SELECT job_id, manifest_json, edit_epoch FROM jobs WHERE current_state = 'active' AND job_id IN (SELECT DISTINCT job_id FROM chunks WHERE state = 'pending') {}", Self::ORDER)).and_then(|mut s| s.query_map([], |r| { let text: String = r.get(1)?;
            let tags = serde_json::from_str::<TagsOnly>(&text).map(|t| t.tags_required).unwrap_or_default(); Ok((r.get(0)?, text, r.get(2)?, tags, true)) })?.collect()).unwrap_or_default() };
        for n in free {
            'jobs: for (job, text, epoch, tags, has) in jobs.iter_mut() { if !*has || !tags.iter().all(|t| n.tags.iter().any(|x| x == t)) { continue }
                let Ok(m) = serde_json::from_str::<Value>(text) else { continue }; let (job, epoch) = (job.clone(), *epoch);
                // (the first pending chunk this node is not barred from; the bar is almost always empty, so a few rows are read)
                let pending: Vec<(i64, i64, i64, String)> = self.db.prepare_cached("SELECT id, frame_start, frame_end, failed_on FROM chunks WHERE job_id = ? AND state = 'pending' AND failed_on NOT LIKE ? ORDER BY frame_start LIMIT 1").and_then(|mut s| s.query_map(params![job, format!("%\"{}\"%", n.id)], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect()).unwrap_or_default();
                if pending.is_empty() { *has = false; }
                for (id, a, b, barred) in pending { if strings(&barred).contains(&n.id) { continue }
                    let _ = self.db.execute("UPDATE chunks SET state = 'assigned', assigned_to = ?, assigned_at_ms = ?, edit_epoch = ? WHERE id = ?", params![n.id, now as i64, epoch, id]);
                    self.dispatched.insert(n.id.clone(), Instant::now()); fx.push(Effect::Dispatch { node: n.id.clone(), manifest: m, start: a, end: b, epoch }); break 'jobs }
            }
        }
        self.list = Some(self.summaries());
        let _ = self.db.execute_batch("COMMIT");
        if self.last_snapshot.elapsed().as_secs() >= 30 { self.last_snapshot = Instant::now(); self.snapshot(farm); }
        fx
    }
    /// An assignment that could not be delivered goes back to the queue at the next cycle.
    pub fn revert(&mut self, job: String, a: i64, b: i64) { self.reverts.push((job, a, b)); }
    /// The list of jobs (2.6), in the queue's order, archived ones left out.
    fn summaries(&self) -> String {
        // every job's counts in one pass over the chunks
        let mut counts: BTreeMap<String, [i64; 7]> = BTreeMap::new();   // completed, failed, assigned, pending, stopped, first assigned, last completed
        if let Ok(mut st) = self.db.prepare_cached("SELECT job_id, state, COUNT(*), COALESCE(MIN(assigned_at_ms), 0), COALESCE(MAX(completed_at_ms), 0) FROM chunks GROUP BY job_id, state") { if let Ok(rows) = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, i64>(4)?))) {
            for (job, state, n, first, last) in rows.flatten() { let c = counts.entry(job).or_insert([0; 7]); if let Some(i) = ["completed", "failed", "assigned", "pending", "stopped"].iter().position(|x| *x == state) { c[i] = n; }
                if first > 0 && (c[5] == 0 || first < c[5]) { c[5] = first; } c[6] = c[6].max(last); } } }
        let mut out = String::from("["); let mut first_job = true;
        if let Ok(mut st) = self.db.prepare_cached(&format!("SELECT job_id, manifest_json, current_state, priority, submitted_at_ms FROM jobs WHERE current_state != 'archived' {}", Self::ORDER)) { if let Ok(rows) = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, i64>(3)?, r.get::<_, i64>(4)?))) {
            for (id, text, state, priority, at) in rows.flatten() { let m: Value = serde_json::from_str(&text).unwrap_or(Value::Null); let c = counts.get(&id).copied().unwrap_or([0; 7]);
                let mut v = json!({ "job_id": id, "template_id": m["template_id"].as_str().unwrap_or(""), "current_state": state, "priority": priority, "submitted_at_ms": at, "submitted_by": m["submitted_by"].as_str().unwrap_or(""),
                    "frame_start": m["frame_start"].as_i64().unwrap_or(1), "frame_end": m["frame_end"].as_i64().unwrap_or(250), "chunk_size": m["chunk_size"].as_i64().unwrap_or(1), "total_chunks": c[0] + c[1] + c[2] + c[3], "completed_chunks": c[0], "failed_chunks": c[1],
                    "rendering_chunks": c[2], "pending_chunks": c[3], "stopped_chunks": c[4], "first_assigned_ms": c[5], "last_completed_ms": c[6] });
                if let Some(d) = m["output_dir"].as_str() { v["output_dir"] = json!(d); }
                if let Some(o) = m["flags"].as_array().and_then(|f| f.iter().find(|f| f["is_output"] == json!(true))).and_then(|f| f["value"].as_str()).filter(|o| !o.is_empty()) { v["output_stem"] = json!(o); }
                if !first_job { out.push(','); } first_job = false; out.push_str(&v.to_string()); } } }
        out.push(']'); out
    }
    /// The snapshot (3.3): a copy made beside the farm's folder, then copied over the shared file.
    fn snapshot(&self, farm: &Path) {
        let Some(root) = farm.parent() else { return }; let tmp = root.join("snapshot_tmp.db"); let _ = std::fs::remove_file(&tmp); let _ = std::fs::create_dir_all(farm.join("state"));
        match self.db.execute("VACUUM INTO ?", [tmp.to_string_lossy()]) { Ok(_) => { if let Err(e) = std::fs::copy(&tmp, farm.join("state").join("snapshot.db")) { eprintln!("the snapshot could not be put on the share: {e}"); } let _ = std::fs::remove_file(&tmp); } Err(e) => eprintln!("the snapshot could not be made: {e}") }
        let _ = &self.data;
    }
}
