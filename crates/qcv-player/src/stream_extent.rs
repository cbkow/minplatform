//! Duration / frame count for containers that don't declare one — ported from `QCView-Player/src/decode/stream_extent.h`.
//!
//! Animated WebP (FFmpeg 9.0's `webp_anim` demuxer) — and GIF / APNG in the same family — report neither a stream nor a
//! format duration and no frame count, so every consumer that sizes a clip from those came out at 0 and the clip never
//! got a timeline or a first frame. These files are small, so a synchronous packet walk is the honest fix: count the
//! video packets and take the end of the last one as the duration. Gated by file size so a huge duration-less transport
//! stream can't stall an open.
use ffmpeg_sys_next as ff;
use std::ffi::CString;

#[derive(Clone, Copy, Debug, Default)]
pub struct StreamExtent { pub ok: bool, pub frames: i32, /// end of the last packet, microseconds
    pub duration_us: i64 }

pub fn scan_stream_extent(path: &str, stream_idx: i32, max_bytes: u64) -> StreamExtent {
    let mut out = StreamExtent::default();
    let Ok(md) = std::fs::metadata(path) else { return out };
    if md.len() > max_bytes { return out }
    let Ok(cpath) = CString::new(path) else { return out };
    unsafe {
        let mut fmt: *mut ff::AVFormatContext = std::ptr::null_mut();
        if ff::avformat_open_input(&mut fmt, cpath.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut()) < 0 { return out }
        if ff::avformat_find_stream_info(fmt, std::ptr::null_mut()) < 0 || stream_idx < 0 || stream_idx >= (*fmt).nb_streams as i32 { ff::avformat_close_input(&mut fmt); return out }
        let tb = (**(*fmt).streams.add(stream_idx as usize)).time_base;
        let pkt = ff::av_packet_alloc();
        if pkt.is_null() { ff::avformat_close_input(&mut fmt); return out }
        let (mut max_end, mut last_duration) = (ff::AV_NOPTS_VALUE, 0i64);
        while ff::av_read_frame(fmt, pkt) >= 0 {
            if (*pkt).stream_index == stream_idx {
                out.frames += 1;
                let ts = if (*pkt).pts != ff::AV_NOPTS_VALUE { (*pkt).pts } else { (*pkt).dts };
                if (*pkt).duration > 0 { last_duration = (*pkt).duration; }
                if ts != ff::AV_NOPTS_VALUE { let end = ts + if (*pkt).duration > 0 { (*pkt).duration } else { last_duration }; max_end = if max_end == ff::AV_NOPTS_VALUE { end } else { max_end.max(end) }; }
            }
            ff::av_packet_unref(pkt);
        }
        let mut p = pkt; ff::av_packet_free(&mut p);
        ff::avformat_close_input(&mut fmt);
        if max_end != ff::AV_NOPTS_VALUE { out.duration_us = ff::av_rescale_q(max_end, tb, ff::AVRational { num: 1, den: 1_000_000 }); }
        out.ok = out.frames > 0;
    }
    out
}
