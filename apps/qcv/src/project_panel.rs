//! The left rail's Media section (titled "Add Media" until 2026-11-08 — the user: "rename … to Media … more logical"; QCView's own word for the list) — QCView's `LeftRail.qml` media card carried: the project's name in the header, the
//! "Media" list grouped by QCView's section labels (Videos · Audio · Images · Image Sequences · Playlists · Dual Views ·
//! Live) with the type glyph + a count on the bin header (the user 2026-11-07: "move the icons from the media items to the
//! bins too, we don't need so many icons"), rows = the name elided in the middle; the LOADED row carries QCView's treatment
//! (LeftRail.qml's muted cobalt fill — one of the four blue uses, canon row 162 — with the name BOLD in gray-900, Spectrum 2's
//! current-item treatment: fill + weight, no outline, since a ring means keyboard focus there; QCView's 1-px border not carried), selected rows in the deliberate grey; single click selects (Ctrl / Cmd
//! toggles, Shift extends a range), double-click LOADS (QCView: "activation moved to double-click"), an empty list reads
//! "Drop media here to add to the project". An "Add Media…" pill opens the file dialog.
//! The row context menu is QCView's `rowContextMenu` (Create Playlist from Selection · Delete); right-clicking a row outside
//! the selection replaces it first, inside keeps the group. F2 / the new-playlist flow rename inline (Enter / a click away
//! commits, Esc dismisses — QCView's `commitEditedItemName` / `renameClickAway`). The Playlists header is always shown with
//! its "+" (QCView's `createEmptyPlaylistAndRename`). DEVIATION: the menu's clip-chain entries (Input · Use Clip Chain of ·
//! Reset Clip Chain) arrive with the colour chain (canon §18.6) — they would be inert here.
//! THE LOADED PLAYLIST'S TREE (the user 2026-11-08: "go with the tree") — QCView's Inspector card "Playlist Contents" moved here:
//! the loaded playlist's row opens into its clips, indented, each with its type glyph; the playing clip in the quiet selected
//! grey (no blue); a click seeks to it (QCView `seekToPlaylistItem`); the clip count + canvas ride the playlist row in the
//! subdued voice. Dragging a clip row reorders the playlist (the user 2026-11-08: "is it possible to drag/drop reorder in this
//! tree?") — an insertion line between the rows, the row's chip under the pointer, ONE undoable move on release (the clip bar's
//! edit); a click without a drag seeks, on release. Trim / remove stay on the clip bar; Delete here still means the project's rows.
use kurbo::Rect;
use min_theme::tokens as t;
use min_ui::{anim::{mix, Ramp}, paint, ActionButton, Tooltips, Event, FieldResponse, Family, Fonts, ImeEvent, KeyEvent, Menu, MenuItem, Section, TextButton, TextField, Variant};
use qcv_project::{MediaType, ProjectManager};
use std::time::Instant;
use vello::Scene;

#[derive(Clone, Debug, PartialEq)]
pub enum Cmd { Load(String), Remove(Vec<String>), AddMedia, /// QCView `createPlaylistFromSelection`: the eligible ids in bin order
    CreatePlaylist(Vec<String>), /// QCView `createEmptyPlaylistAndRename`
    NewPlaylist, Rename(String, String), /// rows dragged out of the bin and released at (x, y) — the shell resolves the target
    DropRows(Vec<String>, f64, f64), /// a clip row of the loaded playlist's tree: seek to the n-th clip (QCView `seekToPlaylistItem`)
    SeekClip(usize), /// a tree row dragged to insertion slot `to` (0..=n among the playlist's clips)
    MoveClip { clip: String, to: usize } }

struct Row { id: String, name: String, media_type: MediaType, rect: Rect, hovered: bool, hover: Ramp, failed: bool, /// QCView's `unsupported`: the codec has no decoder, or the load failed → the warning badge
    unsupported: bool }
struct Group { label: &'static str, icon: &'static str, kind: MediaType, header: Rect, rows: Vec<Row> }
/// The loaded playlist's contents as the shell reads them off the transport.
#[derive(Clone, Debug, PartialEq)]
/// `clips` = (clip id, name, kind) in playlist order.
pub struct TreeModel { pub playlist: String, pub clips: Vec<(String, String, MediaType)>, pub current: Option<usize>, pub info: String }
struct Tree { model: TreeModel, rows: Vec<Rect>, hover: Option<usize>, /// a press on row i at (x, y); past the threshold it becomes the drag (row, pointer)
    press: Option<(usize, f64, f64)>, drag: Option<(usize, (f64, f64))> }

pub struct ProjectPanel {
    pub section: Section,
    add: TextButton,
    groups: Vec<Group>,
    pub selected: Vec<String>, last_selected: Option<String>,
    active: String, title: String, dirty: bool,
    revision_seen: u64, height: f64,
    last_click: Option<(String, Instant)>,
    menu: Menu, /// the row being renamed inline and its field
    editing: Option<(String, TextField)>, /// set by the shell after a create: start the rename once the row exists
    pub pending_rename: Option<String>, new_playlist: ActionButton,
    /// a press on a row (id, x, y) that becomes a drag past the threshold; the drag: (ids in bin order, label, pointer)
    press: Option<(String, f64, f64)>, drag: Option<(Vec<String>, String, (f64, f64))>,
    tooltips: Tooltips, badge_rects: Vec<(String, Rect)>,
    tree: Option<Tree>,
}

fn type_icon(t: MediaType) -> &'static str { match t { MediaType::Video => "video", MediaType::Audio => "music-notes", MediaType::Image => "image", MediaType::ImageSequence => "film-strip", MediaType::Playlist => "playlist", MediaType::DualPair => "frame-corners", MediaType::LiveStream => "broadcast" } }
/// QCView `sectionLabel`.
fn section_label(t: MediaType) -> &'static str { match t { MediaType::Video => "Videos", MediaType::Audio => "Audio", MediaType::Image => "Images", MediaType::ImageSequence => "Image Sequences", MediaType::Playlist => "Playlists", MediaType::DualPair => "Dual Views", MediaType::LiveStream => "Live" } }
const DOUBLE_CLICK_MS: u128 = 400;

impl ProjectPanel {
    pub fn new() -> ProjectPanel { ProjectPanel { section: Section::new("Media", true), add: TextButton::new("Add Media…", Variant::Secondary).size_s(), groups: vec![], selected: vec![], last_selected: None, active: String::new(), title: String::new(), dirty: false, revision_seen: u64::MAX, height: 0.0, last_click: None, menu: Menu::new(), editing: None, pending_rename: None, new_playlist: ActionButton::new("plus", "New Playlist").size_s(), press: None, drag: None, tooltips: Tooltips::default(), badge_rects: vec![], tree: None } }
    /// The loaded playlist's tree (None = no playlist loaded); true when it changed.
    pub fn set_tree(&mut self, m: Option<TreeModel>) -> bool {
        if self.tree.as_ref().map(|t| &t.model) == m.as_ref() { return false }
        let keep = self.tree.as_ref().map(|t| (t.press, t.drag)).unwrap_or((None, None));
        self.tree = m.map(|model| Tree { model, rows: vec![], hover: None, press: keep.0, drag: keep.1 }); true
    }
    /// Mirror the project when it changed (the manager's revision).
    pub fn sync(&mut self, pm: &ProjectManager, active_loaded_id: &str) -> bool {
        if pm.revision() == self.revision_seen && self.active == active_loaded_id { return false }
        self.revision_seen = pm.revision(); self.active = active_loaded_id.to_string();
        self.title = pm.name().to_string(); self.dirty = pm.dirty();
        // QCView's list: items in bin order, grouped by the section label of their type (ImageSequence splits out of the Images bin)
        let order = [MediaType::Video, MediaType::Audio, MediaType::Image, MediaType::ImageSequence, MediaType::Playlist, MediaType::DualPair, MediaType::LiveStream];
        let mut groups: Vec<Group> = vec![];
        for t in order {
            let mut rows = vec![];
            for bn in pm.bins() { for id in &bn.item_ids { if let Some(it) = pm.item(id) { if it.media_type == t { rows.push(Row { id: it.id.clone(), name: it.name.clone(), media_type: t, rect: Rect::ZERO, hovered: false, hover: Ramp::new(0.0, t::ANIMATION_DURATION_HOVER), failed: it.load_failed, unsupported: (it.video.loaded && it.video.unsupported_codec) || it.load_failed }); } } } }
            // the Playlists header stays even when empty: it carries the "+" (QCView's fallback Playlists header)
            if !rows.is_empty() || t == MediaType::Playlist { groups.push(Group { label: section_label(t), icon: type_icon(t), kind: t, header: Rect::ZERO, rows }); }
        }
        self.groups = groups;
        let ids: Vec<String> = self.groups.iter().flat_map(|g| g.rows.iter().map(|r| r.id.clone())).collect();
        self.selected.retain(|s| ids.contains(s));
        if self.editing.as_ref().is_some_and(|(id, _)| !ids.contains(id)) { self.editing = None; }
        true
    }
    /// A row drag is in flight: the shell routes every pointer event here until the release (QCView's MouseArea drag).
    pub fn is_dragging(&self) -> bool { self.drag.is_some() || self.tree.as_ref().is_some_and(|t| t.drag.is_some() || t.press.is_some()) }
    /// The harness: the tree's clip rows.
    pub fn tree_rows(&self) -> Vec<Rect> { self.tree.as_ref().map(|t| t.rows.clone()).unwrap_or_default() }
    /// The insertion slot (0..=n) for pointer y over the tree: past each row's middle.
    fn tree_slot(tr: &Tree, y: f64) -> usize { tr.rows.iter().filter(|r| y > r.center().y).count() }
    /// QCView's drag payload: the dragged row alone, or — when it is part of a multi-selection — every selected non-playlist
    /// row in bin order. Playlists are never dragged (no on-disk path).
    fn drag_payload(&self, id: &str) -> Vec<String> {
        let is_pl = |r: &Row| matches!(r.media_type, MediaType::Playlist | MediaType::DualPair);
        let rows: Vec<&Row> = self.groups.iter().flat_map(|g| g.rows.iter()).collect();
        if rows.iter().any(|r| r.id == id && is_pl(r)) { return vec![] }   // QCView: `Drag.active` is false on a playlist row, whatever the selection
        if self.selected.len() > 1 && self.selected.iter().any(|s| s == id) { rows.iter().filter(|r| !is_pl(r) && self.selected.contains(&r.id)).map(|r| r.id.clone()).collect() }
        else { rows.iter().filter(|r| r.id == id && !is_pl(r)).map(|r| r.id.clone()).collect() }
    }
    /// Is an inline rename open (the shell routes keys and IME here first)?
    pub fn is_editing(&self) -> bool { self.editing.is_some() }
    pub fn menu_open(&self) -> bool { self.menu.is_open() }
    /// F2 / after a create: open the inline editor on `id` with its name selected.
    pub fn begin_rename(&mut self, id: &str, fonts: &mut Fonts) -> bool {
        let Some(name) = self.groups.iter().flat_map(|g| g.rows.iter()).find(|r| r.id == id).map(|r| r.name.clone()) else { return false };
        let mut f = TextField::new(Family::Sans, t::RAIL_LABEL_SIZE); f.set_text(&name); f.focused = true; f.select_all(fonts);
        self.editing = Some((id.to_string(), f)); true
    }
    /// F2 with exactly one row selected (QCView: a no-op for 0 or 2+).
    pub fn rename_selection(&mut self, fonts: &mut Fonts) -> bool { if self.selected.len() == 1 { let id = self.selected[0].clone(); self.begin_rename(&id, fonts) } else { false } }
    /// Commit the open rename (Enter, a click away, focus lost); an unchanged or empty name commits nothing.
    pub fn commit_rename(&mut self) -> Option<Cmd> {
        let (id, f) = self.editing.take()?;
        let name = f.text().trim().to_string();
        let old = self.groups.iter().flat_map(|g| g.rows.iter()).find(|r| r.id == id).map(|r| r.name.clone()).unwrap_or_default();
        (!name.is_empty() && name != old).then(|| Cmd::Rename(id, name))
    }
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) -> Option<Cmd> {
        let (_, f) = self.editing.as_mut()?;
        match f.key(k, fonts) { FieldResponse::Accepted => self.commit_rename(), FieldResponse::Cancelled => { self.editing = None; None } _ => None }
    }
    pub fn ime(&mut self, e: &ImeEvent, fonts: &mut Fonts) { if let Some((_, f)) = self.editing.as_mut() { f.ime(e, fonts); } }
    /// QCView `selectAllMedia` (Ctrl/Cmd+A): every source row — playlists and dual views skipped.
    pub fn select_all_media(&mut self) -> bool {
        let ns: Vec<String> = self.groups.iter().filter(|g| !matches!(g.kind, MediaType::Playlist | MediaType::DualPair)).flat_map(|g| g.rows.iter().map(|r| r.id.clone())).collect();
        self.last_selected = ns.last().cloned(); let changed = ns != self.selected; self.selected = ns; changed
    }
    /// QCView `playlistablePathsFromSelection`: Videos, Images, Image Sequences in bin order (Audio is refused by createPlaylist).
    fn playlistable_selection(&self) -> Vec<String> {
        self.groups.iter().flat_map(|g| g.rows.iter()).filter(|r| matches!(r.media_type, MediaType::Video | MediaType::Image | MediaType::ImageSequence) && self.selected.contains(&r.id)).map(|r| r.id.clone()).collect()
    }
    pub fn layout(&mut self, fonts: &mut Fonts, x0: f64, x1: f64, y: f64, scale: f64) -> f64 {
        let hh = Section::height(scale);
        self.section.rect = Rect::new(x0, y, x1, y + hh);
        if !self.section.expanded { self.height = hh; return hh }
        let pad = t::SPACING_100 as f64 * scale; let gap = t::RAIL_ROW_GAP as f64 * scale; let row_h = t::COMPONENT_HEIGHT_75 as f64 * scale;
        let mut cy = y + hh;
        // the header line: the project's name (subdued) with the Add Media pill at the right
        let (bw, bh) = self.add.measure(fonts, scale);
        self.add.rect = Rect::new(x1 - pad - bw, cy, x1 - pad, cy + bh); cy += bh + pad;
        if self.groups.is_empty() { cy += row_h + pad; }   // the drop hint
        for (gi, g) in self.groups.iter_mut().enumerate() {
            if gi > 0 { cy += t::RAIL_GROUP_GAP as f64 * scale - pad / 2.0; }   // sub-headers part by space
            let gh = (fonts.detail(g.label, t::RAIL_GROUP_HEADER_SIZE * scale as f32, peniko::Color::WHITE).height() as f64).max(t::ICON_SIZE_SMALL as f64 * scale);
            let gh = if g.kind == MediaType::Playlist { gh.max(t::COMPONENT_HEIGHT_75 as f64 * scale) } else { gh };
            g.header = Rect::new(x0 + pad, cy, x1 - pad, cy + gh); cy += gh + gap * 2.0;
            if g.kind == MediaType::Playlist { let b = t::COMPONENT_HEIGHT_75 as f64 * scale; self.new_playlist.rect = Rect::new(g.header.x1 - b, g.header.y0 + (gh - b) / 2.0, g.header.x1, g.header.y0 + (gh + b) / 2.0); }
            for r in &mut g.rows {
                r.rect = Rect::new(x0 + pad / 2.0, cy, x1 - pad / 2.0, cy + row_h); cy += row_h + gap;
                // the loaded playlist opens into its clips, indented one step
                if let Some(tr) = self.tree.as_mut().filter(|t| t.model.playlist == r.id) {
                    let indent = t::SPACING_300 as f64 * scale;
                    tr.rows = tr.model.clips.iter().map(|_| { let rr = Rect::new(x0 + pad / 2.0 + indent, cy, x1 - pad / 2.0, cy + row_h); cy += row_h + gap; rr }).collect();
                }
            }
            cy += pad / 2.0;
        }
        cy += t::RAIL_SECTION_GAP as f64 * scale - pad / 2.0;
        if let Some((id, f)) = self.editing.as_mut() { if let Some(r) = self.groups.iter().flat_map(|g| g.rows.iter()).find(|r| r.id == *id) { f.layout(fonts, r.rect.inset((-pad / 4.0, -2.0 * scale)), scale); } }
        self.height = cy - y; self.height
    }
    /// QCView's Shift range: within the list the anchor lives in (the media bin vs the Playlists section); across them the
    /// ids don't contain both ends and the click falls back to a single replace.
    fn range_ids(&self, anchor: &str, target: &str) -> Vec<String> {
        let is_pl = |id: &str| self.groups.iter().any(|g| g.kind == MediaType::Playlist && g.rows.iter().any(|r| r.id == id));
        if is_pl(anchor) != is_pl(target) { return vec![] }
        let want = is_pl(anchor);
        self.groups.iter().filter(|g| (g.kind == MediaType::Playlist) == want).flat_map(|g| g.rows.iter().map(|r| r.id.clone())).collect()
    }
    /// Returns (commands, redraw). `ctrl` / `shift` are the shell's modifiers.
    pub fn event(&mut self, e: &Event, now: Instant, ctrl: bool, shift: bool, fonts: &mut Fonts, bounds: Rect, scale: f64) -> (Vec<Cmd>, bool) {
        let mut cmds = vec![];
        // the open menu takes everything (QCView's popup is modal to its window)
        if self.menu.is_open() {
            let (chosen, d, _) = self.menu.event(e);
            if let Some(i) = chosen { match i { 0 => { let ids = self.playlistable_selection(); if !ids.is_empty() { cmds.push(Cmd::CreatePlaylist(ids)); } } _ => if let Some(c) = self.remove_selection() { cmds.push(c); } } }
            return (cmds, d || chosen.is_some())
        }
        // the row drag: past 6 px (QCView's drag.threshold) the press becomes a drag; the release hands it to the shell
        if let Some((ids, label, _)) = self.drag.as_ref() {
            match *e {
                Event::Move { x, y } => { let (ids, label) = (ids.clone(), label.clone()); self.drag = Some((ids, label, (x, y))); return (cmds, true) }
                Event::Up { x, y, button: min_ui::Button::Primary } => { cmds.push(Cmd::DropRows(ids.clone(), x, y)); self.drag = None; self.press = None; return (cmds, true) }
                Event::Leave => { return (cmds, false) }
                _ => return (cmds, false),
            }
        }
        if let (Some((id, px, py)), Event::Move { x, y }) = (self.press.clone(), *e) {
            if ((x - px).powi(2) + (y - py).powi(2)).sqrt() > 6.0 * scale {
                let ids = self.drag_payload(&id);
                if !ids.is_empty() {
                    let first = self.groups.iter().flat_map(|g| g.rows.iter()).find(|r| r.id == ids[0]).map(|r| r.name.clone()).unwrap_or_default();
                    let label = if ids.len() > 1 { format!("{first}  +{}", ids.len() - 1) } else { first };
                    self.drag = Some((ids, label, (x, y))); self.last_click = None;
                    return (cmds, true)
                }
                self.press = None;
            }
        }
        if matches!(e, Event::Up { .. }) { self.press = None; }
        // a press anywhere but the rename field commits it and goes on to its target (QCView's renameClickAway)
        if let Event::Down { x, y, .. } = *e {
            if self.editing.as_ref().is_some_and(|(_, f)| !f.hit(x, y)) { if let Some(c) = self.commit_rename() { cmds.push(c); } }
        }
        if let Some((_, f)) = self.editing.as_mut() { if e.pos().is_some_and(|(x, y)| f.hit(x, y)) || matches!(e, Event::Up { .. }) { let d = f.pointer(e, fonts); return (cmds, d) } }
        let (toggled, mut dirty) = self.section.event(e, now); if toggled { dirty = true; }
        if !self.section.expanded { return (cmds, dirty) }
        if self.new_playlist.event(e, now) == min_ui::Response::Clicked { cmds.push(Cmd::NewPlaylist); dirty = true; }
        // right-click a row: outside the selection replaces it, inside keeps the group; then the menu at the pointer
        if let Event::Down { x, y, button: min_ui::Button::Secondary } = *e {
            if let Some(id) = self.groups.iter().flat_map(|g| g.rows.iter()).find(|r| r.rect.contains(kurbo::Point::new(x, y))).map(|r| r.id.clone()) {
                if !self.selected.contains(&id) { self.selected = vec![id.clone()]; self.last_selected = Some(id); }
                let can_playlist = !self.playlistable_selection().is_empty();
                self.menu.open_at(vec![MenuItem::action("Create Playlist from Selection", can_playlist), MenuItem::Separator, MenuItem::with_shortcut("Delete", !self.selected.is_empty(), "Del")], fonts, x, y, bounds, scale);
                return (cmds, true)
            }
        }
        let (clicked, d) = self.add.event(e, now); dirty |= d; if clicked { cmds.push(Cmd::AddMedia); }
        for g in &mut self.groups { for r in &mut g.rows {
            let inside = e.pos().is_some_and(|(x, y)| r.rect.contains(kurbo::Point::new(x, y))) && !matches!(e, Event::Leave);
            if inside != r.hovered { r.hovered = inside; r.hover.set_target(if inside { 1.0 } else { 0.0 }, now); dirty = true; }
        } }
        // QCView's badge tooltip
        { let over = e.pos().filter(|_| !matches!(e, Event::Leave)).and_then(|(x, y)| self.badge_rects.iter().find(|(_, r)| r.contains(kurbo::Point::new(x, y)))).map(|(_, r)| (7u64, *r, "Can’t be played here: unsupported format or un-debayered camera raw"));
          if self.tooltips.hover(over, now) { dirty = true; } if matches!(e, Event::Down { .. }) { self.tooltips.pressed(now); } }
        if let Some(tr) = self.tree.as_mut() {
            // a tree drag owns the pointer until the release
            if let Some((i, _)) = tr.drag {
                match *e {
                    Event::Move { x, y } => { tr.drag = Some((i, (x, y))); return (cmds, true) }
                    Event::Up { y, .. } => { let to = Self::tree_slot(tr, y); if to != i && to != i + 1 { cmds.push(Cmd::MoveClip { clip: tr.model.clips[i].0.clone(), to }); } tr.drag = None; tr.press = None; return (cmds, true) }
                    _ => return (cmds, false),
                }
            }
            if let Some((i, px, py)) = tr.press {
                match *e {
                    Event::Move { x, y } if ((x - px).powi(2) + (y - py).powi(2)).sqrt() > 6.0 * scale => { tr.drag = Some((i, (x, y))); return (cmds, true) }
                    Event::Up { .. } => { tr.press = None; cmds.push(Cmd::SeekClip(i)); return (cmds, true) }   // a click: seek on release
                    _ => {}
                }
            }
            let at = |x: f64, y: f64| tr.rows.iter().position(|r| r.contains(kurbo::Point::new(x, y)));
            let h = match *e { Event::Leave => None, _ => e.pos().and_then(|(x, y)| at(x, y)) };
            if h != tr.hover { tr.hover = h; dirty = true; }
            if let Event::Down { x, y, button: min_ui::Button::Primary } = *e && let Some(i) = at(x, y) { tr.press = Some((i, x, y)); return (cmds, true) }
        }
        if let Event::Down { x, y, button: min_ui::Button::Primary } = *e {
            let hit = self.groups.iter().flat_map(|g| g.rows.iter()).find(|r| r.rect.contains(kurbo::Point::new(x, y))).map(|r| r.id.clone());
            if let Some(id) = hit {
                self.press = Some((id.clone(), x, y));
                // QCView: a second click on the same row inside the double-click window loads it
                if self.last_click.as_ref().is_some_and(|(lid, t0)| *lid == id && now.duration_since(*t0).as_millis() <= DOUBLE_CLICK_MS) { cmds.push(Cmd::Load(id.clone())); self.last_click = None; }
                else {
                    self.last_click = Some((id.clone(), now));
                    if ctrl { if let Some(i) = self.selected.iter().position(|s| *s == id) { self.selected.remove(i); } else { self.selected.push(id.clone()); } }
                    else if shift { if let Some(anchor) = self.last_selected.clone() { let ids = self.range_ids(&anchor, &id); if let (Some(a), Some(b)) = (ids.iter().position(|s| *s == anchor), ids.iter().position(|s| *s == id)) { let (lo, hi) = (a.min(b), a.max(b)); self.selected = ids[lo..=hi].to_vec(); } else { self.selected = vec![id.clone()]; } } else { self.selected = vec![id.clone()]; } }
                    else { self.selected = vec![id.clone()]; }
                    self.last_selected = Some(id);
                }
                dirty = true;
            }
        }
        (cmds, dirty)
    }
    /// Delete / Backspace with a selection → remove them (QCView's Delete menu item).
    pub fn remove_selection(&mut self) -> Option<Cmd> { if self.selected.is_empty() { None } else { Some(Cmd::Remove(std::mem::take(&mut self.selected))) } }
    /// The badge tooltip's delay / timeout clock (true = redraw).
    pub fn tick(&mut self, now: Instant) -> bool { self.tooltips.tick(now) }
    pub fn animating(&self, now: Instant) -> bool { self.section.animating(now) || self.add.animating(now) || self.new_playlist.animating(now) || self.groups.iter().any(|g| g.rows.iter().any(|r| r.hover.animating(now))) }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, now: Instant, first: bool) {
        let gap = t::RAIL_SECTION_GAP as f64 * scale;
        min_ui::section::paint_tile(scene, Rect::new(self.section.rect.x0, self.section.rect.y0, self.section.rect.x1, self.section.rect.y0 + self.height - gap / 2.0), scale);
        self.section.paint(scene, fonts, scale, now, first);
        if !self.section.expanded { return }
        let pad = t::SPACING_100 as f64 * scale;
        // the project's name beside the pill (a dot when unsaved)
        let title = format!("{}{}", self.title, if self.dirty { " •" } else { "" });
        let l = fonts.layout(&title, Family::Sans, t::RAIL_LABEL_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_VALUE_COLOR));
        min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((self.section.rect.x0 + pad, self.add.rect.y0 + (self.add.rect.height() - l.height() as f64) / 2.0)));
        self.add.paint(scene, fonts, scale, now);
        if self.groups.is_empty() {
            let l = fonts.layout("Drop media here to add to the project", Family::Sans, t::RAIL_LABEL_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::DISABLED_CONTENT_COLOR));
            min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((self.section.rect.x0 + pad, self.add.rect.y1 + pad + (t::COMPONENT_HEIGHT_75 as f64 * scale - l.height() as f64) / 2.0)));
        }
        let mut badges: Vec<(String, Rect)> = vec![];
        for g in &self.groups {
            // the bin header: the type glyph once, then "Label (n)" — the rows carry no icon
            let mut hx = g.header.x0;
            if let Some(ic) = fonts.icon(g.icon, t::ICON_SIZE_SMALL * scale as f32, paint::color(t::RAIL_GROUP_HEADER_COLOR)) { min_ui::fonts::draw_layout(scene, &ic, kurbo::Affine::translate((hx, g.header.y0 + (g.header.height() - ic.height() as f64) / 2.0))); hx += t::ICON_SIZE_SMALL as f64 * scale + t::SPACING_75 as f64 * scale; }
            let h = fonts.detail(&format!("{} ({})", g.label, g.rows.len()), t::RAIL_GROUP_HEADER_SIZE * scale as f32, paint::color(t::RAIL_GROUP_HEADER_COLOR));
            min_ui::fonts::draw_layout(scene, &h, kurbo::Affine::translate((hx, g.header.y0 + (g.header.height() - h.height() as f64) / 2.0)));
            if g.kind == MediaType::Playlist { self.new_playlist.paint(scene, fonts, scale, now); }
            for r in &g.rows {
                if self.editing.as_ref().is_some_and(|(id, _)| *id == r.id) { continue }   // the field paints over this row
                let selected = self.selected.contains(&r.id); let active = r.id == self.active;
                let radius = t::CORNER_RADIUS_SMALL_DEFAULT as f64 * scale;
                let bg = if active { t::RAIL_ROW_ACTIVE_FILL } else if selected { t::NEUTRAL_SELECTED_BACKGROUND_COLOR_DEFAULT } else { mix(t::HOVER_BACKGROUND_COLOR_IDLE, t::HOVER_BACKGROUND_COLOR, r.hover.value(now)) };
                if bg.a > 0.0 { paint::rounded(scene, r.rect, radius, bg); }
                let c = if r.failed { t::DISABLED_CONTENT_COLOR } else if active { t::RAIL_ROW_ACTIVE_LABEL } else { t::RAIL_LABEL_COLOR };
                let x = r.rect.x0 + pad;
                let mut right = r.rect.x1 - pad;
                if r.unsupported { if let Some(g) = fonts.icon("warning", t::ICON_SIZE_SMALL * scale as f32, paint::color(t::NOTICE_VISUAL_COLOR)) { let gx = right - g.width() as f64; let gy = r.rect.y0 + (r.rect.height() - g.height() as f64) / 2.0; min_ui::fonts::draw_layout(scene, &g, kurbo::Affine::translate((gx, gy))); badges.push((r.id.clone(), Rect::new(gx, gy, right, gy + g.height() as f64))); right = gx - t::SPACING_75 as f64 * scale; } }
                // the loaded playlist's row: its clip count + canvas, subdued, at the right
                let tree = self.tree.as_ref().filter(|tr| tr.model.playlist == r.id);
                if let Some(tr) = tree {
                    let il = fonts.layout(&tr.model.info, Family::Sans, t::RAIL_GROUP_HEADER_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(if active { t::RAIL_ROW_ACTIVE_LABEL } else { t::RAIL_GROUP_HEADER_COLOR }));
                    let ix = right - il.width() as f64; min_ui::fonts::draw_layout(scene, &il, kurbo::Affine::translate((ix, r.rect.y0 + (r.rect.height() - il.height() as f64) / 2.0))); right = ix - t::SPACING_200 as f64 * scale;
                }
                let weight = if active { t::FONT_WEIGHT_BOLD } else { t::FONT_WEIGHT_REGULAR } as f32;   // Spectrum 2 marks the current item with fill + weight, never an outline (a ring = keyboard focus)
                let l = elided(fonts, &r.name, weight, c, (right - x).max(20.0), scale);
                min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((x, r.rect.y0 + (r.rect.height() - l.height() as f64) / 2.0)));
                // the clips: glyph + name, the playing one in the quiet selected grey (blue stays on the loaded row, canon row 162)
                if let Some(tr) = tree {
                    for (i, ((_, name, kind), rr)) in tr.model.clips.iter().zip(&tr.rows).enumerate() {
                        let cur = tr.model.current == Some(i);
                        let bg = if cur { t::NEUTRAL_SELECTED_BACKGROUND_COLOR_DEFAULT } else if tr.hover == Some(i) { t::HOVER_BACKGROUND_COLOR } else { t::HOVER_BACKGROUND_COLOR_IDLE };
                        if bg.a > 0.0 { paint::rounded(scene, *rr, radius, bg); }
                        let mut cx = rr.x0 + pad;
                        if let Some(ic) = fonts.icon(type_icon(*kind), t::ICON_SIZE_SMALL * scale as f32, paint::color(t::RAIL_GROUP_HEADER_COLOR)) { min_ui::fonts::draw_layout(scene, &ic, kurbo::Affine::translate((cx, rr.y0 + (rr.height() - ic.height() as f64) / 2.0))); cx += t::ICON_SIZE_SMALL as f64 * scale + t::SPACING_75 as f64 * scale; }
                        let cl = elided(fonts, name, if cur { t::FONT_WEIGHT_BOLD as f32 } else { t::FONT_WEIGHT_REGULAR as f32 }, if cur { t::GRAY_900 } else { t::RAIL_LABEL_COLOR }, (rr.x1 - pad - cx).max(20.0), scale);
                        min_ui::fonts::draw_layout(scene, &cl, kurbo::Affine::translate((cx, rr.y0 + (rr.height() - cl.height() as f64) / 2.0)));
                    }
                }
            }
        }
            if let Some((_, f)) = self.editing.as_mut() { f.paint(scene, fonts, scale); }
        self.badge_rects = badges;
    }
    /// The row menu, drawn after every panel so it sits on top.
    pub fn paint_menu(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, bounds: Rect, now: Instant) {
        self.tooltips.paint(scene, fonts, scale, bounds, now);
        self.menu.paint(scene, fonts, scale);
        // the drag ghost: the row's chip under the pointer (QCView's `Drag.Automatic` image)
        // the tree drag: the insertion line between the rows (the clip bar's insert colour) + the row's chip under the pointer
        let tree_ghost = self.tree.as_ref().and_then(|tr| tr.drag.map(|(i, p)| { let to = Self::tree_slot(tr, p.1); (tr.model.clips[i].1.clone(), p, to, tr.rows.clone()) }));
        if let Some((_, _, to, rows)) = &tree_ghost && let (Some(first), Some(last)) = (rows.first(), rows.last()) {
            let gap = t::RAIL_ROW_GAP as f64 * scale;
            let y = if *to < rows.len() { rows[*to].y0 - gap / 2.0 } else { last.y1 + gap / 2.0 };
            paint::rect(scene, Rect::new(first.x0, y - scale, first.x1, y + scale), t::PLAYLIST_BAR_INSERT_COLOR);
        }
        let ghost = self.drag.as_ref().map(|(_, l, p)| (l.clone(), *p)).or_else(|| tree_ghost.map(|(l, p, _, _)| (l, p)));
        if let Some((label, (x, y))) = &ghost {
            let l = fonts.layout(label, Family::Sans, t::RAIL_LABEL_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_ROW_ACTIVE_LABEL));
            let pad = t::SPACING_100 as f64 * scale; let h = t::COMPONENT_HEIGHT_75 as f64 * scale;
            let r = Rect::new(x + 12.0 * scale, y + 8.0 * scale, x + 12.0 * scale + l.width() as f64 + 2.0 * pad, y + 8.0 * scale + h);
            paint::rounded(scene, r, t::CORNER_RADIUS_SMALL_DEFAULT as f64 * scale, t::RAIL_ROW_ACTIVE_FILL);
            min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((r.x0 + pad, r.y0 + (h - l.height() as f64) / 2.0)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use min_ui::Button;
    fn panel() -> (ProjectPanel, ProjectManager, Fonts) {
        let mut pm = ProjectManager::new();
        // real (empty) files: the manager admits by extension and existence
        let dir = std::env::temp_dir().join(format!("qcv-pp-{}-{:?}", std::process::id(), std::thread::current().id())); std::fs::create_dir_all(&dir).unwrap();
        for n in ["a.mov", "b.mov", "c.wav", "d.mov"] { let p = dir.join(n); std::fs::write(&p, b"").unwrap(); pm.add_media_file(&p.to_string_lossy()); }
        pm.create_playlist(&[], "Playlist_01", 24.0, 1920, 1080, 0, 0);
        let mut pp = ProjectPanel::new(); let mut fonts = Fonts::new();
        pp.sync(&pm, ""); pp.layout(&mut fonts, 0.0, 280.0, 0.0, 1.0);
        (pp, pm, fonts)
    }
    fn row(pp: &ProjectPanel, name: &str) -> (String, f64, f64) { let r = pp.groups.iter().flat_map(|g| g.rows.iter()).find(|r| r.name.starts_with(name)).unwrap(); (r.id.clone(), r.rect.center().x, r.rect.center().y) }
    const B: Rect = Rect::new(0.0, 0.0, 1600.0, 900.0);

    #[test]
    fn right_click_outside_replaces_inside_keeps_and_the_menu_gates_create_playlist() {
        let (mut pp, _pm, mut f) = panel(); let now = Instant::now();
        let (a, ax, ay) = row(&pp, "a"); let (b, bx, by) = row(&pp, "b"); let (c, cx, cy) = row(&pp, "c");
        pp.event(&Event::Down { x: ax, y: ay, button: Button::Primary }, now, false, false, &mut f, B, 1.0);
        pp.event(&Event::Down { x: bx, y: by, button: Button::Primary }, now, true, false, &mut f, B, 1.0);
        assert_eq!(pp.selected, vec![a.clone(), b.clone()]);
        pp.event(&Event::Down { x: bx, y: by, button: Button::Secondary }, now, false, false, &mut f, B, 1.0);
        assert!(pp.menu_open() && pp.selected.len() == 2, "inside the selection keeps the group");
        assert!(matches!(&pp.menu.items[0], MenuItem::Action { enabled: true, .. }));
        pp.menu.close();
        pp.event(&Event::Down { x: cx, y: cy, button: Button::Secondary }, now, false, false, &mut f, B, 1.0);
        assert_eq!(pp.selected, vec![c], "outside replaces");
        assert!(matches!(&pp.menu.items[0], MenuItem::Action { enabled: false, .. }), "audio alone cannot make a playlist");
    }
    #[test]
    fn the_drag_carries_the_selection_in_bin_order_and_never_a_playlist() {
        let (mut pp, _pm, _f) = panel();
        let (a, ..) = row(&pp, "a"); let (b, ..) = row(&pp, "b"); let (d, ..) = row(&pp, "d"); let (pl, ..) = row(&pp, "Playlist");
        pp.selected = vec![d.clone(), a.clone(), pl.clone()];
        assert_eq!(pp.drag_payload(&d), vec![a.clone(), d.clone()], "bin order, the playlist left out");
        assert_eq!(pp.drag_payload(&b), vec![b], "a row outside the selection travels alone");
        assert!(pp.drag_payload(&pl).is_empty(), "a playlist is never dragged");
    }
    #[test]
    fn a_press_past_the_threshold_drags_and_the_release_reports_where() {
        let (mut pp, _pm, mut f) = panel(); let now = Instant::now();
        let (a, ax, ay) = row(&pp, "a");
        pp.event(&Event::Down { x: ax, y: ay, button: Button::Primary }, now, false, false, &mut f, B, 1.0);
        pp.event(&Event::Move { x: ax + 3.0, y: ay }, now, false, false, &mut f, B, 1.0);
        assert!(!pp.is_dragging(), "under 6 px stays a click");
        pp.event(&Event::Move { x: ax + 40.0, y: ay }, now, false, false, &mut f, B, 1.0);
        assert!(pp.is_dragging());
        let (c, _) = pp.event(&Event::Up { x: 900.0, y: 300.0, button: Button::Primary }, now, false, false, &mut f, B, 1.0);
        assert_eq!(c, vec![Cmd::DropRows(vec![a], 900.0, 300.0)]); assert!(!pp.is_dragging());
    }
    #[test]
    fn shift_range_stays_in_its_list_and_select_all_skips_playlists() {
        let (mut pp, _pm, mut f) = panel(); let now = Instant::now();
        let (a, ax, ay) = row(&pp, "a"); let (_, dx, dy) = row(&pp, "d"); let (pl, px, py) = row(&pp, "Playlist");
        pp.event(&Event::Down { x: ax, y: ay, button: Button::Primary }, now, false, false, &mut f, B, 1.0);
        pp.event(&Event::Up { x: ax, y: ay, button: Button::Primary }, now, false, false, &mut f, B, 1.0);
        pp.event(&Event::Down { x: dx, y: dy, button: Button::Primary }, now, false, true, &mut f, B, 1.0);
        pp.event(&Event::Up { x: dx, y: dy, button: Button::Primary }, now, false, true, &mut f, B, 1.0);
        assert_eq!(pp.selected.len(), 3, "a..d in display order: the Videos a, b, d (Audio sits below)");
        pp.event(&Event::Down { x: px, y: py, button: Button::Primary }, now, false, true, &mut f, B, 1.0);
        assert_eq!(pp.selected, vec![pl], "across the media / playlist lists falls back to a replace");
        assert!(pp.select_all_media() && pp.selected.len() == 4 && !pp.selected.iter().any(|s| *s == row(&pp, "Playlist").0));
        let _ = a;
    }
    #[test]
    fn rename_commits_trimmed_and_refuses_empty_or_unchanged() {
        let (mut pp, _pm, mut f) = panel();
        let (a, ..) = row(&pp, "a");
        let name = |k: &str| KeyEvent { key: min_ui::Key::Char(k.into()), text: Some(k.into()), shift: false, cmd: false };
        assert!(pp.begin_rename(&a, &mut f));
        for ch in [" ", "x", " "] { pp.key(&name(ch), &mut f); }
        assert_eq!(pp.commit_rename(), Some(Cmd::Rename(a.clone(), "x".into())));
        assert!(pp.begin_rename(&a, &mut f)); assert_eq!(pp.commit_rename(), None, "unchanged commits nothing");
        assert!(pp.begin_rename(&a, &mut f)); pp.key(&KeyEvent { key: min_ui::Key::Named(min_ui::Named::Backspace), text: None, shift: false, cmd: false }, &mut f);
        assert_eq!(pp.commit_rename(), None, "empty is refused (QCView renameMediaItem)");
    }
}

/// A row label elided in the middle (QCView's `Text.ElideMiddle`: keep the head and the tail).
fn elided(fonts: &mut Fonts, name: &str, weight: f32, c: min_theme::Color, avail: f64, scale: f64) -> min_ui::fonts::TextLayout {
    let mut l = fonts.layout(name, Family::Sans, t::RAIL_LABEL_SIZE * scale as f32, weight, paint::color(c));
    if l.width() as f64 > avail {
        let chars: Vec<char> = name.chars().collect(); let mut keep = chars.len();
        while keep > 2 { keep -= 1; let head = keep / 2; let tail = keep - head; let s: String = format!("{}…{}", chars[..head].iter().collect::<String>(), chars[chars.len() - tail..].iter().collect::<String>()); l = fonts.layout(&s, Family::Sans, t::RAIL_LABEL_SIZE * scale as f32, weight, paint::color(c)); if l.width() as f64 <= avail { break } }
    }
    l
}
