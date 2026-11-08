//! TIFF — QCView's `TIFFLoader::load` (`decode/tiff_image_loader.cpp`): 8-bit anything-RGB-ish (gray, palette, RGB[A],
//! CMYK) → U8 with the Orientation tag honoured (QCView's `TIFFReadRGBAImageOriented(…, ORIENTATION_TOPLEFT)`), 16-bit
//! integer RGB[A] → U16.
//!
//! Divergences, with reasons: QCView marked 16-bit FLOAT and 32-bit FLOAT TIFFs high-depth but copied their bits into
//! an RGBA16 integer buffer ("the pixels will look wrong" — its own comment), and refused 16-bit gray. Here float TIFFs
//! land as F16 (32-bit converted, 16-bit kept), 16-bit gray reads, and the orientation applies at every depth.
//! Planar (separate-plane) files read too. What the `tiff` crate refuses — PALETTE, subsampled YCbCr, CIE / ICC Lab, old
//! JPEG — falls back to QCView's own path, libtiff's `TIFFReadRGBAImageOriented(…, ORIENTATION_TOPLEFT)` → 8-bit RGBA
//! (canon row 189 ruled 2026-11-08: "Cover those TIFFs too"); alpha is kept only when the file declares extra samples.
use crate::{Error, Image, Sample, Samples};
use tiff::{decoder::{Decoder, DecodingResult}, tags::Tag, ColorType};

pub fn load(path: &str) -> Result<Image, Error> {
    match load_crate(path) {
        Ok(img) => Ok(img),
        Err(Error::Decode(p, why)) => libtiff_rgba(path).map_err(|_| Error::Decode(p, why)),
        Err(e) => Err(e),
    }
}

/// QCView's 8-bit TIFF path verbatim: libtiff converts anything RGB-ish (palette, YCbCr at any subsampling, Lab, CMYK,
/// separations, old JPEG) to ABGR words, top-left first.
fn libtiff_rgba(path: &str) -> Result<Image, Error> {
    use crate::sys::tiff as t;
    let fail = |m: &str| Error::Decode(path.into(), format!("libtiff: {m}"));
    unsafe { t::TIFFSetWarningHandler(None); t::TIFFSetErrorHandler(None); }   // a broken frame is reported by the cache, not on stderr
    #[cfg(windows)] let tif = { let w: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect(); unsafe { t::TIFFOpenW(w.as_ptr() as _, c"r".as_ptr()) } };
    #[cfg(not(windows))] let tif = { let c = std::ffi::CString::new(path).map_err(|_| fail("path"))?; unsafe { t::TIFFOpen(c.as_ptr(), c"r".as_ptr()) } };
    if tif.is_null() { return Err(fail("open")) }
    struct Close(*mut t::TIFF); impl Drop for Close { fn drop(&mut self) { unsafe { t::TIFFClose(self.0) } } }
    let _c = Close(tif);
    let (mut w, mut h) = (0u32, 0u32);
    if unsafe { t::TIFFGetField(tif, t::TIFFTAG_IMAGEWIDTH, &mut w as *mut u32) } == 0 || unsafe { t::TIFFGetField(tif, t::TIFFTAG_IMAGELENGTH, &mut h as *mut u32) } == 0 { return Err(fail("size")) }
    if w == 0 || h == 0 || w as u64 * h as u64 > 1 << 28 { return Err(fail("size")) }
    let mut extra: u16 = 0; let mut kinds: *mut u16 = std::ptr::null_mut();
    let alpha = unsafe { t::TIFFGetField(tif, t::TIFFTAG_EXTRASAMPLES, &mut extra as *mut u16, &mut kinds as *mut *mut u16) } != 0 && extra > 0;
    let mut abgr = vec![0u32; w as usize * h as usize];
    if unsafe { t::TIFFReadRGBAImageOriented(tif, w, h, abgr.as_mut_ptr(), t::ORIENTATION_TOPLEFT as i32, 0) } == 0 { return Err(fail("read")) }
    let rgba: Vec<u8> = abgr.iter().flat_map(|v| [*v as u8, (*v >> 8) as u8, (*v >> 16) as u8, (*v >> 24) as u8]).collect();   // TIFFGetR/G/B/A
    Ok(if alpha { Image::from_interleaved(w, h, Sample::U8, Samples::U8(&rgba), 4, true, true) }
       else { let rgb: Vec<u8> = rgba.as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect(); Image::from_interleaved(w, h, Sample::U8, Samples::U8(&rgb), 3, true, false) })
}

fn load_crate(path: &str) -> Result<Image, Error> {
    let f = std::fs::File::open(path).map_err(|e| Error::Open(format!("{path}: {e}")))?;
    let e = |e: tiff::TiffError| Error::Decode(path.into(), e.to_string());
    let mut d = Decoder::new(std::io::BufReader::new(f)).map_err(e)?;
    let (w, h) = d.dimensions().map_err(e)?;
    let ct = d.colortype().map_err(e)?;
    let planar = d.find_tag_unsigned::<u16>(Tag::PlanarConfiguration).map_err(e)? == Some(2);
    let orientation = d.find_tag_unsigned::<u16>(Tag::Orientation).map_err(e)?.unwrap_or(1);
    let photometric = d.find_tag_unsigned::<u16>(Tag::PhotometricInterpretation).map_err(e)?.unwrap_or(2);
    let palette = if matches!(ct, ColorType::Palette(_)) { Some(d.get_tag_u16_vec(Tag::ColorMap).map_err(e)?) } else { None };
    let mut buf = DecodingResult::U8(vec![]);
    d.read_image_to_buffer(&mut buf).map_err(e)?;
    let px = w as usize * h as usize;
    let (channels, color, alpha) = match ct {
        ColorType::Gray(_) => (1, false, false), ColorType::GrayA(_) => (2, false, true),
        ColorType::RGB(_) | ColorType::YCbCr(_) => (3, true, false), ColorType::RGBA(_) => (4, true, true),
        ColorType::Palette(_) => (1, true, false), ColorType::CMYK(_) => (4, true, false), ColorType::CMYKA(_) => (5, true, true),
        // FFmpeg (and others) write gray + alpha / RGB + alpha without ExtraSamples: the crate calls it multiband.
        // libtiff's RGBA helper (QCView's 8-bit path) reads the extra sample as alpha — so do we.
        ColorType::Multiband { num_samples: 2, .. } if photometric <= 1 => (2, false, true),
        ColorType::Multiband { num_samples: 4, .. } if photometric == 2 => (4, true, true),
        other => return Err(Error::Decode(path.into(), format!("{other:?} TIFF"))),
    };
    if matches!(ct, ColorType::YCbCr(_)) { return Err(Error::Decode(path.into(), "YCbCr TIFF".into())) }
    // Planar files arrive plane after plane — interleave them so one splitter serves both layouts.
    fn interleave<T: Copy>(v: Vec<T>, px: usize, ch: usize) -> Vec<T> { (0..px * ch).map(|i| v[(i % ch) * px + i / ch]).collect() }
    fn fixed<T: Copy>(v: Vec<T>, planar: bool, px: usize, ch: usize) -> Vec<T> { if planar && ch > 1 { interleave(v, px, ch) } else { v } }
    let mut img = match buf {
        DecodingResult::U8(v) => {
            let v = fixed(v, planar, px, channels);
            if let Some(map) = palette {
                // TIFF palettes are 16-bit R…, G…, B… runs; QCView's RGBA helper keeps the high byte.
                let n = map.len() / 3;
                let rgb: Vec<u8> = v.iter().flat_map(|&i| { let i = i as usize; [(map[i] >> 8) as u8, (map[n + i] >> 8) as u8, (map[2 * n + i] >> 8) as u8] }).collect();
                Image::from_interleaved(w, h, Sample::U8, Samples::U8(&rgb), 3, true, false)
            } else if matches!(ct, ColorType::CMYK(_) | ColorType::CMYKA(_)) {
                // libtiff's RGBA helper: R = (255 − C)(255 − K) / 255 per channel.
                let rgba: Vec<u8> = v.chunks_exact(channels).flat_map(|p| { let k = 255 - p[3] as u32; let c = |x: u8| ((255 - x as u32) * k / 255) as u8; [c(p[0]), c(p[1]), c(p[2]), if alpha { p[4] } else { 255 }] }).collect();
                Image::from_interleaved(w, h, Sample::U8, Samples::U8(&rgba), 4, true, alpha)
            } else { Image::from_interleaved(w, h, Sample::U8, Samples::U8(&v), channels, color, alpha) }
        }
        DecodingResult::U16(v) if !matches!(ct, ColorType::CMYK(_) | ColorType::CMYKA(_) | ColorType::Palette(_)) =>
            Image::from_interleaved(w, h, Sample::U16, Samples::U16(&fixed(v, planar, px, channels)), channels, color, alpha),
        DecodingResult::F16(v) => { let v: Vec<u16> = fixed(v, planar, px, channels).iter().map(|x| x.to_bits()).collect(); Image::from_interleaved(w, h, Sample::F16, Samples::U16(&v), channels, color, alpha) }
        DecodingResult::F32(v) => { let v: Vec<u16> = fixed(v, planar, px, channels).iter().map(|x| half::f16::from_f32(*x).to_bits()).collect(); Image::from_interleaved(w, h, Sample::F16, Samples::U16(&v), channels, color, alpha) }
        _ => return Err(Error::Decode(path.into(), format!("{ct:?} TIFF at this depth"))),
    };
    match orientation { 2 => img.flip(true, false), 3 => img.flip(true, true), 4 => img.flip(false, true), _ => {} }
    Ok(img)
}

pub fn dimensions(path: &str) -> Result<(u32, u32), Error> {
    let f = std::fs::File::open(path).map_err(|e| Error::Open(format!("{path}: {e}")))?;
    Decoder::new(std::io::BufReader::new(f)).and_then(|mut d| d.dimensions()).map_err(|e| Error::Decode(path.into(), e.to_string()))
}
