//! Per-codec hardware-decode routing rules shared by every `get_format` callback — ported from
//! `QCView-Player/src/decode/hw_routing.h` and the `hwaccelGetFormat` in `video_decoder.cpp`.
//!
//! ProRes RAW (2026-09-09): FFmpeg 9.0 registers `prores_raw_videotoolbox` and `prores_raw_vulkan` hwaccels. Neither is
//! usable yet — VideoToolbox returns a null image buffer for every frame (-12905) on an M5 Max / macOS 26.6.2 with an
//! iPhone 17 Pro RAW HQ clip, and the Vulkan hwaccel's output is flipped and range-shifted upstream. Software decode +
//! swscale debayer runs at ~30 ms/frame for 4K with slice threads (thread_policy).
//!
//! Windows routes per codec as QCView does (`vulkanPreferredCodec`): the compute-decoded intra codecs — ProRes, FFV1, APV —
//! take FFmpeg's Vulkan device (`OpenOptions::hw_intra`, S6, canon row 149: GPU ProRes on Windows is REQUIRED); the inter
//! codecs take D3D12VA on the presenter's own device (S1: zero-copy, 0.27 ms/frame) where QCView used D3D11VA. The
//! `get_format` callback reads the device the codec context carries, so one callback serves every device.
use crate::hw;
use ffmpeg_sys_next as ff;

/// True when no hwaccel may be selected for this codec — `get_format` must fall straight through to the first software format.
pub fn software_only_codec(id: ff::AVCodecID) -> bool { id == ff::AVCodecID::AV_CODEC_ID_PRORES_RAW }

/// QCView `vulkanPreferredCodec`: the intra codecs FFmpeg decodes on Vulkan compute — the ones that get the Vulkan device on
/// Windows (on macOS VideoToolbox already takes ProRes, so `hw_intra` is None there and this never selects anything).
pub fn vulkan_preferred_codec(id: ff::AVCodecID) -> bool { matches!(id, ff::AVCodecID::AV_CODEC_ID_PRORES | ff::AVCodecID::AV_CODEC_ID_FFV1 | ff::AVCodecID::AV_CODEC_ID_APV) }

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

/// FFmpeg's pixfmt-selection callback: the attached device's hwaccel format when the codec offers it, else the first software
/// format. Set as `get_format` only when a hw device context was attached — otherwise FFmpeg uses its default selection.
pub unsafe extern "C" fn hwaccel_get_format(ctx: *mut ff::AVCodecContext, fmts: *const ff::AVPixelFormat) -> ff::AVPixelFormat {
    unsafe {
        if !ctx.is_null() && software_only_codec((*ctx).codec_id) {
            let sw = first_software_format(fmts);
            log::info!("get_format: software-only codec, hwaccels skipped → {:?}", sw);
            return sw;
        }
        let want = if ctx.is_null() { None } else { hw::device_pix_fmt((*ctx).hw_device_ctx) };
        if let Some(want) = want { let mut p = fmts; while *p != ff::AVPixelFormat::AV_PIX_FMT_NONE { if *p == want { return *p } p = p.add(1); } }
        let sw = first_software_format(fmts);
        log::info!("get_format: codec has no {:?} → {:?} (SW fallback)", want, sw);
        sw
    }
}
