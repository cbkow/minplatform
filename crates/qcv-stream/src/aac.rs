//! The stream's audio (QCView `src/output/aac_encoder.cpp`): FFmpeg's native `aac` — 48 kHz stereo, planar float in, 1024
//! samples a frame, the AudioSpecificConfig as global extradata for the muxer. pts in samples (1 / 48000).
use crate::EncodedUnit;
use ffmpeg_sys_next as ff;
use std::ffi::CStr;

pub struct Aac { ctx: *mut ff::AVCodecContext, frame: *mut ff::AVFrame, pkt: *mut ff::AVPacket }
unsafe impl Send for Aac {}

fn averr(code: i32) -> String { let mut b = [0 as libc::c_char; 256]; unsafe { ff::av_strerror(code, b.as_mut_ptr(), b.len()); CStr::from_ptr(b.as_ptr()).to_string_lossy().into_owned() } }

impl Aac {
    pub const FRAME: usize = 1024;
    /// The error strings are QCView's.
    pub fn open(kbps: u32) -> Result<Aac, String> {
        unsafe {
            let codec = ff::avcodec_find_encoder_by_name(c"aac".as_ptr());
            if codec.is_null() { return Err("FFmpeg has no 'aac' encoder".into()) }
            let ctx = ff::avcodec_alloc_context3(codec);
            if ctx.is_null() { return Err("could not allocate the AAC context".into()) }
            let mut a = Aac { ctx, frame: std::ptr::null_mut(), pkt: std::ptr::null_mut() };
            (*ctx).sample_rate = 48000; (*ctx).sample_fmt = ff::AVSampleFormat::AV_SAMPLE_FMT_FLTP; (*ctx).bit_rate = kbps as i64 * 1000;
            (*ctx).time_base = ff::AVRational { num: 1, den: 48000 }; (*ctx).flags |= ff::AV_CODEC_FLAG_GLOBAL_HEADER as i32;
            ff::av_channel_layout_default(&mut (*ctx).ch_layout, 2);
            let r = ff::avcodec_open2(ctx, codec, std::ptr::null_mut());
            if r < 0 { return Err(format!("avcodec_open2(aac): {}", averr(r))) }
            if (*ctx).frame_size != Self::FRAME as i32 { return Err(format!("aac frame size is {}, expected 1024", (*ctx).frame_size)) }
            a.frame = ff::av_frame_alloc(); a.pkt = ff::av_packet_alloc();
            if a.frame.is_null() || a.pkt.is_null() { return Err("could not allocate the AAC frame/packet".into()) }
            (*a.frame).nb_samples = Self::FRAME as i32; (*a.frame).format = ff::AVSampleFormat::AV_SAMPLE_FMT_FLTP as i32; (*a.frame).sample_rate = 48000;
            ff::av_channel_layout_copy(&mut (*a.frame).ch_layout, &(*ctx).ch_layout);
            let r = ff::av_frame_get_buffer(a.frame, 0);
            if r < 0 { return Err(format!("av_frame_get_buffer: {}", averr(r))) }
            log::info!("stream: AAC-LC 48 kHz stereo {kbps} kbps");
            Ok(a)
        }
    }
    /// The AudioSpecificConfig.
    pub fn extradata(&self) -> Vec<u8> { unsafe { if (*self.ctx).extradata.is_null() { vec![] } else { std::slice::from_raw_parts((*self.ctx).extradata, (*self.ctx).extradata_size as usize).to_vec() } } }
    /// One 1024-sample frame in; whatever the encoder has ready out (it holds a frame of priming).
    pub fn encode(&mut self, l: &[f32], r: &[f32], pts: i64, out: &mut Vec<EncodedUnit>) {
        debug_assert!(l.len() == Self::FRAME && r.len() == Self::FRAME);
        unsafe {
            if ff::av_frame_make_writable(self.frame) < 0 { return }
            std::ptr::copy_nonoverlapping(l.as_ptr(), (*self.frame).data[0] as *mut f32, Self::FRAME);
            std::ptr::copy_nonoverlapping(r.as_ptr(), (*self.frame).data[1] as *mut f32, Self::FRAME);
            (*self.frame).pts = pts;
            if ff::avcodec_send_frame(self.ctx, self.frame) < 0 { return }
            while ff::avcodec_receive_packet(self.ctx, self.pkt) >= 0 {
                let p = &*self.pkt;
                out.push(EncodedUnit { data: std::slice::from_raw_parts(p.data, p.size as usize).to_vec(), pts: p.pts, dur: if p.duration > 0 { p.duration } else { Self::FRAME as i64 }, keyframe: true, audio: true });
                ff::av_packet_unref(self.pkt);
            }
        }
    }
}
impl Drop for Aac { fn drop(&mut self) { unsafe { ff::av_frame_free(&mut self.frame); ff::av_packet_free(&mut self.pkt); ff::avcodec_free_context(&mut self.ctx); } } }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_tone_encodes_with_a_config_and_sample_timestamps() {
        let mut a = Aac::open(160).expect("the aac encoder");
        assert!(a.extradata().len() >= 2, "an AudioSpecificConfig");
        let tone: Vec<f32> = (0..1024).map(|i| (i as f32 * 0.05).sin() * 0.25).collect();
        let mut out = vec![];
        for k in 0..20 { a.encode(&tone, &tone, k * 1024, &mut out); }
        assert!(out.len() >= 18, "{} packets", out.len());
        assert!(out.iter().all(|u| u.audio && !u.data.is_empty() && u.dur == 1024));
        assert!(out.windows(2).all(|w| w[1].pts - w[0].pts == 1024), "a constant 1024-sample cadence");
    }
}
