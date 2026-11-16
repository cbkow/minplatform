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
    fn identities_of_this_machine_only() {
        let p = if cfg!(windows) { PathBuf::from("C:\\Users\\a\\Desktop") } else { PathBuf::from("/Users/a/Desktop") };
        assert_eq!(to_native(&to_identity(&p)), Some(p)); assert_eq!(to_native(if cfg!(windows) { "native:mac//x" } else { "native:win/C:\\x" }), None); assert_eq!(to_native("vol:1234/a/b"), None); assert_eq!(to_native("/plain"), None);
    }
}
