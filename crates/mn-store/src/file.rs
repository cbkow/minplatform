//! The collection's database (rows 271, 272, 283): a tree of documents — folders, notes, canvases — and each note's
//! blocks. It runs on the staged WORK copy only ([`crate::staging`]); publishing writes a compact copy for the rename.
//!
//! The mark a 2.0 build reads: it answers "made with a newer minNotes" ONLY for `doc_meta.format = 'mnd2'` with a
//! `schema_version` above its own 2 — any other format name gets its "earlier minNotes" message (spec §3.2). So the
//! mark is `mnd2` with schema 3, and `doc_meta` keeps the 2.0 columns so that build can read the row.
use std::{path::Path, time::{SystemTime, UNIX_EPOCH}};
use rusqlite::{params, Connection, OptionalExtension};
use mn_doc::{BlockId, Doc, Rank};
use crate::{codec, old::{self, Gate}};

pub const FORMAT: &str = "mnd2";
pub const SCHEMA: i64 = 3;

const DDL: &str = "
CREATE TABLE IF NOT EXISTS doc_meta (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  title TEXT, schema_version INTEGER, app_version TEXT,
  created INTEGER, modified INTEGER, last_cursor TEXT, page_width INTEGER, format TEXT
);
CREATE TABLE IF NOT EXISTS documents (
  id TEXT PRIMARY KEY, parent TEXT REFERENCES documents(id) ON DELETE CASCADE,
  rank TEXT NOT NULL, kind TEXT NOT NULL, title TEXT NOT NULL DEFAULT '',
  created INTEGER, modified INTEGER
);
CREATE INDEX IF NOT EXISTS documents_tree ON documents(parent, rank);
CREATE TABLE IF NOT EXISTS note_blocks (
  id TEXT PRIMARY KEY, doc TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
  parent TEXT, rank TEXT NOT NULL, kind TEXT NOT NULL, attrs TEXT, text TEXT NOT NULL DEFAULT '',
  created INTEGER, modified INTEGER
);
CREATE INDEX IF NOT EXISTS note_blocks_tree ON note_blocks(doc, parent, rank);
";

#[derive(Debug)]
pub enum Error { Gate(Gate), Sql(rusqlite::Error), Io(std::io::Error), Corrupt(String) }
impl std::error::Error for Error {}
impl From<rusqlite::Error> for Error { fn from(e: rusqlite::Error) -> Self { Error::Sql(e) } }
impl From<std::io::Error> for Error { fn from(e: std::io::Error) -> Self { Error::Io(e) } }
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { match self { Error::Gate(g) => g.fmt(f), Error::Sql(e) => e.fmt(f), Error::Io(e) => e.fmt(f), Error::Corrupt(s) => f.write_str(s) } }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DocKind { Folder, Note, Canvas }
impl DocKind {
    fn name(self) -> &'static str { match self { DocKind::Folder => "folder", DocKind::Note => "note", DocKind::Canvas => "canvas" } }
    fn parse(s: &str) -> DocKind { match s { "folder" => DocKind::Folder, "canvas" => DocKind::Canvas, _ => DocKind::Note } }
}
/// One entry of the collection's tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry { pub id: BlockId, pub parent: Option<BlockId>, pub kind: DocKind, pub title: String }

pub struct Collection { db: Connection, pending: Vec<(BlockId, Doc)> }

/// What a merge on save could not simply join.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct MergeReport {
    pub tree: Vec<TreeNote>,
    /// Per note: the cases its blocks' merge reported.
    pub notes: Vec<(BlockId, Vec<mn_doc::Note>)>,
    /// The notes whose content here changed through the merge (an open editor has to reload them).
    pub changed: Vec<BlockId>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeNote {
    /// Both gave it a different title: mine stands.
    RenamedByBoth(BlockId),
    /// Both moved it to a different folder: it is where mine put it.
    MovedByBoth(BlockId),
    /// One side removed it, the other changed it: it stays.
    KeptOverDelete(BlockId),
}

fn now_ms() -> i64 { SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0) }
fn id_of(s: &str) -> Result<BlockId, Error> { BlockId::parse(s).ok_or_else(|| Error::Corrupt(format!("not an id: {s:?}"))) }

impl Collection {
    /// Opens the staged work copy at `path`, creating a new empty collection when there is no file. A file of another
    /// format is refused with the reason (a 2.0 file is not opened here: it is CONVERTED, see [`old`]).
    pub fn open(path: &Path) -> Result<Collection, Error> {
        let existed = path.exists();
        let db = Connection::open(path)?;
        if existed {
            match old::marks(&db).ok_or(Error::Gate(Gate::NotMinNotes))? {
                (f, v) if f == FORMAT && v == SCHEMA => {}
                (f, v) if f == FORMAT && v > SCHEMA => return Err(Error::Gate(Gate::Newer)),
                (_, v) if v > 0 => return Err(Error::Gate(Gate::Earlier)),
                _ => return Err(Error::Gate(Gate::NotMinNotes)),
            }
        }
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;")?;
        db.execute_batch(DDL)?;
        let c = Collection { db, pending: vec![] };
        if !existed { c.stamp()?; }
        Ok(c)
    }
    /// Opens a staged copy for reading only (the base, or someone else's saved file during a merge).
    pub fn open_read(path: &Path) -> Result<Collection, Error> {
        let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|_| Error::Gate(Gate::NotMinNotes))?;
        match old::marks(&db).ok_or(Error::Gate(Gate::NotMinNotes))? {
            (f, v) if f == FORMAT && v == SCHEMA => Ok(Collection { db, pending: vec![] }),
            (f, v) if f == FORMAT && v > SCHEMA => Err(Error::Gate(Gate::Newer)),
            (_, v) if v > 0 => Err(Error::Gate(Gate::Earlier)),
            _ => Err(Error::Gate(Gate::NotMinNotes)),
        }
    }
    /// Writes the format mark; `created` is set once, `modified` every time.
    fn stamp(&self) -> Result<(), Error> {
        let t = now_ms();
        self.db.execute("INSERT INTO doc_meta (id, schema_version, app_version, created, modified, format) VALUES (1, ?1, ?2, ?3, ?3, ?4)
                         ON CONFLICT(id) DO UPDATE SET schema_version = ?1, app_version = ?2, modified = ?3, format = ?4", params![SCHEMA, env!("CARGO_PKG_VERSION"), t, FORMAT])?;
        Ok(())
    }
    /// A compact, complete copy of the collection at `to` — what [`crate::staging::Session::save`] renames over the original.
    pub fn publish(&self, to: &Path) -> Result<(), Error> {
        self.stamp()?;
        self.db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        self.db.execute("VACUUM INTO ?1", [to.to_string_lossy()])?;
        Ok(())
    }

    // ── the tree of documents
    /// Adds a folder, note or canvas as the last child of `parent` (None = the collection's top level).
    pub fn create(&self, parent: Option<BlockId>, kind: DocKind, title: &str) -> Result<BlockId, Error> {
        let last: Option<String> = self.db.query_row("SELECT rank FROM documents WHERE parent IS ?1 ORDER BY rank DESC LIMIT 1", [parent.map(|p| p.to_string())], |r| r.get(0)).optional()?;
        let rank = match last.as_deref().and_then(|r| Rank::parse(r).ok()) { Some(r) => r.after(), None => Rank::first() };
        let (id, t) = (BlockId::new(), now_ms());
        self.db.execute("INSERT INTO documents (id, parent, rank, kind, title, created, modified) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)", params![id.to_string(), parent.map(|p| p.to_string()), rank.as_str(), kind.name(), title, t])?;
        Ok(id)
    }
    /// The children of `parent` in order.
    pub fn list(&self, parent: Option<BlockId>) -> Result<Vec<Entry>, Error> {
        let mut s = self.db.prepare("SELECT id, kind, title FROM documents WHERE parent IS ?1 ORDER BY rank, rowid")?;
        let rows: Vec<(String, String, String)> = s.query_map([parent.map(|p| p.to_string())], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
        rows.into_iter().map(|(id, kind, title)| Ok(Entry { id: id_of(&id)?, parent, kind: DocKind::parse(&kind), title })).collect()
    }
    /// The first line of a note's first block (what an unnamed note is called by), or "" when it has none.
    pub fn first_line(&self, note: BlockId) -> Result<String, Error> {
        let t: Option<String> = self.db.query_row("SELECT text FROM note_blocks WHERE doc = ?1 AND parent IS NULL AND kind NOT IN ('media', 'divider', 'columns', 'table') ORDER BY rank, rowid LIMIT 1", [note.to_string()], |r| r.get(0)).optional()?;
        Ok(t.unwrap_or_default().lines().next().unwrap_or("").trim().chars().take(80).collect())
    }
    pub fn rename(&self, id: BlockId, title: &str) -> Result<(), Error> { self.db.execute("UPDATE documents SET title = ?2, modified = ?3 WHERE id = ?1", params![id.to_string(), title, now_ms()])?; Ok(()) }
    /// Removes an entry with everything under it.
    pub fn remove(&self, id: BlockId) -> Result<(), Error> { self.db.execute("DELETE FROM documents WHERE id = ?1", [id.to_string()])?; Ok(()) }

    // ── a note's blocks
    pub fn load_note(&self, note: BlockId) -> Result<Doc, Error> {
        let mut s = self.db.prepare("SELECT id, parent, rank, kind, attrs, text FROM note_blocks WHERE doc = ?1 ORDER BY rank, rowid")?;
        let rows: Vec<(String, Option<String>, String, String, Option<String>, String)> = s.query_map([note.to_string()], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))?.collect::<Result<_, _>>()?;
        // rows come in rank order; grouped by parent, each group is that parent's list in order. The tree is then
        // built from the top down, so a parent always exists before its children.
        let total = rows.len();
        let mut by_parent: std::collections::HashMap<Option<String>, Vec<(String, String, String, Option<String>, String)>> = std::collections::HashMap::new();
        for (id, parent, rank, kind, attrs, text) in rows { by_parent.entry(parent).or_default().push((id, rank, kind, attrs, text)); }
        let mut doc = Doc::new(); let mut ranks = Vec::with_capacity(total);
        let mut todo: Vec<Option<String>> = vec![None];
        while let Some(parent) = todo.pop() {
            let pid = match &parent { Some(p) => Some(id_of(p)?), None => None };
            for (at, (id_text, rank, kind, attrs, text)) in by_parent.remove(&parent).unwrap_or_default().into_iter().enumerate() {
                let id = id_of(&id_text)?;
                let (kind, text, spans, conflict) = codec::decode(&kind, attrs.as_deref(), text);
                doc.insert(pid, at, id, kind, text).map_err(|e| Error::Corrupt(format!("{id}: {e:?}")))?;
                doc.set_spans(id, spans).ok(); doc.set_conflict(id, conflict).ok();
                ranks.push((id, rank)); todo.push(Some(id_text));
            }
        }
        if doc.len() != total { return Err(Error::Corrupt(format!("{} blocks have no parent in the note", total - doc.len()))) }
        // the file's ranks are kept where they are well-formed and in order; otherwise the note is ranked afresh
        if !doc.adopt_ranks(ranks.iter().filter_map(|(id, r)| Some((*id, Rank::parse(r).ok()?)))) { doc.rerank(); }
        Ok(doc)
    }
    /// Writes a note's blocks: rows that changed are rewritten, rows no longer in the note go, the rest are untouched
    /// (their `modified` stays; a block's `created` is set once and survives every later save — row 284 e).
    pub fn save_note(&mut self, note: BlockId, doc: &Doc) -> Result<(), Error> {
        let tx = self.db.transaction()?;
        let t = now_ms(); let key = note.to_string();
        let old: std::collections::HashMap<String, (Option<String>, String, String, Option<String>, String)> = {
            let mut s = tx.prepare("SELECT id, parent, rank, kind, attrs, text FROM note_blocks WHERE doc = ?1")?;
            s.query_map([&key], |r| Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))))?.collect::<Result<_, _>>()?
        };
        let mut kept = std::collections::HashSet::new();
        for b in doc.walk() {
            let id = b.id.to_string(); let (kind, attrs, text) = codec::encode(b);
            let row = (b.parent.map(|p| p.to_string()), b.rank.to_string(), kind.to_string(), attrs, text);
            if old.get(&id) != Some(&row) {
                tx.execute("INSERT INTO note_blocks (id, doc, parent, rank, kind, attrs, text, created, modified) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
                            ON CONFLICT(id) DO UPDATE SET doc = ?2, parent = ?3, rank = ?4, kind = ?5, attrs = ?6, text = ?7, modified = ?8", params![id, key, row.0, row.1, row.2, row.3, row.4, t])?;
            }
            kept.insert(id);
        }
        for id in old.keys().filter(|k| !kept.contains(*k)) { tx.execute("DELETE FROM note_blocks WHERE id = ?1", [id])?; }
        tx.execute("UPDATE documents SET modified = ?2 WHERE id = ?1", params![key, t])?;
        tx.commit()?; Ok(())
    }
    /// (created, modified) of a block, in ms since the epoch.
    pub fn block_times(&self, id: BlockId) -> Result<Option<(i64, i64)>, Error> { Ok(self.db.query_row("SELECT created, modified FROM note_blocks WHERE id = ?1", [id.to_string()], |r| Ok((r.get(0)?, r.get(1)?))).optional()?) }

    /// Every entry of the tree, each list in order.
    fn entries(&self) -> Result<Vec<Entry>, Error> {
        let mut s = self.db.prepare("SELECT id, parent, kind, title FROM documents ORDER BY rank, rowid")?;
        let rows: Vec<(String, Option<String>, String, String)> = s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<Result<_, _>>()?;
        rows.into_iter().map(|(id, parent, kind, title)| Ok(Entry { id: id_of(&id)?, parent: match parent { Some(p) => Some(id_of(&p)?), None => None }, kind: DocKind::parse(&kind), title })).collect()
    }

    /// The merge on save (rows 272, 285): this collection is MINE (the staged work copy); `base` is the file as it was
    /// opened (None: it did not exist), `theirs` a staged copy of what is at the original's place now. The tree of
    /// documents is merged by the rules of row 285 — a title is content, a folder is a parent — and every note by
    /// [`mn_doc::merge`]. The result is written into this collection; the report says what could not simply be joined.
    /// (A canvas has no content here yet: its entry merges, nothing more.)
    pub fn merge_from(&mut self, base: Option<&Path>, theirs: &Path) -> Result<MergeReport, Error> {
        use std::collections::{HashMap, HashSet};
        let base = match base { Some(p) => Some(Collection::open_read(p)?), None => None };
        let theirs = Collection::open_read(theirs)?;
        let index = |v: Vec<Entry>| -> (Vec<BlockId>, HashMap<BlockId, Entry>) { (v.iter().map(|e| e.id).collect(), v.into_iter().map(|e| (e.id, e)).collect()) };
        let (b_order, b) = index(match &base { Some(c) => c.entries()?, None => vec![] });
        let (m_order, m) = index(self.entries()?);
        let (t_order, t) = index(theirs.entries()?);
        let note = |c: Option<&Collection>, e: Option<&Entry>| -> Result<Doc, Error> { match (c, e) { (Some(c), Some(e)) if e.kind == DocKind::Note => c.load_note(e.id), _ => Ok(Doc::new()) } };
        let mut report = MergeReport::default();

        // who survives: deleted on one side and untouched on the other (its title, its folder, its blocks) is gone
        let mut alive: HashSet<BlockId> = HashSet::new();
        for id in m_order.iter().chain(&t_order) {
            let keep = match (m.get(id), t.get(id), b.get(id)) {
                (Some(_), Some(_), _) | (_, _, None) => true,
                (Some(e), None, Some(o)) => { let touched = e != o || note(Some(self), Some(e))? != note(base.as_ref(), Some(o))?; if touched { report.tree.push(TreeNote::KeptOverDelete(*id)); } touched }
                (None, Some(e), Some(o)) => { let touched = e != o || note(Some(&theirs), Some(e))? != note(base.as_ref(), Some(o))?; if touched { report.tree.push(TreeNote::KeptOverDelete(*id)); } touched }
                (None, None, _) => false,
            };
            if keep { alive.insert(*id); }
        }
        // title and folder: the side's that changed it; mine where both did
        let mut merged: HashMap<BlockId, Entry> = HashMap::new();
        for id in &alive {
            let e = match (m.get(id), t.get(id)) {
                (Some(a), Some(c)) => {
                    let o = b.get(id);
                    let title = if o.is_none_or(|o| o.title != a.title) { if o.is_none_or(|o| o.title != c.title) && a.title != c.title { report.tree.push(TreeNote::RenamedByBoth(*id)); } a.title.clone() } else { c.title.clone() };
                    let parent = if o.is_none_or(|o| o.parent != a.parent) { if o.is_none_or(|o| o.parent != c.parent) && a.parent != c.parent { report.tree.push(TreeNote::MovedByBoth(*id)); } a.parent } else { c.parent };
                    Entry { id: *id, parent, kind: a.kind, title }
                }
                (Some(a), None) => a.clone(), (None, Some(c)) => c.clone(), _ => continue,
            };
            merged.insert(*id, e);
        }
        for e in merged.values_mut() { if e.parent.is_some_and(|p| !alive.contains(&p)) { e.parent = None; } }
        // the notes: three-way where both sides have one; theirs where only they do
        for id in &alive {
            let e = &merged[id]; if e.kind != DocKind::Note { continue }
            match (m.get(id), t.get(id)) {
                (Some(a), Some(c)) => {
                    let (mine, theirs_doc) = (self.load_note(a.id)?, theirs.load_note(c.id)?);
                    if mine == theirs_doc { continue }
                    let r = mn_doc::merge(&note(base.as_ref(), b.get(id))?, &mine, &theirs_doc);
                    if !r.notes.is_empty() { report.notes.push((*id, r.notes)); }
                    if r.doc != mine { report.changed.push(*id); self.pending.push((*id, r.doc)); }
                }
                (None, Some(c)) => { report.changed.push(*id); self.pending.push((*id, theirs.load_note(c.id)?)); }
                _ => {}
            }
        }
        // write the tree: each list in merged order, with fresh ranks
        let mut parents: Vec<Option<BlockId>> = vec![None]; parents.extend(alive.iter().map(|x| Some(*x)));
        let tx = self.db.transaction()?; let now = now_ms();
        for id in m_order.iter().filter(|x| !alive.contains(*x)) { tx.execute("DELETE FROM documents WHERE id = ?1", [id.to_string()])?; }
        tx.execute_batch("PRAGMA defer_foreign_keys = ON;")?;
        for p in parents {
            let list = |order: &[BlockId]| -> Vec<BlockId> { order.iter().filter(|x| alive.contains(*x) && merged[*x].parent == p).copied().collect() };
            let base_list: Vec<BlockId> = b_order.iter().filter(|x| b[*x].parent == p).copied().collect();
            let order = mn_doc::merge_order(&base_list, list(&m_order), list(&t_order), |x| !b.contains_key(&x) && !t.contains_key(&x));
            for (id, rank) in order.iter().zip(mn_doc::rank::spread(order.len())) {
                let e = &merged[id];
                tx.execute("INSERT INTO documents (id, parent, rank, kind, title, created, modified) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
                            ON CONFLICT(id) DO UPDATE SET parent = ?2, rank = ?3, title = ?5", params![id.to_string(), e.parent.map(|x| x.to_string()), rank.as_str(), e.kind.name(), e.title, now])?;
            }
        }
        tx.commit()?;
        for (id, doc) in std::mem::take(&mut self.pending) { self.save_note(id, &doc)?; }
        report.tree.sort_by_key(|n| format!("{n:?}")); report.tree.dedup(); report.changed.sort();
        Ok(report)
    }

    /// Converts a 2.0 file (a STAGED copy of it) into this collection: one note per page, under a folder named
    /// `title` (row 283, ruling 5: pages only — the canvas comes with the canvas step). Returns the notes made and what
    /// the conversion had to repair, per page.
    pub fn import_v2(&mut self, old_file: &Path, title: &str) -> Result<Vec<(BlockId, Vec<String>)>, Error> {
        let pages = old::read_pages(old_file).map_err(Error::Gate)?;
        let folder = self.create(None, DocKind::Folder, title)?;
        let mut out = Vec::new();
        for (i, page) in pages.iter().enumerate() {
            let converted = mn_doc::legacy::convert_page(&page.blocks);
            // a page had no title of its own: the first heading or line names the note
            let name = converted.doc.walk().iter().find(|b| b.kind.is_text() && !b.text.trim().is_empty()).map(|b| b.text.lines().next().unwrap_or("").trim().chars().take(80).collect::<String>()).unwrap_or_else(|| format!("Page {}", i + 1));
            let note = self.create(Some(folder), DocKind::Note, &name)?;
            self.save_note(note, &converted.doc)?;
            out.push((note, converted.notes));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use mn_doc::{inline::Mark, ColumnKind, ColumnSpec, Kind, Spans};
    use crate::staging::Session;
    fn scratch(name: &str) -> PathBuf { let d = std::env::temp_dir().join(format!("mn-store-{name}-{}", BlockId::new())); std::fs::create_dir_all(d.join("share")).unwrap(); std::fs::create_dir_all(d.join("staging")).unwrap(); d }
    /// A note with every kind of block, formatting over a non-BMP character, and both containers.
    fn sample() -> Doc {
        let mut d = Doc::new(); let id = BlockId::new; let mut top = 0;
        let mut add = |d: &mut Doc, parent: Option<BlockId>, kind: Kind, text: &str| { let x = id(); let at = d.children(parent).len(); d.insert(parent, at, x, kind, text).unwrap(); if parent.is_none() { top += 1; } x };
        let h = add(&mut d, None, Kind::Heading { level: 2 }, "Title \u{1F600} bold");
        let mut s = Spans::new(); s.put(11, 15, Mark::Bold); s.put(0, 5, Mark::Link("https://example.com".into())); d.set_spans(h, s).unwrap();
        add(&mut d, None, Kind::Quote, "quoted"); add(&mut d, None, Kind::Code { lang: "rust".into() }, "fn main() {}\n");
        add(&mut d, None, Kind::Bullet { depth: 1 }, "b"); add(&mut d, None, Kind::Numbered { depth: 0 }, "n"); add(&mut d, None, Kind::Task { depth: 2, state: 1 }, "t");
        add(&mut d, None, Kind::Divider, ""); add(&mut d, None, Kind::Media { descriptor: r#"{"h":3,"src":".minnotes/x.png","w":4}"#.into() }, "");
        let cols = add(&mut d, None, Kind::Columns, "");
        let c1 = add(&mut d, Some(cols), Kind::Column { width: 0.3333 }, ""); let c2 = add(&mut d, Some(cols), Kind::Column { width: 0.6667 }, "");
        add(&mut d, Some(c1), Kind::Paragraph, "left");
        let t = add(&mut d, Some(c2), Kind::Table { header_rows: 1, columns: vec![ColumnSpec { width: Some(200), align: 2, bg: "#111111".into(), ..Default::default() }, ColumnSpec { kind: ColumnKind::Choice { options: vec![mn_doc::ChoiceOption { id: "A".into(), label: "Open".into(), color: "#58A65C".into() }] }, ..Default::default() }, ColumnSpec { kind: ColumnKind::Timecode { fps: 23.976 }, ..Default::default() }, ColumnSpec { kind: ColumnKind::Check, ..Default::default() }] }, "");
        let r = add(&mut d, Some(t), Kind::Row { bg: "#222222".into(), fg: String::new() }, "");
        for i in 0..4 { let c = add(&mut d, Some(r), Kind::Cell { bg: String::new(), fg: if i == 1 { "#ffffff".into() } else { String::new() } }, ""); add(&mut d, Some(c), Kind::Paragraph, "cell"); }
        add(&mut d, None, Kind::Paragraph, "");
        d
    }

    #[test]
    fn a_note_round_trips_and_a_resave_touches_only_what_changed() {
        let d = scratch("roundtrip"); let path = d.join("work.mnd");
        let mut c = Collection::open(&path).unwrap();
        let folder = c.create(None, DocKind::Folder, "Project").unwrap();
        let note = c.create(Some(folder), DocKind::Note, "First").unwrap(); let other = c.create(Some(folder), DocKind::Canvas, "Board").unwrap();
        assert_eq!(c.list(Some(folder)).unwrap().iter().map(|e| (e.id, e.kind, e.title.as_str())).collect::<Vec<_>>(), [(note, DocKind::Note, "First"), (other, DocKind::Canvas, "Board")]);
        let mut doc = sample();
        c.save_note(note, &doc).unwrap();
        drop(c);
        let mut c = Collection::open(&path).unwrap();
        let back = c.load_note(note).unwrap();
        assert_eq!(back, doc);
        for b in doc.walk() { assert_eq!(back.get(b.id).unwrap().rank, b.rank); }                       // the file's ranks are kept
        // change one block; every other row keeps its times, and the changed one keeps its creation time
        let first = doc.children(None)[0]; let second = doc.children(None)[1];
        let before = (c.block_times(first).unwrap().unwrap(), c.block_times(second).unwrap().unwrap());
        std::thread::sleep(std::time::Duration::from_millis(5));
        doc.set_text(second, "quoted, edited").unwrap(); let gone = *doc.children(None).last().unwrap(); doc.remove(gone).unwrap();
        c.save_note(note, &doc).unwrap();
        let after = (c.block_times(first).unwrap().unwrap(), c.block_times(second).unwrap().unwrap());
        assert_eq!(after.0, before.0);
        assert!(after.1.0 == before.1.0 && after.1.1 > before.1.1);
        assert_eq!((c.load_note(note).unwrap(), c.block_times(gone).unwrap()), (doc, None));
        c.remove(folder).unwrap();
        assert!(c.list(None).unwrap().is_empty() && c.load_note(note).unwrap().is_empty());                // a folder goes with its notes and their blocks
        std::fs::remove_dir_all(d).unwrap();
    }

    /// A 2.0 file as the old app writes it (the DDL of the spec §3.1), with synthetic content.
    fn old_file(path: &Path, format: &str, schema: i64) {
        let db = Connection::open(path).unwrap();
        db.execute_batch("CREATE TABLE blocks (id TEXT PRIMARY KEY, rank TEXT NOT NULL, depth INTEGER NOT NULL DEFAULT 0, type TEXT NOT NULL, attrs TEXT, content TEXT NOT NULL DEFAULT '', created INTEGER, modified INTEGER, page_id TEXT);
            CREATE TABLE doc_meta (id INTEGER PRIMARY KEY CHECK (id = 1), title TEXT, schema_version INTEGER, app_version TEXT, created INTEGER, modified INTEGER, last_cursor TEXT, page_width INTEGER, format TEXT);
            CREATE TABLE elements (id TEXT PRIMARY KEY, kind TEXT NOT NULL, x REAL NOT NULL DEFAULT 0, y REAL NOT NULL DEFAULT 0, w REAL NOT NULL DEFAULT 0, h REAL NOT NULL DEFAULT 0, z INTEGER NOT NULL DEFAULT 0, parent_id TEXT, json TEXT NOT NULL DEFAULT '{}', created INTEGER, modified INTEGER);").unwrap();
        db.execute("INSERT INTO doc_meta (id, schema_version, app_version, created, modified, format) VALUES (1, ?1, '2.0.6', 1, 1, ?2)", params![schema, format]).unwrap();
        let ulid = |n: u128| BlockId::from_u128((0x0190_0000_0000u128 << 80) + n).to_string();
        db.execute("INSERT INTO elements (id, kind, z, created) VALUES (?1, 'page', 1, 2), (?2, 'page', 0, 1), (?3, 'ink', 0, 0)", params![ulid(1), ulid(2), ulid(3)]).unwrap();
        let rows: &[(u128, &str, i64, &str, Option<&str>, &str, Option<u128>)] = &[
            (10, "k", 0, "paragraph", None, "second page, last", Some(1)),
            (11, "V", 0, "heading", Some(r#"{"level":2}"#), "Second page", Some(1)),
            (20, "V", 0, "paragraph", None, "First page", Some(2)),
            (21, "c", 0, "split", Some(r#"{"ratios":[0.5,0.5]}"#), "", Some(2)),
            (22, "g", 0, "paragraph", Some(r#"{"cell":0}"#), "left", Some(2)),
            (23, "i", 1, "list_item", Some(r#"{"cell":1}"#), "right", Some(2)),
            (30, "z", 0, "paragraph", None, "belongs to no page", None),
        ];
        for (n, rank, depth, kind, attrs, content, page) in rows { db.execute("INSERT INTO blocks (id, rank, depth, type, attrs, content, created, modified, page_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, 1, ?7)", params![ulid(*n), rank, depth, kind, attrs, content, page.map(ulid)]).unwrap(); }
    }

    #[test]
    fn a_2_0_file_converts_to_one_note_per_page() {
        let d = scratch("import"); let old = d.join("old.mnd"); old_file(&old, "mnd2", 2);
        let mut c = Collection::open(&d.join("work.mnd")).unwrap();
        let made = c.import_v2(&old, "Converted").unwrap();
        assert_eq!(made.len(), 2);
        assert!(made.iter().all(|(_, notes)| notes.is_empty()));
        let folder = c.list(None).unwrap(); assert_eq!((folder.len(), folder[0].kind, folder[0].title.as_str()), (1, DocKind::Folder, "Converted"));
        let notes = c.list(Some(folder[0].id)).unwrap();
        assert_eq!(notes.iter().map(|n| n.title.as_str()).collect::<Vec<_>>(), ["First page", "Second page"]);       // pages in the old order (z, then created); named by their first line
        let first = c.load_note(notes[0].id).unwrap();
        let texts: Vec<&str> = first.walk().iter().filter(|b| b.kind.is_text()).map(|b| b.text.as_str()).collect();
        assert_eq!(texts, ["First page", "left", "right", "belongs to no page"]);                                     // the orphan block joins the first page
        assert!(matches!(first.get(first.children(None)[1]).unwrap().kind, Kind::Columns));
        assert_eq!(c.load_note(notes[1].id).unwrap().walk().iter().map(|b| b.text.as_str()).collect::<Vec<_>>(), ["Second page", "second page, last"]);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn each_reader_refuses_the_others_files_with_the_right_reason() {
        let d = scratch("gate");
        let new = d.join("new.mnd"); drop(Collection::open(&new).unwrap());
        assert_eq!(old::read_pages(&new).unwrap_err(), Gate::Newer);                          // what a 2.0 build answers: "made with a newer minNotes"
        let v2 = d.join("v2.mnd"); old_file(&v2, "mnd2", 2);
        assert!(matches!(Collection::open(&v2), Err(Error::Gate(Gate::Earlier))));            // a 2.0 file is converted, not opened
        let v1 = d.join("v1.mnd"); old_file(&v1, "mnd", 1);
        assert_eq!(old::read_pages(&v1).unwrap_err(), Gate::Earlier);                         // 1.x: dropped (row 283)
        let later = d.join("later.mnd"); old_file(&later, "mnd2", 4);
        assert!(matches!(Collection::open(&later), Err(Error::Gate(Gate::Newer))));
        let junk = d.join("junk.mnd"); std::fs::write(&junk, "not a database at all, just text").unwrap();
        assert!(matches!(Collection::open(&junk), Err(Error::Gate(Gate::NotMinNotes))));
        assert_eq!(old::read_pages(&junk).unwrap_err(), Gate::NotMinNotes);
        assert_eq!(Gate::Newer.to_string(), "This document was made with a newer minNotes and can't be opened.");
        std::fs::remove_dir_all(d).unwrap();
    }

    /// Rows 272 + 283 together: work in staging, publish by rename, reopen what was published.
    #[test]
    fn a_collection_is_published_through_staging_and_reopens() {
        let d = scratch("publish"); let file = d.join("share/notes.mnd");
        let mut s = Session::create(&d.join("staging"), &file).unwrap();
        let mut c = Collection::open(&s.work()).unwrap();
        let note = c.create(None, DocKind::Note, "Note").unwrap(); let doc = sample(); c.save_note(note, &doc).unwrap();
        assert!(!file.exists());                                                              // nothing at the real place until Save
        s.save(|tmp| c.publish(tmp).map_err(std::io::Error::other)).unwrap();
        assert_eq!(std::fs::read_dir(d.join("share")).unwrap().count(), 1);                   // one file: no journal, no temporary beside it
        drop(c); s.close().unwrap();
        let s2 = Session::open(&d.join("staging"), &file).unwrap();
        let c2 = Collection::open(&s2.work()).unwrap();
        assert_eq!(c2.load_note(note).unwrap(), doc);
        assert_eq!(std::fs::read(s2.base().unwrap()).unwrap(), std::fs::read(&file).unwrap());
        drop(c2); s2.close().unwrap(); std::fs::remove_dir_all(d).unwrap();
    }

    /// Rows 272 + 285 end to end: two people open one file; one saves; the other's save merges.
    #[test]
    fn a_save_over_someone_elses_save_merges_notes_and_the_tree() {
        let d = scratch("merge"); let file = d.join("share/notes.mnd");
        // the file both open: one folder, two notes
        let (folder, n1, n2, ids) = {
            let mut s = Session::create(&d.join("staging"), &file).unwrap(); let mut c = Collection::open(&s.work()).unwrap();
            let folder = c.create(None, DocKind::Folder, "Project").unwrap();
            let (n1, n2) = (c.create(Some(folder), DocKind::Note, "One").unwrap(), c.create(Some(folder), DocKind::Note, "Two").unwrap());
            let mut doc = Doc::new(); let ids: Vec<BlockId> = (0..3).map(|_| BlockId::new()).collect();
            for (i, t) in ["first", "second", "third"].iter().enumerate() { doc.insert(None, i, ids[i], Kind::Paragraph, *t).unwrap(); }
            c.save_note(n1, &doc).unwrap(); c.save_note(n2, &Doc::new()).unwrap();
            s.save(|tmp| c.publish(tmp).map_err(std::io::Error::other)).unwrap(); drop(c); s.close().unwrap();
            (folder, n1, n2, ids)
        };
        let mut mine = Session::open(&d.join("staging"), &file).unwrap(); let mut my = Collection::open(&mine.work()).unwrap();
        let mut other = Session::open(&d.join("staging"), &file).unwrap(); let mut their = Collection::open(&other.work()).unwrap();
        // they: edit the first and second paragraphs of One, rename Two, add a note, and save
        let mut doc = their.load_note(n1).unwrap(); doc.set_text(ids[0], "first, theirs").unwrap(); doc.set_text(ids[1], "second, theirs").unwrap(); their.save_note(n1, &doc).unwrap();
        their.rename(n2, "Two, renamed").unwrap(); let n3 = their.create(Some(folder), DocKind::Note, "Three").unwrap();
        let mut d3 = Doc::new(); d3.insert(None, 0, BlockId::new(), Kind::Paragraph, "their new note").unwrap(); their.save_note(n3, &d3).unwrap();
        other.save(|tmp| their.publish(tmp).map_err(std::io::Error::other)).unwrap();
        // me: edit the second and third paragraphs of One, delete Two (which they renamed), add a note of my own
        let mut doc = my.load_note(n1).unwrap(); doc.set_text(ids[1], "second, mine").unwrap(); doc.set_text(ids[2], "third, mine").unwrap(); my.save_note(n1, &doc).unwrap();
        my.remove(n2).unwrap(); let n4 = my.create(Some(folder), DocKind::Note, "Four").unwrap();
        // my save: the original changed → fetch, merge, publish
        assert!(mine.original_changed());
        let (theirs_copy, stamp) = mine.fetch_theirs().unwrap();
        let report = my.merge_from(mine.base().as_deref(), &theirs_copy).unwrap();
        mine.save_merged(stamp, |tmp| my.publish(tmp).map_err(std::io::Error::other)).unwrap();
        assert!(!mine.original_changed());
        // what is published
        let s = Session::open(&d.join("staging"), &file).unwrap(); let c = Collection::open(&s.work()).unwrap();
        assert_eq!(c.list(Some(folder)).unwrap().iter().map(|e| e.title.as_str()).collect::<Vec<_>>(), ["One", "Two, renamed", "Four", "Three"]);
        let one = c.load_note(n1).unwrap();
        assert_eq!(one.walk().iter().map(|b| b.text.as_str()).collect::<Vec<_>>(), ["first, theirs", "second, mine", "second, theirs", "third, mine"]);
        assert_eq!(one.get(one.children(None)[2]).unwrap().conflict, Some(ids[1]));
        assert_eq!(c.load_note(n3).unwrap(), d3);
        assert_eq!(report.tree, [TreeNote::KeptOverDelete(n2)]);
        assert_eq!((report.notes.len(), report.notes[0].0, report.changed.contains(&n1) && report.changed.contains(&n3)), (1, n1, true));
        let _ = n4;
        // a save that would overwrite a THIRD save is refused, and nothing is written — told by content, not by date
        let (_, old_print) = mine.fetch_theirs().unwrap();
        let mut bytes = std::fs::read(&file).unwrap(); let n = bytes.len(); bytes[n - 1] ^= 1; std::fs::write(&file, &bytes).unwrap();   // someone saves again: same size, same second
        let e = mine.save_merged(old_print, |tmp| my.publish(tmp).map_err(std::io::Error::other)).unwrap_err();
        assert_eq!((e.kind(), std::fs::read_dir(d.join("share")).unwrap().count(), std::fs::read(&file).unwrap()), (std::io::ErrorKind::Interrupted, 1, bytes));
        drop((c, my, their)); s.close().unwrap(); mine.close().unwrap(); other.close().unwrap(); std::fs::remove_dir_all(d).unwrap();
    }
}
