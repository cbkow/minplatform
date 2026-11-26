//! The thumbnail extension's Rust half (canon rows 610 point 7, 612): one small picture of a file, on the CPU, in the
//! look QCView gives a file with no colour chain. An EXR: its first layer, linear light through the sRGB curve, its
//! alpha kept. A video (MXF): its FIRST frame through the family's decoder and poster stage - an SDR clip's code values
//! as stored, an HDR one brought to SDR on a small engine with no window. The conversion of an image's planes is the
//! file browser's (`apps/ufb/src/thumbs/helper.rs` `rgba8`), kept the same so a file looks the same in both.
use std::ffi::{c_char, CStr};

fn half(b: u16) -> f32 {
    let (s, e, m) = ((b >> 15) & 1, (b >> 10) & 0x1f, (b & 0x3ff) as f32);
    let v = match e { 0 => m * 2f32.powi(-24), 31 => if m == 0.0 { f32::INFINITY } else { 0.0 }, _ => (1.0 + m / 1024.0) * 2f32.powi(e as i32 - 15) };
    if s == 1 { -v } else { v }
}
fn srgb(l: f32) -> u8 { let l = l.clamp(0.0, 1.0); let v = if l <= 0.003_130_8 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 }; (v * 255.0 + 0.5) as u8 }

/// The decoder's planes (G, B, R [, A]) as straight-alpha RGBA, 8 bits: code values as they are; linear light divided
/// by its alpha, then the sRGB curve.
fn rgba8(img: &min_image::Image) -> Option<(Vec<u8>, u32, u32)> {
    let (w, h, p) = (img.width as usize, img.height as usize, img.pitch as usize);
    let bytes = img.sample.bytes(); let planes = if img.alpha { 4 } else { 3 };
    if w == 0 || h == 0 || img.planes.len() < planes || w * bytes > p || img.planes.iter().take(planes).any(|pl| pl.len() < h * p) { return None }
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
    Some((out, w as u32, h as u32))
}

/// Straight RGBA made to fit `px` on its longer side (never enlarged) and multiplied by its alpha, as the system's
/// drawing wants it: each new pixel is the average of those it covers.
fn fitted_premultiplied(rgba: &[u8], w: u32, h: u32, px: u32) -> (Vec<u8>, u32, u32) {
    let k = (px.max(1) as f64 / w.max(h) as f64).min(1.0);
    let (tw, th) = (((w as f64 * k).round() as usize).max(1), ((h as f64 * k).round() as usize).max(1)); let (w, h) = (w as usize, h as usize);
    let xs: Vec<(usize, usize)> = (0..tw).map(|x| { let a = x * w / tw; (a, ((x + 1) * w / tw).max(a + 1).min(w)) }).collect();
    let mut out = vec![0u8; tw * th * 4];
    for y in 0..th {
        let (y0, y1) = { let a = y * h / th; (a, ((y + 1) * h / th).max(a + 1).min(h)) };
        for (x, &(x0, x1)) in xs.iter().enumerate() {
            let mut s = [0u64; 4];
            for yy in y0..y1 { for xx in x0..x1 { let p = &rgba[(yy * w + xx) * 4..][..4]; let a = p[3] as u64; s[0] += p[0] as u64 * a; s[1] += p[1] as u64 * a; s[2] += p[2] as u64 * a; s[3] += a; } }
            let n = ((y1 - y0) * (x1 - x0)) as u64; let o = &mut out[(y * tw + x) * 4..][..4];
            for c in 0..3 { o[c] = ((s[c] + n * 255 / 2) / (n * 255)) as u8; } o[3] = ((s[3] + n / 2) / n) as u8;
        }
    }
    (out, tw as u32, th as u32)
}

fn exr(path: &str, px: u32) -> Option<(Vec<u8>, u32, u32)> {
    let img = min_image::load(path, "", 8).ok()?; if img.sentinel.is_some() { return None }
    let (rgba, w, h) = rgba8(&img)?; Some(fitted_premultiplied(&rgba, w, h, px))
}

fn video(path: &str, px: u32) -> Option<(Vec<u8>, u32, u32)> {
    use qcv_player::thumbnail_cache::{ThumbLoader, VideoThumbLoader};
    let meta = { let s = qcv_player::Session::open(path, &qcv_player::OpenOptions { threads: 1, hw: None, hw_intra: None, seq: None, fast: true, live: None }).ok()?; if s.info.width == 0 || s.info.height == 0 { return None } s.info.meta };
    let mut l = VideoThumbLoader::open(path)?;
    let frame = l.load_thumbnail(0, px.clamp(16, 1024))?;
    // an HDR clip needs the presenter's pipeline to come to SDR; an SDR one is its code values, and needs no device
    let hdr = matches!(meta.transfer, min_present::Transfer::Pq | min_present::Transfer::Hlg);
    let mut poster = if hdr { qcv_player::poster::Poster::headless() } else { Default::default() };
    let (rgba, w, h) = poster.convert(None, &frame, &meta);
    (w > 0 && h > 0 && rgba.len() == (w as usize) * (h as usize) * 4).then_some((rgba, w, h))
}

/// A file's thumbnail: RGBA, 8 bits, multiplied by its alpha, top row first, at most `max_px` on its longer side; the
/// caller frees it with `qlt_free`. Null when there is no picture to give (the system then draws its own icon).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn qlt_thumb(path: *const c_char, max_px: u32, w: *mut u32, h: *mut u32) -> *mut u8 {
    if path.is_null() { return std::ptr::null_mut() }
    let path = unsafe { CStr::from_ptr(path) }.to_string_lossy().into_owned();
    let ext = std::path::Path::new(&path).extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
    let made = std::panic::catch_unwind(|| if ext == "exr" { exr(&path, max_px) } else { video(&path, max_px) }).ok().flatten();
    let Some((rgba, tw, th)) = made else { return std::ptr::null_mut() };
    unsafe { *w = tw; *h = th; }
    Box::into_raw(rgba.into_boxed_slice()) as *mut u8
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn qlt_free(p: *mut u8, w: u32, h: u32) { if !p.is_null() { drop(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(p, (w as usize) * (h as usize) * 4)) }); } }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_picture_is_fitted_and_multiplied_by_its_alpha() {
        // 4 x 2: the left half opaque red, the right half clear
        let mut src = vec![0u8; 4 * 2 * 4]; for y in 0..2 { for x in 0..2 { src[(y * 4 + x) * 4..][..4].copy_from_slice(&[255, 0, 0, 255]); } }
        let (out, w, h) = fitted_premultiplied(&src, 4, 2, 2);
        assert_eq!((w, h), (2, 1)); assert_eq!(&out[..4], &[255, 0, 0, 255]); assert_eq!(&out[4..], &[0, 0, 0, 0]);
        let (same, w, h) = fitted_premultiplied(&src, 4, 2, 64); assert_eq!((w, h, same.len()), (4, 2, 32), "never enlarged");
    }
}
