//! The move from UFB 1.4.x (canon rows 457, 519 point 3, 532): what an installed 1.4.x keeps in its data folder -
//! `settings.json` and the database `ufb_v5.db` - is taken into this build's own, ONCE.
//!
//! - **The database.** In the release, whose data folder IS 1.4.x's, it is opened in place - after one copy of it is
//!   put beside it (`ufb_v5.db.before-2.0`); the schema is 1.4.2's and what this build adds are tables of its own, so
//!   1.4.2 can still open it. In any other build the database is COPIED into this build's folder (a whole, consistent
//!   copy made by SQLite itself, also while 1.4.x runs) - and only into an empty one: a database of this build that
//!   already holds jobs or items is never written over.
//! - **Path mappings** (`pathMappings`): each pair becomes a mapping of the Settings card (row 517) - the same pair,
//!   so the same identity and the same metadata. A macOS side written from the home folder (`~/...`) is spelled out.
//! - **Bookmarks** (the database's table): one marked as a project folder becomes a saved folder share whose folders
//!   are jobs; any other a favourite of the sidebar. One that leads nowhere on this machine is counted and left.
//! - **Sync** (`meshSync`): the farm's folder and this machine's name. Whether sync is ON is taken over by the
//!   release alone - any other build speaks a mesh of its own and is switched on by hand.
//! - Not taken: the window's place, the open tabs, the views kept per folder, the thumbnails (made anew).
//! - macOS, in place: the EMPTY folders 1.4.x's sync mounts appeared at (`~/ufb/mounts/...`) are taken away.
//!
//! `UFB --import-1.4 <folder>` does it by hand and says what it did, in numbers (a look at a COPY of real data).
use std::path::{Path, PathBuf};

#[derive(Debug, Default, PartialEq)]
pub struct Imported { pub mappings: usize, pub mappings_there: usize, pub favorites: usize, pub jobs_folders: usize, pub bookmarks_lost: usize, pub sync: bool, pub farm: bool,
    /// (subscriptions, items, columns) of the database taken over; none: no database was there
    pub rows: Option<(i64, i64, i64)>, pub notes: Vec<String> }

fn home() -> PathBuf { crate::places::home() }
fn spelled(mac: &str) -> String { match mac.strip_prefix("~/") { Some(rest) => home().join(rest).to_string_lossy().into_owned(), None if mac == "~" => home().to_string_lossy().into_owned(), None => mac.to_string() } }

/// Take over what lies in `from` (1.4.x's data folder, or a copy of it). Must run before this build's database is opened.
pub fn import(from: &Path) -> Result<Imported, String> {
    let mut done = Imported::default();
    let dir = crate::identity::data_dir().ok_or("this build has no data folder")?; std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let same = std::fs::canonicalize(from).ok() == std::fs::canonicalize(&dir).ok();
    // -- the settings: the mappings first (every stored path is read by them)
    let text = std::fs::read_to_string(from.join("settings.json")).or_else(|_| std::fs::read_to_string(from.join("settings.json.bak"))).ok();
    let v: serde_json::Value = text.as_deref().and_then(|t| serde_json::from_str(t).ok()).unwrap_or(serde_json::Value::Null);
    if v.is_null() { done.notes.push("no settings file of 1.4.x was found: the mappings and sync were not taken".into()); }
    let mut manual = crate::shares::load_manual(); let saved = crate::shares::load();
    let known = |manual: &[crate::shares::Manual], win: &str, mac: &str| { let id = crate::db::volume_uuid(win, mac); manual.iter().any(|m| crate::db::volume_uuid(&m.win, &m.mac) == id) || saved.iter().any(|s| crate::db::volume_uuid(&s.win, &s.mac) == id) };
    for m in v["pathMappings"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
        let (win, mac) = (m["win"].as_str().unwrap_or("").trim().to_string(), spelled(m["mac"].as_str().unwrap_or("").trim())); if win.is_empty() || mac.is_empty() { continue }
        if known(&manual, &win, &mac) { done.mappings_there += 1; continue }
        manual.push(crate::shares::Manual { name: m["label"].as_str().unwrap_or("").to_string(), win, mac, on: m["enabled"].as_bool().unwrap_or(true) }); done.mappings += 1;
    }
    if done.mappings > 0 { crate::shares::save_manual(&manual); }
    let mounts = crate::shares::mounts(); let states: Vec<crate::shares::State> = saved.iter().map(|s| crate::shares::resolve(s, &mounts, &|p| p.is_dir())).collect();
    crate::db::set_mappings(crate::shares::mappings_now(&saved, &states, &mounts));
    // -- the database
    let (src, dst) = (from.join("ufb_v5.db"), dir.join("ufb_v5.db"));
    if src.is_file() {
        let quoted = |p: &Path| p.to_string_lossy().replace('\'', "''");
        let open_ro = |p: &Path| rusqlite::Connection::open_with_flags(p, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI).map_err(|e| format!("the database of 1.4.x could not be opened: {e}"));
        if same {
            let keep = dir.join("ufb_v5.db.before-2.0");
            if !keep.exists() { open_ro(&src)?.execute(&format!("VACUUM INTO '{}'", quoted(&keep)), []).map_err(|e| format!("the copy kept of 1.4.x's database could not be made: {e}"))?; }
        } else {
            if dst.is_file() { let n: i64 = open_ro(&dst).and_then(|c| c.query_row("SELECT (SELECT count(*) FROM subscriptions) + (SELECT count(*) FROM item_metadata)", [], |r| r.get(0)).map_err(|e| e.to_string()))?;
                if n > 0 { return Err("this build's own database already holds jobs or items: it is not written over (move it away, or start with another data folder)".into()) }
                for tail in ["", "-wal", "-shm"] { let _ = std::fs::remove_file(dir.join(format!("ufb_v5.db{tail}"))); } }
            open_ro(&src)?.execute(&format!("VACUUM INTO '{}'", quoted(&dst)), []).map_err(|e| format!("the database could not be copied: {e}"))?;
        }
        let c = open_ro(&dst)?;
        done.rows = c.query_row("SELECT (SELECT count(*) FROM subscriptions), (SELECT count(*) FROM item_metadata), (SELECT count(*) FROM column_definitions)", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).ok();
        // -- the bookmarks: a project folder is a jobs folder; any other a favourite
        let maps = crate::db::mappings(); let mut shares = saved.clone(); let mut changed = false;
        let marks: Vec<(String, String, bool)> = c.prepare("SELECT path, COALESCE(display_name, ''), COALESCE(is_project_folder, 0) FROM bookmarks ORDER BY id").and_then(|mut st| st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0))).map(|rows| rows.flatten().collect())).unwrap_or_default();
        for (path, name, project) in marks {
            let Some(native) = crate::db::native_in(&path, &maps) else { done.bookmarks_lost += 1; continue };
            if project {
                if shares.iter().any(|s| Path::new(&s.local) == native) { continue }
                if let Ok(mut sh) = crate::shares::parse(&native.to_string_lossy()) { if !name.is_empty() { sh.name = name; } sh.jobs = true; shares.push(sh); changed = true; done.jobs_folders += 1; }
            } else if crate::places::add_favorite(&native, &name) { done.favorites += 1; }
        }
        if changed { crate::shares::save(&shares); }
    } else { done.notes.push("no database of 1.4.x was found: nothing of the metadata was taken".into()); }
    // -- sync: where the farm is and what this machine is called in it; ON only where the mesh is 1.4.x's own
    let (farm, node) = (spelled(v["meshSync"]["farmPath"].as_str().unwrap_or("").trim()), v["meshSync"]["nodeId"].as_str().unwrap_or("").trim().to_string());
    if !farm.is_empty() { let on = v["meshSync"]["enabled"].as_bool().unwrap_or(false) && crate::identity::kind() == crate::identity::Kind::Release;
        crate::settings::with(|s| { s.set_str("sync/farm", &farm); if !node.is_empty() { s.set_str("sync/node", &node); } if on { s.set_bool("sync/on", true); } }); done.farm = true; done.sync = on; }
    // -- macOS, the move in place: the folders 1.4.x's sync mounts appeared at (`~/ufb/mounts/<share>`) are taken away
    // when they are EMPTY - a folder that still holds something (a mount that is up) is never touched
    // (the release alone does it: a move tried on a copy must leave the real home folder as it is)
    if same && cfg!(target_os = "macos") && crate::identity::kind() == crate::identity::Kind::Release { let base = home().join("ufb"); let mounts = base.join("mounts");
        if let Ok(rd) = std::fs::read_dir(&mounts) { for e in rd.flatten() { let _ = std::fs::remove_dir(e.path()); } }
        let _ = std::fs::remove_dir(&mounts); let _ = std::fs::remove_dir(&base); }
    crate::settings::with(|s| s.set_str("migrate/from_1_4", &crate::format::now_secs().to_string()));
    Ok(done)
}
/// What was taken over, in numbers (no name of a job, a share or a folder is said).
pub fn report(d: &Imported) -> String {
    let mut s = format!("path mappings: {} taken, {} were there already\nfavourites: {}; jobs folders: {}; bookmarks that lead nowhere on this machine: {}\nsync: the farm's folder {}; switched {}",
        d.mappings, d.mappings_there, d.favorites, d.jobs_folders, d.bookmarks_lost, if d.farm { "taken" } else { "none set in 1.4.x" }, if d.sync { "ON (as in 1.4.x)" } else { "off (this build's mesh is its own: switch it on in Settings)" });
    match d.rows { Some((subs, items, cols)) => s.push_str(&format!("\nthe database: {subs} subscribed jobs, {items} items with metadata, {cols} columns")), None => s.push_str("\nthe database: none") }
    for n in &d.notes { s.push_str(&format!("\nnote: {n}")); }
    s
}
