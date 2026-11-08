//! `min-image` — ONE image decoding stack for the family (canon §10.4, rows 34 / 148; Phase 2 step 2c). QCView's loaders
//! (`decode/{exr,png,tiff,jpeg}_image_loader`, `image_loader.h`) carried: an [`Image`] is the decoded frame as planar planes
//! in the presenter's RGB order (G, B, R [, A]), rows padded to 256 bytes; EXR arrives as half floats (linear light),
//! the 8 / 16-bit formats as integers. Gap / broken sentinels stand in for missing / undecodable frames (QCView's
//! `MakeGapSentinel` / `MakeBrokenSentinel`).
pub mod exr;
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

/// QCView: files smaller than this are truncated / corrupt — the loader does not even open them.
pub const MIN_FILE_BYTES: u64 = 512;

impl Image {
    /// QCView's sentinels at the source size: gap = transparent, broken = #787878 opaque. 8-bit planes.
    pub fn sentinel(kind: Sentinel, width: u32, height: u32) -> Image {
        let pitch = pitch_for(width, Sample::U8);
        let (v, a) = match kind { Sentinel::Gap => (0u8, 0u8), Sentinel::Broken => (0x78, 255) };
        let plane = |x: u8| vec![x; pitch as usize * height as usize];
        Image { width, height, sample: Sample::U8, planes: vec![plane(v), plane(v), plane(v), plane(a)], pitch, alpha: true, sentinel: Some(kind) }
    }
}
