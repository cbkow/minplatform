//! Seeking for demuxers whose `av_seek_frame()` leaves them unable to deliver another packet — ported from
//! `QCView-Player/src/decode/seek_compat.h` (2026-09-09).
//!
//! FFmpeg 9.0's animated-WebP demuxer (`webp_anim`) accepts a seek and then returns end-of-stream for every subsequent
//! read. Every decoder here seeks to frame 0 right after opening, so an animated WebP opened, counted its frames and then
//! never showed one. GIF and APNG (`AVFMT_GENERIC_INDEX`) seek fine. `seek_stream` is a drop-in for `av_seek_frame()`
//! that, for such demuxers, reopens the container at the start instead; callers already decode forward to the target.
//! The old context is only released once the replacement is open, so a failed reopen leaves the caller's pointer valid.
use ffmpeg_sys_next as ff;
use std::ffi::{CStr, CString};

pub unsafe fn demuxer_needs_reopen_to_seek(fmt: *const ff::AVFormatContext) -> bool {
    unsafe {
        if fmt.is_null() || (*fmt).iformat.is_null() || (*(*fmt).iformat).name.is_null() { return false }
        CStr::from_ptr((*(*fmt).iformat).name).to_bytes() == b"webp_anim"
    }
}
/// ≥ 0 on success (positioned at or before `pts` for normal demuxers, at the START for reopen-to-seek demuxers), < 0 on
/// failure. `*fmt` may be replaced by a fresh context.
pub unsafe fn seek_stream(fmt: *mut *mut ff::AVFormatContext, stream_idx: i32, pts: i64, flags: i32) -> i32 {
    unsafe {
        if fmt.is_null() || (*fmt).is_null() { return -1 }
        if !demuxer_needs_reopen_to_seek(*fmt) { return ff::av_seek_frame(*fmt, stream_idx, pts, flags) }
        if (**fmt).url.is_null() { return -1 }
        let url = CString::from(CStr::from_ptr((**fmt).url));
        let mut fresh: *mut ff::AVFormatContext = std::ptr::null_mut();
        if ff::avformat_open_input(&mut fresh, url.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut()) < 0 { return -1 }
        if ff::avformat_find_stream_info(fresh, std::ptr::null_mut()) < 0 || stream_idx < 0 || stream_idx >= (*fresh).nb_streams as i32 { ff::avformat_close_input(&mut fresh); return -1 }
        ff::avformat_close_input(fmt);
        *fmt = fresh;
        0
    }
}
