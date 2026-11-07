//! Per-codec hardware-decode routing rules shared by every `get_format` callback — ported from
//! `QCView-Player/src/decode/hw_routing.h` and the `hwaccelGetFormat` in `video_decoder.cpp`.
//!
//! ProRes RAW (2026-09-09): FFmpeg 9.0 registers `prores_raw_videotoolbox` and `prores_raw_vulkan` hwaccels. Neither is
//! usable yet — VideoToolbox returns a null image buffer for every frame (-12905) on an M5 Max / macOS 26.6.2 with an
//! iPhone 17 Pro RAW HQ clip, and the Vulkan hwaccel's output is flipped and range-shifted upstream. Software decode +
//! swscale debayer runs at ~30 ms/frame for 4K with slice threads (thread_policy).
//!
//! DIVERGENCE (row 45, canon §12.3): QCView routes Windows per codec to Vulkan (ProRes, FFV1, APV) or D3D11VA; wgpu has no
//! D3D11 backend, so the platform's Windows hwaccel is D3D12VA on the presenter's own device (S1 Windows: zero-copy,
//! 0.27 ms/frame); ProRes stays software on NVIDIA until the Vulkan-video route is measured.
use crate::hw::HW_PIX_FMT;
use ffmpeg_sys_next as ff;

/// True when no hwaccel may be selected for this codec — `get_format` must fall straight through to the first software format.
pub fn software_only_codec(id: ff::AVCodecID) -> bool { id == ff::AVCodecID::AV_CODEC_ID_PRORES_RAW }

/// The first SOFTWARE format in a `get_format` list (`fmts[0]` may be a foreign hwaccel such as `vaapi` for VVC).
pub unsafe fn first_software_format(fmts: *const ff::AVPixelFormat) -> ff::AVPixelFormat {
    unsafe {
        let mut p = fmts;
        while *p != ff::AVPixelFormat::AV_PIX_FMT_NONE {
            let d = ff::av_pix_fmt_desc_get(*p);
            if !d.is_null() && ((*d).flags as i32 & ff::AV_PIX_FMT_FLAG_HWACCEL) == 0 { return *p }
            p = p.add(1);
        }
        *fmts
    }
}

/// FFmpeg's pixfmt-selection callback: the platform hwaccel format when the codec offers it, else the first software format.
/// Set as `get_format` only when a hw device context was attached — otherwise FFmpeg uses its default selection.
pub unsafe extern "C" fn hwaccel_get_format(ctx: *mut ff::AVCodecContext, fmts: *const ff::AVPixelFormat) -> ff::AVPixelFormat {
    unsafe {
        if !ctx.is_null() && software_only_codec((*ctx).codec_id) {
            let sw = first_software_format(fmts);
            log::info!("get_format: software-only codec, hwaccels skipped → {:?}", sw);
            return sw;
        }
        let mut p = fmts;
        while *p != ff::AVPixelFormat::AV_PIX_FMT_NONE { if *p == HW_PIX_FMT { return *p } p = p.add(1); }
        let sw = first_software_format(fmts);
        log::info!("get_format: codec has no {:?} → {:?} (SW fallback)", HW_PIX_FMT, sw);
        sw
    }
}
