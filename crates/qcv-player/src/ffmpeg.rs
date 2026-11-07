//! Thin RAII over the raw FFmpeg 9 bindings (canon §4.3): demux + decode of one video stream, the platform hwaccel
//! through a shared `HwDevice`, colour metadata from the stream's tags, software frames repacked to 256-byte pitches
//! for the presenter's upload path, hardware frames handed over by reference.
use crate::{hw::{HwDevice, HW_PIX_FMT}, Error};
use ffmpeg_sys_next as ff;
use min_present::{Chroma, ColorMeta, Frame, Matrix, NativeFrame, Pixels, Plane, PlanarLayout, Primaries, Range, Transfer};
use qcv_timeline::Rate;
use std::{ffi::CString, ptr, sync::Arc, time::Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeqOptions { pub rate: Rate, pub start_number: i64 }
#[derive(Clone, Default)]
pub struct OpenOptions { /// 0 = FFmpeg's choice
    pub threads: i32, pub hw: Option<HwDevice>, /// SLICE threading only: one frame in, one frame out (the scrub decoder on intra codecs)
    pub low_latency: bool, /// an image sequence (the path is an image2 printf pattern)
    pub seq: Option<SeqOptions> }

/// A hardware frame: an `av_frame_clone` reference that owns the platform buffer until dropped.
pub struct HwFrame(pub *mut ff::AVFrame);
unsafe impl Send for HwFrame {}
unsafe impl Sync for HwFrame {}   // only ever read (the platform handle) once cloned
impl Drop for HwFrame { fn drop(&mut self) { unsafe { ff::av_frame_free(&mut self.0) } } }
impl Clone for HwFrame { fn clone(&self) -> Self { HwFrame(unsafe { ff::av_frame_clone(self.0) }) } }
#[cfg(windows)]
struct Dx12Keep { _frame: HwFrame, handle: Box<min_present::Dx12FrameHandle> }
#[cfg(windows)] unsafe impl Send for Dx12Keep {}
#[cfg(windows)] unsafe impl Sync for Dx12Keep {}

pub struct Decoded { pub frame: Frame, /// seconds
    pub pts: f64, pub keyframe: bool, /// receive_frame wall time incl. the repack, ms
    pub decode_ms: f32 }

pub struct Decoder {
    fmt: *mut ff::AVFormatContext, ctx: *mut ff::AVCodecContext, pkt: *mut ff::AVPacket, frame: *mut ff::AVFrame,
    _hw: Option<HwDevice>, stream: i32, time_base: f64,
    pub width: u32, pub height: u32, pub codec: String, pub pix_fmt: String,
    pub rate: Rate, pub fps: f64, pub meta: ColorMeta,
    /// seconds (stream or container)
    pub duration: f64, /// the stream's first pts in seconds (0 when untagged)
    pub start: f64,
    pub loop_at_eof: bool, pub at_eof: bool, pub hw_active: bool,
}
unsafe impl Send for Decoder {}

fn averror(e: i32) -> i32 { -e }

unsafe extern "C" fn pick_hw(_ctx: *mut ff::AVCodecContext, fmts: *const ff::AVPixelFormat) -> ff::AVPixelFormat {
    unsafe { let mut p = fmts; while *p != ff::AVPixelFormat::AV_PIX_FMT_NONE { if *p == HW_PIX_FMT { return *p; } p = p.add(1); } *fmts }
}

fn meta_from(ctx: &ff::AVCodecContext, format: ff::AVPixelFormat) -> ColorMeta {
    use ff::{AVColorPrimaries as P, AVColorSpace as S, AVColorTransferCharacteristic as T};
    let (depth, cw, ch, rgb) = unsafe { let d = ff::av_pix_fmt_desc_get(format); if d.is_null() { (8, 1, 1, false) } else { ((*d).comp[0].depth as u32, (*d).log2_chroma_w, (*d).log2_chroma_h, ((*d).flags & ff::AV_PIX_FMT_FLAG_RGB as u64) != 0) } };
    let sd = ctx.height <= 576;
    let matrix = if rgb { Matrix::Identity } else { match ctx.colorspace { S::AVCOL_SPC_BT709 => Matrix::Bt709, S::AVCOL_SPC_BT470BG | S::AVCOL_SPC_SMPTE170M | S::AVCOL_SPC_SMPTE240M => Matrix::Bt601, S::AVCOL_SPC_BT2020_NCL | S::AVCOL_SPC_BT2020_CL => Matrix::Bt2020, S::AVCOL_SPC_RGB => Matrix::Identity, _ => if sd { Matrix::Bt601 } else { Matrix::Bt709 } } };
    let transfer = match ctx.color_trc { T::AVCOL_TRC_IEC61966_2_1 => Transfer::Srgb, T::AVCOL_TRC_SMPTE2084 => Transfer::Pq, T::AVCOL_TRC_ARIB_STD_B67 => Transfer::Hlg, T::AVCOL_TRC_LINEAR => Transfer::Linear, _ => if rgb && depth >= 16 { Transfer::Linear } else { Transfer::Bt1886 } };
    let primaries = match ctx.color_primaries { P::AVCOL_PRI_BT2020 => Primaries::Bt2020, P::AVCOL_PRI_SMPTE432 | P::AVCOL_PRI_SMPTE431 => Primaries::P3D65, _ => Primaries::Bt709 };
    let chroma = if rgb { Chroma::Rgb } else { match (cw, ch) { (1, 1) => Chroma::C420, (1, 0) => Chroma::C422, _ => Chroma::C444 } };
    let sar = if ctx.sample_aspect_ratio.num > 0 && ctx.sample_aspect_ratio.den > 0 { (ctx.sample_aspect_ratio.num as u32, ctx.sample_aspect_ratio.den as u32) } else { (1, 1) };
    ColorMeta { matrix, range: if ctx.color_range == ff::AVColorRange::AVCOL_RANGE_JPEG { Range::Full } else { Range::Limited }, transfer, primaries, bit_depth: depth, chroma, rotation: 0, sar, mastering: None }
}

impl Decoder {
    pub fn open(path: &str, opts: &OpenOptions) -> Result<Self, Error> {
        unsafe {
            let cpath = CString::new(path).map_err(|_| Error::Open("path".into()))?;
            let mut fmt: *mut ff::AVFormatContext = ptr::null_mut();
            let mut dict: *mut ff::AVDictionary = ptr::null_mut();
            if let Some(seq) = &opts.seq {
                let set = |d: &mut *mut ff::AVDictionary, k: &str, v: &str| { let k = CString::new(k).unwrap(); let v = CString::new(v).unwrap(); ff::av_dict_set(d, k.as_ptr(), v.as_ptr(), 0); };
                set(&mut dict, "framerate", &format!("{}/{}", seq.rate.num, seq.rate.den));
                set(&mut dict, "start_number", &seq.start_number.to_string());
            }
            let r = ff::avformat_open_input(&mut fmt, cpath.as_ptr(), ptr::null_mut(), &mut dict);
            ff::av_dict_free(&mut dict);
            if r < 0 { return Err(Error::Open(format!("avformat_open_input({path}) = {r}"))); }
            if ff::avformat_find_stream_info(fmt, ptr::null_mut()) < 0 { ff::avformat_close_input(&mut fmt); return Err(Error::Open("find_stream_info".into())); }
            let mut dec: *const ff::AVCodec = ptr::null();
            let stream = ff::av_find_best_stream(fmt, ff::AVMediaType::AVMEDIA_TYPE_VIDEO, -1, -1, &mut dec, 0);
            if stream < 0 { ff::avformat_close_input(&mut fmt); return Err(Error::NoVideo); }
            let st = *(*fmt).streams.add(stream as usize);
            let ctx = ff::avcodec_alloc_context3(dec);
            ff::avcodec_parameters_to_context(ctx, (*st).codecpar);
            (*ctx).thread_count = opts.threads;
            if opts.low_latency { (*ctx).thread_type = ff::FF_THREAD_SLICE as i32; }
            // the hwaccel only when this codec advertises it for our device type (FFmpeg's exr / ProRes-on-NVIDIA decode in software)
            let mut hw_active = false;
            if let Some(hw) = &opts.hw {
                let mut i = 0; let mut supported = false;
                loop { let c = ff::avcodec_get_hw_config(dec, i); if c.is_null() { break } if (*c).device_type == hw.device_type() && ((*c).methods & ff::AV_CODEC_HW_CONFIG_METHOD_HW_DEVICE_CTX as i32) != 0 { supported = true; break } i += 1; }
                if supported && HW_PIX_FMT != ff::AVPixelFormat::AV_PIX_FMT_NONE {
                    (*ctx).hw_device_ctx = ff::av_buffer_ref(hw.raw());
                    (*ctx).get_format = Some(pick_hw);
                    hw_active = true;
                }
            }
            if ff::avcodec_open2(ctx, dec, ptr::null_mut()) < 0 { let mut c = ctx; ff::avcodec_free_context(&mut c); ff::avformat_close_input(&mut fmt); return Err(Error::Open("avcodec_open2".into())); }
            let fr = (*st).avg_frame_rate;
            let rate = Rate::from_rational(fr.num, fr.den).or_else(|| opts.seq.map(|s| s.rate)).unwrap_or(Rate::new(24, 1));
            let codec = std::ffi::CStr::from_ptr((*dec).name).to_string_lossy().into_owned();
            let format = std::mem::transmute::<i32, ff::AVPixelFormat>((*(*st).codecpar).format);
            let pf = ff::av_get_pix_fmt_name(format);
            let pix_fmt = if pf.is_null() { "?".into() } else { std::ffi::CStr::from_ptr(pf).to_string_lossy().into_owned() };
            let meta = meta_from(&*ctx, format);
            Ok(Decoder { fmt, ctx, pkt: ff::av_packet_alloc(), frame: ff::av_frame_alloc(), _hw: if hw_active { opts.hw.clone() } else { None }, stream, time_base: ff::av_q2d((*st).time_base),
                width: (*ctx).width as u32, height: (*ctx).height as u32, codec, pix_fmt, rate, fps: rate.as_f64(), meta,
                duration: if (*st).duration > 0 { (*st).duration as f64 * ff::av_q2d((*st).time_base) } else if (*fmt).duration > 0 { (*fmt).duration as f64 / ff::AV_TIME_BASE as f64 } else { 0.0 },
                start: if (*st).start_time != ff::AV_NOPTS_VALUE { (*st).start_time as f64 * ff::av_q2d((*st).time_base) } else { 0.0 },
                loop_at_eof: false, at_eof: false, hw_active })
        }
    }
    /// Frame count from the duration at the stream's rate.
    pub fn frames(&self) -> i64 { (self.duration * self.fps).round() as i64 }
    /// Frame number of a pts (seconds), relative to the stream start.
    pub fn frame_no(&self, pts: f64) -> i64 { ((pts - self.start) * self.fps).round() as i64 }
    pub fn is_intra(&self) -> bool { matches!(self.codec.as_str(), "prores" | "dnxhd" | "exr" | "mjpeg" | "png" | "tiff" | "ffv1" | "dpx") }
    /// Seek so that the next decoded frame is the keyframe at or before `t` seconds (flushes the codec).
    pub fn seek(&mut self, t: f64) {
        unsafe { let ts = (t / self.time_base).round() as i64; ff::av_seek_frame(self.fmt, self.stream, ts, ff::AVSEEK_FLAG_BACKWARD); ff::avcodec_flush_buffers(self.ctx); self.at_eof = false; }
    }
    /// Seek + decode forward to the first frame with pts ≥ t − ½ frame. Returns (frame, frames decoded on the way).
    pub fn decode_to(&mut self, t: f64) -> Option<(Decoded, u32)> {
        self.seek(t);
        let half = if self.fps > 0.0 { 0.5 / self.fps } else { 0.0 };
        let mut n = 0;
        loop { let f = self.next()?; n += 1; if f.pts >= t - half { return Some((f, n)); } }
    }
    /// Next decoded frame; loops to the start at EOF when `loop_at_eof`, else returns None once (`at_eof` set).
    pub fn next(&mut self) -> Option<Decoded> {
        unsafe {
            loop {
                let t0 = Instant::now();
                let r = ff::avcodec_receive_frame(self.ctx, self.frame);
                if r == 0 { return self.wrap(t0).map_err(|e| log::warn!("{e}")).ok(); }
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
    unsafe fn wrap(&self, t0: Instant) -> Result<Decoded, Error> { unsafe {
        let f = &*self.frame;
        let pts = if f.pts != ff::AV_NOPTS_VALUE { f.pts as f64 * self.time_base } else { f.best_effort_timestamp as f64 * self.time_base };
        let keyframe = (f.flags & ff::AV_FRAME_FLAG_KEY) != 0;
        let mut meta = self.meta;
        if f.color_range == ff::AVColorRange::AVCOL_RANGE_JPEG { meta.range = Range::Full } else if f.color_range == ff::AVColorRange::AVCOL_RANGE_MPEG { meta.range = Range::Limited }
        let format = std::mem::transmute::<i32, ff::AVPixelFormat>(f.format);
        let (w, h) = (f.width as u32, f.height as u32);
        if format == HW_PIX_FMT && format != ff::AVPixelFormat::AV_PIX_FMT_NONE {
            let hwf = HwFrame(ff::av_frame_clone(self.frame));
            #[cfg(target_os = "macos")]
            let native = { let handle = (*hwf.0).data[3] as *mut std::ffi::c_void; NativeFrame { handle, keep: Arc::new(hwf) } };
            #[cfg(windows)]
            let native = {
                let af = &*((*hwf.0).data[0] as *const crate::hw::AVD3D12VAFrame);
                let handle = Box::new(min_present::Dx12FrameHandle { resource: af.texture, fence: af.sync_ctx.fence, fence_value: af.sync_ctx.fence_value });
                let ptr = &*handle as *const _ as *mut std::ffi::c_void;
                NativeFrame { handle: ptr, keep: Arc::new(Dx12Keep { _frame: hwf, handle }) }
            };
            #[cfg(not(any(target_os = "macos", windows)))]
            let native = { let _ = hwf; unreachable!() };
            return Ok(Decoded { frame: Frame { width: w, height: h, pixels: Pixels::Native(native), meta }, pts, keyframe, decode_ms: t0.elapsed().as_secs_f32() * 1000.0 });
        }
        let (bps, sx, sy, layout) = match format {
            ff::AVPixelFormat::AV_PIX_FMT_GBRPF16LE | ff::AVPixelFormat::AV_PIX_FMT_GBRAPF16LE => (2, 1, 1, PlanarLayout::RgbF16),
            ff::AVPixelFormat::AV_PIX_FMT_YUV420P => (1, 2, 2, PlanarLayout::Ycc { bytes_per_sample: 1 }), ff::AVPixelFormat::AV_PIX_FMT_YUV422P => (1, 2, 1, PlanarLayout::Ycc { bytes_per_sample: 1 }), ff::AVPixelFormat::AV_PIX_FMT_YUV444P => (1, 1, 1, PlanarLayout::Ycc { bytes_per_sample: 1 }),
            ff::AVPixelFormat::AV_PIX_FMT_YUV420P10LE | ff::AVPixelFormat::AV_PIX_FMT_YUV420P12LE => (2, 2, 2, PlanarLayout::Ycc { bytes_per_sample: 2 }), ff::AVPixelFormat::AV_PIX_FMT_YUV422P10LE | ff::AVPixelFormat::AV_PIX_FMT_YUV422P12LE => (2, 2, 1, PlanarLayout::Ycc { bytes_per_sample: 2 }), ff::AVPixelFormat::AV_PIX_FMT_YUV444P10LE | ff::AVPixelFormat::AV_PIX_FMT_YUV444P12LE => (2, 1, 1, PlanarLayout::Ycc { bytes_per_sample: 2 }),
            other => { let n = ff::av_get_pix_fmt_name(other); return Err(Error::PixFmt(if n.is_null() { format!("{other:?}") } else { std::ffi::CStr::from_ptr(n).to_string_lossy().into_owned() })) }
        };
        if layout == PlanarLayout::RgbF16 { meta.matrix = Matrix::Identity; meta.chroma = Chroma::Rgb; if meta.transfer == Transfer::Bt1886 { meta.transfer = Transfer::Linear } meta.bit_depth = 16; }
        let mut planes = Vec::with_capacity(3);
        for i in 0..3 {
            let (pw, ph) = if i == 0 { (w, h) } else { (w.div_ceil(sx), h.div_ceil(sy)) };
            let (pw, ph) = if layout == PlanarLayout::RgbF16 { (w, h) } else { (pw, ph) };
            let row = pw * bps; let pitch = row.div_ceil(256) * 256;
            let mut data = vec![0u8; (pitch * ph) as usize];
            let src = f.data[i]; let ls = f.linesize[i] as usize;
            for y in 0..ph as usize { ptr::copy_nonoverlapping(src.add(y * ls), data.as_mut_ptr().add(y * pitch as usize), row as usize); }
            planes.push(Plane { data, width: pw, height: ph, pitch });
        }
        Ok(Decoded { frame: Frame { width: w, height: h, pixels: Pixels::Planar { planes, layout }, meta }, pts, keyframe, decode_ms: t0.elapsed().as_secs_f32() * 1000.0 })
    } }
}
impl Drop for Decoder {
    fn drop(&mut self) { unsafe { ff::av_frame_free(&mut self.frame); ff::av_packet_free(&mut self.pkt); ff::avcodec_free_context(&mut self.ctx); ff::avformat_close_input(&mut self.fmt); } }
}
