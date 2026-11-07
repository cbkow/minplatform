//! `qcv-meta` — the family's media metadata (canon §23, rows 132–136). Ported from `QCView-Player/src/project/`
//! `media_item.h` (`VideoMetadata` / `AdobeMetadata` / `ImageSequenceData`), `ffmpeg_metadata_extractor.{h,cpp}` (the
//! extractor and the three probes: the colour-tag name tables, the pixel-format depth / alpha rules, the display-matrix
//! rotation snap, the embedded-timecode walk, the ProRes fourcc / profile flavours, the container family split on
//! `major_brand`, the MXF operational pattern from the OP UL, the authoring-tool assembly, the DNxHD / DNxHR CID tables with
//! the fixed-frame bitrate, the all-streams audio rule, the HDR10 side data, the container-range capture between open and
//! find_stream_info), `adobe_metadata_extractor.{h,cpp}` (exiftool's dozen keys — read here from the XMP packet FFmpeg
//! exports and the container's own tags, row 134; the `xmp` module), `decode/timecode_formatter.{h,cpp}` (row 136; lives
//! here so ufb / minNotes get it through the family crate, re-exported by `qcv-player` for the Transport), and
//! `metadata_service.{h,cpp}` (the thread-pool service, with row 132's latest-wins generations). `decode/stream_extent.h`
//! (the packet-count scan for duration-less containers) lives here too and `qcv-player` takes it from here.
//! MIT OR Apache-2.0 like `min-paths`; the C ABI (`qcv-meta-ffi`) arrives with the first non-Rust consumer.
use ffmpeg_sys_next as ff;
use std::ffi::{CStr, CString};

pub mod xmp;
pub mod timecode;
pub mod stream_extent;
pub mod service;
pub use timecode::TimecodeFormatter;
pub use service::{MetadataService, MetadataEvent};
pub use stream_extent::{scan_stream_extent, StreamExtent};

/// QCView's `VideoMetadata`: what the Inspector, the bins and the Transport read.
#[derive(Clone, Debug, PartialEq)]
pub struct VideoMetadata {
    /// true after an extract / probe populated it
    pub loaded: bool,
    pub file_size: i64,
    pub width: i32, pub height: i32, pub frame_rate: f64, pub total_frames: i32, /// seconds
    pub duration: f64,
    pub video_codec: String, /// display-ready flavour ("ProRes 422 HQ", "DNxHD 175x", "High 4:2:2"); empty hides the row
    pub codec_profile: String, pub pixel_format: String, pub bit_depth: i32, pub has_alpha: bool, pub is_rgb: bool,
    pub sar_num: i32, pub sar_den: i32, pub is_anamorphic: bool, /// clockwise display degrees {0, 90, 180, 270}
    pub rotation_deg: i32,
    pub colorspace: String, pub color_primaries: String, pub color_transfer: String, pub color_range: String, /// "p-t-m" indices; empty when all unspecified
    pub nclc_tag: String, /// the video stream's range as the DEMUXER set it (before find_stream_info folds in the bitstream)
    pub container_range_tag: String,
    pub is_hdr_content: bool, pub max_cll: i32, pub max_fall: i32, pub mastering_max_nits: f64,
    pub has_embedded_timecode: bool, pub embedded_timecode: String,
    pub audio_codec: String, pub audio_sample_rate: i32, pub audio_channels: i32, pub audio_stream_count: i32, pub audio_channel_layout_name: String,
    /// a present video stream with no decoder (camera RAW …)
    pub unsupported_codec: bool, pub camera_vendor: String, pub camera_model: String,
    pub container_format: String, pub mxf_operational_pattern: String, pub encoder_tool: String, pub container_bitrate: i64, pub video_bitrate: i64,
}
/// QCView's `VideoMetadata` struct defaults: bitDepth 8, SAR 1:1 (the `.qcvproj` writer sees these for an unprobed item).
impl Default for VideoMetadata { fn default() -> Self { VideoMetadata { loaded: false, file_size: 0, width: 0, height: 0, frame_rate: 0.0, total_frames: 0, duration: 0.0, video_codec: String::new(), codec_profile: String::new(), pixel_format: String::new(), bit_depth: 8, has_alpha: false, is_rgb: false, sar_num: 1, sar_den: 1, is_anamorphic: false, rotation_deg: 0, colorspace: String::new(), color_primaries: String::new(), color_transfer: String::new(), color_range: String::new(), nclc_tag: String::new(), container_range_tag: String::new(), is_hdr_content: false, max_cll: 0, max_fall: 0, mastering_max_nits: 0.0, has_embedded_timecode: false, embedded_timecode: String::new(), audio_codec: String::new(), audio_sample_rate: 0, audio_channels: 0, audio_stream_count: 0, audio_channel_layout_name: String::new(), unsupported_codec: false, camera_vendor: String::new(), camera_model: String::new(), container_format: String::new(), mxf_operational_pattern: String::new(), encoder_tool: String::new(), container_bitrate: 0, video_bitrate: 0 } } }
impl VideoMetadata { fn fresh() -> VideoMetadata { VideoMetadata::default() } }

/// QCView's `AdobeMetadata`: the project links and the timecodes exiftool used to read (row 134: from FFmpeg now).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AdobeMetadata {
    pub loaded: bool,
    pub ae_project_path: String, pub premiere_win_path: String, pub premiere_mac_path: String,
    pub qt_start_timecode: String, pub qt_timecode: String, pub qt_creation_date: String, pub qt_media_create_date: String,
    pub mxf_start_timecode: String, pub mxf_timecode_at_start: String, pub xmp_start_timecode: String, pub xmp_alt_timecode: String, pub userdata_timecode: String,
}
impl AdobeMetadata {
    pub fn has_any_adobe_project(&self) -> bool { !self.ae_project_path.is_empty() || !self.premiere_win_path.is_empty() || !self.premiere_mac_path.is_empty() }
    pub fn has_any_timecode(&self) -> bool { [&self.qt_start_timecode, &self.qt_timecode, &self.mxf_start_timecode, &self.mxf_timecode_at_start, &self.xmp_start_timecode, &self.xmp_alt_timecode, &self.userdata_timecode].iter().any(|s| !s.is_empty()) }
}

/// QCView's `ImageSequenceData` (the sequence's own facts; `min-image` fills the layers in 2c).
#[derive(Clone, Debug, PartialEq)]
pub struct ImageSequenceData {
    /// e.g. "shot_%04d.exr"
    pub pattern: String, /// the full path pattern for libavformat's image2
    pub ffmpeg_pattern: String, pub directory: String, pub frame_count: i32, pub start_frame: i32, pub end_frame: i32, pub frame_rate: f64, pub duration: f64, pub width: i32, pub height: i32,
    /// "EXR", "TIFF", "PNG", "JPEG" …
    pub format: String, /// the selected EXR layer
    pub layer: String, pub available_layers: Vec<String>, pub compression: String, /// an optional companion .wav
    pub audio_file: String,
    /// frames absent on disk (0-based offsets from `start_frame`; in memory only — QCView does not persist them)
    pub missing_frames: Vec<i32>,
    /// the per-clip cache stride (QCView: persisted so "downshift this 4K seq to 3×" survives a reopen); 1 = every frame
    pub cache_stride: i32,
}
impl Default for ImageSequenceData { fn default() -> Self { ImageSequenceData { pattern: String::new(), ffmpeg_pattern: String::new(), directory: String::new(), frame_count: 0, start_frame: 1, end_frame: 1, frame_rate: 23.976, duration: 0.0, width: 0, height: 0, format: String::new(), layer: String::new(), available_layers: vec![], compression: String::new(), audio_file: String::new(), missing_frames: vec![], cache_stride: 1 } } }

// ---- the name tables -----------------------------------------------------------------------------------------------
pub fn colorspace_name(cs: ff::AVColorSpace) -> &'static str {
    use ff::AVColorSpace::*;
    match cs { AVCOL_SPC_RGB => "rgb", AVCOL_SPC_BT709 => "bt709", AVCOL_SPC_FCC => "fcc", AVCOL_SPC_BT470BG => "bt470bg", AVCOL_SPC_SMPTE170M => "smpte170m", AVCOL_SPC_SMPTE240M => "smpte240m", AVCOL_SPC_YCGCO => "ycgco", AVCOL_SPC_BT2020_NCL => "bt2020nc", AVCOL_SPC_BT2020_CL => "bt2020c", AVCOL_SPC_SMPTE2085 => "smpte2085", AVCOL_SPC_CHROMA_DERIVED_NCL => "chroma-derived-nc", AVCOL_SPC_CHROMA_DERIVED_CL => "chroma-derived-c", AVCOL_SPC_ICTCP => "ictcp", _ => "" }
}
pub fn primaries_name(p: ff::AVColorPrimaries) -> &'static str {
    use ff::AVColorPrimaries::*;
    match p { AVCOL_PRI_BT709 => "bt709", AVCOL_PRI_BT470M => "bt470m", AVCOL_PRI_BT470BG => "bt470bg", AVCOL_PRI_SMPTE170M => "smpte170m", AVCOL_PRI_SMPTE240M => "smpte240m", AVCOL_PRI_FILM => "film", AVCOL_PRI_BT2020 => "bt2020", AVCOL_PRI_SMPTE428 => "smpte428", AVCOL_PRI_SMPTE431 => "smpte431", AVCOL_PRI_SMPTE432 => "smpte432", AVCOL_PRI_EBU3213 => "ebu3213", _ => "" }
}
pub fn transfer_name(t: ff::AVColorTransferCharacteristic) -> &'static str {
    use ff::AVColorTransferCharacteristic::*;
    match t { AVCOL_TRC_BT709 => "bt709", AVCOL_TRC_GAMMA22 => "gamma22", AVCOL_TRC_GAMMA28 => "gamma28", AVCOL_TRC_SMPTE170M => "smpte170m", AVCOL_TRC_SMPTE240M => "smpte240m", AVCOL_TRC_LINEAR => "linear", AVCOL_TRC_LOG => "log100", AVCOL_TRC_LOG_SQRT => "log316", AVCOL_TRC_IEC61966_2_4 => "iec61966-2-4", AVCOL_TRC_BT1361_ECG => "bt1361e", AVCOL_TRC_IEC61966_2_1 => "iec61966-2-1", AVCOL_TRC_BT2020_10 => "bt2020-10", AVCOL_TRC_BT2020_12 => "bt2020-12", AVCOL_TRC_SMPTE2084 => "smpte2084", AVCOL_TRC_SMPTE428 => "smpte428", AVCOL_TRC_ARIB_STD_B67 => "arib-std-b67", _ => "" }
}
pub fn range_name(r: ff::AVColorRange) -> &'static str { match r { ff::AVColorRange::AVCOL_RANGE_MPEG => "limited", ff::AVColorRange::AVCOL_RANGE_JPEG => "full", _ => "" } }
/// Bit depth from the pixel-format name (the old app's rule).
pub fn bit_depth_from_pix_fmt_name(name: &str) -> i32 { if name.contains("16le") || name.contains("16be") || name.contains("p16") { 16 } else if name.contains("12le") || name.contains("12be") || name.contains("p12") { 12 } else if name.contains("10le") || name.contains("10be") || name.contains("p10") { 10 } else { 8 } }
pub fn has_alpha_from_pix_fmt_name(name: &str) -> bool { name.starts_with("yuva") || name.starts_with("gbra") || name.contains("rgba") || name.contains("bgra") || name.contains("argb") || name.contains("abgr") || name.contains("ayuv") }
/// PQ or HLG at ≥ 10 bits.
pub fn is_hdr_from_transfer(transfer: &str, bit_depth: i32) -> bool { bit_depth >= 10 && (transfer == "smpte2084" || transfer == "arib-std-b67") }
pub fn nclc_tag_from_indices(p: i32, t: i32, m: i32) -> String { if p == 0 && t == 0 && m == 0 { String::new() } else { format!("{p}-{t}-{m}") } }

// ---- DNxHD / DNxHR -------------------------------------------------------------------------------------------------
/// The 32-bit Compression ID at byte 0x28 of every VC-3 frame header (dnxhddec.c); FFmpeg's profile is generic.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DnxHeader { pub cid: i32, pub bit_depth: i32, pub interlaced: bool, /// the packet size (== the coded frame size for DNxHD)
    pub frame_bytes: i32 }
pub fn parse_dnx_header(buf: &[u8]) -> Option<DnxHeader> {
    if buf.len() < 0x2D { return None }
    let prefix_a = buf[0] == 0 && buf[1] == 0 && buf[2] == 0x02 && buf[3] == 0x80 && (1..=3).contains(&buf[4]);
    let prefix_b = buf[0] == 0 && buf[1] == 0 && buf[2] == 0x03 && buf[3] == 0x8C && buf[4] == 0x03;
    if !prefix_a && !prefix_b { return None }
    let bit_depth = match buf[0x21] >> 5 { 1 => 8, 2 => 10, 3 => 12, _ => 0 };
    let cid = ((buf[0x28] as i32) << 24) | ((buf[0x29] as i32) << 16) | ((buf[0x2A] as i32) << 8) | buf[0x2B] as i32;
    if cid <= 0 { return None }
    Some(DnxHeader { cid, bit_depth, interlaced: (buf[5] & 2) != 0, frame_bytes: buf.len() as i32 })
}
struct DnxHdEntry { cid: i32, ten_bit: bool, is_444: bool, mbps: [i32; 5] }
/// Avid's bandwidth names by CID × frame-rate bucket {23.976/24, 25, 29.97/30, 50, 59.94/60}; 0 = not a valid combination.
const DNXHD_TABLE: [DnxHdEntry; 12] = [
    DnxHdEntry { cid: 1235, ten_bit: true,  is_444: false, mbps: [175, 185, 220, 365, 440] },
    DnxHdEntry { cid: 1237, ten_bit: false, is_444: false, mbps: [115, 120, 145, 240, 290] },
    DnxHdEntry { cid: 1238, ten_bit: false, is_444: false, mbps: [175, 185, 220, 365, 440] },
    DnxHdEntry { cid: 1241, ten_bit: true,  is_444: false, mbps: [0, 185, 220, 185, 220] },
    DnxHdEntry { cid: 1242, ten_bit: false, is_444: false, mbps: [0, 120, 145, 120, 145] },
    DnxHdEntry { cid: 1243, ten_bit: false, is_444: false, mbps: [0, 185, 220, 185, 220] },
    DnxHdEntry { cid: 1244, ten_bit: false, is_444: false, mbps: [0, 120, 145, 120, 145] },
    DnxHdEntry { cid: 1250, ten_bit: true,  is_444: false, mbps: [90, 90, 110, 175, 220] },
    DnxHdEntry { cid: 1251, ten_bit: false, is_444: false, mbps: [90, 90, 110, 175, 220] },
    DnxHdEntry { cid: 1252, ten_bit: false, is_444: false, mbps: [60, 60, 75, 115, 145] },
    DnxHdEntry { cid: 1253, ten_bit: false, is_444: false, mbps: [36, 36, 45, 75, 90] },
    DnxHdEntry { cid: 1256, ten_bit: true,  is_444: true,  mbps: [350, 365, 440, 730, 880] },
];
fn dnx_fps_bucket(fps: f64) -> usize { if fps < 24.5 { 0 } else if fps < 27.5 { 1 } else if fps < 35.0 { 2 } else if fps < 55.0 { 3 } else { 4 } }
pub fn dnx_profile_name(h: &DnxHeader, fps: f64) -> String {
    match h.cid { 1270 => return "DNxHR 444".into(), 1271 => return "DNxHR HQX".into(), 1272 => return "DNxHR HQ".into(), 1273 => return "DNxHR SQ".into(), 1274 => return "DNxHR LB".into(), _ => {} }
    for e in &DNXHD_TABLE {
        if e.cid != h.cid { continue }
        let mbps = if fps > 0.0 { e.mbps[dnx_fps_bucket(fps)] } else { 0 };
        if mbps <= 0 { break }
        let mut name = if e.is_444 { "DNxHD 444 ".to_string() } else { "DNxHD ".to_string() };
        name.push_str(&mbps.to_string()); if e.ten_bit { name.push('x'); }
        return name;
    }
    let mut name = "DNxHD".to_string();   // thin-raster / legacy CIDs: stay truthful
    if h.bit_depth > 8 { name.push_str(&format!(" {}-bit", h.bit_depth)); }
    name.push_str(&format!(" (CID {})", h.cid)); name
}
pub fn dnx_is_fixed_frame_size(cid: i32) -> bool { (1235..=1260).contains(&cid) }

// ---- the FFmpeg extractor -------------------------------------------------------------------------------------------
unsafe fn cstr(p: *const libc::c_char) -> String { unsafe { if p.is_null() { String::new() } else { CStr::from_ptr(p).to_string_lossy().into_owned() } } }
unsafe fn dict_value(d: *mut ff::AVDictionary, key: &str) -> String { unsafe {
    if d.is_null() { return String::new() }
    let k = CString::new(key).unwrap();
    let e = ff::av_dict_get(d, k.as_ptr(), std::ptr::null(), 0);
    if e.is_null() { String::new() } else { cstr((*e).value).trim().to_string() }
} }
pub fn pix_fmt_name(f: ff::AVPixelFormat) -> String { unsafe { cstr(ff::av_get_pix_fmt_name(f)) } }

/// `libavutil/mastering_display_metadata.h` (not in the sys bindings' feature set): the C layouts, verbatim.
#[repr(C)] struct AVContentLightMetadata { max_cll: libc::c_uint, max_fall: libc::c_uint }
#[repr(C)] struct AVMasteringDisplayMetadata { display_primaries: [[ff::AVRational; 2]; 3], white_point: [ff::AVRational; 2], min_luminance: ff::AVRational, max_luminance: ff::AVRational, has_primaries: libc::c_int, has_luminance: libc::c_int }

struct Opened { ctx: *mut ff::AVFormatContext, container_range: ff::AVColorRange }
impl Drop for Opened { fn drop(&mut self) { unsafe { if !self.ctx.is_null() { ff::avformat_close_input(&mut self.ctx); } } } }
/// `openFile`: the video stream's `color_range` is captured BETWEEN open and find_stream_info (the latter runs the decoder
/// and folds bitstream tags / decoder conventions into codecpar).
unsafe fn open_file(path: &str) -> Option<Opened> { unsafe {
    let cpath = CString::new(path).ok()?;
    let mut ctx: *mut ff::AVFormatContext = ff::avformat_alloc_context();
    if ctx.is_null() { return None }
    if ff::avformat_open_input(&mut ctx, cpath.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut()) < 0 { return None }
    let mut o = Opened { ctx, container_range: ff::AVColorRange::AVCOL_RANGE_UNSPECIFIED };
    for i in 0..(*ctx).nb_streams { let s = *(*ctx).streams.add(i as usize); if !s.is_null() && !(*s).codecpar.is_null() && (*(*s).codecpar).codec_type == ff::AVMediaType::AVMEDIA_TYPE_VIDEO { o.container_range = (*(*s).codecpar).color_range; break } }
    if ff::avformat_find_stream_info(ctx, std::ptr::null_mut()) < 0 { return None }
    Some(o)
} }
unsafe fn best_video_stream(ctx: *mut ff::AVFormatContext) -> i32 { unsafe { ff::av_find_best_stream(ctx, ff::AVMediaType::AVMEDIA_TYPE_VIDEO, -1, -1, std::ptr::null_mut(), 0) } }
/// Display-matrix rotation → clockwise display degrees snapped to quarters (phones land near-quarter, −89.97 …).
unsafe fn rotation_from_stream(stream: *mut ff::AVStream) -> i32 { unsafe {
    let cp = (*stream).codecpar;
    let sd = ff::av_packet_side_data_get((*cp).coded_side_data, (*cp).nb_coded_side_data, ff::AVPacketSideDataType::AV_PKT_DATA_DISPLAYMATRIX);
    if sd.is_null() || (*sd).size < 9 * 4 { return 0 }
    let theta = ff::av_display_rotation_get((*sd).data as *const i32);
    if theta.is_nan() { return 0 }
    let mut rot = ((-theta).round() as i32) % 360; if rot < 0 { rot += 360; }
    ((rot + 45) / 90 * 90) % 360
} }
unsafe fn read_embedded_timecode(ctx: *mut ff::AVFormatContext, video: *mut ff::AVStream) -> String { unsafe {
    if !video.is_null() { let tc = dict_value((*video).metadata, "timecode"); if !tc.is_empty() { return tc } }
    let tc = dict_value((*ctx).metadata, "timecode"); if !tc.is_empty() { return tc }
    for i in 0..(*ctx).nb_streams { let tc = dict_value((**(*ctx).streams.add(i as usize)).metadata, "timecode"); if !tc.is_empty() { return tc } }   // some containers put TC on a data stream
    String::new()
} }
const fn mktag(a: u8, b: u8, c: u8, d: u8) -> u32 { (a as u32) | ((b as u32) << 8) | ((c as u32) << 16) | ((d as u32) << 24) }
/// ProRes: the MOV fourcc is authoritative (present even when the header probe leaves `profile` unset); ProRes-in-MXF has
/// no fourcc → the stream profile FFmpeg filled from the frame header. Other codecs: FFmpeg's profile name or empty.
unsafe fn codec_profile_name(cp: *const ff::AVCodecParameters) -> String { unsafe {
    if (*cp).codec_id == ff::AVCodecID::AV_CODEC_ID_PRORES {
        let by_tag = match (*cp).codec_tag { t if t == mktag(b'a', b'p', b'c', b'o') => "ProRes 422 Proxy", t if t == mktag(b'a', b'p', b'c', b's') => "ProRes 422 LT", t if t == mktag(b'a', b'p', b'c', b'n') => "ProRes 422", t if t == mktag(b'a', b'p', b'c', b'h') => "ProRes 422 HQ", t if t == mktag(b'a', b'p', b'4', b'h') => "ProRes 4444", t if t == mktag(b'a', b'p', b'4', b'x') => "ProRes 4444 XQ", _ => "" };
        if !by_tag.is_empty() { return by_tag.into() }
        return match (*cp).profile { 0 => "ProRes 422 Proxy", 1 => "ProRes 422 LT", 2 => "ProRes 422", 3 => "ProRes 422 HQ", 4 => "ProRes 4444", 5 => "ProRes 4444 XQ", _ => "ProRes" }.into();
    }
    cstr(ff::avcodec_profile_name((*cp).codec_id, (*cp).profile))
} }
fn file_ext_lower(path: &str) -> String { std::path::Path::new(path).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default() }
/// Display-ready container family: the ISO-BMFF union is split on `major_brand`, the extension as the tiebreak. Never empty
/// for an opened file — the empty value is the "cache predates the field" sentinel.
unsafe fn container_format_name(ctx: *mut ff::AVFormatContext, path: &str) -> String { unsafe {
    let name = if (*ctx).iformat.is_null() { String::new() } else { cstr((*(*ctx).iformat).name) };
    let ext = file_ext_lower(path);
    if name == "mxf" { return "MXF".into() }
    if name.starts_with("mov,") {
        let brand = dict_value((*ctx).metadata, "major_brand").to_lowercase();
        if brand.starts_with("qt") { return "QuickTime".into() }
        if brand.starts_with("3g") { return "3GPP".into() }
        if brand == "mj2" || brand == "mjp2" { return "Motion JPEG 2000".into() }
        if !brand.is_empty() { return "MP4".into() }
        return if ext == "mov" { "QuickTime".into() } else { "MP4".into() }   // no ftyp at all: classic QuickTime writers omit it
    }
    if name.starts_with("matroska") { return if ext == "webm" { "WebM".into() } else { "Matroska".into() } }
    match name.as_str() { "avi" => return "AVI".into(), "mpegts" => return "MPEG-TS".into(), "mpeg" => return "MPEG-PS".into(), "gxf" => return "GXF".into(), "asf" => return "ASF / WMV".into(), "wav" => return "WAV".into(), "aiff" => return "AIFF".into(), "flac" => return "FLAC".into(), "mp3" => return "MP3".into(), "ogg" => return "Ogg".into(), "ivf" => return "IVF".into(), "yuv4mpegpipe" => return "Y4M".into(), _ => {} }
    if name.starts_with("image2") { return "Image sequence".into() }
    if !(*ctx).iformat.is_null() && !(*(*ctx).iformat).long_name.is_null() { return cstr((*(*ctx).iformat).long_name) }
    if name.is_empty() { "Unknown".into() } else { name }
} }
/// SMPTE ST 377-1 from mxfdec's `operational_pattern_ul`: byte 12 = item complexity (1/2/3 = OP1/2/3, 0x10 = OP-Atom),
/// byte 13 = package complexity (a/b/c).
pub fn mxf_operational_pattern_name(ul: &str) -> String {
    let ul: String = ul.chars().filter(|c| *c != '.').collect();
    if ul.len() != 32 { return String::new() }
    let (Ok(item), Ok(pkg)) = (i32::from_str_radix(&ul[24..26], 16), i32::from_str_radix(&ul[26..28], 16)) else { return String::new() };
    if item == 0x10 { return "OP-Atom".into() }
    if (1..=3).contains(&item) { let mut op = format!("OP{item}"); if (1..=3).contains(&pkg) { op.push((b'a' + (pkg - 1) as u8) as char); } return op }
    format!("OP {item:02x}.{pkg:02x}")   // Sony's optical-disc variant (0x40) and the rest: the raw bytes
}
/// MXF Identification set → "Adobe Media Encoder 13.0.2 (win32)" (the vendor prepended only when the product doesn't name
/// it); everything else the `encoder` tag.
pub fn encoder_tool_name(company: &str, product: &str, version: &str, platform: &str, encoder: &str) -> String {
    if product.is_empty() { return if encoder.is_empty() { company.into() } else { encoder.into() } }
    let mut tool = product.to_string();
    if !company.is_empty() { let first = company.split(' ').next().unwrap_or(""); if !first.is_empty() && !tool.to_lowercase().contains(&first.to_lowercase()) { tool = format!("{company} {tool}"); } }
    if !version.is_empty() { tool.push(' '); tool.push_str(version); }
    if !platform.is_empty() { tool.push_str(&format!(" ({platform})")); }
    tool
}
unsafe fn fill_container_info(ctx: *mut ff::AVFormatContext, path: &str, m: &mut VideoMetadata) { unsafe {
    m.container_format = container_format_name(ctx, path);
    m.mxf_operational_pattern = mxf_operational_pattern_name(&dict_value((*ctx).metadata, "operational_pattern_ul"));
    m.encoder_tool = encoder_tool_name(&dict_value((*ctx).metadata, "company_name"), &dict_value((*ctx).metadata, "product_name"), &dict_value((*ctx).metadata, "product_version"), &dict_value((*ctx).metadata, "application_platform"), &dict_value((*ctx).metadata, "encoder"));
    if (*ctx).bit_rate > 0 { m.container_bitrate = (*ctx).bit_rate; } else if m.file_size > 0 && m.duration > 0.0 { m.container_bitrate = (m.file_size as f64 * 8.0 / m.duration).round() as i64; }   // the whole-file average when the container declares none (MXF)
} }
unsafe fn read_first_dnx_header(ctx: *mut ff::AVFormatContext, idx: i32) -> Option<DnxHeader> { unsafe {
    let pkt = ff::av_packet_alloc(); if pkt.is_null() { return None }
    let mut found = None;
    for _ in 0..256 {
        if ff::av_read_frame(ctx, pkt) < 0 { break }
        if (*pkt).stream_index == idx { found = parse_dnx_header(std::slice::from_raw_parts((*pkt).data, (*pkt).size.max(0) as usize)); ff::av_packet_unref(pkt); break }
        ff::av_packet_unref(pkt);
    }
    let mut p = pkt; ff::av_packet_free(&mut p);
    found
} }
unsafe fn refine_dnx_profile(ctx: *mut ff::AVFormatContext, idx: i32, m: &mut VideoMetadata) { unsafe {
    let Some(h) = read_first_dnx_header(ctx, idx) else { return };
    m.codec_profile = dnx_profile_name(&h, m.frame_rate);
    if m.video_bitrate <= 0 && dnx_is_fixed_frame_size(h.cid) && h.frame_bytes > 0 && m.frame_rate > 0.0 { m.video_bitrate = (h.frame_bytes as f64 * 8.0 * m.frame_rate).round() as i64; }
} }
unsafe fn extract_video_stream(ctx: *mut ff::AVFormatContext, path: &str, m: &mut VideoMetadata) { unsafe {
    let idx = best_video_stream(ctx); if idx < 0 { return }
    let stream = *(*ctx).streams.add(idx as usize); let cp = (*stream).codecpar;
    m.width = (*cp).width; m.height = (*cp).height;
    let sar = ff::av_guess_sample_aspect_ratio(ctx, stream, std::ptr::null_mut());
    if sar.num > 0 && sar.den > 0 { m.sar_num = sar.num; m.sar_den = sar.den; }
    m.is_anamorphic = m.sar_num != m.sar_den;
    m.rotation_deg = rotation_from_stream(stream);
    let mut fr = (*stream).r_frame_rate; if fr.num == 0 || fr.den == 0 { fr = (*stream).avg_frame_rate; }
    m.frame_rate = if fr.num > 0 && fr.den > 0 { fr.num as f64 / fr.den as f64 } else { 24.0 };
    if (*ctx).duration != ff::AV_NOPTS_VALUE {
        m.duration = (*ctx).duration as f64 / ff::AV_TIME_BASE as f64;
        m.total_frames = (m.duration * m.frame_rate).round() as i32;   // round, don't truncate (720.99999 → 721)
    } else {
        let ext = scan_stream_extent(path, idx, 0);   // duration-less containers (animated WebP / GIF / APNG): count the packets
        if ext.ok { m.total_frames = ext.frames; m.duration = if ext.duration_us > 0 { ext.duration_us as f64 / 1e6 } else { ext.frames as f64 / m.frame_rate }; }
    }
    let codec = ff::avcodec_find_decoder((*cp).codec_id);
    m.video_codec = if codec.is_null() { "unknown".into() } else { cstr((*codec).name) };
    m.codec_profile = codec_profile_name(cp);
    if (*cp).bit_rate > 0 { m.video_bitrate = (*cp).bit_rate; }
    if (*cp).codec_id == ff::AVCodecID::AV_CODEC_ID_DNXHD { refine_dnx_profile(ctx, idx, m); }
    m.unsupported_codec = codec.is_null();
    let fmt: ff::AVPixelFormat = std::mem::transmute::<i32, ff::AVPixelFormat>((*cp).format);
    m.pixel_format = pix_fmt_name(fmt);
    m.bit_depth = bit_depth_from_pix_fmt_name(&m.pixel_format); m.has_alpha = has_alpha_from_pix_fmt_name(&m.pixel_format);
    let desc = ff::av_pix_fmt_desc_get(fmt); if !desc.is_null() { m.is_rgb = ((*desc).flags as i32 & ff::AV_PIX_FMT_FLAG_RGB) != 0; }
    if (*cp).format == ff::AVPixelFormat::AV_PIX_FMT_NONE as i32 {
        if (*cp).codec_id == ff::AVCodecID::AV_CODEC_ID_PRORES_RAW { m.pixel_format = "bayer_rggb16le".into(); m.bit_depth = 12; m.has_alpha = false; m.is_rgb = false; }   // the CODED precision
        else if (*cp).bits_per_raw_sample > 0 { m.bit_depth = (*cp).bits_per_raw_sample; }
    }
    m.colorspace = colorspace_name((*cp).color_space).into(); m.color_primaries = primaries_name((*cp).color_primaries).into();
    m.color_transfer = transfer_name((*cp).color_trc).into(); m.color_range = range_name((*cp).color_range).into();
    m.nclc_tag = nclc_tag_from_indices((*cp).color_primaries as i32, (*cp).color_trc as i32, (*cp).color_space as i32);
    m.is_hdr_content = is_hdr_from_transfer(&m.color_transfer, m.bit_depth);
    let sd = ff::av_packet_side_data_get((*cp).coded_side_data, (*cp).nb_coded_side_data, ff::AVPacketSideDataType::AV_PKT_DATA_CONTENT_LIGHT_LEVEL);
    if !sd.is_null() && (*sd).size >= std::mem::size_of::<AVContentLightMetadata>() { let cll = &*((*sd).data as *const AVContentLightMetadata); m.max_cll = cll.max_cll as i32; m.max_fall = cll.max_fall as i32; }
    let sd = ff::av_packet_side_data_get((*cp).coded_side_data, (*cp).nb_coded_side_data, ff::AVPacketSideDataType::AV_PKT_DATA_MASTERING_DISPLAY_METADATA);
    if !sd.is_null() && (*sd).size >= std::mem::size_of::<AVMasteringDisplayMetadata>() { let md = &*((*sd).data as *const AVMasteringDisplayMetadata); if md.has_luminance != 0 && md.max_luminance.den > 0 { m.mastering_max_nits = md.max_luminance.num as f64 / md.max_luminance.den as f64; } }
    let tc = read_embedded_timecode(ctx, stream);
    if !tc.is_empty() { m.has_embedded_timecode = true; m.embedded_timecode = tc; }
} }
/// ALL audio streams, not best-stream's pick: a broadcast "8-track" master is 8 mono streams, not one of 8 channels.
unsafe fn extract_audio_stream(ctx: *mut ff::AVFormatContext, m: &mut VideoMetadata) { unsafe {
    let (mut total, mut count, mut first) = (0, 0, -1i32);
    for i in 0..(*ctx).nb_streams { let s = *(*ctx).streams.add(i as usize); if s.is_null() || (*s).codecpar.is_null() || (*(*s).codecpar).codec_type != ff::AVMediaType::AVMEDIA_TYPE_AUDIO { continue } count += 1; total += (*(*s).codecpar).ch_layout.nb_channels; if first < 0 { first = i as i32; } }
    if first < 0 { return }
    let cp = (**(*ctx).streams.add(first as usize)).codecpar;
    let codec = ff::avcodec_find_decoder((*cp).codec_id);
    m.audio_codec = if codec.is_null() { "unknown".into() } else { cstr((*codec).name) };
    m.audio_sample_rate = (*cp).sample_rate; m.audio_channels = total; m.audio_stream_count = count;
    if count == 1 && (*cp).ch_layout.nb_channels > 0 && (*cp).ch_layout.order != ff::AVChannelOrder::AV_CHANNEL_ORDER_UNSPEC {
        let mut buf = [0 as libc::c_char; 64];
        if ff::av_channel_layout_describe(&(*cp).ch_layout, buf.as_mut_ptr(), buf.len()) > 0 { m.audio_channel_layout_name = cstr(buf.as_ptr()); }
    } else if count > 1 && total == count { m.audio_channel_layout_name = format!("{count} mono tracks"); }
    else if total > 0 { m.audio_channel_layout_name = format!("{total} channels"); }
} }

/// `FFmpegMetadataExtractor::extract`.
pub fn extract(path: &str) -> VideoMetadata {
    let mut m = VideoMetadata::fresh();
    if path.is_empty() { return m }
    unsafe {
        let Some(o) = open_file(path) else { return m };
        let ctx = o.ctx;
        m.container_range_tag = range_name(o.container_range).into();
        m.file_size = std::fs::metadata(path).map(|md| md.len() as i64).unwrap_or(0);
        if (*ctx).duration != ff::AV_NOPTS_VALUE { m.duration = (*ctx).duration as f64 / ff::AV_TIME_BASE as f64; }
        extract_video_stream(ctx, path, &mut m);
        m.loaded = true;   // BEFORE the audio extract: video facts + an empty audio block beat a stuck "Reading metadata…"
        extract_audio_stream(ctx, &mut m);
        m.camera_vendor = dict_value((*ctx).metadata, "company_name"); m.camera_model = dict_value((*ctx).metadata, "product_name");
        fill_container_info(ctx, path, &mut m);
    }
    m
}
/// `probeRotation`: −1 when the file won't open.
pub fn probe_rotation(path: &str) -> i32 {
    if path.is_empty() { return -1 }
    unsafe { let Some(o) = open_file(path) else { return -1 }; let idx = best_video_stream(o.ctx); if idx >= 0 { rotation_from_stream(*(*o.ctx).streams.add(idx as usize)) } else { -1 } }
}
/// `probeContainer`: the cache-migration probe (container family / OP / tool / bitrates / flavour / rate); `loaded` false
/// when the file won't open (retry later).
pub fn probe_container(path: &str) -> VideoMetadata {
    let mut m = VideoMetadata::fresh();
    if path.is_empty() { return m }
    unsafe {
        let Some(o) = open_file(path) else { return m };
        let ctx = o.ctx;
        m.container_range_tag = range_name(o.container_range).into();
        m.file_size = std::fs::metadata(path).map(|md| md.len() as i64).unwrap_or(0);
        if (*ctx).duration != ff::AV_NOPTS_VALUE { m.duration = (*ctx).duration as f64 / ff::AV_TIME_BASE as f64; }
        let idx = best_video_stream(ctx);
        if idx >= 0 {
            let stream = *(*ctx).streams.add(idx as usize); let cp = (*stream).codecpar;
            let mut fr = (*stream).r_frame_rate; if fr.num == 0 || fr.den == 0 { fr = (*stream).avg_frame_rate; }
            m.frame_rate = if fr.num > 0 && fr.den > 0 { fr.num as f64 / fr.den as f64 } else { 0.0 };
            m.codec_profile = codec_profile_name(cp);
            if (*cp).bit_rate > 0 { m.video_bitrate = (*cp).bit_rate; }
            if (*cp).codec_id == ff::AVCodecID::AV_CODEC_ID_DNXHD { refine_dnx_profile(ctx, idx, &mut m); }
        }
        fill_container_info(ctx, path, &mut m);
        m.loaded = true;
    }
    m
}
pub fn probe_duration(path: &str) -> f64 {
    if path.is_empty() { return 0.0 }
    unsafe { let Some(o) = open_file(path) else { return 0.0 }; if (*o.ctx).duration != ff::AV_NOPTS_VALUE { (*o.ctx).duration as f64 / ff::AV_TIME_BASE as f64 } else { 0.0 } }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn tables() {
        assert_eq!(bit_depth_from_pix_fmt_name("yuv422p10le"), 10); assert_eq!(bit_depth_from_pix_fmt_name("yuva444p12le"), 12); assert_eq!(bit_depth_from_pix_fmt_name("gbrpf16le"), 16); assert_eq!(bit_depth_from_pix_fmt_name("yuv420p"), 8);
        assert!(has_alpha_from_pix_fmt_name("yuva444p12le") && has_alpha_from_pix_fmt_name("bgra") && !has_alpha_from_pix_fmt_name("yuv422p10le"));
        assert!(is_hdr_from_transfer("smpte2084", 10) && !is_hdr_from_transfer("smpte2084", 8) && !is_hdr_from_transfer("bt709", 10));
        assert_eq!(nclc_tag_from_indices(1, 1, 1), "1-1-1"); assert_eq!(nclc_tag_from_indices(0, 0, 0), "");
        assert_eq!(mxf_operational_pattern_name("060e2b34.04010101.0d010201.01010900"), "OP1a"); assert_eq!(mxf_operational_pattern_name("060e2b34.04010102.0d010201.10030000"), "OP-Atom"); assert_eq!(mxf_operational_pattern_name("bad"), "");
        assert_eq!(encoder_tool_name("Adobe Systems Incorporated", "Adobe Media Encoder", "13.0.2", "win32", ""), "Adobe Media Encoder 13.0.2 (win32)");
        assert_eq!(encoder_tool_name("ARRI", "ALEXA 35", "", "", ""), "ARRI ALEXA 35"); assert_eq!(encoder_tool_name("", "", "", "", "Lavf61"), "Lavf61");
        let h = DnxHeader { cid: 1235, bit_depth: 10, interlaced: false, frame_bytes: 917504 };
        assert_eq!(dnx_profile_name(&h, 23.976), "DNxHD 175x"); assert_eq!(dnx_profile_name(&h, 59.94), "DNxHD 440x"); assert_eq!(dnx_profile_name(&DnxHeader { cid: 1272, ..h }, 24.0), "DNxHR HQ"); assert_eq!(dnx_profile_name(&DnxHeader { cid: 1241, ..h }, 24.0), "DNxHD 10-bit (CID 1241)");
        let mut buf = vec![0u8; 0x30]; buf[2] = 0x02; buf[3] = 0x80; buf[4] = 0x01; buf[0x21] = 2 << 5; buf[0x28..0x2C].copy_from_slice(&1235i32.to_be_bytes());
        assert_eq!(parse_dnx_header(&buf), Some(DnxHeader { cid: 1235, bit_depth: 10, interlaced: false, frame_bytes: 0x30 }));
    }
}
