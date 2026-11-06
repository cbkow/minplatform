//! Thin RAII over the raw FFmpeg 9 bindings (canon §4: no safe wrapper covers hwaccel/zero-copy, so the
//! platform writes its own). Software path: planes repacked to 256-byte pitches for wgpu upload.
//! VideoToolbox path (`hw`): frames stay in CVPixelBuffers (`AVFrame.data[3]`), handed over by reference.
use ffmpeg_sys_next as ff;
use std::{ffi::CString, ptr, time::Instant};

pub struct Plane { pub data: Vec<u8>, pub width: u32, pub height: u32, pub pitch: u32 }
#[derive(Clone, Copy, PartialEq, Debug)] pub enum Layout { Ycc, RgbPlanarF16 }   // RgbPlanarF16 = FFmpeg gbrp*f16 (EXR): planes G, B, R, linear light
/// A VideoToolbox frame: an `av_frame_clone` reference that owns the CVPixelBuffer until dropped.
pub struct VtFrame(pub *mut ff::AVFrame);
unsafe impl Send for VtFrame {}
impl VtFrame { pub fn pixel_buffer(&self) -> *mut std::ffi::c_void { unsafe { (*self.0).data[3] as *mut _ } } }
impl Drop for VtFrame { fn drop(&mut self) { unsafe { ff::av_frame_free(&mut self.0) } } }
unsafe impl Sync for VtFrame {}   // the AVFrame ref is only ever read (the pixel buffer) once cloned
impl Clone for VtFrame { fn clone(&self) -> Self { VtFrame(unsafe { ff::av_frame_clone(self.0) }) } }   // a refcounted AVFrame ref (the CVPixelBuffer is shared)

pub struct Frame {
    pub planes: Vec<Plane>,        // software path: Y, U, V
    pub vt: Option<VtFrame>,       // VideoToolbox path
    pub bytes_per_sample: u32,     // 1 or 2
    pub bit_depth: u32,            // 8 or 10
    pub full_range: bool,
    pub pts: f64,                  // seconds
    pub layout: Layout,
    pub keyframe: bool,
    pub decode_ms: f32,            // receive_frame wall time incl. the repack
}

pub struct Decoder {
    fmt: *mut ff::AVFormatContext,
    ctx: *mut ff::AVCodecContext,
    pkt: *mut ff::AVPacket,
    frame: *mut ff::AVFrame,
    hw: *mut ff::AVBufferRef,
    stream: i32,
    time_base: f64,
    pub width: u32,
    pub height: u32,
    pub codec: String,
    pub pix_fmt: String,
    pub fps: f64,
    pub trc: i32,          // AVColorTransferCharacteristic (16 = SMPTE 2084 PQ)
    pub colorspace: i32,   // AVColorSpace (9 = BT.2020 NCL)
    pub duration: f64,     // seconds (stream or container)
    pub start: f64,        // the stream's first pts in seconds (0 when untagged)
    pub loop_at_eof: bool, // S1: loop; S2: stop (next() returns None at EOF)
    pub at_eof: bool,
}
unsafe impl Send for Decoder {}

fn averror(e: i32) -> i32 { -e }

unsafe extern "C" fn pick_videotoolbox(_ctx: *mut ff::AVCodecContext, fmts: *const ff::AVPixelFormat) -> ff::AVPixelFormat {
    unsafe {
        let mut p = fmts;
        while *p != ff::AVPixelFormat::AV_PIX_FMT_NONE { if *p == ff::AVPixelFormat::AV_PIX_FMT_VIDEOTOOLBOX { return *p; } p = p.add(1); }
        *fmts
    }
}

impl Decoder {
    pub fn open(path: &str, threads: i32, hw: bool) -> Result<Self, String> { Self::open_opts(path, threads, hw, false) }
    /// `low_latency` = SLICE threading only: one frame in, one frame out (the scrub decoder); frame threading adds a pipeline of frames of delay after every flush.
    pub fn open_opts(path: &str, threads: i32, hw: bool, low_latency: bool) -> Result<Self, String> {
        unsafe {
            let cpath = CString::new(path).unwrap();
            let mut fmt: *mut ff::AVFormatContext = ptr::null_mut();
            // image sequences (an FFmpeg image2 pattern such as dir/frame_%04d.exr): 24 fps unless the caller says otherwise
            let mut opts: *mut ff::AVDictionary = ptr::null_mut();
            if path.contains('%') { let k = CString::new("framerate").unwrap(); let v = CString::new("24").unwrap(); ff::av_dict_set(&mut opts, k.as_ptr(), v.as_ptr(), 0); }
            let r = ff::avformat_open_input(&mut fmt, cpath.as_ptr(), ptr::null_mut(), &mut opts);
            ff::av_dict_free(&mut opts);
            if r < 0 { return Err("avformat_open_input".into()); }
            if ff::avformat_find_stream_info(fmt, ptr::null_mut()) < 0 { return Err("find_stream_info".into()); }
            let mut dec: *const ff::AVCodec = ptr::null();
            let stream = ff::av_find_best_stream(fmt, ff::AVMediaType::AVMEDIA_TYPE_VIDEO, -1, -1, &mut dec, 0);
            if stream < 0 { return Err("no video stream".into()); }
            let st = *(*fmt).streams.add(stream as usize);
            let ctx = ff::avcodec_alloc_context3(dec);
            ff::avcodec_parameters_to_context(ctx, (*st).codecpar);
            (*ctx).thread_count = threads;
            if low_latency { (*ctx).thread_type = ff::FF_THREAD_SLICE as i32; }
            let mut hwref: *mut ff::AVBufferRef = ptr::null_mut();
            if hw {
                if ff::av_hwdevice_ctx_create(&mut hwref, ff::AVHWDeviceType::AV_HWDEVICE_TYPE_VIDEOTOOLBOX, ptr::null(), ptr::null_mut(), 0) < 0 { return Err("av_hwdevice_ctx_create(videotoolbox)".into()); }
                (*ctx).hw_device_ctx = ff::av_buffer_ref(hwref);
                (*ctx).get_format = Some(pick_videotoolbox);
            }
            if ff::avcodec_open2(ctx, dec, ptr::null_mut()) < 0 { return Err("avcodec_open2".into()); }
            let fr = (*st).avg_frame_rate;
            let codec = std::ffi::CStr::from_ptr((*dec).name).to_string_lossy().into_owned();
            let pf = ff::av_get_pix_fmt_name(std::mem::transmute::<i32, ff::AVPixelFormat>((*(*st).codecpar).format));
            let pix_fmt = if pf.is_null() { "?".into() } else { std::ffi::CStr::from_ptr(pf).to_string_lossy().into_owned() };
            Ok(Decoder { fmt, ctx, pkt: ff::av_packet_alloc(), frame: ff::av_frame_alloc(), hw: hwref, stream, time_base: ff::av_q2d((*st).time_base),
                width: (*ctx).width as u32, height: (*ctx).height as u32, codec, pix_fmt, fps: if fr.den != 0 { fr.num as f64 / fr.den as f64 } else { 0.0 },
                trc: (*ctx).color_trc as i32, colorspace: (*ctx).colorspace as i32,
                duration: if (*st).duration > 0 { (*st).duration as f64 * ff::av_q2d((*st).time_base) } else if (*fmt).duration > 0 { (*fmt).duration as f64 / ff::AV_TIME_BASE as f64 } else { 0.0 },
                start: if (*st).start_time != ff::AV_NOPTS_VALUE { (*st).start_time as f64 * ff::av_q2d((*st).time_base) } else { 0.0 },
                loop_at_eof: true, at_eof: false })
        }
    }
    /// Frame number of a pts (seconds), relative to the stream start.
    pub fn frame_no(&self, pts: f64) -> i64 { ((pts - self.start) * self.fps).round() as i64 }
    pub fn bytes_per_sample_hint(&self) -> u32 { if self.pix_fmt.contains("10") || self.pix_fmt.contains("12") || self.pix_fmt.contains("16") || self.pix_fmt.contains("f16") { 2 } else { 1 } }
    pub fn is_intra(&self) -> bool { matches!(self.codec.as_str(), "prores" | "dnxhd" | "exr" | "mjpeg" | "png" | "tiff" | "ffv1") }
    /// Seek so that the next decoded frame is the keyframe at or before `t` seconds (flushes the codec).
    pub fn seek(&mut self, t: f64) {
        unsafe {
            let ts = (t / self.time_base).round() as i64;
            ff::av_seek_frame(self.fmt, self.stream, ts, ff::AVSEEK_FLAG_BACKWARD);
            ff::avcodec_flush_buffers(self.ctx);
            self.at_eof = false;
        }
    }
    /// Seek + decode forward to the first frame with pts ≥ t − ½ frame. Returns (frame, frames decoded on the way).
    pub fn decode_to(&mut self, t: f64) -> Option<(Frame, u32)> {
        self.seek(t);
        let half = if self.fps > 0.0 { 0.5 / self.fps } else { 0.0 };
        let mut n = 0;
        loop { let f = self.next()?; n += 1; if f.pts >= t - half { return Some((f, n)); } }
    }
    /// Next decoded frame; loops to the start at EOF when `loop_at_eof` (S1), else returns None once.
    pub fn next(&mut self) -> Option<Frame> {
        unsafe {
            loop {
                let t0 = Instant::now();
                let r = ff::avcodec_receive_frame(self.ctx, self.frame);
                if r == 0 { return Some(self.wrap(t0)); }
                if r != averror(libc::EAGAIN) && r != ff::AVERROR_EOF { return None; }
                if r == ff::AVERROR_EOF { if !self.loop_at_eof { self.at_eof = true; return None; } ff::avcodec_flush_buffers(self.ctx); ff::av_seek_frame(self.fmt, self.stream, 0, ff::AVSEEK_FLAG_BACKWARD); }
                loop {
                    let rr = ff::av_read_frame(self.fmt, self.pkt);
                    if rr < 0 { ff::avcodec_send_packet(self.ctx, ptr::null()); break; }
                    if (*self.pkt).stream_index == self.stream { let s = ff::avcodec_send_packet(self.ctx, self.pkt); ff::av_packet_unref(self.pkt); if s < 0 && s != averror(libc::EAGAIN) { return None; } break; }
                    ff::av_packet_unref(self.pkt);
                }
            }
        }
    }
    unsafe fn wrap(&self, t0: Instant) -> Frame { unsafe {
        let f = &*self.frame;
        let pts = if f.pts != ff::AV_NOPTS_VALUE { f.pts as f64 * self.time_base } else { f.best_effort_timestamp as f64 * self.time_base };
        let full_range = f.color_range == ff::AVColorRange::AVCOL_RANGE_JPEG;
        let keyframe = (f.flags & ff::AV_FRAME_FLAG_KEY) != 0;
        let format = std::mem::transmute::<i32, ff::AVPixelFormat>(f.format);
        if format == ff::AVPixelFormat::AV_PIX_FMT_VIDEOTOOLBOX {
            let sw = (*self.ctx).sw_pix_fmt;
            let depth = match sw { ff::AVPixelFormat::AV_PIX_FMT_YUV420P10LE | ff::AVPixelFormat::AV_PIX_FMT_YUV422P10LE | ff::AVPixelFormat::AV_PIX_FMT_YUV444P10LE | ff::AVPixelFormat::AV_PIX_FMT_P010LE | ff::AVPixelFormat::AV_PIX_FMT_P210LE => 10, _ => 8 };
            return Frame { planes: vec![], vt: Some(VtFrame(ff::av_frame_clone(self.frame))), bytes_per_sample: if depth == 10 { 2 } else { 1 }, bit_depth: depth, full_range, pts, layout: Layout::Ycc, keyframe, decode_ms: t0.elapsed().as_secs_f32() * 1000.0 };
        }
        let (bps, depth, sx, sy, layout) = match format {
            ff::AVPixelFormat::AV_PIX_FMT_GBRPF16LE => (2, 16, 1, 1, Layout::RgbPlanarF16),
            ff::AVPixelFormat::AV_PIX_FMT_GBRAPF16LE => (2, 16, 1, 1, Layout::RgbPlanarF16),
            _ => { let (a, b, c, d) = match format {
            ff::AVPixelFormat::AV_PIX_FMT_YUV420P => (1, 8, 2, 2), ff::AVPixelFormat::AV_PIX_FMT_YUV422P => (1, 8, 2, 1), ff::AVPixelFormat::AV_PIX_FMT_YUV444P => (1, 8, 1, 1),
            ff::AVPixelFormat::AV_PIX_FMT_YUV420P10LE => (2, 10, 2, 2), ff::AVPixelFormat::AV_PIX_FMT_YUV422P10LE => (2, 10, 2, 1), ff::AVPixelFormat::AV_PIX_FMT_YUV444P10LE => (2, 10, 1, 1),
            other => panic!("unsupported pix_fmt {other:?}"),
            }; (a, b, c, d, Layout::Ycc) }
        };
        let mut planes = Vec::with_capacity(3);
        for i in 0..3 {
            let (w, h) = if i == 0 { (f.width as u32, f.height as u32) } else { ((f.width as u32).div_ceil(sx), (f.height as u32).div_ceil(sy)) };
            let row = w * bps; let pitch = row.div_ceil(256) * 256;
            let mut data = vec![0u8; (pitch * h) as usize];
            let src = f.data[i]; let ls = f.linesize[i] as usize;
            for y in 0..h as usize { ptr::copy_nonoverlapping(src.add(y * ls), data.as_mut_ptr().add(y * pitch as usize), row as usize); }
            planes.push(Plane { data, width: w, height: h, pitch });
        }
        Frame { planes, vt: None, bytes_per_sample: bps, bit_depth: depth, full_range, pts, layout, keyframe, decode_ms: t0.elapsed().as_secs_f32() * 1000.0 }
    } }
}
impl Drop for Decoder {
    fn drop(&mut self) { unsafe { ff::av_frame_free(&mut self.frame); ff::av_packet_free(&mut self.pkt); ff::avcodec_free_context(&mut self.ctx); if !self.hw.is_null() { ff::av_buffer_unref(&mut self.hw); } ff::avformat_close_input(&mut self.fmt); } }
}
