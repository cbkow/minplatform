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
