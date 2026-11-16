//! The tracker (canon row 491; UFB 1.4.2's `TrackerView`, read into `docs/phase5/step0-projects-metadata.md`, section
//! 5.4): every TRACKED item - of one job (its Tracker), or of all the subscribed jobs (All Tracked) - in one list, with
//! the metadata columns of every section they come from laid side by side.
//!
//! The list is the item list itself ([`ItemList`]): this builds what it shows. The columns of the sections are merged
//! by name and type (1.4.2's rule: the first one met gives the order, the width and the options); each row's values
//! are re-keyed to the merged columns, and [`Tracker::target`] says which real column of which section a cell is - or
//! that the row's section has no such column, and the cell is not one. Two columns are the tracker's own and are not
//! edited: the job (All Tracked only) and the section.
use crate::db::{kind_of, ColDef, ItemMeta};
use crate::itemlist::ItemList;
use crate::jobview::Item;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The tracker's own columns: their keys in a row's values (no column's uuid begins so).
pub const PROJECT: &str = "#project"; pub const FOLDER: &str = "#folder";
#[derive(Clone, Debug, PartialEq)]
pub enum Scope { Job(PathBuf), All }
/// The dates a row must have one of, to be shown (1.4.2's "Due" chips).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Due { All, Overdue, Today, Week, Month }
pub const DUES: [(Due, &str); 5] = [(Due::All, "All"), (Due::Overdue, "Overdue"), (Due::Today, "Today"), (Due::Week, "This Week"), (Due::Month, "This Month")];

pub struct Tracker {
    pub scope: Scope, pub list: ItemList, /// the rows shown, and the one chosen
    pub rows: Vec<Item>, pub chosen: Option<usize>, pub due: Due, /// a date column is among those in view: the Due chips are offered
    pub dated: bool,
    /// every tracked row: (the item, its job, its section, what it carries)
    all: Vec<(Item, PathBuf, String)>, /// the real columns of each (job, section), by the merged column's key
    real: HashMap<(PathBuf, String), HashMap<String, ColDef>>,
}
fn key_of(c: &ColDef) -> String { format!("{}|{}", c.name.to_lowercase(), kind_of(&c.kind)) }
/// A date (ms of a local midnight) is in the bucket, `today` being the ms of today's midnight.
pub fn in_bucket(due: Due, date: i64, today: i64) -> bool {
    const DAY: i64 = 86_400_000;
    match due { Due::All => true, Due::Overdue => date < today, Due::Today => date >= today && date < today + DAY, Due::Week => date >= today && date < today + 7 * DAY,
        // (the same day of the next month, as 1.4.2 has it)
        Due::Month => { let c = crate::format::local(today / 1000 + 43_200); let (y, m) = if c.month == 12 { (c.year + 1, 1) } else { (c.year, c.month + 1) }; date >= today && date < crate::itemlist::midnight_ms(y, m, c.day.min(28)) } }
}

impl Tracker {
    pub fn new(scope: Scope) -> Tracker { let mut t = Tracker { scope, list: ItemList::default(), rows: vec![], chosen: None, due: Due::All, dated: false, all: vec![], real: HashMap::new() }; t.list.show_modified = true; t.reload(); t }
    /// Read the tracked items and their sections' columns again.
    pub fn reload(&mut self) {
        let was = self.chosen.and_then(|i| self.rows.get(i)).map(|i| i.path.clone());
        let tracked = crate::db::with(|d| d.tracked()).unwrap_or_default(); let subs: Vec<PathBuf> = crate::db::with(|d| d.subscriptions()).unwrap_or_default().into_iter().map(|s| s.path).collect();
        let names: HashMap<PathBuf, String> = crate::db::with(|d| d.subscriptions()).unwrap_or_default().into_iter().map(|s| (s.path, s.name)).collect();
        let mine = |job: &Path| match &self.scope { Scope::Job(j) => job == j, Scope::All => subs.iter().any(|s| s == job) };
        let (mut merged, mut real, mut meta, mut all): (Vec<ColDef>, HashMap<(PathBuf, String), HashMap<String, ColDef>>, HashMap<PathBuf, ItemMeta>, Vec<(Item, PathBuf, String)>) = (vec![], HashMap::new(), HashMap::new(), vec![]);
        for (path, job, folder, m) in tracked.into_iter().filter(|t| mine(&t.1)) {
            let cols = real.entry((job.clone(), folder.clone())).or_insert_with(|| { let defs = crate::db::with(|d| d.columns(&job, &folder)).unwrap_or_default(); let mut by = HashMap::new();
                for c in defs.into_iter().filter(|c| c.visible) { let k = key_of(&c); if !merged.iter().any(|m: &ColDef| m.uuid == k) { let mut shown = c.clone(); shown.uuid = k.clone(); merged.push(shown); } by.insert(k, c); } by });
            // the row's values under the merged columns' keys, with the tracker's own two
            let mut values = serde_json::Map::new(); for (k, c) in cols.iter() { if let Some(v) = m.values.get(&c.uuid) { values.insert(k.clone(), v.clone()); } }
            values.insert(PROJECT.into(), names.get(&job).cloned().unwrap_or_else(|| job.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()).into()); values.insert(FOLDER.into(), folder.clone().into());
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            // (the date shown is when the item's metadata was last changed, not the folder's own - 1.4.2)
            all.push((Item { name, path: path.clone(), modified: if m.modified > 0 { m.modified / 1000 } else { i64::MIN }, project: path.clone(), renders: path.clone(), proxies: None }, job, folder));
            meta.insert(path, ItemMeta { values, tracked: true, modified: m.modified });
        }
        all.sort_by(|a, b| crate::model::natural_key(&a.0.name).cmp(&crate::model::natural_key(&b.0.name)));
        let own = |key: &str, name: &str, width: f64| ColDef { id: -1, name: name.into(), kind: "text".into(), order: 0, width, visible: true, uuid: key.into(), options: vec![] };
        let mut cols = vec![]; if self.scope == Scope::All { cols.push(own(PROJECT, "Job", 170.0)); } cols.push(own(FOLDER, "Section", 90.0)); cols.extend(merged);
        self.dated = cols.iter().any(|c| kind_of(&c.kind) == "date");
        // (a width dragged in the tracker is kept while it is open; the sections' own layouts are not touched by it)
        for c in cols.iter_mut() { if let Some(old) = self.list.cols.iter().find(|o| o.uuid == c.uuid) { c.width = old.width; } }
        self.list.cols = cols; self.list.hidden.clear(); self.list.meta = meta; self.all = all; self.real = real;
        if let Some((crate::itemlist::SortBy::Col(u), _)) = &self.list.sort { if !self.list.cols.iter().any(|c| &c.uuid == u) { self.list.sort = None; } }
        self.filter(); self.chosen = was.and_then(|w| self.rows.iter().position(|r| r.path == w));
    }
    /// The rows the Due chip leaves: one whose ANY date falls in the bucket (1.4.2).
    pub fn filter(&mut self) {
        let now = crate::format::local(crate::format::now_secs()); let today = crate::itemlist::midnight_ms(now.year, now.month, now.day);
        let dates: Vec<&ColDef> = self.list.cols.iter().filter(|c| kind_of(&c.kind) == "date").collect(); let due = if self.dated { self.due } else { Due::All };
        self.rows = self.all.iter().filter(|(it, ..)| due == Due::All || dates.iter().any(|c| self.list.value(it, c).and_then(|v| v.as_f64()).is_some_and(|d| in_bucket(due, d as i64, today)))).map(|(it, ..)| it.clone()).collect();
        self.list.reorder(&self.rows); self.chosen = None;
    }
    /// Where a row belongs: its job and its section.
    pub fn home(&self, item: &Path) -> Option<(PathBuf, String)> { self.all.iter().find(|(it, ..)| it.path == item).map(|(_, j, f)| (j.clone(), f.clone())) }
    /// The real column a cell is: (the job, the section, the column as that section has it). None: the tracker's own
    /// column, or a column the row's section does not have.
    pub fn target(&self, item: &Path, col: usize) -> Option<(PathBuf, String, ColDef)> {
        let (job, folder) = self.home(item)?; let key = &self.list.cols.get(col)?.uuid; if key.starts_with('#') { return None }
        let c = self.real.get(&(job.clone(), folder.clone()))?.get(key)?.clone(); Some((job, folder, c))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_due_chips_are_1_4_2s_buckets() {
        const DAY: i64 = 86_400_000; let today = crate::itemlist::midnight_ms(2026, 11, 16);
        assert!(in_bucket(Due::All, 0, today)); assert!(in_bucket(Due::Overdue, today - DAY, today)); assert!(!in_bucket(Due::Overdue, today, today));
        assert!(in_bucket(Due::Today, today, today)); assert!(!in_bucket(Due::Today, today + DAY, today));
        assert!(in_bucket(Due::Week, today + 6 * DAY, today)); assert!(!in_bucket(Due::Week, today + 7 * DAY, today)); assert!(!in_bucket(Due::Week, today - DAY, today), "what is overdue is not this week's");
        assert!(in_bucket(Due::Month, crate::itemlist::midnight_ms(2026, 12, 15), today)); assert!(!in_bucket(Due::Month, crate::itemlist::midnight_ms(2026, 12, 16), today));
    }
    #[test]
    fn columns_merge_by_name_and_type() {
        let c = |name: &str, kind: &str| ColDef { id: 1, name: name.into(), kind: kind.into(), order: 0, width: 120.0, visible: true, uuid: "u".into(), options: vec![] };
        assert_eq!(key_of(&c("Status", "Dropdown")), key_of(&c("status", "dropdown"))); assert_ne!(key_of(&c("Status", "text")), key_of(&c("Status", "dropdown"))); assert_eq!(key_of(&c("Notes", "note")), "notes|notes");
    }
}
