//! PDF pages for the PDF block (canon row 315; spike S10): the ONE place that knows the engine. It answers four
//! things - how many pages and how large, a page drawn at a width, a page's characters with their places - so another
//! engine can take its place without the block changing. The engine is `hayro` (pure Rust).
//!
//! Everything here runs on a helper thread. A file's bytes are kept for the last few files asked for (turning pages
//! of a large file on a share must not read it again each time). The engine is young: every call into it is fenced,
//! and a file it cannot take is said in words.
use hayro::hayro_interpret::TransformExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// A character of a page: what it is and where (points from the page's top-left corner, y down).
#[derive(Clone, Debug, PartialEq)]
pub struct Char { pub text: String, pub x: f32, pub y: f32, pub w: f32, pub h: f32 }
/// A page's characters in the order the page draws them, and the page's size in points.
#[derive(Debug, Default)]
pub struct PageText { pub chars: Vec<Char>, pub w: f32, #[allow(dead_code)] pub h: f32 }

type Kept = (PathBuf, u64, Arc<Vec<u8>>);
static FILES: Mutex<Vec<Kept>> = Mutex::new(Vec::new());
/// The most bytes of files kept between calls.
const KEEP: usize = 512 << 20;

fn bytes(path: &Path) -> Result<Arc<Vec<u8>>, String> {
    let m = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let stamp = m.len() ^ m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0).rotate_left(20);
    if let Some(k) = FILES.lock().unwrap().iter().find(|k| k.0 == path && k.1 == stamp) { return Ok(k.2.clone()) }
    let data = Arc::new(std::fs::read(path).map_err(|e| e.to_string())?);
    let mut kept = FILES.lock().unwrap(); kept.retain(|k| k.0 != path); kept.insert(0, (path.to_path_buf(), stamp, data.clone()));
    let mut total = 0usize; kept.retain(|k| { total += k.2.len(); total <= KEEP || std::ptr::eq(k.2.as_ref(), data.as_ref()) });
    Ok(data)
}
fn fenced<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> { std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_| Err("this file's content stopped the PDF reader".into())) }
fn open(path: &Path) -> Result<hayro::hayro_syntax::Pdf, String> { hayro::hayro_syntax::Pdf::new(bytes(path)?).map_err(|e| format!("not a PDF the reader takes ({e:?})")) }

/// The number of pages, and the first page's size in points (as it is seen: its rotation applied).
pub fn info(path: &Path) -> Result<(usize, f32, f32), String> {
    fenced(|| { let pdf = open(path)?; let pages = pdf.pages(); let first = pages.iter().next().ok_or("the PDF has no pages")?; let (w, h) = first.render_dimensions(); Ok((pages.len(), w, h)) })
}
/// A page as opaque RGBA (on white), `width` pixels wide (its height follows); `page` past the end is the last.
pub fn page(path: &Path, page: usize, width: u32) -> Result<(Vec<u8>, u32, u32), String> {
    fenced(|| {
        let pdf = open(path)?; let pages = pdf.pages(); if pages.len() == 0 { return Err("the PDF has no pages".into()) }
        let p = &pages[page.min(pages.len() - 1)]; let (pw, ph) = p.render_dimensions(); if pw <= 0.0 || ph <= 0.0 { return Err("the page has no size".into()) }
        // the engine's picture has 16-bit sides
        let scale = (width.max(1) as f32 / pw).min(65000.0 / pw.max(ph));
        let cache = hayro::RenderCache::new();
        let pm = hayro::render(p, &cache, &Default::default(), &Default::default(), &hayro::PixmapSettings { x_scale: scale, y_scale: scale, bg_color: hayro::vello_cpu::color::palette::css::WHITE });
        let (w, h) = (pm.width() as u32, pm.height() as u32); if w == 0 || h == 0 { return Err("the page came out empty".into()) }
        let mut rgba = Vec::with_capacity((w * h * 4) as usize); for c in pm.data() { rgba.extend_from_slice(&[c.r, c.g, c.b, 255]); }
        Ok((rgba, w, h))
    })
}

/// Keeps every glyph the page shows: its character and its place.
struct Collect { chars: Vec<Char> }
impl<'a> hayro::hayro_interpret::Device<'a> for Collect {
    fn draw_path(&mut self, _: &hayro::kurbo::BezPath, _: hayro::hayro_interpret::DrawProps<'a>, _: &hayro::hayro_interpret::DrawMode) {}
    fn push_clip_path(&mut self, _: &hayro::hayro_interpret::ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<hayro::hayro_interpret::SoftMask<'a>>, _: hayro::hayro_interpret::BlendMode) {}
    fn draw_glyph_run(&mut self, run: &hayro::hayro_interpret::font::GlyphRun<'_, 'a>, props: hayro::hayro_interpret::DrawProps<'a>, _: &hayro::hayro_interpret::DrawMode) {
        use hayro::hayro_interpret::hayro_cmap::BfString;
        let first = self.chars.len();
        for g in run.glyphs() {
            let Some(u) = g.as_unicode() else { continue };
            let t = props.transform * g.transform(); let c = t.as_coeffs();
            // a glyph is drawn in thousandths of its size: the up vector's length is the size (the engine's own rule)
            let em = ((c[2] * c[2] + c[3] * c[3]).sqrt() * 1000.0) as f32; if !(em.is_finite() && em > 0.1) { continue }
            let text = match u { BfString::Char(ch) => ch.to_string(), BfString::String(s) => s };
            if text.is_empty() || text.chars().all(|ch| ch.is_control()) { continue }
            self.chars.push(Char { text, x: c[4] as f32, y: c[5] as f32 - em * 0.8, w: em * 0.5, h: em });
        }
        // a character is as wide as the way to the next one of its run (the last keeps half its size)
        for i in first..self.chars.len().saturating_sub(1) { let (a, b) = (&self.chars[i], &self.chars[i + 1]); let d = b.x - a.x; if (b.y - a.y).abs() < a.h * 0.3 && d > 0.0 && d < a.h * 2.5 { self.chars[i].w = d; } }
    }
    fn draw_image(&mut self, _: hayro::hayro_interpret::Image<'a, '_>, _: hayro::hayro_interpret::ImageDrawProps<'a>) {}
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}
}
/// A page's characters with their places.
pub fn text(path: &Path, page: usize) -> Result<PageText, String> {
    fenced(|| {
        let pdf = open(path)?; let pages = pdf.pages(); if pages.len() == 0 { return Err("the PDF has no pages".into()) }
        let p = &pages[page.min(pages.len() - 1)]; let (w, h) = p.render_dimensions();
        let cache = hayro::hayro_interpret::InterpreterCache::new();
        let mut ctx = hayro::hayro_interpret::Context::new(p.initial_transform(true).to_kurbo(), hayro::kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), &cache, p.xref(), Default::default());
        let mut c = Collect { chars: vec![] };
        hayro::hayro_interpret::interpret_page(p, &mut ctx, &mut c);
        Ok(PageText { chars: c.chars, w, h })
    })
}

impl PageText {
    /// The character at a point of the page (points), else the nearest on the nearest line.
    pub fn at(&self, x: f32, y: f32) -> Option<usize> {
        if let Some(i) = self.chars.iter().position(|c| x >= c.x && x <= c.x + c.w && y >= c.y && y <= c.y + c.h) { return Some(i) }
        // how far above or below counts four times how far along: a point beside a line picks that line
        let d = |c: &Char| { let dx = if x < c.x { c.x - x } else if x > c.x + c.w { x - c.x - c.w } else { 0.0 }; let dy = if y < c.y { c.y - y } else if y > c.y + c.h { y - c.y - c.h } else { 0.0 }; dx + dy * 4.0 };
        (0..self.chars.len()).min_by(|a, b| d(&self.chars[*a]).total_cmp(&d(&self.chars[*b])))
    }
    /// The text of characters `a..=b` as words and lines: a gap along a line is a space, a new line a line end.
    pub fn words(&self, a: usize, b: usize) -> String {
        let (a, b) = (a.min(b), a.max(b).min(self.chars.len().saturating_sub(1))); let mut out = String::new();
        for i in a..=b { let Some(c) = self.chars.get(i) else { break };
            if i > a { let p = &self.chars[i - 1];
                if (c.y - p.y).abs() > p.h * 0.5 || c.x + c.w < p.x - p.h { if !out.ends_with('\n') { while out.ends_with(' ') { out.pop(); } out.push('\n'); } }
                else if c.x - (p.x + p.w) > p.h * 0.2 && !out.ends_with(' ') && !c.text.starts_with(' ') { out.push(' '); } }
            out.push_str(&c.text);
        }
        out.trim().to_string()
    }
    /// The rectangles (points) that cover characters `a..=b`, one per stretch of a line.
    pub fn rects(&self, a: usize, b: usize) -> Vec<(f32, f32, f32, f32)> {
        let (a, b) = (a.min(b), a.max(b).min(self.chars.len().saturating_sub(1))); let mut out: Vec<(f32, f32, f32, f32)> = vec![];
        for c in self.chars.get(a..=b).unwrap_or(&[]) {
            match out.last_mut() { Some(r) if (c.y - r.1).abs() < c.h * 0.5 && c.x >= r.0 - 1.0 && c.x - r.2 < c.h * 1.5 => { r.2 = r.2.max(c.x + c.w); r.3 = r.3.max(c.y + c.h); r.1 = r.1.min(c.y); } _ => out.push((c.x, c.y, c.x + c.w, c.y + c.h)) }
        }
        out
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    /// A one-page PDF, 200 x 100 points, with two lines of Helvetica (a font every reader has) and a red square.
    pub(crate) fn sample() -> Vec<u8> {
        let content = "1 0 0 rg 150 10 40 40 re f 0 g BT /F1 12 Tf 20 70 Td (Hello page) Tj 0 -20 Td (Second line) Tj ET";
        let objs = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(), "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()), "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_string() ];
        let mut out = b"%PDF-1.4\n".to_vec(); let mut at = vec![];
        for (i, o) in objs.iter().enumerate() { at.push(out.len()); out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes()); }
        let xref = out.len(); out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
        for a in at { out.extend_from_slice(format!("{a:010} 00000 n \n").as_bytes()); }
        out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes()); out
    }
    #[test]
    fn a_page_is_measured_drawn_and_read_with_its_characters_places() {
        let dir = std::env::temp_dir().join(format!("mn-pdf-test-{}", std::process::id())); std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("one.pdf"); std::fs::write(&p, sample()).unwrap();
        assert_eq!(info(&p), Ok((1, 200.0, 100.0)));
        let (rgba, w, h) = page(&p, 0, 400).unwrap(); assert_eq!((w, h), (400, 200));
        // the red square is at the page's lower right (the page's y runs up, the picture's down); the paper is white
        let px = |x: usize, y: usize| &rgba[(y * 400 + x) * 4..][..4]; assert_eq!(px(340, 140), [255, 0, 0, 255]); assert_eq!(px(5, 5), [255, 255, 255, 255]);
        let t = text(&p, 0).unwrap(); assert_eq!((t.w, t.h), (200.0, 100.0));
        assert_eq!(t.words(0, t.chars.len() - 1), "Hello page\nSecond line");
        // the first line's baseline is 70 points up from the bottom: 30 down from the top
        let hch = &t.chars[0]; assert_eq!(hch.text, "H"); assert!((hch.x - 20.0).abs() < 0.5 && hch.y < 30.0 && hch.y + hch.h > 28.0, "{hch:?}");
        assert_eq!(t.at(21.0, 25.0), Some(0)); let second = t.at(22.0, 48.0).unwrap(); assert_eq!(t.chars[second].text, "S");
        assert_eq!(t.words(second, second + 5), "Second"); assert_eq!(t.rects(0, t.chars.len() - 1).len(), 2);
        assert!(info(&dir.join("gone.pdf")).is_err()); std::fs::write(&p, b"%PDF-1.4 nothing").unwrap(); assert!(page(&p, 0, 100).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
