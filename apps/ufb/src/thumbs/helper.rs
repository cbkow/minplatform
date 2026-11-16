//! The helper: this executable started with `--thumb-helper` (canon row 456; spike S18). It reads one request, does
//! the work, writes one answer, for ever; it ends when its standard input closes (the application is gone or wants it
//! gone). It has no window, no event loop and no run loop. Everything that reads a file's CONTENTS happens here: a
//! damaged file can end a helper, never the application.
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
    let test = std::env::var_os("UFB_HELPER_TEST").is_some();
    let (mut inp, mut out) = std_files();
    // the hello: the application bounds a start from the spawn to this frame
    if write_response(&mut *out, &Response::default()).is_err() { std::process::exit(0) }
    loop {
        let Ok(req) = read_request(&mut *inp) else { std::process::exit(0) };
        let resp = handle(&req, test);
        if write_response(&mut *out, &resp).is_err() { std::process::exit(0) }
    }
}

fn handle(req: &Request, test: bool) -> Response {
    let t = Instant::now();
    let mut r = Response { id: req.id, ..Default::default() };
    let soft = Duration::from_millis(req.soft_ms.max(1) as u64);
    let res: Result<Img, (u8, i64)> = match req.op {
        OP_PING => return r,
        OP_DECODE => decode(&req.path, req.size as u32).map_err(|_| (FAILED, 0)),
        OP_THUMB => sys::thumbnail(&req.path, req.size as u32, soft),
        OP_ICON => sys::icon(&req.path, req.size as u32, soft),
        OP_TEST_ABORT if test => std::process::abort(),
        OP_TEST_HANG if test => loop { std::thread::sleep(Duration::from_secs(3600)); },
        _ => Err((FAILED, 0)),
    };
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
pub fn ours(ext: &str) -> bool { matches!(ext, "exr" | "dpx" | "tif" | "tiff" | "psd" | "psb" | "tga") }

/// Our own decoders: the file -> a picture that fits `px`.
pub fn decode(path: &str, px: u32) -> Result<Img, String> {
    let px = px.clamp(16, 1024);
    let ext = Path::new(path).extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
    let (rgba, w, h) = match ext.as_str() {
        // the flattened picture, read row by row at the size asked; opaque
        "psd" | "psb" => { let f = min_image::psd::load(Path::new(path), px)?; (f.rgba, f.w, f.h) }
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
    fn our_decoders_take_the_production_formats_only() {
        for e in ["exr", "dpx", "tif", "tiff", "psd", "psb", "tga"] { assert!(ours(e)); }
        for e in ["png", "jpg", "mov", "pdf", "", "/"] { assert!(!ours(e), "{e} is the system's"); }
    }
}
