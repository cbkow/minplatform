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

struct Made { page: usize, width: u32, px: Result<(Vec<u8>, u32, u32), String> }

pub struct Studio {
    pub host: Host, path: PathBuf, name: String,
    /// every page's size in points; one entry (the record's own shape) until the file has been read
    sizes: Vec<(f32, f32)>, sizes_rx: Option<Receiver<Result<Vec<(f32, f32)>, String>>>,
    pub scroll: f64, pub rect: Rect, scale: f64,
    pics: HashMap<usize, (u32, peniko::ImageBrush)>, asked: HashMap<usize, u32>, failed: HashMap<usize, String>,
    tx: Sender<Made>, rx: Receiver<Made>, wake: std::sync::Arc<dyn Fn() + Send + Sync>,
    back: ActionButton, opened: Instant, /// the page to begin at, once the file's pages are known
    pub start: usize,
}

fn fill(scene: &mut vello::Scene, r: Rect, c: peniko::Color) { scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, c, None, &r); }
/// Every page's size, read once off the window's thread.
pub fn sizes(path: &std::path::Path) -> Result<Vec<(f32, f32)>, String> { crate::pdf::sizes(path) }

impl Studio {
    /// `first`: the page's shape as the record has it, shown until the file's own pages are known; `page`: where to begin.
    pub fn open(host: Host, path: PathBuf, name: String, first: (f32, f32), wake: impl Fn() + Send + Sync + 'static) -> Studio {
        let wake: std::sync::Arc<dyn Fn() + Send + Sync> = std::sync::Arc::new(wake);
        let (tx, rx) = channel(); let (stx, srx) = channel();
        { let (p, w) = (path.clone(), wake.clone()); let _ = std::thread::Builder::new().name("mn-studio-sizes".into()).spawn(move || { let _ = stx.send(sizes(&p)); w(); }); }
        Studio { host, path, name, sizes: vec![first], sizes_rx: Some(srx), scroll: 0.0, rect: Rect::ZERO, scale: 1.0, pics: HashMap::new(), asked: HashMap::new(), failed: HashMap::new(), tx, rx, wake,
            back: ActionButton::new("arrow-left", "Back (Esc)"), opened: Instant::now(), start: 0 }
    }
    pub fn pages(&self) -> usize { self.sizes.len() }
    /// The column: each page's rectangle in the view's own space (y from the column's top), and its whole height.
    fn column(&self) -> (Vec<Rect>, f64) {
        let s = self.scale; let (gap, margin) = (GAP * s, MARGIN * s);
        let widest = self.sizes.iter().map(|p| p.0).fold(1.0f32, f32::max) as f64;
        // fit width: the widest page fills the view - but no wider than a page reads well at (until the zoom of stage 2 gives the choice)
        let k = ((self.rect.width() - 2.0 * margin).max(100.0 * s)).min(900.0 * s) / widest;
        let mut y = gap + 36.0 * s; let mut out = Vec::with_capacity(self.sizes.len());
        for (w, h) in &self.sizes { let (pw, ph) = (*w as f64 * k, *h as f64 * k); let x = (self.rect.width() - pw) / 2.0; out.push(Rect::new(x, y, x + pw, y + ph)); y += ph + gap; }
        (out, y)
    }
    fn limit(&mut self) { let (_, h) = self.column(); self.scroll = self.scroll.clamp(0.0, (h - self.rect.height()).max(0.0)); }
    /// The page whose picture holds a window point, with the page's rectangle in the window.
    pub fn page_at(&self, x: f64, y: f64) -> Option<(usize, Rect)> {
        let (col, _) = self.column(); let off = kurbo::Vec2::new(self.rect.x0, self.rect.y0 - self.scroll);
        col.iter().enumerate().map(|(i, r)| (i, *r + off)).find(|(_, r)| r.contains(kurbo::Point::new(x, y)))
    }
    /// The page most of the view shows (for the readout).
    pub fn current(&self) -> usize { let (col, _) = self.column(); let mid = self.scroll + self.rect.height() * 0.5; col.iter().position(|r| r.y1 >= mid).unwrap_or(col.len().saturating_sub(1)) }
    pub fn go_to(&mut self, page: usize) { let (col, _) = self.column(); if let Some(r) = col.get(page) { self.scroll = r.y0 - GAP * self.scale; self.limit(); } }
    pub fn wheel(&mut self, dy: f64) { self.scroll += dy; self.limit(); }
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
    pub fn pointer(&mut self, e: &Event) -> bool { self.back.event(e, Instant::now()) == Response::Clicked }
    /// The head's band (Back, the name): a press there is no press on a page.
    pub fn in_head(&self, y: f64) -> bool { y < self.rect.y0 + 36.0 * self.scale }
    /// What arrived from the helpers is taken up. True: draw again.
    pub fn poll(&mut self) -> bool {
        let mut again = false;
        if let Some(rx) = &self.sizes_rx { if let Ok(r) = rx.try_recv() { self.sizes_rx = None; again = true;
            match r { Ok(s) if !s.is_empty() => { self.sizes = s; self.pics.clear(); self.asked.clear(); let at = self.start.min(self.sizes.len() - 1); self.go_to(at); } Ok(_) => {} Err(why) => { self.failed.insert(0, why); } } } }
        while let Ok(m) = self.rx.try_recv() { again = true; if std::env::var_os("MN_STUDIO_LOG").is_some() { eprintln!("studio: made page {} at {} ok {} (asked {:?})", m.page, m.width, m.px.is_ok(), self.asked.get(&m.page)); }
            if self.asked.get(&m.page) != Some(&m.width) { continue }   // asked for at another width since
            match m.px { Ok((px, w, h)) => { self.pics.insert(m.page, (m.width, image_rgba8(w, h, px))); self.failed.remove(&m.page); } Err(why) => { self.failed.insert(m.page, why); } }
            self.asked.remove(&m.page); }
        again
    }
    fn ask(&mut self, page: usize, width: u32) {
        if self.asked.get(&page) == Some(&width) || self.pics.get(&page).is_some_and(|p| p.0 == width) || self.failed.contains_key(&page) { return }
        if std::env::var_os("MN_STUDIO_LOG").is_some() { eprintln!("studio: ask page {page} at {width}"); }
        self.asked.insert(page, width); let (tx, path, wake) = (self.tx.clone(), self.path.clone(), self.wake.clone());
        let _ = std::thread::Builder::new().name("mn-studio-page".into()).spawn(move || { let px = crate::pdf::page(&path, page, width); let _ = tx.send(Made { page, width, px }); wake(); });
    }
    /// Lays the view out in `rect` and paints it: the ground, the pages in view (white until their picture is made),
    /// each page's drawings (`ink`: the lines of a page, in the page's own record units, with the record's width for
    /// that page), the head with Back and the name. True: draw again (pictures are on their way).
    pub fn paint(&mut self, scene: &mut vello::Scene, fonts: &mut Fonts, rect: Rect, scale: f64, ink: &dyn Fn(usize, (f64, f64)) -> (Vec<crate::ink::Line>, f64, Option<Rect>)) -> bool {
        let resized = (rect.width() - self.rect.width()).abs() > 0.5; let at = if resized && self.rect.width() > 0.0 { Some(self.current()) } else { None };
        self.rect = rect; self.scale = scale; if let Some(p) = at { self.go_to(p); } self.limit();
        let mut again = self.poll(); let s = scale;
        fill(scene, rect, color(t::BACKGROUND_BASE_COLOR));
        scene.push_clip_layer(peniko::Fill::NonZero, Affine::IDENTITY, &rect);
        let (col, _) = self.column(); let off = kurbo::Vec2::new(rect.x0, rect.y0 - self.scroll);
        let seen: Vec<usize> = col.iter().enumerate().filter(|(_, r)| { let r = **r + off; r.y1 > rect.y0 && r.y0 < rect.y1 }).map(|(i, _)| i).collect();
        let (lo, hi) = (seen.first().copied().unwrap_or(0).saturating_sub(AROUND), (seen.last().copied().unwrap_or(0) + AROUND).min(col.len().saturating_sub(1)));
        self.pics.retain(|p, _| *p >= lo && *p <= hi); self.asked.retain(|p, _| *p >= lo && *p <= hi);
        for i in lo..=hi { let Some(r) = col.get(i) else { continue }; let width = (r.width().round() as u32).clamp(64, WIDEST); self.ask(i, width); }
        for i in seen { let r = col[i] + off;
            fill(scene, r, peniko::Color::WHITE);
            match self.pics.get(&i) { Some((_, b)) => image(scene, b, r), None => { again = true;
                if let Some(why) = self.failed.get(&i) { let l = fonts.layout_fit(why, Family::Sans, (13.0 * s) as f32, 400.0, peniko::Color::from_rgb8(120, 120, 120), r.width() - 32.0 * s); draw_layout(scene, &l, Affine::translate((r.x0 + 16.0 * s, r.y0 + 16.0 * s))); again = false; } } }
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
        // the head: Back, the file's name, the page of pages
        let head = Rect::new(rect.x0, rect.y0, rect.x1, rect.y0 + 36.0 * s); fill(scene, head, color(t::BACKGROUND_LAYER_1_COLOR));
        let (bw, bh) = self.back.measure(fonts, s); self.back.rect = Rect::new(head.x0 + 8.0 * s, head.center().y - bh / 2.0, head.x0 + 8.0 * s + bw, head.center().y + bh / 2.0);
        let now = Instant::now(); self.back.paint(scene, fonts, s, now); again |= self.back.animating(now);
        let title = fonts.layout_fit(&self.name, Family::Sans, (13.0 * s) as f32, 500.0, color(t::NEUTRAL_CONTENT_COLOR_DEFAULT), head.width() * 0.6);
        draw_layout(scene, &title, Affine::translate((self.back.rect.x1 + 10.0 * s, head.center().y - title.height() as f64 / 2.0)));
        let n = format!("{} / {}", self.current() + 1, self.pages()); let nl = fonts.layout(&n, Family::Sans, (12.0 * s) as f32, 400.0, color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT));
        draw_layout(scene, &nl, Affine::translate((head.x1 - 14.0 * s - nl.width() as f64, head.center().y - nl.height() as f64 / 2.0)));
        scene.stroke(&kurbo::Stroke::new(s.max(1.0)), Affine::IDENTITY, color(t::DIVIDER_COLOR), None, &kurbo::Line::new((head.x0, head.y1), (head.x1, head.y1)));
        let _ = self.opened; again
    }
}
