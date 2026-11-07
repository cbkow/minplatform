//! Legal-range (16–235) → full-range expansion for RGB-family sources — ported from `QCView-Player/src/decode/rgb_range.h`.
//!
//! swscale applies no range handling to an RGB→RGB conversion, so every place that turns an RGB frame into pixels has to
//! do the expansion itself — otherwise legal-range RGB masters (Avid DNxHR 444 RGB, MXF RGBA tagged 64/940) come up flat.
//! One rule for all of them: override Full → never; override Limited → always; Auto → when the frame is tagged limited.
//! DIVERGENCE (same rule, different place): QCView expands on the CPU with a LUT over RGBA8 / RGBA64; the platform keeps
//! the planes at native depth and expands in stage 1 with the same constants (`(code − 16·K) / (219·K)`).
use ffmpeg_sys_next as ff;

/// The Range pill: 0 = Auto, 1 = Full, 2 = Limited (`MediaItem` in QCView).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RangeOverride { #[default] Auto, Full, Limited }

pub fn is_rgb_pixel_format(fmt: ff::AVPixelFormat) -> bool {
    let d = unsafe { ff::av_pix_fmt_desc_get(fmt) };
    !d.is_null() && (unsafe { (*d).flags } as i32 & ff::AV_PIX_FMT_FLAG_RGB) != 0
}
/// Whether an RGB frame's levels must be expanded 16–235 → 0–255 before display.
pub unsafe fn rgb_frame_needs_legal_expansion(frame: *const ff::AVFrame, over: RangeOverride) -> bool {
    if frame.is_null() { return false }
    let f = unsafe { &*frame };
    if !is_rgb_pixel_format(unsafe { std::mem::transmute::<i32, ff::AVPixelFormat>(f.format) }) { return false }
    match over { RangeOverride::Full => false, RangeOverride::Limited => true, RangeOverride::Auto => f.color_range == ff::AVColorRange::AVCOL_RANGE_MPEG }
}
/// Phase J.1: sources deeper than 8 bits (10/12-bit YUV, 16-bit RGB, 16-bit Bayer) keep their precision — the platform's
/// planes are UNORM16, never half (11 significant bits would lose precision near white); RGBA16F stays with EXR.
pub fn source_needs_16bit_publish(fmt: ff::AVPixelFormat) -> bool {
    let d = unsafe { ff::av_pix_fmt_desc_get(fmt) };
    if d.is_null() { return false }
    let d = unsafe { &*d };
    if (d.flags as i32 & ff::AV_PIX_FMT_FLAG_BAYER) != 0 { return true }
    (0..d.nb_components as usize).any(|i| d.comp[i].depth > 8)
}

/// `expandRgba8LegalToFull`: 16–235 → 0–255 on packed RGBA8 (alpha untouched), rounded as QCView's LUT does.
pub fn expand_rgba8_legal_to_full(rgba: &mut [u8]) {
    let lut: [u8; 256] = std::array::from_fn(|v| (((v as i32 - 16) * 255 + 109) / 219).clamp(0, 255) as u8);
    for px in rgba.chunks_exact_mut(4) { px[0] = lut[px[0] as usize]; px[1] = lut[px[1] as usize]; px[2] = lut[px[2] as usize]; }
}
