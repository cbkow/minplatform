//! QCView's `project_io.cpp` — `.qcvproj` v2 save / load, byte-for-byte. The writer reproduces Qt's
//! `QJsonDocument::toJson(Indented)` as QCView's files show it: keys sorted, ONE space of indentation per level,
//! `"key": value`, empty containers inline (`[]`, `{}`), integral doubles written as integers (`24`, `-1`), no trailing
//! newline. `${PROJECT_DIR}` collapses paths under the project's folder (prefix + separator check; `://` untouched).
//! Symmetry is the load-bearing invariant: every field written round-trips through load to the same model.
use crate::model::*;
use serde_json::{json, Map, Value};
use std::path::Path;

pub const SCHEMA_TAG: &str = "qcview.project.v2";
pub const SCHEMA_VERSION: i64 = 2;
pub const PROJECT_DIR_TOKEN: &str = "${PROJECT_DIR}";

/// Everything the file holds (the manager owns the live state; this is its serialised shape).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct ProjectFile {
    pub uuid: String, pub name: String, pub created_at: String, pub modified_at: String,
    pub media: Vec<MediaItem>, pub bins: Vec<ProjectBin>,
    pub active_item_id: String, pub b_source_media_id: String,
}

// ---- ${PROJECT_DIR} ----------------------------------------------------------------------------------------------
fn clean_path(p: &str) -> String {
    // QDir::cleanPath: forward slashes, no trailing slash (except the root), `.` and `..` folded
    let s = p.replace('\\', "/");
    let abs = s.starts_with('/');
    let mut parts: Vec<&str> = vec![];
    for seg in s.split('/') { match seg { "" | "." => {} ".." => { if parts.last().is_some_and(|l| *l != "..") { parts.pop(); } else if !abs { parts.push(".."); } } x => parts.push(x) } }
    let joined = parts.join("/");
    if abs { format!("/{joined}") } else if joined.is_empty() { ".".into() } else { joined }
}
pub fn collapse_project_dir(abs_path: &str, project_dir: &str) -> String {
    if abs_path.is_empty() || project_dir.is_empty() { return abs_path.to_string() }
    if abs_path.contains("://") { return abs_path.to_string() }   // live-stream URLs pass through
    let (np, nd) = (clean_path(abs_path), clean_path(project_dir));
    if np == nd { return PROJECT_DIR_TOKEN.to_string() }
    if let Some(rest) = np.strip_prefix(&format!("{nd}/")) { return format!("{PROJECT_DIR_TOKEN}/{rest}") }
    abs_path.to_string()
}
pub fn expand_project_dir(stored: &str, project_dir: &str) -> String {
    if stored.is_empty() || stored.contains("://") { return stored.to_string() }
    let Some(rest) = stored.strip_prefix(PROJECT_DIR_TOKEN) else { return stored.to_string() };
    if rest.is_empty() { return project_dir.to_string() }
    let rel = rest.strip_prefix('/').unwrap_or(rest);
    format!("{}/{}", project_dir.trim_end_matches('/'), rel)
}

// ---- Qt's indented JSON ------------------------------------------------------------------------------------------
fn num_f64(v: f64) -> Value {
    // Qt prints an integral double without a decimal point; serde_json would print `24.0`
    if v.is_finite() && v.fract() == 0.0 && v.abs() < 9007199254740992.0 { Value::from(v as i64) } else { json!(v) }
}
fn write_qt(v: &Value, depth: usize, out: &mut String) {
    let pad = |n: usize| " ".repeat(n);
    match v {
        Value::Object(m) => {
            if m.is_empty() { out.push_str("{}"); return }
            let mut keys: Vec<&String> = m.keys().collect(); keys.sort();
            out.push_str("{\n");
            for (i, k) in keys.iter().enumerate() {
                out.push_str(&pad(depth + 1)); out.push_str(&serde_json::to_string(k).unwrap()); out.push_str(": ");
                write_qt(&m[*k], depth + 1, out);
                out.push_str(if i + 1 < keys.len() { ",\n" } else { "\n" });
            }
            out.push_str(&pad(depth)); out.push('}');
        }
        Value::Array(a) => {
            if a.is_empty() { out.push_str("[]"); return }
            out.push_str("[\n");
            for (i, x) in a.iter().enumerate() { out.push_str(&pad(depth + 1)); write_qt(x, depth + 1, out); out.push_str(if i + 1 < a.len() { ",\n" } else { "\n" }); }
            out.push_str(&pad(depth)); out.push(']');
        }
        Value::String(s) => out.push_str(&serde_json::to_string(s).unwrap()),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Null => out.push_str("null"),
    }
}
/// The document as QCView writes it (no trailing newline).
pub fn to_qt_json(v: &Value) -> String { let mut s = String::new(); write_qt(v, 0, &mut s); s }

// ---- sub-structs -------------------------------------------------------------------------------------------------
fn video_to_json(v: &VideoMetadata) -> Value {
    json!({ "loaded": v.loaded, "fileSize": v.file_size, "width": v.width, "height": v.height, "frameRate": num_f64(v.frame_rate), "totalFrames": v.total_frames, "duration": num_f64(v.duration),
        "videoCodec": v.video_codec, "codecProfile": v.codec_profile, "containerFormat": v.container_format, "mxfOperationalPattern": v.mxf_operational_pattern, "containerBitrate": v.container_bitrate, "videoBitrate": v.video_bitrate, "encoderTool": v.encoder_tool,
        "pixelFormat": v.pixel_format, "bitDepth": v.bit_depth, "hasAlpha": v.has_alpha, "sarNum": v.sar_num, "sarDen": v.sar_den, "isAnamorphic": v.is_anamorphic, "rotationDeg": v.rotation_deg, "unsupportedCodec": v.unsupported_codec,
        "cameraVendor": v.camera_vendor, "cameraModel": v.camera_model, "colorspace": v.colorspace, "colorPrimaries": v.color_primaries, "colorTransfer": v.color_transfer, "colorRange": v.color_range, "nclcTag": v.nclc_tag, "isHdrContent": v.is_hdr_content,
        "maxCll": v.max_cll, "maxFall": v.max_fall, "masteringMaxNits": num_f64(v.mastering_max_nits), "isRgb": v.is_rgb, "containerRangeTag": v.container_range_tag,
        "audioCodec": v.audio_codec, "audioSampleRate": v.audio_sample_rate, "audioChannels": v.audio_channels, "hasEmbeddedTimecode": v.has_embedded_timecode, "embeddedTimecode": v.embedded_timecode, "audioChannelLayoutName": v.audio_channel_layout_name })
}
fn s(o: &Map<String, Value>, k: &str) -> String { o.get(k).and_then(Value::as_str).unwrap_or("").to_string() }
fn i(o: &Map<String, Value>, k: &str, d: i64) -> i64 { o.get(k).and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64))).unwrap_or(d) }
fn f(o: &Map<String, Value>, k: &str, d: f64) -> f64 { o.get(k).and_then(Value::as_f64).unwrap_or(d) }
fn b(o: &Map<String, Value>, k: &str) -> bool { o.get(k).and_then(Value::as_bool).unwrap_or(false) }
fn obj<'a>(o: &'a Map<String, Value>, k: &str) -> Map<String, Value> { o.get(k).and_then(Value::as_object).cloned().unwrap_or_default() }
fn video_from_json(o: &Map<String, Value>) -> VideoMetadata {
    let mut v = VideoMetadata { loaded: b(o, "loaded"), file_size: i(o, "fileSize", 0), width: i(o, "width", 0) as i32, height: i(o, "height", 0) as i32, frame_rate: f(o, "frameRate", 0.0), total_frames: i(o, "totalFrames", 0) as i32, duration: f(o, "duration", 0.0),
        video_codec: s(o, "videoCodec"), codec_profile: s(o, "codecProfile"), container_format: s(o, "containerFormat"), mxf_operational_pattern: s(o, "mxfOperationalPattern"), container_bitrate: i(o, "containerBitrate", 0), video_bitrate: i(o, "videoBitrate", 0), encoder_tool: s(o, "encoderTool"),
        pixel_format: s(o, "pixelFormat"), ..Default::default() };
    // RGB-family flag: pre-field caches re-derive it from the pixel format name (QCView asks libavutil; the name rule is qcv-meta's)
    v.is_rgb = if o.contains_key("isRgb") { b(o, "isRgb") } else if !v.pixel_format.is_empty() { let p = &v.pixel_format; p.starts_with("rgb") || p.starts_with("bgr") || p.starts_with("gbr") || p.starts_with("argb") || p.starts_with("abgr") || p.starts_with("rgba") || p.starts_with("bgra") || p.starts_with("gbra") || p.starts_with("0rgb") || p.starts_with("0bgr") || p.starts_with("x2rgb") || p.starts_with("x2bgr") } else { false };
    v.bit_depth = i(o, "bitDepth", 8) as i32; v.has_alpha = b(o, "hasAlpha");
    v.sar_num = i(o, "sarNum", 1) as i32; v.sar_den = i(o, "sarDen", 1) as i32; if v.sar_num <= 0 || v.sar_den <= 0 { v.sar_num = 1; v.sar_den = 1; } v.is_anamorphic = v.sar_num != v.sar_den;
    v.rotation_deg = i(o, "rotationDeg", -1) as i32; if ![0, 90, 180, 270].contains(&v.rotation_deg) { v.rotation_deg = -1; }   // −1 = unknown: the manager schedules a header probe
    v.unsupported_codec = b(o, "unsupportedCodec"); v.camera_vendor = s(o, "cameraVendor"); v.camera_model = s(o, "cameraModel");
    v.colorspace = s(o, "colorspace"); v.color_primaries = s(o, "colorPrimaries"); v.max_cll = i(o, "maxCll", 0) as i32; v.max_fall = i(o, "maxFall", 0) as i32; v.mastering_max_nits = f(o, "masteringMaxNits", 0.0);
    v.color_transfer = s(o, "colorTransfer"); v.color_range = s(o, "colorRange"); v.container_range_tag = s(o, "containerRangeTag"); v.nclc_tag = s(o, "nclcTag"); v.is_hdr_content = b(o, "isHdrContent");
    v.audio_codec = s(o, "audioCodec"); v.audio_sample_rate = i(o, "audioSampleRate", 0) as i32; v.audio_channels = i(o, "audioChannels", 0) as i32; v.has_embedded_timecode = b(o, "hasEmbeddedTimecode"); v.embedded_timecode = s(o, "embeddedTimecode"); v.audio_channel_layout_name = s(o, "audioChannelLayoutName");
    v
}
fn adobe_to_json(a: &AdobeMetadata) -> Value {
    json!({ "loaded": a.loaded, "aeProjectPath": a.ae_project_path, "premiereWinPath": a.premiere_win_path, "premiereMacPath": a.premiere_mac_path, "qtStartTimecode": a.qt_start_timecode, "qtTimecode": a.qt_timecode, "qtCreationDate": a.qt_creation_date, "qtMediaCreateDate": a.qt_media_create_date,
        "mxfStartTimecode": a.mxf_start_timecode, "mxfTimecodeAtStart": a.mxf_timecode_at_start, "xmpStartTimecode": a.xmp_start_timecode, "xmpAltTimecode": a.xmp_alt_timecode, "userdataTimecode": a.userdata_timecode })
}
fn adobe_from_json(o: &Map<String, Value>) -> AdobeMetadata {
    AdobeMetadata { loaded: b(o, "loaded"), ae_project_path: s(o, "aeProjectPath"), premiere_win_path: s(o, "premiereWinPath"), premiere_mac_path: s(o, "premiereMacPath"), qt_start_timecode: s(o, "qtStartTimecode"), qt_timecode: s(o, "qtTimecode"), qt_creation_date: s(o, "qtCreationDate"), qt_media_create_date: s(o, "qtMediaCreateDate"),
        mxf_start_timecode: s(o, "mxfStartTimecode"), mxf_timecode_at_start: s(o, "mxfTimecodeAtStart"), xmp_start_timecode: s(o, "xmpStartTimecode"), xmp_alt_timecode: s(o, "xmpAltTimecode"), userdata_timecode: s(o, "userdataTimecode") }
}
fn range_to_json(r: &InOutRange) -> Value { json!({ "inPoint": num_f64(r.in_point), "outPoint": num_f64(r.out_point) }) }
fn range_from_json(o: &Map<String, Value>) -> InOutRange { InOutRange { in_point: f(o, "inPoint", -1.0), out_point: f(o, "outPoint", -1.0) } }
fn seq_to_json(q: &ImageSequenceData, dir: &str) -> Value {
    json!({ "pattern": q.pattern, "ffmpegPattern": collapse_project_dir(&q.ffmpeg_pattern, dir), "directory": collapse_project_dir(&q.directory, dir), "frameCount": q.frame_count, "startFrame": q.start_frame, "endFrame": q.end_frame, "frameRate": num_f64(q.frame_rate), "duration": num_f64(q.duration),
        "width": q.width, "height": q.height, "format": q.format, "layer": q.layer, "availableLayers": q.available_layers, "compression": q.compression, "audioFile": collapse_project_dir(&q.audio_file, dir), "cacheStride": q.cache_stride })
}
fn seq_from_json(o: &Map<String, Value>, dir: &str) -> ImageSequenceData {
    ImageSequenceData { pattern: s(o, "pattern"), ffmpeg_pattern: expand_project_dir(&s(o, "ffmpegPattern"), dir), directory: expand_project_dir(&s(o, "directory"), dir), frame_count: i(o, "frameCount", 0) as i32, start_frame: i(o, "startFrame", 1) as i32, end_frame: i(o, "endFrame", 1) as i32, frame_rate: f(o, "frameRate", 23.976), duration: f(o, "duration", 0.0),
        width: i(o, "width", 0) as i32, height: i(o, "height", 0) as i32, format: s(o, "format"), layer: s(o, "layer"), available_layers: o.get("availableLayers").and_then(Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default(), compression: s(o, "compression"), audio_file: expand_project_dir(&s(o, "audioFile"), dir), missing_frames: vec![], cache_stride: i(o, "cacheStride", 1) as i32 }
}
fn playlist_to_json(p: &PlaylistData) -> Value {
    json!({ "masterFps": num_f64(p.master_fps), "canvasWidth": p.canvas_width, "canvasHeight": p.canvas_height, "defaultGapFrames": p.default_gap_frames, "loop": p.loop_, "items": p.items.iter().map(|e| json!({ "mediaId": e.media_id, "range": range_to_json(&e.range) })).collect::<Vec<_>>() })
}
fn playlist_from_json(o: &Map<String, Value>) -> PlaylistData {
    PlaylistData { master_fps: f(o, "masterFps", 24.0), canvas_width: i(o, "canvasWidth", 1920) as i32, canvas_height: i(o, "canvasHeight", 1080) as i32, default_gap_frames: i(o, "defaultGapFrames", 10) as i32, loop_: b(o, "loop"),
        items: o.get("items").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_object).map(|em| PlaylistEntry { media_id: s(em, "mediaId"), range: range_from_json(&obj(em, "range")) }).collect()).unwrap_or_default() }
}
fn dual_to_json(d: &DualPairData) -> Value {
    json!({ "mediaIdA": d.media_id_a, "mediaIdB": d.media_id_b, "masterFps": num_f64(d.master_fps), "masterDuration": num_f64(d.master_duration), "clipsA": d.clips_a, "clipsB": d.clips_b, "inPoint": d.in_point, "outPoint": d.out_point })
}
fn dual_from_json(o: &Map<String, Value>) -> DualPairData {
    DualPairData { media_id_a: s(o, "mediaIdA"), media_id_b: s(o, "mediaIdB"), master_fps: f(o, "masterFps", 24.0), master_duration: f(o, "masterDuration", 0.0), clips_a: o.get("clipsA").and_then(Value::as_array).cloned().unwrap_or_default(), clips_b: o.get("clipsB").and_then(Value::as_array).cloned().unwrap_or_default(), in_point: i(o, "inPoint", -1) as i32, out_point: i(o, "outPoint", -1) as i32 }
}
pub fn media_item_to_json(it: &MediaItem, dir: &str) -> Value {
    let mut o = Map::new();
    o.insert("id".into(), json!(it.id)); o.insert("name".into(), json!(it.name)); o.insert("path".into(), json!(collapse_project_dir(&it.path, dir))); o.insert("type".into(), json!(it.media_type.as_str()));
    o.insert("duration".into(), num_f64(it.duration)); o.insert("hasAudio".into(), json!(it.has_audio)); o.insert("thumbnailPath".into(), json!(collapse_project_dir(&it.thumbnail_path, dir)));
    o.insert("range".into(), range_to_json(&it.range)); o.insert("imageSeq".into(), seq_to_json(&it.image_seq, dir)); o.insert("playlist".into(), playlist_to_json(&it.playlist)); o.insert("video".into(), video_to_json(&it.video)); o.insert("adobe".into(), adobe_to_json(&it.adobe));
    o.insert("videoRangeOverride".into(), json!(it.video_range_override.as_str()));
    if it.transfer_override != TransferOverride::Auto { o.insert("transferOverride".into(), json!(it.transfer_override as i64)); }   // absent = Auto
    o.insert("pixelAspectMode".into(), json!(it.pixel_aspect_mode as i64)); o.insert("customParNum".into(), json!(it.custom_par_num)); o.insert("customParDen".into(), json!(it.custom_par_den));
    o.insert("rotationOverride".into(), json!(it.rotation_override)); o.insert("audioRoutingMode".into(), json!(it.audio_routing_mode as i64));
    o.insert("dualPair".into(), dual_to_json(&it.dual_pair));
    if !it.ocio_clip.is_empty() { o.insert("ocioClip".into(), Value::Object(it.ocio_clip.clone())); }
    if !it.min_color_clip.is_empty() { o.insert("minColorClip".into(), Value::Object(it.min_color_clip.clone())); }
    Value::Object(o)
}
pub fn media_item_from_json(o: &Map<String, Value>, dir: &str) -> MediaItem {
    let mut it = MediaItem { id: s(o, "id"), name: s(o, "name"), path: expand_project_dir(&s(o, "path"), dir), media_type: MediaType::parse(&s(o, "type")), duration: f(o, "duration", 0.0), has_audio: b(o, "hasAudio"), thumbnail_path: expand_project_dir(&s(o, "thumbnailPath"), dir),
        range: range_from_json(&obj(o, "range")), image_seq: seq_from_json(&obj(o, "imageSeq"), dir), playlist: playlist_from_json(&obj(o, "playlist")), video: video_from_json(&obj(o, "video")), adobe: adobe_from_json(&obj(o, "adobe")),
        video_range_override: VideoRange::parse(&s(o, "videoRangeOverride")), transfer_override: TransferOverride::from_i64(i(o, "transferOverride", 0)), pixel_aspect_mode: PixelAspectMode::from_i64(i(o, "pixelAspectMode", 0)),
        custom_par_num: i(o, "customParNum", 1) as i32, custom_par_den: i(o, "customParDen", 1) as i32, rotation_override: i(o, "rotationOverride", -1) as i32, audio_routing_mode: AudioRoutingMode::from_i64(i(o, "audioRoutingMode", 0)),
        ocio_clip: obj(o, "ocioClip"), min_color_clip: obj(o, "minColorClip"), dual_pair: dual_from_json(&obj(o, "dualPair")), load_failed: false };
    if it.custom_par_num <= 0 || it.custom_par_den <= 0 { it.custom_par_num = 1; it.custom_par_den = 1; }
    if ![0, 90, 180, 270].contains(&it.rotation_override) { it.rotation_override = -1; }
    it
}
fn bin_to_json(bn: &ProjectBin) -> Value { json!({ "name": bn.name, "type": bn.accepts.as_str(), "open": bn.is_open, "item_ids": bn.item_ids }) }

/// The whole document as a JSON value (the writer then formats it Qt's way). `project_dir` = the folder the file lives in.
pub fn to_value(p: &ProjectFile, project_dir: &str, modified_at: &str) -> Value {
    json!({ "schema": SCHEMA_TAG, "version": SCHEMA_VERSION,
        "project": { "uuid": p.uuid, "name": p.name, "created_at": p.created_at, "modified_at": modified_at },
        "media": p.media.iter().map(|it| media_item_to_json(it, project_dir)).collect::<Vec<_>>(),
        "bins": p.bins.iter().map(bin_to_json).collect::<Vec<_>>(),
        "ui_state": { "active_item_id": p.active_item_id, "b_source_media_id": p.b_source_media_id } })
}
pub fn project_dir_of(file_path: &str) -> String { Path::new(file_path).parent().map(|d| d.to_string_lossy().replace('\\', "/")).unwrap_or_default() }

/// Save atomically (a sibling temp file renamed over the old one — QSaveFile's commit). `modified_at` is stamped by the caller.
pub fn save(p: &ProjectFile, file_path: &str, modified_at: &str) -> Result<(), String> {
    if file_path.is_empty() { return Err("Save: empty path.".into()) }
    let text = to_qt_json(&to_value(p, &project_dir_of(file_path), modified_at));
    let path = Path::new(file_path);
    let tmp = path.with_extension(format!("qcvproj.{}.tmp", std::process::id()));
    std::fs::write(&tmp, text.as_bytes()).map_err(|e| format!("Save: cannot write '{file_path}': {e}"))?;
    std::fs::rename(&tmp, path).map_err(|e| { let _ = std::fs::remove_file(&tmp); format!("Save: commit to '{file_path}' failed: {e}") })?;
    Ok(())
}
/// Parse a document; the migrations: no bins → the six, pre-2.2.3 → the Live bin appended, dangling ui ids cleared.
pub fn parse(text: &str, project_dir: &str, file_path: &str) -> Result<ProjectFile, String> {
    let root: Value = serde_json::from_str(text).map_err(|e| format!("Open: parse error in '{file_path}': {e}"))?;
    let root = root.as_object().ok_or_else(|| format!("Open: parse error in '{file_path}': not an object"))?;
    let schema = s(root, "schema");
    if schema != SCHEMA_TAG { return Err(format!("Open: '{file_path}' is not a v2 .qcvproj (schema='{schema}')")) }
    let project = obj(root, "project");
    let media: Vec<MediaItem> = root.get("media").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_object).map(|o| media_item_from_json(o, project_dir)).collect()).unwrap_or_default();
    let mut bins: Vec<ProjectBin> = root.get("bins").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_object).map(|bo| ProjectBin { name: s(bo, "name"), accepts: MediaType::parse(&s(bo, "type")), is_open: b(bo, "open"), item_ids: bo.get("item_ids").and_then(Value::as_array).map(|ids| ids.iter().filter_map(|v| v.as_str()).filter(|id| !id.is_empty()).map(str::to_string).collect()).unwrap_or_default() }).collect()).unwrap_or_default();
    if bins.is_empty() { log::warn!("ProjectIo::load: no bins in file; seeding canonical defaults"); bins = seed_bins(); }
    if !bins.iter().any(|bn| bn.accepts == MediaType::LiveStream) { bins.push(ProjectBin { name: "Live".into(), accepts: MediaType::LiveStream, item_ids: vec![], is_open: false }); log::info!("ProjectIo::load: pre-2.2.3 project — appended Live bin shell"); }
    let ui = obj(root, "ui_state");
    let active = s(&ui, "active_item_id"); let bsrc = s(&ui, "b_source_media_id");
    let active = if !active.is_empty() && media.iter().any(|m| m.id == active) { active } else { String::new() };
    let bsrc = if !bsrc.is_empty() && media.iter().any(|m| m.id == bsrc) { bsrc } else { String::new() };
    Ok(ProjectFile { uuid: s(&project, "uuid"), name: s(&project, "name"), created_at: s(&project, "created_at"), modified_at: s(&project, "modified_at"), media, bins, active_item_id: active, b_source_media_id: bsrc })
}
pub fn load(file_path: &str) -> Result<ProjectFile, String> {
    let text = std::fs::read_to_string(file_path).map_err(|e| format!("Open: cannot read '{file_path}': {e}"))?;
    parse(&text, &project_dir_of(file_path), file_path)
}
