//! The decoded-frame type and its colour metadata (canon §11.1, row 39: carried end to end from day one).
use std::{any::Any, ffi::c_void, sync::Arc};

/// The YUV→RGB matrix (swscale's `SWS_CS_*` table, as QCView's `yuv_planar.h` carries it: Kr / Kb).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)] pub enum Matrix { Bt601, #[default] Bt709, Bt2020, Smpte240m, Fcc, /// RGB sources (EXR, planar RGB): no matrix
    Identity }
impl Matrix {
    /// Kr / Kb — `R = Y + 2(1−Kr)·Cr, B = Y + 2(1−Kb)·Cb, G = (Y − Kr·R − Kb·B) / (1−Kr−Kb)`; 601 / 709 / 2020 / 240M / FCC decode as swscale did.
    pub fn kr_kb(self) -> (f32, f32) { match self { Matrix::Bt601 => (0.299, 0.114), Matrix::Smpte240m => (0.212, 0.087), Matrix::Fcc => (0.30, 0.11), Matrix::Bt2020 => (0.2627, 0.0593), Matrix::Bt709 | Matrix::Identity => (0.2126, 0.0722) } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)] pub enum Range { #[default] Limited, Full }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)] pub enum Transfer { Srgb, #[default] Bt1886, Pq, Hlg, Linear }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)] pub enum Primaries { #[default] Bt709, P3D65, Bt2020 }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)] pub enum Chroma { C420, #[default] C422, C444, Rgb }
/// Mastering display + content light levels when the container has them (HDR10 static metadata).
#[derive(Clone, Copy, Debug, PartialEq)] pub struct Mastering { pub max_cll: u32, pub max_fall: u32, pub max_luminance: f32, pub min_luminance: f32 }

/// What stage 1 needs to reach linear working RGB, and what stage 3 needs to know about the source.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ColorMeta {
    pub matrix: Matrix, pub range: Range, pub transfer: Transfer, pub primaries: Primaries,
    pub bit_depth: u32, pub chroma: Chroma,
    /// `range` was set by the user's Range override, not detected: the import's own attachment (a CVPixelBuffer's
    /// range key) must not overrule it (the Inspector's Range pill had no effect on zero-copy frames, 2026-11-07).
    pub range_forced: bool,
    /// Quarter-turn rotation in degrees (0 / 90 / 180 / 270) from the container's display matrix.
    pub rotation: u16,
    /// Sample aspect ratio; (1, 1) for square pixels.
    pub sar: (u32, u32),
    pub mastering: Option<Mastering>,
}
impl ColorMeta {
    /// The shader's transfer / matrix / primaries selectors (one float each — the uniform block is tiny and proven).
    pub(crate) fn selectors(&self) -> (f32, f32, f32) {
        (match self.transfer { Transfer::Srgb => 0.0, Transfer::Bt1886 => 1.0, Transfer::Pq => 2.0, Transfer::Hlg => 3.0, Transfer::Linear => 4.0 },
         match self.matrix { Matrix::Bt601 | Matrix::Smpte240m | Matrix::Fcc => 0.0, Matrix::Bt709 => 1.0, Matrix::Bt2020 => 2.0, Matrix::Identity => 3.0 },   // the shader takes Kr / Kb, not this selector
         match self.primaries { Primaries::Bt709 => 0.0, Primaries::P3D65 => 1.0, Primaries::Bt2020 => 2.0 })
    }
}

/// The bytes behind a CPU plane: the decoder's own buffer, or one SHARED with a cache — an image sequence's frame stays
/// in the sequence cache while it is shown, and the presenter reads it there (no 50-MB copy per 4K EXR frame).
pub type PlaneBytes = Arc<dyn AsRef<[u8]> + Send + Sync>;
/// One CPU plane, rows padded to a 256-byte pitch (wgpu's `write_texture` requirement) by whoever decoded it.
pub struct Plane { pub data: PlaneBytes, pub width: u32, pub height: u32, pub pitch: u32 }
impl Plane {
    pub fn owned(data: Vec<u8>, width: u32, height: u32, pitch: u32) -> Plane { Plane { data: Arc::new(data), width, height, pitch } }
    pub fn bytes(&self) -> &[u8] { (*self.data).as_ref() }
}

/// Plane layouts (QCView's `YuvPlanarDesc`): levels are computed in CODE values — `code_bits` = depth + shift (10 for
/// LSB-aligned yuv422p10le, 16 for MSB-aligned P010 / VideoToolbox 10-bit), `bytes_per_sample` 1 or 2 (R8 / R16 textures).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanarLayout {
    /// Y, Cb, Cr [, A] planes — or Y, interleaved CbCr [, A] (`interleaved_chroma`: NV12 / P010 style, U first).
    Ycc { bytes_per_sample: u32, code_bits: u32, interleaved_chroma: bool, alpha: bool },
    /// Planar RGB in FFmpeg's `gbrp` order (G, B, R [, A]); `float` = half floats, linear light (EXR); `legal_expand` =
    /// `rgb_range`'s 16–235 → 0–255 rule for legal-range RGB masters. The matrix is `Identity`.
    Rgb { bytes_per_sample: u32, code_bits: u32, float: bool, alpha: bool, legal_expand: bool },
}

/// A platform frame the GPU can sample without a copy. `handle` is the OS object (macOS: `CVPixelBufferRef`;
/// Windows: a `Dx12FrameHandle`); `keep` owns whatever keeps it alive (the decoder's frame reference) until the
/// presenter has released its ring slot.
pub struct NativeFrame { pub handle: *mut c_void, pub keep: Arc<dyn Any + Send + Sync> }
// SAFETY: the handle is only dereferenced on the presenter's thread; `keep` is what the decoder promised is thread-safe.
unsafe impl Send for NativeFrame {}
unsafe impl Sync for NativeFrame {}

/// Windows: what a D3D12VA frame hands the presenter (the producer fills it from `AVD3D12VAFrame` and keeps the AVFrame).
#[repr(C)] #[derive(Clone, Copy, Debug)]
/// A D3D12VA frame (`resource` = the NV12 / P010 texture, `plane_count` 0) or — S6 — the Vulkan bridge's plane set (`planes[..plane_count]`,
/// one single-plane shared texture each, `interleaved_chroma` when plane 1 is CbCr, `code_bits` the stored depth: 10 / 12 LSB-aligned in
/// 16-bit textures, 8 in 8-bit). `fence` / `fence_value`: the presenter waits before sampling.
pub struct Dx12FrameHandle { pub resource: *mut c_void, pub fence: *mut c_void, pub fence_value: u64, pub planes: [*mut c_void; 4], pub plane_count: u8, pub interleaved_chroma: bool, pub code_bits: u8 }

pub enum Pixels { Planar { planes: Vec<Plane>, layout: PlanarLayout }, Native(NativeFrame) }

pub struct Frame { pub width: u32, pub height: u32, pub pixels: Pixels, pub meta: ColorMeta }
impl Frame {
    pub fn is_native(&self) -> bool { matches!(self.pixels, Pixels::Native(_)) }
}
