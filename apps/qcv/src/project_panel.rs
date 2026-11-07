//! The left rail's Add Media section — QCView's `LeftRail.qml` media card carried: the project's name in the header, the
//! "Media" list grouped by QCView's section labels (Videos · Audio · Images · Image Sequences · Playlists · Dual Views ·
//! Live) with the type glyph + a count on the bin header (the user 2026-11-07: "move the icons from the media items to the
//! bins too, we don't need so many icons"), rows = the name elided in the middle; the LOADED row carries QCView's treatment
//! (LeftRail.qml's muted cobalt fill — one of the four blue uses, canon row 162 — with the name BOLD in gray-900, Spectrum 2's
//! current-item treatment: fill + weight, no outline, since a ring means keyboard focus there; QCView's 1-px border not carried), selected rows in the deliberate grey; single click selects (Ctrl / Cmd
//! toggles, Shift extends a range), double-click LOADS (QCView: "activation moved to double-click"), an empty list reads
//! "Drop media here to add to the project". An "Add Media…" pill opens the file dialog. Context menus and drag-to-playlist
//! wait for the menu node and DnD slice.
use kurbo::Rect;
use min_theme::tokens as t;
use min_ui::{anim::{mix, Ramp}, paint, Event, Family, Fonts, Section, TextButton, Variant};
use qcv_project::{MediaType, ProjectManager};
use std::time::Instant;
use vello::Scene;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cmd { Load(String), Remove(Vec<String>), AddMedia }

struct Row { id: String, name: String, rect: Rect, hovered: bool, hover: Ramp, failed: bool }
struct Group { label: &'static str, icon: &'static str, header: Rect, rows: Vec<Row> }

pub struct ProjectPanel {
    pub section: Section,
    add: TextButton,
    groups: Vec<Group>,
    pub selected: Vec<String>, last_selected: Option<String>,
    active: String, title: String, dirty: bool,
    revision_seen: u64, height: f64,
    last_click: Option<(String, Instant)>,
}

fn type_icon(t: MediaType) -> &'static str { match t { MediaType::Video => "video", MediaType::Audio => "music-notes", MediaType::Image => "image", MediaType::ImageSequence => "film-strip", MediaType::Playlist => "playlist", MediaType::DualPair => "frame-corners", MediaType::LiveStream => "broadcast" } }
/// QCView `sectionLabel`.
fn section_label(t: MediaType) -> &'static str { match t { MediaType::Video => "Videos", MediaType::Audio => "Audio", MediaType::Image => "Images", MediaType::ImageSequence => "Image Sequences", MediaType::Playlist => "Playlists", MediaType::DualPair => "Dual Views", MediaType::LiveStream => "Live" } }
const DOUBLE_CLICK_MS: u128 = 400;

impl ProjectPanel {
    pub fn new() -> ProjectPanel { ProjectPanel { section: Section::new("Add Media", true), add: TextButton::new("Add Media…", Variant::Secondary).size_s(), groups: vec![], selected: vec![], last_selected: None, active: String::new(), title: String::new(), dirty: false, revision_seen: u64::MAX, height: 0.0, last_click: None } }
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
            for bn in pm.bins() { for id in &bn.item_ids { if let Some(it) = pm.item(id) { if it.media_type == t { rows.push(Row { id: it.id.clone(), name: it.name.clone(), rect: Rect::ZERO, hovered: false, hover: Ramp::new(0.0, t::ANIMATION_DURATION_HOVER), failed: it.load_failed }); } } } }
            if !rows.is_empty() { groups.push(Group { label: section_label(t), icon: type_icon(t), header: Rect::ZERO, rows }); }
        }
        self.groups = groups;
        let ids: Vec<String> = self.groups.iter().flat_map(|g| g.rows.iter().map(|r| r.id.clone())).collect();
        self.selected.retain(|s| ids.contains(s));
        true
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
        for g in &mut self.groups {
            let gh = (fonts.detail(g.label, t::RAIL_GROUP_HEADER_SIZE * scale as f32, peniko::Color::WHITE).height() as f64).max(t::ICON_SIZE_SMALL as f64 * scale);
            g.header = Rect::new(x0 + pad, cy, x1 - pad, cy + gh); cy += gh + gap * 2.0;
            for r in &mut g.rows { r.rect = Rect::new(x0 + pad / 2.0, cy, x1 - pad / 2.0, cy + row_h); cy += row_h + gap; }
            cy += pad / 2.0;
        }
        cy += pad;
        self.height = cy - y; self.height
    }
    fn row_ids(&self) -> Vec<String> { self.groups.iter().flat_map(|g| g.rows.iter().map(|r| r.id.clone())).collect() }
    /// Returns (commands, redraw). `ctrl` / `shift` are the shell's modifiers.
    pub fn event(&mut self, e: &Event, now: Instant, ctrl: bool, shift: bool) -> (Vec<Cmd>, bool) {
        let (toggled, mut dirty) = self.section.event(e, now); if toggled { dirty = true; }
        let mut cmds = vec![];
        if !self.section.expanded { return (cmds, dirty) }
        let (clicked, d) = self.add.event(e, now); dirty |= d; if clicked { cmds.push(Cmd::AddMedia); }
        for g in &mut self.groups { for r in &mut g.rows {
            let inside = e.pos().is_some_and(|(x, y)| r.rect.contains(kurbo::Point::new(x, y))) && !matches!(e, Event::Leave);
            if inside != r.hovered { r.hovered = inside; r.hover.set_target(if inside { 1.0 } else { 0.0 }, now); dirty = true; }
        } }
        if let Event::Down { x, y, button: min_ui::Button::Primary } = *e {
            let hit = self.groups.iter().flat_map(|g| g.rows.iter()).find(|r| r.rect.contains(kurbo::Point::new(x, y))).map(|r| r.id.clone());
            if let Some(id) = hit {
                // QCView: a second click on the same row inside the double-click window loads it
                if self.last_click.as_ref().is_some_and(|(lid, t0)| *lid == id && now.duration_since(*t0).as_millis() <= DOUBLE_CLICK_MS) { cmds.push(Cmd::Load(id.clone())); self.last_click = None; }
                else {
                    self.last_click = Some((id.clone(), now));
                    if ctrl { if let Some(i) = self.selected.iter().position(|s| *s == id) { self.selected.remove(i); } else { self.selected.push(id.clone()); } }
                    else if shift { if let Some(anchor) = self.last_selected.clone() { let ids = self.row_ids(); if let (Some(a), Some(b)) = (ids.iter().position(|s| *s == anchor), ids.iter().position(|s| *s == id)) { let (lo, hi) = (a.min(b), a.max(b)); self.selected = ids[lo..=hi].to_vec(); } } else { self.selected = vec![id.clone()]; } }
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
    pub fn animating(&self, now: Instant) -> bool { self.section.animating(now) || self.add.animating(now) || self.groups.iter().any(|g| g.rows.iter().any(|r| r.hover.animating(now))) }
    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, now: Instant, first: bool) {
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
        for g in &self.groups {
            // the bin header: the type glyph once, then "Label (n)" — the rows carry no icon
            let mut hx = g.header.x0;
            if let Some(ic) = fonts.icon(g.icon, t::ICON_SIZE_SMALL * scale as f32, paint::color(t::RAIL_GROUP_HEADER_COLOR)) { min_ui::fonts::draw_layout(scene, &ic, kurbo::Affine::translate((hx, g.header.y0 + (g.header.height() - ic.height() as f64) / 2.0))); hx += t::ICON_SIZE_SMALL as f64 * scale + t::SPACING_75 as f64 * scale; }
            let h = fonts.detail(&format!("{} ({})", g.label, g.rows.len()), t::RAIL_GROUP_HEADER_SIZE * scale as f32, paint::color(t::RAIL_GROUP_HEADER_COLOR));
            min_ui::fonts::draw_layout(scene, &h, kurbo::Affine::translate((hx, g.header.y0 + (g.header.height() - h.height() as f64) / 2.0)));
            for r in &g.rows {
                let selected = self.selected.contains(&r.id); let active = r.id == self.active;
                let radius = t::CORNER_RADIUS_SMALL_DEFAULT as f64 * scale;
                let bg = if active { t::RAIL_ROW_ACTIVE_FILL } else if selected { t::NEUTRAL_SELECTED_BACKGROUND_COLOR_DEFAULT } else { mix(t::HOVER_BACKGROUND_COLOR_IDLE, t::HOVER_BACKGROUND_COLOR, r.hover.value(now)) };
                if bg.a > 0.0 { paint::rounded(scene, r.rect, radius, bg); }
                let c = if r.failed { t::DISABLED_CONTENT_COLOR } else if active { t::RAIL_ROW_ACTIVE_LABEL } else { t::RAIL_LABEL_COLOR };
                let x = r.rect.x0 + pad;
                let avail = (r.rect.x1 - pad - x).max(20.0);
                let weight = if active { t::FONT_WEIGHT_BOLD } else { t::FONT_WEIGHT_REGULAR } as f32;   // Spectrum 2 marks the current item with fill + weight, never an outline (a ring = keyboard focus)
                let mut l = fonts.layout(&r.name, Family::Sans, t::RAIL_LABEL_SIZE * scale as f32, weight, paint::color(c));
                if l.width() as f64 > avail {
                    // ElideMiddle: keep the head and the tail
                    let chars: Vec<char> = r.name.chars().collect(); let mut keep = chars.len();
                    while keep > 2 { keep -= 1; let head = keep / 2; let tail = keep - head; let s: String = format!("{}…{}", chars[..head].iter().collect::<String>(), chars[chars.len() - tail..].iter().collect::<String>()); l = fonts.layout(&s, Family::Sans, t::RAIL_LABEL_SIZE * scale as f32, weight, paint::color(c)); if l.width() as f64 <= avail { break } }
                }
                min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((x, r.rect.y0 + (r.rect.height() - l.height() as f64) / 2.0)));
            }
        }
    }
}
