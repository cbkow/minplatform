//! `scanStreamExtent` — ported from `QCView-Player/src/decode/stream_extent.h`: for containers without a declared duration
//! (animated WebP / GIF / APNG, some raw streams) count the video packets and the last packet's end so the bin and the
//! timeline get a real length. `max_bytes` 0 = no cap.
use ffmpeg_sys_next as ff;
use std::ffi::CString;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StreamExtent { pub ok: bool, pub frames: i32, /// the end of the last packet, microseconds
    pub duration_us: i64 }

pub fn scan_stream_extent(path: &str, stream_idx: i32, max_bytes: u64) -> StreamExtent {
    let Ok(cpath) = CString::new(path) else { return StreamExtent::default() };
    unsafe {
        let mut fmt: *mut ff::AVFormatContext = std::ptr::null_mut();
        if ff::avformat_open_input(&mut fmt, cpath.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut()) < 0 { return StreamExtent::default() }
        if ff::avformat_find_stream_info(fmt, std::ptr::null_mut()) < 0 || stream_idx < 0 || stream_idx >= (*fmt).nb_streams as i32 { ff::avformat_close_input(&mut fmt); return StreamExtent::default() }
        let st = *(*fmt).streams.add(stream_idx as usize); let tb = (*st).time_base;
        let pkt = ff::av_packet_alloc();
        let (mut frames, mut last_end, mut bytes) = (0i32, i64::MIN, 0u64);
        while ff::av_read_frame(fmt, pkt) >= 0 {
            if (*pkt).stream_index == stream_idx {
                frames += 1; bytes += (*pkt).size.max(0) as u64;
                let ts = if (*pkt).pts != ff::AV_NOPTS_VALUE { (*pkt).pts } else { (*pkt).dts };
                if ts != ff::AV_NOPTS_VALUE { let end = ts + (*pkt).duration.max(0); if end > last_end { last_end = end; } }
            }
            ff::av_packet_unref(pkt);
            if max_bytes > 0 && bytes > max_bytes { break }
        }
        let mut p = pkt; ff::av_packet_free(&mut p);
        ff::avformat_close_input(&mut fmt);
        let duration_us = if last_end == i64::MIN { 0 } else { ff::av_rescale_q(last_end, tb, ff::AVRational { num: 1, den: 1_000_000 }) };
        StreamExtent { ok: frames > 0, frames, duration_us }
    }
}
