//! A pane: one place, its listing, its history (canon rows 447, 449, 451, 463). The LIST is min-ui's table with
//! folders that open in place; the GRID its tiles. The pane owns the model and hands the views what each row shows.
//!
//! The file system is never asked on this thread: a navigation starts a worker (`lister`) and takes its chunks as
//! they come; whatever arrives for a reading the pane no longer waits for is dropped (its request number is unknown).
//! No watcher and no polling (a ruling): the listing is read again on navigation, on Refresh, and when the window
//! becomes active - then only if the folder's own modification time changed.
use crate::format::{self, Civil};
use crate::history::{History, Look};
use crate::icons::{bucket, Icons};
use crate::lister::{self, Msg, Port};
use crate::model::{Kids, Model, SortBy, TOP};
use crate::thumbs::{self, Thumbs};
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use min_ui::{fonts::draw_layout, grid::Grid, paint, selection::Selection, table::{self, Column, RowInfo, Table, TableSource}, Event, Family, Fonts, GridSource, KeyEvent, Mods, ViewEvent};
use std::borrow::Cow;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use vello::Scene;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode { List, Grid }
#[derive(Clone, Debug, PartialEq)]
pub enum Load { Loading, Ready, Failed(String) }

pub struct Pane {
    pub history: History, pub model: Model, pub table: Table, pub grid: Grid, pub mode: Mode, pub load: Load,
    /// the room left on the folder's volume; the folder's own modification time at the last reading
    pub free: Option<u64>, mtime: Option<i64>,
    /// the folders opened in place, each with its own modification time as it was read (a file taken out of one of them changes that, not the folder in view's)
    inner: Vec<(PathBuf, Option<i64>)>,
    /// the readings waited for: request -> the entry they are for ([`TOP`]: the folder itself), each with its way to call it off
    reqs: HashMap<u64, (u32, Arc<AtomicBool>)>, probe: Option<u64>,
    /// what to put back when the reading is over: the choice and the scroll (back / forward / up / refresh), the folders to open in place again (refresh)
    restore: Option<Look>, reopen: Vec<Vec<String>>,
    /// (how many are chosen, the bytes of the chosen files)
    chosen: (usize, u64),
    /// the search field's text for this pane (it filters the folder in view; a navigation clears it)
    pub search: String,
    /// set when the folder itself has just been read to its end (the window tells the thumbnails' cache)
    pub listed: bool,
    /// a context menu was asked for: on a row (already chosen) or on the empty part, at this point (the window shows it)
    pub menu_at: Option<(Option<usize>, (f64, f64))>,
    /// a press on a chosen entry was moved: the window starts the system's drag of what is chosen
    pub drag_out: bool,
    /// what a hand said of this pane's sequences (row 475): those to be shown as separate files (by their entry's place and name),
    /// and files to be one sequence whatever the rules say; said again when their folder is read again
    split: Vec<PathBuf>, forced: Vec<Vec<PathBuf>>,
    /// a double click on a file (the row): the lightbox, or the default application (the window decides); "Open" on a file: the default application
    pub preview: Option<usize>, pub open_default: Option<PathBuf>,
    pub rect: Rect, scale: f64,
}

/// The table's rows as the model has them.
struct Rows<'a> { model: &'a Model, icons: &'a Icons, px: u32, today: Civil,
    /// the pictures of single files: the numbers they are looked up by, for the rows in view from `first` on
    thumbs: &'a Thumbs, keys: &'a [Option<u64>], first: usize }
impl Rows<'_> {
    /// A row's own picture (a thumbnail, the icon of that very file) when it has come; else its type's icon.
    fn picture(&self, i: usize) -> Option<&peniko::ImageBrush> {
        i.checked_sub(self.first).and_then(|k| self.keys.get(k)).copied().flatten().and_then(|k| self.thumbs.peek(k)).or_else(|| self.icons.peek(self.model.ext(self.model.row(i)?), self.px))
    }
}
impl TableSource for Rows<'_> {
    fn len(&self) -> usize { self.model.len() }
    fn row(&self, i: usize) -> RowInfo {
        match self.model.row(i) { Some(id) => { let n = self.model.node(id); RowInfo { indent: n.depth, expandable: n.dir, expanded: self.model.is_expanded(id), dim: n.hidden, plain: false } }
            None => RowInfo { indent: self.model.note(i).map(|n| n.0).unwrap_or(0), expandable: false, expanded: false, dim: true, plain: true } }
    }
    fn text(&self, i: usize, col: usize) -> Cow<'_, str> {
        let Some(id) = self.model.row(i) else { return if col == 0 { Cow::Owned(self.model.note(i).map(|n| n.1).unwrap_or_default()) } else { Cow::Borrowed("") } };
        let n = self.model.node(id);
        match col { 0 => Cow::Borrowed(&*n.name), 1 => Cow::Owned(format::date_text(n.mtime, self.today)), 2 => if n.dir || n.package { Cow::Borrowed("--") } else { Cow::Owned(format::size_text(n.size, format::DECIMAL)) }, _ => Cow::Borrowed(self.model.kind(id)) }
    }
    fn icon(&self, i: usize) -> Option<&peniko::ImageBrush> { self.picture(i) }
}
impl GridSource for Rows<'_> {
    fn len(&self) -> usize { self.model.len() }
    fn name(&self, i: usize) -> Cow<'_, str> { match self.model.row(i) { Some(id) => Cow::Borrowed(&*self.model.node(id).name), None => Cow::Borrowed("") } }
    fn image(&self, i: usize) -> Option<&peniko::ImageBrush> { self.picture(i) }
    fn dim(&self, i: usize) -> bool { self.model.row(i).is_some_and(|id| self.model.node(id).hidden) }
}
/// The rows' names alone: what the keys need (type-ahead, the triangle), with no icons in hand.
struct Names<'a>(&'a Model);
impl TableSource for Names<'_> {
    fn len(&self) -> usize { self.0.len() }
    fn row(&self, i: usize) -> RowInfo { match self.0.row(i) { Some(id) => { let n = self.0.node(id); RowInfo { indent: n.depth, expandable: n.dir, expanded: self.0.is_expanded(id), dim: n.hidden, plain: false } } None => RowInfo { indent: self.0.note(i).map(|n| n.0).unwrap_or(0), plain: true, ..Default::default() } } }
    fn text(&self, i: usize, col: usize) -> Cow<'_, str> { match (self.0.row(i), col) { (Some(id), 0) => Cow::Borrowed(&*self.0.node(id).name), _ => Cow::Borrowed("") } }
}
impl GridSource for Names<'_> { fn len(&self) -> usize { self.0.len() } fn name(&self, i: usize) -> Cow<'_, str> { match self.0.row(i) { Some(id) => Cow::Borrowed(&*self.0.node(id).name), None => Cow::Borrowed("") } } }

pub fn columns() -> Vec<Column> { vec![Column::new("Name", 0.0, 160.0), Column::new("Date Modified", 150.0, 70.0), Column::new("Size", 86.0, 54.0).right(), Column::new("Kind", 150.0, 60.0)] }

impl Pane {
    pub fn new(path: PathBuf, port: &Port) -> Pane {
        let mut p = Pane { history: History::new(path), model: Model::new(), table: Table::new(columns()), grid: Grid::new(96.0), mode: Mode::List, load: Load::Loading, free: None, mtime: None, inner: vec![], reqs: HashMap::new(), probe: None, restore: None, reopen: vec![], chosen: (0, 0), search: String::new(), listed: false, menu_at: None, drag_out: false, split: vec![], forced: vec![], preview: None, open_default: None, rect: Rect::ZERO, scale: 1.0 };
        p.read(port, None, vec![]); p
    }
    pub fn path(&self) -> &Path { self.history.current() }
    /// The folder's name, as a title.
    pub fn title(&self) -> String { self.path().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| self.path().to_string_lossy().into_owned()) }
    pub fn sel(&self) -> &Selection { if self.mode == Mode::List { &self.table.sel } else { &self.grid.sel } }
    fn sel_mut(&mut self) -> &mut Selection { if self.mode == Mode::List { &mut self.table.sel } else { &mut self.grid.sel } }
    fn scroll(&self) -> f64 { if self.mode == Mode::List { self.table.bar.offset } else { self.grid.bar.offset } }
    pub fn loading(&self) -> bool { !self.reqs.is_empty() }

    /// How the pane looks now, to come back to: the chosen entries of the folder itself, the scroll.
    fn look(&self) -> Look {
        let selected = self.sel().iter().filter_map(|i| self.model.row(i)).filter(|id| self.model.node(*id).depth == 0).take(10_000).map(|id| self.model.node(id).name.to_string()).collect();
        Look { selected, scroll: self.scroll() }
    }
    /// Read the current folder anew. `restore`: what to choose and where to scroll once it is read.
    fn read(&mut self, port: &Port, restore: Option<Look>, reopen: Vec<Vec<String>>) {
        for (_, (_, cancel)) in self.reqs.drain() { cancel.store(true, Ordering::Relaxed); }
        self.model.clear(); self.model.tree = self.mode == Mode::List; self.model.rebuild();
        self.table.sel.reset(); self.grid.sel.reset(); self.table.sel.resize(0); self.grid.sel.resize(0); self.table.bar.offset = 0.0; self.grid.bar.offset = 0.0;
        self.free = None; self.inner.clear(); self.load = Load::Loading; self.restore = restore; self.reopen = reopen; self.chosen = (0, 0); self.probe = None; self.search.clear();
        let (req, cancel) = (lister::next_req(), Arc::new(AtomicBool::new(false)));
        self.reqs.insert(req, (TOP, cancel.clone())); lister::list(port, req, self.path().to_path_buf(), cancel);
    }
    pub fn go(&mut self, port: &Port, path: PathBuf) { let look = self.look(); if self.history.go(path, look) { self.read(port, None, vec![]); } }
    pub fn back(&mut self, port: &Port) { let look = self.look(); if let Some(stop) = self.history.back(look) { self.read(port, Some(stop.look), vec![]); } }
    pub fn forward(&mut self, port: &Port) { let look = self.look(); if let Some(stop) = self.history.forward(look) { self.read(port, Some(stop.look), vec![]); } }
    /// To the enclosing folder; the folder left is the one chosen there.
    pub fn up(&mut self, port: &Port) { let look = self.look(); if let Some(stop) = self.history.up(look) { self.read(port, Some(stop.look), vec![]); } }
    /// Read again what is shown: the choice, the scroll and the folders opened in place are kept as far as they still exist.
    pub fn refresh(&mut self, port: &Port) { let (look, open, search) = (self.look(), self.model.expanded(), self.search.clone()); self.read(port, Some(look), open); self.model.set_filter(&search); self.search = search; }
    /// Read again and, once read, choose the entries with these names (what an operation made).
    pub fn refresh_choosing(&mut self, port: &Port, names: Vec<String>) {
        let (mut look, open, search) = (self.look(), self.model.expanded(), self.search.clone()); if !names.is_empty() { look.selected = names; }
        self.read(port, Some(look), open); self.model.set_filter(&search); self.search = search;
    }
    /// How many entries are chosen.
    pub fn chosen_count(&self) -> usize { self.chosen.0 }
    /// The entry at `path`, when it is in this pane's listing (the folder's own, or one opened in place).
    fn id_at(&self, path: &Path) -> Option<u32> { let rel: Vec<String> = path.strip_prefix(self.path()).ok()?.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect(); self.model.id_of(&rel) }
    /// Where the name of the entry at `path` is written now: the place of the field that renames it.
    pub fn name_rect(&self, path: &Path) -> Option<Rect> {
        let id = self.id_at(path)?; let row = self.model.row_of(id)?;
        Some(if self.mode == Mode::List { let n = self.model.node(id); self.table.name_rect(row, &RowInfo { indent: n.depth, expandable: n.dir, expanded: false, dim: false, plain: false }) } else { self.grid.label_rect(row) })
    }
    /// Another entry beside the one at `path` already has `name` (letters' case apart).
    pub fn name_taken(&self, path: &Path, name: &str) -> bool {
        let Some(id) = self.id_at(path) else { return false };
        self.model.find_ci(self.model.node(id).parent, name).is_some_and(|other| other != id)
    }
    /// The window became active: ask (off this thread) whether the folder changed since it was read.
    pub fn activated(&mut self, port: &Port) { if self.load == Load::Loading || self.probe.is_some() { return } let req = lister::next_req(); self.probe = Some(req); lister::probe(port, req, self.path().to_path_buf(), self.inner.clone()); }

    /// Change the model (its entries, its options) and keep what is chosen, by entry, across the new rows.
    fn keep(&mut self, change: impl FnOnce(&mut Model)) {
        let (ids, cursor, anchor): (Vec<u32>, Option<u32>, Option<u32>) = { let s = self.sel(); (s.iter().filter_map(|i| self.model.row(i)).collect(), s.cursor.and_then(|c| self.model.row(c)), s.anchor.and_then(|a| self.model.row(a))) };
        change(&mut self.model); self.model.rebuild();
        let n = self.model.len(); let rows: Vec<usize> = ids.iter().filter_map(|id| self.model.row_of(*id)).collect();
        let (cursor, anchor) = (cursor.and_then(|c| self.model.row_of(c)), anchor.and_then(|a| self.model.row_of(a)));
        self.table.sel.resize(n); self.grid.sel.resize(n);
        let s = self.sel_mut(); s.set(rows); s.cursor = cursor; s.anchor = anchor;
        self.count();
    }
    fn count(&mut self) { let (mut n, mut bytes) = (0, 0u64); for i in self.sel().iter() { n += 1; if let Some(id) = self.model.row(i) { let e = self.model.node(id); if !e.dir { bytes += e.size; } } } self.chosen = (n, bytes); }
    /// Choose the entries of the folder with these names (what a stop of the history kept); the first is brought into view.
    fn choose(&mut self, names: &[String]) {
        let rows: Vec<usize> = names.iter().filter_map(|n| self.model.find_top(n)).filter_map(|id| self.model.row_of(id)).collect();
        let first = rows.iter().copied().min(); let s = self.sel_mut(); s.set(rows); s.cursor = first; s.anchor = first; self.count();
    }
    /// Start reading a folder opened in place.
    fn read_inside(&mut self, port: &Port, id: u32) { let (req, cancel) = (lister::next_req(), Arc::new(AtomicBool::new(false))); self.reqs.insert(req, (id, cancel.clone())); lister::list(port, req, self.model.path_of(id, self.path()), cancel); }
    /// After a refresh: open again, as their parents are read, the folders that were open.
    fn reopen(&mut self, port: &Port) {
        let wanted = std::mem::take(&mut self.reopen); let mut later = vec![];
        for path in wanted {
            // walk the names down through what is read so far
            let mut at = TOP; let mut found = true;
            for name in &path { match self.model.find_in(at, name) { Some(id) => at = id, None => { found = false; break } } }
            if found && at != TOP { if self.model.expand(at) { self.read_inside(port, at); } } else if self.model.waits(&path) { later.push(path); }
        }
        self.reopen = later;
    }
    /// Something came from a worker. True when it was this pane's.
    pub fn on_msg(&mut self, port: &Port, m: &mut Option<Msg>) -> bool {
        let req = match m { Some(Msg::Chunk { req, .. } | Msg::Done { req, .. } | Msg::Failed { req, .. } | Msg::Probed { req, .. }) => *req, _ => return false };
        if self.probe == Some(req) {
            if let Some(Msg::Probed { mtime, inner_changed, .. }) = m.take() { self.probe = None; if self.load == Load::Ready && ((mtime.is_some() && mtime != self.mtime) || inner_changed) { self.refresh(port); } else if matches!(self.load, Load::Failed(_)) && mtime.is_some() { self.refresh(port); } }
            return true
        }
        let Some((parent, _)) = self.reqs.get(&req).map(|(p, c)| (*p, c.clone())) else { return false };
        match m.take() {
            Some(Msg::Chunk { entries, .. }) => self.keep(|model| model.add(parent, entries)),
            Some(Msg::Done { mtime, free, .. }) => {
                self.reqs.remove(&req); self.keep(|model| model.set_state(parent, Kids::Loaded)); self.reapply_sequences(parent); self.keep(|_| {});
                if parent != TOP { let at = self.model.path_of(parent, self.path()); self.inner.retain(|(p, _)| *p != at); self.inner.push((at, mtime)); }
                if parent == TOP {
                    self.load = Load::Ready; self.mtime = mtime; self.free = free; self.listed = true;
                    if let Some(look) = self.restore.take() { self.choose(&look.selected); if self.mode == Mode::List { self.table.bar.offset = look.scroll; } else { self.grid.bar.offset = look.scroll; } if look.scroll == 0.0 { if let Some(c) = self.sel().cursor { self.reveal_later(c); } } }
                }
                self.reopen(port); self.keep(|_| {});
            }
            Some(Msg::Failed { why, .. }) => { self.reqs.remove(&req); if parent == TOP { self.load = Load::Failed(why.clone()); } self.keep(|model| model.set_state(parent, Kids::Failed(why))); }
            _ => {}
        }
        true
    }
    /// A row to bring into view at the next layout (the views know their size only then).
    fn reveal_later(&mut self, row: usize) { let rh = table::ROW_HEIGHT * self.scale; if self.mode == Mode::List { self.table.bar.offset = (row as f64 * rh - 2.0 * rh).max(0.0); } else { let g = self.grid.geometry(); self.grid.bar.offset = (g.cell(row).y0 - g.top).max(0.0); } }

    pub fn set_mode(&mut self, mode: Mode) {
        if mode == self.mode { return }
        // the choice goes with the entries from one view to the other
        let (ids, cursor): (Vec<u32>, Option<u32>) = { let s = self.sel(); (s.iter().filter_map(|i| self.model.row(i)).collect(), s.cursor.and_then(|c| self.model.row(c))) };
        self.sel_mut().reset(); self.mode = mode; self.model.tree = mode == Mode::List; self.model.rebuild();
        let n = self.model.len(); self.table.sel.resize(n); self.grid.sel.resize(n);
        let rows: Vec<usize> = ids.iter().filter_map(|id| self.model.row_of(*id)).collect(); let cursor = cursor.and_then(|c| self.model.row_of(c)).or(rows.first().copied());
        let s = self.sel_mut(); s.set(rows); s.cursor = cursor; s.anchor = cursor; self.count();
        if let Some(c) = cursor { self.reveal_later(c); }
    }
    pub fn set_sort(&mut self, by: SortBy, ascending: bool) { self.table.sort = (by.column(), ascending); self.keep(|m| { m.sort = by; m.ascending = ascending; }); }
    pub fn set_hidden(&mut self, on: bool) { self.keep(|m| m.show_hidden = on); }
    pub fn set_folders_first(&mut self, on: bool) { self.keep(|m| m.folders_first = on); }
    /// The search field's text filters the folder in view.
    pub fn set_search(&mut self, text: &str) { self.search = text.to_string(); if self.model.filter() != text.to_lowercase() { self.keep(|m| m.set_filter(text)); } }
    pub fn select_all(&mut self) { self.sel_mut().select_all(); self.count(); }
    /// Rows a..=b are chosen (the scripted looks).
    pub fn select_rows(&mut self, a: usize, b: usize) { let n = self.model.len(); if n == 0 { return } let s = self.sel_mut(); s.resize(n); s.select_only(a.min(n - 1)); s.extend_to(b.min(n - 1)); self.count(); }
    pub fn scroll_to(&mut self, row: usize) { self.reveal_later(row); }

    /// Open a folder in place (the list).
    pub fn expand(&mut self, port: &Port, row: usize) { let Some(id) = self.model.row(row) else { return }; let mut read = false; self.keep(|m| read = m.expand(id)); if read { self.read_inside(port, id); } }
    pub fn collapse(&mut self, row: usize) { if let Some(id) = self.model.row(row) { self.keep(|m| m.collapse(id)); } }
    /// A double click, the open key: a folder is gone into. A file: nothing yet (the lightbox is a later slice; the system is NOT asked to open it).
    pub fn activate(&mut self, port: &Port, row: usize) { if let Some(id) = self.model.row(row) { if self.model.node(id).dir { let p = self.model.path_of(id, self.path()); self.go(port, p); } else { self.preview = Some(row); } } }
    /// The Open command: the entry under the cursor, when it is chosen - a folder is gone into, a file is opened in its default application (row 473, point 2).
    pub fn open_chosen(&mut self, port: &Port) { if let Some(c) = self.sel().cursor.filter(|c| self.sel().is_selected(*c)) { match self.model.row(c) { Some(id) if !self.model.node(id).dir => self.open_default = Some(self.model.path_of(id, self.path())), _ => self.activate(port, c) } } }
    /// What row `row` is, for the lightbox: (where, its name, its size, its kind, whether it is a folder).
    pub fn entry(&self, row: usize) -> Option<(PathBuf, String, u64, String, bool)> { let id = self.model.row(row)?; let n = self.model.node(id); Some((self.model.path_of(self.model.rep(id), self.path()), n.name.to_string(), n.size, self.model.kind(id).to_string(), n.dir)) }
    /// The sequence at a row, as the player opens it: the pattern of its frames' names (`dir/shot_010.%04d.exr`), its
    /// first and last frame. None: no sequence, or one made by hand whose names are no such pattern.
    pub fn sequence_at(&self, row: usize) -> Option<(String, i64, i64)> {
        let id = self.model.row(row)?; if !self.model.is_sequence(id) { return None } let files = self.model.files_of(id);
        let parse = |f: u32| min_paths::sequence::parse_frame(&self.model.node(f).name).map(|(b, s, d, e)| (b, s, d, e.to_string()));
        let (base, sep, digits, ext) = parse(*files.first()?)?; let (b2, s2, last, e2) = parse(*files.last()?)?;
        if (b2.to_lowercase(), s2, e2.to_lowercase(), last.len()) != (base.to_lowercase(), sep.clone(), ext.to_lowercase(), digits.len()) { return None }
        let dir = self.model.path_of(*files.first()?, self.path()).parent()?.to_path_buf(); let esc = |t: &str| t.replace('%', "%%");
        Some((dir.join(format!("{}{}%0{}d.{}", esc(&base), esc(&sep), digits.len(), esc(&ext))).to_string_lossy().into_owned(), digits.parse().ok()?, last.parse().ok()?))
    }
    /// The row under the cursor, when it is chosen.
    pub fn cursor_row(&self) -> Option<usize> { self.sel().cursor.filter(|c| self.sel().is_selected(*c)) }
    /// The row the entry at `path` is shown at.
    pub fn row_at_path(&self, path: &Path) -> Option<usize> { self.model.row_of(self.model.shown_as(self.id_at(path)?)) }
    /// The next row (`step` 1) or the one before (-1) that is not a folder.
    pub fn next_file(&self, from: usize, step: i32) -> Option<usize> { let n = self.model.len() as i64; let mut i = from as i64 + step as i64; while i >= 0 && i < n { if self.model.row(i as usize).is_some_and(|id| !self.model.node(id).dir) { return Some(i as usize) } i += step as i64; } None }
    /// Choose this row alone and bring it into view (the lightbox moved on to it).
    pub fn choose_row(&mut self, row: usize) { self.select_rows(row, row); if self.mode == Mode::List { self.table.reveal(row); } else { self.grid.reveal(row); } }

    /// The folder shown at a point of the pane, if one is.
    pub fn folder_at(&self, x: f64, y: f64) -> Option<PathBuf> {
        let row = if self.mode == Mode::List { self.table.row_at(x, y) } else { self.grid.tile_at(x, y) }?;
        let id = self.model.row(row)?; self.model.node(id).dir.then(|| self.model.path_of(id, self.path()))
    }
    /// The chosen entries, for a menu: (where, it is a folder, it is a package). No more than `max`.
    /// (A sequence shown as one entry is given by the frame that stands for it - its middle one: what a menu's "Open"
    /// hands to an application. For everything that acts on FILES see [`Pane::chosen_files`].)
    pub fn chosen_entries(&self, max: usize) -> Vec<(PathBuf, bool, bool)> {
        self.sel().iter().filter_map(|i| self.model.row(i)).take(max).map(|id| { let n = self.model.node(id); (self.model.path_of(self.model.rep(id), self.path()), n.dir, n.package) }).collect()
    }
    /// Every file and folder the chosen entries stand for: a sequence is all of its frames (copy, move, trash, a drag).
    pub fn chosen_files(&self) -> Vec<PathBuf> { self.sel().iter().filter_map(|i| self.model.row(i)).flat_map(|id| self.model.files_of(id)).map(|id| self.model.path_of(id, self.path())).collect() }
    /// The chosen entries for Duplicate: the plain files and folders; and each chosen sequence as its frames - (the
    /// file, the sequence's base, what follows the base in the frame's name) - so that it is duplicated AS a sequence.
    /// A sequence made by hand whose names the detector cannot read goes with the plain files.
    pub fn chosen_for_duplicate(&self) -> (Vec<PathBuf>, Vec<Vec<(PathBuf, String, String)>>) {
        let (mut plain, mut seqs) = (vec![], vec![]);
        for id in self.sel().iter().filter_map(|i| self.model.row(i)) {
            let files = self.model.files_of(id);
            let frames: Option<Vec<(PathBuf, String, String)>> = self.model.is_sequence(id).then(|| files.iter().map(|f| { let name = &*self.model.node(*f).name; let (base, ..) = min_paths::sequence::parse_frame(name)?; Some((self.model.path_of(*f, self.path()), base.clone(), name[base.len()..].to_string())) }).collect()).flatten();
            match frames { Some(fr) => seqs.push(fr), None => plain.extend(files.iter().map(|f| self.model.path_of(*f, self.path()))) }
        }
        (plain, seqs)
    }
    /// The one chosen entry is a sequence shown as one.
    pub fn chosen_sequence(&self) -> bool { let mut it = self.sel().iter().filter_map(|i| self.model.row(i)); matches!((it.next(), it.next()), (Some(id), None) if self.model.is_sequence(id)) }
    /// Sequences are shown as one entry each, or every file by itself (row 475).
    pub fn set_sequences(&mut self, on: bool) { self.keep(|m| m.sequences = on); }
    /// The chosen sequence is shown as its separate files after all; remembered for this folder while the pane lives.
    pub fn split_chosen(&mut self) {
        let ids: Vec<u32> = self.sel().iter().filter_map(|i| self.model.row(i)).filter(|id| self.model.is_sequence(*id)).collect(); if ids.is_empty() { return }
        for id in &ids { let at = self.model.path_of(*id, self.path()); if !self.split.contains(&at) { self.split.push(at); } }
        self.keep(|m| for id in &ids { m.split(*id); });
    }
    /// The chosen files become one sequence, whatever the rules say. False: their names differ in more than a number.
    pub fn group_chosen(&mut self) -> bool {
        let ids: Vec<u32> = self.sel().iter().filter_map(|i| self.model.row(i)).collect(); let paths: Vec<PathBuf> = ids.iter().map(|id| self.model.path_of(*id, self.path())).collect();
        let mut made = None; self.keep(|m| made = m.force_sequence(&ids));
        match made { Some(id) => { self.forced.push(paths); if let Some(row) = self.model.row_of(id) { self.choose_row(row); } true } None => false }
    }
    /// After a folder was read: what a hand said of its sequences before is said again (split ones, forced ones).
    fn reapply_sequences(&mut self, parent: u32) {
        let dir = if parent == TOP { self.path().to_path_buf() } else { self.model.path_of(parent, self.path()) };
        let rel = |p: &Path| -> Option<Vec<String>> { Some(p.strip_prefix(self.history.current()).ok()?.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect()) };
        let split: Vec<u32> = self.split.iter().filter(|p| p.parent() == Some(&dir)).filter_map(|p| self.model.id_of(&rel(p)?)).collect();
        let forced: Vec<Vec<u32>> = self.forced.iter().filter(|g| g.first().and_then(|p| p.parent()) == Some(&dir)).filter_map(|g| g.iter().map(|p| self.model.id_of(&rel(p)?)).collect::<Option<Vec<u32>>>()).collect();
        for id in split { self.model.split(id); } for g in forced { self.model.force_sequence(&g); }
    }
    /// The folders in view, each with where its row or tile is: a drop on one goes INTO it.
    pub fn folder_zones(&self) -> Vec<(Rect, PathBuf)> {
        let n = self.model.len(); let list = self.mode == Mode::List;
        let (a, b) = if list { table::visible_rows(self.table.bar.offset, self.table.body().height(), table::ROW_HEIGHT * self.scale, n) } else { self.grid.geometry().visible(self.grid.bar.offset, self.rect.height(), n) };
        let clip = if list { self.table.body() } else { self.rect };
        (a..b).filter_map(|i| { let id = self.model.row(i)?; if !self.model.node(id).dir { return None } let r = if list { self.table.row_rect(i) } else { self.grid.cell_rect(i) }.intersect(clip); (r.width() > 0.0 && r.height() > 0.0).then(|| (r, self.model.path_of(id, self.path()))) }).collect()
    }
    /// The middle of a row, as the pointer would find it (the scripted looks).
    pub fn row_point(&self, row: usize) -> Option<(f64, f64)> { if row >= self.model.len() { return None } let r = if self.mode == Mode::List { self.table.row_rect(row) } else { self.grid.cell_rect(row) }; Some(((r.x0 + r.x1) / 2.0, (r.y0 + r.y1) / 2.0)) }

    fn handle(&mut self, port: &Port, events: Vec<ViewEvent>) {
        for e in events { match e {
            ViewEvent::Activated(row) => { self.activate(port, row); return }
            ViewEvent::SelectionChanged => self.count(),
            ViewEvent::SortChanged(col, asc) => self.set_sort(SortBy::of_column(col), asc),
            ViewEvent::Expand(row) => self.expand(port, row),
            ViewEvent::Collapse(row) => self.collapse(row),
            ViewEvent::ContextMenu(row, at) => self.menu_at = Some((row, at)),
            ViewEvent::DragStart(_) => self.drag_out = true,
        } }
    }
    /// A pointer event. True when a repaint is needed.
    pub fn pointer(&mut self, e: &Event, mods: Mods, now: Instant, port: &Port) -> bool {
        let (events, repaint) = if self.mode == Mode::List { self.table.event(e, &Names(&self.model), mods, now) } else { self.grid.event(e, mods, now) };
        let any = !events.is_empty(); self.handle(port, events); repaint || any
    }
    /// A key for the view. True when it was the view's.
    pub fn key(&mut self, k: &KeyEvent, now: Instant, port: &Port) -> bool {
        let (events, taken) = if self.mode == Mode::List { self.table.key(k, &Names(&self.model), now) } else { self.grid.key(k, &Names(&self.model), now) };
        self.handle(port, events); taken
    }
    pub fn wheel(&mut self, dy: f64, now: Instant) -> bool { if self.mode == Mode::List { self.table.wheel(dy, now) } else { self.grid.wheel(dy, now) } }
    pub fn animating(&self, now: Instant) -> bool { self.table.animating(now) || self.grid.animating(now) }
    pub fn resizing(&self) -> bool { self.mode == Mode::List && self.table.resizing() }

    /// Place the views (before the events and the paint of a frame).
    pub fn layout(&mut self, rect: Rect, scale: f64, active: bool) {
        self.rect = rect; self.scale = scale; let n = self.model.len();
        self.table.focused = active; self.grid.focused = active;
        if self.mode == Mode::List { self.table.layout(rect, n, scale); } else { self.grid.layout(rect, n, scale); }
    }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, icons: &mut Icons, thumbs: &mut Thumbs, scale: f64, now: Instant) {
        let rect = self.rect; if rect.width() <= 1.0 || rect.height() <= 1.0 { return }
        let today = format::local(format::now_secs());
        // the icons of what is in view are asked for (they come later, on their own thread)
        let px = if self.mode == Mode::List { bucket(16.0 * scale) } else { bucket(self.grid.tile * scale) };
        let (a, b) = if self.mode == Mode::List { table::visible_rows(self.table.bar.offset, self.table.body().height(), table::ROW_HEIGHT * scale, self.model.len()) } else { self.grid.geometry().visible(self.grid.bar.offset, rect.height(), self.model.len()) };
        // ... and so are the pictures of single files, made in the helpers; only what is in view is asked for
        let grid = self.mode == Mode::Grid; let tpx = if grid { thumbs::picture_px(self.grid.tile * scale) } else { 32 };
        let mut keys: Vec<Option<u64>> = Vec::with_capacity(b.saturating_sub(a));
        for i in a..b {
            let mut key = None;
            if let Some(id) = self.model.row(i) {
                icons.get(self.model.ext(id), px);
                // (a sequence has ONE picture: its middle frame's - row 456, point 2)
                let src = self.model.rep(id); let n = self.model.node(src);
                if let Some(kind) = thumbs::plan(self.model.ext(src), n.package, n.dir, n.size, grid) { key = Some(thumbs.want(&self.model.path_of(src, self.history.current()), self.model.ext(src), n.size, n.mtime, tpx, kind)); }
            }
            keys.push(key);
        }
        let rows = Rows { model: &self.model, icons, px, today, thumbs, keys: &keys, first: a };
        if self.mode == Mode::List { self.table.paint(scene, fonts, &rows, now); } else { self.grid.paint(scene, fonts, &rows, now); }
        // an empty pane says why
        if self.model.len() == 0 {
            let (text, bad) = match &self.load { Load::Loading => ("Loading\u{2026}".to_string(), false), Load::Failed(why) => (format!("This folder cannot be read: {why}"), true),
                Load::Ready => (if self.model.filter().is_empty() { if self.model.counts().1 > 0 { "Only hidden items here".to_string() } else { "This folder is empty".to_string() } } else { "Nothing here matches the search".to_string() }, false) };
            let top = if self.mode == Mode::List { self.table.body().y0 } else { rect.y0 }; let size = t::BODY_SIZE_M * scale as f32;
            let icon = if bad { fonts.icon("warning", size * 1.2, paint::color(t::NOTICE_VISUAL_COLOR)) } else { None }; let iw = icon.as_ref().map(|i| i.width() as f64 + 8.0 * scale).unwrap_or(0.0);
            let l = fonts.layout_fit(&text, Family::Sans, size, t::FONT_WEIGHT_REGULAR as f32, paint::color(if bad { t::NEUTRAL_CONTENT_COLOR_DEFAULT } else { t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT }), rect.width() - 32.0 * scale - iw);
            let x = rect.x0 + (rect.width() - l.width() as f64 - iw) / 2.0; let y = top + 28.0 * scale;
            if let Some(i) = &icon { draw_layout(scene, i, Affine::translate((x, y + (l.height() as f64 - i.height() as f64) / 2.0))); }
            draw_layout(scene, &l, Affine::translate((x + iw, y)));
        }
    }
    /// The status bar's left: the items, the hidden ones, the chosen ones and their size.
    pub fn status(&self) -> String {
        if let Load::Failed(_) = self.load { return String::new() }
        let (shown, hidden) = self.model.counts(); let mut s = if shown == 1 { "1 item".to_string() } else { format!("{} items", group(shown)) };
        if hidden > 0 { s.push_str(&format!(", {} hidden", group(hidden))); }
        if self.loading() { s.push_str("  \u{b7}  reading\u{2026}"); }
        let (n, bytes) = self.chosen; if n > 0 { s.push_str(&format!("  \u{b7}  {} selected, {}", group(n), format::size_text(bytes, format::DECIMAL))); }
        // one sequence chosen: how many frames it has, and how many of its range are not there
        if n == 1 { if let Some((frames, missing)) = self.sel().iter().next().and_then(|i| self.model.row(i)).and_then(|id| self.model.frames(id)) { s.push_str(&format!("  \u{b7}  {} frames", group(frames))); if missing > 0 { s.push_str(&format!(", {} missing", group(missing))); } } }
        s
    }
}
/// 20000 as "20,000".
pub fn group(n: usize) -> String { let s = n.to_string(); let mut out = String::new(); for (i, c) in s.chars().enumerate() { if i > 0 && (s.len() - i) % 3 == 0 { out.push(','); } out.push(c); } out }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lister::tests::Temp;
    use std::sync::mpsc::{channel, Receiver};
    use std::time::Duration;
    fn port() -> (Port, Receiver<Msg>) { let (tx, rx) = channel(); (Port { tx, wake: Arc::new(|| {}) }, rx) }
    /// Take what the workers send until the pane waits for nothing more.
    fn settle(p: &mut Pane, port: &Port, rx: &Receiver<Msg>) { while p.loading() { let m = rx.recv_timeout(Duration::from_secs(30)).expect("the worker answers"); let mut m = Some(m); p.on_msg(port, &mut m); } }
    fn names(p: &Pane) -> Vec<String> { (0..p.model.len()).map(|i| match p.model.row(i) { Some(id) => format!("{}{}", "  ".repeat(p.model.node(id).depth as usize), p.model.node(id).name), None => "  <note>".into() }).collect() }
    fn chosen(p: &Pane) -> Vec<String> { p.sel().iter().filter_map(|i| p.model.row(i)).map(|id| p.model.node(id).name.to_string()).collect() }
    fn tree() -> Temp { let t = Temp::new("pane"); for f in ["shot_10.exr", "shot_2.exr", "notes.pdf"] { t.file(f, 10); } t.hidden_file(".secret", 10); t.file("renders/beauty_2.exr", 100); t.file("renders/beauty_10.exr", 100); t.file("renders/v002/a.txt", 1); t.dir("assets"); t }

    #[test]
    fn a_folder_is_listed_sorted_and_opened_in_place() {
        let t = tree(); let (port, rx) = port(); let mut p = Pane::new(t.0.clone(), &port);
        assert_eq!(p.load, Load::Loading); settle(&mut p, &port, &rx); assert_eq!(p.load, Load::Ready);
        assert_eq!(names(&p), ["assets", "renders", "notes.pdf", "shot_2.exr", "shot_10.exr"]); assert_eq!(p.status(), "5 items, 1 hidden"); assert!(p.free.is_some());
        p.expand(&port, 1); assert_eq!(names(&p)[2], "  <note>", "\"Loading\" under the folder until its entries come"); settle(&mut p, &port, &rx);
        assert_eq!(names(&p), ["assets", "renders", "  v002", "  beauty_2.exr", "  beauty_10.exr", "notes.pdf", "shot_2.exr", "shot_10.exr"]);
        // the choice stays on its entries when rows move: choose shot_2 and a file inside, then close the folder
        p.select_rows(6, 6); p.table.sel.click(3, Mods { shift: false, cmd: true }); p.count(); assert_eq!(chosen(&p), ["beauty_2.exr", "shot_2.exr"]); assert_eq!(p.chosen, (2, 110));
        p.collapse(1); assert_eq!(names(&p).len(), 5); assert_eq!(chosen(&p), ["shot_2.exr"]); assert_eq!(p.status(), "5 items, 1 hidden  \u{b7}  1 selected, 10 bytes");
        p.set_sort(SortBy::Name, false); assert_eq!(names(&p), ["renders", "assets", "shot_10.exr", "shot_2.exr", "notes.pdf"]); assert_eq!(chosen(&p), ["shot_2.exr"]);
        p.set_hidden(true); assert_eq!(names(&p).len(), 6); assert_eq!(p.status(), "6 items  \u{b7}  1 selected, 10 bytes");
        p.set_search("SHOT"); assert_eq!(names(&p), ["shot_10.exr", "shot_2.exr"]); p.set_search(""); assert_eq!(names(&p).len(), 6);
        // the grid shows the folder's own entries, and the choice goes with it
        p.set_sort(SortBy::Name, true); p.set_hidden(false); p.expand(&port, 1); settle(&mut p, &port, &rx); assert_eq!(names(&p).len(), 8);
        p.set_mode(Mode::Grid); assert_eq!(names(&p).len(), 5); assert_eq!(chosen(&p), ["shot_2.exr"]); p.set_mode(Mode::List); assert_eq!(names(&p).len(), 8); assert_eq!(chosen(&p), ["shot_2.exr"]);
    }
    #[test]
    fn navigation_history_and_what_comes_back() {
        let t = tree(); let (port, rx) = port(); let mut p = Pane::new(t.0.clone(), &port); settle(&mut p, &port, &rx);
        // into "renders" by its row; folders opened in place are forgotten by a navigation
        p.expand(&port, 0); settle(&mut p, &port, &rx); p.select_rows(3, 4); assert_eq!(chosen(&p), ["shot_2.exr", "shot_10.exr"]);
        p.activate(&port, 1); assert_eq!(p.path(), t.0.join("renders")); assert_eq!(p.model.len(), 0, "the old rows are gone at once"); settle(&mut p, &port, &rx);
        assert_eq!(names(&p), ["v002", "beauty_2.exr", "beauty_10.exr"]); assert!(p.history.can_back() && !p.history.can_forward());
        p.activate(&port, 1); assert_eq!(p.path(), t.0.join("renders"), "a file is not gone into"); assert_eq!(p.preview.take(), Some(1), "a double click on a file is the lightbox's (or the default application's): the window decides");
        assert_eq!(p.next_file(0, 1), Some(1)); assert_eq!(p.next_file(1, 1), Some(2)); assert_eq!(p.next_file(2, 1), None); assert_eq!(p.next_file(1, -1), None, "the folder before it is stepped over, and there is nothing before that");
        assert_eq!(p.entry(1).map(|e| (e.1, e.4)), Some(("beauty_2.exr".to_string(), false))); assert_eq!(p.row_at_path(&t.0.join("renders/beauty_10.exr")), Some(2));
        // back: the choice is put back; nothing is open in place any more
        p.back(&port); settle(&mut p, &port, &rx); assert_eq!(p.path(), t.0); assert_eq!(chosen(&p), ["shot_2.exr", "shot_10.exr"]); assert_eq!(names(&p).len(), 5); assert!(p.history.can_forward());
        p.forward(&port); settle(&mut p, &port, &rx); assert_eq!(p.path(), t.0.join("renders"));
        // up: the folder left is the one chosen
        p.up(&port); settle(&mut p, &port, &rx); assert_eq!(p.path(), t.0); assert_eq!(chosen(&p), ["renders"]);
        p.go(&port, t.0.join("renders").join("v002")); settle(&mut p, &port, &rx); assert_eq!(names(&p), ["a.txt"]); assert_eq!(p.title(), "v002");
        p.up(&port); settle(&mut p, &port, &rx); assert_eq!(chosen(&p), ["v002"]);
    }
    #[test]
    fn a_reading_nobody_waits_for_is_dropped_and_a_bad_folder_says_so() {
        let t = tree(); let (port, rx) = port(); let mut p = Pane::new(t.0.clone(), &port);
        // gone elsewhere before the first reading was taken in: its chunks are not this pane's any more
        let first = rx.recv_timeout(Duration::from_secs(30)).unwrap(); p.go(&port, t.0.join("renders"));
        let mut m = Some(first); assert!(!p.on_msg(&port, &mut m), "the first reading's answer is dropped"); assert_eq!(p.model.len(), 0);
        settle(&mut p, &port, &rx); assert_eq!(names(&p), ["v002", "beauty_2.exr", "beauty_10.exr"]);
        p.go(&port, t.0.join("nowhere")); settle(&mut p, &port, &rx); assert_eq!(p.load, Load::Failed("it does not exist".into())); assert_eq!(p.model.len(), 0); assert_eq!(p.status(), "");
        p.back(&port); settle(&mut p, &port, &rx); assert_eq!(p.load, Load::Ready, "the pane still goes back");
    }
    #[test]
    fn refresh_keeps_the_choice_and_what_is_open_and_sees_what_changed() {
        let t = tree(); let (port, rx) = port(); let mut p = Pane::new(t.0.clone(), &port); settle(&mut p, &port, &rx);
        p.expand(&port, 1); settle(&mut p, &port, &rx); p.expand(&port, 2); settle(&mut p, &port, &rx); assert_eq!(names(&p)[2..4], ["  v002", "    a.txt"]);
        p.select_rows(7, 7); assert_eq!(chosen(&p), ["shot_2.exr"]);
        t.file("renders/v002/b.txt", 1); t.file("added.mov", 5);
        p.refresh(&port); settle(&mut p, &port, &rx); settle(&mut p, &port, &rx);
        assert_eq!(names(&p), ["assets", "renders", "  v002", "    a.txt", "    b.txt", "  beauty_2.exr", "  beauty_10.exr", "added.mov", "notes.pdf", "shot_2.exr", "shot_10.exr"]); assert_eq!(chosen(&p), ["shot_2.exr"]);
        // the window becomes active: nothing changed - nothing is read again; something changed - it is
        p.activated(&port); let mut m = Some(rx.recv_timeout(Duration::from_secs(30)).unwrap()); assert!(p.on_msg(&port, &mut m)); assert!(!p.loading(), "unchanged: not read again");
        // (no second has to pass: the folder's time is compared as finely as the system keeps it)
        t.file("later.txt", 1);
        p.activated(&port); let mut m = Some(rx.recv_timeout(Duration::from_secs(30)).unwrap()); assert!(p.on_msg(&port, &mut m)); assert!(p.loading(), "the folder's own time changed: read again"); settle(&mut p, &port, &rx);
        assert!(names(&p).contains(&"later.txt".to_string()));
        // a file taken out of a folder OPENED IN PLACE changes that folder's time, not the one in view's: seen too
        assert!(names(&p).contains(&"    b.txt".to_string())); std::fs::remove_file(t.0.join("renders/v002/b.txt")).unwrap();
        p.activated(&port); let mut m = Some(rx.recv_timeout(Duration::from_secs(30)).unwrap()); assert!(p.on_msg(&port, &mut m)); assert!(p.loading(), "read again"); settle(&mut p, &port, &rx); settle(&mut p, &port, &rx);
        assert!(!names(&p).contains(&"    b.txt".to_string()) && names(&p).contains(&"    a.txt".to_string()));
        assert_eq!(group(20000), "20,000"); assert_eq!(group(999), "999"); assert_eq!(group(1234567), "1,234,567");
    }
}
