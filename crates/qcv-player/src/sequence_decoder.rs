//! The image-sequence side of a transport slot (step 2c piece 3): QCView's sequence mode — `startImageSequence`: the
//! [`ImageSequenceCache`] fills around a playhead that follows the WALL clock, and the renderer pulls the playhead's frame
//! or HOLDS the last good one (a miss, a gap, a broken frame: the picture stays, the band shows the hole; QCView's upload
//! thread never receives sentinels) — behind the same calls the transport makes of a [`VideoDecoder`], so playback, seeks,
//! scrubs, steps, loops, in / out and playlists run unchanged.
//!
//! How the decoder protocol maps: `seek_to_frame` = a user seek (`updatePlayhead(f, force)`), counted at once; the landing
//! is the target frame entering the cache (a sentinel target lands too, holding the picture, so a seek onto a hole never
//! hangs). `scrub_to` = a drag move (`updatePlayhead(f)`, no force — QCView: forcing it per move starved the IO pool).
//! `fetch_due(f)` = the playhead moves to `f`; the frame comes back once, when cached.
//!
//! Formats `min-image` does not read (DPX, TGA, BMP) stay on FFmpeg's image2 demuxer through the [`VideoDecoder`]
//! (row 148: "FFmpeg's decoder stays the fallback").
use crate::{ffmpeg::SessionInfo, frame_handle::FrameHandle, frame_index::FrameIndex, image_sequence_cache::{ImageSequenceCache, StreamingConfig}, rgb_range::RangeOverride, video_decoder::{State, VideoDecoder}, Error};
use min_image::{Image, Sample};
use min_present::{Chroma, ColorMeta, Frame, Matrix, Pixels, Plane, PlanarLayout, Primaries, Range, Transfer};
use qcv_timeline::Rate;
use std::sync::{atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering::*}, Arc, Mutex};

pub struct SequenceDecoder {
    cache: ImageSequenceCache, pub info: SessionInfo, index: Arc<FrameIndex>, fps: f64,
    playing: AtomicBool, seeks: AtomicU32, published: AtomicU64, current: AtomicI32,
    /// a user seek's target, still to land
    want: Mutex<Option<i32>>,
    /// the frame `fetch_due` handed out last (each frame comes back once)
    last: AtomicI32,
    shown: Mutex<Option<(i32, Arc<Image>)>>,
}

/// One plane of a cached frame, read in place by the presenter (no copy).
struct PlaneOf(Arc<Image>, usize);
impl AsRef<[u8]> for PlaneOf { fn as_ref(&self) -> &[u8] { &self.0.planes[self.1] } }

/// Whether the cache reads this pattern (else the FFmpeg image2 fallback).
pub fn cache_reads(pattern: &str) -> bool { min_image::Format::of(pattern).is_some() }

impl SequenceDecoder {
    /// `pattern` = the full printf path (`dir/name_%04d.exr`); `start..=end` the file numbers.
    pub fn open(pattern: &str, rate: Rate, start: i64, end: i64, config: StreamingConfig, layer: &str) -> Result<SequenceDecoder, Error> {
        let p = std::path::Path::new(pattern);
        let dir = p.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
        let file = p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        let cache = ImageSequenceCache::open(&dir, &file, start as i32, end as i32, rate.as_f64(), layer, config).map_err(|e| Error::Open(e.to_string()))?;
        let fmt = min_image::Format::of(&file).unwrap();
        let (name, short) = match fmt { min_image::Format::Exr => ("OpenEXR", "exr"), min_image::Format::Png => ("PNG", "png"), min_image::Format::Tiff => ("TIFF", "tiff"), min_image::Format::Jpeg => ("JPEG", "mjpeg"), min_image::Format::Dpx => ("DPX", "dpx"), min_image::Format::Tga => ("TGA", "targa"), min_image::Format::Bmp => ("BMP", "bmp") };
        let frames = cache.frame_count();
        let float = fmt == min_image::Format::Exr;
        let meta = rgb_meta(if float { Sample::F16 } else { Sample::U8 });
        let info = SessionInfo { width: cache.width(), height: cache.height(), frame_count: frames, codec_name: name.into(), codec_short: short.into(), pix_fmt: if float { "gbrpf16le".into() } else { "gbrp".into() },
            hw_accel: String::new(), source_timecode: None, drop_frame: false, time_base: (rate.den as i32, rate.num as i32), frame_rate: (rate.num as i32, rate.den as i32), intra_only: true, meta,
            duration_secs: frames as f64 / rate.as_f64() };
        let index = Arc::new(FrameIndex::new(rate.den as i32, rate.num as i32, rate.num as i32, rate.den as i32, frames, true));
        Ok(SequenceDecoder { cache, info, index, fps: rate.as_f64(), playing: AtomicBool::new(false), seeks: AtomicU32::new(0), published: AtomicU64::new(0), current: AtomicI32::new(-1),
            want: Mutex::new(Some(0)), last: AtomicI32::new(-1), shown: Mutex::new(None) })
    }
    pub fn cache(&self) -> &ImageSequenceCache { &self.cache }
    /// The inspector's frame-rate chips (QCView `TimelineController::setFrameRate`): the frame count stays, the cache keeps its frames.
    pub fn set_rate(&mut self, rate: Rate) {
        let frames = self.info.frame_count;
        self.fps = rate.as_f64();
        self.info.time_base = (rate.den as i32, rate.num as i32); self.info.frame_rate = (rate.num as i32, rate.den as i32); self.info.duration_secs = frames as f64 / self.fps;
        self.index = Arc::new(FrameIndex::new(rate.den as i32, rate.num as i32, rate.num as i32, rate.den as i32, frames, true));
    }
    /// At a cache stride > 1 an off-grid frame is never cached: QCView's renderer shows the closest cached frame
    /// (`getClosestFrame`) — here only a grid neighbour (within the stride), so a stale window never stands in.
    fn frame_or_grid_neighbour(&self, f: i32) -> Option<Arc<Image>> {
        if let Some(img) = self.cache.frame(f) { return Some(img) }
        let stride = self.cache.stride();
        if stride <= 1 { return None }
        self.cache.closest_frame(f).filter(|(k, _)| (k - f).abs() < stride).map(|(_, img)| img)
    }
    pub fn frame_index(&self) -> &Arc<FrameIndex> { &self.index }

    pub fn play(&self) { self.playing.store(true, Release) }
    pub fn pause(&self) { self.playing.store(false, Release) }
    pub fn is_playing(&self) -> bool { self.playing.load(Acquire) }
    pub fn state(&self) -> State { State::Decoding }
    pub fn seeks_performed(&self) -> u32 { self.seeks.load(Acquire) }
    pub fn published_seq(&self) -> u64 { self.published.load(Acquire) }
    pub fn current_frame(&self) -> i32 { self.current.load(Acquire) }

    /// A user seek: the cache bumps its generation and puts `f` at the front of the queue.
    pub fn seek_to_frame(&self, f: i32) {
        self.cache.update_playhead(f, true);
        *self.want.lock().unwrap() = Some(f.clamp(0, self.cache.frame_count() - 1));
        self.last.store(-1, Release);
        self.seeks.fetch_add(1, AcqRel);
    }
    /// A drag move: the playhead follows without a forced seek.
    pub fn scrub_to(&self, f: i32) { self.cache.update_playhead(f, false); self.last.store(-1, Release); }

    /// The seek target once it is cached (a sentinel target lands without a picture: the old one holds).
    pub fn fetch_front(&self) -> Option<FrameHandle> {
        let mut want = self.want.lock().unwrap();
        let f = (*want)?;
        let img = self.frame_or_grid_neighbour(f)?;
        *want = None;
        self.landed(f, img)
    }
    /// The playhead is at `f` (`i32::MAX` = wherever a gesture left it): the frame, once, when cached.
    pub fn fetch_due(&self, f: i32) -> (Option<FrameHandle>, u32) {
        if self.want.lock().unwrap().is_some() { return (self.fetch_front(), 0) }
        let f = if f == i32::MAX { self.cache.playhead() } else { if f != self.cache.playhead() { self.cache.update_playhead(f, false) } f };
        if self.last.load(Acquire) == f { return (None, 0) }
        let Some(img) = self.frame_or_grid_neighbour(f) else { return (None, 0) };
        self.last.store(f, Release);
        (self.landed(f, img), 0)
    }
    fn landed(&self, f: i32, img: Arc<Image>) -> Option<FrameHandle> {
        self.current.store(f, Release);
        self.published.fetch_add(1, AcqRel);
        if img.sentinel.is_some() { return None }
        *self.shown.lock().unwrap() = Some((f, img.clone()));
        Some(handle(img, f, self.fps))
    }
    /// The frame shown last (diagnostics: `fetch_latest`).
    pub fn fetch_latest(&self) -> Option<FrameHandle> { self.shown.lock().unwrap().clone().map(|(f, i)| handle(i, f, self.fps)) }
}

/// QCView's still pipeline is RGB at full range; 8 / 16-bit stills are display-referred sRGB, half floats linear light.
fn rgb_meta(s: Sample) -> ColorMeta {
    ColorMeta { matrix: Matrix::Identity, range: Range::Full, transfer: if s == Sample::F16 { Transfer::Linear } else { Transfer::Srgb }, primaries: Primaries::Bt709,
        bit_depth: match s { Sample::U8 => 8, _ => 16 }, chroma: Chroma::Rgb, range_forced: false, rotation: 0, sar: (1, 1), mastering: None }
}

/// A cached frame as the presenter's planar RGB frame, its planes read in place.
pub fn handle(img: Arc<Image>, f: i32, fps: f64) -> FrameHandle {
    let (bps, code_bits, float) = match img.sample { Sample::U8 => (1, 8, false), Sample::U16 => (2, 16, false), Sample::F16 => (2, 16, true) };
    let planes = (0..img.planes.len()).map(|i| Plane { data: Arc::new(PlaneOf(img.clone(), i)), width: img.width, height: img.height, pitch: img.pitch }).collect();
    let frame = Frame { width: img.width, height: img.height, pixels: Pixels::Planar { planes, layout: PlanarLayout::Rgb { bytes_per_sample: bps, code_bits, float, alpha: img.alpha, legal_expand: false } }, meta: rgb_meta(img.sample) };
    FrameHandle { frame, pts: f as i64, pts_us: if fps > 0.0 { (f as f64 / fps * 1e6) as i64 } else { 0 }, frame_no: f, keyframe: true, decode_ms: 0.0 }
}

/// A transport slot's decoder: a video stream, or an image sequence through the cache.
pub enum SlotDecoder { Video(VideoDecoder), Sequence(SequenceDecoder) }
macro_rules! both { ($s:expr, $d:ident => $e:expr) => { match $s { SlotDecoder::Video($d) => $e, SlotDecoder::Sequence($d) => $e } } }
impl SlotDecoder {
    pub fn play(&self) { both!(self, d => d.play()) }
    pub fn pause(&self) { both!(self, d => d.pause()) }
    pub fn is_playing(&self) -> bool { both!(self, d => d.is_playing()) }
    pub fn state(&self) -> State { both!(self, d => d.state()) }
    pub fn seek_to_frame(&self, f: i32) { both!(self, d => d.seek_to_frame(f)) }
    pub fn seeks_performed(&self) -> u32 { both!(self, d => d.seeks_performed()) }
    pub fn published_seq(&self) -> u64 { both!(self, d => d.published_seq()) }
    pub fn current_frame(&self) -> i32 { both!(self, d => d.current_frame()) }
    pub fn fetch_front(&self) -> Option<FrameHandle> { both!(self, d => d.fetch_front()) }
    pub fn fetch_due(&self, f: i32) -> (Option<FrameHandle>, u32) { both!(self, d => d.fetch_due(f)) }
    pub fn fetch_latest(&self) -> Option<FrameHandle> { both!(self, d => d.fetch_latest()) }
    /// A scrub move with no scrub decoder: the video seeks, the sequence's playhead follows.
    pub fn scrub_to(&self, f: i32) { match self { SlotDecoder::Video(d) => d.seek_to_frame(f), SlotDecoder::Sequence(d) => d.scrub_to(f) } }
    pub fn frame_index(&self) -> Option<&Arc<FrameIndex>> { match self { SlotDecoder::Video(d) => d.frame_index(), SlotDecoder::Sequence(d) => Some(d.frame_index()) } }
    pub fn front_is_external(&self) -> bool { match self { SlotDecoder::Video(d) => d.front_is_external(), SlotDecoder::Sequence(_) => false } }
    pub fn queued(&self) -> usize { match self { SlotDecoder::Video(d) => d.queued(), SlotDecoder::Sequence(_) => 0 } }
    pub fn set_playback_speed(&self, s: f64) { if let SlotDecoder::Video(d) = self { d.set_playback_speed(s) } }
    pub fn set_range_override(&self, r: RangeOverride) { if let SlotDecoder::Video(d) = self { d.set_range_override(r) } }
    pub fn set_read_ahead_range(&self, first: i32, last: i32) { if let SlotDecoder::Video(d) = self { d.set_read_ahead_range(first, last) } }
    pub fn decode_error_count(&self) -> u32 { match self { SlotDecoder::Video(d) => d.decode_error_count(), SlotDecoder::Sequence(d) => d.cache.failed_count() as u32 } }
    pub fn last_decode_error(&self) -> String { match self { SlotDecoder::Video(d) => d.last_decode_error(), SlotDecoder::Sequence(d) => d.cache.failed_frames().first().map(|f| format!("frame {f} failed")).unwrap_or_default() } }
    pub fn as_video(&self) -> Option<&VideoDecoder> { match self { SlotDecoder::Video(d) => Some(d), _ => None } }
    pub fn as_sequence(&self) -> Option<&SequenceDecoder> { match self { SlotDecoder::Sequence(d) => Some(d), _ => None } }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};
    /// A synthetic PNG sequence (FFmpeg; fixtures never travel): 1001–1024.
    fn sequence(tag: &str) -> Option<std::path::PathBuf> {
        let d = std::env::temp_dir().join(format!("qcv-seqdec-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d); std::fs::create_dir_all(&d).unwrap();
        let ok = std::process::Command::new("ffmpeg").args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc2=size=64x36:rate=24", "-frames:v", "24", "-start_number", "1001"])
            .arg(d.join("f_%04d.png")).status().map(|s| s.success()).unwrap_or(false);
        ok.then_some(d)
    }
    fn wait(mut f: impl FnMut() -> bool) -> bool { let t = Instant::now(); while t.elapsed() < Duration::from_secs(5) { if f() { return true } std::thread::sleep(Duration::from_millis(5)) } false }
    #[test]
    fn stride_shows_the_grid_neighbour_of_an_off_grid_frame() {
        let Some(d) = sequence("stride") else { eprintln!("ffmpeg missing: skipped"); return };
        let dec = SequenceDecoder::open(&d.join("f_%04d.png").to_string_lossy(), Rate::new(24, 1), 1001, 1024, StreamingConfig::from_settings(4, 24), "").unwrap();
        dec.cache().set_stride(2);
        // the open filled at stride 1; the new grid (0, 2, 4, …) evicts the off-grid frames
        assert!(wait(|| dec.cache().frame(5).is_none() && dec.cache().frame(4).is_some()), "stride 2 drops frame 5 and keeps frame 4");
        dec.seek_to_frame(5);   // off the grid: never cached itself
        assert!(wait(|| dec.fetch_front().is_some()), "an off-grid seek target must land on a grid neighbour");
        let _ = std::fs::remove_dir_all(d);
    }
    #[test]
    fn a_loop_wrap_keeps_the_failed_ticks() {
        let Some(d) = sequence("wrap") else { eprintln!("ffmpeg missing: skipped"); return };
        std::fs::remove_file(d.join("f_1006.png")).unwrap();   // frame 5: a gap
        let src = crate::Source::Sequence { pattern: d.join("f_%04d.png").to_string_lossy().into_owned(), rate: Rate::new(60, 1), start: 1001, end: 1024, label: String::new() };   // 60 fps: one frame per tick at most (a faster clock skipped frames under load and read the wrap as a seek — the slack rule, correctly)
        let mut t = crate::Transport::open(&src, crate::TransportOptions { loop_enabled: true, scrub: None, seq: StreamingConfig::from_settings(4, 24), ..Default::default() }).unwrap();
        let failed = |t: &crate::Transport| t.decoder().as_sequence().unwrap().cache().failed_frames();
        assert!(wait(|| failed(&t).contains(&5)), "the gap flags");
        t.play();
        let gen0 = t.decoder().as_sequence().unwrap().cache().generation();
        let t0 = Instant::now();
        while t.stats.loops < 2 && t0.elapsed() < Duration::from_secs(8) { let _ = t.tick(Instant::now()); std::thread::sleep(Duration::from_millis(1)); }
        assert!(t.stats.loops >= 2, "played through two wraps");
        assert_eq!(t.decoder().as_sequence().unwrap().cache().generation(), gen0, "a wrap is not a seek (QCView's frame advance): no generation bump, no amnesty");
        assert!(failed(&t).contains(&5), "the tick survives the wraps");
        let _ = std::fs::remove_dir_all(d);
    }
    #[test]
    fn a_layer_switch_reloads_the_frame_at_the_new_layer() {
        let d = std::env::temp_dir().join(format!("qcv-seqdec-layers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d); std::fs::create_dir_all(&d).unwrap();
        let ff = |src: &str, stem: &str| std::process::Command::new("ffmpeg").args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", src, "-frames:v", "4", "-pix_fmt", "gbrpf32le", "-c:v", "exr", "-start_number", "1"]).arg(d.join(format!("{stem}_%04d.exr"))).status().map(|s| s.success()).unwrap_or(false);
        if !(ff("testsrc2=size=64x36:rate=24", "a") && ff("smptebars=size=64x36:rate=24", "b")) { eprintln!("ffmpeg missing: skipped"); return }
        for n in 1..=4 {
            let ok = std::process::Command::new("exrmultipart").arg("-combine").arg("-i").arg(format!("{}::beauty", d.join(format!("a_{n:04}.exr")).display())).arg(format!("{}::diffuse", d.join(format!("b_{n:04}.exr")).display())).arg("-o").arg(d.join(format!("r_{n:04}.exr"))).stdout(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false);
            if !ok { eprintln!("exrmultipart missing: skipped"); return }
        }
        let src = crate::Source::Sequence { pattern: d.join("r_%04d.exr").to_string_lossy().into_owned(), rate: Rate::new(24, 1), start: 1, end: 4, label: String::new() };
        let mut t = crate::Transport::open(&src, crate::TransportOptions { scrub: None, seq: StreamingConfig::from_settings(4, 24), seq_layer: "beauty".into(), ..Default::default() }).unwrap();
        let cache = |t: &crate::Transport| t.decoder().as_sequence().unwrap().cache().frame(0).filter(|i| i.sentinel.is_none()).map(|i| i.planes[0].clone());
        assert!(wait(|| cache(&t).is_some())); let beauty = cache(&t).unwrap();
        assert!(t.set_sequence_layer("diffuse"));
        assert_eq!(t.timeline.tracks[0].clips[0].sequence_exr_layer, "diffuse", "the clip carries the layer (the hover keys)");
        assert!(wait(|| cache(&t).is_some_and(|p| p != beauty)), "frame 0 reloads at the diffuse layer");
        let _ = std::fs::remove_dir_all(d);
    }
    #[test]
    fn set_rate_keeps_the_frames_and_retimes_them() {
        let Some(d) = sequence("rate") else { eprintln!("ffmpeg missing: skipped"); return };
        let mut dec = SequenceDecoder::open(&d.join("f_%04d.png").to_string_lossy(), Rate::new(24, 1), 1001, 1024, StreamingConfig::from_settings(4, 24), "").unwrap();
        assert!(wait(|| dec.cache().frame(10).is_some()));
        dec.set_rate(Rate::new(25, 1));
        assert_eq!(dec.info.frame_count, 24); assert_eq!(dec.info.frame_rate, (25, 1)); assert!((dec.info.duration_secs - 24.0 / 25.0).abs() < 1e-9);
        assert!(dec.cache().frame(10).is_some(), "the cache keeps its frames across a rate change");
        let _ = std::fs::remove_dir_all(d);
    }
}
