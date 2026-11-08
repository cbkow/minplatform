//! EXR through OpenEXRCore (row 148: the C library, FFI). QCView `EXRImageLoader::loadFrame` carried rule for rule: the part
//! named `layer` (else part 0); channels `<layer>.R/G/B/A`, else bare `R/G/B/A` when a layer was asked for but is absent,
//! else the first prefixed layer when none was asked for and the file has no bare RGB (Blender multi-part renders); the image
//! is the DISPLAY window, the data window placed in it, zeros outside. DEVIATIONS (written): chunks decode in parallel on our
//! threads (Imf's global pool, 2–8 threads, is how QCView got its "parallel mode"); the output is planar G, B, R [, A] halves
//! (the presenter's planar RGB) instead of interleaved RGBA16F, so no alpha is synthesised — three planes mean opaque.
use crate::{pitch_for, sys::*, Error, Image, Sample, MIN_FILE_BYTES};
use std::{ffi::{CStr, CString}, sync::atomic::{AtomicUsize, Ordering}};

struct Ctx(exr_context_t);
unsafe impl Send for Ctx {}
unsafe impl Sync for Ctx {}   // OpenEXRCore reads chunks concurrently through a const context
impl Drop for Ctx { fn drop(&mut self) { unsafe { exr_finish(&mut self.0); } } }

fn ok(r: exr_result_t, what: &str, path: &str) -> Result<(), Error> { if r == 0 { Ok(()) } else { Err(Error::Decode(path.into(), format!("{what} failed ({r})"))) } }

fn open(path: &str) -> Result<Ctx, Error> {
    if std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) < MIN_FILE_BYTES { return Err(Error::Open(format!("{path}: missing or truncated"))) }
    let c = CString::new(path).map_err(|_| Error::Open(path.into()))?;
    let mut init = exr_context_initializer_t { size: std::mem::size_of::<exr_context_initializer_t>(), ..Default::default() };
    init.error_handler_fn = Some(quiet);   // errors come back as results; the default handler prints every one to stderr
    let mut ctx: exr_context_t = std::ptr::null_mut();
    ok(unsafe { exr_start_read(&mut ctx, c.as_ptr(), &init) }, "exr_start_read", path)?;
    Ok(Ctx(ctx))
}
unsafe extern "C" fn quiet(_: exr_const_context_t, _: exr_result_t, _: *const std::os::raw::c_char) {}

fn channels(ctx: &Ctx, part: i32) -> Vec<(String, exr_pixel_type_t)> {
    let mut cl: *const exr_attr_chlist_t = std::ptr::null();
    if unsafe { exr_get_channels(ctx.0, part, &mut cl) } != 0 || cl.is_null() { return vec![] }
    let cl = unsafe { &*cl };
    (0..cl.num_channels.max(0) as usize).map(|i| { let e = unsafe { &*cl.entries.add(i) }; (unsafe { CStr::from_ptr(e.name.str_) }.to_string_lossy().into_owned(), e.pixel_type) }).collect()
}
fn part_name(ctx: &Ctx, part: i32) -> Option<String> { let mut n: *const std::os::raw::c_char = std::ptr::null(); (unsafe { exr_get_name(ctx.0, part, &mut n) } == 0 && !n.is_null()).then(|| unsafe { CStr::from_ptr(n) }.to_string_lossy().into_owned()) }

/// The layers a file offers (QCView's layer discovery): part names for multi-part files, else the channel prefixes owning an
/// `.R` ("" for the bare RGB).
pub fn layers(path: &str) -> Result<Vec<String>, Error> {
    let ctx = open(path)?;
    let mut n = 0; ok(unsafe { exr_get_count(ctx.0, &mut n) }, "exr_get_count", path)?;
    if n > 1 { return Ok((0..n).filter_map(|p| part_name(&ctx, p)).collect()) }
    let ch = channels(&ctx, 0);
    let mut out: Vec<String> = vec![];
    if ch.iter().any(|(c, _)| c == "R") { out.push(String::new()); }
    for (c, _) in &ch { if let Some(p) = c.strip_suffix(".R") { if !out.iter().any(|o| o == p) { out.push(p.to_string()); } } }
    Ok(out)
}

/// Decode `path` (QCView `loadFrame`): `layer` "" = the default; `threads` 0 = all cores, capped at 8 (QCView's probe: 8 is
/// the knee on 4K DWAB).
pub fn load(path: &str, layer: &str, threads: usize) -> Result<Image, Error> {
    let ctx = open(path)?;
    let mut parts = 0; ok(unsafe { exr_get_count(ctx.0, &mut parts) }, "exr_get_count", path)?;
    let part = if layer.is_empty() || parts <= 1 { 0 } else { (0..parts).find(|p| part_name(&ctx, *p).as_deref() == Some(layer)).unwrap_or(0) };
    let mut disp = exr_attr_box2i_t::default(); let mut data = exr_attr_box2i_t::default();
    ok(unsafe { exr_get_display_window(ctx.0, part, &mut disp) }, "display window", path)?;
    ok(unsafe { exr_get_data_window(ctx.0, part, &mut data) }, "data window", path)?;
    let (w, h) = (disp.max.x - disp.min.x + 1, disp.max.y - disp.min.y + 1);
    let (dw, dh) = (data.max.x - data.min.x + 1, data.max.y - data.min.y + 1);
    if w <= 0 || h <= 0 || dw <= 0 || dh <= 0 { return Err(Error::Decode(path.into(), "empty window".into())) }
    // channel discovery, QCView's order of fallbacks
    let chans = channels(&ctx, part);
    let has = |n: &str| chans.iter().any(|(c, _)| c == n);
    let mut prefix = if layer.is_empty() { String::new() } else { format!("{layer}.") };
    if !has(&format!("{prefix}R")) && !layer.is_empty() { prefix.clear(); }
    if !has(&format!("{prefix}R")) && layer.is_empty() { if let Some(fb) = chans.iter().find_map(|(c, _)| c.strip_suffix(".R").map(|s| s.to_string())) { prefix = format!("{fb}."); } }
    let names = ["G", "B", "R", "A"].map(|c| format!("{prefix}{c}"));   // the presenter's planar order
    if !(has(&names[0]) && has(&names[1]) && has(&names[2])) { return Err(Error::NoRgb(path.into(), layer.into())) }
    let alpha = has(&names[3]);
    let nplanes = if alpha { 4 } else { 3 };
    // decode into data-window planes, chunk by chunk, in parallel
    let pitch = pitch_for(dw as u32, Sample::F16) as usize;
    let mut planes: Vec<Vec<u8>> = (0..nplanes).map(|_| vec![0u8; pitch * dh as usize]).collect();
    let mut storage: exr_storage_t = 0; ok(unsafe { exr_get_storage(ctx.0, part, &mut storage) }, "storage", path)?;
    let chunks: Vec<(i32, i32)> = if storage == exr_storage_t_EXR_STORAGE_TILED {
        let (mut cx, mut cy) = (0, 0); ok(unsafe { exr_get_tile_counts(ctx.0, part, 0, 0, &mut cx, &mut cy) }, "tile counts", path)?;
        (0..cy).flat_map(|y| (0..cx).map(move |x| (x, y))).collect()
    } else if storage == exr_storage_t_EXR_STORAGE_SCANLINE {
        let mut lines = 0; ok(unsafe { exr_get_scanlines_per_chunk(ctx.0, part, &mut lines) }, "scanlines per chunk", path)?;
        (0..(dh + lines - 1) / lines.max(1)).map(|i| (-1, data.min.y + i * lines)).collect()
    } else { return Err(Error::Decode(path.into(), "deep EXR is not supported".into())) };
    let tiled = storage == exr_storage_t_EXR_STORAGE_TILED;
    let next = AtomicUsize::new(0); let failed = AtomicUsize::new(0);
    let threads = if threads == 0 { std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).clamp(1, 8) } else { threads }.min(chunks.len().max(1));
    let bases: Vec<usize> = planes.iter_mut().map(|p| p.as_mut_ptr() as usize).collect();
    let cnames: Vec<CString> = names.iter().take(nplanes).map(|n| CString::new(n.as_str()).unwrap()).collect();
    let ectx = &ctx;   // the whole Sync wrapper, not its raw pointer field (closures capture fields disjointly)
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                let mut dec = exr_decode_pipeline_t::default(); let mut inited = false;
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed); if i >= chunks.len() { break }
                    let (cx, cy) = chunks[i];
                    let mut ci = exr_chunk_info_t::default();
                    let r = unsafe { if tiled { exr_read_tile_chunk_info(ectx.0, part, cx, cy, 0, 0, &mut ci) } else { exr_read_scanline_chunk_info(ectx.0, part, cy, &mut ci) } };
                    if r != 0 { failed.fetch_add(1, Ordering::Relaxed); continue }
                    let r = unsafe { if inited { exr_decoding_update(ectx.0, part, &ci, &mut dec) } else { exr_decoding_initialize(ectx.0, part, &ci, &mut dec) } };
                    if r != 0 { failed.fetch_add(1, Ordering::Relaxed); continue }
                    inited = true;
                    for k in 0..dec.channel_count.max(0) as usize {
                        let ch = unsafe { &mut *dec.channels.add(k) };
                        let name = unsafe { CStr::from_ptr(ch.channel_name) };
                        let slot = cnames.iter().position(|n| n.as_c_str() == name);
                        match slot {
                            Some(p) if ch.x_samples == 1 && ch.y_samples == 1 => {
                                let off = (ci.start_y - data.min.y) as usize * pitch + (ci.start_x - data.min.x) as usize * 2;
                                ch.__bindgen_anon_1.decode_to_ptr = (bases[p] + off) as *mut u8;
                                ch.user_pixel_stride = 2; ch.user_line_stride = pitch as i32; ch.user_data_type = exr_pixel_type_t_EXR_PIXEL_HALF as u16; ch.user_bytes_per_element = 2;
                            }
                            _ => { ch.__bindgen_anon_1.decode_to_ptr = std::ptr::null_mut(); }
                        }
                    }
                    let r = unsafe { exr_decoding_choose_default_routines(ectx.0, part, &mut dec) };
                    if r != 0 || unsafe { exr_decoding_run(ectx.0, part, &mut dec) } != 0 { failed.fetch_add(1, Ordering::Relaxed); }
                }
                if inited { unsafe { exr_decoding_destroy(ectx.0, &mut dec); } }
            });
        }
    });
    let f = failed.load(Ordering::Relaxed);
    if f > 0 { return Err(Error::Decode(path.into(), format!("{f} of {} chunks failed", chunks.len()))) }
    // the display window: the fast path when it equals the data window, else the intersection copied in, zeros around it
    if disp.min.x == data.min.x && disp.min.y == data.min.y && w == dw && h == dh {
        return Ok(Image { width: w as u32, height: h as u32, sample: Sample::F16, planes, pitch: pitch as u32, alpha, sentinel: None })
    }
    let dpitch = pitch_for(w as u32, Sample::F16) as usize;
    let (x0, x1) = (disp.min.x.max(data.min.x), disp.max.x.min(data.max.x)); let (y0, y1) = (disp.min.y.max(data.min.y), disp.max.y.min(data.max.y));
    let out: Vec<Vec<u8>> = planes.iter().map(|src| {
        let mut dst = vec![0u8; dpitch * h as usize];
        if x1 >= x0 { for y in y0..=y1 {
            let s = (y - data.min.y) as usize * pitch + (x0 - data.min.x) as usize * 2; let d = (y - disp.min.y) as usize * dpitch + (x0 - disp.min.x) as usize * 2; let n = (x1 - x0 + 1) as usize * 2;
            dst[d..d + n].copy_from_slice(&src[s..s + n]);
        } }
        dst
    }).collect();
    Ok(Image { width: w as u32, height: h as u32, sample: Sample::F16, planes: out, pitch: dpitch as u32, alpha, sentinel: None })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Option<String> { let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/exr_4k/frame_0001.exr"); p.exists().then(|| p.to_string_lossy().into_owned()) }
    fn half(b: &[u8]) -> f32 { let h = u16::from_le_bytes([b[0], b[1]]); let s = if h & 0x8000 != 0 { -1.0 } else { 1.0 }; let e = ((h >> 10) & 0x1f) as i32; let m = (h & 0x3ff) as f32; s * if e == 0 { m / 1024.0 * 2f32.powi(-14) } else { (1.0 + m / 1024.0) * 2f32.powi(e - 15) } }
    #[test]
    fn the_4k_fixture_decodes_to_three_half_planes_with_plausible_values() {
        let Some(p) = fixture() else { eprintln!("fixture absent — skipped"); return };
        let t = std::time::Instant::now();
        let img = load(&p, "", 0).expect("decode");
        eprintln!("4K EXR decode {:.1} ms", t.elapsed().as_secs_f32() * 1000.0);
        assert_eq!((img.width, img.height, img.planes.len(), img.alpha), (3840, 2160, 3, false));
        assert_eq!(img.pitch as usize % 256, 0);
        let row = img.height as usize / 2 * img.pitch as usize + img.width as usize; // the centre pixel
        let (g, b, r) = (half(&img.planes[0][row..]), half(&img.planes[1][row..]), half(&img.planes[2][row..]));
        assert!([g, b, r].iter().all(|v| v.is_finite() && *v >= 0.0 && *v < 64.0), "{r} {g} {b}");
        assert_eq!(layers(&p).unwrap(), vec![String::new()]);
    }
    #[test]
    fn a_missing_file_and_a_layer_without_rgb_fail_cleanly() {
        assert!(matches!(load("/nope/x.exr", "", 0), Err(Error::Open(_))));
        let Some(p) = fixture() else { return };
        assert!(load(&p, "nonexistent", 0).is_ok(), "an absent layer falls back to the bare RGB (QCView)");
    }
}
#[cfg(test)]
mod probe {
    #[test] #[ignore]
    fn print_points() {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/exr_4k/frame_0001.exr");
        let img = super::load(&p.to_string_lossy(), "", 0).unwrap();
        let half = |b: &[u8]| { let h = u16::from_le_bytes([b[0], b[1]]); let s = if h & 0x8000 != 0 { -1.0f32 } else { 1.0 }; let e = ((h >> 10) & 0x1f) as i32; let m = (h & 0x3ff) as f32; s * if e == 0 { m / 1024.0 * 2f32.powi(-14) } else { (1.0 + m / 1024.0) * 2f32.powi(e - 15) } };
        for (x, y) in [(1920usize, 1080usize), (100, 100), (3000, 1800), (500, 1500)] {
            let o = y * img.pitch as usize + x * 2;
            println!("PT {x} {y} {:.5} {:.5} {:.5}", half(&img.planes[2][o..]), half(&img.planes[0][o..]), half(&img.planes[1][o..]));
        }
    }
}
