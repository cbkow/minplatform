//! TGA and BMP — ours (canon row 191, the user 2026-11-08: "Support them natively"; QCView read neither) through the pure-Rust
//! decoders of the `image` crate, built with those two formats only. Both are 8-bit formats: RGB, or RGBA when the file
//! carries alpha (TGA's attribute bits, BMP's alpha mask); palette and 16-bit (5-6-5 / 5-5-5) layouts expand to 8-bit RGB as
//! the crate decodes them.
use crate::{Error, Image, Sample, Samples};
use image::ImageReader;

fn reader(path: &str) -> Result<ImageReader<std::io::BufReader<std::fs::File>>, Error> {
    ImageReader::open(path).map_err(|e| Error::Open(format!("{path}: {e}")))?.with_guessed_format().map_err(|e| Error::Open(format!("{path}: {e}")))
}

pub fn load(path: &str) -> Result<Image, Error> {
    let img = reader(path)?.decode().map_err(|e| Error::Decode(path.into(), e.to_string()))?;
    let (w, h) = (img.width(), img.height());
    Ok(if img.color().has_alpha() { Image::from_interleaved(w, h, Sample::U8, Samples::U8(img.to_rgba8().as_raw()), 4, true, true) }
       else { Image::from_interleaved(w, h, Sample::U8, Samples::U8(img.to_rgb8().as_raw()), 3, true, false) })
}

pub fn dimensions(path: &str) -> Result<(u32, u32), Error> { reader(path)?.into_dimensions().map_err(|e| Error::Decode(path.into(), e.to_string())) }
