//! Thin RAII over the raw FFmpeg 9 bindings (canon §4: no safe wrapper covers hwaccel/zero-copy, so the
//! platform writes its own). Software path: planes repacked to 256-byte pitches for wgpu upload.
//! VideoToolbox path (`hw`): frames stay in CVPixelBuffers (`AVFrame.data[3]`), handed over by reference.
use ffmpeg_sys_next as ff;
use std::{ffi::CString, ptr, time::Instant};

pub struct Plane { pub data: Vec<u8>, pub width: u32, pub height: u32, pub pitch: u32 }
/// A VideoToolbox frame: an `av_frame_clone` reference that owns the CVPixelBuffer until dropped.
pub struct VtFrame(pub *mut ff::AVFrame);
unsafe impl Send for VtFrame {}
impl VtFrame { pub fn pixel_buffer(&self) -> *mut std::ffi::c_void { unsafe { (*self.0).data[3] as *mut _ } } }
impl Drop for VtFrame { fn drop(&mut self) { unsafe { ff::av_frame_free(&mut self.0) } } }

pub struct Frame {
    pub planes: Vec<Plane>,        // software path: Y, U, V
    pub vt: Option<VtFrame>,       // VideoToolbox path
    pub bytes_per_sample: u32,     // 1 or 2
    pub bit_depth: u32,            // 8 or 10
    pub full_range: bool,
    pub pts: f64,                  // seconds
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
    pub fn open(path: &str, threads: i32, hw: bool) -> Result<Self, String> {
        unsafe {
            let cpath = CString::new(path).unwrap();
            let mut fmt: *mut ff::AVFormatContext = ptr::null_mut();
            if ff::avformat_open_input(&mut fmt, cpath.as_ptr(), ptr::null_mut(), ptr::null_mut()) < 0 { return Err("avformat_open_input".into()); }
            if ff::avformat_find_stream_info(fmt, ptr::null_mut()) < 0 { return Err("find_stream_info".into()); }
            let mut dec: *const ff::AVCodec = ptr::null();
            let stream = ff::av_find_best_stream(fmt, ff::AVMediaType::AVMEDIA_TYPE_VIDEO, -1, -1, &mut dec, 0);
            if stream < 0 { return Err("no video stream".into()); }
            let st = *(*fmt).streams.add(stream as usize);
            let ctx = ff::avcodec_alloc_context3(dec);
            ff::avcodec_parameters_to_context(ctx, (*st).codecpar);
            (*ctx).thread_count = threads;
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
                trc: (*ctx).color_trc as i32, colorspace: (*ctx).colorspace as i32 })
        }
    }
    /// Next decoded frame; loops to the start at EOF (returns None only on a hard error).
    pub fn next(&mut self) -> Option<Frame> {
        unsafe {
            loop {
                let t0 = Instant::now();
                let r = ff::avcodec_receive_frame(self.ctx, self.frame);
                if r == 0 { return Some(self.wrap(t0)); }
                if r != averror(libc::EAGAIN) && r != ff::AVERROR_EOF { return None; }
                if r == ff::AVERROR_EOF { ff::avcodec_flush_buffers(self.ctx); ff::av_seek_frame(self.fmt, self.stream, 0, ff::AVSEEK_FLAG_BACKWARD); }
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
        let format = std::mem::transmute::<i32, ff::AVPixelFormat>(f.format);
        if format == ff::AVPixelFormat::AV_PIX_FMT_VIDEOTOOLBOX {
            let sw = (*self.ctx).sw_pix_fmt;
            let depth = match sw { ff::AVPixelFormat::AV_PIX_FMT_YUV420P10LE | ff::AVPixelFormat::AV_PIX_FMT_YUV422P10LE | ff::AVPixelFormat::AV_PIX_FMT_YUV444P10LE | ff::AVPixelFormat::AV_PIX_FMT_P010LE | ff::AVPixelFormat::AV_PIX_FMT_P210LE => 10, _ => 8 };
            return Frame { planes: vec![], vt: Some(VtFrame(ff::av_frame_clone(self.frame))), bytes_per_sample: if depth == 10 { 2 } else { 1 }, bit_depth: depth, full_range, pts, decode_ms: t0.elapsed().as_secs_f32() * 1000.0 };
        }
        let (bps, depth, sx, sy) = match format {
            ff::AVPixelFormat::AV_PIX_FMT_YUV420P => (1, 8, 2, 2), ff::AVPixelFormat::AV_PIX_FMT_YUV422P => (1, 8, 2, 1), ff::AVPixelFormat::AV_PIX_FMT_YUV444P => (1, 8, 1, 1),
            ff::AVPixelFormat::AV_PIX_FMT_YUV420P10LE => (2, 10, 2, 2), ff::AVPixelFormat::AV_PIX_FMT_YUV422P10LE => (2, 10, 2, 1), ff::AVPixelFormat::AV_PIX_FMT_YUV444P10LE => (2, 10, 1, 1),
            other => panic!("unsupported pix_fmt {other:?}"),
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
        Frame { planes, vt: None, bytes_per_sample: bps, bit_depth: depth, full_range, pts, decode_ms: t0.elapsed().as_secs_f32() * 1000.0 }
    } }
}
impl Drop for Decoder {
    fn drop(&mut self) { unsafe { ff::av_frame_free(&mut self.frame); ff::av_packet_free(&mut self.pkt); ff::avcodec_free_context(&mut self.ctx); if !self.hw.is_null() { ff::av_buffer_unref(&mut self.hw); } ff::avformat_close_input(&mut self.fmt); } }
}
