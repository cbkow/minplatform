//! The item list of a job's section (canon rows 482, 489; UFB 1.4.2's `ItemListPanel` and `CellEditor`, read into
//! `docs/phase5/step0-projects-metadata.md`, sections 4.2 and 5.2): a row an item - its star, its name, one cell for
//! each metadata column in view, the date it was changed - under a header whose titles sort and whose dividers size.
//!
//! The values are 1.4.2's, to the byte (the database is the same thing): text and notes and links a string, a number a
//! number, a checkbox true or false, a dropdown or a priority the option's NAME, a date the milliseconds of that day's
//! LOCAL midnight; clearing writes `null` and keeps the key. What this file does is show them, sort by them, and take
//! the pointer; the window writes what was changed (`db.rs`) and reads the section again.
use crate::db::{kind_of, ColDef, ItemMeta};
use crate::format;
use crate::jobview::Item;
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use min_ui::{fonts::draw_layout, paint, Button, Event, Family, Fonts, ScrollBar, TextField};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;
use vello::Scene;

/// Logical px.
pub const HEAD: f64 = 26.0; pub const ROW: f64 = 28.0; const STAR: f64 = 30.0; const NAME_MIN: f64 = 120.0; const COL_MIN: f64 = 50.0; const MODIFIED: f64 = 118.0; const GRIP: f64 = 4.0; const PAD: f64 = 8.0;

/// What the list is sorted by: nothing (the items as the folder lists them), the name, the date, a column (its uuid).
#[derive(Clone, Debug, PartialEq)]
pub enum SortBy { Name, Modified, Col(String) }
/// What the pointer did, for the window to carry out.
#[derive(Clone, Debug, PartialEq)]
pub enum Act { /// an item was pressed (its place among the job's items); its star; a checkbox cell (the column's place among those in view)
    Choose(usize), Star(usize), Check(usize, usize), /// a text-like cell was pressed: its field is to open; a dropdown cell: its options, at this point
    Edit(usize, usize), Options(usize, usize, (f64, f64)), /// the secondary button on the header (on a column in view, or not), or on a row
    HeadMenu(Option<usize>, (f64, f64)), RowMenu(usize, (f64, f64)), /// a divider was let go: the column's new width
    Width(usize, f64), /// a link of a Links cell was pressed: the address, as it is handed to the system (or a `ufb:` / `union:` link)
    Link(String) }
/// A cell being typed into.
pub struct Edit { pub item: PathBuf, pub col: usize, pub field: TextField }

pub struct ItemList {
    /// the columns in view, those hidden by this user, what the items carry
    pub cols: Vec<ColDef>, pub hidden: Vec<ColDef>, pub meta: HashMap<PathBuf, ItemMeta>,
    /// rows whose folder is not there (the tracker, row 493): their name says so
    pub missing: std::collections::HashSet<PathBuf>,
    /// items chosen beside the one in front (their places among the items; row 530): shown chosen as it is
    pub also: std::collections::BTreeSet<usize>,
    pub sort: Option<(SortBy, bool)>, /// the items' places in the job's list, in the order shown
    pub order: Vec<usize>, pub show_modified: bool,
    pub rect: Rect, pub bar: ScrollBar, /// sideways, when the columns are wider than the list (Shift with the wheel, or the bar at its foot)
    pub hbar: ScrollBar, pub hover: Option<usize>, pub edit: Option<Edit>,
    /// where the columns lie (px): the name, each column in view, the date; the divider in hand (column, x at the press, width then)
    name_x: (f64, f64), xs: Vec<(f64, f64)>, mod_x: Option<(f64, f64)>, drag: Option<(usize, f64, f64)>, over_div: bool, scale: f64,
    /// the links of the Links cells as they were drawn (row 584): where each lies, what it opens; the pointer is on one
    links: Vec<(Rect, String)>, over_link: bool,
}

/// The milliseconds of a day's local midnight (1.4.2's date value).
pub fn midnight_ms(year: i32, month: u32, day: u32) -> i64 {
    let utc = format::days_from_civil(year, month, day) * 86400;
    // the zone's offset AT that day (looked up twice: the first guess may lie on the other side of a change of clocks)
    let offset = |at: i64| { let c = format::local(at); format::days_from_civil(c.year, c.month, c.day) * 86400 + c.hour as i64 * 3600 + c.minute as i64 * 60 - at.div_euclid(60) * 60 };
    let first = utc - offset(utc); (utc - offset(first)) * 1000
}
/// A date as it is typed: `2026-11-16` (also with `/` or `.`), `today`, `tomorrow`. None: not a date.
pub fn parse_date(text: &str) -> Option<i64> {
    let s = text.trim().to_lowercase(); let now = format::local(format::now_secs());
    if s == "today" { return Some(midnight_ms(now.year, now.month, now.day)) }
    if s == "tomorrow" { let c = format::civil_at((format::days_from_civil(now.year, now.month, now.day) + 1) * 86400, 0); return Some(midnight_ms(c.year, c.month, c.day)) }
    let parts: Vec<&str> = s.split(['-', '/', '.']).collect(); if parts.len() != 3 { return None }
    let (y, m, d) = (parts[0].parse::<i32>().ok()?, parts[1].parse::<u32>().ok()?, parts[2].parse::<u32>().ok()?);
    if !(1970..=9999).contains(&y) || !(1..=12).contains(&m) || d < 1 { return None }
    // (a day the month does not have is no date: the 31st of November comes back as the 1st of December)
    let c = format::civil_at(format::days_from_civil(y, m, d) * 86400, 0); (c.year == y && c.month == m && c.day == d).then(|| midnight_ms(y, m, d))
}
/// A date value as the list shows it (and as its field takes it): `2026-11-16`. A string is read as 1.4.2 reads one.
pub fn date_text(v: &Value) -> String {
    let ms = match v { Value::Number(n) => n.as_f64().map(|f| f as i64), Value::String(s) => parse_date(s), _ => None };
    match ms { Some(ms) => { let c = format::local(ms.div_euclid(1000) + 43_200); format!("{:04}-{:02}-{:02}", c.year, c.month, c.day) } None => String::new() }
}
/// A cell's value as text (a checkbox: "true" / "").
pub fn text_of(kind: &str, v: Option<&Value>) -> String {
    let Some(v) = v.filter(|v| !v.is_null()) else { return String::new() };
    match kind_of(kind) {
        "date" => date_text(v),
        "checkbox" => if is_checked(Some(v)) { "true".into() } else { String::new() },
        "number" => match v { Value::Number(n) => match n.as_f64() { Some(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", f as i64), Some(f) => format!("{f}"), None => n.to_string() }, Value::String(s) => s.clone(), _ => String::new() },
        _ => match v { Value::String(s) => s.clone(), Value::Number(n) => n.to_string(), Value::Bool(b) => b.to_string(), _ => String::new() },
    }
}
/// A checkbox cell is ticked (1.4.2 also reads `"true"` and `1`).
pub fn is_checked(v: Option<&Value>) -> bool { match v { Some(Value::Bool(b)) => *b, Some(Value::String(s)) => s == "true", Some(Value::Number(n)) => n.as_f64() == Some(1.0), _ => false } }
/// What a typed text becomes as a column's value. Empty clears (`null`). Err: not a value of that type (a date).
pub fn value_of(kind: &str, text: &str) -> Result<Value, String> {
    let s = text.trim(); if s.is_empty() { return Ok(Value::Null) }
    Ok(match kind_of(kind) {
        // (1.4.2: a number that cannot be read is stored as null)
        "number" => s.replace(',', ".").parse::<f64>().ok().and_then(serde_json::Number::from_f64).map(Value::Number).unwrap_or(Value::Null),
        "date" => Value::from(parse_date(s).ok_or_else(|| "A date is typed as 2026-11-16, today or tomorrow.".to_string())?),
        _ => Value::String(text.trim_end().to_string()),
    })
}
/// A colour written as `#rrggbb`.
pub fn hex(c: &str) -> Option<min_theme::Color> {
    let h = c.trim().strip_prefix('#')?; if h.len() != 6 { return None } let n = u32::from_str_radix(h, 16).ok()?;
    Some(min_theme::Color::rgba((n >> 16 & 255) as f32 / 255.0, (n >> 8 & 255) as f32 / 255.0, (n & 255) as f32 / 255.0, 1.0))
}
/// The colours options get when none is written (1.4.2's small palette, without its black and white).
pub const PALETTE: [&str; 8] = ["#4a90e2", "#7ed321", "#f5a623", "#d0021b", "#9013fe", "#50e3c2", "#bd10e0", "#9b9b9b"];
/// Options as they are typed: `WIP #f5a623, Review, Final` - a name, then perhaps its colour. One without a colour
/// gets the next of the palette.
pub fn parse_options(text: &str) -> Vec<(String, Option<String>)> {
    let mut out: Vec<(String, Option<String>)> = vec![];
    for part in text.split([',', '\n']) {
        let part = part.trim(); if part.is_empty() { continue }
        let (name, color) = match part.rsplit_once(char::is_whitespace) { Some((n, c)) if hex(c).is_some() => (n.trim(), Some(c.to_lowercase())), _ => (part, None) };
        if name.is_empty() || out.iter().any(|o| o.0.eq_ignore_ascii_case(name)) { continue }
        let color = color.or_else(|| Some(PALETTE[out.len() % PALETTE.len()].to_string())); out.push((name.to_string(), color));
    }
    out
}
#[cfg_attr(not(test), allow(dead_code))]
pub fn options_text(options: &[(String, Option<String>)]) -> String { options.iter().map(|(n, c)| match c { Some(c) => format!("{n} {c}"), None => n.clone() }).collect::<Vec<_>>().join(", ") }

/// The order two cells sort in: numbers by their size, everything else as text without regard to case; an empty
/// one is always last (1.4.2's rule - the caller keeps empties last in both directions).
fn compare(kind: &str, a: Option<&Value>, b: Option<&Value>) -> std::cmp::Ordering {
    let num = |v: Option<&Value>| match v { Some(Value::Number(n)) => n.as_f64(), _ => None };
    if matches!(kind_of(kind), "number" | "date") { if let (Some(x), Some(y)) = (num(a), num(b)) { return x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal) } }
    text_of(kind, a).to_lowercase().cmp(&text_of(kind, b).to_lowercase())
}

impl Default for ItemList { fn default() -> ItemList { ItemList { cols: vec![], hidden: vec![], meta: HashMap::new(), missing: Default::default(), also: Default::default(), sort: None, order: vec![], show_modified: true, rect: Rect::ZERO, bar: ScrollBar::default(), hbar: ScrollBar::horizontal(), hover: None, edit: None, name_x: (0.0, 0.0), xs: vec![], mod_x: None, drag: None, over_div: false, scale: 1.0, links: vec![], over_link: false } } }
impl ItemList {
    /// The columns and the values of a (job, section), as the database has them now.
    pub fn load(&mut self, job: &std::path::Path, folder: &str) {
        let all = crate::db::with(|d| d.columns(job, folder)).unwrap_or_default(); let (shown, hidden): (Vec<ColDef>, Vec<ColDef>) = all.into_iter().partition(|c| c.visible);
        self.cols = shown; self.hidden = hidden; self.meta = crate::db::with(|d| d.folder_meta(job, folder)).unwrap_or_default();
        if let Some((SortBy::Col(u), _)) = &self.sort { if !self.cols.iter().any(|c| &c.uuid == u) { self.sort = None; } }
    }
    pub fn clear(&mut self) { self.cols.clear(); self.hidden.clear(); self.meta.clear(); self.order.clear(); self.edit = None; self.sort = None; self.bar = ScrollBar::default(); self.hover = None; }
    pub fn value<'a>(&'a self, item: &Item, c: &ColDef) -> Option<&'a Value> { self.meta.get(&item.path).and_then(|m| m.values.get(&c.uuid)).filter(|v| !v.is_null()) }
    pub fn tracked(&self, item: &Item) -> bool { self.meta.get(&item.path).is_some_and(|m| m.tracked) }
    /// The order the items are shown in.
    pub fn reorder(&mut self, items: &[Item]) {
        let mut order: Vec<usize> = (0..items.len()).collect();
        if let Some((by, asc)) = self.sort.clone() {
            let col = match &by { SortBy::Col(u) => self.cols.iter().find(|c| &c.uuid == u).cloned(), _ => None };
            order.sort_by(|a, b| { let (x, y) = (&items[*a], &items[*b]);
                let (ea, eb) = match (&by, &col) { (SortBy::Col(_), Some(c)) => (self.value(x, c).is_none(), self.value(y, c).is_none()), _ => (false, false) };
                // empty cells last, whichever way round
                if ea != eb { return ea.cmp(&eb) }
                let o = match (&by, &col) { (SortBy::Name, _) => crate::model::natural_key(&x.name).cmp(&crate::model::natural_key(&y.name)), (SortBy::Modified, _) => x.modified.cmp(&y.modified),
                    (SortBy::Col(_), Some(c)) => compare(&c.kind, self.value(x, c), self.value(y, c)), _ => std::cmp::Ordering::Equal };
                if asc { o } else { o.reverse() } });
        }
        self.order = order;
    }
    /// A press on a title: ascending, then descending, then as the folder lists them (1.4.2's three steps).
    fn press_title(&mut self, by: SortBy) { self.sort = match &self.sort { Some((b, true)) if *b == by => Some((by, false)), Some((b, false)) if *b == by => None, _ => Some((by, true)) }; }

    fn head(&self) -> Rect { Rect::new(self.rect.x0, self.rect.y0, self.rect.x1, (self.rect.y0 + HEAD * self.scale).min(self.rect.y1)) }
    pub fn body(&self) -> Rect { Rect::new(self.rect.x0, self.head().y1, self.rect.x1, self.rect.y1) }
    /// Where everything lies: the star, the name (it takes what the others leave), the columns, the date.
    pub fn layout(&mut self, rect: Rect, rows: usize, s: f64) {
        self.rect = rect; self.scale = s;
        let widths: Vec<f64> = self.cols.iter().map(|c| c.width.max(COL_MIN) * s).collect(); let date = if self.show_modified { MODIFIED * s } else { 0.0 };
        let name = (rect.width() - STAR * s - widths.iter().sum::<f64>() - date).max(NAME_MIN * s);
        // (columns wider than the list: it scrolls sideways)
        self.hbar.set(rect, STAR * s + name + widths.iter().sum::<f64>() + date, s);
        let mut x = rect.x0 - self.hbar.offset + STAR * s; self.name_x = (x, x + name); x += name;
        self.xs = widths.iter().map(|w| { let r = (x, x + w); x += w; r }).collect();
        self.mod_x = self.show_modified.then_some((x, x + date));
        self.bar.set(self.body(), rows as f64 * ROW * s + 8.0 * s, s);
    }
    pub fn row_rect(&self, pos: usize) -> Rect { let b = self.body(); let y = b.y0 + 4.0 * self.scale + pos as f64 * ROW * self.scale - self.bar.offset; Rect::new(b.x0, y, b.x1, y + ROW * self.scale) }
    fn row_at(&self, x: f64, y: f64) -> Option<usize> { let b = self.body(); if !b.contains(kurbo::Point::new(x, y)) { return None } let k = ((y - b.y0 - 4.0 * self.scale + self.bar.offset) / (ROW * self.scale)).floor(); (k >= 0.0 && (k as usize) < self.order.len()).then_some(k as usize) }
    fn col_at(&self, x: f64) -> Option<usize> { self.xs.iter().position(|(a, b)| x >= *a && x < *b) }
    /// The place of a cell (window px), as its field lies over it.
    pub fn cell_rect(&self, pos: usize, col: usize) -> Rect { let r = self.row_rect(pos); let (a, b) = self.xs.get(col).copied().unwrap_or((r.x0, r.x1)); Rect::new(a + 2.0 * self.scale, r.y0 + 2.0 * self.scale, (b - 2.0 * self.scale).min(self.rect.x1), r.y1 - 2.0 * self.scale) }
    /// Scroll so that the item at `pos` (in the order shown) is in view.
    pub fn reveal(&mut self, pos: usize) { let (top, h, row) = (pos as f64 * ROW * self.scale, self.body().height() - 8.0 * self.scale, ROW * self.scale); if top < self.bar.offset { self.bar.offset = top; } else if top + row > self.bar.offset + h { self.bar.offset = (top + row - h).max(0.0); } }
    /// A divider is under the pointer or in hand (the window shows the resize cursor).
    pub fn resizing(&self) -> bool { self.over_div || self.drag.is_some() }
    /// The pointer is on a link of a Links cell (the window shows the hand).
    pub fn on_link(&self) -> bool { self.over_link }
    fn link_at(&self, x: f64, y: f64) -> Option<&str> { let b = self.body(); if !b.contains(kurbo::Point::new(x, y)) { return None } self.links.iter().find(|(r, _)| r.contains(kurbo::Point::new(x, y))).map(|(_, u)| u.as_str()) }
    pub fn animating(&self, now: Instant) -> bool { self.bar.animating(now) || self.hbar.animating(now) || self.edit.is_some() }

    /// A cell's field is opened over it, with what the cell holds, all of it chosen.
    pub fn begin_edit(&mut self, fonts: &mut Fonts, items: &[Item], item: usize, col: usize) {
        let (Some(it), Some(c)) = (items.get(item), self.cols.get(col)) else { return };
        let mut field = TextField::new(Family::Sans, t::BODY_SIZE_S); field.set_text_end(&text_of(&c.kind, self.value(it, c)), fonts); field.select_all(fonts); field.focused = true;
        if kind_of(&c.kind) == "date" { field.placeholder = "2026-11-16".into(); }
        self.edit = Some(Edit { item: it.path.clone(), col, field });
    }
    /// What the open field holds, as the column's value: (the item, the column's uuid, the value). Err: not a value
    /// of the column's type - the field stays open and is marked.
    pub fn typed(&mut self) -> Option<Result<(PathBuf, String, Value), String>> {
        let e = self.edit.as_mut()?; let c = self.cols.get(e.col)?;
        Some(match value_of(&c.kind, e.field.text()) { Ok(v) => Ok((e.item.clone(), c.uuid.clone(), v)), Err(why) => { e.field.error = true; Err(why) } })
    }

    /// The pointer. `items`: the job's items.
    pub fn event(&mut self, e: &Event, items: &[Item], now: Instant) -> Vec<Act> {
        let mut out = vec![]; let s = self.scale; self.bar.event(e, now); if self.hbar.event(e, now).1 { return out }
        let at = e.pos(); let head = self.head();
        // a divider in hand: the column's width follows the pointer
        if let Some((col, x0, w0)) = self.drag {
            match *e { Event::Move { x, .. } => if let Some(c) = self.cols.get_mut(col) { c.width = (w0 + (x - x0) / s).clamp(COL_MIN, 600.0); },
                Event::Up { button: Button::Primary, .. } | Event::Leave => { self.drag = None; if let Some(c) = self.cols.get(col) { out.push(Act::Width(col, c.width)); } }
                _ => {} }
            return out
        }
        self.over_div = at.is_some_and(|(x, y)| head.contains(kurbo::Point::new(x, y)) && self.xs.iter().any(|(_, b)| (x - b).abs() <= GRIP * s));
        self.hover = at.and_then(|(x, y)| self.row_at(x, y));
        self.over_link = at.is_some_and(|(x, y)| self.link_at(x, y).is_some());
        match *e {
            Event::Down { x, y, button } if head.contains(kurbo::Point::new(x, y)) => {
                let col = self.col_at(x);
                if button == Button::Secondary { out.push(Act::HeadMenu(col, (x, y))); }
                else if button == Button::Primary {
                    if let Some(k) = self.xs.iter().position(|(_, b)| (x - b).abs() <= GRIP * s) { self.drag = Some((k, x, self.cols[k].width.max(COL_MIN))); }
                    else if let Some(k) = col { let u = self.cols[k].uuid.clone(); self.press_title(SortBy::Col(u)); self.reorder(items); }
                    else if self.mod_x.is_some_and(|(a, b)| x >= a && x < b) { self.press_title(SortBy::Modified); self.reorder(items); }
                    else if x >= self.name_x.0 && x < self.name_x.1 { self.press_title(SortBy::Name); self.reorder(items); }
                }
            }
            Event::Down { x, y, button } => if let Some(pos) = self.row_at(x, y) { let i = self.order[pos];
                if button == Button::Secondary { out.push(Act::Choose(i)); out.push(Act::RowMenu(i, (x, y))); }
                else if button == Button::Primary {
                    out.push(Act::Choose(i));
                    if x < self.name_x.0 { out.push(Act::Star(i)); }
                    // a link of a Links cell opens (1.4.2: "click opens externally"); the rest of the cell is typed into
                    else if let Some(u) = self.link_at(x, y) { out.push(Act::Link(u.to_string())); }
                    else if let Some(k) = self.col_at(x) { match kind_of(&self.cols[k].kind) {
                        "checkbox" => out.push(Act::Check(i, k)),
                        // (a dropdown with no options is typed into, as 1.4.2's falls back to text)
                        "dropdown" | "priority" if !self.cols[k].options.is_empty() => { let r = self.cell_rect(pos, k); out.push(Act::Options(i, k, (r.x0, r.y1))); }
                        _ => out.push(Act::Edit(i, k)),
                    } }
                }
            },
            _ => {}
        }
        out
    }

    /// Draw the list. `chosen`: the chosen item's place among the job's; `focused`: the list holds the arrow keys.
    #[allow(clippy::too_many_arguments)]
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, items: &[Item], chosen: Option<usize>, focused: bool, title: &str, empty: &str, now: Instant) {
        let s = self.scale; let (ink, sub) = (paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT), paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT)); let small = t::BODY_SIZE_S * s as f32;
        let (head, body) = (self.head(), self.body()); let pad = PAD * s;
        // -- the header: "Items (n)" over the name, each column's name, "Modified"; the sort's mark; the dividers
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &head);
        let mark = |by: &SortBy| self.sort.as_ref().filter(|(b, _)| b == by).map(|(_, asc)| if *asc { "caret-up" } else { "caret-down" });
        let title_at = |scene: &mut Scene, fonts: &mut Fonts, text: &str, x: (f64, f64), mark: Option<&str>, first: bool| {
            let l = fonts.layout_fit(text, Family::Sans, small, t::FONT_WEIGHT_BOLD as f32, if mark.is_some() || first { ink } else { sub }, (x.1 - x.0 - pad * 2.0 - if mark.is_some() { 14.0 * s } else { 0.0 }).max(4.0));
            draw_layout(scene, &l, Affine::translate((x.0 + pad, head.y0 + (head.height() - l.height() as f64) / 2.0)));
            if let Some(g) = mark.and_then(|m| fonts.icon(m, 10.0 * s as f32, ink)) { draw_layout(scene, &g, Affine::translate((x.0 + pad + l.width() as f64 + 5.0 * s, head.y0 + (head.height() - g.height() as f64) / 2.0))); }
        };
        title_at(scene, fonts, title, (self.name_x.0 - STAR * s + 4.0 * s, self.name_x.1), mark(&SortBy::Name), true);
        for (k, c) in self.cols.iter().enumerate() { let x = self.xs[k]; paint::rect(scene, Rect::new(x.0, head.y0 + 6.0 * s, x.0 + s, head.y1 - 6.0 * s), t::DIVIDER_COLOR); title_at(scene, fonts, &c.name, x, mark(&SortBy::Col(c.uuid.clone())), false); }
        if let Some(x) = self.mod_x { paint::rect(scene, Rect::new(x.0, head.y0 + 6.0 * s, x.0 + s, head.y1 - 6.0 * s), t::DIVIDER_COLOR); title_at(scene, fonts, "Modified", x, mark(&SortBy::Modified), false); }
        scene.pop_layer();
        paint::hline(scene, head.x0, head.x1, head.y1 - s, s, t::DIVIDER_COLOR);
        // -- the rows
        let today = format::local(format::now_secs()); self.links.clear();
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &body);
        for (pos, i) in self.order.iter().enumerate() {
            let Some(it) = items.get(*i) else { continue }; let row = self.row_rect(pos); if row.y1 < body.y0 || row.y0 > body.y1 { continue }
            let r = Rect::new(row.x0 + 6.0 * s, row.y0, row.x1 - 6.0 * s, row.y1); let here = chosen == Some(*i) || self.also.contains(i); let strong = here && focused;
            if here { paint::rounded(scene, r, t::CORNER_RADIUS_SMALL_DEFAULT as f64 * s, if focused { t::RAIL_ROW_ACTIVE_FILL } else { t::HOVER_BACKGROUND_COLOR }); } else if self.hover == Some(pos) { paint::rounded(scene, r, t::CORNER_RADIUS_SMALL_DEFAULT as f64 * s, t::HOVER_BACKGROUND_COLOR); }
            let color = if strong { paint::color(t::RAIL_ROW_ACTIVE_LABEL) } else { ink }; let quiet = if strong { color } else { sub }; let mid = |h: f64| row.y0 + (row.height() - h) / 2.0;
            // the star: filled when the item is tracked
            let tracked = self.tracked(it);
// (the icon face has one star: a tracked item's is in the notice colour and always there, on a disc of it;
            // an untracked one's is quiet and shown only under the pointer or on the chosen row)
            let sx = self.name_x.0 - STAR * s + 3.0 * s;
            if tracked { let d = 20.0 * s; let c = t::NOTICE_VISUAL_COLOR; paint::rounded(scene, Rect::new(sx + (STAR * s - d) / 2.0, mid(d), sx + (STAR * s + d) / 2.0, mid(d) + d), d / 2.0, min_theme::Color::rgba(c.r, c.g, c.b, if strong { 0.95 } else { 0.28 })); }
            if tracked || here || self.hover == Some(pos) { if let Some(g) = fonts.icon("star", 14.0 * s as f32, if tracked && strong { paint::color(t::BACKGROUND_LAYER_1_COLOR) } else if tracked { paint::color(t::NOTICE_VISUAL_COLOR) } else { quiet }) {
                draw_layout(scene, &g, Affine::translate((sx + (STAR * s - g.width() as f64) / 2.0, mid(g.height() as f64)))); } }
            let lost = self.missing.contains(&it.path);
            let nl = fonts.layout_fit(&if lost { format!("{}  \u{2014}  not found", it.name) } else { it.name.clone() }, Family::Sans, small, t::FONT_WEIGHT_REGULAR as f32, if lost && !strong { paint::color(t::NEGATIVE_CONTENT_COLOR_DEFAULT) } else { color }, (self.name_x.1 - self.name_x.0 - pad).max(10.0));
            draw_layout(scene, &nl, Affine::translate((self.name_x.0 + 2.0 * s, mid(nl.height() as f64))));
            for (k, c) in self.cols.iter().enumerate() {
                let (a, b) = self.xs[k]; if a > row.x1 { break } let v = self.value(it, c); let room = (b - a - pad * 2.0).max(4.0);
                if self.edit.as_ref().is_some_and(|e| e.col == k && e.item == it.path) { continue }
                match kind_of(&c.kind) {
                    "checkbox" => { let z = min_ui::checkbox::size(s).min(row.height() - 8.0 * s); min_ui::checkbox::paint_box(scene, fonts, Rect::new(a + pad, mid(z), a + pad + z, mid(z) + z), Some(is_checked(v)), true, false, s); }
                    "dropdown" | "priority" => { let text = text_of(&c.kind, v); if !text.is_empty() {
                        // a chip in the option's colour; a value that matches no option is grey (1.4.2)
                        crate::chips::paint_chip(scene, fonts, &text, c.options.iter().find(|o| o.0 == text).and_then(|o| o.1.as_deref()), a + pad - 2.0 * s, row.y0 + row.height() / 2.0, room + 4.0 * s, s); } }

                    // links (row 584): as many as the cell holds, each in the accent colour and pressed by itself; the
                    // rest as a count. A text that is no address is drawn as plain text.
                    "links" => { let all = crate::weblink::of_cell(&text_of(&c.kind, v)); let (mut x, end) = (a + pad, a + pad + room); let gap = 10.0 * s;
                        for (n, (written, opens)) in all.iter().enumerate() {
                            let rest = all.len() - n; let more = if rest > 1 { fonts.layout_fit(&format!("+{}", rest - 1), Family::Sans, small, t::FONT_WEIGHT_REGULAR as f32, quiet, 60.0 * s).width() as f64 + gap } else { 0.0 };
                            let shown = written.strip_prefix("https://").or_else(|| written.strip_prefix("http://")).unwrap_or(written); let shown = shown.strip_prefix("www.").unwrap_or(shown).trim_end_matches('/');
                            let full = fonts.layout_fit(shown, Family::Sans, small, t::FONT_WEIGHT_REGULAR as f32, color, 4000.0).width() as f64;
                            // (the first is always drawn, cut short if it must be; a later one only whole)
                            if n > 0 && x + full + more > end { let l = fonts.layout_fit(&format!("+{rest}"), Family::Sans, small, t::FONT_WEIGHT_REGULAR as f32, quiet, (end - x).max(4.0)); draw_layout(scene, &l, Affine::translate((x, mid(l.height() as f64)))); break }
                            let ink = if opens.is_some() && !strong { paint::color(t::ACCENT_CONTENT_COLOR_DEFAULT) } else { color };
                            let l = fonts.layout_fit(shown, Family::Sans, small, t::FONT_WEIGHT_REGULAR as f32, ink, (end - x - more).max(4.0)); let (w, h) = (l.width() as f64, l.height() as f64);
                            draw_layout(scene, &l, Affine::translate((x, mid(h))));
                            if let Some(u) = opens { let r = Rect::new(x, mid(h), x + w, mid(h) + h); if self.over_link && self.hover == Some(pos) { paint::rect(scene, Rect::new(r.x0, r.y1 - s, r.x1, r.y1), if strong { t::RAIL_ROW_ACTIVE_LABEL } else { t::ACCENT_CONTENT_COLOR_DEFAULT }); } self.links.push((r, u.clone())); }
                            x += w + gap;
                        } }
                    kind => { let text = text_of(&c.kind, v); let shown = match kind { "notes" => text.lines().next().unwrap_or("").to_string(), _ => text };
                        if !shown.is_empty() { let l = fonts.layout_fit(&shown, Family::Sans, small, t::FONT_WEIGHT_REGULAR as f32, color, room); let x = if kind == "number" { b - pad - l.width() as f64 } else { a + pad }; draw_layout(scene, &l, Affine::translate((x, mid(l.height() as f64)))); } }
                }
            }
            if let Some((a, b)) = self.mod_x { let dl = fonts.layout_fit(&format::date_text(it.modified, today), Family::Sans, small, t::FONT_WEIGHT_REGULAR as f32, quiet, (b - a - pad * 2.0).max(4.0)); draw_layout(scene, &dl, Affine::translate((a + pad, mid(dl.height() as f64)))); }
        }
        if items.is_empty() && !empty.is_empty() { let l = fonts.layout_fit(empty, Family::Sans, small, t::FONT_WEIGHT_REGULAR as f32, sub, body.width() - 24.0 * s); draw_layout(scene, &l, Affine::translate((body.x0 + 14.0 * s, body.y0 + 8.0 * s))); }
        // the field of the cell being typed into lies over it (its row gone: the edit is over)
        let place = self.edit.as_ref().and_then(|e| self.order.iter().position(|i| items.get(*i).is_some_and(|it| it.path == e.item)).map(|pos| self.cell_rect(pos, e.col)));
        match (self.edit.as_mut(), place) { (Some(e), Some(r)) => { e.field.layout(fonts, r, s); e.field.paint(scene, fonts, s); } (Some(_), None) => self.edit = None, _ => {} }
        scene.pop_layer();
        self.bar.paint(scene, now); self.hbar.paint(scene, now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn item(name: &str, modified: i64) -> Item { let p = PathBuf::from("/j/3d").join(name); Item { name: name.into(), path: p.clone(), modified, project: p.clone(), renders: p, proxies: None } }
    fn col(name: &str, kind: &str) -> ColDef { ColDef { id: 1, name: name.into(), kind: kind.into(), order: 0, width: 120.0, visible: true, uuid: format!("u-{name}"), options: vec![] } }
    #[test]
    fn values_as_1_4_2_stores_them() {
        // a date: the milliseconds of that day's LOCAL midnight; shown and typed as 2026-11-16
        let ms = parse_date("2026-11-16").unwrap(); let c = format::local(ms / 1000); assert_eq!((c.year, c.month, c.day, c.hour, c.minute), (2026, 11, 16, 0, 0));
        assert_eq!(date_text(&json!(ms)), "2026-11-16"); assert_eq!(parse_date("2026/3/1"), parse_date("2026-03-01")); assert_eq!(date_text(&json!("2026-03-01")), "2026-03-01", "a string is read too");
        assert!(parse_date("2026-11-31").is_none()); assert!(parse_date("next week").is_none()); assert!(parse_date("today").is_some());
        assert_eq!(value_of("date", "  ").unwrap(), Value::Null, "empty clears"); assert!(value_of("date", "soon").is_err());
        assert_eq!(value_of("number", "12").unwrap(), json!(12.0)); assert_eq!(value_of("number", "1,5").unwrap(), json!(1.5)); assert_eq!(value_of("number", "abc").unwrap(), Value::Null, "an unreadable number is null (1.4.2)");
        assert_eq!(value_of("text", " a b ").unwrap(), json!(" a b")); assert_eq!(value_of("Note", "x").unwrap(), json!("x"));
        assert_eq!(text_of("number", Some(&json!(12.0))), "12"); assert_eq!(text_of("number", Some(&json!(1.5))), "1.5"); assert_eq!(text_of("text", Some(&Value::Null)), "");
        assert!(is_checked(Some(&json!(true)))); assert!(is_checked(Some(&json!("true")))); assert!(is_checked(Some(&json!(1)))); assert!(!is_checked(Some(&json!("yes")))); assert!(!is_checked(None));
    }
    #[test]
    fn options_are_typed_with_or_without_a_colour() {
        let o = parse_options("WIP #F5A623, Needs Review, final,, wip");
        assert_eq!(o, [("WIP".to_string(), Some("#f5a623".to_string())), ("Needs Review".to_string(), Some(PALETTE[1].to_string())), ("final".to_string(), Some(PALETTE[2].to_string()))], "a colour of its own, else the palette's next; no name twice");
        assert_eq!(parse_options(&options_text(&o)), o); assert!(hex("#12345").is_none()); assert_eq!(hex("#ff0000").map(|c| (c.r, c.g)), Some((1.0, 0.0)));
    }
    #[test]
    fn sorting_is_three_steps_and_keeps_empty_cells_last() {
        let items = [item("b", 30), item("a", 10), item("c", 20)]; let mut l = ItemList::default(); l.cols = vec![col("Frames", "number"), col("Artist", "text")];
        let mut put = |name: &str, c: &str, v: Value| { l.meta.entry(PathBuf::from("/j/3d").join(name)).or_default().values.insert(format!("u-{c}"), v); };
        put("b", "Frames", json!(100)); put("a", "Frames", json!(9)); put("b", "Artist", json!("zoe")); put("c", "Artist", json!("Adam")); put("a", "Artist", Value::Null);
        l.reorder(&items); assert_eq!(l.order, [0, 1, 2], "unsorted: as the folder lists them");
        l.press_title(SortBy::Name); l.reorder(&items); assert_eq!(l.order, [1, 0, 2]); l.press_title(SortBy::Name); l.reorder(&items); assert_eq!(l.order, [2, 0, 1]); l.press_title(SortBy::Name); l.reorder(&items); assert_eq!(l.order, [0, 1, 2], "the third press: off");
        l.press_title(SortBy::Col("u-Frames".into())); l.reorder(&items); assert_eq!(l.order, [1, 0, 2], "numbers by size (9 before 100), the empty one last");
        l.press_title(SortBy::Col("u-Frames".into())); l.reorder(&items); assert_eq!(l.order, [0, 1, 2], "descending: the empty one still last");
        l.press_title(SortBy::Col("u-Artist".into())); l.reorder(&items); assert_eq!(l.order, [2, 0, 1], "text without regard to case; null is empty");
        l.press_title(SortBy::Modified); l.reorder(&items); assert_eq!(l.order, [1, 2, 0]);
    }
}
