//! THE PDF STUDIO (row 362; spec `docs/phase3/step-pdf-studio.md`): ONE PDF block of a note, or PDF card of a canvas,
//! shown large - every page in a column that scrolls - over the window's page and sidebar. It is the same block: its
//! file, its drawings. Nothing new is stored, the tree does not change, and the note or canvas under it is not touched
//! (its scroll, its camera, its editor stay as they are).
//!
//! Built so far: entering and leaving, the column at the width of the view, the wheel, the keys that scroll, the
//! pages' own drawings shown (stage 1); DRAWING on the page under the pointer with the tools a page has in a note
//! (stage 3 - the gesture, its undo step and the tools' band are the note's own: `App::page_press` and its kin). Still
//! to come: the strip of small pages and zoom (2), selecting text (4), find (5), drawing on a canvas's card (6).
//!
//! The pages are made here, not by `media.rs`: a page is asked of `pdf::page` on a helper thread at the width it is
//! shown at, and only the pages near the view are kept (a page at this size is tens of megabytes; a long document
//! would otherwise hold gigabytes).
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Instant;
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use min_ui::{action_button::{ActionButton, Response}, fonts::draw_layout, paint::{color, image, image_rgba8}, Event, Family, Fonts};
use mn_doc::BlockId;

/// Whose PDF it is: a block of the open note, or a card of the open canvas.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Host { Block(BlockId), Card(BlockId) }

/// The gap between pages and the margin beside them, in logical points.
const GAP: f64 = 14.0;
const MARGIN: f64 = 40.0;
/// Pages kept beyond the ones in view, each way.
const AROUND: usize = 2;
/// The widest a page's picture is made (the engine's pictures and the renderer's atlas both have limits).
const WIDEST: u32 = 4096;

struct Made { page: usize, width: u32, px: Result<(Vec<u8>, u32, u32), String>, /// for the strip of small pages
    small: bool }

/// The strip of small pages at a PDF's left (row 367b): its width, a small page's width, the room under one for its number.
const STRIP_W: f64 = 148.0;
const SMALL_W: f64 = 112.0;
const SMALL_GAP: f64 = 26.0;

/// How large the pages are shown: to the view's width (a PDF's first look: no wider than reads well), the whole page
/// in the view (a picture's first look), or by a factor of the page's own units (1 = a PDF's point per logical point;
/// a picture's pixel per physical pixel).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Zoom { Width, Whole, By(f64) }

/// Find through a PDF's pages (row 367d): the words, where they were found (page, first and last character), the one
/// stood on, how far the reading of the pages has come.
pub struct Find { pub field: min_ui::TextField, query: String, pub hits: Vec<(usize, usize, usize)>, pub at: usize, read: usize, turn: std::sync::Arc<std::sync::atomic::AtomicU64>, rx: Option<Receiver<(u64, usize, Vec<(usize, usize)>)>> }

/// A page's characters as one run of lower-case letters without spaces, each letter's place in it mapped back to its
/// character: a PDF gives glyphs, with or without spaces between words, so the words asked for are matched the same way.
fn matches(text: &crate::pdf::PageText, needle: &str) -> Vec<(usize, usize)> {
    let needle: Vec<char> = needle.chars().filter(|c| !c.is_whitespace()).flat_map(|c| c.to_lowercase()).collect(); if needle.is_empty() { return vec![] }
    let (mut hay, mut map) = (Vec::new(), Vec::new());
    for (i, c) in text.chars.iter().enumerate() { for ch in c.text.chars().filter(|c| !c.is_whitespace()).flat_map(|c| c.to_lowercase()) { hay.push(ch); map.push(i); } }
    let mut out = vec![]; let mut i = 0;
    while i + needle.len() <= hay.len() { if hay[i..i + needle.len()] == needle[..] { out.push((map[i], map[i + needle.len() - 1])); i += needle.len(); } else { i += 1; } }
    out
}

pub struct Studio {
    pub host: Host, path: PathBuf, name: String,
    /// every page's size in points; one entry (the record's own shape) until the file has been read
    sizes: Vec<(f32, f32)>, sizes_rx: Option<Receiver<Result<Vec<(f32, f32)>, String>>>,
    pub scroll: f64, pub scroll_x: f64, /// the pages' place (the view less its strip)
    pub rect: Rect, /// the whole view: the strip of small pages, the pages, the head
    full: Rect, strip_scroll: f64, smalls: HashMap<usize, peniko::ImageBrush>, smalls_asked: std::collections::HashSet<usize>, strip_at: usize, scale: f64, pub zoom: Zoom, /// when the zoom last changed: the sharp pictures are asked for once it has rested
    zoomed: Instant, /// a picture, not a PDF: one page, whose picture the caller hands to `paint`
    pub picture: bool,
    pics: HashMap<usize, (u32, peniko::ImageBrush)>, asked: HashMap<usize, u32>, failed: HashMap<usize, String>,
    tx: Sender<Made>, rx: Receiver<Made>, wake: std::sync::Arc<dyn Fn() + Send + Sync>,
    /// pages' text, read when first wanted (selecting; row 367c)
    texts: HashMap<usize, std::sync::Arc<crate::pdf::PageText>>, /// the text selected: (page, where it began, where it ends), and a drag is extending it
    pub sel: Option<(usize, usize, usize)>, selecting: bool, pub find: Option<Find>,
    back: ActionButton, b_out: ActionButton, b_in: ActionButton, b_fit: ActionButton, opened: Instant, /// the page to begin at, once the file's pages are known
    pub start: usize,
}

fn fill(scene: &mut vello::Scene, r: Rect, c: peniko::Color) { scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, c, None, &r); }
/// Every page's size, read once off the window's thread.
pub fn sizes(path: &std::path::Path) -> Result<Vec<(f32, f32)>, String> { crate::pdf::sizes(path) }

impl Studio {
    /// `first`: the page's shape as the record has it, shown until the file's own pages are known; `page`: where to begin.
    pub fn open(host: Host, path: PathBuf, name: String, first: (f32, f32), wake: impl Fn() + Send + Sync + 'static) -> Studio { Studio::open_with(host, path, name, first, wake, true) }
    fn open_with(host: Host, path: PathBuf, name: String, first: (f32, f32), wake: impl Fn() + Send + Sync + 'static, read: bool) -> Studio {
        let wake: std::sync::Arc<dyn Fn() + Send + Sync> = std::sync::Arc::new(wake);
        let (tx, rx) = channel(); let (stx, srx) = channel();
        if read { let (p, w) = (path.clone(), wake.clone()); let _ = std::thread::Builder::new().name("mn-studio-sizes".into()).spawn(move || { let _ = stx.send(sizes(&p)); w(); }); }
        Studio { host, path, name, sizes: vec![first], sizes_rx: read.then_some(srx), scroll: 0.0, scroll_x: 0.0, rect: Rect::ZERO, full: Rect::ZERO, strip_scroll: 0.0, smalls: HashMap::new(), smalls_asked: Default::default(), strip_at: usize::MAX, scale: 1.0, zoom: Zoom::Width, zoomed: Instant::now() - std::time::Duration::from_secs(1), picture: false, pics: HashMap::new(), asked: HashMap::new(), failed: HashMap::new(), tx, rx, wake, texts: HashMap::new(), sel: None, selecting: false, find: None,
            back: ActionButton::new("arrow-left", "Back (Esc)"), b_out: ActionButton::new("minus", "Zoom out").size_s(), b_in: ActionButton::new("plus", "Zoom in").size_s(), b_fit: ActionButton::new("arrows-in-simple", "Fit").size_s(), opened: Instant::now(), start: 0 }
    }
    /// A picture's view: one page of the picture's own size; nothing is read here (the caller hands its picture to `paint`).
    pub fn open_picture(host: Host, path: PathBuf, name: String, size: (f32, f32), wake: impl Fn() + Send + Sync + 'static) -> Studio {
        let mut st = Studio::open_with(host, path, name, size, wake, false); st.picture = true; st.zoom = Zoom::Whole; st
    }
    pub fn pages(&self) -> usize { self.sizes.len() }
    // ── selecting a page's text (row 367c) ──
    fn text_of(&mut self, page: usize) -> Option<std::sync::Arc<crate::pdf::PageText>> {
        if self.picture { return None } if let Some(t) = self.texts.get(&page) { return Some(t.clone()) }
        let t = std::sync::Arc::new(crate::pdf::text(&self.path, page).ok()?); if self.texts.len() > 64 { self.texts.clear(); } self.texts.insert(page, t.clone()); Some(t)
    }
    /// The character of a page at a window point: (the page, the character).
    fn char_at(&mut self, x: f64, y: f64, page: Option<usize>) -> Option<(usize, usize)> {
        let (col, _) = self.column(); let off = kurbo::Vec2::new(self.rect.x0, self.rect.y0 - self.scroll);
        let i = match page { Some(p) => p, None => col.iter().position(|r| (*r + off).contains(kurbo::Point::new(x, y)))? }; let r = *col.get(i)? + off;
        let t = self.text_of(i)?; let k = t.w as f64 / r.width().max(1.0); t.at(((x - r.x0) * k) as f32, ((y - r.y0) * k) as f32).map(|c| (i, c))
    }
    /// A press on a page with no tool in hand: a selection begins at the character there. True: there was text to begin at.
    pub fn select_press(&mut self, x: f64, y: f64) -> bool { match self.char_at(x, y, None) { Some((p, c)) => { self.sel = Some((p, c, c)); self.selecting = true; true } None => { self.sel = None; false } } }
    pub fn selecting(&self) -> bool { self.selecting }
    pub fn select_drag(&mut self, x: f64, y: f64) { if let Some((p, a, _)) = self.sel { if let Some((_, c)) = self.char_at(x, y, Some(p)) { self.sel = Some((p, a, c)); } } }
    /// The drag ends; a press that did not move selects nothing.
    pub fn select_end(&mut self) { self.selecting = false; if self.sel.is_some_and(|(_, a, b)| a == b) { self.sel = None; } }
    /// Every character of the page in view.
    pub fn select_all(&mut self) { let p = self.current(); if let Some(t) = self.text_of(p) { if !t.chars.is_empty() { self.sel = Some((p, 0, t.chars.len() - 1)); } } }
    pub fn selected_text(&mut self) -> Option<String> { let (p, a, b) = self.sel?; let t = self.text_of(p)?; Some(t.words(a, b)).filter(|s| !s.is_empty()) }
    // ── find (row 367d) ──
    /// The find field is opened (or given the keys again); the words in it are looked for through every page on a helper thread.
    pub fn find_open(&mut self, fonts: &mut Fonts) {
        if self.picture { return }
        match self.find.as_mut() { Some(f) => { f.field.focused = true; f.field.select_all(fonts); } None => { let mut field = min_ui::TextField::new(Family::Sans, 12.0); field.placeholder = "Find in this PDF\u{2026}".into(); field.set_text_end("", fonts); field.focused = true;
            self.find = Some(Find { field, query: String::new(), hits: vec![], at: 0, read: 0, turn: Default::default(), rx: None }); } }
    }
    pub fn find_close(&mut self) { if let Some(f) = self.find.take() { f.turn.fetch_add(1, std::sync::atomic::Ordering::Relaxed); } }
    pub fn finding(&self) -> bool { self.find.as_ref().is_some_and(|f| f.field.focused) }
    /// A key to the find field. Enter goes to the next place found (Shift: the one before).
    pub fn find_key(&mut self, k: &min_ui::KeyEvent, fonts: &mut Fonts) {
        let Some(f) = self.find.as_mut() else { return };
        if matches!(k.key, min_ui::Key::Named(min_ui::Named::Enter)) { self.find_step(!k.shift); return }
        f.field.key(k, fonts); let q = f.field.text().trim().to_string(); if q == f.query { return }
        f.query = q.clone(); f.hits.clear(); f.at = 0; f.read = 0; let turn = f.turn.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        if q.chars().filter(|c| !c.is_whitespace()).count() == 0 { f.rx = None; return }
        let (tx, rx) = channel(); f.rx = Some(rx); let (path, wake, flag, n) = (self.path.clone(), self.wake.clone(), f.turn.clone(), self.sizes.len());
        let _ = std::thread::Builder::new().name("mn-studio-find".into()).spawn(move || { for p in 0..n { if flag.load(std::sync::atomic::Ordering::Relaxed) != turn { return }
            let found = crate::pdf::text(&path, p).map(|t| matches(&t, &q)).unwrap_or_default(); if tx.send((turn, p, found)).is_err() { return } if p % 8 == 7 || p + 1 == n { wake(); } } });
    }
    /// To the next place found, or the one before; the pages scroll to it.
    pub fn find_step(&mut self, forward: bool) {
        let Some(f) = self.find.as_mut() else { return }; if f.hits.is_empty() { return }
        f.at = if forward { (f.at + 1) % f.hits.len() } else { (f.at + f.hits.len() - 1) % f.hits.len() }; let (p, a, _) = f.hits[f.at];
        self.find_show(p, a);
    }
    fn find_show(&mut self, page: usize, ch: usize) {
        let (col, _) = self.column(); let Some(r) = col.get(page).copied() else { return };
        let y = self.text_of(page).and_then(|t| t.chars.get(ch).map(|c| c.y as f64 / t.w as f64 * r.width())).unwrap_or(0.0);
        self.scroll = r.y0 + y - self.rect.height() * 0.35; self.limit();
    }
    /// The whole view (presses anywhere in it are its own).
    pub fn area(&self) -> Rect { self.full }
    /// The strip of small pages: a PDF of more than one page has it.
    fn strip(&self) -> Option<Rect> { (!self.picture && self.sizes.len() > 1).then(|| Rect::new(self.full.x0, self.full.y0 + 36.0 * self.scale, self.full.x0 + STRIP_W * self.scale, self.full.y1)) }
    pub fn in_strip(&self, x: f64, y: f64) -> bool { self.strip().is_some_and(|r| r.contains(kurbo::Point::new(x, y))) }
    /// The small pages' places in the strip's own space (y from its top), and the whole height.
    fn strip_rows(&self) -> (Vec<Rect>, f64) {
        let s = self.scale; let w = SMALL_W * s; let x = (STRIP_W * s - w) / 2.0; let mut y = 12.0 * s; let mut out = Vec::with_capacity(self.sizes.len());
        for (pw, ph) in &self.sizes { let h = w * (*ph as f64 / (*pw as f64).max(1.0)); out.push(Rect::new(x, y, x + w, y + h)); y += h + SMALL_GAP * s; }
        (out, y)
    }
    fn strip_limit(&mut self) { if let Some(r) = self.strip() { let (_, h) = self.strip_rows(); self.strip_scroll = self.strip_scroll.clamp(0.0, (h - r.height()).max(0.0)); } }
    /// The wheel at a window point: over the strip it moves the strip, elsewhere the pages.
    pub fn wheel_at(&mut self, x: f64, y: f64, dx: f64, dy: f64) { if self.in_strip(x, y) { self.strip_scroll += dy; self.strip_limit(); } else { self.wheel(dx, dy); } }
    /// The factor from a page's own units to the window's pixels.
    fn factor(&self) -> f64 {
        let s = self.scale; let margin = MARGIN * s; let widest = self.sizes.iter().map(|p| p.0).fold(1.0f32, f32::max) as f64; let tallest = self.sizes.iter().map(|p| p.1).fold(1.0f32, f32::max) as f64;
        let (vw, vh) = ((self.rect.width() - 2.0 * margin).max(100.0 * s), (self.rect.height() - (36.0 + 2.0 * GAP) * s).max(100.0 * s));
        match self.zoom { Zoom::Width => vw.min(900.0 * s) / widest, Zoom::Whole => (vw / widest).min(vh / tallest), Zoom::By(z) => if self.picture { z } else { z * s } }
    }
    /// The zoom as a person reads it: 100 = a PDF at its paper size on the screen, a picture pixel for pixel.
    pub fn percent(&self) -> f64 { let k = self.factor(); if self.picture { k * 100.0 } else { k / self.scale * 100.0 } }
    /// Zoom by a factor about a point of the window (what is under it stays under it); the view's fit is given up.
    pub fn zoom_by(&mut self, by: f64, at: (f64, f64)) {
        let k0 = self.factor(); let unit = if self.picture { 1.0 } else { self.scale };
        let z = (k0 * by / unit).clamp(0.05, 16.0); let k1 = z * unit; if (k1 - k0).abs() < 1e-9 { return }
        let (col0, _) = self.column(); let (ax, ay) = (at.0 - self.rect.x0, at.1 - self.rect.y0);
        // the page under the point, and where in it the point is (as a fraction): it is found again after the change
        let hit = col0.iter().enumerate().find(|(_, r)| ay + self.scroll <= r.y1).map(|(i, r)| (i, (ax - r.x0) / r.width().max(1.0), (ay + self.scroll - r.y0) / r.height().max(1.0)));
        self.zoom = Zoom::By(z); self.zoomed = Instant::now();
        if let Some((i, fx, fy)) = hit { let (col1, _) = self.column_raw(); if let Some(r) = col1.get(i) { self.scroll = r.y0 + fy * r.height() - ay; self.scroll_x = r.x0 + fx * r.width() - ax; } }
        self.limit();
    }
    pub fn zoom_to(&mut self, z: Zoom) { let at = self.current(); self.zoom = z; self.zoomed = Instant::now(); self.scroll_x = 0.0; self.go_to(at); }
    /// The column: each page's rectangle in the view's own space (y from the column's top; x moved by the sideways
    /// scroll when the pages are wider than the view), and its whole height.
    fn column(&self) -> (Vec<Rect>, f64) { let (mut col, h) = self.column_raw(); if self.scroll_x != 0.0 { for r in col.iter_mut() { *r = *r - kurbo::Vec2::new(self.scroll_x, 0.0); } } (col, h) }
    fn column_raw(&self) -> (Vec<Rect>, f64) {
        let s = self.scale; let (gap, margin) = (GAP * s, MARGIN * s); let k = self.factor();
        let mut y = gap + 36.0 * s; let mut out = Vec::with_capacity(self.sizes.len());
        for (w, h) in &self.sizes { let (pw, ph) = (*w as f64 * k, *h as f64 * k); let x = if pw + 2.0 * margin <= self.rect.width() { (self.rect.width() - pw) / 2.0 } else { margin }; out.push(Rect::new(x, y, x + pw, y + ph)); y += ph + gap; }
        // one page that is shorter than the view stands in its middle
        if out.len() == 1 && y < self.rect.height() { let d = (self.rect.height() - y) / 2.0; out[0] = out[0] + kurbo::Vec2::new(0.0, d); }
        (out, y)
    }
    /// How far the pages reach past the view sideways.
    fn over_x(&self) -> f64 { let (col, _) = self.column_raw(); let m = MARGIN * self.scale; (col.iter().map(|r| r.x1).fold(0.0, f64::max) + m - self.rect.width()).max(0.0) }
    fn limit(&mut self) { let (_, h) = self.column(); self.scroll = self.scroll.clamp(0.0, (h - self.rect.height()).max(0.0)); self.scroll_x = self.scroll_x.clamp(0.0, self.over_x()); }
    /// The page whose picture holds a window point, with the page's rectangle in the window.
    pub fn page_at(&self, x: f64, y: f64) -> Option<(usize, Rect)> {
        let (col, _) = self.column(); let off = kurbo::Vec2::new(self.rect.x0, self.rect.y0 - self.scroll);
        col.iter().enumerate().map(|(i, r)| (i, *r + off)).find(|(_, r)| r.contains(kurbo::Point::new(x, y)))
    }
    /// The page most of the view shows (for the readout).
    pub fn current(&self) -> usize { let (col, _) = self.column(); let mid = self.scroll + self.rect.height() * 0.5; col.iter().position(|r| r.y1 >= mid).unwrap_or(col.len().saturating_sub(1)) }
    pub fn go_to(&mut self, page: usize) { let (col, _) = self.column(); if let Some(r) = col.get(page) { self.scroll = r.y0 - GAP * self.scale; self.limit(); } }
    pub fn wheel(&mut self, dx: f64, dy: f64) { self.scroll += dy; self.scroll_x += dx; self.limit(); }
    /// A key that moves the view (`PageDown`, `PageUp`, `Down`, `Up`, `Home`, `End`, `Space`): true when it was one.
    pub fn key(&mut self, what: &str, shift: bool) -> bool {
        let (page, line) = (self.rect.height() * 0.9, 60.0 * self.scale);
        match what {
            "PageDown" => self.scroll += page, "PageUp" => self.scroll -= page, "Down" => self.scroll += line, "Up" => self.scroll -= line,
            "Home" => self.scroll = 0.0, "End" => self.scroll = f64::MAX, "Space" => self.scroll += if shift { -page } else { page },
            _ => return false,
        }
        self.limit(); true
    }
    /// A pointer event: true when Back was pressed.
    pub fn pointer(&mut self, e: &Event) -> bool { let now = Instant::now(); let mid = (self.rect.center().x, self.rect.center().y);
        // a small page pressed: the pages go to it
        if let (Event::Down { x, y, .. }, Some(sr)) = (e, self.strip()) { if sr.contains(kurbo::Point::new(*x, *y)) { let (rows, _) = self.strip_rows(); let ly = *y - sr.y0 + self.strip_scroll; if let Some(i) = rows.iter().position(|r| ly >= r.y0 - 4.0 && ly <= r.y1 + SMALL_GAP * self.scale - 4.0) { self.go_to(i); } return false } }
        if self.b_out.event(e, now) == Response::Clicked { self.zoom_by(1.0 / 1.25, mid); } if self.b_in.event(e, now) == Response::Clicked { self.zoom_by(1.25, mid); }
        if self.b_fit.event(e, now) == Response::Clicked { let z = if self.picture || self.zoom == Zoom::Width { Zoom::Whole } else { Zoom::Width }; self.zoom_to(z); }
        self.back.event(e, now) == Response::Clicked }
    /// The head's band (Back, the name): a press there is no press on a page.
    pub fn in_head(&self, y: f64) -> bool { y < self.full.y0 + 36.0 * self.scale }
    /// What arrived from the helpers is taken up. True: draw again.
    pub fn poll(&mut self) -> bool {
        let mut again = false;
        if let Some(rx) = &self.sizes_rx { if let Ok(r) = rx.try_recv() { self.sizes_rx = None; again = true;
            match r { Ok(s) if !s.is_empty() => { self.sizes = s; self.pics.clear(); self.asked.clear(); self.smalls.clear(); self.smalls_asked.clear(); let at = self.start.min(self.sizes.len() - 1); self.go_to(at); } Ok(_) => {} Err(why) => { self.failed.insert(0, why); } } } }
        if let Some(f) = self.find.as_mut() { let turn = f.turn.load(std::sync::atomic::Ordering::Relaxed); let mut first = None;
            if let Some(rx) = &f.rx { while let Ok((g, p, found)) = rx.try_recv() { if g != turn { continue } again = true; f.read = p + 1; let was = f.hits.is_empty(); f.hits.extend(found.into_iter().map(|(a, b)| (p, a, b))); if was && !f.hits.is_empty() { first = Some((f.hits[0].0, f.hits[0].1)); } } }
            if let Some((p, a)) = first { self.find_show(p, a); } }
        while let Ok(m) = self.rx.try_recv() { again = true; if std::env::var_os("MN_STUDIO_LOG").is_some() { eprintln!("studio: made page {} at {} ok {} (asked {:?})", m.page, m.width, m.px.is_ok(), self.asked.get(&m.page)); }
            if m.small { if let Ok((px, w, h)) = m.px { self.smalls.insert(m.page, image_rgba8(w, h, px)); } continue }
            if self.asked.get(&m.page) != Some(&m.width) { continue }   // asked for at another width since
            match m.px { Ok((px, w, h)) => { self.pics.insert(m.page, (m.width, image_rgba8(w, h, px))); self.failed.remove(&m.page); } Err(why) => { self.failed.insert(m.page, why); } }
            self.asked.remove(&m.page); }
        again
    }
    fn ask(&mut self, page: usize, width: u32) {
        if self.asked.get(&page) == Some(&width) || self.pics.get(&page).is_some_and(|p| p.0 == width) || self.failed.contains_key(&page) { return }
        if std::env::var_os("MN_STUDIO_LOG").is_some() { eprintln!("studio: ask page {page} at {width}"); }
        self.asked.insert(page, width); let (tx, path, wake) = (self.tx.clone(), self.path.clone(), self.wake.clone());
        let _ = std::thread::Builder::new().name("mn-studio-page".into()).spawn(move || { let px = crate::pdf::page(&path, page, width); let _ = tx.send(Made { page, width, px, small: false }); wake(); });
    }
    /// Lays the view out in `rect` and paints it: the ground, the pages in view (white until their picture is made),
    /// each page's drawings (`ink`: the lines of a page, in the page's own record units, with the record's width for
    /// that page), the head with Back and the name. True: draw again (pictures are on their way).
    pub fn paint(&mut self, scene: &mut vello::Scene, fonts: &mut Fonts, rect: Rect, scale: f64, own: Option<&peniko::ImageBrush>, ink: &dyn Fn(usize, (f64, f64)) -> (Vec<crate::ink::Line>, f64, Option<Rect>)) -> bool {
        self.full = rect; self.scale = scale; let strip = self.strip(); let rect = match strip { Some(sr) => Rect::new(sr.x1, rect.y0, rect.x1, rect.y1), None => rect };
        let resized = (rect.width() - self.rect.width()).abs() > 0.5; let at = if resized && self.rect.width() > 0.0 { Some(self.current()) } else { None };
        self.rect = rect; self.scale = scale; if let Some(p) = at { self.go_to(p); } self.limit();
        let mut again = self.poll(); let s = scale;
        fill(scene, rect, color(t::BACKGROUND_BASE_COLOR));
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &rect);
        let (col, _) = self.column(); let off = kurbo::Vec2::new(rect.x0, rect.y0 - self.scroll);
        let seen: Vec<usize> = col.iter().enumerate().filter(|(_, r)| { let r = **r + off; r.y1 > rect.y0 && r.y0 < rect.y1 }).map(|(i, _)| i).collect();
        let (lo, hi) = (seen.first().copied().unwrap_or(0).saturating_sub(AROUND), (seen.last().copied().unwrap_or(0) + AROUND).min(col.len().saturating_sub(1)));
        self.pics.retain(|p, _| *p >= lo && *p <= hi); self.asked.retain(|p, _| *p >= lo && *p <= hi);
        // (while the zoom is changing the pictures held are shown at the new size; the sharp ones are asked for once it has rested)
        let rested = self.zoomed.elapsed().as_millis() >= 150; if !rested { again = true; }
        if !self.picture && rested { for i in lo..=hi { let Some(r) = col.get(i) else { continue }; let width = (r.width().round() as u32).clamp(64, WIDEST); self.ask(i, width); } }
        for i in seen { let r = col[i] + off;
            fill(scene, r, peniko::Color::WHITE);
            match own.filter(|_| self.picture).or(self.pics.get(&i).map(|p| &p.1)) { Some(b) => image(scene, b, r), None => { again = true;
                if let Some(why) = self.failed.get(&i) { let l = fonts.layout_fit(why, Family::Sans, (13.0 * s) as f32, 400.0, peniko::Color::from_rgb8(120, 120, 120), r.width() - 32.0 * s); draw_layout(scene, &l, Affine::translate((r.x0 + 16.0 * s, r.y0 + 16.0 * s))); again = false; } } }
            // the text selected on this page, and the places found on it (the one stood on stronger)
            { let mut marks: Vec<(usize, usize, peniko::Color)> = vec![];
                if let Some(f) = &self.find { for (n, (p, a, b)) in f.hits.iter().enumerate() { if *p == i { marks.push((*a, *b, if n == f.at { peniko::Color::from_rgba8(255, 170, 0, 150) } else { peniko::Color::from_rgba8(255, 220, 0, 80) })); } } }
                if let Some((p, a, b)) = self.sel { if p == i { marks.push((a.min(b), a.max(b), peniko::Color::from_rgba8(64, 105, 253, 90))); } }
                if !marks.is_empty() { if let Some(t) = self.text_of(i) { let k = r.width() / t.w as f64; for (a, b, c) in marks { for (x0, y0, x1, y1) in t.rects(a, b) { fill(scene, Rect::new(r.x0 + x0 as f64 * k, r.y0 + y0 as f64 * k, r.x0 + x1 as f64 * k, r.y0 + y1 as f64 * k), c); } } } } }
            // the page's drawings: stored in the record's own units for that page, shown at the page's size here
            let (pw, ph) = self.sizes.get(i).copied().unwrap_or((1.0, 1.0)); let (lines, rec_w, picked) = ink(i, (pw as f64, ph as f64));
            if !lines.is_empty() && rec_w > 0.0 { let k = r.width() / rec_w; scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &r);
                for l in &lines { crate::ink::drawn(l, kurbo::Point::ORIGIN).paint(scene, Affine::translate((r.x0, r.y0)) * Affine::scale(k)); }
                // the picked stroke's box
                if let Some(b) = picked { let b = Rect::new(r.x0 + b.x0 * k, r.y0 + b.y0 * k, r.x0 + b.x1 * k, r.y0 + b.y1 * k).inflate(4.0 * s, 4.0 * s); scene.stroke(&kurbo::Stroke::new(1.5 * s), Affine::IDENTITY, color(t::ACCENT_BACKGROUND_COLOR_DEFAULT), None, &b); }
                scene.pop_layer(); }
            scene.stroke(&kurbo::Stroke::new(s.max(1.0)), Affine::IDENTITY, color(t::DIVIDER_COLOR), None, &r);
        }
        scene.pop_layer();
        // the strip of small pages (row 367b): every page small, the one in view marked; a press goes to a page; it keeps the page in view in sight
        if let Some(sr) = strip {
            fill(scene, sr, color(t::BACKGROUND_LAYER_1_COLOR)); scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &sr);
            let cur = self.current(); let (rows, _) = self.strip_rows();
            if cur != self.strip_at { self.strip_at = cur; if let Some(r) = rows.get(cur) { let (top, bot) = (r.y0 - 12.0 * s, r.y1 + SMALL_GAP * s); if top < self.strip_scroll { self.strip_scroll = top; } else if bot > self.strip_scroll + sr.height() { self.strip_scroll = bot - sr.height(); } } self.strip_limit(); }
            let seen: Vec<usize> = rows.iter().enumerate().filter(|(_, r)| { let y0 = r.y0 - self.strip_scroll; y0 < sr.height() && r.y1 - self.strip_scroll > 0.0 }).map(|(i, _)| i).collect();
            // (small pictures are kept for the rows near the ones in sight only: a long document has hundreds)
            if let (Some(a), Some(z)) = (seen.first().copied(), seen.last().copied()) { self.smalls.retain(|p, _| *p + 40 >= a && *p <= z + 40); self.smalls_asked.retain(|p| *p + 40 >= a && *p <= z + 40); }
            for i in seen { let r = rows[i] + kurbo::Vec2::new(sr.x0, sr.y0 - self.strip_scroll);
                if i == cur { scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, color(t::BACKGROUND_LAYER_2_COLOR), None, &kurbo::RoundedRect::from_rect(Rect::new(r.x0 - 6.0 * s, r.y0 - 6.0 * s, r.x1 + 6.0 * s, r.y1 + (SMALL_GAP - 6.0) * s), 6.0 * s)); }
                fill(scene, r, peniko::Color::WHITE);
                match self.smalls.get(&i) { Some(b) => image(scene, b, r), None => { again = true;
                    if self.smalls_asked.insert(i) { let (tx, path, wake, width) = (self.tx.clone(), self.path.clone(), self.wake.clone(), (SMALL_W * s).round().max(32.0) as u32);
                        let _ = std::thread::Builder::new().name("mn-studio-small".into()).spawn(move || { let px = crate::pdf::page(&path, i, width); let _ = tx.send(Made { page: i, width, px, small: true }); wake(); }); } } }
                if i == cur { scene.stroke(&kurbo::Stroke::new(1.5 * s), Affine::IDENTITY, color(t::ACCENT_BACKGROUND_COLOR_DEFAULT), None, &r); }
                let n = fonts.layout(&(i + 1).to_string(), Family::Sans, (11.0 * s) as f32, if i == cur { 600.0 } else { 400.0 }, color(if i == cur { t::NEUTRAL_CONTENT_COLOR_DEFAULT } else { t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT }));
                draw_layout(scene, &n, Affine::translate((r.center().x - n.width() as f64 / 2.0, r.y1 + 3.0 * s))); }
            scene.pop_layer();
            scene.stroke(&kurbo::Stroke::new(s.max(1.0)), Affine::IDENTITY, color(t::DIVIDER_COLOR), None, &kurbo::Line::new((sr.x1, sr.y0), (sr.x1, sr.y1)));
        }
        // the head: Back, the file's name, the page of pages
        let rect = self.full; let head = Rect::new(rect.x0, rect.y0, rect.x1, rect.y0 + 36.0 * s); fill(scene, head, color(t::BACKGROUND_LAYER_1_COLOR));
        let (bw, bh) = self.back.measure(fonts, s); self.back.rect = Rect::new(head.x0 + 8.0 * s, head.center().y - bh / 2.0, head.x0 + 8.0 * s + bw, head.center().y + bh / 2.0);
        let now = Instant::now(); self.back.paint(scene, fonts, s, now); again |= self.back.animating(now);
        let title = fonts.layout_fit(&self.name, Family::Sans, (13.0 * s) as f32, 500.0, color(t::NEUTRAL_CONTENT_COLOR_DEFAULT), head.width() * 0.6);
        draw_layout(scene, &title, Affine::translate((self.back.rect.x1 + 10.0 * s, head.center().y - title.height() as f64 / 2.0)));
        // the zoom, at the head's right: out, the figure, in, fit
        let mut x = head.x1 - 10.0 * s; for b in [&mut self.b_fit, &mut self.b_in] { let (w, h) = b.measure(fonts, s); b.rect = Rect::new(x - w, head.center().y - h / 2.0, x, head.center().y + h / 2.0); x -= w + 2.0 * s; }
        let pc = fonts.layout(&format!("{} %", self.percent().round() as i64), Family::Sans, (12.0 * s) as f32, 500.0, color(t::NEUTRAL_CONTENT_COLOR_DEFAULT)); x -= pc.width() as f64 + 6.0 * s; draw_layout(scene, &pc, Affine::translate((x, head.center().y - pc.height() as f64 / 2.0))); x -= 6.0 * s;
        { let (w, h) = self.b_out.measure(fonts, s); self.b_out.rect = Rect::new(x - w, head.center().y - h / 2.0, x, head.center().y + h / 2.0); x -= w + 14.0 * s; }
        for b in [&self.b_out, &self.b_in, &self.b_fit] { b.paint(scene, fonts, s, now); again |= b.animating(now); }
        if let Some(f) = self.find.as_mut() { let fw = 220.0 * s; let fr = Rect::new(x - fw, head.y0 + 5.0 * s, x, head.y1 - 5.0 * s); f.field.layout(fonts, fr, s); f.field.paint(scene, fonts, s); x -= fw + 10.0 * s;
            let said = if f.query.is_empty() { String::new() } else if f.hits.is_empty() { if f.read < self.sizes.len() { format!("reading page {} of {}", f.read + 1, self.sizes.len()) } else { "not found".to_string() } } else { format!("{} of {}{}", f.at + 1, f.hits.len(), if f.read < self.sizes.len() { "\u{2026}" } else { "" }) };
            let cl = fonts.layout(&said, Family::Sans, (12.0 * s) as f32, 400.0, color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT)); draw_layout(scene, &cl, Affine::translate((x - cl.width() as f64, head.center().y - cl.height() as f64 / 2.0))); x -= cl.width() as f64 + 14.0 * s;
            if f.rx.is_some() && f.read < self.sizes.len() && !f.query.is_empty() { again = true; } }
        let n = if self.picture { String::new() } else { format!("{} / {}", self.current() + 1, self.pages()) }; let nl = fonts.layout(&n, Family::Sans, (12.0 * s) as f32, 400.0, color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT));
        draw_layout(scene, &nl, Affine::translate((x - nl.width() as f64, head.center().y - nl.height() as f64 / 2.0)));
        scene.stroke(&kurbo::Stroke::new(s.max(1.0)), Affine::IDENTITY, color(t::DIVIDER_COLOR), None, &kurbo::Line::new((head.x0, head.y1), (head.x1, head.y1)));
        let _ = self.opened; again
    }
}
