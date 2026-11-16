//! The helper: this executable started with `--thumb-helper` (canon row 456; spike S18). It reads one request, does
//! the work, writes one answer, for ever; it ends when its standard input closes (the application is gone or wants it
//! gone). It has no window, no event loop and no run loop. Everything that reads a file's CONTENTS happens here: a
//! damaged file can end a helper, never the application.
use super::cache::{self, Cache, Kept};
use super::proto::*;
use super::sys;
use std::fs::File;
use std::mem::ManuallyDrop;
use std::path::Path;
use std::time::{Duration, Instant};

/// RGBA, 8 bits, colour multiplied by its alpha, sRGB, top row first.
pub struct Img { pub w: u32, pub h: u32, pub rgba: Vec<u8> }

fn std_files() -> (ManuallyDrop<File>, ManuallyDrop<File>) {
    #[cfg(unix)] unsafe { use std::os::fd::FromRawFd; (ManuallyDrop::new(File::from_raw_fd(0)), ManuallyDrop::new(File::from_raw_fd(1))) }
    #[cfg(windows)] unsafe { use std::os::windows::io::{AsRawHandle, FromRawHandle};
        (ManuallyDrop::new(File::from_raw_handle(std::io::stdin().as_raw_handle())), ManuallyDrop::new(File::from_raw_handle(std::io::stdout().as_raw_handle()))) }
}

/// A crash ends the helper AT ONCE - no crash reporter, no report written per damaged file (S18): the application
/// sees the pipe close.
#[cfg(unix)]
fn crash_handlers() {
    extern "C" fn die(sig: i32) { unsafe { libc::_exit(128 + sig) } }
    for s in [libc::SIGABRT, libc::SIGSEGV, libc::SIGBUS, libc::SIGILL, libc::SIGFPE, libc::SIGTRAP] { unsafe { libc::signal(s, die as extern "C" fn(i32) as usize); } }
}
#[cfg(windows)]
fn crash_handlers() {
    // no error box, no Windows Error Reporting dialog: the process just ends
    use windows::Win32::System::Diagnostics::Debug::*;
    unsafe { SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX); }
}
#[cfg(not(any(unix, windows)))]
fn crash_handlers() {}

/// If the application is killed while this helper is busy (or hung), nothing closes a pipe the helper is not reading.
/// Unix: the parent's id changes when it dies. (Windows: the application puts its helpers in a job that dies with it.)
fn watch_parent() {
    #[cfg(unix)] { let parent = unsafe { libc::getppid() };
        std::thread::spawn(move || loop { std::thread::sleep(Duration::from_millis(500)); if unsafe { libc::getppid() } != parent { std::process::exit(0) } }); }
}

pub fn run() -> ! {
    crash_handlers();
    // a panic of ours is a crash like any other: no message, the process ends
    std::panic::set_hook(Box::new(|_| std::process::abort()));
    watch_parent();
    sys::init();
    let mut ctx = Ctx { test: std::env::var_os("UFB_HELPER_TEST").is_some(), cache: None, poster: None };
    let (mut inp, mut out) = std_files();
    // the hello: the application bounds a start from the spawn to this frame
    if write_response(&mut *out, &Response::default()).is_err() { std::process::exit(0) }
    loop {
        let Ok(req) = read_request(&mut *inp) else { std::process::exit(0) };
        let resp = handle(&req, &mut ctx);
        if write_response(&mut *out, &resp).is_err() { std::process::exit(0) }
    }
}

/// What a helper holds between requests: the cache on disk, opened at the first request that needs it (the folder
/// comes from the application, in `UFB_THUMB_CACHE`; without it nothing is kept).
struct Ctx { test: bool, cache: Option<Option<Cache>>, /// the colour stage of video frames (an engine on the GPU, without a window), made at the first video
    poster: Option<qcv_player::poster::Poster> }
impl Ctx {
    fn cache(&mut self) -> Option<&mut Cache> {
        self.cache.get_or_insert_with(|| std::env::var_os("UFB_THUMB_CACHE").filter(|d| !d.is_empty()).and_then(|d| Cache::open(Path::new(&d)))).as_mut()
    }
}

fn handle(req: &Request, ctx: &mut Ctx) -> Response {
    let t = Instant::now(); let test = ctx.test;
    let mut r = Response { id: req.id, ..Default::default() };
    let soft = Duration::from_millis(req.soft_ms.max(1) as u64);
    let (path, px) = (Path::new(&req.path), req.size as u32);
    // a picture to be kept is made at the cache's size, kept, and answered at the size asked
    let keep = req.flags & KEEP != 0 && matches!(req.op, OP_DECODE | OP_VIDEO | OP_THUMB) && ctx.cache().is_some();
    let make = if keep { cache::MASTER.max(px) } else { px };
    let res: Result<Img, (u8, i64)> = match req.op {
        OP_PING => return r,
        OP_DECODE => decode(&req.path, make).map_err(|_| (FAILED, 0)),
        OP_VIDEO => video_frame(&req.path, make, ctx.poster.get_or_insert_with(qcv_player::poster::Poster::headless)).map_err(|_| (FAILED, 0)),
        OP_THUMB => sys::thumbnail(&req.path, make, soft),
        OP_ICON => sys::icon(&req.path, px, soft),
        OP_CACHED => match ctx.cache().map(|c| c.lookup(path, req.fsize, req.mtime)) { Some(Kept::Hit(img)) => Ok(img), Some(Kept::NoPicture) => Err((NONE, 0)), _ => Err((MISS, 0)) },
        OP_MARK => { if let Some(c) = ctx.cache() { c.mark(path, req.fsize, req.mtime); } return r }
        OP_PRUNE => { if let Some(c) = ctx.cache() { r.aux = c.prune(path) as u32; } return r }
        OP_FORGET => { if let Some(c) = ctx.cache() { c.forget(path); } return r }
        OP_TEST_ABORT if test => std::process::abort(),
        OP_TEST_HANG if test => loop { std::thread::sleep(Duration::from_secs(3600)); },
        _ => Err((FAILED, 0)),
    };
    let res = res.map(|img| {
        let img = if matches!(req.op, OP_DECODE | OP_VIDEO | OP_THUMB) { fitted(img, make) } else { img };
        if keep && img.w.max(img.h) <= cache::MASTER { if let Some(c) = ctx.cache() { c.keep(path, req.fsize, req.mtime, &img); } }
        fitted(img, px)
    });
    match res {
        Ok(img) if img.w > 0 && img.h > 0 && img.w <= u16::MAX as u32 && img.h <= u16::MAX as u32 && img.rgba.len() == (img.w * img.h * 4) as usize && img.rgba.len() <= MAX_PAYLOAD => { r.w = img.w as u16; r.h = img.h as u16; r.payload = img.rgba; }
        Ok(_) => r.status = FAILED,
        Err((status, code)) => { r.status = status; r.aux = code as u32; }
    }
    r.work_us = t.elapsed().as_micros().min(u32::MAX as u128) as u32;
    r
}

/// The types our own decoders make the picture of (canon row 456, point 1: a frame looks the same here as in the
/// family's other applications). Everything else is the system's to draw.
pub fn ours(ext: &str) -> bool { matches!(ext, "exr" | "dpx" | "tif" | "tiff" | "psd" | "psb" | "tga" | "hdr") }
/// The files the family's player reads a frame of (QCView's list of videos, as minNotes has it: without GIF and WebP,
/// which are pictures).
pub const VIDEO: &[&str] = &["mov", "mp4", "m4v", "mxf", "mkv", "avi", "webm", "ts", "mts", "m2ts", "m2t", "mpg", "mpeg", "mpe", "m2v", "vob", "3gp", "3g2", "wmv", "asf", "flv", "f4v", "ogv", "ogm", "mjpeg", "mjpg", "y4m", "dv", "mj2"];
/// Who makes a file's picture first: our decoders, the player, or the system ([`OP_DECODE`], [`OP_VIDEO`], [`OP_THUMB`]).
pub fn maker(ext: &str) -> u8 { if ours(ext) { OP_DECODE } else if VIDEO.contains(&ext) { OP_VIDEO } else { OP_THUMB } }

/// A video's picture: the frame in the MIDDLE of the clip (a sequence's rule, row 456 - the first frames are so often
/// black or a slate), decoded by the family's player, its colour by the presenter's pipeline (an HDR clip is brought
/// to SDR as QCView's thumbnails are), turned as the container says. Opaque.
fn video_frame(path: &str, px: u32, poster: &mut qcv_player::poster::Poster) -> Result<Img, String> {
    use qcv_player::thumbnail_cache::{ThumbLoader, VideoThumbLoader};
    let (frames, meta) = { let s = qcv_player::Session::open(path, &qcv_player::OpenOptions { threads: 1, hw: None, hw_intra: None, seq: None, fast: true, live: None }).map_err(|e| e.to_string())?;
        if s.info.width == 0 || s.info.height == 0 { return Err("the file has no picture".into()) } (s.info.frame_count.max(1), s.info.meta) };
    let mut l = VideoThumbLoader::open(path).ok_or("the decoder could not open the file")?;
    let px = px.clamp(16, 1024);
    let frame = l.load_thumbnail(frames / 2, px).or_else(|| l.load_thumbnail(0, px)).ok_or("no frame could be decoded")?;
    let (rgba, w, h) = poster.convert(None, &frame, &meta);
    if w == 0 || h == 0 || rgba.len() != (w as usize) * (h as usize) * 4 { return Err("an empty picture".into()) }
    // as the viewer shows the clip: its pixels' shape (an anamorphic or SD clip), then the container's quarter turns
    Ok(turned(widened(Img { w, h, rgba }, meta.sar), qcv_meta::probe_rotation(path).rem_euclid(360) as u16))
}
/// A picture whose pixels are not square, made of square ones: its width takes the pixels' shape (`sar` = width : height
/// of one pixel), each new pixel between its two neighbours.
pub fn widened(img: Img, sar: (u32, u32)) -> Img {
    let k = sar.0 as f64 / sar.1.max(1) as f64; if sar.0 == 0 || (k - 1.0).abs() < 0.01 || !(0.25..=4.0).contains(&k) { return img }
    let (w, h) = (img.w as usize, img.h as usize); let tw = ((w as f64 * k).round() as usize).max(1); let src = &img.rgba;
    let mut out = vec![0u8; tw * h * 4];
    for x in 0..tw {
        let f = ((x as f64 + 0.5) / k - 0.5).clamp(0.0, (w - 1) as f64); let (a, t) = (f.floor() as usize, f.fract()); let b = (a + 1).min(w - 1);
        for y in 0..h { for c in 0..4 { out[(y * tw + x) * 4 + c] = (src[(y * w + a) * 4 + c] as f64 * (1.0 - t) + src[(y * w + b) * 4 + c] as f64 * t + 0.5) as u8; } }
    }
    Img { w: tw as u32, h: img.h, rgba: out }
}
/// A picture turned clockwise by a quarter-turn count of degrees (90, 180, 270).
pub fn turned(img: Img, degrees: u16) -> Img {
    let (w, h) = (img.w as usize, img.h as usize); let src = &img.rgba;
    let (tw, th) = match degrees % 360 { 90 | 270 => (h, w), 180 => (w, h), _ => return img };
    let mut out = vec![0u8; tw * th * 4];
    for y in 0..th { for x in 0..tw {
        let (sx, sy) = match degrees % 360 { 90 => (y, h - 1 - x), 180 => (w - 1 - x, h - 1 - y), _ => (w - 1 - y, x) };
        out[(y * tw + x) * 4..(y * tw + x) * 4 + 4].copy_from_slice(&src[(sy * w + sx) * 4..(sy * w + sx) * 4 + 4]);
    } }
    Img { w: tw as u32, h: th as u32, rgba: out }
}
/// A Radiance picture (`.hdr`): linear light, shown as an EXR is - clipped at 1 and given the sRGB curve. Straight RGBA.
fn radiance(path: &str) -> Result<(Vec<u8>, u32, u32), String> {
    use image::ImageDecoder;
    let dec = image::codecs::hdr::HdrDecoder::new(std::io::BufReader::new(File::open(path).map_err(|e| e.to_string())?)).map_err(|e| e.to_string())?;
    let (w, h) = dec.dimensions(); if w == 0 || h == 0 || (w as u64) * (h as u64) > 200_000_000 { return Err("a picture of no usable size".into()) }
    let mut buf = vec![0u8; dec.total_bytes() as usize]; dec.read_image(&mut buf).map_err(|e| e.to_string())?;
    if buf.len() != (w as usize) * (h as usize) * 12 { return Err("not three floats a pixel".into()) }
    let out: Vec<u8> = buf.chunks_exact(12).flat_map(|p| { let f = |i: usize| srgb(f32::from_ne_bytes([p[i], p[i + 1], p[i + 2], p[i + 3]])); [f(0), f(4), f(8), 255] }).collect();
    Ok((out, w, h))
}

/// Our own decoders: the file -> a picture that fits `px`.
pub fn decode(path: &str, px: u32) -> Result<Img, String> {
    let px = px.clamp(16, 1024);
    let ext = Path::new(path).extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
    let (rgba, w, h) = match ext.as_str() {
        // the flattened picture, read row by row at the size asked; opaque
        "psd" | "psb" => { let f = min_image::psd::load(Path::new(path), px)?; (f.rgba, f.w, f.h) }
        "hdr" => radiance(path)?,
        _ => { let img = min_image::load_thumbnail(path, "", px).map_err(|e| e.to_string())?; if img.sentinel.is_some() { return Err("not a picture".into()) } rgba8(&img)? }
    };
    if w == 0 || h == 0 || rgba.len() != (w as usize) * (h as usize) * 4 { return Err("an empty picture".into()) }
    let (tw, th) = fit(w, h, px);
    let mut out = if (tw, th) == (w, h) { rgba } else { shrink(&rgba, w, h, tw, th) };
    // straight alpha -> colour multiplied by its alpha (what the views draw)
    for p in out.chunks_exact_mut(4) { let a = p[3] as u32; if a < 255 { for c in &mut p[..3] { *c = ((*c as u32 * a + 127) / 255) as u8; } } }
    Ok(Img { w: tw, h: th, rgba: out })
}

/// The size with the longer side at most `px` that keeps the picture's shape (never larger than the picture).
pub fn fit(w: u32, h: u32, px: u32) -> (u32, u32) {
    let k = (px as f64 / w.max(h) as f64).min(1.0);
    (((w as f64 * k).round() as u32).clamp(1, w), ((h as f64 * k).round() as u32).clamp(1, h))
}
fn half(b: u16) -> f32 {
    let (s, e, m) = ((b >> 15) & 1, (b >> 10) & 0x1f, (b & 0x3ff) as f32);
    let v = match e { 0 => m * 2f32.powi(-24), 31 => if m == 0.0 { f32::INFINITY } else { 0.0 }, _ => (1.0 + m / 1024.0) * 2f32.powi(e as i32 - 15) };
    if s == 1 { -v } else { v }
}
fn srgb(l: f32) -> u8 { let l = l.clamp(0.0, 1.0); let v = if l <= 0.003_130_8 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 }; (v * 255.0 + 0.5) as u8 }
/// The decoder's planes (G, B, R [, A]) as straight-alpha RGBA, 8 bits (as minNotes shows the same files): code
/// values as they are; linear light (EXR) divided by its alpha, then the sRGB curve.
fn rgba8(img: &min_image::Image) -> Result<(Vec<u8>, u32, u32), String> {
    let (w, h, p) = (img.width as usize, img.height as usize, img.pitch as usize);
    let bytes = img.sample.bytes(); let planes = if img.alpha { 4 } else { 3 };
    if img.planes.len() < planes || w * bytes > p || img.planes.iter().take(planes).any(|pl| pl.len() < h * p) { return Err("the decoder gave a picture that is not whole".into()) }
    let mut out = vec![255u8; w * h * 4];
    let a = img.alpha.then(|| &img.planes[3]);
    for y in 0..h {
        let (g, b, r) = (&img.planes[0][y * p..], &img.planes[1][y * p..], &img.planes[2][y * p..]);
        let row = &mut out[y * w * 4..(y + 1) * w * 4];
        match img.sample {
            min_image::Sample::U8 => for x in 0..w { row[x * 4] = r[x]; row[x * 4 + 1] = g[x]; row[x * 4 + 2] = b[x]; if let Some(a) = a { row[x * 4 + 3] = a[y * p + x]; } },
            min_image::Sample::U16 => for x in 0..w { row[x * 4] = r[x * 2 + 1]; row[x * 4 + 1] = g[x * 2 + 1]; row[x * 4 + 2] = b[x * 2 + 1]; if let Some(a) = a { row[x * 4 + 3] = a[y * p + x * 2 + 1]; } },
            min_image::Sample::F16 => for x in 0..w {
                let f = |pl: &[u8]| half(u16::from_le_bytes([pl[x * 2], pl[x * 2 + 1]]));
                let al = a.map(|a| half(u16::from_le_bytes([a[y * p + x * 2], a[y * p + x * 2 + 1]])).clamp(0.0, 1.0)).unwrap_or(1.0);
                let un = |v: f32| if al > 0.0 && al < 1.0 { v / al } else { v };
                row[x * 4] = srgb(un(f(r))); row[x * 4 + 1] = srgb(un(f(g))); row[x * 4 + 2] = srgb(un(f(b))); row[x * 4 + 3] = (al * 255.0 + 0.5) as u8;
            },
        }
    }
    Ok((out, w as u32, h as u32))
}
/// A picture whose colour is already multiplied by its alpha, made to fit `px` (never enlarged): each new pixel is
/// the plain average of those it covers.
pub fn fitted(img: Img, px: u32) -> Img {
    let (tw, th) = fit(img.w, img.h, px.max(1)); if (tw, th) == (img.w, img.h) { return img }
    let (w, h, tw, th) = (img.w as usize, img.h as usize, tw as usize, th as usize); let src = &img.rgba;
    let mut out = vec![0u8; tw * th * 4];
    let xs: Vec<(usize, usize)> = (0..tw).map(|x| { let a = x * w / tw; (a, ((x + 1) * w / tw).max(a + 1).min(w)) }).collect();
    for y in 0..th {
        let (y0, y1) = { let a = y * h / th; (a, ((y + 1) * h / th).max(a + 1).min(h)) };
        for (x, (x0, x1)) in xs.iter().enumerate() {
            let mut sum = [0u32; 4];
            for sy in y0..y1 { for p in src[(sy * w + x0) * 4..(sy * w + x1) * 4].chunks_exact(4) { for c in 0..4 { sum[c] += p[c] as u32; } } }
            let n = ((y1 - y0) * (x1 - x0)) as u32; let o = &mut out[(y * tw + x) * 4..(y * tw + x) * 4 + 4];
            for c in 0..4 { o[c] = ((sum[c] + n / 2) / n) as u8; }
        }
    }
    Img { w: tw as u32, h: th as u32, rgba: out }
}
/// A smaller picture by averaging the pixels each new one covers (colour weighted by its alpha). Straight alpha.
fn shrink(src: &[u8], w: u32, h: u32, tw: u32, th: u32) -> Vec<u8> {
    let (w, h, tw, th) = (w as usize, h as usize, tw as usize, th as usize);
    let mut out = vec![0u8; tw * th * 4];
    let xs: Vec<(usize, usize)> = (0..tw).map(|x| { let a = x * w / tw; (a, ((x + 1) * w / tw).max(a + 1).min(w)) }).collect();
    for y in 0..th {
        let (y0, y1) = { let a = y * h / th; (a, ((y + 1) * h / th).max(a + 1).min(h)) };
        for (x, (x0, x1)) in xs.iter().enumerate() {
            let (mut r, mut g, mut b, mut a) = (0u64, 0u64, 0u64, 0u64);
            for sy in y0..y1 { let row = &src[(sy * w + x0) * 4..(sy * w + x1) * 4]; for px in row.chunks_exact(4) { let al = px[3] as u64; r += px[0] as u64 * al; g += px[1] as u64 * al; b += px[2] as u64 * al; a += al; } }
            let n = ((y1 - y0) * (x1 - x0)) as u64; let o = &mut out[(y * tw + x) * 4..(y * tw + x) * 4 + 4];
            if a > 0 { o[0] = ((r + a / 2) / a) as u8; o[1] = ((g + a / 2) / a) as u8; o[2] = ((b + a / 2) / a) as u8; o[3] = ((a + n / 2) / n) as u8; }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_picture_is_fitted_and_never_enlarged() {
        assert_eq!(fit(2000, 1000, 500), (500, 250)); assert_eq!(fit(1000, 2000, 500), (250, 500)); assert_eq!(fit(100, 50, 500), (100, 50)); assert_eq!(fit(4000, 1, 256), (256, 1));
    }
    #[test]
    fn shrinking_averages_and_keeps_transparency() {
        // 4 x 2: the left half opaque red, the right half clear
        let mut src = vec![0u8; 4 * 2 * 4]; for y in 0..2 { for x in 0..2 { let i = (y * 4 + x) * 4; src[i] = 255; src[i + 3] = 255; } }
        let out = shrink(&src, 4, 2, 2, 1); assert_eq!(&out[..4], [255, 0, 0, 255]); assert_eq!(&out[4..], [0, 0, 0, 0]);
        // one pixel over both: red at half strength, not darkened by the clear half
        let one = shrink(&src, 4, 2, 1, 1); assert_eq!(one, [255, 0, 0, 128]);
        assert_eq!(srgb(0.0), 0); assert_eq!(srgb(1.0), 255); assert_eq!(srgb(0.18), 118); assert_eq!(srgb(7.0), 255);
        assert!((half(0x3c00) - 1.0).abs() < 1e-6 && (half(0x3800) - 0.5).abs() < 1e-6);
    }
    #[test]
    fn a_premultiplied_picture_is_fitted_by_plain_averages() {
        // 4 x 2, premultiplied: the left half opaque red, the right half clear
        let mut src = vec![0u8; 4 * 2 * 4]; for y in 0..2 { for x in 0..2 { let i = (y * 4 + x) * 4; src[i] = 255; src[i + 3] = 255; } }
        let half = fitted(Img { w: 4, h: 2, rgba: src.clone() }, 2); assert_eq!((half.w, half.h), (2, 1)); assert_eq!(half.rgba, [255, 0, 0, 255, 0, 0, 0, 0]);
        let one = fitted(Img { w: 4, h: 2, rgba: src.clone() }, 1); assert_eq!(one.rgba, [128, 0, 0, 128], "half covered: half the colour, half the alpha");
        let same = fitted(Img { w: 4, h: 2, rgba: src.clone() }, 512); assert_eq!((same.w, same.h, same.rgba), (4, 2, src), "never enlarged");
    }
    #[test]
    fn our_decoders_take_the_production_formats_only() {
        for e in ["exr", "dpx", "tif", "tiff", "psd", "psb", "tga", "hdr"] { assert!(ours(e)); assert_eq!(maker(e), OP_DECODE); }
        for e in ["png", "jpg", "mov", "pdf", "", "/"] { assert!(!ours(e), "{e} is not our decoders'"); }
        for e in ["mov", "mp4", "mxf", "mkv"] { assert_eq!(maker(e), OP_VIDEO, "{e}: the player's"); }
        for e in ["png", "jpg", "pdf", "gif", "webp", "", "/"] { assert_eq!(maker(e), OP_THUMB, "{e}: the system's"); }
        // pixels twice as wide as high: the picture becomes twice as wide; square ones: untouched
        let wide = widened(Img { w: 2, h: 1, rgba: [[0u8, 0, 0, 255], [200, 200, 200, 255]].concat() }, (2, 1)); assert_eq!((wide.w, wide.h), (4, 1)); assert_eq!((wide.rgba[0], wide.rgba[12]), (0, 200)); assert!(wide.rgba[4] < wide.rgba[8]);
        assert_eq!(widened(Img { w: 2, h: 1, rgba: vec![9; 8] }, (1, 1)).w, 2); assert_eq!(widened(Img { w: 2, h: 1, rgba: vec![9; 8] }, (0, 0)).w, 2);
        // a quarter turn clockwise: 2 x 1 (A B) becomes 1 x 2 (A over B); back again after four
        let px = |v: u8| [v, v, v, 255]; let ab = Img { w: 2, h: 1, rgba: [px(1), px(2)].concat() };
        let t = turned(ab, 90); assert_eq!((t.w, t.h, t.rgba.clone()), (1, 2, [px(1), px(2)].concat()));
        let t = turned(t, 90); assert_eq!((t.w, t.h, t.rgba.clone()), (2, 1, [px(2), px(1)].concat()), "half a turn");
        let t = turned(turned(t, 180), 0); assert_eq!(t.rgba, [px(1), px(2)].concat()); let t = turned(t, 270); assert_eq!((t.w, t.h, t.rgba), (1, 2, [px(2), px(1)].concat()));
    }
}
