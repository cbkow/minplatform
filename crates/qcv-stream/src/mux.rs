//! The muxer and the wire (QCView `src/output/stream_muxer.cpp`): libavformat — MPEG-TS over `srt://` (caller mode; the key
//! as the stream id; latency in ms) or FLV over `rtmp(s)://` (the key as the stream name, in the URL). FLV wants the
//! parameter sets in its header: known ones (a reconnect) go in at open; otherwise the header waits for the first keyframe
//! and is written from the SPS / PPS inside it — libavformat turns the Annex-B units into length-prefixed ones itself. One video stream (H.264 Annex-B, 1 / 90000,
//! BT.709 limited tags), optionally one audio stream (AAC, 1 / 48000). Every blocking call ends when the session's `stop`
//! flag is set — the flag is the session's and is never reset here (QCView reset its own at each open, so a Stop racing a
//! reconnect could wait out the 5 s connect timeout).
use crate::EncodedUnit;
use ffmpeg_sys_next as ff;
use std::ffi::{c_void, CStr, CString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme { Srt, Rtmp }
/// QCView's rule (a case-sensitive prefix): `srt://` → MPEG-TS, `rtmp://` / `rtmps://` → FLV.
pub fn scheme(url: &str) -> Option<Scheme> {
    if url.starts_with("srt://") { Some(Scheme::Srt) } else if url.starts_with("rtmp://") || url.starts_with("rtmps://") { Some(Scheme::Rtmp) } else { None }
}
/// QCView's refusal, word for word (ruling 4: said at Start, not retried forever).
pub fn scheme_error(url: &str) -> String { format!("the URL must start with srt://, rtmp:// or rtmps://: {url}") }

/// QCView `MuxConfig`.
#[derive(Clone, Debug, Default)]
pub struct MuxConfig {
    pub url: String, pub width: u32, pub height: u32, pub fps_num: u32, pub fps_den: u32, pub video_kbps: u32,
    /// 0 = video only
    pub audio_kbps: u32, /// the AAC encoder's AudioSpecificConfig
    pub audio_extradata: Vec<u8>,
    pub srt_latency_ms: u32, pub stream_id: String, pub passphrase: String,
}
pub const CONNECT_TIMEOUT_MS: u32 = 5000;

fn averr(code: i32) -> String {
    let mut buf = [0 as libc::c_char; 256];
    unsafe { ff::av_strerror(code, buf.as_mut_ptr(), buf.len()); CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned() }
}
extern "C" fn interrupted(opaque: *mut c_void) -> i32 { unsafe { (*(opaque as *const AtomicBool)).load(Ordering::Acquire) as i32 } }
fn set(d: &mut *mut ff::AVDictionary, k: &str, v: &str) { let (k, v) = (CString::new(k).unwrap(), CString::new(v).unwrap()); unsafe { ff::av_dict_set(d, k.as_ptr(), v.as_ptr(), 0); } }

pub struct Muxer { ctx: *mut ff::AVFormatContext, pkt: *mut ff::AVPacket, stop: Arc<AtomicBool>, connected: bool, header: bool, flv: bool, /// the last keyframe's SPS + PPS (Annex-B)
    params: Vec<u8>, video: *mut ff::AVStream, audio: *mut ff::AVStream, /// the elementary streams' payload written (the kbps shown)
    pub bytes: u64 }
unsafe impl Send for Muxer {}

impl Muxer {
    pub fn new(stop: Arc<AtomicBool>) -> Muxer { Muxer { ctx: std::ptr::null_mut(), pkt: unsafe { ff::av_packet_alloc() }, stop, connected: false, header: false, flv: false, params: vec![], video: std::ptr::null_mut(), audio: std::ptr::null_mut(), bytes: 0 } }
    pub fn is_open(&self) -> bool { !self.ctx.is_null() && self.connected }
    /// Connect and write the header. The error strings are QCView's.
    pub fn open(&mut self, cfg: &MuxConfig) -> Result<(), String> {
        self.close();
        let (name, flv) = match scheme(&cfg.url) { Some(Scheme::Srt) => ("mpegts", false), Some(Scheme::Rtmp) => ("flv", true), None => return Err(scheme_error(&cfg.url)) };
        self.flv = flv;
        unsafe {
            let cname = CString::new(name).unwrap();
            let r = ff::avformat_alloc_output_context2(&mut self.ctx, std::ptr::null(), cname.as_ptr(), std::ptr::null());
            if r < 0 || self.ctx.is_null() { self.ctx = std::ptr::null_mut(); return Err(format!("{name} muxer: {}", averr(r))) }
            let v = ff::avformat_new_stream(self.ctx, std::ptr::null());
            if v.is_null() { self.close(); return Err("could not add the video stream".into()) }
            (*v).id = 0; (*v).time_base = ff::AVRational { num: 1, den: 90000 }; (*v).avg_frame_rate = ff::AVRational { num: cfg.fps_num as i32, den: cfg.fps_den.max(1) as i32 };
            let p = (*v).codecpar;
            (*p).codec_type = ff::AVMediaType::AVMEDIA_TYPE_VIDEO; (*p).codec_id = ff::AVCodecID::AV_CODEC_ID_H264;
            (*p).width = cfg.width as i32; (*p).height = cfg.height as i32; (*p).format = ff::AVPixelFormat::AV_PIX_FMT_NV12 as i32; (*p).bit_rate = cfg.video_kbps as i64 * 1000;
            (*p).color_primaries = ff::AVColorPrimaries::AVCOL_PRI_BT709; (*p).color_trc = ff::AVColorTransferCharacteristic::AVCOL_TRC_BT709;
            (*p).color_space = ff::AVColorSpace::AVCOL_SPC_BT709; (*p).color_range = ff::AVColorRange::AVCOL_RANGE_MPEG;
            self.video = v;
            if flv && !self.params.is_empty() && !self.set_video_extradata() { self.close(); return Err("could not allocate the video extradata".into()) }
            if cfg.audio_kbps > 0 {
                let a = ff::avformat_new_stream(self.ctx, std::ptr::null());
                if a.is_null() { self.close(); return Err("could not add the audio stream".into()) }
                (*a).id = 1; (*a).time_base = ff::AVRational { num: 1, den: 48000 };
                let p = (*a).codecpar;
                (*p).codec_type = ff::AVMediaType::AVMEDIA_TYPE_AUDIO; (*p).codec_id = ff::AVCodecID::AV_CODEC_ID_AAC;
                (*p).sample_rate = 48000; (*p).format = ff::AVSampleFormat::AV_SAMPLE_FMT_FLTP as i32; (*p).bit_rate = cfg.audio_kbps as i64 * 1000; (*p).frame_size = 1024;
                ff::av_channel_layout_default(&mut (*p).ch_layout, 2);
                if !cfg.audio_extradata.is_empty() {
                    let n = cfg.audio_extradata.len();
                    let e = ff::av_mallocz(n + ff::AV_INPUT_BUFFER_PADDING_SIZE as usize) as *mut u8;
                    if e.is_null() { self.close(); return Err("could not allocate the audio extradata".into()) }
                    std::ptr::copy_nonoverlapping(cfg.audio_extradata.as_ptr(), e, n); (*p).extradata = e; (*p).extradata_size = n as i32;
                }
                self.audio = a;
            }
            // the protocol's options (QCView mux:157-171). SRT: a caller, the receive window in µs, live transport. RTMP: a live
            // publish; the socket's timeout matches SRT's connect timeout (QCView set none: a silent host held "connecting" for
            // the OS's own minute-plus)
            let mut popts: *mut ff::AVDictionary = std::ptr::null_mut();
            if flv { set(&mut popts, "rtmp_live", "live"); set(&mut popts, "rw_timeout", &(CONNECT_TIMEOUT_MS as u64 * 1000).to_string()); }   // `rw_timeout`, the generic one: RTMP's own `timeout` means LISTEN
            else {
                set(&mut popts, "mode", "caller"); set(&mut popts, "latency", &(cfg.srt_latency_ms as u64 * 1000).to_string());
                set(&mut popts, "connect_timeout", &CONNECT_TIMEOUT_MS.to_string()); set(&mut popts, "pkt_size", "1316"); set(&mut popts, "transtype", "live");
                if !cfg.stream_id.is_empty() { set(&mut popts, "streamid", &cfg.stream_id); }
                if !cfg.passphrase.is_empty() { set(&mut popts, "passphrase", &cfg.passphrase); set(&mut popts, "pbkeylen", "16"); }
            }
            (*self.ctx).interrupt_callback = ff::AVIOInterruptCB { callback: Some(interrupted), opaque: Arc::as_ptr(&self.stop) as *mut c_void };
            let url = CString::new(cfg.url.as_str()).map_err(|_| scheme_error(&cfg.url))?;
            let r = ff::avio_open2(&mut (*self.ctx).pb, url.as_ptr(), ff::AVIO_FLAG_WRITE, &(*self.ctx).interrupt_callback, &mut popts);
            ff::av_dict_free(&mut popts);
            if r < 0 { self.close(); return Err(format!("connect: {}", averr(r))) }
        }
        self.connected = true;
        // FLV with no parameter sets yet: the header waits for the first keyframe
        if flv && self.params.is_empty() { return Ok(()) }
        self.write_header()
    }
    fn set_video_extradata(&mut self) -> bool {
        unsafe {
            let p = (*self.video).codecpar; let n = self.params.len();
            ff::av_freep(&mut (*p).extradata as *mut *mut u8 as *mut c_void); (*p).extradata_size = 0;
            let e = ff::av_mallocz(n + ff::AV_INPUT_BUFFER_PADDING_SIZE as usize) as *mut u8;
            if e.is_null() { return false }
            std::ptr::copy_nonoverlapping(self.params.as_ptr(), e, n); (*p).extradata = e; (*p).extradata_size = n as i32;
        }
        true
    }
    fn write_header(&mut self) -> Result<(), String> {
        unsafe {
            let mut mopts: *mut ff::AVDictionary = std::ptr::null_mut();
            if self.flv { set(&mut mopts, "flvflags", "no_duration_filesize"); } else { set(&mut mopts, "mpegts_flags", "+resend_headers+initial_discontinuity"); }
            let r = ff::avformat_write_header(self.ctx, &mut mopts);
            ff::av_dict_free(&mut mopts);
            if r < 0 { self.close(); return Err(format!("write_header: {}", averr(r))) }
        }
        self.header = true;
        Ok(())
    }
    /// One unit to the wire (interleaved). An error means the connection is gone: the session reconnects.
    pub fn write(&mut self, u: &EncodedUnit) -> Result<(), String> {
        if !self.is_open() { return Err("write: not connected".into()) }
        if !u.audio && u.keyframe { let p = parameter_sets(&u.data); if !p.is_empty() { self.params = p; } }
        if !self.header {
            // FLV before its header: nothing goes out until a keyframe brings the parameter sets
            if u.audio || !u.keyframe { return Ok(()) }
            if self.params.is_empty() { return Err("the keyframe carries no SPS/PPS for the FLV header".into()) }
            if !self.set_video_extradata() { return Err("could not allocate the video extradata".into()) }
            self.write_header()?;
        }
        let (st, tb) = if u.audio { (self.audio, ff::AVRational { num: 1, den: 48000 }) } else { (self.video, ff::AVRational { num: 1, den: 90000 }) };
        if st.is_null() { return Ok(()) }
        unsafe {
            ff::av_packet_unref(self.pkt);
            // borrowed bytes: the interleaver makes its own counted copy
            (*self.pkt).data = u.data.as_ptr() as *mut u8; (*self.pkt).size = u.data.len() as i32; (*self.pkt).stream_index = (*st).index;
            (*self.pkt).pts = ff::av_rescale_q(u.pts, tb, (*st).time_base); (*self.pkt).dts = (*self.pkt).pts; (*self.pkt).duration = ff::av_rescale_q(u.dur, tb, (*st).time_base);
            if u.keyframe || u.audio { (*self.pkt).flags |= ff::AV_PKT_FLAG_KEY; }
            let r = ff::av_interleaved_write_frame(self.ctx, self.pkt);
            if r < 0 { return Err(format!("{}: {}", if u.audio { "write audio" } else { "write" }, averr(r))) }
        }
        self.bytes += u.data.len() as u64;
        Ok(())
    }
    /// The trailer when a header went out, then the connection.
    pub fn close(&mut self) {
        if self.ctx.is_null() { return }
        unsafe {
            if self.header { ff::av_write_trailer(self.ctx); }
            if !(*self.ctx).pb.is_null() { ff::avio_closep(&mut (*self.ctx).pb); }
            ff::avformat_free_context(self.ctx);
        }
        self.ctx = std::ptr::null_mut(); self.video = std::ptr::null_mut(); self.audio = std::ptr::null_mut(); self.header = false; self.connected = false;
    }
}
/// The SPS (7) and PPS (8) units of an Annex-B access unit, start codes kept.
pub fn parameter_sets(annexb: &[u8]) -> Vec<u8> {
    let mut starts = vec![]; let mut i = 0;
    while i + 3 <= annexb.len() { if annexb[i] == 0 && annexb[i + 1] == 0 && annexb[i + 2] == 1 { starts.push(i + 3); i += 3; } else { i += 1; } }
    let mut out = vec![];
    for (n, &s) in starts.iter().enumerate() {
        let mut e = starts.get(n + 1).map_or(annexb.len(), |x| x - 3);
        while e > s && annexb[e - 1] == 0 { e -= 1; }   // the next start code's leading zero
        if s < e && matches!(annexb[s] & 0x1f, 7 | 8) { out.extend_from_slice(&[0, 0, 0, 1]); out.extend_from_slice(&annexb[s..e]); }
    }
    out
}

impl Drop for Muxer { fn drop(&mut self) { self.close(); unsafe { ff::av_packet_free(&mut self.pkt); } } }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_scheme_picks_the_container_as_qcview() {
        assert_eq!(scheme("srt://host:9000"), Some(Scheme::Srt)); assert_eq!(scheme("rtmp://host/app"), Some(Scheme::Rtmp)); assert_eq!(scheme("rtmps://host/app"), Some(Scheme::Rtmp));
        assert_eq!(scheme("SRT://host:9000"), None, "case-sensitive, as QCView"); assert_eq!(scheme("http://host"), None);
        assert_eq!(scheme_error("http://x"), "the URL must start with srt://, rtmp:// or rtmps://: http://x");
    }
    #[test]
    fn the_parameter_sets_come_out_of_a_keyframe() {
        let au = [0, 0, 0, 1, 0x67, 1, 2, 3, 0, 0, 0, 1, 0x68, 9, 0, 0, 0, 1, 0x65, 7, 7, 7];
        assert_eq!(parameter_sets(&au), vec![0, 0, 0, 1, 0x67, 1, 2, 3, 0, 0, 0, 1, 0x68, 9]);
        assert!(parameter_sets(&[0, 0, 0, 1, 0x41, 5, 5]).is_empty(), "a P frame has none");
    }
}
