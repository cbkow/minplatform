//! A folder's listing as the pane holds it (spike S17's model, canon rows 451 / 463): the entries flat in one vector,
//! the ROWS in view as a `Vec<u32>` of indices into it, a natural-order key made once per name. A folder opened in
//! place adds its entries to the same vector and its rows under its own; closing it only takes the rows away.
//!
//! Order: folders first when that option is on (whatever the direction), then the column - Name in natural order
//! (`shot_2` before `shot_10`), case-blind - and the same order inside every folder opened in place. Hidden entries
//! are left out unless they are asked for. The search's filter applies to the folder in view, not to what is opened
//! inside it.
use crate::format;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// "No entry": the folder in view itself, as a parent.
pub const TOP: u32 = u32::MAX;
/// A row that is not an entry but a line about the folder above it ("Loading...", why it cannot be read).
const NOTE: u32 = 1 << 31;

/// An entry as the worker thread read it, with its sort key already made (off the UI thread).
#[derive(Clone, Debug)]
pub struct RawEntry { pub name: String, pub size: u64, pub mtime: i64, pub dir: bool, pub hidden: bool, /// a folder the system shows as one item (an application): `dir` is false for it
    pub package: bool, pub key: Box<[u8]> }
impl RawEntry {
    pub fn new(name: String, size: u64, mtime: i64, dir: bool, hidden: bool) -> RawEntry { let key = natural_key(&name); RawEntry { name, size, mtime, dir, hidden, package: false, key } }
    pub fn package(mut self) -> RawEntry { self.package = true; self.dir = false; self.size = 0; self }
}

/// The natural, case-blind order as bytes: letters folded to lower case, a run of digits as a marker, its length and
/// its digits without leading zeros - so that a plain byte comparison puts `shot_2` before `shot_10` (S17).
pub fn natural_key(name: &str) -> Box<[u8]> {
    let mut out = Vec::with_capacity(name.len() + 4); let mut digits: Vec<u8> = Vec::new(); let mut buf = [0u8; 4];
    let flush = |digits: &mut Vec<u8>, out: &mut Vec<u8>| { if digits.is_empty() { return } let s = digits.iter().position(|d| *d != b'0').unwrap_or(digits.len()); out.push(b'0'); out.push((digits.len() - s).min(255) as u8); out.extend_from_slice(&digits[s..]); digits.clear(); };
    for ch in name.chars() {
        if ch.is_ascii_digit() { digits.push(ch as u8); continue }
        flush(&mut digits, &mut out);
        for l in ch.to_lowercase() { out.extend_from_slice(l.encode_utf8(&mut buf).as_bytes()); }
    }
    flush(&mut digits, &mut out);
    out.into_boxed_slice()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortBy { Name, Date, Size, Kind }
impl SortBy {
    pub fn column(self) -> usize { match self { SortBy::Name => 0, SortBy::Date => 1, SortBy::Size => 2, SortBy::Kind => 3 } }
    pub fn of_column(c: usize) -> SortBy { match c { 1 => SortBy::Date, 2 => SortBy::Size, 3 => SortBy::Kind, _ => SortBy::Name } }
    pub fn parse(s: &str) -> Option<SortBy> { Some(match s { "name" => SortBy::Name, "date" => SortBy::Date, "size" => SortBy::Size, "kind" => SortBy::Kind, _ => return None }) }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kids { Loading, Loaded, Failed(String) }
struct Opened { state: Kids, children: Vec<u32>, expanded: bool }

pub struct Node { pub name: Box<str>, key: Box<[u8]>, pub size: u64, pub mtime: i64, pub parent: u32, pub depth: u16, /// its type: an index into the model's table of extensions and kinds
    pub ty: u16, pub dir: bool, pub hidden: bool, /// a folder shown as one item: no size of its own, never gone into
    pub package: bool }

pub struct Model {
    nodes: Vec<Node>, top: Vec<u32>, top_state: Kids, opened: HashMap<u32, Opened>,
    /// The rows in view, in order.
    rows: Vec<u32>, pos: Vec<u32>,
    pub sort: SortBy, pub ascending: bool, pub folders_first: bool, pub show_hidden: bool,
    /// Rows of folders opened in place are shown (the list); without it only the folder's own entries (the grid).
    pub tree: bool,
    filter: String,
    /// (extension - "/" a folder, "" none -, kind)
    types: Vec<(Box<str>, Box<str>)>, type_ids: HashMap<String, u16>,
    shown_top: usize, hidden_top: usize,
}

impl Default for Model { fn default() -> Self { Model::new() } }
impl Model {
    pub fn new() -> Model {
        Model { nodes: vec![], top: vec![], top_state: Kids::Loading, opened: HashMap::new(), rows: vec![], pos: vec![], sort: SortBy::Name, ascending: true, folders_first: true, show_hidden: false, tree: true, filter: String::new(),
            types: vec![("/".into(), "Folder".into()), ("".into(), "File".into())], type_ids: HashMap::new(), shown_top: 0, hidden_top: 0 }
    }
    /// Forget the folder (a navigation): entries, rows, what was opened in place. The options stay.
    pub fn clear(&mut self) { self.nodes.clear(); self.top.clear(); self.top_state = Kids::Loading; self.opened.clear(); self.rows.clear(); self.pos.clear(); self.filter.clear(); self.shown_top = 0; self.hidden_top = 0; }
    fn type_of(&mut self, name: &str, dir: bool, package: bool) -> u16 {
        if dir { return 0 } let Some(ext) = format::extension(name) else { return 1 };
        // a package of an extension is a type of its own beside a file of it: its kind is the table's, else "Package"
        let key = if package { format!("{ext}/") } else { ext.clone() };
        if let Some(id) = self.type_ids.get(&key) { return *id }
        if self.types.len() >= u16::MAX as usize { return 1 }
        let kind = if package { format::kind_of_package(&ext) } else { format::kind_of_extension(Some(&ext)) };
        let id = self.types.len() as u16; self.types.push((ext.as_str().into(), kind.into())); self.type_ids.insert(key, id); id
    }
    /// Entries read for `parent` ([`TOP`]: the folder in view) are added. Call [`Model::rebuild`] afterwards.
    pub fn add(&mut self, parent: u32, entries: Vec<RawEntry>) {
        let depth = if parent == TOP { 0 } else { self.nodes[parent as usize].depth + 1 };
        let first = self.nodes.len() as u32;
        for e in entries { let ty = self.type_of(&e.name, e.dir, e.package); self.nodes.push(Node { name: e.name.into_boxed_str(), key: e.key, size: e.size, mtime: e.mtime, parent, depth, ty, dir: e.dir, hidden: e.hidden, package: e.package }); }
        let ids = first..self.nodes.len() as u32;
        if parent == TOP { self.top.extend(ids); } else if let Some(o) = self.opened.get_mut(&parent) { o.children.extend(ids); }
    }
    /// How the reading of `parent` stands.
    pub fn set_state(&mut self, parent: u32, state: Kids) { if parent == TOP { self.top_state = state; } else if let Some(o) = self.opened.get_mut(&parent) { o.state = state; } }
    pub fn node(&self, id: u32) -> &Node { &self.nodes[id as usize] }
    pub fn len(&self) -> usize { self.rows.len() }
    /// The entry of row `i`; none for a row that is a line about a folder.
    pub fn row(&self, i: usize) -> Option<u32> { self.rows.get(i).copied().filter(|r| r & NOTE == 0) }
    /// A row that is a line about the folder above it: how deep it lies and what it says.
    pub fn note(&self, i: usize) -> Option<(u16, String)> {
        let r = *self.rows.get(i)?; if r & NOTE == 0 { return None } let id = r & !NOTE;
        let text = match self.opened.get(&id).map(|o| &o.state) { Some(Kids::Failed(why)) => format!("This folder cannot be read: {why}"), _ => "Loading\u{2026}".to_string() };
        Some((self.nodes[id as usize].depth + 1, text))
    }
    /// The row an entry is shown at.
    pub fn row_of(&self, id: u32) -> Option<usize> { self.pos.get(id as usize).copied().filter(|p| *p != u32::MAX).map(|p| p as usize) }
    pub fn is_expanded(&self, id: u32) -> bool { self.opened.get(&id).is_some_and(|o| o.expanded) }
    pub fn kind(&self, id: u32) -> &str { &self.types[self.nodes[id as usize].ty as usize].1 }
    /// The entry's extension for its icon: "/" a folder, "" none.
    pub fn ext(&self, id: u32) -> &str { &self.types[self.nodes[id as usize].ty as usize].0 }
    /// (entries of the folder in view that are shown, entries left out by the hidden rule)
    pub fn counts(&self) -> (usize, usize) { (self.shown_top, self.hidden_top) }
    pub fn filter(&self) -> &str { &self.filter }
    pub fn set_filter(&mut self, text: &str) { self.filter = text.to_lowercase(); }
    /// The entry of the folder in view with this name.
    pub fn find_top(&self, name: &str) -> Option<u32> { self.top.iter().copied().find(|id| &*self.nodes[*id as usize].name == name) }
    /// The entry called `name` in `parent` ([`TOP`]: the folder in view), as far as it is read.
    pub fn find_in(&self, parent: u32, name: &str) -> Option<u32> { if parent == TOP { self.find_top(name) } else { self.opened.get(&parent)?.children.iter().copied().find(|id| &*self.nodes[*id as usize].name == name) } }
    /// A path of names may still appear: a folder on the way to it is still being read.
    pub fn waits(&self, path: &[String]) -> bool {
        let mut at = TOP;
        for name in path {
            let state = if at == TOP { Some(&self.top_state) } else { self.opened.get(&at).map(|o| &o.state) };
            match self.find_in(at, name) { Some(id) => at = id, None => return state == Some(&Kids::Loading) }
        }
        false
    }
    /// Where an entry lies below `root` (the folder in view).
    pub fn path_of(&self, id: u32, root: &Path) -> PathBuf {
        let mut parts = vec![]; let mut at = id; while at != TOP { parts.push(&*self.nodes[at as usize].name); at = self.nodes[at as usize].parent; }
        let mut p = root.to_path_buf(); for part in parts.iter().rev() { p.push(part); } p
    }
    /// The entry's name with the folders opened in place above it: what it is known by across a new reading.
    pub fn rel_of(&self, id: u32) -> Vec<String> { let mut parts = vec![]; let mut at = id; while at != TOP { parts.push(self.nodes[at as usize].name.to_string()); at = self.nodes[at as usize].parent; } parts.reverse(); parts }

    /// A folder is opened in place. True when its entries have to be read (the caller starts that).
    pub fn expand(&mut self, id: u32) -> bool {
        if !self.nodes.get(id as usize).is_some_and(|n| n.dir) { return false }
        match self.opened.get_mut(&id) { Some(o) => { o.expanded = true; false } None => { self.opened.insert(id, Opened { state: Kids::Loading, children: vec![], expanded: true }); true } }
    }
    /// A folder opened in place is closed: its rows go, what was read of it is kept for the next opening.
    pub fn collapse(&mut self, id: u32) { if let Some(o) = self.opened.get_mut(&id) { o.expanded = false; } }
    /// The folders opened in place, as paths of names from the folder in view.
    pub fn expanded(&self) -> Vec<Vec<String>> { let mut v: Vec<Vec<String>> = self.opened.iter().filter(|(_, o)| o.expanded).map(|(id, _)| self.rel_of(*id)).collect(); v.sort(); v }

    fn shows(&self, id: u32) -> bool { self.show_hidden || !self.nodes[id as usize].hidden }
    fn cmp(&self, a: u32, b: u32) -> Ordering {
        let (x, y) = (&self.nodes[a as usize], &self.nodes[b as usize]);
        if self.folders_first && x.dir != y.dir { return if x.dir { Ordering::Less } else { Ordering::Greater } }
        let name = || x.key.cmp(&y.key).then_with(|| x.name.cmp(&y.name));
        let o = match self.sort {
            SortBy::Name => name(),
            SortBy::Date => x.mtime.cmp(&y.mtime).then_with(name),
            SortBy::Size => (if x.dir { 0 } else { x.size }).cmp(&(if y.dir { 0 } else { y.size })).then_with(name),
            SortBy::Kind => self.types[x.ty as usize].1.to_lowercase().cmp(&self.types[y.ty as usize].1.to_lowercase()).then_with(name),
        };
        if self.ascending { o } else { o.reverse() }
    }
    fn sorted(&self, mut ids: Vec<u32>) -> Vec<u32> {
        if self.sort == SortBy::Kind {
            // the kinds' order is found once, not at every comparison
            let mut rank: Vec<u16> = (0..self.types.len() as u16).collect(); rank.sort_by_key(|t| self.types[*t as usize].1.to_lowercase()); let mut of = vec![0u16; rank.len()]; for (r, t) in rank.iter().enumerate() { of[*t as usize] = r as u16; }
            let n = &self.nodes; let (ff, asc) = (self.folders_first, self.ascending);
            ids.sort_unstable_by(|a, b| { let (x, y) = (&n[*a as usize], &n[*b as usize]); if ff && x.dir != y.dir { return if x.dir { Ordering::Less } else { Ordering::Greater } }
                let o = of[x.ty as usize].cmp(&of[y.ty as usize]).then_with(|| x.key.cmp(&y.key)).then_with(|| x.name.cmp(&y.name)); if asc { o } else { o.reverse() } });
        } else { ids.sort_unstable_by(|a, b| self.cmp(*a, *b)); }
        ids
    }
    fn push(&self, rows: &mut Vec<u32>, id: u32) {
        rows.push(id);
        if !self.tree { return } let Some(o) = self.opened.get(&id).filter(|o| o.expanded) else { return };
        for c in self.sorted(o.children.iter().copied().filter(|c| self.shows(*c)).collect()) { self.push(rows, c); }
        if o.state != Kids::Loaded { rows.push(id | NOTE); }
    }
    /// Make the rows anew from the entries, the options and what is opened in place.
    pub fn rebuild(&mut self) {
        self.hidden_top = if self.show_hidden { 0 } else { self.top.iter().filter(|id| self.nodes[**id as usize].hidden).count() };
        let top: Vec<u32> = self.top.iter().copied().filter(|id| self.shows(*id) && (self.filter.is_empty() || self.nodes[*id as usize].name.to_lowercase().contains(&self.filter))).collect();
        self.shown_top = top.len();
        let mut rows = Vec::with_capacity(top.len());
        for id in self.sorted(top) { self.push(&mut rows, id); }
        self.pos.clear(); self.pos.resize(self.nodes.len(), u32::MAX);
        for (i, r) in rows.iter().enumerate() { if r & NOTE == 0 { self.pos[*r as usize] = i as u32; } }
        self.rows = rows;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn file(name: &str, size: u64, mtime: i64) -> RawEntry { RawEntry::new(name.into(), size, mtime, false, name.starts_with('.')) }
    fn dir(name: &str, mtime: i64) -> RawEntry { RawEntry::new(name.into(), 0, mtime, true, name.starts_with('.')) }
    fn names(m: &Model) -> Vec<String> { (0..m.len()).map(|i| match m.row(i) { Some(id) => format!("{}{}", "  ".repeat(m.node(id).depth as usize), m.node(id).name), None => { let (d, t) = m.note(i).unwrap(); format!("{}<{t}>", "  ".repeat(d as usize)) } }).collect() }
    fn model(entries: Vec<RawEntry>) -> Model { let mut m = Model::new(); m.add(TOP, entries); m.set_state(TOP, Kids::Loaded); m.rebuild(); m }

    #[test]
    fn natural_order_puts_2_before_10_whatever_the_case() {
        let mut v = vec!["shot_10", "Shot_2", "shot_1", "SHOT_03", "shot_3b", "shot", "Shot_2_a", "shot_002", "éclair", "Eclair", "10", "9", "a.txt", "a1.txt"];
        v.sort_by(|a, b| natural_key(a).cmp(&natural_key(b)).then_with(|| a.cmp(b)));
        assert_eq!(v, ["9", "10", "a.txt", "a1.txt", "Eclair", "shot", "shot_1", "Shot_2", "shot_002", "Shot_2_a", "SHOT_03", "shot_3b", "shot_10", "éclair"]);
        assert_eq!(natural_key("Shot_007"), natural_key("shot_7"), "case and leading zeros do not count");
        assert!(natural_key("v99") < natural_key("v100") && natural_key("v100") < natural_key("v1000"));
        let long = format!("f{}", "9".repeat(300)); assert!(natural_key("f1") < natural_key(&long), "a very long number does not break the key");
    }
    fn sample() -> Model {
        model(vec![file("shot_10.exr", 500, 30), file("shot_2.exr", 900, 10), dir("renders", 20), file("Notes.pdf", 100, 40), dir("Assets", 50), file(".DS_Store", 1, 60), file("plate.mov", 5000, 5), dir(".cache", 70)])
    }
    #[test]
    fn folders_first_then_the_column_in_either_direction() {
        let mut m = sample();
        assert_eq!(names(&m), ["Assets", "renders", "Notes.pdf", "plate.mov", "shot_2.exr", "shot_10.exr"]);
        m.ascending = false; m.rebuild(); assert_eq!(names(&m), ["renders", "Assets", "shot_10.exr", "shot_2.exr", "plate.mov", "Notes.pdf"], "descending: the folders stay first");
        m.ascending = true; m.sort = SortBy::Size; m.rebuild(); assert_eq!(names(&m), ["Assets", "renders", "Notes.pdf", "shot_10.exr", "shot_2.exr", "plate.mov"]);
        m.sort = SortBy::Date; m.rebuild(); assert_eq!(names(&m), ["renders", "Assets", "plate.mov", "shot_2.exr", "shot_10.exr", "Notes.pdf"]);
        m.ascending = false; m.rebuild(); assert_eq!(names(&m), ["Assets", "renders", "Notes.pdf", "shot_10.exr", "shot_2.exr", "plate.mov"]);
        // by kind: "OpenEXR image", "PDF document", "QuickTime movie"; the same kind by name
        m.ascending = true; m.sort = SortBy::Kind; m.rebuild(); assert_eq!(names(&m), ["Assets", "renders", "shot_2.exr", "shot_10.exr", "Notes.pdf", "plate.mov"]);
        m.folders_first = false; m.sort = SortBy::Name; m.rebuild(); assert_eq!(names(&m), ["Assets", "Notes.pdf", "plate.mov", "renders", "shot_2.exr", "shot_10.exr"], "folders among the files when the option is off");
        m.sort = SortBy::Kind; m.rebuild(); assert_eq!(names(&m), ["Assets", "renders", "shot_2.exr", "shot_10.exr", "Notes.pdf", "plate.mov"], "\"Folder\" sorts as a kind");
    }
    #[test]
    fn hidden_entries_are_left_out_until_asked_for() {
        let mut m = sample();
        assert_eq!(m.counts(), (6, 2)); assert!(m.find_top(".DS_Store").is_some_and(|id| m.row_of(id).is_none()));
        m.show_hidden = true; m.rebuild(); assert_eq!(m.counts(), (8, 0)); assert_eq!(names(&m)[..3], [".cache", "Assets", "renders"]); assert_eq!(names(&m)[3], ".DS_Store");
        m.show_hidden = false; m.set_filter("SHOT"); m.rebuild(); assert_eq!(names(&m), ["shot_2.exr", "shot_10.exr"]); assert_eq!(m.counts(), (2, 2));
        m.set_filter("e"); m.rebuild(); assert_eq!(names(&m), ["Assets", "renders", "Notes.pdf", "plate.mov", "shot_2.exr", "shot_10.exr"]);
        m.set_filter("zzz"); m.rebuild(); assert_eq!(m.len(), 0); m.set_filter(""); m.rebuild(); assert_eq!(m.len(), 6);
    }
    #[test]
    fn a_folder_opened_in_place_puts_its_rows_under_its_own() {
        let mut m = sample(); let renders = m.find_top("renders").unwrap();
        assert!(m.expand(renders), "its entries have to be read"); m.rebuild();
        assert_eq!(names(&m), ["Assets", "renders", "  <Loading\u{2026}>", "Notes.pdf", "plate.mov", "shot_2.exr", "shot_10.exr"]);
        m.add(renders, vec![file("beauty_10.exr", 3, 1), file("beauty_9.exr", 2, 2), dir("v002", 3), file(".hidden", 1, 4)]); m.set_state(renders, Kids::Loaded); m.rebuild();
        assert_eq!(names(&m), ["Assets", "renders", "  v002", "  beauty_9.exr", "  beauty_10.exr", "Notes.pdf", "plate.mov", "shot_2.exr", "shot_10.exr"]);
        let v002 = m.row(2).unwrap(); assert_eq!(m.node(v002).depth, 1); assert_eq!(m.path_of(v002, Path::new("/x")), Path::new("/x/renders/v002")); assert_eq!(m.rel_of(v002), ["renders", "v002"]);
        // a second level; the sort holds inside every level
        assert!(m.expand(v002)); m.add(v002, vec![file("b.txt", 1, 1), file("a.txt", 9, 2)]); m.set_state(v002, Kids::Loaded); m.sort = SortBy::Size; m.rebuild();
        assert_eq!(names(&m), ["Assets", "renders", "  v002", "    b.txt", "    a.txt", "  beauty_9.exr", "  beauty_10.exr", "Notes.pdf", "shot_10.exr", "shot_2.exr", "plate.mov"]);
        assert_eq!(m.expanded(), [vec!["renders".to_string()], vec!["renders".to_string(), "v002".to_string()]]);
        // closing the outer folder takes every row inside away; opening it again brings them back without a new reading
        m.sort = SortBy::Name; m.collapse(renders); m.rebuild(); assert_eq!(names(&m), ["Assets", "renders", "Notes.pdf", "plate.mov", "shot_2.exr", "shot_10.exr"]); assert_eq!(m.row_of(v002), None);
        assert!(!m.expand(renders)); m.rebuild(); assert_eq!(m.len(), 11); assert_eq!(m.row_of(v002), Some(2));
        // hidden entries inside follow the same option; the counts are the folder in view's
        m.show_hidden = true; m.rebuild(); assert!(names(&m).contains(&"  .hidden".to_string())); assert_eq!(m.counts(), (8, 0));
        // the grid shows the folder's own entries only
        m.show_hidden = false; m.tree = false; m.rebuild(); assert_eq!(names(&m), ["Assets", "renders", "Notes.pdf", "plate.mov", "shot_2.exr", "shot_10.exr"]);
        // a folder that cannot be read says so under its row
        m.tree = true; let assets = m.find_top("Assets").unwrap(); m.expand(assets); m.set_state(assets, Kids::Failed("Permission denied".into())); m.rebuild();
        assert_eq!(names(&m)[..2], ["Assets", "  <This folder cannot be read: Permission denied>"]);
        assert!(!m.expand(m.find_top("Notes.pdf").unwrap()), "a file does not open in place");
        // a package is one item: among the files, with its own kind, never opened in place
        let mut p = model(vec![dir("renders", 1), dir("Zeta", 2), RawEntry::new("Blender.app".into(), 0, 3, true, false).package(), RawEntry::new("Old.photoslibrary".into(), 0, 4, true, false).package(), file("a.txt", 1, 5), file("clip.app", 9, 6)]);
        assert_eq!(names(&p), ["renders", "Zeta", "a.txt", "Blender.app", "clip.app", "Old.photoslibrary"]);
        let app = p.find_top("Blender.app").unwrap(); assert!(!p.node(app).dir && p.node(app).package); assert_eq!(p.kind(app), "Application"); assert_eq!(p.ext(app), "app"); assert!(!p.expand(app));
        assert_eq!(p.kind(p.find_top("Old.photoslibrary").unwrap()), "Package"); assert_eq!(p.kind(p.find_top("clip.app").unwrap()), "Application");
        m.clear(); m.rebuild(); assert_eq!(m.len(), 0); assert!(m.expanded().is_empty());
    }
    #[test]
    fn a_hundred_thousand_entries_sort_in_one_frame_or_so() {
        let entries: Vec<RawEntry> = (0..100_000u32).map(|i| file(&format!("shot_{}_comp_v{:03}.exr", i.wrapping_mul(2654435761) % 4000, i % 30), i as u64, i as i64)).collect();
        let mut m = model(entries); assert_eq!(m.len(), 100_000);
        for i in (0..99_999).step_by(997) { let (a, b) = (m.row(i).unwrap(), m.row(i + 1).unwrap()); assert!(m.cmp(a, b) != Ordering::Greater); }
        m.sort = SortBy::Kind; m.rebuild(); assert_eq!(m.len(), 100_000);
    }
}
