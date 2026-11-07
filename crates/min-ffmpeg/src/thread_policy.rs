//! Software-decode threading policy shared by every decoder that opens an `AVCodecContext` — ported from
//! `QCView-Player/src/decode/thread_policy.h` (2026-09-09 measurements, vendored FFmpeg 9.0.1 CLI, 32-thread box, ms):
//!
//! ```text
//!   ProRes RAW 4224x3024   frame 4815  slice 323   first frame 1823 → 118
//!   DNxHR 444 12-bit 1080p frame  171  slice 118
//!   ProRes 422 HQ 1080p    frame  104  slice  75
//!   ProRes 4444 720p       frame  102  slice  76
//!   APV 4:2:2 10-bit 1080p frame   94  slice  72
//!   FFV1 1080p (1 slice)   frame  225  slice 186
//! ```
//! Frame threading pipelines N frames before the first one comes out (a full second of black on 4K RAW) and, for these
//! intra codecs, is slower in steady state too — every worker carries a private codec context and the per-frame work is
//! already slice-parallel. Inter codecs (H.264 / HEVC / AV1 / VP9 / VVC in software) keep frame threading: their slices
//! are few and the inter-frame dependency is what frame threads pipeline.
use ffmpeg_sys_next as ff;

/// `user_threads` — 0 = FFmpeg auto (core count), else the user's performance/ffmpegThreads value. Call before `avcodec_open2`.
pub unsafe fn apply_software_thread_policy(ctx: *mut ff::AVCodecContext, codec: *const ff::AVCodec, user_threads: i32) {
    if ctx.is_null() || codec.is_null() { return }
    unsafe {
        let desc = ff::avcodec_descriptor_get((*codec).id);
        let intra_only = !desc.is_null() && ((*desc).props & ff::AV_CODEC_PROP_INTRA_ONLY) != 0;
        let slice_cap = ((*codec).capabilities as u32 & ff::AV_CODEC_CAP_SLICE_THREADS) != 0;
        (*ctx).thread_count = if user_threads > 0 { user_threads } else { 0 };
        (*ctx).thread_type = if intra_only && slice_cap { ff::FF_THREAD_SLICE as i32 } else { (ff::FF_THREAD_FRAME | ff::FF_THREAD_SLICE) as i32 };
    }
}
/// Dual view runs two software decoders at once; give each half the machine (2026-09-09: the 8K ProRes XQ clip decodes
/// in the same 33 ms/frame on 16 slice threads as on 32, so the split costs nothing and removes the contention).
pub fn dual_side_thread_count() -> i32 { let n = unsafe { ff::av_cpu_count() }; if n > 1 { n / 2 } else { 1 } }
pub fn thread_policy_name(ctx: *const ff::AVCodecContext) -> &'static str {
    if ctx.is_null() { return "?" }
    if unsafe { (*ctx).thread_type } == ff::FF_THREAD_SLICE as i32 { "slice" } else { "frame+slice" }
}
