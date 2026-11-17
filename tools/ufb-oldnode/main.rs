//! A genuine UFB 1.4.2 mesh node, headless, in isolation (scratch copy of its core; only the epoch, the folder names
//! and the UDP port differ - a TEST mesh). Commands on stdin, answers on stdout.
use std::io::BufRead; use std::sync::Arc;
use ufb_core::{columns::ColumnConfigManager, db::Database, events::MeshEvents, mesh::coordinator::MeshSyncManager, subscription::SubscriptionManager};
struct Say;
impl MeshEvents for Say {
    fn data_refreshed(&self) { println!("EVENT data_refreshed"); }
    fn metadata_changed(&self, job: &str, item: &str, folder: &str) { println!("EVENT metadata_changed {job} | {item} | {folder}"); }
    fn table_changed(&self, v: serde_json::Value) { println!("EVENT table_changed {v}"); }
}
#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).try_init();
    let a: Vec<String> = std::env::args().collect(); let (scratch, farm, node, ctl, data) = (&a[1], a[2].clone(), a[3].clone(), a[4].parse::<u16>().unwrap(), a[5].parse::<u16>().unwrap()); let tags: Vec<String> = a.get(6).map(|t| t.split(',').filter(|x| !x.is_empty()).map(str::to_string).collect()).unwrap_or_default();
    // THE BELT: nothing of this node may lie outside the scratch folder
    let dir = ufb_core::utils::get_app_data_dir(); assert!(dir.starts_with(scratch), "the app-data folder {} is outside {scratch}: HOME is not redirected", dir.display());
    assert_eq!(ufb_core::mesh::MESH_EPOCH, 9006, "not the test epoch");
    std::fs::create_dir_all(&dir).unwrap(); let dbp = ufb_core::utils::get_database_path(); println!("HARNESS db {}", dbp.display());
    let db = Arc::new(Database::open(&dbp).unwrap()); db.run_migrations().unwrap();
    let cols = Arc::new(ColumnConfigManager::new(db.clone())); let subs = SubscriptionManager::new(db.clone());
    let mgr = MeshSyncManager::new(farm, node, ctl, data, tags, db.clone(), cols, Arc::new(Say)); mgr.set_enabled(true).await; println!("HARNESS started");
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    std::thread::spawn(move || { for l in std::io::stdin().lock().lines().map_while(Result::ok) { if tx.send(l).is_err() { break } } });
    while let Some(line) = rx.recv().await {
        let p: Vec<&str> = line.splitn(2, ' ').collect(); let rest = p.get(1).copied().unwrap_or("");
        match p[0] {
            // set <job>|<item>|<folder>|<tracked 0/1>|<json>
            "set" => { let f: Vec<&str> = rest.splitn(5, '|').collect(); if f.len() == 5 { match subs.upsert_item_metadata(f[0], f[1], f[2], f[4], f[3] == "1") { Ok(t) => { mgr.on_metadata_edited(f[0], f[1], f[4], f[2], f[3] == "1", t).await; println!("SET ok {t}"); } Err(e) => println!("SET err {e}") } } }
            "get" => println!("GET {rest} = {:?}", subs.get_item_metadata(rest)),
            "table" => { mgr.on_table_changed(rest).await; println!("TABLE sent"); }
            "status" => { let s = mgr.get_status(); println!("STATUS leader={} leader_id={} peers={} pending={} enabled={} msg={}", s.is_leader, s.leader_id, s.peer_count, s.pending_edits_count, s.is_enabled, s.status_message); }
            "rows" => { let r = db.with_conn(|c| { let mut st = c.prepare("SELECT item_path, metadata_json, is_tracked, modified_time FROM item_metadata ORDER BY item_path")?; let v: Vec<String> = st.query_map([], |r| Ok(format!("{} {} tracked={} t={}", r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?)))?.filter_map(Result::ok).collect(); Ok(v) }); println!("ROWS {r:?}"); }
            "cols" => { let r = db.with_conn(|c| { let mut st = c.prepare("SELECT job_path, folder_name, column_name, column_type, column_uuid FROM column_definitions ORDER BY column_name")?; let v: Vec<String> = st.query_map([], |r| Ok(format!("{} | {} | {} | {} | {:?}", r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, Option<String>>(4)?)))?.filter_map(Result::ok).collect(); Ok(v) }); println!("COLS {r:?}"); }
            "snap" => { mgr.trigger_snapshot().await; println!("SNAP asked"); }
            "quit" => break,
            _ => println!("? {line}"),
        }
    }
    mgr.shutdown().await; println!("HARNESS stopped");
}
