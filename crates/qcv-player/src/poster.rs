//! A video frame's colour for a still picture of it - a poster, a thumbnail (lifted from minNotes' `video.rs`, canon
//! row 316, for the file browser's thumbnails, row 467: one implementation for the family). The decoder gives R'G'B'
//! code values; a small engine shows them tagged with the clip's transfer and primaries - the presenter's own
//! pipeline (row 216: one pipeline for captures and thumbnails) -, the result is captured, brought to SDR (BT.2390
//! for an HDR source) and encoded as sRGB - as QCView's thumbnails are (`apps/qcv/src/thumb_color.rs`, mirrored
//! here without its looks).
use crate::thumbnail_cache::{ThumbPixels, ThumbSamples};
use min_present::{engine::Engine, ColorMeta, Frame, Pixels, PlanarLayout, Plane};

/// The colour stage, on the thread that owns the device (it draws on the GPU and reads back: once per picture).
#[derive(Default)]
pub struct Poster { engine: Option<Engine> }
impl Poster {
    /// A stage with an engine of its own and no window (a helper process). Without a GPU it still converts - the code
    /// values as stored.
    pub fn headless() -> Poster { Poster { engine: Engine::headless(wgpu::TextureFormat::Rgba16Float).ok() } }
    pub fn has_engine(&self) -> bool { self.engine.is_some() }
    /// Straight RGBA, 8 bits, sRGB. Without a GPU engine: the code values as stored (right for an SDR clip).
    pub fn convert(&mut self, device: Option<(&wgpu::Device, &wgpu::Queue)>, px: &ThumbPixels, meta: &ColorMeta) -> (Vec<u8>, u32, u32) {
        match self.sdr_rgb(device, px, meta) { Some((rgb, w, h)) => (rgb.chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(), w, h), None => (px.rgba8(), px.width, px.height) }
    }
    /// The same as RGB, 8 bits, sRGB; None without a GPU engine (a video made of such frames, canon row 343, is not
    /// made of code values as stored: for an HDR picture they would be wrong).
    pub fn sdr_rgb(&mut self, device: Option<(&wgpu::Device, &wgpu::Queue)>, px: &ThumbPixels, meta: &ColorMeta) -> Option<(Vec<u8>, u32, u32)> {
        if self.engine.is_none() { if let Some((d, q)) = device { self.engine = Some(Engine::new(d.clone(), q.clone(), wgpu::TextureFormat::Rgba16Float)); } }
        let en = self.engine.as_mut()?;
        let (w, h) = (px.width, px.height); if w == 0 || h == 0 { return None }
        let (data, float) = match &px.samples { ThumbSamples::Code16(v) => (v, false), ThumbSamples::Half(v) => (v, true) };
        let pitch = (w * 2).div_ceil(256) * 256;
        let mut planes = [vec![0u8; (pitch * h) as usize], vec![0u8; (pitch * h) as usize], vec![0u8; (pitch * h) as usize]];
        for y in 0..h { for x in 0..w { let i = ((y * w + x) * 3) as usize; let o = (y * pitch + x * 2) as usize;
            for (pl, c) in [(0usize, 1usize), (1, 2), (2, 0)] { planes[pl][o..o + 2].copy_from_slice(&data[i + c].to_ne_bytes()); } } }
        let m = ColorMeta { matrix: min_present::Matrix::Identity, range: min_present::Range::Full, chroma: min_present::Chroma::Rgb, bit_depth: 16, rotation: 0, sar: (1, 1), range_forced: false, ..*meta };
        let [g, b, r] = planes;
        let frame = Frame { width: w, height: h, pixels: Pixels::Planar { planes: vec![Plane::owned(g, w, h, pitch), Plane::owned(b, w, h, pitch), Plane::owned(r, w, h, pitch)], layout: PlanarLayout::Rgb { bytes_per_sample: 2, code_bits: 16, float, alpha: false, legal_expand: false } }, meta: m };
        en.show(&frame).ok()?;
        let peak = en.bypass_hdr_peak();
        let (mut lin, cw, ch) = en.capture(false).ok()?;
        min_present::capture::to_sdr(&mut lin, peak);
        Some((min_present::capture::srgb8(&lin), cw, ch))
    }
}
