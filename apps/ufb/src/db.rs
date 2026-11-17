//! The database of the project side (canon rows 454, 459, 482): `ufb_v5.db`, with the tables, the columns and the
//! pragmas of UFB 1.4.2 (`core/src/db.rs`, read into `docs/phase5/step0-projects-metadata.md`, section 2) - lifted, so
//! that a database of an installed 1.4.x and this one stay the same thing. So far it is USED for the subscriptions only
//! (the jobs in the sidebar); the metadata, the columns and the volumes come with their slices.
//!
//! TEST DATA ONLY (row 459): the file lies in this development build's own settings folder (`UFB_SETTINGS_DIR` moves
//! it), never in the folder an installed UFB keeps its own in. Nothing of a real set-up is read.
//!
//! Paths are stored in 1.4.2's identity form. Without a volume registry (it comes with the mesh's slice) every path is
//! `native:{mac|win}/{path}` - which 1.4.2 never shares with other machines, as it must be for test data.
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// 1.4.2's tables as `run_migrations` leaves them on a new file: the first statements, then the columns it adds.
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS subscriptions (id INTEGER PRIMARY KEY AUTOINCREMENT, job_path TEXT NOT NULL UNIQUE, job_name TEXT NOT NULL, is_active INTEGER NOT NULL DEFAULT 1,
    subscribed_time INTEGER NOT NULL, last_sync_time INTEGER, sync_status TEXT NOT NULL DEFAULT 'Pending', shot_count INTEGER NOT NULL DEFAULT 0);
CREATE INDEX IF NOT EXISTS idx_subs_active ON subscriptions(is_active);
CREATE TABLE IF NOT EXISTS item_metadata (item_path TEXT NOT NULL, job_path TEXT NOT NULL, folder_name TEXT NOT NULL, metadata_json TEXT NOT NULL DEFAULT '{}', is_tracked INTEGER NOT NULL DEFAULT 0,
    created_time INTEGER, modified_time INTEGER, device_id TEXT, PRIMARY KEY (item_path));
CREATE INDEX IF NOT EXISTS idx_item_metadata_job ON item_metadata(job_path);
CREATE INDEX IF NOT EXISTS idx_item_metadata_tracked ON item_metadata(is_tracked);
CREATE INDEX IF NOT EXISTS idx_item_metadata_job_tracked ON item_metadata(job_path, is_tracked);
CREATE TABLE IF NOT EXISTS column_definitions (id INTEGER PRIMARY KEY AUTOINCREMENT, job_path TEXT NOT NULL, folder_name TEXT NOT NULL, column_name TEXT NOT NULL, column_type TEXT NOT NULL DEFAULT 'text',
    column_order INTEGER NOT NULL DEFAULT 0, column_width REAL NOT NULL DEFAULT 120.0, is_visible INTEGER NOT NULL DEFAULT 1, default_value TEXT, column_uuid TEXT);
CREATE INDEX IF NOT EXISTS idx_column_defs_job_folder ON column_definitions(job_path, folder_name);
CREATE TABLE IF NOT EXISTS column_options (id INTEGER PRIMARY KEY AUTOINCREMENT, column_id INTEGER NOT NULL, option_name TEXT NOT NULL, option_color TEXT,
    FOREIGN KEY (column_id) REFERENCES column_definitions(id) ON DELETE CASCADE);
CREATE TABLE IF NOT EXISTS column_layout_personal (id INTEGER PRIMARY KEY AUTOINCREMENT, job_path TEXT NOT NULL, folder_name TEXT NOT NULL, column_name TEXT NOT NULL, column_order INTEGER, column_width REAL,
    is_visible INTEGER, modified_time INTEGER NOT NULL DEFAULT 0, UNIQUE(job_path, folder_name, column_name));
CREATE INDEX IF NOT EXISTS idx_column_layout_personal_lookup ON column_layout_personal(job_path, folder_name);
CREATE TABLE IF NOT EXISTS bookmarks (id INTEGER PRIMARY KEY AUTOINCREMENT, path TEXT NOT NULL UNIQUE, display_name TEXT NOT NULL, created_time INTEGER NOT NULL, is_project_folder INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS column_presets (id INTEGER PRIMARY KEY AUTOINCREMENT, preset_name TEXT NOT NULL UNIQUE, columns_json TEXT NOT NULL, created_time INTEGER NOT NULL, modified_time INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS volumes (uuid TEXT PRIMARY KEY, display_name TEXT NOT NULL, kind TEXT NOT NULL DEFAULT 'smb', connection_hints TEXT NOT NULL DEFAULT '{}', modified_time INTEGER NOT NULL DEFAULT 0, deleted_at INTEGER);
CREATE TABLE IF NOT EXISTS volume_bindings (uuid TEXT PRIMARY KEY, local_mount_root TEXT NOT NULL, bound_time INTEGER NOT NULL, auto_discovered INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS migration_orphans (id INTEGER PRIMARY KEY AUTOINCREMENT, source_table TEXT NOT NULL, original_path TEXT NOT NULL, detail TEXT NOT NULL DEFAULT '{}', quarantined_at INTEGER NOT NULL);
";
/// Ours, beside 1.4.2's tables (it does not know them and does not mind them): what was changed here and has not been
/// accepted by the mesh yet (row 441: the database is the queue - nothing is dropped, and a quit loses nothing).
/// `kind`: `meta` (an item's row, by its identity), `col` (a column, by its uuid); `payload`: a message to send as it
/// is (an item that is no longer here under that path), else what the row holds when it is sent.
const OURS: &str = "CREATE TABLE IF NOT EXISTS mesh_pending (id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL, key TEXT NOT NULL, payload TEXT, queued_time INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS idx_mesh_pending_key ON mesh_pending(kind, key);
CREATE TABLE IF NOT EXISTS mesh_base (item_path TEXT PRIMARY KEY, metadata_json TEXT NOT NULL, is_tracked INTEGER NOT NULL, modified_time INTEGER NOT NULL DEFAULT 0);";
/// `mesh_base`, also ours: for every item, the version this machine last had IN COMMON with the others (row 440) - what
/// it sent and was taken, what it took. Against it a row that arrives shows which cells its sender changed.
/// The columns 1.4.2 adds after the tables (each only where it is missing).
const ADDED: [(&str, &str, &str); 13] = [("item_metadata", "device_id", "TEXT"), ("column_definitions", "modified_time", "INTEGER NOT NULL DEFAULT 0"), ("column_options", "modified_time", "INTEGER NOT NULL DEFAULT 0"),
    ("subscriptions", "modified_time", "INTEGER NOT NULL DEFAULT 0"), ("item_metadata", "deleted_at", "INTEGER"), ("column_definitions", "deleted_at", "INTEGER"), ("column_options", "deleted_at", "INTEGER"),
    ("column_presets", "deleted_at", "INTEGER"), ("subscriptions", "deleted_at", "INTEGER"), ("column_definitions", "template_hash", "TEXT"), ("column_definitions", "trashed_at", "INTEGER"),
    ("column_definitions", "column_uuid", "TEXT"), ("column_definitions", "name_conflict_marker", "TEXT")];
/// 1.4.2's `PRAGMA user_version` once its migrations have run.
const USER_VERSION: i64 = 4;

pub struct Db { conn: Connection }
/// A job in the sidebar: what it is called, where it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Sub { pub name: String, pub path: PathBuf }

fn now_ms() -> i64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0) }
const OS: &str = if cfg!(windows) { "win" } else { "mac" };

// ------------------------------------------------------------------ shared paths (row 494; 1.4.2's `identity.rs`, `volumes.rs`)
/// A share as two machines know it: where it is on Windows and where on macOS. Its identity is derived from that
/// pair alone, so every machine with the same pair has the same one (1.4.2's derivation, to the byte).
#[derive(Clone, Debug, PartialEq)]
pub struct Mapping { pub win: String, pub mac: String, pub uuid: String, /// where the share REALLY is on this machine now (row 498): the system's answer - a Mac may have it as `Mount-2`, Windows under any letter; none: not looked up, or not there
    pub here: Option<String> }
impl Mapping {
    pub fn new(win: &str, mac: &str) -> Mapping { Mapping { win: win.trim().to_string(), mac: mac.trim().to_string(), uuid: volume_uuid(win, mac), here: None } }
    pub fn at(mut self, here: Option<String>) -> Mapping { self.here = here.filter(|h| !h.trim().is_empty()); self }
    /// Where the share is on THIS machine: where the system says it is, else this system's side of the pair.
    pub fn local(&self) -> &str { match &self.here { Some(h) => h, None => if cfg!(windows) { &self.win } else { &self.mac } } }
}
/// The identity of a share from its two sides: the separators and a slash at the end do not count, nor the letters' case.
pub fn volume_uuid(win: &str, mac: &str) -> String {
    const NS: uuid::Uuid = uuid::Uuid::from_u128(0x9e1d_4f60_8b2a_4c17_a3e5_6f0c_2d7b_41a8);
    let key = |s: &str| s.trim().replace('\\', "/").trim_end_matches('/').to_lowercase();
    uuid::Uuid::new_v5(&NS, format!("{}|{}", key(win), key(mac)).as_bytes()).to_string()
}
static MAPPINGS: std::sync::RwLock<Vec<Mapping>> = std::sync::RwLock::new(Vec::new());
/// The shares as they are known now: the saved ones (`shares.rs`) with where each really is, and those the build was
/// started with (`--map <windows side>=<macOS side>`). Set again whenever a share comes, goes or lands elsewhere.
pub fn set_mappings(m: Vec<Mapping>) { *MAPPINGS.write().unwrap() = m; }
pub fn mappings() -> Vec<Mapping> { MAPPINGS.read().unwrap().clone() }
/// A path or a prefix as it is compared: slashes one way, a drive letter dropped, no slash at either end (1.4.2).
fn norm(s: &str) -> String { let t = s.replace('\\', "/"); let b = t.as_bytes(); let t = if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() { &t[2..] } else { &t[..] }; t.trim_start_matches('/').trim_end_matches('/').to_string() }
/// The path under `prefix`, with forward slashes and its own letters' case; none when it is not under it. Compared
/// without regard to separators, a drive letter or case, and only at the edge of a component.
fn rel_under(path: &str, prefix: &str) -> Option<String> {
    let (p, pre) = (norm(path), norm(prefix)); if pre.is_empty() { return None } let (pk, prek) = (p.to_lowercase(), pre.to_lowercase());
    if pk == prek { return Some(String::new()) }
    let with = format!("{prek}/"); pk.starts_with(&with).then(|| p[with.len()..].trim_start_matches('/').to_string())
}
/// A path of this machine in 1.4.2's storage form: `vol:{uuid}/{rel}` under a mapped share (shared with the other
/// machines), `native:{mac|win}/{path}` otherwise (this machine's own; never shared).
pub fn identity_in(p: &Path, maps: &[Mapping]) -> String {
    let text = p.to_string_lossy();
    // where a share REALLY is on this machine comes first (the system's answer: `Mount-2`, a drive letter)
    let mut real: Vec<(&str, &str)> = maps.iter().filter_map(|m| m.here.as_deref().map(|h| (m.uuid.as_str(), h))).collect(); real.sort_by_key(|x| std::cmp::Reverse(norm(x.1).len()));
    for (uuid, prefix) in real { if let Some(rel) = rel_under(&text, prefix) { return if rel.is_empty() { format!("vol:{uuid}") } else { format!("vol:{uuid}/{rel}") } } }
    // a path that arrives as TEXT with a numbered landing in it (`/Volumes/Mount-2/...`: pasted, in a link, from an
    // older version, from another machine) is its share's all the same (row 445)
    let text = landing(&text, maps);
    // the longest prefix first: a share inside another wins over it; either side of a mapping counts (1.4.2)
    let mut pre: Vec<(&str, &str)> = maps.iter().flat_map(|m| [(m.uuid.as_str(), m.win.as_str()), (m.uuid.as_str(), m.mac.as_str())]).filter(|x| !norm(x.1).is_empty()).collect(); pre.sort_by_key(|x| std::cmp::Reverse(norm(x.1).len()));
    for (uuid, prefix) in pre { if let Some(rel) = rel_under(&text, prefix) { return if rel.is_empty() { format!("vol:{uuid}") } else { format!("vol:{uuid}/{rel}") } } }
    format!("native:{OS}/{text}")
}
/// The path of this machine a stored identity names. None: another system's own path, or a share that is not mapped here.
pub fn native_in(id: &str, maps: &[Mapping]) -> Option<PathBuf> {
    if let Some(rest) = id.strip_prefix("vol:") { let (uuid, rel) = rest.split_once('/').unwrap_or((rest, "")); let root = maps.iter().find(|m| m.uuid == uuid)?.local(); if root.trim().is_empty() { return None }
        let joined = if rel.is_empty() { root.to_string() } else { format!("{}/{rel}", root.trim_end_matches(['/', '\\'])) };
        return Some(PathBuf::from(if cfg!(windows) { joined.replace('/', "\\") } else { joined.replace('\\', "/") })) }
    let (os, path) = id.strip_prefix("native:")?.split_once('/')?; (os == OS && !path.is_empty()).then(|| PathBuf::from(path))
}
/// `/Volumes/<name>-<digits>/...` as `/Volumes/<name>/...` when `<name>` is a share's macOS side directly under
/// `/Volumes` and NO share's own side is `<name>-<digits>` (then each is itself alone). Only digits count: `jobs-extra`
/// stays another volume. Anything else is given back as it is.
pub fn landing(path: &str, maps: &[Mapping]) -> String {
    let t = path.replace('\\', "/"); let Some(rest) = t.strip_prefix("/Volumes/").or_else(|| t.strip_prefix("/volumes/")) else { return path.to_string() };
    let (first, tail) = rest.split_once('/').map(|(a, b)| (a, Some(b))).unwrap_or((rest, None));
    let Some((name, digits)) = first.rsplit_once('-') else { return path.to_string() }; if name.is_empty() || digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) { return path.to_string() }
    let side = |m: &Mapping, leaf: &str| norm(&m.mac).to_lowercase() == format!("volumes/{}", leaf.to_lowercase());
    if maps.iter().any(|m| side(m, first)) || !maps.iter().any(|m| side(m, name)) { return path.to_string() }
    match tail { Some(t) => format!("/Volumes/{name}/{t}"), None => format!("/Volumes/{name}") }
}
pub fn to_identity(p: &Path) -> String { identity_in(p, &mappings()) }
pub fn to_native(id: &str) -> Option<PathBuf> { native_in(id, &mappings()) }
/// A path as text from anywhere - another machine's spelling, a numbered landing, either system's separators - as
/// THIS machine's path to the same thing; none when it is under no share this machine knows.
pub fn translate(text: &str) -> Option<PathBuf> { let maps = mappings(); let id = identity_in(Path::new(text), &maps); if id.starts_with("vol:") { native_in(&id, &maps) } else { None } }
/// A path of this machine as the OTHERS should read it: under a share, the share's own side for this system (never
/// the landing's name, never a drive letter the others do not have); else as it is.
pub fn canonical(p: &Path) -> String { let maps = mappings(); let id = identity_in(p, &maps); let Some(rest) = id.strip_prefix("vol:") else { return p.to_string_lossy().into_owned() };
    let (uuid, rel) = rest.split_once('/').unwrap_or((rest, "")); let Some(m) = maps.iter().find(|m| m.uuid == uuid) else { return p.to_string_lossy().into_owned() };
    let root = if cfg!(windows) { &m.win } else { &m.mac }; let joined = if rel.is_empty() { root.clone() } else { format!("{}/{rel}", root.trim_end_matches(['/', '\\'])) };
    if cfg!(windows) { joined.replace('/', "\\") } else { joined.replace('\\', "/") } }
/// What separates the folders in a stored identity: a shared one is written with forward slashes on every system.
fn sep_of(id: &str) -> &'static str { if id.starts_with("vol:") { "/" } else { std::path::MAIN_SEPARATOR_STR } }

impl Db {
    /// Open (or make) the database in a folder, as 1.4.2 opens its own: write-ahead log, foreign keys, a wait on a busy file.
    pub fn open(dir: &Path) -> Result<Db, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let conn = Connection::open(dir.join("ufb_v5.db")).map_err(|e| e.to_string())?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000; PRAGMA synchronous=NORMAL;").map_err(|e| e.to_string())?;
        conn.execute_batch(SCHEMA).map_err(|e| e.to_string())?; conn.execute_batch(OURS).map_err(|e| e.to_string())?;
        for (table, column, kind) in ADDED {
            let has: bool = conn.prepare(&format!("SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1")).and_then(|mut s| s.exists([column])).map_err(|e| e.to_string())?;
            if !has { conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {column} {kind}"), []).map_err(|e| e.to_string())?; }
        }
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(|e| e.to_string())?;
        if v < USER_VERSION { conn.execute_batch(&format!("PRAGMA user_version = {USER_VERSION}")).map_err(|e| e.to_string())?; }
        let db = Db { conn }; db.classify(&mappings()).map_err(|e| e.to_string())?; Ok(db)
    }
    /// Rows stored as this machine's own that lie under a mapped share become the share's (what 1.4.2's migration did
    /// once, done here at every start: a share mapped later takes its rows along). A shared row that is already there
    /// stays; the machine's own copy of it goes. Returns how many rows changed.
    pub fn classify(&self, maps: &[Mapping]) -> rusqlite::Result<usize> {
        let mut n = 0;
        for (table, cols) in [("subscriptions", &["job_path"][..]), ("item_metadata", &["item_path", "job_path"]), ("column_definitions", &["job_path"]), ("column_layout_personal", &["job_path"])] { for col in cols {
            let rows: Vec<String> = { let mut st = self.conn.prepare(&format!("SELECT DISTINCT {col} FROM {table} WHERE {col} LIKE 'native:{OS}/%'"))?; let v = st.query_map([], |r| r.get::<_, String>(0))?.flatten().collect(); v };
            for old in rows { let Some(path) = old.strip_prefix(&format!("native:{OS}/")) else { continue }; let new = identity_in(Path::new(path), maps); if new == old { continue }
                n += self.conn.execute(&format!("UPDATE OR IGNORE {table} SET {col} = ?2 WHERE {col} = ?1"), rusqlite::params![old, new])?;
                // (a row that could not move - the shared one is there already - is this machine's stale copy)
                if *col == "item_path" || table == "subscriptions" { self.conn.execute(&format!("DELETE FROM {table} WHERE {col} = ?1"), [&old])?; } }
        } }
        Ok(n)
    }
    /// A job is shown in the sidebar (1.4.2's statement: an unsubscribed one comes back, its name is taken anew). An
    /// empty name becomes the folder's.
    pub fn subscribe(&self, job: &Path, name: &str) -> Result<(), String> {
        let name = if name.trim().is_empty() { job.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() } else { name.trim().to_string() };
        self.conn.execute("INSERT INTO subscriptions (job_path, job_name, subscribed_time, modified_time, deleted_at) VALUES (?1, ?2, ?3, ?3, NULL)
             ON CONFLICT(job_path) DO UPDATE SET job_name = excluded.job_name, modified_time = excluded.modified_time, deleted_at = NULL", rusqlite::params![to_identity(job), name, now_ms()]).map(|_| ()).map_err(|e| e.to_string())
    }
    /// The job leaves the sidebar; nothing else of it is touched (its metadata stays).
    pub fn unsubscribe(&self, job: &Path) -> Result<(), String> {
        self.conn.execute("UPDATE subscriptions SET modified_time = ?1, deleted_at = ?1 WHERE job_path = ?2", rusqlite::params![now_ms(), to_identity(job)]).map(|_| ()).map_err(|e| e.to_string())
    }
    /// The jobs in the sidebar, by name.
    pub fn subscriptions(&self) -> Vec<Sub> {
        let Ok(mut st) = self.conn.prepare("SELECT job_name, job_path FROM subscriptions WHERE deleted_at IS NULL ORDER BY job_name") else { return vec![] };
        st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map(|rows| rows.flatten().filter_map(|(name, id)| to_native(&id).map(|path| Sub { name, path })).collect()).unwrap_or_default()
    }
}

// ------------------------------------------------------------------ the metadata (row 489; the spec's section 4)
/// A column of a (job, section): 1.4.2's `column_definitions` row with its options, and this user's own order, width
/// and visibility laid over the shared ones (`column_layout_personal`, by the column's name; never shared).
#[derive(Clone, Debug, PartialEq)]
pub struct ColDef { pub id: i64, pub name: String, /// `text` `number` `checkbox` `dropdown` `priority` `date` `notes` `links` (lower case; `note` reads as `notes`, an unknown one as text)
    pub kind: String, pub order: i64, pub width: f64, pub visible: bool, /// the key of the column's values in an item's blob
    pub uuid: String, /// (name, colour as `#hex` or none), in the order they were made
    pub options: Vec<(String, Option<String>)> }
/// What an item carries: the values by the columns' uuid, the star, when it was last changed (ms).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemMeta { pub values: serde_json::Map<String, serde_json::Value>, pub tracked: bool, pub modified: i64 }

/// The eight types the editor offers (1.4.2's list).
pub const TYPES: [&str; 8] = ["text", "dropdown", "date", "priority", "number", "checkbox", "links", "notes"];
/// A column's type as the editors take it: lower case, `note` as `notes`, an unknown one as text.
pub fn kind_of(t: &str) -> &'static str { let t = t.to_lowercase(); TYPES.into_iter().find(|k| *k == t).unwrap_or(if t == "note" { "notes" } else { "text" }) }
/// A column's identity when it is made (1.4.2's derivation, to the byte: two machines that make "the same" column
/// land on one identity). Stored then, and never derived again - a rename keeps it.
pub fn column_uuid(job_identity: &str, folder: &str, name: &str, kind: &str) -> String {
    const NS: uuid::Uuid = uuid::Uuid::from_u128(0x4b2c_9a17_6f3d_4e88_b1a0_5c7e_9d24_38f1);
    uuid::Uuid::new_v5(&NS, format!("{job_identity}\x1f{folder}\x1f{}\x1f{}", name.to_lowercase(), kind.to_lowercase()).as_bytes()).to_string()
}

impl Db {
    /// The columns of a (job, section) that are in use (neither in the trash nor deleted), in this user's order.
    /// (1.4.2 shows only "promoted" columns in its grids; a local job's are never promoted, and are shown - row 489.)
    pub fn columns(&self, job: &Path, folder: &str) -> Vec<ColDef> {
        let jid = to_identity(job);
        let Ok(mut st) = self.conn.prepare("SELECT d.id, d.column_name, d.column_type, COALESCE(p.column_order, d.column_order), COALESCE(p.column_width, d.column_width), COALESCE(p.is_visible, d.is_visible), COALESCE(d.column_uuid, '')
             FROM column_definitions d LEFT JOIN column_layout_personal p ON p.job_path = d.job_path AND p.folder_name = d.folder_name AND p.column_name = d.column_name
             WHERE d.job_path = ?1 AND d.folder_name = ?2 AND d.deleted_at IS NULL AND d.trashed_at IS NULL ORDER BY 4, d.id") else { return vec![] };
        let mut cols: Vec<ColDef> = st.query_map(rusqlite::params![jid, folder], |r| Ok(ColDef { id: r.get(0)?, name: r.get(1)?, kind: r.get(2)?, order: r.get(3)?, width: r.get(4)?, visible: r.get::<_, i64>(5)? != 0, uuid: r.get(6)?, options: vec![] })).map(|rows| rows.flatten().collect()).unwrap_or_default();
        for c in cols.iter_mut() {
            if c.uuid.is_empty() { c.uuid = column_uuid(&jid, folder, &c.name, &c.kind); }
            if let Ok(mut st) = self.conn.prepare("SELECT option_name, option_color FROM column_options WHERE column_id = ?1 AND deleted_at IS NULL ORDER BY id") { c.options = st.query_map([c.id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?.filter(|c| !c.is_empty())))).map(|rows| rows.flatten().collect()).unwrap_or_default(); }
        }
        cols
    }
    /// The columns in the trash of a (job, section): (id, name).
    pub fn trashed_columns(&self, job: &Path, folder: &str) -> Vec<(i64, String)> {
        let Ok(mut st) = self.conn.prepare("SELECT id, column_name FROM column_definitions WHERE job_path = ?1 AND folder_name = ?2 AND deleted_at IS NULL AND trashed_at IS NOT NULL ORDER BY trashed_at DESC") else { return vec![] };
        st.query_map(rusqlite::params![to_identity(job), folder], |r| Ok((r.get(0)?, r.get(1)?))).map(|rows| rows.flatten().collect()).unwrap_or_default()
    }
    fn set_options(&self, id: i64, options: &[(String, Option<String>)], now: i64) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM column_options WHERE column_id = ?1", [id])?;
        for (name, color) in options { self.conn.execute("INSERT INTO column_options (column_id, option_name, option_color, modified_time, deleted_at) VALUES (?1, ?2, ?3, ?4, NULL)", rusqlite::params![id, name, color, now])?; }
        Ok(())
    }
    /// A column is made in a (job, section), after the others. A name in use there (letters' case apart) is refused.
    pub fn add_column(&self, job: &Path, folder: &str, name: &str, kind: &str, options: &[(String, Option<String>)]) -> Result<i64, String> {
        let name = name.trim(); if name.is_empty() { return Err("The column needs a name.".into()) }
        let (jid, now) = (to_identity(job), now_ms());
        let taken: bool = self.conn.prepare("SELECT 1 FROM column_definitions WHERE job_path = ?1 AND folder_name = ?2 AND lower(column_name) = lower(?3) AND deleted_at IS NULL AND trashed_at IS NULL").and_then(|mut s| s.exists(rusqlite::params![jid, folder, name])).map_err(|e| e.to_string())?;
        if taken { return Err(format!("A column named \u{201c}{name}\u{201d} is already there.")) }
        let order: i64 = self.conn.query_row("SELECT COALESCE(MAX(column_order), -1) + 1 FROM column_definitions WHERE job_path = ?1 AND folder_name = ?2 AND deleted_at IS NULL", rusqlite::params![jid, folder], |r| r.get(0)).map_err(|e| e.to_string())?;
        self.conn.execute("INSERT INTO column_definitions (job_path, folder_name, column_name, column_type, column_order, column_width, is_visible, default_value, modified_time, deleted_at, column_uuid) VALUES (?1, ?2, ?3, ?4, ?5, 120.0, 1, NULL, ?6, NULL, ?7)",
            rusqlite::params![jid, folder, name, kind, order, now, column_uuid(&jid, folder, name, kind)]).map_err(|e| e.to_string())?;
        let id = self.conn.last_insert_rowid(); self.set_options(id, options, now).map_err(|e| e.to_string())?; self.note_col(id); Ok(id)
    }
    /// A column's name, type and options are changed. Its values stay (they hang on its uuid, which is kept).
    pub fn update_column(&self, id: i64, name: &str, kind: &str, options: &[(String, Option<String>)]) -> Result<(), String> {
        let name = name.trim(); if name.is_empty() { return Err("The column needs a name.".into()) } let now = now_ms();
        let taken: bool = self.conn.prepare("SELECT 1 FROM column_definitions o JOIN column_definitions me ON me.id = ?1 WHERE o.id != me.id AND o.job_path = me.job_path AND o.folder_name = me.folder_name AND lower(o.column_name) = lower(?2) AND o.deleted_at IS NULL AND o.trashed_at IS NULL").and_then(|mut s| s.exists(rusqlite::params![id, name])).map_err(|e| e.to_string())?;
        if taken { return Err(format!("A column named \u{201c}{name}\u{201d} is already there.")) }
        // (this user's own layout hangs on the NAME: it goes with a rename)
        self.conn.execute("UPDATE OR IGNORE column_layout_personal SET column_name = ?2 WHERE (job_path, folder_name, column_name) IN (SELECT job_path, folder_name, column_name FROM column_definitions WHERE id = ?1)", rusqlite::params![id, name]).map_err(|e| e.to_string())?;
        self.conn.execute("UPDATE column_definitions SET column_name = ?1, column_type = ?2, modified_time = ?3, deleted_at = NULL WHERE id = ?4", rusqlite::params![name, kind, now, id]).map_err(|e| e.to_string())?;
        self.set_options(id, options, now).map_err(|e| e.to_string())?; self.note_col(id); Ok(())
    }
    /// An option is added to a column, after its others (made from a cell: a text that was no option yet).
    pub fn add_option(&self, id: i64, name: &str, color: &str) -> Result<(), String> {
        let now = now_ms(); self.conn.execute("INSERT INTO column_options (column_id, option_name, option_color, modified_time, deleted_at) VALUES (?1, ?2, ?3, ?4, NULL)", rusqlite::params![id, name, color, now]).map_err(|e| e.to_string())?;
        self.conn.execute("UPDATE column_definitions SET modified_time = ?1 WHERE id = ?2", rusqlite::params![now, id]).map_err(|e| e.to_string())?; self.note_col(id); Ok(())
    }
    /// Into the column trash, or out of it (the values stay; a restore under a name now in use is refused).
    pub fn trash_column(&self, id: i64, on: bool) -> Result<(), String> {
        if !on { let taken: bool = self.conn.prepare("SELECT 1 FROM column_definitions o JOIN column_definitions me ON me.id = ?1 WHERE o.id != me.id AND o.job_path = me.job_path AND o.folder_name = me.folder_name AND lower(o.column_name) = lower(me.column_name) AND o.deleted_at IS NULL AND o.trashed_at IS NULL").and_then(|mut s| s.exists([id])).map_err(|e| e.to_string())?;
            if taken { return Err("A column of that name is in use: rename it first.".into()) } }
        let now = now_ms(); self.conn.execute("UPDATE column_definitions SET trashed_at = ?1, modified_time = ?2 WHERE id = ?3", rusqlite::params![on.then_some(now), now, id]).map_err(|e| e.to_string())?; self.note_col(id); Ok(())
    }
    /// This user's own order, width or visibility of a column (none: left as it is). Never shared.
    pub fn set_layout(&self, job: &Path, folder: &str, name: &str, order: Option<i64>, width: Option<f64>, visible: Option<bool>) -> Result<(), String> {
        self.conn.execute("INSERT INTO column_layout_personal (job_path, folder_name, column_name, column_order, column_width, is_visible, modified_time) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(job_path, folder_name, column_name) DO UPDATE SET column_order = COALESCE(excluded.column_order, column_layout_personal.column_order), column_width = COALESCE(excluded.column_width, column_layout_personal.column_width),
             is_visible = COALESCE(excluded.is_visible, column_layout_personal.is_visible), modified_time = excluded.modified_time", rusqlite::params![to_identity(job), folder, name, order, width, visible.map(|v| v as i64), now_ms()]).map(|_| ()).map_err(|e| e.to_string())
    }
    /// What the items of a (job, section) carry, by the item's path.
    pub fn folder_meta(&self, job: &Path, folder: &str) -> std::collections::HashMap<PathBuf, ItemMeta> {
        let Ok(mut st) = self.conn.prepare("SELECT item_path, metadata_json, is_tracked, COALESCE(modified_time, 0) FROM item_metadata WHERE job_path = ?1 AND folder_name = ?2 AND deleted_at IS NULL") else { return Default::default() };
        st.query_map(rusqlite::params![to_identity(job), folder], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?))).map(|rows| rows.flatten().filter_map(|(id, json, tracked, modified)| {
            let values = match serde_json::from_str::<serde_json::Value>(&json) { Ok(serde_json::Value::Object(m)) => m, _ => Default::default() };
            Some((to_native(&id)?, ItemMeta { values, tracked: tracked != 0, modified })) }).collect()).unwrap_or_default()
    }
    /// A folder was renamed or moved by the application (rows 439, 493): every row that hangs on its path, or on a path
    /// under it, is rewritten - an item's metadata, the job a row belongs to, a job's columns and this user's layout of
    /// them, a subscription - in ONE step; the items' rows are stamped as changed (they are ordinary edits to whoever
    /// reads the database). A top-level folder of a job renamed in place takes the section's name in those rows along.
    /// What already lay at the new path is replaced. Returns how many rows moved.
    pub fn move_paths(&self, from: &Path, to: &Path) -> Result<usize, String> {
        let (old, new, now) = (to_identity(from), to_identity(to), now_ms()); let sep = sep_of(&old).to_string(); if old == new { return Ok(0) }
        // (a row is the folder's own, or lies under it: the text after the old path is kept)
        let under = "({c} = ?1 OR substr({c}, 1, length(?1) + 1) = ?1 || ?3)"; let moved = "?2 || substr({c}, length(?1) + 1)";
        let run = || -> rusqlite::Result<usize> {
            let tx = self.conn.unchecked_transaction()?; let mut n = 0;
            let w = |c: &str| under.replace("{c}", c); let m = |c: &str| moved.replace("{c}", c);
            // what lies where the rows go makes way (a row left behind by an earlier rename, a tombstone)
            tx.execute(&format!("DELETE FROM item_metadata WHERE item_path IN (SELECT {} FROM item_metadata WHERE {})", m("item_path"), w("item_path")), rusqlite::params![old, new, sep])?;
            tx.execute(&format!("DELETE FROM subscriptions WHERE job_path IN (SELECT {} FROM subscriptions WHERE {})", m("job_path"), w("job_path")), rusqlite::params![old, new, sep])?;
            n += tx.execute(&format!("UPDATE item_metadata SET item_path = {}, modified_time = ?4 WHERE {}", m("item_path"), w("item_path")), rusqlite::params![old, new, sep, now])?;
            n += tx.execute(&format!("UPDATE item_metadata SET job_path = {} WHERE {}", m("job_path"), w("job_path")), rusqlite::params![old, new, sep])?;
            n += tx.execute(&format!("UPDATE column_definitions SET job_path = {}, modified_time = ?4 WHERE {}", m("job_path"), w("job_path")), rusqlite::params![old, new, sep, now])?;
            n += tx.execute(&format!("UPDATE OR REPLACE column_layout_personal SET job_path = {} WHERE {}", m("job_path"), w("job_path")), rusqlite::params![old, new, sep])?;
            // a subscription called by its folder's name is called by the new one
            if let (Some(a), Some(b)) = (from.file_name(), to.file_name()) { tx.execute("UPDATE subscriptions SET job_name = ?2 WHERE job_path = ?1 AND job_name = ?3", rusqlite::params![old, b.to_string_lossy(), a.to_string_lossy()])?; }
            n += tx.execute(&format!("UPDATE subscriptions SET job_path = {}, modified_time = ?4 WHERE {}", m("job_path"), w("job_path")), rusqlite::params![old, new, sep, now])?;
            // a section renamed in place: the rows of its job that name it
            if let (Some(dir), Some(a), Some(b)) = (from.parent().filter(|d| Some(*d) == to.parent()), from.file_name(), to.file_name()) { let (job, a, b) = (to_identity(dir), a.to_string_lossy().into_owned(), b.to_string_lossy().into_owned());
                for table in ["item_metadata", "column_definitions"] { n += tx.execute(&format!("UPDATE {table} SET folder_name = ?3 WHERE job_path = ?1 AND folder_name = ?2"), rusqlite::params![job, a, b])?; }
                n += tx.execute("UPDATE OR REPLACE column_layout_personal SET folder_name = ?3 WHERE job_path = ?1 AND folder_name = ?2", rusqlite::params![job, a, b])?; }
            tx.commit()?; Ok(n)
        };
        // the mesh is told: the tracked rows that were under the old path are untracked there (a 1.4.2 node keeps a row
        // for good; untracked, it leaves every tracker), and the rows and the columns under the new path are edits of theirs
        let gone: Vec<(String, String, String)> = self.conn.prepare(&format!("SELECT item_path, job_path, folder_name FROM item_metadata WHERE is_tracked = 1 AND deleted_at IS NULL AND {}", under.replace("{c}", "item_path"))).and_then(|mut st| Ok(st.query_map(rusqlite::params![old, new, sep], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.flatten().collect())).unwrap_or_default();
        let n = run().map_err(|e| e.to_string())?;
        for (item, job, folder) in gone { self.note("meta", &item, Some(serde_json::json!({ "job_path": job, "item_path": item, "metadata": "{}", "folder_name": folder, "is_tracked": false, "modified_time": now }).to_string())); }
        let w = under.replace("{c}", "item_path").replace("?1", "?2");
        let moved: Vec<String> = self.conn.prepare(&format!("SELECT item_path FROM item_metadata WHERE {w}")).and_then(|mut st| Ok(st.query_map(rusqlite::params![old, new, sep], |r| r.get(0))?.flatten().collect())).unwrap_or_default();
        for item in moved { self.note("meta", &item, None); }
        let w = under.replace("{c}", "job_path").replace("?1", "?2");
        let cols: Vec<i64> = self.conn.prepare(&format!("SELECT id FROM column_definitions WHERE {w}")).and_then(|mut st| Ok(st.query_map(rusqlite::params![old, new, sep], |r| r.get(0))?.flatten().collect())).unwrap_or_default();
        for id in cols { self.note_col(id); }
        Ok(n)
    }
    /// An item's row is pointed at where its folder went (Relink): its path; and its job and section when the new
    /// place is an item of a job (`<job>/<section>/<item>`) - else they are kept.
    pub fn relink(&self, from: &Path, to: &Path, home: Option<(&Path, &str)>) -> Result<(), String> {
        let now = now_ms(); self.conn.execute("DELETE FROM item_metadata WHERE item_path = ?1", [to_identity(to)]).map_err(|e| e.to_string())?;
        match home { Some((job, folder)) => self.conn.execute("UPDATE item_metadata SET item_path = ?2, job_path = ?3, folder_name = ?4, modified_time = ?5 WHERE item_path = ?1", rusqlite::params![to_identity(from), to_identity(to), to_identity(job), folder, now]),
            None => self.conn.execute("UPDATE item_metadata SET item_path = ?2, modified_time = ?3 WHERE item_path = ?1", rusqlite::params![to_identity(from), to_identity(to), now]) }.map_err(|e| e.to_string())?;
        self.note("meta", &to_identity(to), None); Ok(())
    }
    /// An item's row is taken away (1.4.2's statement: a tombstone).
    pub fn remove_item(&self, item: &Path) -> Result<(), String> {
        let (id, now) = (to_identity(item), now_ms());
        // (a row cannot be deleted across the mesh - 1.4.2 has no such message: the others are told it is untracked, which they all understand)
        if let Ok((job, folder)) = self.conn.query_row("SELECT job_path, folder_name FROM item_metadata WHERE item_path = ?1", [&id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))) { self.note("meta", &id, Some(serde_json::json!({ "job_path": job, "item_path": id, "metadata": "{}", "folder_name": folder, "is_tracked": false, "modified_time": now }).to_string())); }
        self.conn.execute("UPDATE item_metadata SET modified_time = ?1, deleted_at = ?1 WHERE item_path = ?2", rusqlite::params![now, id]).map(|_| ()).map_err(|e| e.to_string())
    }
    /// Every tracked item: (the item, its job, its section, what it carries). Of every job - the caller keeps its own
    /// (1.4.2's statement returns the tracked rows of jobs one is not subscribed to as well).
    pub fn tracked(&self) -> Vec<(PathBuf, PathBuf, String, ItemMeta)> {
        let Ok(mut st) = self.conn.prepare("SELECT item_path, job_path, folder_name, metadata_json, COALESCE(modified_time, 0) FROM item_metadata WHERE is_tracked = 1 AND deleted_at IS NULL") else { return vec![] };
        st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, i64>(4)?))).map(|rows| rows.flatten().filter_map(|(item, job, folder, json, modified)| {
            let values = match serde_json::from_str::<serde_json::Value>(&json) { Ok(serde_json::Value::Object(m)) => m, _ => Default::default() };
            Some((to_native(&item)?, to_native(&job)?, folder, ItemMeta { values, tracked: true, modified })) }).collect()).unwrap_or_default()
    }
    /// An item's row is written (1.4.2's statement: the whole blob, the star, the time; a tombstone is lifted).
    fn put_item(&self, job: &Path, item: &Path, folder: &str, values: &serde_json::Map<String, serde_json::Value>, tracked: bool) -> Result<(), String> {
        self.conn.execute("INSERT INTO item_metadata (item_path, job_path, folder_name, metadata_json, is_tracked, modified_time, deleted_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)
             ON CONFLICT(item_path) DO UPDATE SET metadata_json = excluded.metadata_json, is_tracked = excluded.is_tracked, modified_time = excluded.modified_time, deleted_at = NULL",
            rusqlite::params![to_identity(item), to_identity(job), folder, serde_json::Value::Object(values.clone()).to_string(), tracked as i64, now_ms()]).map(|_| ()).map_err(|e| e.to_string())?;
        self.note("meta", &to_identity(item), None); Ok(())
    }
    fn item(&self, item: &Path) -> ItemMeta {
        self.conn.query_row("SELECT metadata_json, is_tracked, COALESCE(modified_time, 0) FROM item_metadata WHERE item_path = ?1 AND deleted_at IS NULL", [to_identity(item)], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?))).ok()
            .map(|(json, tracked, modified)| ItemMeta { values: match serde_json::from_str::<serde_json::Value>(&json) { Ok(serde_json::Value::Object(m)) => m, _ => Default::default() }, tracked: tracked != 0, modified }).unwrap_or_default()
    }
    /// One cell: the item's blob as the database has it, with this column's value set (`null` clears and keeps the key, as 1.4.2).
    pub fn set_field(&self, job: &Path, item: &Path, folder: &str, uuid: &str, value: serde_json::Value) -> Result<(), String> {
        let mut m = self.item(item); m.values.insert(uuid.to_string(), value); self.put_item(job, item, folder, &m.values, m.tracked)
    }
    /// The star.
    pub fn set_tracked(&self, job: &Path, item: &Path, folder: &str, on: bool) -> Result<(), String> { let m = self.item(item); self.put_item(job, item, folder, &m.values, on) }
}

// ------------------------------------------------------------------ the mesh's side (row 494; `mesh.rs`)
/// What waits to be sent: (its number, `meta` or `col`, the key, a ready message or none).
pub type Pending = (i64, String, String, Option<String>);
impl Db {
    /// Something shared was changed here: it waits for the mesh. Only what lies under a mapped share is ever sent.
    fn note(&self, kind: &str, key: &str, payload: Option<String>) {
        if !key.starts_with("vol:") && kind == "meta" { return }
        // (one entry a row: what is sent is the row as it is THEN - a ready message is kept as it is, each one)
        if payload.is_none() { let _ = self.conn.execute("DELETE FROM mesh_pending WHERE kind = ?1 AND key = ?2 AND payload IS NULL", rusqlite::params![kind, key]); }
        let _ = self.conn.execute("INSERT INTO mesh_pending (kind, key, payload, queued_time) VALUES (?1, ?2, ?3, ?4)", rusqlite::params![kind, key, payload, now_ms()]);
    }
    fn note_col(&self, id: i64) { if let Ok((uuid, job)) = self.conn.query_row("SELECT COALESCE(column_uuid, ''), job_path FROM column_definitions WHERE id = ?1", [id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))) { if job.starts_with("vol:") && !uuid.is_empty() { self.note("col", &uuid, None); } } }
    pub fn pending(&self) -> Vec<Pending> { self.conn.prepare("SELECT id, kind, key, payload FROM mesh_pending ORDER BY id LIMIT 200").and_then(|mut st| Ok(st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.flatten().collect())).unwrap_or_default() }
    pub fn pending_count(&self) -> usize { self.conn.query_row("SELECT count(*) FROM mesh_pending", [], |r| r.get::<_, i64>(0)).unwrap_or(0) as usize }
    pub fn pending_done(&self, id: i64) { let _ = self.conn.execute("DELETE FROM mesh_pending WHERE id = ?1", [id]); }
    /// An item's row as 1.4.2 sends it (`POST /api/metadata/update`): the whole document, as a string, with its star and its time.
    pub fn meta_wire(&self, item_identity: &str) -> Option<serde_json::Value> {
        self.conn.query_row("SELECT job_path, folder_name, metadata_json, is_tracked, COALESCE(modified_time, 0) FROM item_metadata WHERE item_path = ?1 AND deleted_at IS NULL", [item_identity], |r| Ok(serde_json::json!({
            "job_path": r.get::<_, String>(0)?, "item_path": item_identity, "metadata": r.get::<_, String>(2)?, "folder_name": r.get::<_, String>(1)?, "is_tracked": r.get::<_, i64>(3)? != 0, "modified_time": r.get::<_, i64>(4)? }))).ok()
    }
    /// A column as 1.4.2 sends it (`POST /api/table/update`): `col_add` - which its receiver takes as "make it or
    /// bring it up to date" - with the definition as a STRING, its fields in camel case, its options with it.
    pub fn col_wire(&self, uuid: &str) -> Option<serde_json::Value> {
        let (id, job, folder, name, kind, order, width, visible, default, modified, deleted, hash, trashed): (i64, String, String, String, String, i64, f64, i64, Option<String>, i64, Option<i64>, Option<String>, Option<i64>) =
            self.conn.query_row("SELECT id, job_path, folder_name, column_name, column_type, column_order, column_width, is_visible, default_value, modified_time, deleted_at, template_hash, trashed_at FROM column_definitions WHERE column_uuid = ?1 ORDER BY deleted_at IS NOT NULL, modified_time DESC LIMIT 1", [uuid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?, r.get(12)?))).ok()?;
        let options: Vec<serde_json::Value> = self.conn.prepare("SELECT id, option_name, option_color, modified_time, deleted_at FROM column_options WHERE column_id = ?1 ORDER BY id").and_then(|mut st| Ok(st.query_map([id], |r| Ok(serde_json::json!({ "id": r.get::<_, i64>(0)?, "name": r.get::<_, String>(1)?, "color": r.get::<_, Option<String>>(2)?, "modifiedTime": r.get::<_, i64>(3)?, "deletedAt": r.get::<_, Option<i64>>(4)? })))?.flatten().collect())).unwrap_or_default();
        let mut def = serde_json::json!({ "id": id, "jobPath": job, "folderName": folder, "columnName": name, "columnType": kind, "columnOrder": order, "columnWidth": width, "isVisible": visible != 0, "defaultValue": default, "options": options, "modifiedTime": modified, "deletedAt": deleted, "columnUuid": uuid });
        if let Some(h) = hash { def["templateHash"] = h.into(); } if let Some(t) = trashed { def["trashedAt"] = t.into(); }
        Some(serde_json::json!({ "action": "col_add", "def": def.to_string(), "modified_time": modified }))
    }
    pub fn pending_done_through(&self, id: i64) { let _ = self.conn.execute("DELETE FROM mesh_pending WHERE id <= ?1", [id]); }
    pub fn pending_last(&self) -> i64 { self.conn.query_row("SELECT COALESCE(MAX(id), 0) FROM mesh_pending", [], |r| r.get(0)).unwrap_or(0) }
    /// The shared rows as a snapshot file (1.4.2's, to the statement: a SQLite file of four tables - `subscriptions`,
    /// always empty; the columns; their options; the items' rows - with what is deleted or in the trash among them).
    /// `only`: nothing but the rows under this identity (a test's playground). False: there is nothing shared to write.
    pub fn write_snapshot(&self, to: &Path, only: Option<&str>) -> Result<bool, String> {
        let _ = std::fs::remove_file(to); let e = |x: rusqlite::Error| x.to_string();
        let scope = |c: &str| match only { Some(o) => { let o = o.replace('\'', "''"); format!("({c} = '{o}' OR substr({c}, 1, {}) = '{o}/')", o.chars().count() + 1) } None => format!("{c} LIKE 'vol:%'") };
        let any: i64 = self.conn.query_row(&format!("SELECT (SELECT count(*) FROM item_metadata WHERE item_path LIKE 'vol:%' AND {}) + (SELECT count(*) FROM column_definitions WHERE {})", scope("job_path"), scope("job_path")), [], |r| r.get(0)).map_err(e)?;
        if any == 0 { return Ok(false) }
        self.conn.execute("ATTACH DATABASE ?1 AS snap", [to.to_string_lossy()]).map_err(e)?;
        let run = || -> rusqlite::Result<()> {
            self.conn.execute_batch("CREATE TABLE snap.subscriptions (id INTEGER PRIMARY KEY AUTOINCREMENT, job_path TEXT NOT NULL UNIQUE, job_name TEXT NOT NULL, is_active INTEGER NOT NULL DEFAULT 1, subscribed_time INTEGER NOT NULL, last_sync_time INTEGER, sync_status TEXT NOT NULL DEFAULT 'Pending', shot_count INTEGER NOT NULL DEFAULT 0, modified_time INTEGER NOT NULL DEFAULT 0, deleted_at INTEGER);
                CREATE TABLE snap.column_definitions (id INTEGER PRIMARY KEY AUTOINCREMENT, job_path TEXT NOT NULL, folder_name TEXT NOT NULL, column_name TEXT NOT NULL, column_type TEXT NOT NULL DEFAULT 'text', column_order INTEGER NOT NULL DEFAULT 0, column_width REAL NOT NULL DEFAULT 120.0, is_visible INTEGER NOT NULL DEFAULT 1, default_value TEXT, modified_time INTEGER NOT NULL DEFAULT 0, deleted_at INTEGER, template_hash TEXT, trashed_at INTEGER, column_uuid TEXT);
                CREATE TABLE snap.column_options (id INTEGER PRIMARY KEY AUTOINCREMENT, column_id INTEGER NOT NULL, option_name TEXT NOT NULL, option_color TEXT, modified_time INTEGER NOT NULL DEFAULT 0, deleted_at INTEGER, FOREIGN KEY (column_id) REFERENCES column_definitions(id) ON DELETE CASCADE);
                CREATE TABLE snap.item_metadata (item_path TEXT NOT NULL, job_path TEXT NOT NULL, folder_name TEXT NOT NULL, metadata_json TEXT NOT NULL DEFAULT '{}', is_tracked INTEGER NOT NULL DEFAULT 0, created_time INTEGER, modified_time INTEGER, device_id TEXT, deleted_at INTEGER, PRIMARY KEY (item_path));")?;
            self.conn.execute(&format!("INSERT INTO snap.column_definitions (id, job_path, folder_name, column_name, column_type, column_order, column_width, is_visible, default_value, modified_time, deleted_at, template_hash, trashed_at, column_uuid) SELECT id, job_path, folder_name, column_name, column_type, column_order, column_width, is_visible, default_value, modified_time, deleted_at, template_hash, trashed_at, column_uuid FROM main.column_definitions WHERE {}", scope("job_path")), [])?;
            self.conn.execute("INSERT INTO snap.column_options (id, column_id, option_name, option_color, modified_time, deleted_at) SELECT id, column_id, option_name, option_color, modified_time, deleted_at FROM main.column_options WHERE column_id IN (SELECT id FROM snap.column_definitions)", [])?;
            self.conn.execute(&format!("INSERT INTO snap.item_metadata (item_path, job_path, folder_name, metadata_json, is_tracked, created_time, modified_time, device_id, deleted_at) SELECT item_path, job_path, folder_name, metadata_json, is_tracked, created_time, modified_time, device_id, deleted_at FROM main.item_metadata WHERE item_path LIKE 'vol:%' AND {}", scope("job_path")), [])?;
            Ok(())
        };
        let r = run(); let _ = self.conn.execute_batch("DETACH DATABASE snap"); r.map_err(e)?; Ok(true)
    }
    /// A snapshot file is merged into this database (1.4.2's rules): an item's row that is not here is taken, one that
    /// is here is replaced when the snapshot's is NEWER; a column is found by its uuid - taken when it is not here,
    /// brought up to date when the snapshot's is newer, and what is in the trash or deleted THERE is so here whichever
    /// is newer; nothing is ever deleted here. The file is checked first (row 442: a file still on its way - a share
    /// that syncs - is not merged): Err says it is not whole. Returns how many rows changed.
    pub fn merge_snapshot(&self, from: &Path, only: Option<&str>) -> Result<usize, String> {
        let e = |x: rusqlite::Error| x.to_string();
        // (looked at by a connection of its own first: what is no database, or a broken one, is never attached)
        { let c = Connection::open_with_flags(from, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(e)?; let ok: String = c.query_row("PRAGMA quick_check", [], |r| r.get(0)).map_err(e)?; if ok != "ok" { return Err(format!("the snapshot is not whole: {ok}")) }
            let tables: i64 = c.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name IN ('item_metadata', 'column_definitions', 'column_options')", [], |r| r.get(0)).map_err(e)?; if tables != 3 { return Err("the snapshot is not whole: tables are missing".into()) } }
        let scope = |c: &str| match only { Some(o) => { let o = o.replace('\'', "''"); format!("({c} = '{o}' OR substr({c}, 1, {}) = '{o}/')", o.chars().count() + 1) } None => format!("{c} LIKE 'vol:%'") };
        self.conn.execute("ATTACH DATABASE ?1 AS snap", [from.to_string_lossy()]).map_err(e)?;
        let run = || -> rusqlite::Result<usize> {
            // the items' rows: one that differs from ours is merged by cell, as one that arrives in a message is (row 440);
            // one that is deleted there is taken when it is newer (1.4.2)
            let mut n = self.conn.execute(&format!("INSERT INTO main.item_metadata (item_path, job_path, folder_name, metadata_json, is_tracked, created_time, modified_time, device_id, deleted_at)
                 SELECT item_path, job_path, folder_name, metadata_json, is_tracked, created_time, modified_time, device_id, deleted_at FROM snap.item_metadata WHERE item_path LIKE 'vol:%' AND deleted_at IS NOT NULL AND {}
                 ON CONFLICT(item_path) DO UPDATE SET metadata_json = excluded.metadata_json, is_tracked = excluded.is_tracked, modified_time = excluded.modified_time, deleted_at = excluded.deleted_at
                 WHERE excluded.modified_time > COALESCE(item_metadata.modified_time, -1)", scope("job_path")), [])?;
            let rows: Vec<(String, String, String, String, bool, i64)> = { let mut st = self.conn.prepare(&format!("SELECT s.item_path, s.job_path, s.folder_name, s.metadata_json, s.is_tracked, COALESCE(s.modified_time, 0) FROM snap.item_metadata s LEFT JOIN main.item_metadata m ON m.item_path = s.item_path
                 WHERE s.item_path LIKE 'vol:%' AND s.deleted_at IS NULL AND {} AND (m.item_path IS NULL OR m.deleted_at IS NOT NULL OR m.metadata_json != s.metadata_json OR m.is_tracked != s.is_tracked)", scope("s.job_path")))?;
                let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get::<_, i64>(4)? != 0, r.get(5)?)))?.flatten().collect(); v };
            for (item, job, folder, doc, tracked, time) in rows { if self.merge_row(&item, &job, &folder, &doc, tracked, time)? { n += 1; } }
            // a row that is the SAME there and here is in common - that, and nothing sooner: what this machine wrote into a
            // snapshot is not in common until another machine's snapshot shows it has it
            self.conn.execute(&format!("INSERT OR REPLACE INTO mesh_base (item_path, metadata_json, is_tracked, modified_time) SELECT s.item_path, s.metadata_json, s.is_tracked, COALESCE(s.modified_time, 0) FROM snap.item_metadata s JOIN main.item_metadata m ON m.item_path = s.item_path WHERE s.deleted_at IS NULL AND m.deleted_at IS NULL AND m.metadata_json = s.metadata_json AND m.is_tracked = s.is_tracked AND {}", scope("s.job_path")), [])?;
            type Col = (i64, String, String, String, String, i64, f64, i64, Option<String>, i64, Option<i64>, Option<String>, Option<i64>, Option<String>);
            let cols: Vec<Col> = { let mut st = self.conn.prepare(&format!("SELECT id, job_path, folder_name, column_name, column_type, column_order, column_width, is_visible, default_value, modified_time, deleted_at, template_hash, trashed_at, column_uuid FROM snap.column_definitions WHERE {}", scope("job_path")))?;
                let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?, r.get(12)?, r.get(13)?)))?.flatten().collect(); v };
            for (sid, job, folder, name, kind, order, width, visible, default, time, deleted, hash, trashed, uuid) in cols {
                let uuid = uuid.filter(|u| !u.is_empty()).unwrap_or_else(|| column_uuid(&job, &folder, &name, &kind));
                let local: Option<(i64, i64, Option<i64>, Option<i64>)> = self.conn.query_row("SELECT id, modified_time, deleted_at, trashed_at FROM column_definitions WHERE column_uuid = ?1 ORDER BY deleted_at IS NOT NULL LIMIT 1", [&uuid], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).ok()
                    .or_else(|| self.conn.query_row("SELECT id, modified_time, deleted_at, trashed_at FROM column_definitions WHERE job_path = ?1 AND folder_name = ?2 AND column_name = ?3 AND (column_uuid IS NULL OR column_uuid = '') LIMIT 1", rusqlite::params![job, folder, name], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).ok());
                let (id, whole) = match local {
                    None => { self.conn.execute("INSERT INTO column_definitions (job_path, folder_name, column_name, column_type, column_order, column_width, is_visible, default_value, modified_time, deleted_at, template_hash, trashed_at, column_uuid) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)", rusqlite::params![job, folder, name, kind, order, width, visible, default, time, deleted, hash, trashed, uuid])?; n += 1; (self.conn.last_insert_rowid(), true) }
                    Some((id, lt, _, _)) if time > lt => { n += self.conn.execute("UPDATE column_definitions SET job_path = ?1, folder_name = ?2, column_name = ?3, column_type = ?4, column_order = ?5, column_width = ?6, is_visible = ?7, default_value = ?8, modified_time = ?9, deleted_at = ?10, template_hash = ?11, trashed_at = ?12, column_uuid = ?13 WHERE id = ?14", rusqlite::params![job, folder, name, kind, order, width, visible, default, time, deleted, hash, trashed, uuid, id])?; (id, true) }
                    // (not newer - but the trash and a deletion are final: taken all the same)
                    Some((id, _, ld, lt)) => { if (deleted.is_some() && ld.is_none()) || (trashed.is_some() && lt.is_none() && ld.is_none()) { n += self.conn.execute("UPDATE column_definitions SET deleted_at = COALESCE(deleted_at, ?1), trashed_at = COALESCE(trashed_at, ?2) WHERE id = ?3", rusqlite::params![deleted, trashed, id])?; } (id, false) }
                };
                // the options: the snapshot's own when the column was taken from it; else those that are not here yet, by name
                if whole { self.conn.execute("DELETE FROM column_options WHERE column_id = ?1", [id])?; }
                n += self.conn.execute("INSERT INTO column_options (column_id, option_name, option_color, modified_time, deleted_at) SELECT ?1, o.option_name, o.option_color, o.modified_time, o.deleted_at FROM snap.column_options o WHERE o.column_id = ?2 AND NOT EXISTS (SELECT 1 FROM column_options m WHERE m.column_id = ?1 AND m.option_name = o.option_name) ORDER BY o.id", rusqlite::params![id, sid])?;
            }
            Ok(n)
        };
        let r = run(); let _ = self.conn.execute_batch("DETACH DATABASE snap"); r.map_err(e)
    }
    /// A path as a message carries it, in storage form: an identity as it is; a plain path of this machine classified (1.4.2 accepts both).
    fn wire_path(s: &str) -> String { if s.starts_with("vol:") || s.starts_with("native:") { s.to_string() } else { to_identity(Path::new(s)) } }
    /// An item's row arrived (1.4.2's rule): it replaces ours when it is not older; a time of 0 is "now". True: taken.
    pub fn apply_meta(&self, v: &serde_json::Value) -> Result<bool, String> {
        let text = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
        let (Some(job), Some(item)) = (text("job_path").map(|p| Self::wire_path(&p)), text("item_path").map(|p| Self::wire_path(&p))) else { return Err("job_path and item_path are needed".into()) };
        let (doc, folder, tracked) = (text("metadata").unwrap_or_else(|| "{}".into()), text("folder_name").unwrap_or_default(), v.get("is_tracked").and_then(|x| x.as_bool()).unwrap_or(false));
        let mut time = v.get("modified_time").and_then(|x| x.as_i64()).unwrap_or(0); if time == 0 { time = now_ms(); }
        self.merge_row(&item, &job, &folder, &doc, tracked, time).map_err(|e| e.to_string())
    }
    /// An item's row from another machine is MERGED BY CELL into ours (row 440; 1.4.2 replaces the whole row, so of two
    /// people who change different cells of one item at about the same time one loses the edit). Against the version
    /// this machine last had in common with the others: a cell only the sender changed is taken; one only this machine
    /// changed is kept; one both changed goes to the LATER row, without a word (row 441); the star likewise. When what
    /// comes of it is not what arrived, it waits to be sent on - which also mends the copy of a 1.4.2 node. True: our row changed.
    fn merge_row(&self, item: &str, job: &str, folder: &str, doc: &str, tracked: bool, time: i64) -> rusqlite::Result<bool> {
        type Map = serde_json::Map<String, serde_json::Value>;
        let parse = |s: &str| -> Map { match serde_json::from_str::<serde_json::Value>(s) { Ok(serde_json::Value::Object(m)) => m, _ => Map::new() } };
        let local: Option<(String, bool, i64, bool)> = self.conn.query_row("SELECT metadata_json, is_tracked, COALESCE(modified_time, 0), deleted_at IS NOT NULL FROM item_metadata WHERE item_path = ?1", [item], |r| Ok((r.get(0)?, r.get::<_, i64>(1)? != 0, r.get(2)?, r.get(3)?))).ok();
        let base_set = |d: &str, t: bool| self.conn.execute("INSERT INTO mesh_base (item_path, metadata_json, is_tracked, modified_time) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(item_path) DO UPDATE SET metadata_json = excluded.metadata_json, is_tracked = excluded.is_tracked, modified_time = excluded.modified_time", rusqlite::params![item, d, t as i64, time]);
        // not here (or deleted here): the row is taken as it is
        let Some((ldoc, ltracked, ltime, _)) = local.filter(|l| !l.3) else {
            self.conn.execute("INSERT INTO item_metadata (item_path, job_path, folder_name, metadata_json, is_tracked, modified_time, deleted_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)
                 ON CONFLICT(item_path) DO UPDATE SET job_path = excluded.job_path, folder_name = excluded.folder_name, metadata_json = excluded.metadata_json, is_tracked = excluded.is_tracked, modified_time = excluded.modified_time, deleted_at = NULL", rusqlite::params![item, job, folder, doc, tracked as i64, time])?;
            base_set(doc, tracked)?; return Ok(true) };
        let (inc, loc) = (parse(doc), parse(&ldoc));
        let (base, btracked, btime): (Map, bool, i64) = self.conn.query_row("SELECT metadata_json, is_tracked, modified_time FROM mesh_base WHERE item_path = ?1", [item], |r| Ok((parse(&r.get::<_, String>(0)?), r.get::<_, i64>(1)? != 0, r.get(2)?))).unwrap_or((Map::new(), false, 0));
        // a row OLDER than the version in common is no change of its sender's: it is one that was overtaken on its way (or sent again), and is left
        if time < btime { return Ok(false) }
        // (the sender's row wins a cell both changed when it is not older - 1.4.2's ">=" for a row)
        let theirs = time >= ltime; let mut out = Map::new();
        let keys: std::collections::BTreeSet<&String> = inc.keys().chain(loc.keys()).chain(base.keys()).collect();
        // (a cell the sender's row does not HOLD is no change of the sender's: it never saw that column - clearing a cell writes null and keeps the key)
        for k in keys { let (i, l, b) = (inc.get(k), loc.get(k), base.get(k)); let (ic, lc) = (i.is_some() && i != b, l != b);
            if let Some(v) = if ic && (!lc || theirs) { i } else { l } { out.insert(k.clone(), v.clone()); } }
        let (ic, lc) = (tracked != btracked, ltracked != btracked); let otracked = if ic && (!lc || theirs) { tracked } else { ltracked };
        let (same_as_ours, same_as_theirs) = (out == loc && otracked == ltracked, out == inc && otracked == tracked);
        if std::env::var_os("UFB_MESH_LOG").is_some() { eprintln!("UFB_MESH merge {}: theirs star {tracked} t {time} | ours star {ltracked} t {ltime} | common star {btracked} t {btime} -> star {otracked}, {} cell(s)", item.rsplit('/').next().unwrap_or(""), out.len()); }
        // the mesh has the sender's row: that is the common version now (until ours, if it differs, was taken too)
        base_set(doc, tracked)?;
        if same_as_ours && same_as_theirs { if time > ltime { self.conn.execute("UPDATE item_metadata SET modified_time = ?1 WHERE item_path = ?2", rusqlite::params![time, item])?; } return Ok(false) }
        if !same_as_ours { self.conn.execute("UPDATE item_metadata SET job_path = ?1, folder_name = ?2, metadata_json = ?3, is_tracked = ?4, modified_time = ?5, deleted_at = NULL WHERE item_path = ?6", rusqlite::params![job, folder, serde_json::Value::Object(out.clone()).to_string(), otracked as i64, time.max(ltime), item])?; }
        // what we hold is more than what arrived: it is sent on, as a change of this machine's
        if !same_as_theirs { if same_as_ours && ltime < time { self.conn.execute("UPDATE item_metadata SET modified_time = ?1 WHERE item_path = ?2", rusqlite::params![time, item])?; } self.note("meta", item, None); }
        Ok(!same_as_ours)
    }
    /// What this machine sent was taken by the mesh: it is the version in common from now on.
    pub fn base_from_wire(&self, v: &serde_json::Value) { if let (Some(item), Some(doc)) = (v.get("item_path").and_then(|x| x.as_str()), v.get("metadata").and_then(|x| x.as_str())) {
        let _ = self.conn.execute("INSERT INTO mesh_base (item_path, metadata_json, is_tracked, modified_time) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(item_path) DO UPDATE SET metadata_json = excluded.metadata_json, is_tracked = excluded.is_tracked, modified_time = excluded.modified_time", rusqlite::params![item, doc, v.get("is_tracked").and_then(|x| x.as_bool()).unwrap_or(false) as i64, v.get("modified_time").and_then(|x| x.as_i64()).unwrap_or(0)]); } }

    /// A column's change arrived (1.4.2's `apply_table_change`): `col_add` / `col_update` take the definition when it
    /// is NEWER than ours (found by its uuid, else by job, section and name), `col_delete` is final; what 1.4.2 itself
    /// accepts and ignores is accepted and ignored; anything else is an error. True: something changed.
    pub fn apply_table(&self, v: &serde_json::Value) -> Result<bool, String> {
        let action = v.get("action").and_then(|x| x.as_str()).unwrap_or(""); let top = v.get("modified_time").and_then(|x| x.as_i64()).unwrap_or(0);
        match action {
            "col_add" | "col_update" => {
                let def: serde_json::Value = match v.get("def") { Some(serde_json::Value::String(s)) => serde_json::from_str(s).map_err(|e| e.to_string())?, Some(o @ serde_json::Value::Object(_)) => o.clone(), _ => return Err("the column's definition is missing".into()) };
                let text = |k: &str| def.get(k).and_then(|x| x.as_str()).map(str::to_string); let int = |k: &str| def.get(k).and_then(|x| x.as_i64());
                let (Some(job), Some(folder), Some(name)) = (text("jobPath").map(|p| Self::wire_path(&p)), text("folderName"), text("columnName")) else { return Err("the column's definition is incomplete".into()) };
                let kind = text("columnType").unwrap_or_else(|| "text".into()); let uuid = text("columnUuid").filter(|u| !u.is_empty()).unwrap_or_else(|| column_uuid(&job, &folder, &name, &kind));
                let mut time = int("modifiedTime").filter(|t| *t > 0).unwrap_or(top); if time == 0 { time = now_ms(); }
                let found: Option<(i64, i64)> = self.conn.query_row("SELECT id, modified_time FROM column_definitions WHERE column_uuid = ?1 ORDER BY deleted_at IS NOT NULL LIMIT 1", [&uuid], |r| Ok((r.get(0)?, r.get(1)?))).ok()
                    .or_else(|| self.conn.query_row("SELECT id, modified_time FROM column_definitions WHERE job_path = ?1 AND folder_name = ?2 AND column_name = ?3 AND deleted_at IS NULL LIMIT 1", rusqlite::params![job, folder, name], |r| Ok((r.get(0)?, r.get(1)?))).ok());
                let (order, width, visible) = (int("columnOrder").unwrap_or(0), def.get("columnWidth").and_then(|x| x.as_f64()).unwrap_or(120.0), def.get("isVisible").and_then(|x| x.as_bool()).unwrap_or(true) as i64);
                let id = match found {
                    Some((_, local)) if time <= local => return Ok(false),
                    Some((id, _)) => { self.conn.execute("UPDATE column_definitions SET job_path = ?1, folder_name = ?2, column_name = ?3, column_type = ?4, column_order = ?5, column_width = ?6, is_visible = ?7, default_value = ?8, modified_time = ?9, deleted_at = ?10, trashed_at = ?11, template_hash = ?12, column_uuid = ?13 WHERE id = ?14",
                        rusqlite::params![job, folder, name, kind, order, width, visible, text("defaultValue"), time, int("deletedAt"), int("trashedAt"), text("templateHash"), uuid, id]).map_err(|e| e.to_string())?; id }
                    // (`col_update` for a column that is not here: 1.4.2 ignores it)
                    None if action == "col_update" => return Ok(false),
                    None => { self.conn.execute("INSERT INTO column_definitions (job_path, folder_name, column_name, column_type, column_order, column_width, is_visible, default_value, modified_time, deleted_at, trashed_at, template_hash, column_uuid) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                        rusqlite::params![job, folder, name, kind, order, width, visible, text("defaultValue"), time, int("deletedAt"), int("trashedAt"), text("templateHash"), uuid]).map_err(|e| e.to_string())?; self.conn.last_insert_rowid() }
                };
                self.conn.execute("DELETE FROM column_options WHERE column_id = ?1", [id]).map_err(|e| e.to_string())?;
                for o in def.get("options").and_then(|x| x.as_array()).into_iter().flatten() { let Some(n) = o.get("name").and_then(|x| x.as_str()) else { continue }; if o.get("deletedAt").is_some_and(|d| !d.is_null()) { continue }
                    self.conn.execute("INSERT INTO column_options (column_id, option_name, option_color, modified_time, deleted_at) VALUES (?1, ?2, ?3, ?4, NULL)", rusqlite::params![id, n, o.get("color").and_then(|x| x.as_str()), o.get("modifiedTime").and_then(|x| x.as_i64()).filter(|t| *t > 0).unwrap_or(time)]).map_err(|e| e.to_string())?; }
                Ok(true)
            }
            "col_delete" => {
                let text = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string); let at = v.get("deleted_at").and_then(|x| x.as_i64()).filter(|t| *t > 0).unwrap_or_else(now_ms);
                let n = match text("column_uuid").filter(|u| !u.is_empty()) { Some(u) => self.conn.execute("UPDATE column_definitions SET deleted_at = ?1, modified_time = ?1 WHERE column_uuid = ?2 AND (deleted_at IS NULL OR deleted_at < ?1)", rusqlite::params![at, u]),
                    None => self.conn.execute("UPDATE column_definitions SET deleted_at = ?1, modified_time = ?1 WHERE job_path = ?2 AND folder_name = ?3 AND column_name = ?4 AND (deleted_at IS NULL OR deleted_at < ?1)", rusqlite::params![at, text("job_path").map(|p| Self::wire_path(&p)).unwrap_or_default(), text("folder_name").unwrap_or_default(), text("column_name").unwrap_or_default()]) }.map_err(|e| e.to_string())?;
                Ok(n > 0)
            }
            "sub_add" | "sub_remove" | "preset_save" | "preset_delete" => Ok(false),
            other => Err(format!("unknown action {other:?}")),
        }
    }
}

static DB: OnceLock<Option<Mutex<Db>>> = OnceLock::new();
/// The application's database, opened at its first use. None: it could not be opened (said once on the error stream).
pub fn with<R>(f: impl FnOnce(&Db) -> R) -> Option<R> {
    let db = DB.get_or_init(|| { let dir = min_settings::Settings::dir_for("UFB-test", "UFB")?; match Db::open(&dir) { Ok(d) => Some(Mutex::new(d)), Err(e) => { eprintln!("UFB: the database: {e}"); None } } });
    db.as_ref().map(|m| f(&m.lock().unwrap()))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp(name: &str) -> PathBuf { let d = std::env::temp_dir().join(format!("ufb-db-{name}-{}", std::process::id())); let _ = std::fs::remove_dir_all(&d); d }
    #[test]
    fn a_new_file_has_the_tables_and_the_version_of_1_4_2() {
        let dir = temp("schema"); let db = Db::open(&dir).unwrap();
        let cols = |t: &str| -> Vec<String> { db.conn.prepare(&format!("SELECT name FROM pragma_table_info('{t}')")).unwrap().query_map([], |r| r.get(0)).unwrap().flatten().collect() };
        assert_eq!(cols("subscriptions"), ["id", "job_path", "job_name", "is_active", "subscribed_time", "last_sync_time", "sync_status", "shot_count", "modified_time", "deleted_at"]);
        assert_eq!(cols("item_metadata"), ["item_path", "job_path", "folder_name", "metadata_json", "is_tracked", "created_time", "modified_time", "device_id", "deleted_at"]);
        assert_eq!(cols("column_definitions"), ["id", "job_path", "folder_name", "column_name", "column_type", "column_order", "column_width", "is_visible", "default_value", "column_uuid", "modified_time", "deleted_at", "template_hash", "trashed_at", "name_conflict_marker"]);
        assert_eq!(cols("column_options"), ["id", "column_id", "option_name", "option_color", "modified_time", "deleted_at"]);
        for t in ["column_layout_personal", "bookmarks", "column_presets", "volumes", "volume_bindings", "migration_orphans"] { assert!(!cols(t).is_empty(), "{t}"); }
        assert_eq!(db.conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0)).unwrap(), 4);
        // opened again: nothing is added twice
        drop(db); let db = Db::open(&dir).unwrap(); assert_eq!(db.conn.query_row("SELECT count(*) FROM pragma_table_info('subscriptions')", [], |r| r.get::<_, i64>(0)).unwrap(), 10);
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn a_job_is_subscribed_unsubscribed_and_comes_back() {
        let dir = temp("subs"); let db = Db::open(&dir).unwrap(); let root = if cfg!(windows) { "C:\\jobs" } else { "/jobs" }; let (a, b) = (Path::new(root).join("2001_zebra"), Path::new(root).join("2002_apple"));
        db.subscribe(&a, "").unwrap(); db.subscribe(&b, "Apple").unwrap();
        assert_eq!(db.subscriptions(), [Sub { name: "2001_zebra".into(), path: a.clone() }, Sub { name: "Apple".into(), path: b.clone() }], "by name; an empty name is the folder's");
        let stored: String = db.conn.query_row("SELECT job_path FROM subscriptions WHERE job_name = 'Apple'", [], |r| r.get(0)).unwrap();
        assert_eq!(stored, format!("native:{OS}/{}", b.display()), "1.4.2's identity form");
        db.unsubscribe(&a).unwrap(); assert_eq!(db.subscriptions().len(), 1);
        assert_eq!(db.conn.query_row("SELECT count(*) FROM subscriptions", [], |r| r.get::<_, i64>(0)).unwrap(), 2, "a tombstone, not a deletion");
        db.subscribe(&a, "Zebra").unwrap(); assert_eq!(db.subscriptions()[1].name, "Zebra", "it comes back, with the name given now");
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn columns_and_cells_as_1_4_2_keeps_them() {
        let dir = temp("meta"); let db = Db::open(&dir).unwrap(); let root = if cfg!(windows) { "C:\\jobs\\2601_a" } else { "/jobs/2601_a" }; let job = Path::new(root); let (sh010, sh020) = (job.join("3d").join("sh010"), job.join("3d").join("sh020"));
        // the identity is 1.4.2's derivation: the name and the type without regard to case
        assert_eq!(column_uuid("vol:1/j", "3d", "Status", "Dropdown"), column_uuid("vol:1/j", "3d", "status", "dropdown")); assert_ne!(column_uuid("vol:1/j", "3d", "Status", "text"), column_uuid("vol:1/j", "ae", "Status", "text"));
        assert_eq!(column_uuid("vol:1/j", "3d", "Status", "dropdown").len(), 36);
        let status = db.add_column(job, "3d", "Status", "dropdown", &[("WIP".into(), Some("#f5a623".into())), ("Final".into(), None)]).unwrap();
        let due = db.add_column(job, "3d", "Due", "date", &[]).unwrap();
        assert!(db.add_column(job, "3d", "status", "text", &[]).is_err(), "a name in use, letters' case apart"); assert!(db.add_column(job, "3d", "  ", "text", &[]).is_err());
        db.add_column(job, "ae", "Status", "text", &[]).unwrap();
        let cols = db.columns(job, "3d"); assert_eq!(cols.iter().map(|c| (c.name.as_str(), c.kind.as_str(), c.order)).collect::<Vec<_>>(), [("Status", "dropdown", 0), ("Due", "date", 1)]);
        assert_eq!(cols[0].options, [("WIP".to_string(), Some("#f5a623".to_string())), ("Final".to_string(), None)]); assert!(cols.iter().all(|c| c.visible && c.width == 120.0));
        // a cell, the star; the blob is keyed by the column's uuid; clearing keeps the key with null
        db.set_field(job, &sh010, "3d", &cols[0].uuid, serde_json::json!("WIP")).unwrap(); db.set_field(job, &sh010, "3d", &cols[1].uuid, serde_json::json!(1_795_000_000_000i64)).unwrap(); db.set_tracked(job, &sh010, "3d", true).unwrap();
        db.set_tracked(job, &sh020, "3d", true).unwrap(); db.set_tracked(job, &sh020, "3d", false).unwrap();
        let m = db.folder_meta(job, "3d"); assert_eq!(m[&sh010].values[&cols[0].uuid], "WIP"); assert!(m[&sh010].tracked); assert!(!m[&sh020].tracked); assert!(m[&sh010].modified > 0);
        db.set_field(job, &sh010, "3d", &cols[0].uuid, serde_json::Value::Null).unwrap(); let m = db.folder_meta(job, "3d"); assert!(m[&sh010].values[&cols[0].uuid].is_null()); assert!(m[&sh010].tracked, "a cell does not touch the star");
        let stored: String = db.conn.query_row("SELECT item_path FROM item_metadata WHERE is_tracked = 1", [], |r| r.get(0)).unwrap(); assert_eq!(stored, to_identity(&sh010));
        // a rename keeps the uuid (the values hang on it) and takes this user's layout along; the layout lies over the shared one
        db.set_layout(job, "3d", "Due", Some(-1), Some(90.0), None).unwrap(); db.set_layout(job, "3d", "Status", None, None, Some(false)).unwrap();
        let cols2 = db.columns(job, "3d"); assert_eq!(cols2.iter().map(|c| (c.name.as_str(), c.width, c.visible)).collect::<Vec<_>>(), [("Due", 90.0, true), ("Status", 120.0, false)]);
        db.update_column(due, "Deadline", "date", &[]).unwrap(); let cols3 = db.columns(job, "3d"); assert_eq!((cols3[0].name.as_str(), cols3[0].width, &cols3[0].uuid), ("Deadline", 90.0, &cols[1].uuid));
        assert!(db.update_column(due, "STATUS", "date", &[]).is_err());
        // the trash: out of use, back again; not back under a name taken meanwhile
        db.trash_column(status, true).unwrap(); assert_eq!(db.columns(job, "3d").len(), 1); assert_eq!(db.trashed_columns(job, "3d"), [(status, "Status".to_string())]);
        let other = db.add_column(job, "3d", "Status", "text", &[]).unwrap(); assert!(db.trash_column(status, false).is_err()); db.trash_column(other, true).unwrap(); db.trash_column(status, false).unwrap();
        assert_eq!(db.columns(job, "3d").iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Deadline", "Status"]);
        assert_eq!(kind_of("Note"), "notes"); assert_eq!(kind_of("PRIORITY"), "priority"); assert_eq!(kind_of("formula"), "text");
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn the_metadata_follows_a_folder_that_was_renamed_or_moved() {
        let dir = temp("move"); let db = Db::open(&dir).unwrap(); let root = PathBuf::from(if cfg!(windows) { "C:\\jobs" } else { "/jobs" }); let job = root.join("2601_a");
        let (sh010, sh0100) = (job.join("3d").join("sh010"), job.join("3d").join("sh0100"));
        db.subscribe(&job, "").unwrap(); let status = db.add_column(&job, "3d", "Status", "text", &[]).unwrap(); let uuid = db.columns(&job, "3d")[0].uuid.clone(); db.set_layout(&job, "3d", "Status", None, Some(99.0), None).unwrap();
        db.set_field(&job, &sh010, "3d", &uuid, serde_json::json!("WIP")).unwrap(); db.set_tracked(&job, &sh010, "3d", true).unwrap(); db.set_tracked(&job, &sh0100, "3d", true).unwrap();
        // an item renamed: its row goes along; one whose name only BEGINS the same stays (sh0100 is not under sh010)
        let sh011 = job.join("3d").join("sh011"); db.move_paths(&sh010, &sh011).unwrap();
        let m = db.folder_meta(&job, "3d"); assert_eq!(m[&sh011].values[&uuid], "WIP"); assert!(m[&sh011].tracked); assert!(!m.contains_key(&sh010)); assert!(m.contains_key(&sh0100));
        // a section renamed in place: the rows and the columns that name it, and this user's layout
        db.move_paths(&job.join("3d"), &job.join("cg")).unwrap();
        assert!(db.folder_meta(&job, "3d").is_empty()); let m = db.folder_meta(&job, "cg"); assert_eq!(m[&job.join("cg").join("sh011")].values[&uuid], "WIP");
        let cols = db.columns(&job, "cg"); assert_eq!((cols[0].id, cols[0].name.as_str(), cols[0].width, &cols[0].uuid), (status, "Status", 99.0, &uuid), "the column keeps its identity: the values still hang on it");
        // the job renamed: everything of it in one step, and its subscription - called by the folder's new name
        let job2 = root.join("2601_b"); let n = db.move_paths(&job, &job2).unwrap(); assert!(n >= 4);
        assert_eq!(db.subscriptions(), [Sub { name: "2601_b".into(), path: job2.clone() }]); assert_eq!(db.columns(&job2, "cg").len(), 1); assert_eq!(db.folder_meta(&job2, "cg").len(), 2); assert!(db.tracked().iter().all(|t| t.1 == job2));
        // back again (an undo is the same step the other way)
        db.move_paths(&job2, &job).unwrap(); assert_eq!(db.subscriptions()[0].path, job); assert_eq!(db.folder_meta(&job, "cg").len(), 2);
        // a row left at the place something is moved to makes way
        let stray = job.join("cg").join("sh020"); db.set_tracked(&job, &stray, "cg", true).unwrap(); db.move_paths(&job.join("cg").join("sh011"), &stray).unwrap(); assert_eq!(db.folder_meta(&job, "cg")[&stray].values[&uuid], "WIP");
        // relink and remove
        let gone = job.join("cg").join("lost"); db.set_tracked(&job, &gone, "cg", true).unwrap(); let found = job.join("ae").join("found"); db.relink(&gone, &found, Some((&job, "ae"))).unwrap();
        assert!(db.folder_meta(&job, "ae")[&found].tracked); assert!(!db.folder_meta(&job, "cg").contains_key(&gone)); db.remove_item(&found).unwrap(); assert!(db.folder_meta(&job, "ae").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn a_mapped_share_gives_the_same_identity_on_both_systems() {
        // 1.4.2's derivation: separators, a slash at the end and case do not count
        let m = Mapping::new("C:\\Volumes\\gfx\\", "/Volumes/gfx"); assert_eq!(m.uuid, volume_uuid("c:/volumes/GFX", "/volumes/gfx/")); assert_ne!(m.uuid, volume_uuid("D:\\gfx", "/Volumes/gfx")); assert_eq!(m.uuid.len(), 36);
        let maps = [m.clone()];
        // either side's spelling of a path under the share is the same identity; the rest keeps its case and gets forward slashes
        let (a, b) = (identity_in(Path::new("/Volumes/gfx/jobs/0001_Test/3d/sh010"), &maps), identity_in(Path::new("C:\\Volumes\\gfx\\jobs\\0001_Test\\3d\\sh010"), &maps));
        assert_eq!(a, format!("vol:{}/jobs/0001_Test/3d/sh010", m.uuid)); assert_eq!(a, b); assert_eq!(identity_in(Path::new("/volumes/GFX"), &maps), format!("vol:{}", m.uuid));
        // only at the edge of a component: a share whose name merely begins the same is another place
        assert!(identity_in(Path::new("/Volumes/gfx-backup/jobs"), &maps).starts_with("native:")); assert!(identity_in(Path::new("/Volumes/gfxtra/jobs"), &maps).starts_with("native:")); assert!(identity_in(Path::new("/Users/a/Desktop"), &maps).starts_with("native:"));
        // back to this machine's own spelling
        let here = native_in(&a, &maps).unwrap(); assert_eq!(here, PathBuf::from(if cfg!(windows) { "C:\\Volumes\\gfx\\jobs\\0001_Test\\3d\\sh010" } else { "/Volumes/gfx/jobs/0001_Test/3d/sh010" }));
        assert_eq!(native_in("vol:0000/x", &maps), None, "a share that is not mapped here"); assert_eq!(native_in(&format!("vol:{}", m.uuid), &maps), Some(PathBuf::from(m.local())));
        // a share inside another wins over it
        let inner = Mapping::new("C:\\Volumes\\gfx\\jobs", "/Volumes/gfx/jobs"); assert_eq!(identity_in(Path::new("/Volumes/gfx/jobs/x"), &[m.clone(), inner.clone()]), format!("vol:{}/x", inner.uuid));
    }
    #[test]
    fn a_share_is_found_wherever_it_landed() {
        let jobs = Mapping::new("\\\\nas\\Jobs", "/Volumes/Jobs"); let id = |p: &str, maps: &[Mapping]| identity_in(Path::new(p), maps);
        // as TEXT: the numbered landing is the share (only digits count; the rest of the path is kept)
        let maps = [jobs.clone()]; let want = format!("vol:{}/2601_a/3d", jobs.uuid);
        assert_eq!(id("/Volumes/Jobs/2601_a/3d", &maps), want); assert_eq!(id("/Volumes/Jobs-2/2601_a/3d", &maps), want); assert_eq!(id("/Volumes/Jobs-17/2601_a/3d", &maps), want); assert_eq!(id("/Volumes/Jobs-2", &maps), format!("vol:{}", jobs.uuid));
        assert!(id("/Volumes/Jobs-extra/x", &maps).starts_with("native:"), "not digits: another volume"); assert!(id("/Volumes/Jobs-/x", &maps).starts_with("native:")); assert!(id("/Volumes/Other-2/x", &maps).starts_with("native:"));
        assert_eq!(landing("/Volumes/Jobs-2/a b/c", &maps), "/Volumes/Jobs/a b/c"); assert_eq!(landing("/Users/a/Jobs-2/x", &maps), "/Users/a/Jobs-2/x", "only directly under /Volumes");
        // where `Jobs` and `Jobs-2` are BOTH shares, each is itself alone
        let two = Mapping::new("\\\\nas\\Jobs-2", "/Volumes/Jobs-2"); let both = [jobs.clone(), two.clone()];
        assert_eq!(id("/Volumes/Jobs-2/x", &both), format!("vol:{}/x", two.uuid)); assert_eq!(id("/Volumes/Jobs/x", &both), format!("vol:{}/x", jobs.uuid));
        // ON the machine: the share is where the system says it is - a landing on a Mac, a letter on Windows - and that is
        // where a stored identity leads back to; what the others are told is the share's own side, never the landing
        let here = if cfg!(windows) { "J:\\" } else { "/Volumes/Jobs-3" }; let landed = [jobs.clone().at(Some(here.into()))];
        let inside = Path::new(here).join("2601_a"); assert_eq!(identity_in(&inside, &landed), format!("vol:{}/2601_a", jobs.uuid));
        assert_eq!(native_in(&format!("vol:{}/2601_a", jobs.uuid), &landed), Some(inside.clone()));
        set_mappings(landed.to_vec()); assert_eq!(canonical(&inside), if cfg!(windows) { "\\\\nas\\Jobs\\2601_a" } else { "/Volumes/Jobs/2601_a" });
        // a path from another machine, with ITS landing in it, opens here where the share is here
        assert_eq!(translate("/Volumes/Jobs-9/2601_a"), Some(inside)); assert_eq!(translate("/Users/a/Desktop"), None); set_mappings(vec![]);
    }
    #[test]
    fn rows_of_this_machine_under_a_share_mapped_later_become_the_shares() {
        let dir = temp("classify"); let db = Db::open(&dir).unwrap(); let root = if cfg!(windows) { "C:\\Volumes\\gfx" } else { "/Volumes/gfx" }; let job = Path::new(root).join("jobs").join("0001_Test"); let local = Path::new(if cfg!(windows) { "C:\\local\\job" } else { "/local/job" });
        db.subscribe(&job, "").unwrap(); db.subscribe(local, "").unwrap(); db.add_column(&job, "3d", "Status", "text", &[]).unwrap(); let uuid = db.columns(&job, "3d")[0].uuid.clone();
        db.set_field(&job, &job.join("3d").join("sh010"), "3d", &uuid, serde_json::json!("WIP")).unwrap();
        let maps = [Mapping::new("C:\\Volumes\\gfx", "/Volumes/gfx")]; let n = db.classify(&maps).unwrap(); assert!(n >= 4, "{n}");
        let stored: Vec<String> = db.conn.prepare("SELECT job_path FROM subscriptions ORDER BY job_path").unwrap().query_map([], |r| r.get(0)).unwrap().flatten().collect();
        assert_eq!(stored.len(), 2); assert!(stored[0].starts_with("native:"), "what is not under the share stays this machine's"); assert_eq!(stored[1], format!("vol:{}/jobs/0001_Test", maps[0].uuid));
        let (item, jobp): (String, String) = db.conn.query_row("SELECT item_path, job_path FROM item_metadata", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap(); assert_eq!(item, format!("vol:{}/jobs/0001_Test/3d/sh010", maps[0].uuid)); assert_eq!(jobp, stored[1]);
        assert_eq!(db.classify(&maps).unwrap(), 0, "done once");
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn what_is_shared_waits_for_the_mesh_and_arrives_as_1_4_2_sends_it() {
        // (two databases stand for two machines; a path under a share is given to both as its identity)
        let (da, db_) = (temp("mesh-a"), temp("mesh-b")); let (a, b) = (Db::open(&da).unwrap(), Db::open(&db_).unwrap());
        let job = "vol:1111/jobs/0001_Test"; let item = format!("{job}/3d/sh010");
        // a column made on A, as its message
        a.conn.execute("INSERT INTO column_definitions (job_path, folder_name, column_name, column_type, column_order, column_width, is_visible, modified_time, column_uuid) VALUES (?1, '3d', 'Status', 'dropdown', 0, 120.0, 1, 1000, 'u-status')", [job]).unwrap();
        let id = a.conn.last_insert_rowid(); a.set_options(id, &[("WIP".into(), Some("#e6b422".into())), ("Final".into(), None)], 1000).unwrap(); a.note_col(id);
        assert_eq!(a.pending().iter().map(|p| (p.1.as_str(), p.2.as_str())).collect::<Vec<_>>(), [("col", "u-status")]);
        let msg = a.col_wire("u-status").unwrap(); assert_eq!(msg["action"], "col_add"); assert!(msg["def"].is_string(), "the definition travels as a string");
        let def: serde_json::Value = serde_json::from_str(msg["def"].as_str().unwrap()).unwrap(); assert_eq!((def["columnName"].as_str(), def["columnUuid"].as_str(), def["jobPath"].as_str(), def["options"][0]["name"].as_str()), (Some("Status"), Some("u-status"), Some(job), Some("WIP"))); assert!(def.get("templateHash").is_none());
        assert!(b.apply_table(&msg).unwrap()); assert!(!b.apply_table(&msg).unwrap(), "the same again: not newer, ignored");
        let (name, opts): (String, i64) = b.conn.query_row("SELECT column_name, (SELECT count(*) FROM column_options o WHERE o.column_id = d.id) FROM column_definitions d WHERE column_uuid = 'u-status'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap(); assert_eq!((name.as_str(), opts), ("Status", 2));
        // an item's row on A, as its message; B takes it; an older one does not replace a newer
        a.conn.execute("INSERT INTO item_metadata (item_path, job_path, folder_name, metadata_json, is_tracked, modified_time) VALUES (?1, ?2, '3d', '{\"u-status\":\"WIP\"}', 1, 2000)", rusqlite::params![item, job]).unwrap(); a.note("meta", &item, None); a.note("meta", &item, None);
        assert_eq!(a.pending().iter().filter(|p| p.1 == "meta").count(), 1, "one entry a row, however often it changed");
        let msg = a.meta_wire(&item).unwrap(); assert_eq!((msg["item_path"].as_str(), msg["metadata"].as_str(), msg["is_tracked"].as_bool(), msg["modified_time"].as_i64()), (Some(item.as_str()), Some("{\"u-status\":\"WIP\"}"), Some(true), Some(2000)));
        assert!(b.apply_meta(&msg).unwrap());
        let mut older = msg.clone(); older["modified_time"] = 1500.into(); older["metadata"] = "{}".into(); assert!(!b.apply_meta(&older).unwrap(), "older than ours: left");
        let mut same = msg.clone(); same["metadata"] = "{\"u-status\":\"Final\"}".into(); assert!(b.apply_meta(&same).unwrap(), "not older (the same time): it replaces (1.4.2: >=)");
        assert_eq!(b.conn.query_row("SELECT metadata_json FROM item_metadata WHERE item_path = ?1", [&item], |r| r.get::<_, String>(0)).unwrap(), "{\"u-status\":\"Final\"}");
        // what is this machine's own is never queued; what 1.4.2 ignores is ignored; anything else is an error
        a.note("meta", "native:mac//Users/a/x", None); assert_eq!(a.pending_count(), 2);
        assert!(!b.apply_table(&serde_json::json!({ "action": "sub_add" })).unwrap()); assert!(b.apply_table(&serde_json::json!({ "action": "nonsense" })).is_err());
        assert!(b.apply_table(&serde_json::json!({ "action": "col_delete", "column_uuid": "u-status", "deleted_at": 5000 })).unwrap());
        let p = a.pending(); a.pending_done(p[0].0); assert_eq!(a.pending_count(), 1);
        let _ = std::fs::remove_dir_all(&da); let _ = std::fs::remove_dir_all(&db_);
    }
    #[test]
    fn a_snapshot_carries_what_is_shared_and_two_machines_that_only_share_a_folder_converge() {
        let (da, db_, dir) = (temp("snap-a"), temp("snap-b"), temp("snap-files")); let (a, b) = (Db::open(&da).unwrap(), Db::open(&db_).unwrap()); std::fs::create_dir_all(&dir).unwrap();
        let job = "vol:1111/jobs/0001_Test"; let (i1, i2) = (format!("{job}/3d/sh010"), format!("{job}/3d/sh020"));
        let col = |d: &Db, name: &str, uuid: &str, time: i64, opts: &[&str]| { d.conn.execute("INSERT INTO column_definitions (job_path, folder_name, column_name, column_type, column_order, column_width, is_visible, modified_time, column_uuid) VALUES (?1, '3d', ?2, 'dropdown', 0, 120.0, 1, ?3, ?4)", rusqlite::params![job, name, time, uuid]).unwrap(); let id = d.conn.last_insert_rowid(); d.set_options(id, &opts.iter().map(|o| (o.to_string(), None)).collect::<Vec<_>>(), time).unwrap(); };
        let row = |d: &Db, item: &str, doc: &str, time: i64| { d.conn.execute("INSERT OR REPLACE INTO item_metadata (item_path, job_path, folder_name, metadata_json, is_tracked, modified_time) VALUES (?1, ?2, '3d', ?3, 1, ?4)", rusqlite::params![item, job, doc, time]).unwrap(); };
        let doc = |d: &Db, item: &str| d.conn.query_row("SELECT metadata_json FROM item_metadata WHERE item_path = ?1", [item], |r| r.get::<_, String>(0)).ok();
        // nothing shared: nothing is written
        let f = dir.join("s1.db"); assert!(!a.write_snapshot(&f, None).unwrap()); a.conn.execute("INSERT INTO item_metadata (item_path, job_path, folder_name) VALUES ('native:mac//x/i', 'native:mac//x', '3d')", []).unwrap(); assert!(!a.write_snapshot(&f, None).unwrap(), "this machine's own rows are never in a snapshot");
        // A has a column and a row; B has another row, and an OLDER copy of A's
        col(&a, "Status", "u-status", 1000, &["WIP", "Final"]); row(&a, &i1, "{\"u-status\":\"WIP\"}", 2000); row(&b, &i1, "{}", 1500); row(&b, &i2, "{\"u-status\":\"Final\"}", 3000);
        assert!(a.write_snapshot(&f, None).unwrap());
        // the file is 1.4.2's: four tables, the subscriptions' empty
        { let c = Connection::open(&f).unwrap(); let names: Vec<String> = c.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name").unwrap().query_map([], |r| r.get(0)).unwrap().flatten().collect(); assert_eq!(names, ["column_definitions", "column_options", "item_metadata", "subscriptions"]);
            assert_eq!(c.query_row("SELECT count(*) FROM subscriptions", [], |r| r.get::<_, i64>(0)).unwrap(), 0); assert_eq!(c.query_row("SELECT count(*) FROM item_metadata", [], |r| r.get::<_, i64>(0)).unwrap(), 1); }
        // B merges A's: the newer row replaces its older one, the column and its options arrive, its own row stays
        assert!(b.merge_snapshot(&f, None).unwrap() >= 2); assert_eq!(doc(&b, &i1).as_deref(), Some("{\"u-status\":\"WIP\"}")); assert_eq!(doc(&b, &i2).as_deref(), Some("{\"u-status\":\"Final\"}"));
        assert_eq!(b.conn.query_row("SELECT count(*) FROM column_options o JOIN column_definitions d ON d.id = o.column_id WHERE d.column_uuid = 'u-status'", [], |r| r.get::<_, i64>(0)).unwrap(), 2);
        assert_eq!(b.merge_snapshot(&f, None).unwrap(), 0, "the same again changes nothing");
        // B writes ITS snapshot (now the union), A merges it: both hold the same - through the folder alone, no message between them
        let g = dir.join("s2.db"); assert!(b.write_snapshot(&g, None).unwrap()); assert!(a.merge_snapshot(&g, None).unwrap() >= 1); assert_eq!(doc(&a, &i2).as_deref(), Some("{\"u-status\":\"Final\"}")); assert_eq!(doc(&a, &i1), doc(&b, &i1));
        // an older row in a snapshot does not replace a newer one here; a column in the trash there is in the trash here even when ours is newer
        row(&a, &i1, "{\"u-status\":\"Final\"}", 9000); b.conn.execute("UPDATE column_definitions SET trashed_at = 5000 WHERE column_uuid = 'u-status'", []).unwrap(); a.conn.execute("UPDATE column_definitions SET modified_time = 8000 WHERE column_uuid = 'u-status'", []).unwrap();
        let h = dir.join("s3.db"); b.write_snapshot(&h, None).unwrap(); a.merge_snapshot(&h, None).unwrap(); assert_eq!(doc(&a, &i1).as_deref(), Some("{\"u-status\":\"Final\"}"));
        assert_eq!(a.conn.query_row("SELECT trashed_at FROM column_definitions WHERE column_uuid = 'u-status'", [], |r| r.get::<_, Option<i64>>(0)).unwrap(), Some(5000));
        // the playground: only what is under it is written and merged
        row(&a, "vol:1111/jobs/other/3d/x", "{}", 100); let k = dir.join("s4.db"); a.write_snapshot(&k, Some(job)).unwrap(); let c = Connection::open(&k).unwrap(); assert_eq!(c.query_row("SELECT count(*) FROM item_metadata WHERE job_path != ?1", [job], |r| r.get::<_, i64>(0)).unwrap(), 0); drop(c);
        // a file that is not whole is refused, and nothing is changed
        let bad = dir.join("bad.db"); let bytes = std::fs::read(&f).unwrap(); std::fs::write(&bad, &bytes[..bytes.len() / 2]).unwrap(); assert!(b.merge_snapshot(&bad, None).is_err()); std::fs::write(&bad, b"not a database").unwrap(); assert!(b.merge_snapshot(&bad, None).is_err());
        assert_eq!(b.pending_count(), 0, "what a snapshot brings is not sent on as a change of this machine's"); assert_eq!(a.pending().iter().map(|p| p.2.as_str()).collect::<Vec<_>>(), [i1.as_str()], "but a row of ours that is NEWER than the snapshot's is: the others get it");
        for d in [da, db_, dir] { let _ = std::fs::remove_dir_all(d); }
    }
    #[test]
    fn two_machines_that_change_different_cells_of_one_item_both_keep_their_edit() {
        let (da, db_) = (temp("cell-a"), temp("cell-b")); let (a, b) = (Db::open(&da).unwrap(), Db::open(&db_).unwrap()); let job = "vol:1111/jobs/0001_Test"; let item = format!("{job}/3d/sh010");
        let put = |d: &Db, doc: &str, tracked: bool, time: i64| { d.conn.execute("INSERT OR REPLACE INTO item_metadata (item_path, job_path, folder_name, metadata_json, is_tracked, modified_time) VALUES (?1, ?2, '3d', ?3, ?4, ?5)", rusqlite::params![item, job, doc, tracked as i64, time]).unwrap(); };
        let get = |d: &Db| d.conn.query_row("SELECT metadata_json, is_tracked FROM item_metadata WHERE item_path = ?1", [&item], |r| Ok((serde_json::from_str::<serde_json::Value>(&r.get::<_, String>(0)?).unwrap(), r.get::<_, i64>(1)? != 0))).unwrap();
        let send = |from: &Db, to: &Db| -> bool { let m = from.meta_wire(&item).unwrap(); let took = to.apply_meta(&m).unwrap(); from.base_from_wire(&m); took };
        let drain = |d: &Db| { for p in d.pending() { d.pending_done(p.0); } };
        // both start from the same row
        put(&a, "{\"status\":\"WIP\",\"artist\":\"Zoe\"}", false, 1000); assert!(send(&a, &b)); assert_eq!(get(&b).0, serde_json::json!({ "status": "WIP", "artist": "Zoe" })); drain(&a); drain(&b);
        // A changes the status, B the artist - at about the same time, B a little later
        put(&a, "{\"status\":\"Final\",\"artist\":\"Zoe\"}", false, 2000); put(&b, "{\"status\":\"WIP\",\"artist\":\"Bea\"}", true, 2100);
        // B's row reaches A: only the artist (and the star) is B's change - A keeps its status
        assert!(send(&b, &a)); assert_eq!(get(&a), (serde_json::json!({ "status": "Final", "artist": "Bea" }), true), "1.4.2 would have left A with B's whole row: the status lost");
        // what A holds is more than what arrived: it waits to be sent on; B takes it and both hold the same
        assert_eq!(a.pending().iter().filter(|p| p.1 == "meta").count(), 1); assert!(send(&a, &b)); assert_eq!(get(&b), get(&a)); drain(&a);
        assert_eq!(b.pending().iter().filter(|p| p.1 == "meta").count(), 0, "it came to rest: nothing goes back and forth");
        // the same cell changed on both: the later row's value, without a word
        put(&a, "{\"status\":\"Hold\",\"artist\":\"Bea\"}", true, 3000); put(&b, "{\"status\":\"Kill\",\"artist\":\"Bea\"}", true, 3500);
        assert!(!send(&a, &b), "A's is older: B keeps its own"); assert_eq!(get(&b).0["status"], "Kill"); assert!(send(&b, &a)); assert_eq!(get(&a).0["status"], "Kill");
        // a cell cleared (null) on one side is a change like any other; a row that brings nothing new changes nothing
        drain(&a); drain(&b); put(&a, "{\"status\":null,\"artist\":\"Bea\"}", true, 4000); assert!(send(&a, &b)); assert!(get(&b).0["status"].is_null()); assert!(!send(&a, &b));
        // through a snapshot the same merge: B changed the artist meanwhile, A's snapshot brings a new status - both survive
        put(&a, "{\"status\":\"Done\",\"artist\":\"Bea\"}", true, 5000); put(&b, "{\"status\":null,\"artist\":\"Cy\"}", true, 4900);
        let f = da.join("s.db"); a.write_snapshot(&f, None).unwrap(); b.merge_snapshot(&f, None).unwrap(); assert_eq!(get(&b).0, serde_json::json!({ "status": "Done", "artist": "Cy" }));
        // TWO MACHINES THAT ONLY SHARE THE FOLDER, each changing another cell of an item neither had synced, each
        // publishing its snapshot before it saw the other's (found in a run of two nodes: one edit was lost). A fills
        // in the notes; B, a moment EARLIER by its clock, fills in the frames and stars the item.
        let item2 = format!("{job}/3d/sh020"); let put2 = |d: &Db, doc: &str, tracked: bool, time: i64| { d.conn.execute("INSERT OR REPLACE INTO item_metadata (item_path, job_path, folder_name, metadata_json, is_tracked, modified_time) VALUES (?1, ?2, '3d', ?3, ?4, ?5)", rusqlite::params![item2, job, doc, tracked as i64, time]).unwrap(); };
        let get2 = |d: &Db| d.conn.query_row("SELECT metadata_json, is_tracked FROM item_metadata WHERE item_path = ?1", [&item2], |r| Ok((serde_json::from_str::<serde_json::Value>(&r.get::<_, String>(0)?).unwrap(), r.get::<_, i64>(1)? != 0))).unwrap();
        put2(&a, "{\"notes\":\"FromA\"}", false, 7000); put2(&b, "{\"frames\":48}", true, 6900);
        let (sa, sb) = (da.join("sa.db"), db_.join("sb.db")); a.write_snapshot(&sa, None).unwrap(); b.write_snapshot(&sb, None).unwrap();
        // each merges the other's; then each publishes again and merges again: both hold both edits and the star
        a.merge_snapshot(&sb, None).unwrap(); b.merge_snapshot(&sa, None).unwrap(); a.write_snapshot(&sa, None).unwrap(); b.write_snapshot(&sb, None).unwrap(); a.merge_snapshot(&sb, None).unwrap(); b.merge_snapshot(&sa, None).unwrap();
        let both = (serde_json::json!({ "notes": "FromA", "frames": 48 }), true); assert_eq!(get2(&a), both); assert_eq!(get2(&b), both);
        // and it comes to rest: another round changes nothing
        a.write_snapshot(&sa, None).unwrap(); b.write_snapshot(&sb, None).unwrap(); assert_eq!(a.merge_snapshot(&sb, None).unwrap() + b.merge_snapshot(&sa, None).unwrap(), 0);
        for d in [da, db_] { let _ = std::fs::remove_dir_all(d); }
    }
    #[test]
    fn identities_of_this_machine_only() {
        let p = if cfg!(windows) { PathBuf::from("C:\\Users\\a\\Desktop") } else { PathBuf::from("/Users/a/Desktop") };
        assert_eq!(to_native(&to_identity(&p)), Some(p)); assert_eq!(to_native(if cfg!(windows) { "native:mac//x" } else { "native:win/C:\\x" }), None); assert_eq!(to_native("vol:1234/a/b"), None); assert_eq!(to_native("/plain"), None);
    }
}
