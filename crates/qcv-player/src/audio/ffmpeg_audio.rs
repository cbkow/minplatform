//! The FFmpeg audio contexts both decoders share (QCView `audio_decoder.cpp` `openAudioStream` / `setupResampler`
//! and `audio_chunk_reader.cpp`): the best audio stream, its codec, and swresample to interleaved f32 stereo at
//! 48 kHz through the routing matrix (`rematrix_volume` 0.9 for swresample's own fold).
use super::routing::{routing_matrix, RoutingMode};
use crate::ffmpeg::av_err;
use crate::Error;
use ffmpeg_sys_next as ff;
use std::ffi::{CStr, CString};
use std::ptr;

pub const OUT_RATE: i32 = 48000;
pub const OUT_CHANNELS: usize = 2;

/// Raw FFmpeg state owned by ONE thread at a time (moved into the decode thread, or confined to the reader's caller).
pub struct AudioCtx {
    pub fmt: *mut ff::AVFormatContext, pub cctx: *mut ff::AVCodecContext, pub swr: *mut ff::SwrContext,
    pub frame: *mut ff::AVFrame, pub pkt: *mut ff::AVPacket,
    pub stream_idx: i32, pub stream_start_time: i64, pub time_base: f64, pub src_rate: i32, pub src_channels: usize,
    pub layout_name: String, pub duration: f64, pub routing: RoutingMode,
    pub swr_buf: Vec<u8>,
}
unsafe impl Send for AudioCtx {}

impl AudioCtx {
    /// `None` = the file has no decodable audio stream (not an error).
    pub fn open(path: &str, routing: RoutingMode) -> Result<Option<AudioCtx>, Error> {
        unsafe {
            let cpath = CString::new(path).map_err(|_| Error::Open("path".into()))?;
            let mut fmt: *mut ff::AVFormatContext = ptr::null_mut();
            let r = ff::avformat_open_input(&mut fmt, cpath.as_ptr(), ptr::null_mut(), ptr::null_mut());
            if r < 0 { return Err(Error::Open(format!("audio: avformat_open_input: {}", av_err(r)))) }
            if ff::avformat_find_stream_info(fmt, ptr::null_mut()) < 0 { ff::avformat_close_input(&mut fmt); return Err(Error::Open("audio: avformat_find_stream_info failed".into())) }
            let mut dec: *const ff::AVCodec = ptr::null();
            let idx = ff::av_find_best_stream(fmt, ff::AVMediaType::AVMEDIA_TYPE_AUDIO, -1, -1, &mut dec, 0);
            if idx < 0 || dec.is_null() { ff::avformat_close_input(&mut fmt); return Ok(None) }
            let st = *(*fmt).streams.add(idx as usize);
            let cp = (*st).codecpar;
            let src_channels = (*cp).ch_layout.nb_channels.max(0) as usize;
            let src_rate = (*cp).sample_rate;
            let mut buf = [0i8; 64];
            let layout_name = if ff::av_channel_layout_describe(&(*cp).ch_layout, buf.as_mut_ptr(), buf.len()) > 0 { CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned() } else { String::new() };
            let time_base = ff::av_q2d((*st).time_base);
            let duration = if (*fmt).duration > 0 { (*fmt).duration as f64 / ff::AV_TIME_BASE as f64 } else if (*st).duration > 0 { (*st).duration as f64 * time_base } else { 0.0 };
            let stream_start_time = if (*st).start_time != ff::AV_NOPTS_VALUE { (*st).start_time } else { 0 };
            let cctx = ff::avcodec_alloc_context3(dec);
            if cctx.is_null() { ff::avformat_close_input(&mut fmt); return Err(Error::Open("audio: avcodec_alloc_context3".into())) }
            (*cctx).pkt_timebase = (*st).time_base;
            if ff::avcodec_parameters_to_context(cctx, cp) < 0 || ff::avcodec_open2(cctx, dec, ptr::null_mut()) < 0 {
                let mut c = cctx; ff::avcodec_free_context(&mut c); ff::avformat_close_input(&mut fmt); return Err(Error::Open("audio: codec context setup failed".into()))
            }
            let mut ctx = AudioCtx { fmt, cctx, swr: ptr::null_mut(), frame: ff::av_frame_alloc(), pkt: ff::av_packet_alloc(), stream_idx: idx, stream_start_time, time_base, src_rate, src_channels, layout_name, duration, routing, swr_buf: vec![] };
            if ctx.frame.is_null() || ctx.pkt.is_null() { return Err(Error::Open("audio: alloc".into())) }
            if !ctx.setup_swr() { return Err(Error::Open("audio: resampler setup failed".into())) }
            Ok(Some(ctx))
        }
    }
    /// swresample → interleaved f32 stereo 48 kHz with the routing matrix (QCView `setupResampler` / `setupSwr`).
    pub fn setup_swr(&mut self) -> bool {
        unsafe {
            self.teardown_swr();
            let mut out_layout: ff::AVChannelLayout = std::mem::zeroed();
            ff::av_channel_layout_default(&mut out_layout, OUT_CHANNELS as i32);
            let mut in_layout: ff::AVChannelLayout = std::mem::zeroed();
            if ff::av_channel_layout_copy(&mut in_layout, &(*self.cctx).ch_layout) < 0 { return false }
            let mut swr: *mut ff::SwrContext = ptr::null_mut();
            let r = ff::swr_alloc_set_opts2(&mut swr, &out_layout, ff::AVSampleFormat::AV_SAMPLE_FMT_FLT, OUT_RATE, &in_layout, (*self.cctx).sample_fmt, (*self.cctx).sample_rate, 0, ptr::null_mut());
            ff::av_channel_layout_uninit(&mut in_layout); ff::av_channel_layout_uninit(&mut out_layout);
            if r < 0 || swr.is_null() { return false }
            match routing_matrix(self.routing, self.src_channels) {
                Some(m) => { if ff::swr_set_matrix(swr, m.as_ptr(), self.src_channels as i32) < 0 { log::warn!("audio: swr_set_matrix failed (mode {:?}, {} ch)", self.routing, self.src_channels); } }
                None => { let k = CString::new("rematrix_volume").unwrap(); ff::av_opt_set_double(swr as *mut _, k.as_ptr(), 0.9, 0); }
            }
            if ff::swr_init(swr) < 0 { ff::swr_free(&mut swr); return false }
            self.swr = swr; true
        }
    }
    pub fn teardown_swr(&mut self) { unsafe { if !self.swr.is_null() { ff::swr_free(&mut self.swr); } } }
    /// Convert the decoded `frame` into `swr_buf` (interleaved f32 stereo); returns the output frames.
    pub fn convert(&mut self) -> i32 {
        unsafe {
            let delay = ff::swr_get_delay(self.swr, (*self.cctx).sample_rate as i64);
            let dst_n = ff::av_rescale_rnd(delay + (*self.frame).nb_samples as i64, OUT_RATE as i64, (*self.cctx).sample_rate as i64, ff::AVRounding::AV_ROUND_UP) as usize;
            let need = dst_n * OUT_CHANNELS * 4;
            if self.swr_buf.len() < need { self.swr_buf.resize(need, 0); }
            let mut dst = self.swr_buf.as_mut_ptr();
            ff::swr_convert(self.swr, &mut dst, dst_n as i32, (*self.frame).data.as_ptr() as *mut *const u8, (*self.frame).nb_samples)
        }
    }
    /// Seek the container to `sec` (backward keyframe), flush the codec.
    pub fn seek(&mut self, sec: f64) {
        unsafe {
            let mut ts = (sec / self.time_base) as i64;
            if self.stream_start_time > 0 { ts += self.stream_start_time; }
            if ff::av_seek_frame(self.fmt, self.stream_idx, ts, ff::AVSEEK_FLAG_BACKWARD) < 0 { ff::av_seek_frame(self.fmt, self.stream_idx, 0, ff::AVSEEK_FLAG_BACKWARD); }
            ff::avcodec_flush_buffers(self.cctx);
        }
    }
    /// The decoded frame's start in stream seconds (after the stream's start), if it has a pts.
    pub fn frame_start_sec(&self) -> Option<f64> { unsafe { let pts = (*self.frame).pts; if pts == ff::AV_NOPTS_VALUE { None } else { Some((pts - self.stream_start_time) as f64 * self.time_base) } } }
    pub fn frame_samples(&self) -> i32 { unsafe { (*self.frame).nb_samples } }
    /// Per-source-channel peaks of the decoded frame, pre-fold (QCView `computeFramePeaks`), into `peaks[..16]`.
    pub fn frame_peaks(&self, peaks: &mut [f32; 16]) {
        unsafe {
            let f = self.frame; let n = (*f).nb_samples as usize; let nch = self.src_channels.min(16);
            if n == 0 || nch == 0 { return }
            let fmt = std::mem::transmute::<i32, ff::AVSampleFormat>((*f).format);
            use ff::AVSampleFormat::*;
            fn peak<T: Copy + Into<f64>>(p: *const T, stride: usize, n: usize, inv: f32) -> f32 { let mut m = 0.0f64; for i in 0..n { let v: f64 = unsafe { *p.add(i * stride) }.into(); let a = v.abs(); if a > m { m = a; } } m as f32 * inv }
            match fmt {
                AV_SAMPLE_FMT_FLT => for ch in 0..nch { peaks[ch] = peak((*f).data[0] as *const f32, nch, n, 1.0).min(f32::MAX).max(0.0).min(1e9) ; peaks[ch] = peak(((*f).data[0] as *const f32).add(ch), self.src_channels, n, 1.0); },
                AV_SAMPLE_FMT_FLTP => for ch in 0..nch { peaks[ch] = peak((*f).data[ch] as *const f32, 1, n, 1.0); },
                AV_SAMPLE_FMT_S16 => for ch in 0..nch { peaks[ch] = peak(((*f).data[0] as *const i16).add(ch), self.src_channels, n, 1.0 / 32768.0); },
                AV_SAMPLE_FMT_S16P => for ch in 0..nch { peaks[ch] = peak((*f).data[ch] as *const i16, 1, n, 1.0 / 32768.0); },
                AV_SAMPLE_FMT_S32 => for ch in 0..nch { peaks[ch] = peak(((*f).data[0] as *const i32).add(ch), self.src_channels, n, 1.0 / 2147483647.0); },
                AV_SAMPLE_FMT_S32P => for ch in 0..nch { peaks[ch] = peak((*f).data[ch] as *const i32, 1, n, 1.0 / 2147483647.0); },
                _ => {}
            }
        }
    }
}
impl Drop for AudioCtx {
    fn drop(&mut self) { unsafe {
        self.teardown_swr();
        if !self.frame.is_null() { ff::av_frame_free(&mut self.frame); }
        if !self.pkt.is_null() { ff::av_packet_free(&mut self.pkt); }
        if !self.cctx.is_null() { ff::avcodec_free_context(&mut self.cctx); }
        if !self.fmt.is_null() { ff::avformat_close_input(&mut self.fmt); }
    } }
}
