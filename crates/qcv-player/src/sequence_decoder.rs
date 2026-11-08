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
        let (name, short) = match fmt { min_image::Format::Exr => ("OpenEXR", "exr"), min_image::Format::Png => ("PNG", "png"), min_image::Format::Tiff => ("TIFF", "tiff"), min_image::Format::Jpeg => ("JPEG", "mjpeg") };
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
        let img = self.cache.frame(f)?;
        *want = None;
        self.landed(f, img)
    }
    /// The playhead is at `f` (`i32::MAX` = wherever a gesture left it): the frame, once, when cached.
    pub fn fetch_due(&self, f: i32) -> (Option<FrameHandle>, u32) {
        if self.want.lock().unwrap().is_some() { return (self.fetch_front(), 0) }
        let f = if f == i32::MAX { self.cache.playhead() } else { if f != self.cache.playhead() { self.cache.update_playhead(f, false) } f };
        if self.last.load(Acquire) == f { return (None, 0) }
        let Some(img) = self.cache.frame(f) else { return (None, 0) };
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
