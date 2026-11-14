//! Reading a minNotes 2.0 file (`mnd2`, schema ≤ 2; `docs/phase3/step1-core-spec.md` §3). Read only, and only ever
//! pointed at a staged copy — never at a file where it lives (row 272).
use std::path::Path;
use rusqlite::{Connection, OpenFlags};
use mn_doc::legacy::OldBlock;

#[derive(Debug, PartialEq, Eq)]
pub enum Gate {
    /// Not a database, or one without minNotes' marks.
    NotMinNotes,
    /// A 1.x file (row 283: opened in the old app and saved as 2.0 first), or some other older format.
    Earlier,
    /// A file of the NEW format (or a later one): not for this reader.
    Newer,
}
impl std::fmt::Display for Gate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self { Gate::NotMinNotes => "This file isn't a minNotes document.", Gate::Earlier => "This document was made with an earlier minNotes and can't be opened.", Gate::Newer => "This document was made with a newer minNotes and can't be opened." })
    }
}

/// One page of the old file: its element id and its blocks in the page's order.
#[derive(Debug)]
pub struct Page { pub id: String, pub blocks: Vec<OldBlock> }

/// `doc_meta.format` and `schema_version` ("" and 0 when missing), or None when the file is not a usable database.
pub(crate) fn marks(db: &Connection) -> Option<(String, i64)> {
    db.query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get::<_, i64>(0)).ok()?;
    Some(db.query_row("SELECT coalesce(format, ''), coalesce(schema_version, 0) FROM doc_meta WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap_or((String::new(), 0)))
}
fn has_column(db: &Connection, table: &str, column: &str) -> bool {
    db.prepare(&format!("SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?1")).and_then(|mut s| s.exists([column])).unwrap_or(false)
}

/// Reads every page of a 2.0 file. Blocks that belong to no page go to the first one, as the old app put them; equal
/// ranks keep the order the rows were written in.
pub fn read_pages(path: &Path) -> Result<Vec<Page>, Gate> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|_| Gate::NotMinNotes)?;
    match marks(&db).ok_or(Gate::NotMinNotes)? {
        (f, v) if f == "mnd2" && v <= 2 => {}
        (f, _) if f == "mnd2" => return Err(Gate::Newer),
        (_, v) if v > 0 => return Err(Gate::Earlier),
        _ => return Err(Gate::NotMinNotes),
    }
    let ids: Vec<String> = db.prepare("SELECT id FROM elements WHERE kind = 'page' ORDER BY z, created, id")
        .and_then(|mut s| s.query_map([], |r| r.get(0))?.collect()).unwrap_or_default();
    let paged = has_column(&db, "blocks", "page_id");
    let read = |sql: &str, arg: Option<&str>| -> Vec<OldBlock> {
        let Ok(mut s) = db.prepare(sql) else { return vec![] };
        let map = |r: &rusqlite::Row<'_>| Ok(OldBlock { id: r.get(0)?, depth: r.get::<_, Option<i64>>(1)?.unwrap_or(0), kind: r.get(2)?, attrs: r.get(3)?, content: r.get::<_, Option<String>>(4)?.unwrap_or_default() });
        match arg { Some(a) => s.query_map([a], map).map(|x| x.flatten().collect()), None => s.query_map([], map).map(|x| x.flatten().collect()) }.unwrap_or_default()
    };
    const COLS: &str = "SELECT id, depth, type, attrs, content FROM blocks";
    if ids.is_empty() || !paged { return Ok(vec![Page { id: ids.first().cloned().unwrap_or_default(), blocks: read(&format!("{COLS} ORDER BY rank, rowid"), None) }]) }
    let mut pages: Vec<Page> = ids.iter().map(|id| Page { id: id.clone(), blocks: read(&format!("{COLS} WHERE page_id = ?1 ORDER BY rank, rowid"), Some(id)) }).collect();
    let mut orphans = read(&format!("{COLS} WHERE page_id IS NULL OR page_id NOT IN (SELECT id FROM elements WHERE kind = 'page') ORDER BY rank, rowid"), None);
    pages[0].blocks.append(&mut orphans);
    Ok(pages)
}

/// Is this (a STAGED copy of) a minNotes 2.0 document - the one kind of earlier file that is imported (row 377)?
pub fn is_v2(path: &Path) -> bool {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok().and_then(|db| marks(&db)).is_some_and(|(f, v)| f == "mnd2" && v <= 2)
}

/// What lies on a 2.0 file's canvas beside its pages, counted by kind: an import of the pages leaves these behind, and says so.
pub fn leftovers(path: &Path) -> Vec<(String, i64)> {
    let Ok(db) = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) else { return vec![] };
    db.prepare("SELECT kind, count(*) FROM elements WHERE kind <> 'page' GROUP BY kind ORDER BY kind").and_then(|mut s| s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect()).unwrap_or_default()
}

/// A small 2.0 document with synthetic content, as the old application writes one (its DDL): two pages - a heading, text
/// with a mark, a task, a table of two columns, a picture kept with the document, a file named by a path - and three
/// things on the canvas outside the pages. For the import's tests and scripted runs; never a user's file.
pub fn write_sample(path: &Path) -> rusqlite::Result<()> {
    let db = Connection::open(path)?;
    db.execute_batch("CREATE TABLE blocks (id TEXT PRIMARY KEY, rank TEXT NOT NULL, depth INTEGER NOT NULL DEFAULT 0, type TEXT NOT NULL, attrs TEXT, content TEXT NOT NULL DEFAULT '', created INTEGER, modified INTEGER, page_id TEXT);
        CREATE TABLE doc_meta (id INTEGER PRIMARY KEY CHECK (id = 1), title TEXT, schema_version INTEGER, app_version TEXT, created INTEGER, modified INTEGER, last_cursor TEXT, page_width INTEGER, format TEXT);
        CREATE TABLE elements (id TEXT PRIMARY KEY, kind TEXT NOT NULL, x REAL NOT NULL DEFAULT 0, y REAL NOT NULL DEFAULT 0, w REAL NOT NULL DEFAULT 0, h REAL NOT NULL DEFAULT 0, z INTEGER NOT NULL DEFAULT 0, parent_id TEXT, json TEXT NOT NULL DEFAULT '{}', created INTEGER, modified INTEGER);
        INSERT INTO doc_meta (id, schema_version, app_version, created, modified, format) VALUES (1, 2, '2.0.6', 1, 1, 'mnd2');")?;
    let id = |n: u128| mn_doc::BlockId::from_u128((0x0190_0000_0000u128 << 80) + n).to_string();
    db.execute("INSERT INTO elements (id, kind, z, created) VALUES (?1, 'page', 0, 1), (?2, 'page', 1, 2), (?3, 'media', 2, 3), (?4, 'media', 3, 4), (?5, 'text', 4, 5)", rusqlite::params![id(1), id(2), id(3), id(4), id(5)])?;
    let rows: &[(u128, &str, i64, &str, Option<&str>, &str, u128)] = &[
        (10, "B", 0, "heading", Some(r#"{"level":1}"#), "Shot list", 1),
        (11, "D", 0, "paragraph", Some(r#"{"spans":[{"s":4,"e":8,"k":"bold"}]}"#), "The reel is locked on Friday.", 1),
        (12, "F", 0, "task_item", Some(r#"{"state":2}"#), "Send the harbour shots", 1),
        (13, "H", 0, "task_item", None, "Book the mix", 1),
        (14, "J", 0, "split", Some(r#"{"ratios":[0.5,0.5]}"#), "", 1),
        (15, "L", 0, "paragraph", Some(r#"{"cell":0}"#), "left column", 1),
        (16, "N", 0, "paragraph", Some(r#"{"cell":1}"#), "right column", 1),
        (17, "P", 0, "media", None, r#"{"h":90,"name":"kept.png","src":".minnotes/kept.png","w":160}"#, 1),
        (20, "B", 0, "paragraph", None, "Delivery notes", 2),
        (21, "D", 0, "list_item", None, "ProRes 4444", 2),
        (22, "F", 1, "list_item", None, "Rec. 709", 2),
        (23, "H", 0, "media", None, r#"{"ext":"xlsx","kind":"file","name":"Budget.xlsx","src":"/nowhere/Budget.xlsx"}"#, 2),
    ];
    for (n, rank, depth, kind, attrs, content, page) in rows { db.execute("INSERT INTO blocks (id, rank, depth, type, attrs, content, created, modified, page_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, 1, ?7)", rusqlite::params![id(*n), rank, depth, kind, attrs, content, id(*page)])?; }
    Ok(())
}
