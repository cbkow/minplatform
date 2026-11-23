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
/// "No sequence".
const NONE: u32 = u32::MAX;
/// An image sequence shown as ONE entry (canon rows 449, 475): its own entry, the files it stands for in frame order,
/// and whether a hand has asked for it to be shown as separate files after all.
struct Seq { node: u32, members: Vec<u32>, split: bool }
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
    pub package: bool, /// this entry IS a sequence (an index into the model's sequences); this file is a frame of one
    seq: u32, frame_of: u32 }

pub struct Model {
    nodes: Vec<Node>, top: Vec<u32>, top_state: Kids, opened: HashMap<u32, Opened>,
    /// The rows in view, in order.
    rows: Vec<u32>, pos: Vec<u32>,
    pub sort: SortBy, pub ascending: bool, pub folders_first: bool, pub show_hidden: bool,
    /// Rows of folders opened in place are shown (the list); without it only the folder's own entries (the grid).
    pub tree: bool,
    /// Image sequences are shown as one entry each (row 475); off: every file by itself. And the rules they are found by.
    pub sequences: bool, pub rules: min_paths::sequence::GroupRules, seqs: Vec<Seq>,
    filter: String,
    /// (extension - "/" a folder, "" none -, kind)
    types: Vec<(Box<str>, Box<str>)>, type_ids: HashMap<String, u16>,
    shown_top: usize, hidden_top: usize,
}

impl Default for Model { fn default() -> Self { Model::new() } }
impl Model {
    pub fn new() -> Model {
        Model { nodes: vec![], top: vec![], top_state: Kids::Loading, opened: HashMap::new(), rows: vec![], pos: vec![], sort: SortBy::Name, ascending: true, folders_first: true, show_hidden: false, tree: true, sequences: true, rules: Default::default(), seqs: vec![], filter: String::new(),
            types: vec![("/".into(), "Folder".into()), ("".into(), "File".into())], type_ids: HashMap::new(), shown_top: 0, hidden_top: 0 }
    }
    /// Forget the folder (a navigation): entries, rows, what was opened in place. The options stay.
    pub fn clear(&mut self) { self.nodes.clear(); self.top.clear(); self.top_state = Kids::Loading; self.opened.clear(); self.rows.clear(); self.pos.clear(); self.filter.clear(); self.shown_top = 0; self.hidden_top = 0; self.seqs.clear(); }
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
        for e in entries { let ty = self.type_of(&e.name, e.dir, e.package); self.nodes.push(Node { name: e.name.into_boxed_str(), key: e.key, size: e.size, mtime: e.mtime, parent, depth, ty, dir: e.dir, hidden: e.hidden, package: e.package, seq: NONE, frame_of: NONE }); }
        let ids = first..self.nodes.len() as u32;
        if parent == TOP { self.top.extend(ids); } else if let Some(o) = self.opened.get_mut(&parent) { o.children.extend(ids); }
    }
    /// How the reading of `parent` stands.
    pub fn set_state(&mut self, parent: u32, state: Kids) {
        // a folder read to its end: its image sequences are found (once all its names are known)
        if state == Kids::Loaded { self.find_sequences(parent); }
        if parent == TOP { self.top_state = state; } else if let Some(o) = self.opened.get_mut(&parent) { o.state = state; }
    }
    fn children(&self, parent: u32) -> &[u32] { if parent == TOP { &self.top } else { self.opened.get(&parent).map(|o| o.children.as_slice()).unwrap_or(&[]) } }
    /// The image sequences among `parent`'s files, by the family's detector and the browser's rules: each becomes ONE
    /// entry beside its files, which are then shown only when sequences are off (or that one is split).
    fn find_sequences(&mut self, parent: u32) {
        let files: Vec<u32> = self.children(parent).iter().copied().filter(|id| { let n = &self.nodes[*id as usize]; !n.dir && !n.package && n.seq == NONE && n.frame_of == NONE }).collect();
        if files.len() < 2 { return }
        let names: Vec<&str> = files.iter().map(|id| &*self.nodes[*id as usize].name).collect();
        let groups = min_paths::sequence::group(&names, &self.rules);
        for g in groups { let members: Vec<u32> = g.members.iter().map(|i| files[*i]).collect(); let (name, ext) = (g.display_name(), g.ext.to_lowercase()); self.make_sequence(parent, members, name, &ext); }
    }
    fn make_sequence(&mut self, parent: u32, members: Vec<u32>, name: String, ext: &str) -> u32 {
        let depth = if parent == TOP { 0 } else { self.nodes[parent as usize].depth + 1 };
        let (size, mtime, hidden) = members.iter().fold((0u64, i64::MIN, true), |a, id| { let n = &self.nodes[*id as usize]; (a.0 + n.size, a.1.max(n.mtime), a.2 && n.hidden) });
        let ty = self.type_of_sequence(ext); let (idx, id) = (self.seqs.len() as u32, self.nodes.len() as u32);
        for m in &members { self.nodes[*m as usize].frame_of = idx; }
        self.nodes.push(Node { key: natural_key(&name), name: name.into_boxed_str(), size, mtime, parent, depth, ty, dir: false, hidden, package: false, seq: idx, frame_of: NONE });
        if parent == TOP { self.top.push(id); } else if let Some(o) = self.opened.get_mut(&parent) { o.children.push(id); }
        self.seqs.push(Seq { node: id, members, split: false }); id
    }
    fn type_of_sequence(&mut self, ext: &str) -> u16 {
        let key = format!("{ext}#"); if let Some(id) = self.type_ids.get(&key) { return *id } if self.types.len() >= u16::MAX as usize { return 1 }
        let kind = format!("{} sequence", format::kind_of_extension(Some(ext)).trim_end_matches(" image"));
        let id = self.types.len() as u16; self.types.push((ext.into(), kind.into())); self.type_ids.insert(key, id); id
    }
    /// This entry is a sequence shown as one.
    pub fn is_sequence(&self, id: u32) -> bool { self.nodes[id as usize].seq != NONE }
    /// The files an entry stands for: a sequence's frames in order; else the entry itself.
    pub fn files_of(&self, id: u32) -> Vec<u32> { match self.nodes[id as usize].seq { NONE => vec![id], s => self.seqs[s as usize].members.clone() } }
    /// The file that stands for an entry where ONE file is needed (its picture, its preview): a sequence's MIDDLE frame
    /// (row 456, point 2); else the entry itself.
    pub fn rep(&self, id: u32) -> u32 { match self.nodes[id as usize].seq { NONE => id, s => { let m = &self.seqs[s as usize].members; m[m.len() / 2] } } }
    /// How many frames a sequence has, and how many of its range have no file.
    pub fn frames(&self, id: u32) -> Option<(usize, usize)> { let s = self.nodes[id as usize].seq; (s != NONE).then(|| { let m = &self.seqs[s as usize].members; let name = &self.nodes[id as usize].name; let span = name.rsplit_once('[').and_then(|(_, r)| r.split_once(']')).and_then(|(r, _)| r.split_once('-')).and_then(|(a, b)| Some(b.parse::<i64>().ok()? - a.parse::<i64>().ok()? + 1)).unwrap_or(m.len() as i64); (m.len(), (span.max(m.len() as i64) as usize) - m.len()) }) }
    /// The entry a file is shown AS: the sequence it is a frame of, while that is shown as one; else itself.
    pub fn shown_as(&self, id: u32) -> u32 { match self.nodes[id as usize].frame_of { NONE => id, s => { let q = &self.seqs[s as usize]; if self.sequences && !q.split { q.node } else { id } } } }
    /// A sequence is shown as its separate files after all (a hand's word against the detector's). Call [`Model::rebuild`].
    pub fn split(&mut self, id: u32) { let s = self.nodes[id as usize].seq; if s != NONE { self.seqs[s as usize].split = true; } }
    /// Files chosen by hand become ONE sequence, whatever the rules say - when their names differ only in a number.
    /// The new entry, or none when they are not such a set. Call [`Model::rebuild`].
    pub fn force_sequence(&mut self, ids: &[u32]) -> Option<u32> {
        let parent = self.nodes.get(*ids.first()? as usize)?.parent;
        if ids.iter().any(|id| { let n = &self.nodes[*id as usize]; n.parent != parent || n.dir || n.package || n.seq != NONE }) { return None }
        let names: Vec<&str> = ids.iter().map(|id| &*self.nodes[*id as usize].name).collect();
        let g = min_paths::sequence::force_group(&names)?; let members: Vec<u32> = g.members.iter().map(|i| ids[*i]).collect();
        // (a file that was a frame of a split sequence leaves it)
        let ext = g.ext.rsplit('.').next().unwrap_or("").to_lowercase(); Some(self.make_sequence(parent, members, g.display_name(), &ext))
    }
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
    /// The entry of `parent` whose name is `name` but for the case of its letters (a volume that does not tell them apart).
    pub fn find_ci(&self, parent: u32, name: &str) -> Option<u32> {
        let want = name.to_lowercase(); let is = |id: &u32| self.nodes[*id as usize].name.to_lowercase() == want;
        if parent == TOP { self.top.iter().copied().find(|id| is(id)) } else { self.opened.get(&parent)?.children.iter().copied().find(|id| is(id)) }
    }
    /// The entry at a path of names from the folder in view.
    pub fn id_of(&self, rel: &[String]) -> Option<u32> { let mut at = TOP; for name in rel { at = self.find_in(at, name)?; } (at != TOP).then_some(at) }
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
    /// A folder is read WITHOUT being opened, to see what it holds. True when its entries have to be read (the caller
    /// starts that); false when it is read, being read or open already.
    pub fn peek(&mut self, id: u32) -> bool {
        if !self.nodes.get(id as usize).is_some_and(|n| n.dir) || self.opened.contains_key(&id) { return false }
        self.opened.insert(id, Opened { state: Kids::Loading, children: vec![], expanded: false }); true
    }
    /// How many rows a folder read to its end comes to as the view is set (a sequence shown as one is one row); none
    /// while it is not read.
    pub fn rows_inside(&self, id: u32) -> Option<usize> { let o = self.opened.get(&id)?; (o.state == Kids::Loaded).then(|| o.children.iter().filter(|c| self.shows(**c)).count()) }
    /// A folder opened in place is closed: its rows go, what was read of it is kept for the next opening.
    pub fn collapse(&mut self, id: u32) { if let Some(o) = self.opened.get_mut(&id) { o.expanded = false; } }
    /// The folders opened in place, as paths of names from the folder in view.
    pub fn expanded(&self) -> Vec<Vec<String>> { let mut v: Vec<Vec<String>> = self.opened.iter().filter(|(_, o)| o.expanded).map(|(id, _)| self.rel_of(*id)).collect(); v.sort(); v }

    /// An entry is part of the view as it is set: a sequence when sequences are shown as one (and it is not split), its
    /// frames when they are not.
    fn in_view(&self, id: u32) -> bool {
        let n = &self.nodes[id as usize];
        if n.seq != NONE { return self.sequences && !self.seqs[n.seq as usize].split }
        // (a file forced into a later sequence belongs to that one: `frame_of` names the last)
        n.frame_of == NONE || !(self.sequences && !self.seqs[n.frame_of as usize].split)
    }
    fn shows(&self, id: u32) -> bool { self.in_view(id) && (self.show_hidden || !self.nodes[id as usize].hidden) }
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
        self.hidden_top = if self.show_hidden { 0 } else { self.top.iter().filter(|id| self.in_view(**id) && self.nodes[**id as usize].hidden).count() };
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
    #[test]
    fn an_image_sequence_is_one_entry_and_a_hand_can_overrule_the_rules() {
        let mut entries: Vec<RawEntry> = (1001..=1240).map(|f| file(&format!("shot_010.{f}.exr"), 1000, f as i64)).collect();
        entries.extend([file("comp_v001.exr", 5, 1), file("comp_v002.exr", 5, 2), file("comp_v003.exr", 5, 3), file("notes.txt", 7, 4), dir("renders", 5), file("two.0001.dpx", 1, 6), file("two.0002.dpx", 1, 7)]);
        let mut m = model(entries);
        // 240 frames are ONE entry: among the files, with the frames' total size, the newest frame's date, its own kind
        assert_eq!(names(&m), ["renders", "comp_v001.exr", "comp_v002.exr", "comp_v003.exr", "notes.txt", "shot_010.[1001-1240].exr", "two.0001.dpx", "two.0002.dpx"], "versions are not frames; two files are not a clip");
        let seq = m.find_top("shot_010.[1001-1240].exr").unwrap(); assert!(m.is_sequence(seq)); assert_eq!((m.node(seq).size, m.node(seq).mtime), (240_000, 1240)); assert_eq!(m.kind(seq), "OpenEXR sequence"); assert_eq!(m.ext(seq), "exr"); assert_eq!(m.frames(seq), Some((240, 0)));
        assert_eq!(m.counts(), (8, 0)); assert_eq!(m.files_of(seq).len(), 240); assert_eq!(&*m.node(m.rep(seq)).name, "shot_010.1121.exr", "the middle frame stands for it");
        let frame = m.find_top("shot_010.1001.exr").unwrap(); assert_eq!(m.row_of(frame), None); assert_eq!(m.shown_as(frame), seq); assert_eq!(m.shown_as(m.find_top("notes.txt").unwrap()), m.find_top("notes.txt").unwrap());
        // the search finds it by its own name
        m.set_filter("shot_010"); m.rebuild(); assert_eq!(names(&m), ["shot_010.[1001-1240].exr"]); m.set_filter(""); m.rebuild();
        // sequences off: every file by itself, and no sequence entry
        m.sequences = false; m.rebuild(); assert_eq!(m.len(), 240 + 7); assert_eq!(m.row_of(seq), None); assert!(m.row_of(frame).is_some()); assert_eq!(m.shown_as(frame), frame);
        m.sequences = true; m.rebuild(); assert_eq!(m.len(), 8);
        // a hand: these three versions ARE a sequence; and that long one is to be shown as its files
        let v: Vec<u32> = ["comp_v001.exr", "comp_v002.exr", "comp_v003.exr"].iter().map(|n| m.find_top(n).unwrap()).collect();
        let forced = m.force_sequence(&v).unwrap(); m.rebuild(); assert_eq!(&*m.node(forced).name, "comp_v[001-003].exr"); assert_eq!(names(&m)[1], "comp_v[001-003].exr"); assert_eq!(m.len(), 6);
        assert!(m.force_sequence(&[m.find_top("notes.txt").unwrap(), m.find_top("two.0001.dpx").unwrap()]).is_none(), "names that differ in more than a number");
        m.split(seq); m.rebuild(); assert_eq!(m.len(), 5 + 240); assert!(m.row_of(frame).is_some() && m.row_of(seq).is_none());
        // a gap in the range is counted
        let g = model(vec![file("a.0001.exr", 1, 1), file("a.0002.exr", 1, 1), file("a.0009.exr", 1, 1)]); assert_eq!(g.frames(g.find_top("a.[0001-0009].exr").unwrap()), Some((3, 6)));
        // a folder opened in place has its own sequences
        let mut t = model(vec![dir("renders", 1)]); let r = t.find_top("renders").unwrap(); t.expand(r); t.add(r, (1..=5).map(|f| file(&format!("b.{f:04}.exr"), 1, 1)).collect()); t.set_state(r, Kids::Loaded); t.rebuild();
        assert_eq!(names(&t), ["renders", "  b.[0001-0005].exr"]);
    }

}
