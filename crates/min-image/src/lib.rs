//! `min-image` — ONE image decoding stack for the family (canon §10.4, rows 34 / 148; Phase 2 step 2c). QCView's loaders
//! (`decode/{exr,png,tiff,jpeg}_image_loader`, `image_loader.h`) carried: an [`Image`] is the decoded frame as planar planes
//! in the presenter's RGB order (G, B, R [, A]), rows padded to 256 bytes; EXR arrives as half floats (linear light),
//! the 8 / 16-bit formats as integers. Gap / broken sentinels stand in for missing / undecodable frames (QCView's
//! `MakeGapSentinel` / `MakeBrokenSentinel`).
//!
//! PNG and TIFF decode through pure-Rust crates (`png`, `tiff`) where QCView links libpng / libtiff — both formats are
//! lossless, so the crates are bit-identical to FFmpeg's decode on every layout tested, with no system library to pin
//! and memory safety on hostile files. JPEG is LOSSY: its pixels depend on the decoder's IDCT and upsampling, and the
//! pure-Rust decoder measured 2–4 code values off libjpeg-turbo — so JPEG goes through libjpeg-turbo (TurboJPEG), as
//! in QCView, bit-identical to `djpeg`. Where QCView's loader was wrong the fix is written beside the code.
pub mod exr;
pub mod jpeg;
pub mod png;
pub mod tiff;
mod sys;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{0}")] Open(String),
    #[error("{0}: missing RGB channels (layer '{1}')")] NoRgb(String, String),
    #[error("{0}: {1}")] Decode(String, String),
}

/// The sample type of an [`Image`]'s planes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sample { /// IEEE half, linear light (EXR)
    F16, /// 8-bit code values
    U8, /// 16-bit code values
    U16 }
impl Sample { pub fn bytes(self) -> usize { match self { Sample::U8 => 1, _ => 2 } } }

/// What a frame is, when it is not a decoded picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sentinel { /// a frame number with no file on disk: transparent (QCView `MakeGapSentinel`)
    Gap, /// a file that would not decode: solid #787878 (QCView `MakeBrokenSentinel`)
    Broken }

/// A decoded image: planes in G, B, R [, A] order (FFmpeg's gbrp order — what `min_present`'s planar RGB path samples).
#[derive(Clone, Debug)]
pub struct Image { pub width: u32, pub height: u32, pub sample: Sample, pub planes: Vec<Vec<u8>>, /// bytes per row (256-aligned)
    pub pitch: u32, pub alpha: bool, pub sentinel: Option<Sentinel> }

/// Rows padded to 256 bytes (wgpu's `write_texture` bytes-per-row rule).
pub fn pitch_for(width: u32, sample: Sample) -> u32 { ((width as usize * sample.bytes()).div_ceil(256) * 256) as u32 }

/// Which loader a path goes to — QCView's `thumbnail_image_provider` / `media_extensions.h` table, case-insensitive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format { Exr, Png, Tiff, Jpeg }
impl Format {
    pub fn of(path: &str) -> Option<Format> {
        let ext = std::path::Path::new(path).extension()?.to_str()?.to_ascii_lowercase();
        Some(match ext.as_str() { "exr" => Format::Exr, "png" => Format::Png, "tif" | "tiff" => Format::Tiff, "jpg" | "jpeg" => Format::Jpeg, _ => return None })
    }
}

/// Decode any still the family reads. `layer` matters to EXR only (QCView's `loadFrame(path, layer, mode)`);
/// `threads` is QCView's latency-tier hint (`setDecodeThreads`): N > 0 parallelises ONE frame (seek target, first
/// frame), 0 = the caller's worker pool already runs frames in parallel.
pub fn load(path: &str, layer: &str, threads: usize) -> Result<Image, Error> {
    match Format::of(path) {
        Some(Format::Exr) => exr::load(path, layer, threads),
        Some(Format::Png) => png::load(path),
        Some(Format::Tiff) => tiff::load(path),
        Some(Format::Jpeg) => jpeg::load(path),
        None => Err(Error::Open(format!("{path}: not an image format min-image reads"))),
    }
}

/// Interleaved samples as a loader produced them, before [`Image::from_interleaved`] splits them into planes.
pub(crate) enum Samples<'a> { U8(&'a [u8]), /// 16-bit integers or IEEE half bits, host order
    U16(&'a [u16]) }

impl Image {
    /// Split interleaved samples into G, B, R [, A] planes. `color` = R, G, B in the first three channels (else one
    /// gray channel copied to all three); `alpha` = the LAST channel is alpha. `channels` = samples per pixel.
    pub(crate) fn from_interleaved(width: u32, height: u32, sample: Sample, data: Samples, channels: usize, color: bool, alpha: bool) -> Image {
        let pitch = pitch_for(width, sample);
        let (w, h, b) = (width as usize, height as usize, sample.bytes());
        let src_of = |plane: usize| -> usize { match plane { 3 => channels - 1, _ if !color => 0, 0 => 1, 1 => 2, _ => 0 } };
        let n = if alpha { 4 } else { 3 };
        let planes = (0..n).map(|pl| {
            let c = src_of(pl);
            let mut out = vec![0u8; pitch as usize * h];
            for y in 0..h {
                let row = &mut out[y * pitch as usize..y * pitch as usize + w * b];
                match &data {
                    Samples::U8(d) => { let s = &d[y * w * channels..]; for x in 0..w { row[x] = s[x * channels + c] } }
                    Samples::U16(d) => { let s = &d[y * w * channels..]; for x in 0..w { row[x * 2..x * 2 + 2].copy_from_slice(&s[x * channels + c].to_le_bytes()) } }
                }
            }
            out
        }).collect();
        Image { width, height, sample, planes, pitch, alpha, sentinel: None }
    }

    /// Mirror in place (TIFF orientations 2–4).
    pub(crate) fn flip(&mut self, horizontal: bool, vertical: bool) {
        let (w, h, b, p) = (self.width as usize, self.height as usize, self.sample.bytes(), self.pitch as usize);
        for plane in &mut self.planes {
            if vertical { for y in 0..h / 2 { let (top, bot) = plane.split_at_mut((h - 1 - y) * p); top[y * p..y * p + w * b].swap_with_slice(&mut bot[..w * b]) } }
            if horizontal { for y in 0..h { let row = &mut plane[y * p..y * p + w * b]; for x in 0..w / 2 { for k in 0..b { row.swap(x * b + k, (w - 1 - x) * b + k) } } } }
        }
    }

    /// QCView's sentinels at the source size: gap = transparent, broken = #787878 opaque. 8-bit planes.
    pub fn sentinel(kind: Sentinel, width: u32, height: u32) -> Image {
        let pitch = pitch_for(width, Sample::U8);
        let (v, a) = match kind { Sentinel::Gap => (0u8, 0u8), Sentinel::Broken => (0x78, 255) };
        let plane = |x: u8| vec![x; pitch as usize * height as usize];
        Image { width, height, sample: Sample::U8, planes: vec![plane(v), plane(v), plane(v), plane(a)], pitch, alpha: true, sentinel: Some(kind) }
    }
}
