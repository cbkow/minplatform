//! Spike S10 (docs/spikes/S10-pdf-engine.md): pdfium against hayro on the user's own PDF and Illustrator files.
//! `s10-pdf <folder with libpdfium> <out folder> <width px> <file>...` - for each file and engine: the time to open,
//! the pages, the time to render the first and a middle page at the width, the PNGs written, and (pdfium) the
//! characters of the first page that come with a place. Files are named by their order only: nothing of them is printed.
use std::time::Instant;

fn png(path: &std::path::Path, rgba: &[u8], w: u32, h: u32) {
    let mut e = png::Encoder::new(std::io::BufWriter::new(std::fs::File::create(path).unwrap()), w, h);
    e.set_color(png::ColorType::Rgba); e.set_depth(png::BitDepth::Eight); e.set_compression(png::Compression::Fast);
    e.write_header().unwrap().write_image_data(rgba).unwrap();
}
/// The characters a page's glyphs stand for, with the place of each: what text selection needs of an engine. hayro
/// gives no ready call for it; its interpreter hands every glyph run to a `Device`, and this one keeps them.
#[allow(dead_code)]
struct Chars { with_place: usize, without: usize, sample: String, min_y: f64, max_y: f64 }
impl<'a> hayro::hayro_interpret::Device<'a> for Chars {
    fn draw_path(&mut self, _: &hayro::kurbo::BezPath, _: hayro::hayro_interpret::DrawProps<'a>, _: &hayro::hayro_interpret::DrawMode) {}
    fn push_clip_path(&mut self, _: &hayro::hayro_interpret::ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<hayro::hayro_interpret::SoftMask<'a>>, _: hayro::hayro_interpret::BlendMode) {}
    fn draw_glyph_run(&mut self, run: &hayro::hayro_interpret::font::GlyphRun<'_, 'a>, props: hayro::hayro_interpret::DrawProps<'a>, _: &hayro::hayro_interpret::DrawMode) {
        for g in run.glyphs() { match g.as_unicode() {
            Some(u) => { self.with_place += 1; let at = (props.transform * g.transform()).translation(); self.min_y = self.min_y.min(at.y); self.max_y = self.max_y.max(at.y);
                if self.sample.len() < 40 { match u { hayro::hayro_interpret::hayro_cmap::BfString::Char(c) => self.sample.push(c), hayro::hayro_interpret::hayro_cmap::BfString::String(x) => self.sample.push_str(&x) } } }
            None => self.without += 1 } }
    }
    fn draw_image(&mut self, _: hayro::hayro_interpret::Image<'a, '_>, _: hayro::hayro_interpret::ImageDrawProps<'a>) {}
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}
}
fn ms(t: Instant) -> f64 { t.elapsed().as_secs_f64() * 1000.0 }

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (lib, out, width) = (&a[1], std::path::PathBuf::from(&a[2]), a[3].parse::<u32>().unwrap());
    std::fs::create_dir_all(&out).unwrap();
    use pdfium_render::prelude::*;
    let pdfium = Pdfium::new(Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(lib)).expect("libpdfium"));
    println!("file\tMB\tengine\topen ms\tpages\tfirst ms\tmiddle ms\tchars with a place\tnote");
    let mut kept: Vec<(Vec<u8>, usize, usize)> = vec![];
    for (i, file) in a[4..].iter().enumerate() {
        let n = i + 1; let mb = std::fs::metadata(file).map(|m| m.len() as f64 / 1e6).unwrap_or(0.0);
        // pdfium
        let t = Instant::now();
        match pdfium.load_pdf_from_file(file, None) {
            Err(e) => println!("{n}\t{mb:.1}\tpdfium\t-\t-\t-\t-\t-\t{e:?}"),
            Ok(doc) => { let open = ms(t); let pages = doc.pages().len() as usize; let mut times = vec![]; let mut chars = 0usize; kept.clear();
                for (k, p) in [0usize, pages / 2].into_iter().enumerate() { if k == 1 && p == 0 { times.push(f64::NAN); continue }
                    let page = doc.pages().get(p as _).unwrap(); let t = Instant::now();
                    let bmp = page.render_with_config(&PdfRenderConfig::new().set_target_width(width as _)).unwrap(); let rgba = bmp.as_rgba_bytes(); times.push(ms(t));
                    png(&out.join(format!("{n:02}-p{p}-pdfium.png")), &rgba, bmp.width() as u32, bmp.height() as u32); kept.push((rgba.to_vec(), bmp.width() as usize, bmp.height() as usize));
                    if k == 0 { if let Ok(text) = page.text() { chars = text.chars().iter().filter(|c| c.loose_bounds().is_ok_and(|b| b.width().value > 0.0)).count(); } } }
                println!("{n}\t{mb:.1}\tpdfium\t{open:.0}\t{pages}\t{:.0}\t{:.0}\t{chars}\t", times[0], times[1]); }
        }
        // hayro
        let t = Instant::now();
        let kept_ref = &kept;
        let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let data = std::fs::read(file).unwrap();
            let pdf = match hayro::hayro_syntax::Pdf::new(data) { Ok(p) => p, Err(e) => return Err(format!("{e:?}")) };
            let open = ms(t); let pages = pdf.pages().len(); let mut times = vec![]; let mut diffs: Vec<f64> = vec![];
            for (k, p) in [0usize, pages / 2].into_iter().enumerate() { if k == 1 && p == 0 { times.push(f64::NAN); continue }
                let (pw, _) = pdf.pages()[p].render_dimensions(); let scale = width as f32 / pw; let t = Instant::now();
                let pm = hayro::render_pdf(&pdf, scale, Default::default(), &Default::default(), Some(p..=p)).and_then(|mut v| v.pop());
                times.push(ms(t));
                // how far the two engines' pictures are apart: the mean difference per colour value (0..255), and the share of pixels more than 24 apart
                if let (Some(pm), Some((ref_rgba, rw, rh))) = (pm.as_ref(), kept_ref.get(diffs.len())) { let (w, h) = ((pm.width() as usize).min(*rw), (pm.height() as usize).min(*rh)); let d = pm.data(); let (mut sum, mut far) = (0u64, 0u64);
                    for y in 0..h { for x in 0..w { let a = &ref_rgba[(y * rw + x) * 4..]; let b = d[y * pm.width() as usize + x]; let m = (a[0] as i32 - b.r as i32).abs().max((a[1] as i32 - b.g as i32).abs()).max((a[2] as i32 - b.b as i32).abs()); sum += m as u64; if m > 24 { far += 1; } } }
                    diffs.push(sum as f64 / (w * h) as f64); diffs.push(far as f64 * 100.0 / (w * h) as f64); }
                if let Some(pm) = pm { if let Ok(bytes) = pm.into_png() { let _ = std::fs::write(out.join(format!("{n:02}-p{p}-hayro.png")), bytes); } } }
            // the first page's characters with their places, by a device of our own
            let page = &pdf.pages()[0]; let cache = hayro::hayro_interpret::InterpreterCache::new();
            let mut ctx = hayro::hayro_interpret::Context::new(hayro::kurbo::Affine::IDENTITY, hayro::kurbo::Rect::new(0.0, 0.0, 1e5, 1e5), &cache, page.xref(), Default::default());
            let mut chars = Chars { with_place: 0, without: 0, sample: String::new(), min_y: f64::MAX, max_y: f64::MIN };
            hayro::hayro_interpret::interpret_page(page, &mut ctx, &mut chars);
            Ok((open, pages, times, chars.with_place, chars.without, diffs))
        }));
        match res { Ok(Ok((open, pages, times, with, without, diffs))) => println!("{n}\t{mb:.1}\thayro\t{open:.0}\t{pages}\t{:.0}\t{:.0}\t{with}\t{without} glyphs without a character; against pdfium (mean difference, % of pixels far apart) per page: {}", times[0], times[1], diffs.iter().map(|d| format!("{d:.2}")).collect::<Vec<_>>().join(" ")), Ok(Err(e)) => println!("{n}\t{mb:.1}\thayro\t-\t-\t-\t-\t-\t{e}"), Err(_) => println!("{n}\t{mb:.1}\thayro\t-\t-\t-\t-\t-\tpanicked") }
    }
}
