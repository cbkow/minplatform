//! S20: the first picture of a file as RGBA8 at most `max` on its long side. Spike code: raw FFmpeg, no colour work,
//! nothing freed on the error paths (the extension's process is short-lived).
use ffmpeg_sys_next as ff;
use std::{ffi::c_char, ptr};

/// Returns a buffer of w*h*4 bytes the caller frees with `s20_free`, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn s20_thumb(path: *const c_char, max: i32, w: *mut i32, h: *mut i32) -> *mut u8 { unsafe {
    let mut fmt = ptr::null_mut();
    if ff::avformat_open_input(&mut fmt, path, ptr::null_mut(), ptr::null_mut()) < 0 { return ptr::null_mut() }
    if ff::avformat_find_stream_info(fmt, ptr::null_mut()) < 0 { return ptr::null_mut() }
    let si = ff::av_find_best_stream(fmt, ff::AVMediaType::AVMEDIA_TYPE_VIDEO, -1, -1, ptr::null_mut(), 0);
    if si < 0 { return ptr::null_mut() }
    let st = *(*fmt).streams.add(si as usize);
    let codec = ff::avcodec_find_decoder((*(*st).codecpar).codec_id);
    if codec.is_null() { return ptr::null_mut() }
    let ctx = ff::avcodec_alloc_context3(codec);
    ff::avcodec_parameters_to_context(ctx, (*st).codecpar); (*ctx).thread_count = 0;
    if ff::avcodec_open2(ctx, codec, ptr::null_mut()) < 0 { return ptr::null_mut() }
    let (pkt, fr) = (ff::av_packet_alloc(), ff::av_frame_alloc()); let mut got = false;
    while !got && ff::av_read_frame(fmt, pkt) >= 0 {
        if (*pkt).stream_index == si { ff::avcodec_send_packet(ctx, pkt); got = ff::avcodec_receive_frame(ctx, fr) == 0; }
        ff::av_packet_unref(pkt);
    }
    if !got { ff::avcodec_send_packet(ctx, ptr::null()); got = ff::avcodec_receive_frame(ctx, fr) == 0; }
    if !got { return ptr::null_mut() }
    let (fw, fh) = ((*fr).width, (*fr).height); let k = (max as f64 / fw.max(fh) as f64).min(1.0);
    let (tw, th) = (((fw as f64 * k) as i32).max(1), ((fh as f64 * k) as i32).max(1));
    let sws = ff::sws_getContext(fw, fh, std::mem::transmute::<i32, ff::AVPixelFormat>((*fr).format), tw, th, ff::AVPixelFormat::AV_PIX_FMT_RGBA, 2 as _, ptr::null_mut(), ptr::null_mut(), ptr::null());
    if sws.is_null() { return ptr::null_mut() }
    let mut buf = vec![255u8; (tw * th * 4) as usize].into_boxed_slice();
    let dst = [buf.as_mut_ptr(), ptr::null_mut(), ptr::null_mut(), ptr::null_mut()]; let stride = [tw * 4, 0, 0, 0];
    ff::sws_scale(sws, (*fr).data.as_ptr() as *const *const u8, (*fr).linesize.as_ptr(), 0, fh, dst.as_ptr(), stride.as_ptr());
    ff::sws_freeContext(sws); let mut fr = fr; ff::av_frame_free(&mut fr); let mut pkt = pkt; ff::av_packet_free(&mut pkt);
    let mut ctx = ctx; ff::avcodec_free_context(&mut ctx); ff::avformat_close_input(&mut fmt);
    *w = tw; *h = th; Box::into_raw(buf) as *mut u8
} }

#[unsafe(no_mangle)]
pub unsafe extern "C" fn s20_free(p: *mut u8, w: i32, h: i32) { unsafe { drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(p, (w * h * 4) as usize))) } }
