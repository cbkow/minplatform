//! QCView's `media_item.h` — the in-memory model, field for field. `VideoMetadata`, `AdobeMetadata` and `ImageSequenceData`
//! are `qcv-meta`'s (the same structs QCView fills); the rest lives here.
pub use qcv_meta::{AdobeMetadata, ImageSequenceData, VideoMetadata};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MediaType { #[default] Video = 0, Audio = 1, Image = 2, ImageSequence = 3, Playlist = 4, DualPair = 5, LiveStream = 6 }
impl MediaType {
    pub fn as_str(self) -> &'static str { match self { MediaType::Video => "Video", MediaType::Audio => "Audio", MediaType::Image => "Image", MediaType::ImageSequence => "ImageSequence", MediaType::Playlist => "Playlist", MediaType::DualPair => "DualPair", MediaType::LiveStream => "LiveStream" } }
    /// QCView `mediaTypeFromString`: an unknown name defaults to Video (with a warning).
    pub fn parse(s: &str) -> MediaType { match s { "Video" => MediaType::Video, "Audio" => MediaType::Audio, "Image" => MediaType::Image, "ImageSequence" => MediaType::ImageSequence, "Playlist" => MediaType::Playlist, "DualPair" => MediaType::DualPair, "LiveStream" => MediaType::LiveStream, other => { log::warn!("ProjectIo: unknown MediaType '{other}' — defaulting to Video"); MediaType::Video } } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TransferOverride { #[default] Auto = 0, SdrRec709 = 1, Pq2020 = 2, PqP3D65 = 3, Hlg = 4, Linear = 5 }
impl TransferOverride { pub fn from_i64(t: i64) -> TransferOverride { match t { 1 => Self::SdrRec709, 2 => Self::Pq2020, 3 => Self::PqP3D65, 4 => Self::Hlg, 5 => Self::Linear, _ => Self::Auto } } }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum VideoRange { #[default] Auto = 0, Full = 1, Limited = 2 }
impl VideoRange {
    pub fn as_str(self) -> &'static str { match self { VideoRange::Full => "full", VideoRange::Limited => "limited", VideoRange::Auto => "auto" } }
    pub fn parse(s: &str) -> VideoRange { match s { "full" => VideoRange::Full, "limited" => VideoRange::Limited, _ => VideoRange::Auto } }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AudioRoutingMode { #[default] Auto = 0, Downmix5_1 = 1, Stereo7_8 = 2 }
impl AudioRoutingMode { pub fn from_i64(m: i64) -> AudioRoutingMode { match m { 1 => Self::Downmix5_1, 2 => Self::Stereo7_8, _ => Self::Auto } } }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PixelAspectMode { #[default] Square = 0, Detected = 1, Custom = 2 }
impl PixelAspectMode { pub fn from_i64(m: i64) -> PixelAspectMode { match m { 1 => Self::Detected, 2 => Self::Custom, _ => Self::Square } } }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InOutRange { pub in_point: f64, pub out_point: f64 }
impl Default for InOutRange { fn default() -> Self { InOutRange { in_point: -1.0, out_point: -1.0 } } }

#[derive(Clone, Debug, PartialEq, Default)]
pub struct PlaylistEntry { pub media_id: String, pub range: InOutRange }
#[derive(Clone, Debug, PartialEq)]
pub struct PlaylistData { pub master_fps: f64, pub canvas_width: i32, pub canvas_height: i32, pub default_gap_frames: i32, pub loop_: bool, pub items: Vec<PlaylistEntry> }
impl Default for PlaylistData { fn default() -> Self { PlaylistData { master_fps: 24.0, canvas_width: 1920, canvas_height: 1080, default_gap_frames: 10, loop_: false, items: vec![] } } }
#[derive(Clone, Debug, PartialEq)]
pub struct DualPairData { pub media_id_a: String, pub media_id_b: String, pub master_fps: f64, pub master_duration: f64, /// clip maps as saved (post-edit), opaque here
    pub clips_a: Vec<serde_json::Value>, pub clips_b: Vec<serde_json::Value>, pub in_point: i32, pub out_point: i32 }
impl Default for DualPairData { fn default() -> Self { DualPairData { media_id_a: String::new(), media_id_b: String::new(), master_fps: 24.0, master_duration: 0.0, clips_a: vec![], clips_b: vec![], in_point: -1, out_point: -1 } } }

#[derive(Clone, Debug, PartialEq)]
pub struct MediaItem {
    pub id: String, pub name: String, pub path: String, pub media_type: MediaType, pub duration: f64, pub has_audio: bool, pub thumbnail_path: String,
    pub range: InOutRange, pub image_seq: ImageSequenceData, pub playlist: PlaylistData, pub video: VideoMetadata, pub adobe: AdobeMetadata,
    pub video_range_override: VideoRange, pub transfer_override: TransferOverride, pub pixel_aspect_mode: PixelAspectMode, pub custom_par_num: i32, pub custom_par_den: i32,
    /// −1 = Auto (follow the detected display matrix)
    pub rotation_override: i32, pub audio_routing_mode: AudioRoutingMode,
    /// the colour pins (opaque maps, carried as saved)
    pub ocio_clip: serde_json::Map<String, serde_json::Value>, pub min_color_clip: serde_json::Map<String, serde_json::Value>,
    pub dual_pair: DualPairData,
    /// in memory only
    pub load_failed: bool,
}
impl Default for MediaItem {
    fn default() -> Self { MediaItem { id: String::new(), name: String::new(), path: String::new(), media_type: MediaType::Video, duration: 0.0, has_audio: false, thumbnail_path: String::new(), range: InOutRange::default(), image_seq: ImageSequenceData::default(), playlist: PlaylistData::default(), video: VideoMetadata::default(), adobe: AdobeMetadata::default(), video_range_override: VideoRange::Auto, transfer_override: TransferOverride::Auto, pixel_aspect_mode: PixelAspectMode::Square, custom_par_num: 1, custom_par_den: 1, rotation_override: -1, audio_routing_mode: AudioRoutingMode::Auto, ocio_clip: Default::default(), min_color_clip: Default::default(), dual_pair: DualPairData::default(), load_failed: false } }
}

/// QCView `ProjectBin`: a fixed shell that accepts one type; items are referenced by id.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectBin { pub name: String, pub accepts: MediaType, pub item_ids: Vec<String>, pub is_open: bool }

/// The six canonical bins (QCView's seeded layout; the manual's "three bins" is wrong — row 141).
pub fn seed_bins() -> Vec<ProjectBin> {
    let b = |name: &str, t, open| ProjectBin { name: name.into(), accepts: t, item_ids: vec![], is_open: open };
    vec![b("Videos", MediaType::Video, true), b("Audio", MediaType::Audio, false), b("Images", MediaType::Image, false), b("Playlists", MediaType::Playlist, false), b("Dual Views", MediaType::DualPair, false), b("Live", MediaType::LiveStream, false)]
}
