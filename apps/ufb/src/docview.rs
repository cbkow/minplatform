//! Documents in the lightbox (slice 4c, canon row 479): text and PDF. A document is read and its pages drawn on a
//! thread of its own; the window draws what has come, scrolls it and asks for the pages in view.
//!
//! - **Text** (plain text, code, Markdown, JSON, logs...): the first 2 MB, monospaced, wrapped at the window's width,
//!   read-only. Markdown is shown as it is written, not rendered.
//! - **PDF**: the family's reader (`min-pdf`), the pages one under the other at the window's width; a page is drawn
//!   when it comes into view, at the size it is shown.
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use min_ui::{fonts::draw_layout, paint, Button, Event, Family, Fonts, ScrollBar};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use vello::Scene;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind { Text, Pdf }
/// The most of a text file that is read and shown.
pub const TEXT_MAX: usize = 2 << 20;
const TEXT: &[&str] = &["txt", "md", "markdown", "json", "log", "csv", "tsv", "xml", "yaml", "yml", "toml", "ini", "cfg", "conf", "py", "rs", "js", "ts", "jsx", "tsx", "c", "h", "cpp", "hpp", "cc", "cs", "java", "sh", "zsh", "bat", "cmd", "ps1", "css", "scss",
    "glsl", "hlsl", "frag", "vert", "osl", "mel", "nk", "edl", "srt", "vtt", "ass", "cube", "lua", "rb", "go", "swift", "kt", "sql", "tex", "plist", "ocio", "usda", "mtl", "html", "htm", "svg", "rst", "nfo", "env", "gitignore", "lock", "cmake", "mk", "gradle", "properties", "ale", "cdl", "ccc", "fdx", "fountain"];
pub fn kind_of(ext: &str) -> Option<Kind> { if ext == "pdf" { Some(Kind::Pdf) } else if TEXT.contains(&ext) { Some(Kind::Text) } else { None } }

enum Got { Text { text: String, cut: bool }, Sizes(Vec<(f32, f32)>), Page { page: usize, width: u32, rgba: Vec<u8>, w: u32, h: u32 }, PageFailed(usize), Failed(String) }

struct TextBody { lines: Vec<String>, cut: bool, /// the rows as wrapped: (line, from byte, to byte), for `cols` characters a row
    rows: Vec<(u32, u32, u32)>, cols: usize }
struct PdfBody { sizes: Vec<(f32, f32)>, /// the pages drawn: the width each was drawn at, its picture
    pages: HashMap<usize, (u32, peniko::ImageBrush)>, failed: Vec<usize>, /// the page at the top of the view
    at: usize }
enum Body { Loading, Text(TextBody), Pdf(PdfBody), Failed(String) }

pub struct Doc {
    pub kind: Kind, body: Body, rx: Receiver<Got>, pub bar: ScrollBar, /// 1: text at its usual size, a PDF's pages as wide as the window allows
    zoom: f64, zoomed: Option<Instant>, pan_x: f64, drag: Option<((f64, f64), f64, f64)>,
    /// goes up whenever what is drawn would differ (the lightbox's layer is rendered again only then)
    pub stamp: u64, since: Instant,
    /// the pages wanted next, the first first; the worker is told there is something
    wanted: Arc<Mutex<Vec<(usize, u32)>>>, kick: Option<Sender<()>>, stop: Arc<AtomicBool>,
}
impl Drop for Doc { fn drop(&mut self) { self.stop.store(true, Ordering::Relaxed); self.kick = None; } }

/// A file's bytes as text: UTF-8 (what is not is shown with marks), UTF-16 by its mark; none when it is not text.
pub fn decode(mut bytes: Vec<u8>) -> Option<(String, bool)> {
    let cut = bytes.len() > TEXT_MAX; if cut { bytes.truncate(TEXT_MAX); }
    let text = if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) { let le = bytes[0] == 0xFF; let u: Vec<u16> = bytes[2..].chunks_exact(2).map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) }).collect(); String::from_utf16_lossy(&u) }
        else { if bytes.iter().take(8192).any(|b| *b == 0) { return None } String::from_utf8_lossy(bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes)).into_owned() };
    // (what was cut ends at a whole line)
    let text = if cut { match text.rfind('\n') { Some(i) => text[..i].to_string(), None => text } } else { text };
    Some((text, cut))
}
/// The text as lines to draw: tabs as four spaces, no other control characters.
fn lines_of(text: &str) -> Vec<String> { text.lines().map(|l| l.replace('\t', "    ").chars().filter(|c| !c.is_control()).collect()).collect() }
/// Every line cut into rows of at most `cols` characters (an empty line is one empty row).
fn wrap(lines: &[String], cols: usize) -> Vec<(u32, u32, u32)> {
    let cols = cols.max(8); let mut rows = Vec::with_capacity(lines.len());
    for (i, l) in lines.iter().enumerate() {
        if l.len() <= cols && l.is_ascii() || l.is_empty() { rows.push((i as u32, 0, l.len() as u32)); continue }
        let (mut from, mut n) = (0usize, 0usize);
        for (b, _) in l.char_indices() { if n == cols { rows.push((i as u32, from as u32, b as u32)); from = b; n = 0; } n += 1; }
        rows.push((i as u32, from as u32, l.len() as u32));
    }
    rows
}

impl Doc {
    pub fn open(kind: Kind, path: &Path, wake: Arc<dyn Fn() + Send + Sync>) -> Doc {
        let (tx, rx) = channel(); let (kick, kicked) = channel::<()>(); let wanted: Arc<Mutex<Vec<(usize, u32)>>> = Default::default(); let stop = Arc::new(AtomicBool::new(false));
        let (p, w, s): (PathBuf, _, _) = (path.to_path_buf(), wanted.clone(), stop.clone());
        let _ = std::thread::Builder::new().name("ufb-doc".into()).spawn(move || {
            let send = |g: Got| { let ok = tx.send(g).is_ok(); wake(); ok };
            match kind {
                Kind::Text => { use std::io::Read;
                    let got = std::fs::File::open(&p).and_then(|f| { let mut b = Vec::new(); f.take(TEXT_MAX as u64 + 1).read_to_end(&mut b)?; Ok(b) });
                    send(match got { Ok(b) => match decode(b) { Some((text, cut)) => Got::Text { text, cut }, None => Got::Failed("This file does not look like text.".into()) }, Err(e) => Got::Failed(format!("This file could not be read: {e}.")) }); }
                Kind::Pdf => {
                    match min_pdf::sizes(&p) { Ok(sizes) if !sizes.is_empty() => { if !send(Got::Sizes(sizes)) { return } } Ok(_) => { send(Got::Failed("This PDF has no pages.".into())); return } Err(e) => { send(Got::Failed(format!("This PDF could not be shown: {e}."))); return } }
                    while kicked.recv().is_ok() {
                        loop { if s.load(Ordering::Relaxed) { return } let Some((page, width)) = w.lock().unwrap().first().copied() else { break };
                            let got = match min_pdf::page(&p, page, width) { Ok((rgba, pw, ph)) => Got::Page { page, width, rgba, w: pw, h: ph }, Err(_) => Got::PageFailed(page) };
                            w.lock().unwrap().retain(|x| *x != (page, width)); if !send(got) { return } }
                    }
                }
            }
        });
        Doc { kind, body: Body::Loading, rx, bar: ScrollBar::default(), zoom: 1.0, zoomed: None, pan_x: 0.0, drag: None, stamp: 0, since: Instant::now(), wanted, kick: Some(kick), stop }
    }
    /// Take in what the worker sent. True when something came.
    pub fn poll(&mut self) -> bool {
        let mut any = false;
        while let Ok(g) = self.rx.try_recv() { any = true; self.stamp += 1;
            match g {
                Got::Text { text, cut } => self.body = Body::Text(TextBody { lines: lines_of(&text), cut, rows: vec![], cols: 0 }),
                Got::Sizes(sizes) => self.body = Body::Pdf(PdfBody { sizes, pages: HashMap::new(), failed: vec![], at: 0 }),
                Got::Page { page, width, rgba, w, h } => if let Body::Pdf(b) = &mut self.body { if rgba.len() == (w * h * 4) as usize && w > 0 { b.pages.insert(page, (width, paint::image_rgba8(w, h, rgba))); } },
                Got::PageFailed(page) => if let Body::Pdf(b) = &mut self.body { if !b.failed.contains(&page) { b.failed.push(page); } },
                Got::Failed(why) => self.body = Body::Failed(why),
            } }
        any
    }
    /// Something is still on its way, or moving: the window keeps drawing.
    pub fn busy(&self, now: Instant) -> bool { matches!(self.body, Body::Loading) || self.zoomed.is_some() || self.bar.animating(now) || self.bar.dragging() || self.drag.is_some() }
    fn limits(&self) -> (f64, f64) { match self.kind { Kind::Text => (0.6, 3.0), Kind::Pdf => (0.25, 5.0) } }
    /// Larger or smaller by `f`; what is at the top of the view stays there.
    pub fn zoom_by(&mut self, f: f64) { let (lo, hi) = self.limits(); let z = (self.zoom * f).clamp(lo, hi); if (z - self.zoom).abs() < 1e-6 { return } self.bar.offset *= z / self.zoom; self.zoom = z; self.zoomed = Some(Instant::now()); self.stamp += 1; }
    pub fn zoom_reset(&mut self) { if self.zoom != 1.0 { let f = 1.0 / self.zoom; self.zoom_by(f); self.pan_x = 0.0; } }
    pub fn wheel(&mut self, dy: f64, now: Instant) { if self.bar.wheel(dy, now) { self.stamp += 1; } }
    /// A key that is the document's: the page keys, Home and End, Up and Down, the zoom. False: not one of ours.
    pub fn key(&mut self, name: &str, ch: &str, area: Rect, now: Instant) -> bool {
        let page = area.height() * 0.9; let line = 60.0;
        let by = match name { "PageDown" => page, "PageUp" => -page, "Down" => line, "Up" => -line, "Home" => -1e12, "End" => 1e12,
            _ => match ch { "=" | "+" => { self.zoom_by(1.25); return true } "-" | "_" => { self.zoom_by(0.8); return true } "0" => { self.zoom_reset(); return true } _ => return false } };
        self.wheel(by, now); true
    }
    /// The pointer: the scroll bar; a drag anywhere else moves the document.
    pub fn pointer(&mut self, e: &Event, area: Rect, now: Instant) {
        let (changed, mine) = self.bar.event(e, now); if changed || mine { self.stamp += 1; } if mine { return }
        match *e {
            Event::Down { x, y, button: Button::Primary } if area.contains(kurbo::Point::new(x, y)) => self.drag = Some(((x, y), self.bar.offset, self.pan_x)),
            Event::Move { x, y } => if let Some((at, o, px)) = self.drag { let to = (o - (y - at.1)).clamp(0.0, self.bar.max()); let pan = px - (x - at.0); if to != self.bar.offset || pan != self.pan_x { self.bar.offset = to; self.pan_x = pan; self.stamp += 1; } },
            Event::Up { button: Button::Primary, .. } | Event::Leave => self.drag = None,
            _ => {}
        }
    }
    /// What the strip says of it.
    pub fn facts(&self) -> Vec<String> {
        match &self.body {
            Body::Text(b) => { let mut v = vec![format!("{} line{}", b.lines.len(), if b.lines.len() == 1 { "" } else { "s" })]; if b.cut { v.push("the first 2 MB shown".into()); } if self.zoom != 1.0 { v.push(format!("{:.0} %", self.zoom * 100.0)); } v }
            Body::Pdf(b) => { let mut v = vec![format!("page {} of {}", b.at + 1, b.sizes.len())]; if self.zoom != 1.0 { v.push(format!("{:.0} %", self.zoom * 100.0)); } v }
            _ => vec![],
        }
    }
    /// The document drawn into `area` (physical px; `s`: the window's scale).
    pub fn draw(&mut self, scene: &mut Scene, fonts: &mut Fonts, area: Rect, s: f64, now: Instant) {
        paint::rect(scene, area, t::BACKGROUND_LAYER_1_COLOR);
        let (ink, sub) = (paint::color(t::NEUTRAL_CONTENT_COLOR_DEFAULT), paint::color(t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT));
        let say = |scene: &mut Scene, fonts: &mut Fonts, text: &str| { let l = fonts.layout_fit(text, Family::Sans, t::BODY_SIZE_M * s as f32, t::FONT_WEIGHT_REGULAR as f32, sub, area.width() - 80.0 * s); draw_layout(scene, &l, Affine::translate((area.center().x - l.width() as f64 / 2.0, area.center().y - l.height() as f64 / 2.0))); };
        // (a zoom is taken as settled a moment after its last step: pages are drawn again at the new size then)
        let settled = self.zoomed.is_none_or(|z| now.duration_since(z) > Duration::from_millis(160)); if settled && self.zoomed.take().is_some() { self.stamp += 1; }
        let margin = 24.0 * s;
        match &mut self.body {
            Body::Loading => if now.duration_since(self.since) > Duration::from_millis(250) { say(scene, fonts, "Loading\u{2026}"); },
            Body::Failed(why) => say(scene, fonts, why),
            Body::Text(b) => {
                let px = (t::BODY_SIZE_S as f64 * s * self.zoom) as f32; let adv = (fonts.layout("0000000000", Family::Mono, px, t::FONT_WEIGHT_REGULAR as f32, ink).width() as f64 / 10.0).max(1.0); let lh = (px as f64 * 1.5).round();
                let cols = (((area.width() - 2.0 * margin - 10.0 * s) / adv).floor() as usize).max(8);
                if b.cols != cols { b.rows = wrap(&b.lines, cols); b.cols = cols; }
                if b.lines.is_empty() { say(scene, fonts, "This file is empty."); }
                self.bar.set(area, b.rows.len() as f64 * lh + 2.0 * margin, s);
                let first = (((self.bar.offset - margin) / lh).floor().max(0.0)) as usize; let n = (area.height() / lh).ceil() as usize + 1;
                for (k, (line, from, to)) in b.rows.iter().enumerate().skip(first).take(n) {
                    let text = &b.lines[*line as usize][*from as usize..*to as usize]; if text.is_empty() { continue }
                    let l = fonts.layout(text, Family::Mono, px, t::FONT_WEIGHT_REGULAR as f32, ink);
                    draw_layout(scene, &l, Affine::translate((area.x0 + margin, area.y0 + margin + k as f64 * lh - self.bar.offset)));
                }
            }
            Body::Pdf(b) => {
                let gap = 14.0 * s; let width = ((area.width() - 2.0 * margin).min(1200.0 * s) * self.zoom).max(40.0);
                let over = (width + 2.0 * margin - area.width()).max(0.0) / 2.0; self.pan_x = self.pan_x.clamp(-over, over);
                let x0 = area.center().x - width / 2.0 - self.pan_x;
                let heights: Vec<f64> = b.sizes.iter().map(|(w, h)| if *w > 0.0 { width * *h as f64 / *w as f64 } else { width * 1.414 }).collect();
                self.bar.set(area, heights.iter().sum::<f64>() + gap * heights.len().saturating_sub(1) as f64 + 2.0 * margin, s);
                // (a page's picture is at most 4096 px on its longer side - memory, and the renderer's own limit for a picture; beyond that it is enlarged)
                let want = |i: usize| -> u32 { let h = heights[i]; let k = (4096.0 / width.max(h)).min(1.0); ((width * k).round() as u32).max(40) };
                let mut wanted = vec![]; let mut y = area.y0 + margin - self.bar.offset; let mut at = None; let mut last_seen = 0;
                for (i, h) in heights.iter().enumerate() {
                    let r = Rect::new(x0, y, x0 + width, y + h); y += h + gap;
                    if r.y1 < area.y0 - area.height() { continue } if r.y0 > area.y1 + area.height() { break }
                    let seen = r.y1 >= area.y0 && r.y0 <= area.y1; if seen { last_seen = i; if at.is_none() && r.y1 > area.y0 + area.height() * 0.35 { at = Some(i); } }
                    match b.pages.get(&i) {
                        Some((w, img)) => { if seen { paint::image(scene, img, r); } if *w != want(i) && settled && !b.failed.contains(&i) { wanted.push((i, want(i))); } }
                        None => { if seen { paint::rect(scene, r, if b.failed.contains(&i) { t::GRAY_300 } else { t::GRAY_800 }); if b.failed.contains(&i) { let l = fonts.layout("This page could not be drawn.", Family::Sans, t::BODY_SIZE_S * s as f32, t::FONT_WEIGHT_REGULAR as f32, sub); draw_layout(scene, &l, Affine::translate((r.center().x - l.width() as f64 / 2.0, r.y0.max(area.y0) + 40.0 * s))); } }
                            if !b.failed.contains(&i) { wanted.push((i, want(i))); } }
                    }
                }
                b.at = at.unwrap_or(last_seen);
                // pages far from the view are let go (a long document must not keep every page's picture)
                let here = b.at; b.pages.retain(|i, _| i.abs_diff(here) <= 4);
                let any = !wanted.is_empty(); *self.wanted.lock().unwrap() = wanted; if any { if let Some(k) = &self.kick { let _ = k.send(()); } }
            }
        }
        self.bar.paint(scene, now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn kinds_by_extension() { assert_eq!(kind_of("pdf"), Some(Kind::Pdf)); assert_eq!(kind_of("md"), Some(Kind::Text)); assert_eq!(kind_of("json"), Some(Kind::Text)); assert_eq!(kind_of("docx"), None); assert_eq!(kind_of("exr"), None); }
    #[test]
    fn text_is_decoded_cut_and_refused_when_it_is_not_text() {
        assert_eq!(decode(b"one\ntwo".to_vec()), Some(("one\ntwo".into(), false)));
        assert_eq!(decode([&[0xEF, 0xBB, 0xBF][..], b"marked"].concat()), Some(("marked".into(), false)), "the UTF-8 mark is not text");
        let le: Vec<u8> = [0xFF, 0xFE].into_iter().chain("h\u{e9}".encode_utf16().flat_map(|u| u.to_le_bytes())).collect(); assert_eq!(decode(le), Some(("h\u{e9}".into(), false)));
        assert_eq!(decode(vec![b'a', 0, b'b']), None, "a file with zero bytes is not text");
        assert_eq!(decode(vec![b'a', 0xFF, b'b']).unwrap().0, "a\u{fffd}b", "what is not UTF-8 is marked, the rest shown");
        let long = format!("{}\nlast line that is cut", "x".repeat(TEXT_MAX - 5)); let (text, cut) = decode(long.into_bytes()).unwrap(); assert!(cut); assert_eq!(text.len(), TEXT_MAX - 5, "cut at a whole line");
    }
    #[test]
    fn lines_are_wrapped_by_characters() {
        let lines = lines_of("short\n\n\tindented\nabcdefghijklmnopqrstuvwxyz\n\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}");
        assert_eq!(lines[2], "    indented");
        let rows = wrap(&lines, 10); let text = |r: &(u32, u32, u32)| lines[r.0 as usize][r.1 as usize..r.2 as usize].to_string();
        assert_eq!(rows.iter().map(text).collect::<Vec<_>>(), ["short", "", "    indent", "ed", "abcdefghij", "klmnopqrst", "uvwxyz", "\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}\u{e9}"]);
    }
    #[test]
    fn a_pdf_gives_its_pages_and_a_text_its_lines() {
        let dir = std::env::temp_dir().join(format!("ufb-doc-{}", std::process::id())); std::fs::create_dir_all(&dir).unwrap();
        let wait = |d: &mut Doc| { let t0 = Instant::now(); while !d.poll() && t0.elapsed() < Duration::from_secs(20) { std::thread::sleep(Duration::from_millis(5)); } };
        std::fs::write(dir.join("a.txt"), "one\ntwo\nthree").unwrap(); let mut d = Doc::open(Kind::Text, &dir.join("a.txt"), Arc::new(|| {})); wait(&mut d);
        assert_eq!(d.facts(), ["3 lines"]);
        std::fs::write(dir.join("a.pdf"), min_pdf::sample()).unwrap(); let mut d = Doc::open(Kind::Pdf, &dir.join("a.pdf"), Arc::new(|| {})); wait(&mut d);
        let Body::Pdf(b) = &d.body else { panic!("the sample PDF opens") }; let n = b.sizes.len(); assert!(n >= 1);
        // a page asked for comes, at the width asked
        *d.wanted.lock().unwrap() = vec![(0, 200)]; d.kick.as_ref().unwrap().send(()).unwrap(); wait(&mut d);
        let Body::Pdf(b) = &d.body else { unreachable!() }; assert_eq!(b.pages.get(&0).map(|p| (p.0, p.1.image.width)), Some((200, 200)));
        std::fs::write(dir.join("bad.pdf"), b"not a pdf").unwrap(); let mut d = Doc::open(Kind::Pdf, &dir.join("bad.pdf"), Arc::new(|| {})); wait(&mut d); assert!(matches!(d.body, Body::Failed(_)));
        let mut d = Doc::open(Kind::Text, &dir.join("missing.txt"), Arc::new(|| {})); wait(&mut d); assert!(matches!(d.body, Body::Failed(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
