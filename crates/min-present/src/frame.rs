//! The decoded-frame type and its colour metadata (canon §11.1, row 39: carried end to end from day one).
use std::{any::Any, ffi::c_void, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)] pub enum Matrix { Bt601, #[default] Bt709, Bt2020, /// RGB sources (EXR, planar RGB): no matrix
    Identity }
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
         match self.matrix { Matrix::Bt601 => 0.0, Matrix::Bt709 => 1.0, Matrix::Bt2020 => 2.0, Matrix::Identity => 3.0 },
         match self.primaries { Primaries::Bt709 => 0.0, Primaries::P3D65 => 1.0, Primaries::Bt2020 => 2.0 })
    }
}

/// One CPU plane, rows padded to a 256-byte pitch (wgpu's `write_texture` requirement) by whoever decoded it.
pub struct Plane { pub data: Vec<u8>, pub width: u32, pub height: u32, pub pitch: u32 }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanarLayout {
    /// Y, Cb, Cr planes; `bytes_per_sample` 1 (8-bit) or 2 (10/12-bit, right-justified in 16 — the shader rescales).
    Ycc { bytes_per_sample: u32 },
    /// Planar RGB half floats in FFmpeg's `gbrp` order (G, B, R), linear light (EXR); the matrix is `Identity`.
    RgbF16,
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
pub struct Dx12FrameHandle { pub resource: *mut c_void, pub fence: *mut c_void, pub fence_value: u64 }

pub enum Pixels { Planar { planes: Vec<Plane>, layout: PlanarLayout }, Native(NativeFrame) }

pub struct Frame { pub width: u32, pub height: u32, pub pixels: Pixels, pub meta: ColorMeta }
impl Frame {
    pub fn is_native(&self) -> bool { matches!(self.pixels, Pixels::Native(_)) }
}
