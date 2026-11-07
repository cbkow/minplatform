//! Clean YUV (colour plan, stage 1) — ported from `QCView-Player/src/decode/yuv_planar.{h,cpp}` and the matrix / range
//! rules of `sws_threaded.h`. Software-decoded YUV frames reach the renderer as planes and convert on the GPU,
//! UNCLAMPED: swscale's RGBA output clipped super-whites and sub-blacks before the colour chain saw them (a DNxHR HQX
//! range chart read 100 % where the same codes through VideoToolbox read 109 %).
//!
//! The maths is the levels maths in code values, generalised to any bit depth and alignment:
//! ```text
//!   code    = sample (0..1 UNORM) × codeMax          (255 or 65535)
//!   limited : Y = (code − 16·K) / (219·K),  C = (code − 128·K) / (224·K)
//!   full    : Y = code / fullMax,           C = (code − chromaMid) / fullMax
//!   R = Y + 2(1−Kr)·Cr,  B = Y + 2(1−Kb)·Cb,  G = (Y − Kr·R − Kb·B) / (1−Kr−Kb)
//! ```
//! with K = 2^(codeBits − 8), codeBits = depth + shift (10 for LSB-aligned yuv422p10le, 16 for MSB-aligned P010).
use crate::rgb_range::RangeOverride;
use ffmpeg_sys_next as ff;
use min_present::{Matrix, Range};

/// Layout of a supported format, independent of frame size.
#[derive(Clone, Copy, Debug, Default)]
pub struct Layout { pub ok: bool, pub plane_count: usize, pub interleaved: bool, pub alpha: bool, /// planar RGB (FFmpeg gbrp*: G, B, R [, A] planes)
    pub rgb: bool, pub float: bool, pub bytes: u32, pub code_bits: u32, pub shift_x: u32, pub shift_y: u32, pub data_index: [usize; 4] }

/// True for the YUV layouts the GPU converts: 8–16-bit, 4:2:0 / 4:2:2 / 4:4:4, planar with optional alpha, or NV12-style
/// interleaved chroma; little-endian, not float. Planar RGB (gbrp*, 8–16-bit and half float) is added for the platform
/// (QCView converts RGB through swscale + `rgb_range.h`). Everything else is unsupported here.
pub fn layout_for(fmt: ff::AVPixelFormat) -> Layout {
    let mut l = Layout { data_index: [0, 1, 2, 3], ..Default::default() };
    let d = unsafe { ff::av_pix_fmt_desc_get(fmt) };
    if d.is_null() { return l }
    let d = unsafe { &*d };
    let flags = d.flags as i32;
    if (flags & (ff::AV_PIX_FMT_FLAG_PAL | ff::AV_PIX_FMT_FLAG_HWACCEL | ff::AV_PIX_FMT_FLAG_BAYER | ff::AV_PIX_FMT_FLAG_BE)) != 0 { return l }
    if d.nb_components != 3 && d.nb_components != 4 { return l }
    if d.log2_chroma_w > 1 || d.log2_chroma_h > 1 { return l }
    let rgb = (flags & ff::AV_PIX_FMT_FLAG_RGB) != 0;
    let float = (flags & ff::AV_PIX_FMT_FLAG_FLOAT) != 0;
    if float && !(rgb && d.comp[0].depth == 16) { return l }   // half-float planar RGB (EXR) only
    let (depth, shift) = (d.comp[0].depth as u32, d.comp[0].shift as u32);
    for c in 1..d.nb_components as usize { if d.comp[c].depth as u32 != depth || d.comp[c].shift as u32 != shift { return l } }
    if depth < 8 || depth + shift > 16 { return l }
    let bytes = if depth + shift > 8 { 2 } else { 1 };
    let alpha = d.nb_components == 4;
    if rgb {
        // planar only, one sample per step, each component alone in its plane
        for c in 0..d.nb_components as usize { if d.comp[c].step as u32 != bytes || d.comp[c].offset != 0 { return l } }
        let planes: Vec<usize> = (0..d.nb_components as usize).map(|c| d.comp[c].plane as usize).collect();
        if planes.iter().enumerate().any(|(i, p)| planes.iter().position(|q| q == p) != Some(i)) { return l }
        l.plane_count = if alpha { 4 } else { 3 };
        // FFmpeg's gbrp plane order is G=0, B=1, R=2 (comp[0] = R lives in plane 2): keep the frame's own order, G, B, R
        l.data_index = [0, 1, 2, 3];
        l.rgb = true; l.float = float; l.alpha = alpha; l.bytes = bytes; l.code_bits = depth + shift; l.ok = true;
        return l;
    }
    if d.comp[0].plane != 0 || d.comp[0].offset != 0 || d.comp[0].step as u32 != bytes { return l }
    if d.comp[1].plane != d.comp[2].plane {
        if d.comp[1].step as u32 != bytes || d.comp[2].step as u32 != bytes || d.comp[1].offset != 0 || d.comp[2].offset != 0 { return l }
        l.plane_count = if alpha { 4 } else { 3 };
        l.data_index[1] = d.comp[1].plane as usize; l.data_index[2] = d.comp[2].plane as usize;
    } else {
        // interleaved chroma in one plane, U first (NV12 / P010 / P016; NV21's V-first order stays unsupported)
        if d.comp[1].step as u32 != 2 * bytes || d.comp[1].offset != 0 || d.comp[2].offset as u32 != bytes { return l }
        l.plane_count = if alpha { 3 } else { 2 };
        l.interleaved = true;
        l.data_index[1] = d.comp[1].plane as usize;
    }
    if alpha {
        if d.comp[3].step as u32 != bytes || d.comp[3].offset != 0 { return l }
        l.data_index[l.plane_count - 1] = d.comp[3].plane as usize;
    }
    l.alpha = alpha; l.bytes = bytes; l.code_bits = depth + shift; l.shift_x = d.log2_chroma_w as u32; l.shift_y = d.log2_chroma_h as u32; l.ok = true;
    l
}
pub fn yuv_planar_format_supported(fmt: ff::AVPixelFormat) -> bool { layout_for(fmt).ok }

/// YUV→RGB matrix for a decoded frame: the tagged one when known, else BT.709 — untagged → BT.709 at EVERY size, on every
/// decode path (`sws_threaded.h`: "the GPU converters use the same rule"; tagged 601 still decodes as 601).
pub fn effective_matrix(colorspace: ff::AVColorSpace) -> Matrix {
    use ff::AVColorSpace as S;
    match colorspace {
        S::AVCOL_SPC_BT709 => Matrix::Bt709,
        S::AVCOL_SPC_BT470BG | S::AVCOL_SPC_SMPTE170M => Matrix::Bt601,
        S::AVCOL_SPC_SMPTE240M => Matrix::Smpte240m,
        S::AVCOL_SPC_FCC => Matrix::Fcc,
        S::AVCOL_SPC_BT2020_NCL | S::AVCOL_SPC_BT2020_CL => Matrix::Bt2020,
        _ => Matrix::Bt709,
    }
}
/// Source range fed to the conversion: the Range pill wins; Auto = the frame's tag, untagged → limited. RGB sources
/// always declare FULL here — the legal→full expansion for RGB is `rgb_range`'s one rule.
pub fn effective_source_range(color_range: ff::AVColorRange, is_rgb: bool, over: RangeOverride) -> Range {
    if is_rgb { return Range::Full }
    match over { RangeOverride::Full => Range::Full, RangeOverride::Limited => Range::Limited, RangeOverride::Auto => if color_range == ff::AVColorRange::AVCOL_RANGE_JPEG { Range::Full } else { Range::Limited } }
}
/// swscale's coefficient table (`SWS_CS_*`) as Kr / Kb.
pub fn kr_kb(m: Matrix) -> (f32, f32) {
    match m { Matrix::Bt601 => (0.299, 0.114), Matrix::Smpte240m => (0.212, 0.087), Matrix::Fcc => (0.30, 0.11), Matrix::Bt2020 => (0.2627, 0.0593), Matrix::Bt709 | Matrix::Identity => (0.2126, 0.0722) }
}
/// The shader's maths on the CPU, from stored integer codes (tests, probes).
pub fn yuv_planar_to_rgb(code_bits: u32, full_range: bool, kr: f32, kb: f32, y_code: f32, u_code: f32, v_code: f32) -> [f32; 3] {
    let k = (1u32 << (code_bits - 8)) as f32; let full_max = ((1u32 << code_bits) - 1) as f32; let mid = (1u32 << (code_bits - 1)) as f32;
    let (y, u, v) = if full_range { (y_code / full_max, (u_code - mid) / full_max, (v_code - mid) / full_max) } else { ((y_code - 16.0 * k) / (219.0 * k), (u_code - 128.0 * k) / (224.0 * k), (v_code - 128.0 * k) / (224.0 * k)) };
    let r = y + 2.0 * (1.0 - kr) * v; let b = y + 2.0 * (1.0 - kb) * u; let g = (y - kr * r - kb * b) / (1.0 - kr - kb);
    [r, g, b]
}

#[cfg(test)]
mod tests {
    use super::*;
    use ff::AVPixelFormat as P;
    #[test] fn untagged_is_bt709_at_every_size() { assert_eq!(effective_matrix(ff::AVColorSpace::AVCOL_SPC_UNSPECIFIED), Matrix::Bt709); assert_eq!(effective_matrix(ff::AVColorSpace::AVCOL_SPC_SMPTE170M), Matrix::Bt601); assert_eq!(effective_matrix(ff::AVColorSpace::AVCOL_SPC_BT2020_NCL), Matrix::Bt2020); }
    #[test] fn layouts() {
        let l = layout_for(P::AV_PIX_FMT_YUV422P10LE); assert!(l.ok && l.plane_count == 3 && !l.interleaved && l.bytes == 2 && l.code_bits == 10 && l.shift_x == 1 && l.shift_y == 0);
        let l = layout_for(P::AV_PIX_FMT_YUVA444P12LE); assert!(l.ok && l.plane_count == 4 && l.alpha && l.code_bits == 12 && l.data_index[3] == 3);
        let l = layout_for(P::AV_PIX_FMT_NV12); assert!(l.ok && l.interleaved && l.plane_count == 2 && l.code_bits == 8);
        let l = layout_for(P::AV_PIX_FMT_P010LE); assert!(l.ok && l.interleaved && l.code_bits == 16 && l.bytes == 2);
        let l = layout_for(P::AV_PIX_FMT_GBRPF16LE); assert!(l.ok && l.rgb && l.float && l.plane_count == 3);
        let l = layout_for(P::AV_PIX_FMT_GBRP10LE); assert!(l.ok && l.rgb && !l.float && l.code_bits == 10);
        assert!(!layout_for(P::AV_PIX_FMT_NV21).ok && !layout_for(P::AV_PIX_FMT_YUV422P10BE).ok && !layout_for(P::AV_PIX_FMT_RGBA).ok && !layout_for(P::AV_PIX_FMT_YUV411P).ok);
    }
    #[test] fn levels_match_qcview_constants() {
        // 10-bit LSB-aligned: 64 → 0, 940 → 1; chroma 512 neutral, 960 → +0.5; 16-bit MSB-aligned (P010): 4096 / 60160 / 32768
        let (kr, kb) = kr_kb(Matrix::Bt709);
        let c = yuv_planar_to_rgb(10, false, kr, kb, 64.0, 512.0, 512.0); assert!(c.iter().all(|v| v.abs() < 1e-5), "{c:?}");
        let c = yuv_planar_to_rgb(10, false, kr, kb, 940.0, 512.0, 512.0); assert!(c.iter().all(|v| (v - 1.0).abs() < 1e-5), "{c:?}");
        let c = yuv_planar_to_rgb(16, false, kr, kb, 60160.0, 32768.0, 32768.0); assert!(c.iter().all(|v| (v - 1.0).abs() < 1e-5), "{c:?}");
        let c = yuv_planar_to_rgb(8, false, kr, kb, 250.0, 128.0, 128.0); assert!(c[0] > 1.0, "super-white survives: {c:?}");
        let c = yuv_planar_to_rgb(8, true, kr, kb, 255.0, 128.0, 128.0); assert!(c.iter().all(|v| (v - 1.0).abs() < 2e-3), "{c:?}");
    }
}
