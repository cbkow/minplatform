//! `MultiStreamAudioDecoder` (QCView `audio/multi_stream_audio_decoder.cpp`): a file with ≥ 2 audio streams — one
//! codec per stream feeding an avfilter graph (`abuffer`×N → `amerge` → `pan` → `aformat` flt/48k/stereo →
//! `abuffersink`) swapped atomically on a seek or a routing change. The routing plan picks WHICH streams to merge and
//! the pan expression (Auto: 7–8 when ≥ 8 channels, a BS.775 fold when ≥ 6, else the first pair; the pair across
//! streams gets `pan=stereo|c0=cL|c1=cR`, a same-stream stereo pair passes through). Peaks per real channel before the
//! fold, 16-channel cap. Seeks go through `avformat_seek_file` on the whole file, every codec flushed, the graph rebuilt.
use super::drain::AudioSource;
use super::ffmpeg_audio::{OUT_CHANNELS, OUT_RATE};
use super::ring::AudioRing;
use super::routing::RoutingMode;
use super::source::AudioSourceCtl;
use super::tempo::{TempoControl, TempoStage};
use crate::ffmpeg::av_err;
use crate::Error;
use ffmpeg_sys_next as ff;
use std::collections::BTreeSet;
use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering::*};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const RING_SAMPLES: usize = 192000 / 4;
const BACK_PRESSURE_SAMPLES: usize = 38400 / 4;
const AV_OPT_SEARCH_CHILDREN: i32 = 1;
const AV_BUFFERSRC_FLAG_KEEP_REF: i32 = 8;

struct Shared {
    ring: AudioRing, running: AtomicBool,
    seek: Mutex<Option<f64>>, seek_cv: Condvar, seek_requested: AtomicBool, eof: AtomicBool,
    decode_pos: AtomicU64, read_pos: AtomicU64, last_seek: AtomicU64,
    peaks: [AtomicU32; 16], routing: AtomicI32, routing_dirty: AtomicBool,
}
fn f2b(v: f64) -> u64 { v.to_bits() } fn b2f(b: u64) -> f64 { f64::from_bits(b) }

/// The FFmpeg state owned by the decode thread.
struct Ctx { fmt: *mut ff::AVFormatContext, decoders: Vec<*mut ff::AVCodecContext>, global_to_audio: Vec<i32>, stream_channels: Vec<usize>, channel_offset: Vec<usize>, pkt: *mut ff::AVPacket, frame: *mut ff::AVFrame, out_frame: *mut ff::AVFrame, pipeline: Option<Pipeline> }
unsafe impl Send for Ctx {}
struct Pipeline { graph: *mut ff::AVFilterGraph, buffersrcs: Vec<*mut ff::AVFilterContext>, sink: *mut ff::AVFilterContext }
impl Drop for Pipeline { fn drop(&mut self) { unsafe { if !self.graph.is_null() { ff::avfilter_graph_free(&mut self.graph); } } } }
impl Drop for Ctx {
    fn drop(&mut self) { unsafe {
        self.pipeline = None;
        for d in &mut self.decoders { if !d.is_null() { ff::avcodec_free_context(d); } }
        if !self.pkt.is_null() { ff::av_packet_free(&mut self.pkt); } if !self.frame.is_null() { ff::av_frame_free(&mut self.frame); } if !self.out_frame.is_null() { ff::av_frame_free(&mut self.out_frame); }
        if !self.fmt.is_null() { ff::avformat_close_input(&mut self.fmt); }
    } }
}

pub struct MultiStreamAudioDecoder {
    sh: Arc<Shared>, thread: Mutex<Option<JoinHandle<()>>>, ctx: Mutex<Option<(Ctx, TempoStage)>>, tempo: Arc<TempoControl>,
    pub duration: f64, pub stream_count: usize, pub total_channels: usize, pub layout_name: String, t0: Instant,
}

/// How many decodable audio streams a file has (the player's dispatch: ≥ 2 → this decoder).
pub fn audio_stream_count(path: &str) -> usize {
    unsafe {
        let Ok(c) = CString::new(path) else { return 0 };
        let mut fmt: *mut ff::AVFormatContext = ptr::null_mut();
        if ff::avformat_open_input(&mut fmt, c.as_ptr(), ptr::null_mut(), ptr::null_mut()) < 0 { return 0 }
        let n = if ff::avformat_find_stream_info(fmt, ptr::null_mut()) >= 0 { (0..(*fmt).nb_streams).filter(|&i| (*(**(*fmt).streams.add(i as usize)).codecpar).codec_type == ff::AVMediaType::AVMEDIA_TYPE_AUDIO).count() } else { 0 };
        ff::avformat_close_input(&mut fmt); n
    }
}

impl MultiStreamAudioDecoder {
    pub fn open(path: &str, routing: RoutingMode) -> Result<Option<MultiStreamAudioDecoder>, Error> {
        unsafe {
            let cpath = CString::new(path).map_err(|_| Error::Open("path".into()))?;
            let mut fmt: *mut ff::AVFormatContext = ptr::null_mut();
            let r = ff::avformat_open_input(&mut fmt, cpath.as_ptr(), ptr::null_mut(), ptr::null_mut());
            if r < 0 { return Err(Error::Open(format!("audio: avformat_open_input: {}", av_err(r)))) }
            if ff::avformat_find_stream_info(fmt, ptr::null_mut()) < 0 { ff::avformat_close_input(&mut fmt); return Err(Error::Open("audio: find_stream_info".into())) }
            let duration = if (*fmt).duration > 0 { (*fmt).duration as f64 / ff::AV_TIME_BASE as f64 } else { 0.0 };
            let mut decoders = vec![]; let mut global_to_audio = vec![-1i32; (*fmt).nb_streams as usize]; let mut stream_channels = vec![];
            for i in 0..(*fmt).nb_streams {
                let st = *(*fmt).streams.add(i as usize); let cp = (*st).codecpar;
                if (*cp).codec_type != ff::AVMediaType::AVMEDIA_TYPE_AUDIO { continue }
                let codec = ff::avcodec_find_decoder((*cp).codec_id); if codec.is_null() { continue }
                let cctx = ff::avcodec_alloc_context3(codec); if cctx.is_null() { continue }
                (*cctx).pkt_timebase = (*st).time_base;
                if ff::avcodec_parameters_to_context(cctx, cp) < 0 || ff::avcodec_open2(cctx, codec, ptr::null_mut()) < 0 { let mut c = cctx; ff::avcodec_free_context(&mut c); continue }
                global_to_audio[i as usize] = decoders.len() as i32; decoders.push(cctx); stream_channels.push((*cctx).ch_layout.nb_channels.max(0) as usize);
            }
            if decoders.is_empty() { ff::avformat_close_input(&mut fmt); return Ok(None) }
            let mut channel_offset = vec![]; let mut total = 0; for &n in &stream_channels { channel_offset.push(total); total += n; }
            let all_mono = stream_channels.iter().all(|&n| n == 1);
            let layout_name = if all_mono { format!("{} mono tracks", decoders.len()) } else { format!("{total} channels") };
            let mut ctx = Ctx { fmt, decoders, global_to_audio, stream_channels, channel_offset, pkt: ff::av_packet_alloc(), frame: ff::av_frame_alloc(), out_frame: ff::av_frame_alloc(), pipeline: None };
            ctx.pipeline = build_pipeline(&ctx, routing);
            if ctx.pipeline.is_none() { return Err(Error::Open("audio: the filter graph could not be built".into())) }
            let sh = Arc::new(Shared { ring: AudioRing::new(RING_SAMPLES), running: AtomicBool::new(false), seek: Mutex::new(None), seek_cv: Condvar::new(), seek_requested: AtomicBool::new(false), eof: AtomicBool::new(false), decode_pos: AtomicU64::new(0), read_pos: AtomicU64::new(0), last_seek: AtomicU64::new(0), peaks: Default::default(), routing: AtomicI32::new(routing as i32), routing_dirty: AtomicBool::new(false) });
            let stage = TempoStage::new(); let tempo = stage.ctl.clone();
            let (stream_count, total_channels) = (ctx.decoders.len(), total);
            Ok(Some(MultiStreamAudioDecoder { sh, thread: Mutex::new(None), ctx: Mutex::new(Some((ctx, stage))), tempo, duration, stream_count, total_channels, layout_name, t0: Instant::now() }))
        }
    }
    pub fn start(&self) {
        let Some((ctx, stage)) = self.ctx.lock().unwrap().take() else { return };
        self.sh.running.store(true, Release);
        let sh = self.sh.clone(); let t0 = self.t0;
        *self.thread.lock().unwrap() = Some(std::thread::Builder::new().name("qcv-audio-multi".into()).spawn(move || decode_thread(ctx, stage, sh, t0)).expect("multi audio thread"));
    }
    pub fn stop(&self) { self.sh.running.store(false, Release); self.sh.seek_cv.notify_all(); if let Some(t) = self.thread.lock().unwrap().take() { let _ = t.join(); } }
}
impl Drop for MultiStreamAudioDecoder { fn drop(&mut self) { self.stop(); } }

impl AudioSource for MultiStreamAudioDecoder {
    fn read(&self, out: &mut [f32]) -> usize {
        let n = self.sh.ring.read(out); if n < out.len() { out[n..].fill(0.0); }
        let frames = n / OUT_CHANNELS; let p = b2f(self.sh.read_pos.load(Relaxed)) + frames as f64 / OUT_RATE as f64; self.sh.read_pos.store(f2b(p), Relaxed); frames
    }
    fn seek_pending(&self) -> bool { self.sh.seek_requested.load(Acquire) }
    fn flush_generation(&self) -> u32 { self.sh.ring.stale_generation() }
    fn stale_frames(&self) -> usize { self.sh.ring.stale_samples() / OUT_CHANNELS }
    fn discard_stale(&self) { self.sh.ring.discard_stale() }
}
impl AudioSourceCtl for MultiStreamAudioDecoder {
    fn start(&self) { MultiStreamAudioDecoder::start(self) }
    fn stop(&self) { MultiStreamAudioDecoder::stop(self) }
    fn seek(&self, sec: f64) { *self.sh.seek.lock().unwrap() = Some(sec); self.sh.seek_requested.store(true, Release); self.sh.seek_cv.notify_one(); }
    fn duration(&self) -> f64 { self.duration }
    fn seconds_since_last_seek(&self) -> f64 { let t = b2f(self.sh.last_seek.load(Relaxed)); if t <= 0.0 { f64::MAX } else { self.t0.elapsed().as_secs_f64() - t } }
    fn set_routing_mode(&self, mode: RoutingMode) { if self.sh.routing.swap(mode as i32, AcqRel) != mode as i32 { self.sh.routing_dirty.store(true, Release); } }
    fn routing_mode(&self) -> RoutingMode { match self.sh.routing.load(Relaxed) { 1 => RoutingMode::Downmix5_1, 2 => RoutingMode::Stereo7_8, _ => RoutingMode::Auto } }
    fn set_tempo(&self, tempo: f64) { self.tempo.set_tempo(tempo) }
    fn tempo(&self) -> f64 { self.tempo.tempo() }
    fn source_channels(&self) -> usize { self.total_channels }
    fn layout_name(&self) -> String { self.layout_name.clone() }
    fn peak_levels(&self) -> [f32; 16] { let mut o = [0.0; 16]; for (i, p) in self.sh.peaks.iter().enumerate() { o[i] = f32::from_bits(p.load(Relaxed)); } o }
}

// ---- the routing plan (QCView `resolveRouting`) ---------------------------------------------------------------------
struct Plan { inputs: Vec<usize>, pan: Option<String> }

fn resolve_routing(mode: RoutingMode, stream_channels: &[usize]) -> Plan {
    let total: usize = stream_channels.iter().sum();
    let locate = |gc: usize| -> Option<(usize, usize)> { let mut c = 0; for (i, &n) in stream_channels.iter().enumerate() { if gc >= c && gc < c + n { return Some((i, gc - c)) } c += n; } None };
    let amerge_channel_of = |inputs: &[usize], s: usize, local: usize| -> Option<usize> { let mut a = 0; for &i in inputs { if i == s { return Some(a + local) } a += stream_channels[i]; } None };
    let pluck_pair = |gl: usize, gr: usize| -> Option<Plan> {
        let (sl, ll) = locate(gl)?; let (sr, lr) = locate(gr)?;
        let inputs = if sl == sr { vec![sl] } else if sl < sr { vec![sl, sr] } else { vec![sr, sl] };
        let (al, ar) = (amerge_channel_of(&inputs, sl, ll)?, amerge_channel_of(&inputs, sr, lr)?);
        let merged: usize = inputs.iter().map(|&i| stream_channels[i]).sum();
        let pan = if merged == 2 && al == 0 && ar == 1 { None } else { Some(format!("stereo|c0=c{al}|c1=c{ar}")) };
        Some(Plan { inputs, pan })
    };
    let downmix = || -> Option<Plan> {
        if total < 6 { return None }
        let locs: Vec<(usize, usize)> = (0..6).map(locate).collect::<Option<_>>()?;
        let inputs: Vec<usize> = locs.iter().map(|l| l.0).collect::<BTreeSet<_>>().into_iter().collect();
        let a: Vec<usize> = locs.iter().map(|&(s, l)| amerge_channel_of(&inputs, s, l)).collect::<Option<_>>()?;
        Some(Plan { inputs, pan: Some(format!("stereo|c0=c{}+0.707*c{}+0.707*c{}|c1=c{}+0.707*c{}+0.707*c{}", a[0], a[2], a[4], a[1], a[2], a[5])) })
    };
    let fallback = || -> Plan { if total >= 2 { pluck_pair(0, 1).unwrap_or(Plan { inputs: vec![0], pan: None }) } else { Plan { inputs: if stream_channels.is_empty() { vec![] } else { vec![0] }, pan: None } } };
    match mode {
        RoutingMode::Downmix5_1 => downmix().unwrap_or_else(fallback),
        RoutingMode::Stereo7_8 => if total >= 8 { pluck_pair(6, 7).unwrap_or_else(fallback) } else { fallback() },
        RoutingMode::Auto => { if total >= 8 { if let Some(p) = pluck_pair(6, 7) { return p } } if total >= 6 { if let Some(p) = downmix() { return p } } fallback() }
    }
}

fn build_pipeline(ctx: &Ctx, mode: RoutingMode) -> Option<Pipeline> {
    unsafe {
        let plan = resolve_routing(mode, &ctx.stream_channels);
        if plan.inputs.is_empty() { return None }
        let graph = ff::avfilter_graph_alloc(); if graph.is_null() { return None }
        let mut p = Pipeline { graph, buffersrcs: vec![ptr::null_mut(); ctx.decoders.len()], sink: ptr::null_mut() };
        let by_name = |n: &str| { let c = CString::new(n).unwrap(); ff::avfilter_get_by_name(c.as_ptr()) };
        let (abuffer, abufsink, amerge, pan, aformat) = (by_name("abuffer"), by_name("abuffersink"), by_name("amerge"), by_name("pan"), by_name("aformat"));
        if abuffer.is_null() || abufsink.is_null() || amerge.is_null() || pan.is_null() || aformat.is_null() { log::warn!("multi audio: a required filter is missing"); return None }
        for &si in &plan.inputs {
            let cctx = ctx.decoders[si];
            let mut buf = [0i8; 64];
            let layout = if ff::av_channel_layout_describe(&(*cctx).ch_layout, buf.as_mut_ptr(), buf.len()) > 0 { CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned() } else if (*cctx).ch_layout.nb_channels == 1 { "mono".into() } else { format!("{}c", (*cctx).ch_layout.nb_channels) };
            let fmt_name = CStr::from_ptr(ff::av_get_sample_fmt_name((*cctx).sample_fmt)).to_string_lossy().into_owned();
            let args = CString::new(format!("time_base=1/{}:sample_rate={}:sample_fmt={}:channel_layout={}", (*cctx).sample_rate, (*cctx).sample_rate, fmt_name, layout)).unwrap();
            let name = CString::new(format!("src_{si}")).unwrap();
            let mut src: *mut ff::AVFilterContext = ptr::null_mut();
            if ff::avfilter_graph_create_filter(&mut src, abuffer, name.as_ptr(), args.as_ptr(), ptr::null_mut(), graph) < 0 { log::warn!("multi audio: abuffer failed for stream {si}"); return None }
            p.buffersrcs[si] = src;
        }
        let out_name = CString::new("out").unwrap();
        let sink = ff::avfilter_graph_alloc_filter(graph, abufsink, out_name.as_ptr()); if sink.is_null() { return None }
        let set = |k: &str, v: &str| { let k = CString::new(k).unwrap(); let v = CString::new(v).unwrap(); ff::av_opt_set(sink as *mut _, k.as_ptr(), v.as_ptr(), AV_OPT_SEARCH_CHILDREN) };
        set("sample_formats", "flt"); set("samplerates", &OUT_RATE.to_string()); set("channel_layouts", "stereo");
        if ff::avfilter_init_dict(sink, ptr::null_mut()) < 0 { log::warn!("multi audio: abuffersink init failed"); return None }
        p.sink = sink;
        let mut prev = if plan.inputs.len() == 1 { p.buffersrcs[plan.inputs[0]] } else {
            let mut m: *mut ff::AVFilterContext = ptr::null_mut();
            let args = CString::new(format!("inputs={}", plan.inputs.len())).unwrap(); let nm = CString::new("amerge").unwrap();
            if ff::avfilter_graph_create_filter(&mut m, amerge, nm.as_ptr(), args.as_ptr(), ptr::null_mut(), graph) < 0 { log::warn!("multi audio: amerge failed"); return None }
            for (i, &si) in plan.inputs.iter().enumerate() { if ff::avfilter_link(p.buffersrcs[si], 0, m, i as u32) < 0 { return None } }
            m
        };
        if let Some(expr) = &plan.pan {
            let mut pc: *mut ff::AVFilterContext = ptr::null_mut();
            let args = CString::new(expr.as_str()).unwrap(); let nm = CString::new("pan").unwrap();
            if ff::avfilter_graph_create_filter(&mut pc, pan, nm.as_ptr(), args.as_ptr(), ptr::null_mut(), graph) < 0 { log::warn!("multi audio: pan failed ({expr})"); return None }
            if ff::avfilter_link(prev, 0, pc, 0) < 0 { return None }
            prev = pc;
        }
        let mut af: *mut ff::AVFilterContext = ptr::null_mut();
        let args = CString::new(format!("sample_fmts=flt:sample_rates={OUT_RATE}:channel_layouts=stereo")).unwrap(); let nm = CString::new("aformat").unwrap();
        if ff::avfilter_graph_create_filter(&mut af, aformat, nm.as_ptr(), args.as_ptr(), ptr::null_mut(), graph) < 0 { return None }
        if ff::avfilter_link(prev, 0, af, 0) < 0 || ff::avfilter_link(af, 0, sink, 0) < 0 { return None }
        if ff::avfilter_graph_config(graph, ptr::null_mut()) < 0 { log::warn!("multi audio: graph config failed (mode {mode:?})"); return None }
        log::info!("multi audio: pipeline built — mode {mode:?}, merge {} streams, pan {}", plan.inputs.len(), plan.pan.as_deref().unwrap_or("(none)"));
        Some(p)
    }
}

fn peaks_of(frame: *const ff::AVFrame, base: usize, peaks: &mut [f32; 16]) {
    unsafe {
        let n = (*frame).nb_samples as usize; let nch = (*frame).ch_layout.nb_channels.max(0) as usize; if n == 0 || nch == 0 { return }
        let fmt = std::mem::transmute::<i32, ff::AVSampleFormat>((*frame).format);
        use ff::AVSampleFormat::*;
        fn peak<T: Copy + Into<f64>>(p: *const T, stride: usize, n: usize, inv: f32) -> f32 { let mut m = 0.0f64; for i in 0..n { let v: f64 = unsafe { *p.add(i * stride) }.into(); let a = v.abs(); if a > m { m = a; } } m as f32 * inv }
        for ch in 0..nch {
            let g = base + ch; if g >= 16 { break }
            peaks[g] = match fmt {
                AV_SAMPLE_FMT_FLT => peak(((*frame).data[0] as *const f32).add(ch), nch, n, 1.0), AV_SAMPLE_FMT_FLTP => peak((*frame).extended_data.add(ch).read() as *const f32, 1, n, 1.0),
                AV_SAMPLE_FMT_DBL => peak(((*frame).data[0] as *const f64).add(ch), nch, n, 1.0), AV_SAMPLE_FMT_DBLP => peak((*frame).extended_data.add(ch).read() as *const f64, 1, n, 1.0),
                AV_SAMPLE_FMT_S16 => peak(((*frame).data[0] as *const i16).add(ch), nch, n, 1.0 / 32768.0), AV_SAMPLE_FMT_S16P => peak((*frame).extended_data.add(ch).read() as *const i16, 1, n, 1.0 / 32768.0),
                AV_SAMPLE_FMT_S32 => peak(((*frame).data[0] as *const i32).add(ch), nch, n, 1.0 / 2147483647.0), AV_SAMPLE_FMT_S32P => peak((*frame).extended_data.add(ch).read() as *const i32, 1, n, 1.0 / 2147483647.0),
                _ => continue,
            };
        }
    }
}

fn write_all(sh: &Shared, floats: &[f32]) { let mut total = 0; while total < floats.len() && sh.running.load(Relaxed) && !sh.seek_requested.load(Relaxed) { total += sh.ring.write(&floats[total..]); if total < floats.len() { std::thread::sleep(Duration::from_millis(2)); } } }

fn decode_thread(mut ctx: Ctx, mut tempo: TempoStage, sh: Arc<Shared>, t0: Instant) {
    let mut trim_pending = false; let mut trim_target = 0.0f64; let mut peaks = [0.0f32; 16]; let mut tempo_buf = vec![0.0f32; 4096 * 2];
    unsafe {
        while sh.running.load(Acquire) {
            if sh.routing_dirty.swap(false, AcqRel) { let mode = match sh.routing.load(Relaxed) { 1 => RoutingMode::Downmix5_1, 2 => RoutingMode::Stereo7_8, _ => RoutingMode::Auto }; if let Some(p) = build_pipeline(&ctx, mode) { ctx.pipeline = Some(p); } }
            {
                let mut g = sh.seek.lock().unwrap();
                if let Some(target) = g.take() {
                    // flushAndSeek: seek the whole file, flush every codec, mark the ring stale, rebuild the graph, trim to the target
                    ff::avformat_seek_file(ctx.fmt, -1, i64::MIN, (target * ff::AV_TIME_BASE as f64) as i64, i64::MAX, ff::AVSEEK_FLAG_BACKWARD);
                    for &d in &ctx.decoders { ff::avcodec_flush_buffers(d); }
                    sh.ring.mark_stale(); tempo.flush();
                    let mode = match sh.routing.load(Relaxed) { 1 => RoutingMode::Downmix5_1, 2 => RoutingMode::Stereo7_8, _ => RoutingMode::Auto };
                    if let Some(p) = build_pipeline(&ctx, mode) { ctx.pipeline = Some(p); }
                    sh.decode_pos.store(f2b(target), Relaxed); sh.read_pos.store(f2b(target), Relaxed);
                    trim_pending = true; trim_target = target.max(0.0); sh.eof.store(false, Relaxed);
                    sh.last_seek.store(f2b(t0.elapsed().as_secs_f64()), Relaxed);
                    sh.seek_requested.store(false, Release);
                }
            }
            if sh.ring.fresh_samples() > BACK_PRESSURE_SAMPLES { std::thread::sleep(Duration::from_millis(2)); continue }
            if sh.eof.load(Relaxed) { let g = sh.seek.lock().unwrap(); let _ = sh.seek_cv.wait_timeout_while(g, Duration::from_millis(50), |_| sh.running.load(Relaxed) && !sh.seek_requested.load(Relaxed)).unwrap(); continue }
            let rc = ff::av_read_frame(ctx.fmt, ctx.pkt);
            if rc < 0 { if rc == ff::AVERROR_EOF { sh.eof.store(true, Relaxed); } else { std::thread::sleep(Duration::from_millis(5)); } continue }
            let gi = (*ctx.pkt).stream_index as usize;
            let ai = ctx.global_to_audio.get(gi).copied().unwrap_or(-1);
            if ai < 0 { ff::av_packet_unref(ctx.pkt); continue }
            let ai = ai as usize; let cctx = ctx.decoders[ai];
            if ff::avcodec_send_packet(cctx, ctx.pkt) < 0 { ff::av_packet_unref(ctx.pkt); continue }
            ff::av_packet_unref(ctx.pkt);
            while ff::avcodec_receive_frame(cctx, ctx.frame) == 0 {
                peaks_of(ctx.frame, ctx.channel_offset[ai], &mut peaks);
                for (i, p) in peaks.iter().enumerate() { sh.peaks[i].store(p.to_bits(), Relaxed); }
                let Some(pl) = ctx.pipeline.as_ref() else { ff::av_frame_unref(ctx.frame); continue };
                let src = pl.buffersrcs[ai];
                if src.is_null() || ff::av_buffersrc_add_frame_flags(src, ctx.frame, AV_BUFFERSRC_FLAG_KEEP_REF) < 0 { ff::av_frame_unref(ctx.frame); continue }
                ff::av_frame_unref(ctx.frame);
                while ff::av_buffersink_get_frame(pl.sink, ctx.out_frame) >= 0 {
                    let mut skip = 0usize;
                    if trim_pending {
                        if (*ctx.out_frame).pts == ff::AV_NOPTS_VALUE { trim_pending = false; }
                        else {
                            let tb = ff::av_buffersink_get_time_base(pl.sink);
                            let start = (*ctx.out_frame).pts as f64 * ff::av_q2d(tb);
                            let dur = (*ctx.out_frame).nb_samples as f64 / OUT_RATE as f64;
                            if start + dur <= trim_target { ff::av_frame_unref(ctx.out_frame); continue }
                            if start < trim_target { skip = (((trim_target - start) * OUT_RATE as f64).round() as usize).min((*ctx.out_frame).nb_samples as usize); }
                            trim_pending = false;
                        }
                    }
                    let n = (*ctx.out_frame).nb_samples as usize;
                    if n > skip {
                        let data = if (*ctx.out_frame).extended_data.is_null() { (*ctx.out_frame).data[0] } else { *(*ctx.out_frame).extended_data };
                        let floats = std::slice::from_raw_parts((data as *const f32).add(skip * OUT_CHANNELS), (n - skip) * OUT_CHANNELS);
                        if !tempo.bypassed() { tempo.put(floats, n - skip); loop { if !sh.running.load(Relaxed) || sh.seek_requested.load(Relaxed) { break } let got = tempo.receive(&mut tempo_buf, 4096); if got == 0 { break } write_all(&sh, &tempo_buf[..got * 2]); } }
                        else { write_all(&sh, floats); }
                        let p = b2f(sh.decode_pos.load(Relaxed)) + (n - skip) as f64 / OUT_RATE as f64; sh.decode_pos.store(f2b(p), Relaxed);
                    }
                    ff::av_frame_unref(ctx.out_frame);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn routing_plans() {
        // two mono tracks: merge both, passthrough
        let p = resolve_routing(RoutingMode::Auto, &[1, 1]); assert_eq!(p.inputs, vec![0, 1]); assert!(p.pan.is_none());
        // two stereo streams: the first pair lives in stream 0 alone
        let p = resolve_routing(RoutingMode::Auto, &[2, 2]); assert_eq!(p.inputs, vec![0]); assert!(p.pan.is_none());
        // 7–8 across two 4-channel streams: stream 1's channels 2,3 → pan from amerge channel 2,3 of the single merged input
        let p = resolve_routing(RoutingMode::Auto, &[4, 4]); assert_eq!(p.inputs, vec![1]); assert_eq!(p.pan.as_deref(), Some("stereo|c0=c2|c1=c3"));
        // six mono tracks: the BS.775 fold over the six merged channels
        let p = resolve_routing(RoutingMode::Auto, &[1, 1, 1, 1, 1, 1]); assert_eq!(p.inputs.len(), 6); assert_eq!(p.pan.as_deref(), Some("stereo|c0=c0+0.707*c2+0.707*c4|c1=c1+0.707*c2+0.707*c5"));
        // Stereo7_8 without 8 channels falls back to the first pair
        let p = resolve_routing(RoutingMode::Stereo7_8, &[2, 2]); assert_eq!(p.inputs, vec![0]);
    }
}
