//! The FFmpeg session — the "common FFmpeg-session helper" QCView's Guide 13 §I.13 asked for, extracted from
//! `VideoDecoder::initFFmpeg` / `decodeLoop` (`QCView-Player/src/decode/video_decoder.cpp`): open in QCView's order
//! (first video stream, codec, hwaccel attach with `hw_routing`'s `get_format`, `thread_policy`, `avcodec_open2`,
//! metadata, the source timecode tag, the `FrameIndex` inputs with the duration / `stream_extent` fallbacks), the
//! packet / frame primitives, and the publish rules per frame kind (VideoToolbox zero-copy when the pixel buffer's
//! layout is bridged, else `av_hwframe_transfer_data` to planes; D3D12VA native; software planes through
//! `yuv_planar`). Raw bindings + thin RAII (canon §4.3): no safe wrapper covers hwaccel / zero-copy.
use crate::{frame_handle::FrameHandle, frame_index::FrameIndex, hw::{self, HwDevice}, hw_routing, read_ahead::{FrameSpan, ReadAhead}, rgb_range::{self, RangeOverride}, thread_policy, yuv_planar, Error};
use qcv_meta::stream_extent;
use ffmpeg_sys_next as ff;
use min_present::{Chroma, ColorMeta, Frame, Matrix, NativeFrame, Pixels, Plane, PlanarLayout, Primaries, Range, Transfer};
use qcv_timeline::Rate;
use std::{ffi::{CStr, CString}, ptr, time::Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeqOptions { pub rate: Rate, pub start_number: i64 }
#[derive(Clone, Default)]
pub struct OpenOptions { /// 0 = FFmpeg auto; the user's performance/ffmpegThreads value otherwise
    pub threads: i32, pub hw: Option<HwDevice>, /// the device for the compute-decoded intra codecs (Windows: FFmpeg's Vulkan device for ProRes / FFV1 / APV, S6); None = `hw` serves every codec
    pub hw_intra: Option<HwDevice>, /// an image sequence (the path is an image2 printf pattern)
    pub seq: Option<SeqOptions>, /// `AV_CODEC_FLAG2_FAST` (the thumbnail loader: speed over spec)
    pub fast: bool }

/// A hardware frame: an `av_frame_clone` reference that owns the platform buffer until dropped (QCView retains the
/// CVPixelBuffer itself; holding the AVFrame ref is the same contract and covers D3D12VA's pool slot too).
pub struct HwFrame(pub *mut ff::AVFrame);
unsafe impl Send for HwFrame {}
unsafe impl Sync for HwFrame {}
impl Drop for HwFrame { fn drop(&mut self) { unsafe { ff::av_frame_free(&mut self.0) } } }
#[cfg(windows)]
struct Dx12Keep { _frame: HwFrame, _handle: Box<min_present::Dx12FrameHandle> }
#[cfg(windows)] unsafe impl Send for Dx12Keep {}
#[cfg(windows)] unsafe impl Sync for Dx12Keep {}

/// What `open` learned (QCView's `metadataChanged` surface).
#[derive(Clone, Debug)]
pub struct SessionInfo {
    pub width: u32, pub height: u32, pub frame_count: i32, pub codec_name: String, pub codec_short: String, pub pix_fmt: String,
    /// "" when software, else the hwaccel's name
    pub hw_accel: String,
    /// e.g. "01:00:00;00" (`;` = drop-frame); None when the container has none
    pub source_timecode: Option<String>, pub drop_frame: bool,
    pub time_base: (i32, i32), pub frame_rate: (i32, i32), pub intra_only: bool,
    pub meta: ColorMeta, pub duration_secs: f64,
}
impl SessionInfo { pub fn rate(&self) -> Rate { Rate::from_rational(self.frame_rate.0, self.frame_rate.1).unwrap_or(Rate::new(24, 1)) } }

pub enum Read { Sent, Eof, Error(i32) }
pub enum Recv { Frame(FrameHandle), Again, Eof, Error(i32) }
/// The scrub decoder's raw receive: the session's own frame, valid until `release_raw` / the next receive.
pub enum RecvRaw { Frame(*mut ff::AVFrame), Again, Eof, Error(i32) }

pub struct Session {
    fmt: *mut ff::AVFormatContext, cctx: *mut ff::AVCodecContext, pkt: *mut ff::AVPacket, frame: *mut ff::AVFrame, sw_frame: *mut ff::AVFrame,
    _hw: Option<HwDevice>, pub stream_idx: i32, pub info: SessionInfo, pub range_override: RangeOverride,
    #[allow(dead_code)] logged_hw_fallback: bool, logged_native: bool, logged_planes: bool,
}
unsafe impl Send for Session {}

pub fn av_err(e: i32) -> String { let mut b = [0i8; 128]; unsafe { ff::av_strerror(e, b.as_mut_ptr(), b.len()); CStr::from_ptr(b.as_ptr()).to_string_lossy().into_owned() } }
fn averror(e: i32) -> i32 { -e }
fn cstr(p: *const std::ffi::c_char) -> String { if p.is_null() { String::new() } else { unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned() } }

fn meta_from(ctx: &ff::AVCodecContext, format: ff::AVPixelFormat, over: RangeOverride) -> ColorMeta {
    use ff::{AVColorPrimaries as P, AVColorTransferCharacteristic as T};
    let (depth, cw, ch, rgb) = unsafe { let d = ff::av_pix_fmt_desc_get(format); if d.is_null() { (8, 1, 1, false) } else { ((*d).comp[0].depth as u32, (*d).log2_chroma_w, (*d).log2_chroma_h, ((*d).flags as i32 & ff::AV_PIX_FMT_FLAG_RGB) != 0) } };
    let matrix = if rgb { Matrix::Identity } else { yuv_planar::effective_matrix(ctx.colorspace) };
    let transfer = match ctx.color_trc { T::AVCOL_TRC_IEC61966_2_1 => Transfer::Srgb, T::AVCOL_TRC_SMPTE2084 => Transfer::Pq, T::AVCOL_TRC_ARIB_STD_B67 => Transfer::Hlg, T::AVCOL_TRC_LINEAR => Transfer::Linear, _ => if rgb && depth >= 16 { Transfer::Linear } else { Transfer::Bt1886 } };
    let primaries = match ctx.color_primaries { P::AVCOL_PRI_BT2020 => Primaries::Bt2020, P::AVCOL_PRI_SMPTE432 | P::AVCOL_PRI_SMPTE431 => Primaries::P3D65, _ => Primaries::Bt709 };
    let chroma = if rgb { Chroma::Rgb } else { match (cw, ch) { (1, 1) => Chroma::C420, (1, 0) => Chroma::C422, _ => Chroma::C444 } };
    let sar = if ctx.sample_aspect_ratio.num > 0 && ctx.sample_aspect_ratio.den > 0 { (ctx.sample_aspect_ratio.num as u32, ctx.sample_aspect_ratio.den as u32) } else { (1, 1) };
    ColorMeta { matrix, range: yuv_planar::effective_source_range(ctx.color_range, rgb, over), range_forced: over != RangeOverride::Auto, transfer, primaries, bit_depth: depth, chroma, rotation: 0, sar, mastering: None }
}

impl Session {
    /// QCView's `initFFmpeg`, in its order.
    pub fn open(path: &str, opts: &OpenOptions) -> Result<Session, Error> {
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
            if r < 0 { return Err(Error::Open(format!("avformat_open_input: {}", av_err(r)))) }
            if ff::avformat_find_stream_info(fmt, ptr::null_mut()) < 0 { ff::avformat_close_input(&mut fmt); return Err(Error::Open("avformat_find_stream_info failed".into())) }
            // the FIRST video stream (QCView), not av_find_best_stream
            let mut stream_idx = -1;
            for i in 0..(*fmt).nb_streams { if (*(**(*fmt).streams.add(i as usize)).codecpar).codec_type == ff::AVMediaType::AVMEDIA_TYPE_VIDEO { stream_idx = i as i32; break } }
            if stream_idx < 0 { ff::avformat_close_input(&mut fmt); return Err(Error::NoVideo) }
            let st = *(*fmt).streams.add(stream_idx as usize);
            let codecpar = (*st).codecpar;
            let codec = ff::avcodec_find_decoder((*codecpar).codec_id);
            if codec.is_null() { ff::avformat_close_input(&mut fmt); return Err(Error::Open(format!("no decoder for codec id {:?}", (*codecpar).codec_id))) }
            let cctx = ff::avcodec_alloc_context3(codec);
            ff::avcodec_parameters_to_context(cctx, codecpar);
            // hwaccel: attach when the codec lists our device type and hw_routing allows it; get_format falls through to
            // software when the stream turns out not to use it (QCView: "a free win when it works, silent when it doesn't")
            let mut hw_accel = String::new(); let mut hw_keep = None;
            // QCView's per-codec routing: the compute-decoded intra codecs take `hw_intra` when the app provided one (Windows: Vulkan)
            let routed = if hw_routing::vulkan_preferred_codec((*codecpar).codec_id) { opts.hw_intra.as_ref().or(opts.hw.as_ref()) } else { opts.hw.as_ref() };
            if let Some(hw) = routed {
                if !hw_routing::software_only_codec((*codecpar).codec_id) {
                    let mut i = 0; let mut listed = false;
                    loop { let c = ff::avcodec_get_hw_config(codec, i); if c.is_null() { break } if (*c).device_type == hw.device_type() && ((*c).methods & ff::AV_CODEC_HW_CONFIG_METHOD_HW_DEVICE_CTX as i32) != 0 { listed = true; break } i += 1; }
                    if listed {
                        (*cctx).hw_device_ctx = ff::av_buffer_ref(hw.raw());
                        (*cctx).get_format = Some(hw_routing::hwaccel_get_format);
                        hw_accel = hw.name().to_string(); hw_keep = Some(hw.clone());
                    }
                } else { log::info!("codec=prores_raw → software decode (hw_routing: software-only codec)"); }
            }
            thread_policy::apply_software_thread_policy(cctx, codec, opts.threads);
            if opts.fast { (*cctx).flags2 |= ff::AV_CODEC_FLAG2_FAST as i32; }
            log::info!("threading {} (count={})", thread_policy::thread_policy_name(cctx), (*cctx).thread_count);
            if let e @ ..=-1 = ff::avcodec_open2(cctx, codec, ptr::null_mut()) { let mut c = cctx; ff::avcodec_free_context(&mut c); ff::avformat_close_input(&mut fmt); return Err(Error::Open(format!("avcodec_open2: {}", av_err(e)))) }
            // metadata
            let format = std::mem::transmute::<i32, ff::AVPixelFormat>((*codecpar).format);
            let mut frame_count = (*st).nb_frames as i32;
            let codec_name = { let l = cstr((*codec).long_name); if l.is_empty() { cstr((*codec).name) } else { l } };
            let pix_fmt = cstr(ff::av_get_pix_fmt_name(format));
            let tc_key = CString::new("timecode").unwrap();
            let tc = ff::av_dict_get((*st).metadata, tc_key.as_ptr(), ptr::null(), 0);
            let source_timecode = if tc.is_null() { None } else { Some(cstr((*tc).value)) };
            let drop_frame = source_timecode.as_deref().map(|s| s.contains(';')).unwrap_or(false);
            // FrameIndex inputs: time_base + guessed frame rate; total from nb_frames, else duration × fps, else a packet count
            let tb = (*st).time_base;
            let fr = ff::av_guess_frame_rate(fmt, st, ptr::null_mut());
            let (fr_num, fr_den) = if fr.num > 0 && fr.den > 0 { (fr.num, fr.den) } else if let Some(s) = &opts.seq { (s.rate.num as i32, s.rate.den as i32) } else { (24, 1) };
            let desc = ff::avcodec_descriptor_get((*codecpar).codec_id);
            let intra_only = !desc.is_null() && ((*desc).props & ff::AV_CODEC_PROP_INTRA_ONLY) != 0;
            let mut duration_secs = if (*st).duration > 0 { (*st).duration as f64 * ff::av_q2d(tb) } else if (*fmt).duration > 0 { (*fmt).duration as f64 / ff::AV_TIME_BASE as f64 } else { 0.0 };
            if frame_count <= 0 && (*fmt).duration > 0 { frame_count = ff::av_rescale_q((*fmt).duration, ff::AVRational { num: 1, den: ff::AV_TIME_BASE }, ff::AVRational { num: fr_den, den: fr_num }) as i32; }
            if frame_count <= 0 && (*st).duration > 0 { frame_count = ff::av_rescale_q((*st).duration, tb, ff::AVRational { num: fr_den, den: fr_num }) as i32; }
            if frame_count <= 0 {   // animated WebP / GIF / APNG: count the packets (stream_extent)
                let ext = stream_extent::scan_stream_extent(path, stream_idx, 512 << 20);
                if ext.ok { frame_count = ext.frames; if duration_secs <= 0.0 { duration_secs = ext.duration_us as f64 * 1e-6; } }
            }
            let info = SessionInfo { width: (*cctx).width as u32, height: (*cctx).height as u32, frame_count, codec_name, codec_short: cstr((*codec).name), pix_fmt, hw_accel, source_timecode, drop_frame, time_base: (tb.num, tb.den), frame_rate: (fr_num, fr_den), intra_only, meta: meta_from(&*cctx, format, RangeOverride::Auto), duration_secs };
            Ok(Session { fmt, cctx, pkt: ff::av_packet_alloc(), frame: ff::av_frame_alloc(), sw_frame: ff::av_frame_alloc(), _hw: hw_keep, stream_idx, info, range_override: RangeOverride::Auto, logged_hw_fallback: false, logged_native: false, logged_planes: false })
        }
    }
    pub fn frame_index(&self) -> FrameIndex { let i = &self.info; FrameIndex::new(i.time_base.0, i.time_base.1, i.frame_rate.0, i.frame_rate.1, i.frame_count, i.intra_only) }
    pub fn read_ahead_spans(&mut self, index: &FrameIndex) -> Vec<FrameSpan> { unsafe { ReadAhead::spans_from_index(self.fmt, self.stream_idx, &|ts| index.frame_for_pts(ts), index.total_frames()) } }
    pub fn time_base(&self) -> ff::AVRational { unsafe { let st = *(*self.fmt).streams.add(self.stream_idx as usize); (*st).time_base } }
    /// microseconds (AV_TIME_BASE) → this stream's time-base ticks
    pub fn us_to_pts(&self, us: i64) -> i64 { unsafe { ff::av_rescale_q(us, ff::AVRational { num: 1, den: 1_000_000 }, self.time_base()) } }
    pub fn start_pts(&self) -> i64 { unsafe { let st = *(*self.fmt).streams.add(self.stream_idx as usize); if (*st).start_time != ff::AV_NOPTS_VALUE { (*st).start_time } else { 0 } } }
    pub fn pts_to_us(&self, pts: i64) -> i64 { if pts == ff::AV_NOPTS_VALUE { return 0 } unsafe { let st = *(*self.fmt).streams.add(self.stream_idx as usize); ff::av_rescale_q(pts, (*st).time_base, ff::AVRational { num: 1, den: 1_000_000 }) } }
    /// `av_seek_frame` (or the reopen-to-seek demuxer's reopen) + a codec flush. QCView's performSeek preamble.
    pub fn seek(&mut self, pts: i64, flags: i32) -> i32 {
        unsafe { let r = crate::seek_compat::seek_stream(&mut self.fmt, self.stream_idx, pts, flags); ff::avcodec_flush_buffers(self.cctx); r }
    }
    /// Read the next packet and send the video ones to the codec.
    pub fn read_and_send(&mut self) -> Read {
        unsafe {
            loop {
                let rc = ff::av_read_frame(self.fmt, self.pkt);
                if rc == ff::AVERROR_EOF { ff::avcodec_send_packet(self.cctx, ptr::null()); return Read::Eof }
                if rc < 0 { return Read::Error(rc) }
                if (*self.pkt).stream_index == self.stream_idx {
                    let s = ff::avcodec_send_packet(self.cctx, self.pkt);
                    ff::av_packet_unref(self.pkt);
                    if s < 0 && s != averror(libc::EAGAIN) { log::warn!("send_packet: {}", av_err(s)); }
                    return Read::Sent;
                }
                ff::av_packet_unref(self.pkt);
            }
        }
    }
    /// One decoded frame if the codec has one ready, wrapped per its kind.
    pub fn receive(&mut self, index: &FrameIndex) -> Recv {
        unsafe {
            let t0 = Instant::now();
            let r = ff::avcodec_receive_frame(self.cctx, self.frame);
            if r == averror(libc::EAGAIN) { return Recv::Again }
            if r == ff::AVERROR_EOF { return Recv::Eof }
            if r < 0 { return Recv::Error(r) }
            let out = self.wrap(self.frame, index, t0);
            ff::av_frame_unref(self.frame);
            match out { Ok(h) => Recv::Frame(h), Err(e) => { log::warn!("{e}"); Recv::Again } }
        }
    }
    /// The raw form for the scrub decoder: the frame stays referenced until `release_raw` (or the next receive).
    pub fn receive_raw(&mut self) -> RecvRaw {
        unsafe {
            ff::av_frame_unref(self.frame);
            let r = ff::avcodec_receive_frame(self.cctx, self.frame);
            if r == averror(libc::EAGAIN) { return RecvRaw::Again }
            if r == ff::AVERROR_EOF { return RecvRaw::Eof }
            if r < 0 { return RecvRaw::Error(r) }
            RecvRaw::Frame(self.frame)
        }
    }
    pub fn release_raw(&mut self) { unsafe { ff::av_frame_unref(self.frame) } }
    /// `av_hwframe_transfer_data` of a hardware frame into the session's software landing frame (valid until the next call).
    pub fn hw_readback(&mut self, f: *mut ff::AVFrame) -> Option<*mut ff::AVFrame> {
        unsafe {
            ff::av_frame_unref(self.sw_frame);
            if ff::av_hwframe_transfer_data(self.sw_frame, f, 0) < 0 { return None }
            (*self.sw_frame).pts = (*f).pts; (*self.sw_frame).best_effort_timestamp = (*f).best_effort_timestamp;
            (*self.sw_frame).colorspace = (*f).colorspace; (*self.sw_frame).color_range = (*f).color_range; (*self.sw_frame).color_primaries = (*f).color_primaries; (*self.sw_frame).color_trc = (*f).color_trc;
            Some(self.sw_frame)
        }
    }
    /// A `FrameHandle` from any AVFrame of this session's stream (the scrub cache's entries).
    pub unsafe fn handle_from_avframe(&mut self, f: *mut ff::AVFrame, index: &FrameIndex) -> Result<FrameHandle, Error> { unsafe { self.wrap(f, index, Instant::now()) } }
    unsafe fn wrap(&mut self, frame: *mut ff::AVFrame, index: &FrameIndex, t0: Instant) -> Result<FrameHandle, Error> { unsafe {
        let f = &*frame;
        let pts = if f.best_effort_timestamp != ff::AV_NOPTS_VALUE { f.best_effort_timestamp } else { f.pts };
        let keyframe = (f.flags & ff::AV_FRAME_FLAG_KEY) != 0;
        let format = std::mem::transmute::<i32, ff::AVPixelFormat>(f.format);
        let (w, h) = (f.width as u32, f.height as u32);
        let mut meta = self.info.meta;
        meta.range = yuv_planar::effective_source_range(f.color_range, meta.matrix == Matrix::Identity, self.range_override);
        meta.range_forced = self.range_override != RangeOverride::Auto;
        let fin = |frame: Frame, s: &Session| FrameHandle { frame, pts, pts_us: s.pts_to_us(pts), frame_no: index.frame_for_pts(pts), keyframe, decode_ms: t0.elapsed().as_secs_f32() * 1000.0 };
        if hw::is_hw_format(format) {
            // Vulkan (S6 milestone 1): the GPU decoded it; until the D3D12 bridge lands the frame is read back to planes
            if format == ff::AVPixelFormat::AV_PIX_FMT_VULKAN {
                if !self.logged_hw_fallback {
                    // the frame's Vulkan layout, for the bridge's design: one multiplane image or one image per plane, the VkFormats, the tiling
                    let mut desc = String::new();
                    if !f.hw_frames_ctx.is_null() {
                        let fc = &*((*f.hw_frames_ctx).data as *const ff::AVHWFramesContext);
                        let vk = &*(fc.hwctx as *const ff::AVVulkanFramesContext);
                        let vkf = &*(f.data[0] as *const ff::AVVkFrame);
                        let n_img = vkf.img.iter().take_while(|i| **i != 0).count();
                        let fmts: Vec<String> = vk.format.iter().take_while(|x| **x != 0).map(|x| format!("{x}")).collect();
                        desc = format!(" — sw_format {}, {} VkImage(s), VkFormat(s) [{}], tiling {}, frame flags {:#x}, nb_layers {}", cstr(ff::av_get_pix_fmt_name(fc.sw_format)), n_img, fmts.join(" "), vkf.tiling, vk.flags as u64, vk.nb_layers);
                    }
                    log::info!("Vulkan decoded ({w}x{h}){desc} — hwframe_transfer + planes (the D3D12 bridge is S6's next milestone)"); self.logged_hw_fallback = true;
                }
                let r = ff::av_hwframe_transfer_data(self.sw_frame, frame, 0);
                if r < 0 { return Err(Error::Open(format!("hwframe_transfer_data (vulkan): {}", av_err(r)))) }
                (*self.sw_frame).color_range = f.color_range; (*self.sw_frame).colorspace = f.colorspace; (*self.sw_frame).color_primaries = f.color_primaries; (*self.sw_frame).color_trc = f.color_trc;
                let out = self.planes_from(&*self.sw_frame, meta, index, t0, pts, keyframe);
                ff::av_frame_unref(self.sw_frame);
                return out;
            }
            #[cfg(target_os = "macos")]
            {
                let pix = f.data[3] as *mut std::ffi::c_void;
                if !pix.is_null() && min_present::import::metal::zero_copy_supported(pix) {
                    if !self.logged_native { log::info!("METAL zero-copy publish — '{}' ({w}x{h})", min_present::import::metal::fourcc_name(pix)); self.logged_native = true; }
                    let hwf = HwFrame(ff::av_frame_clone(frame));
                    return Ok(fin(Frame { width: w, height: h, pixels: Pixels::Native(NativeFrame { handle: pix, keep: std::sync::Arc::new(hwf) }), meta }, self));
                }
                // VideoToolbox emits a layout the bridge doesn't wrap: hwframe_transfer + planes — still Media-Engine decoded
                if !self.logged_hw_fallback { log::info!("VT decoded, but '{}' is not in the zero-copy list — hwframe_transfer + planes", if pix.is_null() { "null".into() } else { min_present::import::metal::fourcc_name(pix) }); self.logged_hw_fallback = true; }
                let r = ff::av_hwframe_transfer_data(self.sw_frame, frame, 0);
                if r < 0 { return Err(Error::Open(format!("hwframe_transfer_data: {}", av_err(r)))) }
                let out = self.planes_from(&*self.sw_frame, meta, index, t0, pts, keyframe);
                ff::av_frame_unref(self.sw_frame);
                return out;
            }
            #[cfg(windows)]
            {
                let af = &*((*frame).data[0] as *const crate::hw::AVD3D12VAFrame);
                let hwf = HwFrame(ff::av_frame_clone(frame));
                let handle = Box::new(min_present::Dx12FrameHandle { resource: af.texture, fence: af.sync_ctx.fence, fence_value: af.sync_ctx.fence_value });
                let p = &*handle as *const _ as *mut std::ffi::c_void;
                if !self.logged_native { log::info!("D3D12VA zero-copy publish ({w}x{h})"); self.logged_native = true; }
                return Ok(fin(Frame { width: w, height: h, pixels: Pixels::Native(NativeFrame { handle: p, keep: std::sync::Arc::new(Dx12Keep { _frame: hwf, _handle: handle }) }), meta }, self));
            }
            #[cfg(not(any(target_os = "macos", windows)))]
            { let _ = (w, h); unreachable!() }
        }
        self.planes_from(f, meta, index, t0, pts, keyframe)
    } }
    /// Software planes: the layout from `yuv_planar`, rows padded to 256 bytes for the presenter's upload.
    unsafe fn planes_from(&mut self, f: &ff::AVFrame, mut meta: ColorMeta, index: &FrameIndex, t0: Instant, pts: i64, keyframe: bool) -> Result<FrameHandle, Error> { unsafe {
        let format = std::mem::transmute::<i32, ff::AVPixelFormat>(f.format);
        let l = yuv_planar::layout_for(format);
        if !l.ok { return Err(Error::PixFmt(cstr(ff::av_get_pix_fmt_name(format)))) }
        let (w, h) = (f.width as u32, f.height as u32);
        if !self.logged_planes { log::info!("CPU publish path — pix_fmt={} ({w}x{h}) → GPU planes{}", self.info.pix_fmt, if l.rgb { " (planar RGB)" } else { "" }); self.logged_planes = true; }
        let layout = if l.rgb {
            meta.matrix = Matrix::Identity; meta.chroma = Chroma::Rgb; if l.float && meta.transfer == Transfer::Bt1886 { meta.transfer = Transfer::Linear }
            PlanarLayout::Rgb { bytes_per_sample: l.bytes, code_bits: l.code_bits, float: l.float, alpha: l.alpha, legal_expand: rgb_range::rgb_frame_needs_legal_expansion(f, self.range_override) }
        } else { PlanarLayout::Ycc { bytes_per_sample: l.bytes, code_bits: l.code_bits, interleaved_chroma: l.interleaved, alpha: l.alpha } };
        if l.rgb { meta.range = Range::Full; }
        let mut planes = Vec::with_capacity(l.plane_count);
        for p in 0..l.plane_count {
            let chroma = !l.rgb && p > 0 && !(l.alpha && p == l.plane_count - 1);
            let (pw, ph) = if chroma { (w.div_ceil(1 << l.shift_x), h.div_ceil(1 << l.shift_y)) } else { (w, h) };
            let channels = if chroma && l.interleaved { 2 } else { 1 };
            let row = pw * l.bytes * channels; let pitch = row.div_ceil(256) * 256;
            let mut data = vec![0u8; (pitch * ph) as usize];
            let src = f.data[l.data_index[p]]; let ls = f.linesize[l.data_index[p]] as usize;
            for y in 0..ph as usize { ptr::copy_nonoverlapping(src.add(y * ls), data.as_mut_ptr().add(y * pitch as usize), row as usize); }
            planes.push(Plane { data, width: pw, height: ph, pitch });
        }
        Ok(FrameHandle { frame: Frame { width: w, height: h, pixels: Pixels::Planar { planes, layout }, meta }, pts, pts_us: self.pts_to_us(pts), frame_no: index.frame_for_pts(pts), keyframe, decode_ms: t0.elapsed().as_secs_f32() * 1000.0 })
    } }
}
impl Drop for Session {
    fn drop(&mut self) { unsafe { ff::av_frame_free(&mut self.sw_frame); ff::av_frame_free(&mut self.frame); ff::av_packet_free(&mut self.pkt); ff::avcodec_free_context(&mut self.cctx); ff::avformat_close_input(&mut self.fmt); } }
}
