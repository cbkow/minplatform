//! Writers for captures (canon row 216): sRGB-encoded files — PNG 8-bit (with an sRGB chunk), JPEG 8-bit at QCView's quality 95
//! through TurboJPEG (4:4:4: review stills keep their chroma), TIFF 16-bit RGB (the format people pick for quality — the user
//! 2026-11-08: "give them 16 bit tiffs").
use crate::{sys::tj::*, Error};
use std::io::BufWriter;

pub fn png8(path: &str, rgb: &[u8], w: u32, h: u32) -> Result<(), Error> {
    let f = std::fs::File::create(path).map_err(|e| Error::Open(format!("{path}: {e}")))?;
    let mut enc = png::Encoder::new(BufWriter::new(f), w, h);
    enc.set_color(png::ColorType::Rgb); enc.set_depth(png::BitDepth::Eight); enc.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut wr = enc.write_header().map_err(|e| Error::Decode(path.into(), e.to_string()))?;
    wr.write_image_data(rgb).map_err(|e| Error::Decode(path.into(), e.to_string()))
}

pub fn jpeg8(path: &str, rgb: &[u8], w: u32, h: u32, quality: i32) -> Result<(), Error> {
    let bytes = jpeg8_bytes(rgb, w, h, quality).map_err(|e| Error::Decode(path.into(), e))?;
    std::fs::write(path, bytes).map_err(|e| Error::Open(format!("{path}: {e}")))
}
/// The JPEG in memory (an export embeds it).
pub fn jpeg8_bytes(rgb: &[u8], w: u32, h: u32, quality: i32) -> Result<Vec<u8>, String> {
    struct Handle(tjhandle);
    impl Drop for Handle { fn drop(&mut self) { unsafe { tj3Destroy(self.0) } } }
    #[cfg(tj_init_version)] let hd = Handle(unsafe { tj3InitVersion(TJINIT_COMPRESS as _, TURBOJPEG_VERSION_NUMBER as _) });
    #[cfg(not(tj_init_version))] let hd = Handle(unsafe { tj3Init(TJINIT_COMPRESS as _) });
    if hd.0.is_null() { return Err("tj3Init failed".into()) }
    unsafe { tj3Set(hd.0, TJPARAM_QUALITY as _, quality); tj3Set(hd.0, TJPARAM_SUBSAMP as _, TJSAMP_444 as _); }
    let mut buf: *mut u8 = std::ptr::null_mut(); let mut size: usize = 0;
    let r = unsafe { tj3Compress8(hd.0, rgb.as_ptr(), w as _, 0, h as _, TJPF_RGB as _, &mut buf, &mut size) };
    if r != 0 { return Err(unsafe { std::ffi::CStr::from_ptr(tj3GetErrorStr(hd.0)) }.to_string_lossy().into_owned()) }
    let bytes = unsafe { std::slice::from_raw_parts(buf, size) }.to_vec();
    unsafe { tj3Free(buf as *mut _) };
    Ok(bytes)
}
/// An RGBA PNG in memory (an export's transparent ink layer).
pub fn png_rgba8_bytes(rgba: &[u8], w: u32, h: u32) -> Result<Vec<u8>, String> {
    let mut out = vec![];
    { let mut enc = png::Encoder::new(&mut out, w, h); enc.set_color(png::ColorType::Rgba); enc.set_depth(png::BitDepth::Eight); enc.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
      let mut wr = enc.write_header().map_err(|e| e.to_string())?; wr.write_image_data(rgba).map_err(|e| e.to_string())?; }
    Ok(out)
}

/// PNG 16-bit RGB (the "PNG 16-bit" screenshot choice, row 224) — samples big-endian, as PNG stores them.
pub fn png16(path: &str, rgb: &[u16], w: u32, h: u32) -> Result<(), Error> { png16_any(path, rgb, w, h, png::ColorType::Rgb) }
/// PNG 16-bit RGBA, straight alpha (row 224 + 223).
pub fn png_rgba16(path: &str, rgba: &[u16], w: u32, h: u32) -> Result<(), Error> { png16_any(path, rgba, w, h, png::ColorType::Rgba) }
fn png16_any(path: &str, samples: &[u16], w: u32, h: u32, color: png::ColorType) -> Result<(), Error> {
    let f = std::fs::File::create(path).map_err(|e| Error::Open(format!("{path}: {e}")))?;
    let mut enc = png::Encoder::new(BufWriter::new(f), w, h);
    enc.set_color(color); enc.set_depth(png::BitDepth::Sixteen); enc.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
    let mut wr = enc.write_header().map_err(|e| Error::Decode(path.into(), e.to_string()))?;
    let bytes: Vec<u8> = samples.iter().flat_map(|v| v.to_be_bytes()).collect();
    wr.write_image_data(&bytes).map_err(|e| Error::Decode(path.into(), e.to_string()))
}
/// PNG 8-bit with straight alpha (a screenshot of media with alpha, row 223).
pub fn png_rgba8(path: &str, rgba: &[u8], w: u32, h: u32) -> Result<(), Error> {
    let bytes = png_rgba8_bytes(rgba, w, h).map_err(|e| Error::Decode(path.into(), e))?;
    std::fs::write(path, bytes).map_err(|e| Error::Open(format!("{path}: {e}")))
}
/// TIFF 16-bit RGBA, straight (unassociated) alpha — what compositing apps read without darkening the edges (row 223).
pub fn tiff16_rgba(path: &str, rgba: &[u16], w: u32, h: u32) -> Result<(), Error> {
    let f = std::fs::File::create(path).map_err(|e| Error::Open(format!("{path}: {e}")))?;
    let mut enc = tiff::encoder::TiffEncoder::new(BufWriter::new(f)).map_err(|e| Error::Decode(path.into(), e.to_string()))?;
    enc.write_image::<tiff::encoder::colortype::RGBA16>(w, h, rgba).map_err(|e| Error::Decode(path.into(), e.to_string()))
}

pub fn tiff16(path: &str, rgb: &[u16], w: u32, h: u32) -> Result<(), Error> {
    let f = std::fs::File::create(path).map_err(|e| Error::Open(format!("{path}: {e}")))?;
    let mut enc = tiff::encoder::TiffEncoder::new(BufWriter::new(f)).map_err(|e| Error::Decode(path.into(), e.to_string()))?;
    enc.write_image::<tiff::encoder::colortype::RGB16>(w, h, rgb).map_err(|e| Error::Decode(path.into(), e.to_string()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_three_writers_round_trip_through_our_readers() {
        let dir = std::env::temp_dir(); let (w, h) = (16u32, 8u32);
        let rgb: Vec<u8> = (0..w * h * 3).map(|i| (i * 7 % 256) as u8).collect();
        let p = dir.join(format!("mi-w-{}.png", std::process::id())).to_string_lossy().into_owned(); super::png8(&p, &rgb, w, h).unwrap();
        let img = crate::load(&p, "", 1).unwrap(); assert_eq!((img.width, img.height), (w, h)); let _ = std::fs::remove_file(&p);
        let p = dir.join(format!("mi-w-{}.jpg", std::process::id())).to_string_lossy().into_owned(); super::jpeg8(&p, &rgb, w, h, 95).unwrap();
        let img = crate::load(&p, "", 1).unwrap(); assert_eq!((img.width, img.height), (w, h)); let _ = std::fs::remove_file(&p);
        let rgb16: Vec<u16> = rgb.iter().map(|v| *v as u16 * 257).collect();
        let p = dir.join(format!("mi-w-{}.tif", std::process::id())).to_string_lossy().into_owned(); super::tiff16(&p, &rgb16, w, h).unwrap();
        let img = crate::load(&p, "", 1).unwrap(); assert_eq!((img.width, img.height), (w, h)); let _ = std::fs::remove_file(&p);
        // the alpha writers (row 223)
        let rgba: Vec<u8> = (0..w * h * 4).map(|i| (i * 5 % 256) as u8).collect();
        let p = dir.join(format!("mi-wa-{}.png", std::process::id())).to_string_lossy().into_owned(); super::png_rgba8(&p, &rgba, w, h).unwrap();
        let img = crate::load(&p, "", 1).unwrap(); assert_eq!((img.width, img.height), (w, h)); let _ = std::fs::remove_file(&p);
        let rgba16: Vec<u16> = rgba.iter().map(|v| *v as u16 * 257).collect();
        let p = dir.join(format!("mi-wa-{}.tif", std::process::id())).to_string_lossy().into_owned(); super::tiff16_rgba(&p, &rgba16, w, h).unwrap();
        let img = crate::load(&p, "", 1).unwrap(); assert_eq!((img.width, img.height), (w, h)); let _ = std::fs::remove_file(&p);
        // PNG 16-bit, with and without alpha (row 224)
        for (n, alpha) in [("p16", false), ("pa16", true)] {
            let p = dir.join(format!("mi-{n}-{}.png", std::process::id())).to_string_lossy().into_owned();
            if alpha { super::png_rgba16(&p, &rgba16, w, h).unwrap() } else { super::png16(&p, &rgb16, w, h).unwrap() }
            let img = crate::load(&p, "", 1).unwrap(); assert_eq!((img.width, img.height), (w, h)); let _ = std::fs::remove_file(&p);
        }
    }
}
