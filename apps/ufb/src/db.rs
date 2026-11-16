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
/// A path of this machine in 1.4.2's storage form (no volume is registered yet: `native:`).
pub fn to_identity(p: &Path) -> String { format!("native:{OS}/{}", p.to_string_lossy()) }
/// The path of this machine a stored identity names; none for another system's, and for a volume's (not yet).
pub fn to_native(id: &str) -> Option<PathBuf> { let (os, path) = id.strip_prefix("native:")?.split_once('/')?; (os == OS && !path.is_empty()).then(|| PathBuf::from(path)) }

impl Db {
    /// Open (or make) the database in a folder, as 1.4.2 opens its own: write-ahead log, foreign keys, a wait on a busy file.
    pub fn open(dir: &Path) -> Result<Db, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let conn = Connection::open(dir.join("ufb_v5.db")).map_err(|e| e.to_string())?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000; PRAGMA synchronous=NORMAL;").map_err(|e| e.to_string())?;
        conn.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
        for (table, column, kind) in ADDED {
            let has: bool = conn.prepare(&format!("SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1")).and_then(|mut s| s.exists([column])).map_err(|e| e.to_string())?;
            if !has { conn.execute(&format!("ALTER TABLE {table} ADD COLUMN {column} {kind}"), []).map_err(|e| e.to_string())?; }
        }
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0)).map_err(|e| e.to_string())?;
        if v < USER_VERSION { conn.execute_batch(&format!("PRAGMA user_version = {USER_VERSION}")).map_err(|e| e.to_string())?; }
        Ok(Db { conn })
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
        let id = self.conn.last_insert_rowid(); self.set_options(id, options, now).map_err(|e| e.to_string())?; Ok(id)
    }
    /// A column's name, type and options are changed. Its values stay (they hang on its uuid, which is kept).
    pub fn update_column(&self, id: i64, name: &str, kind: &str, options: &[(String, Option<String>)]) -> Result<(), String> {
        let name = name.trim(); if name.is_empty() { return Err("The column needs a name.".into()) } let now = now_ms();
        let taken: bool = self.conn.prepare("SELECT 1 FROM column_definitions o JOIN column_definitions me ON me.id = ?1 WHERE o.id != me.id AND o.job_path = me.job_path AND o.folder_name = me.folder_name AND lower(o.column_name) = lower(?2) AND o.deleted_at IS NULL AND o.trashed_at IS NULL").and_then(|mut s| s.exists(rusqlite::params![id, name])).map_err(|e| e.to_string())?;
        if taken { return Err(format!("A column named \u{201c}{name}\u{201d} is already there.")) }
        // (this user's own layout hangs on the NAME: it goes with a rename)
        self.conn.execute("UPDATE OR IGNORE column_layout_personal SET column_name = ?2 WHERE (job_path, folder_name, column_name) IN (SELECT job_path, folder_name, column_name FROM column_definitions WHERE id = ?1)", rusqlite::params![id, name]).map_err(|e| e.to_string())?;
        self.conn.execute("UPDATE column_definitions SET column_name = ?1, column_type = ?2, modified_time = ?3, deleted_at = NULL WHERE id = ?4", rusqlite::params![name, kind, now, id]).map_err(|e| e.to_string())?;
        self.set_options(id, options, now).map_err(|e| e.to_string())
    }
    /// An option is added to a column, after its others (made from a cell: a text that was no option yet).
    pub fn add_option(&self, id: i64, name: &str, color: &str) -> Result<(), String> {
        let now = now_ms(); self.conn.execute("INSERT INTO column_options (column_id, option_name, option_color, modified_time, deleted_at) VALUES (?1, ?2, ?3, ?4, NULL)", rusqlite::params![id, name, color, now]).map_err(|e| e.to_string())?;
        self.conn.execute("UPDATE column_definitions SET modified_time = ?1 WHERE id = ?2", rusqlite::params![now, id]).map(|_| ()).map_err(|e| e.to_string())
    }
    /// Into the column trash, or out of it (the values stay; a restore under a name now in use is refused).
    pub fn trash_column(&self, id: i64, on: bool) -> Result<(), String> {
        if !on { let taken: bool = self.conn.prepare("SELECT 1 FROM column_definitions o JOIN column_definitions me ON me.id = ?1 WHERE o.id != me.id AND o.job_path = me.job_path AND o.folder_name = me.folder_name AND lower(o.column_name) = lower(me.column_name) AND o.deleted_at IS NULL AND o.trashed_at IS NULL").and_then(|mut s| s.exists([id])).map_err(|e| e.to_string())?;
            if taken { return Err("A column of that name is in use: rename it first.".into()) } }
        let now = now_ms(); self.conn.execute("UPDATE column_definitions SET trashed_at = ?1, modified_time = ?2 WHERE id = ?3", rusqlite::params![on.then_some(now), now, id]).map(|_| ()).map_err(|e| e.to_string())
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
        let (old, new, now) = (to_identity(from), to_identity(to), now_ms()); let sep = std::path::MAIN_SEPARATOR.to_string(); if old == new { return Ok(0) }
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
        run().map_err(|e| e.to_string())
    }
    /// An item's row is pointed at where its folder went (Relink): its path; and its job and section when the new
    /// place is an item of a job (`<job>/<section>/<item>`) - else they are kept.
    pub fn relink(&self, from: &Path, to: &Path, home: Option<(&Path, &str)>) -> Result<(), String> {
        let now = now_ms(); self.conn.execute("DELETE FROM item_metadata WHERE item_path = ?1", [to_identity(to)]).map_err(|e| e.to_string())?;
        match home { Some((job, folder)) => self.conn.execute("UPDATE item_metadata SET item_path = ?2, job_path = ?3, folder_name = ?4, modified_time = ?5 WHERE item_path = ?1", rusqlite::params![to_identity(from), to_identity(to), to_identity(job), folder, now]),
            None => self.conn.execute("UPDATE item_metadata SET item_path = ?2, modified_time = ?3 WHERE item_path = ?1", rusqlite::params![to_identity(from), to_identity(to), now]) }.map(|_| ()).map_err(|e| e.to_string())
    }
    /// An item's row is taken away (1.4.2's statement: a tombstone).
    pub fn remove_item(&self, item: &Path) -> Result<(), String> { self.conn.execute("UPDATE item_metadata SET modified_time = ?1, deleted_at = ?1 WHERE item_path = ?2", rusqlite::params![now_ms(), to_identity(item)]).map(|_| ()).map_err(|e| e.to_string()) }
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
            rusqlite::params![to_identity(item), to_identity(job), folder, serde_json::Value::Object(values.clone()).to_string(), tracked as i64, now_ms()]).map(|_| ()).map_err(|e| e.to_string())
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
    fn identities_of_this_machine_only() {
        let p = if cfg!(windows) { PathBuf::from("C:\\Users\\a\\Desktop") } else { PathBuf::from("/Users/a/Desktop") };
        assert_eq!(to_native(&to_identity(&p)), Some(p)); assert_eq!(to_native(if cfg!(windows) { "native:mac//x" } else { "native:win/C:\\x" }), None); assert_eq!(to_native("vol:1234/a/b"), None); assert_eq!(to_native("/plain"), None);
    }
}
