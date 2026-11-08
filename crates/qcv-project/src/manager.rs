//! QCView's `ProjectManager` as a plain struct (no signals: the caller compares `revision()`): the media pool, the six
//! bins, the selection, the file identity, QCView's add rules (the extension gate, dedupe by absolute path / by folder +
//! pattern / by exact URL, a still as a 1-frame sequence with `%` doubled, type routing to the bin that accepts it),
//! batch remove with one-step restore in exact positions, the per-clip override setters, the playlist naming rules, and
//! the header-only re-probe schedule on open (rotation −1 / empty container / old ProRes RAW caches).
use crate::{extensions, io::{self, ProjectFile}, model::*};
use std::path::Path;

/// What the loader asks the metadata service to run after an open (QCView: `applyLoadedState`'s migration probes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Probe { Rotation(String, String), Container(String, String), FullVideo(String, String), Adobe(String, String) }

struct RemovedItemSnapshot { item: MediaItem, pool_index: usize, bin_positions: Vec<(usize, usize)> }

pub struct ProjectManager {
    pool: Vec<MediaItem>, bins: Vec<ProjectBin>,
    active_item_id: String, b_source_media_id: String,
    uuid: String, file_path: String, name: String, created_at: String, modified_at: String, dirty: bool,
    last_removal: Vec<RemovedItemSnapshot>,
    revision: u64,
    /// files the user reached for (adds and dedupe hits both count — QCView's Recents feed)
    pub recent: Vec<String>,
}

fn new_uuid() -> String { uuid::Uuid::new_v4().to_string() }
/// Qt::ISODate in UTC: `2026-11-07T18:30:00Z`.
pub fn now_iso_utc() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    let (h, m, sec) = (rem / 3600, rem % 3600 / 60, rem % 60);
    // civil from days (Howard Hinnant)
    let z = days + 719468; let era = z.div_euclid(146097); let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; let y = yoe + era * 400; let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1; let mo = if mp < 10 { mp + 3 } else { mp - 9 }; let y = if mo <= 2 { y + 1 } else { y };
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{sec:02}Z")
}
/// QDateTime::currentDateTime().toString("yyyyMMdd-HHmmss") — local time; the offset is the caller's (the shell knows the zone).
pub fn stamp_local(utc_offset_secs: i64) -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0) + utc_offset_secs;
    let iso = { let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400)); let z = days + 719468; let era = z.div_euclid(146097); let doe = z.rem_euclid(146097); let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; let y = yoe + era * 400; let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); let mp = (5 * doy + 2) / 153; let d = doy - (153 * mp + 2) / 5 + 1; let mo = if mp < 10 { mp + 3 } else { mp - 9 }; let y = if mo <= 2 { y + 1 } else { y }; format!("{y:04}{mo:02}{d:02}-{:02}{:02}{:02}", rem / 3600, rem % 3600 / 60, rem % 60) };
    iso
}

impl Default for ProjectManager { fn default() -> Self { Self::new() } }

impl ProjectManager {
    pub fn new() -> ProjectManager {
        let now = now_iso_utc();
        ProjectManager { pool: vec![], bins: seed_bins(), active_item_id: String::new(), b_source_media_id: String::new(), uuid: new_uuid(), file_path: String::new(), name: "Untitled".into(), created_at: now.clone(), modified_at: now, dirty: false, last_removal: vec![], revision: 0, recent: vec![] }
    }
    // ---- reads ----
    pub fn pool(&self) -> &[MediaItem] { &self.pool }
    pub fn bins(&self) -> &[ProjectBin] { &self.bins }
    pub fn item(&self, id: &str) -> Option<&MediaItem> { self.pool.iter().find(|m| m.id == id) }
    pub fn item_mut(&mut self, id: &str) -> Option<&mut MediaItem> { self.pool.iter_mut().find(|m| m.id == id) }
    pub fn active_item_id(&self) -> &str { &self.active_item_id }
    pub fn active_item(&self) -> Option<&MediaItem> { self.item(&self.active_item_id.clone()) }
    pub fn b_source_media_id(&self) -> &str { &self.b_source_media_id }
    pub fn uuid(&self) -> &str { &self.uuid }
    pub fn file_path(&self) -> &str { &self.file_path }
    pub fn name(&self) -> &str { &self.name }
    pub fn created_at(&self) -> &str { &self.created_at }
    pub fn modified_at(&self) -> &str { &self.modified_at }
    pub fn dirty(&self) -> bool { self.dirty }
    /// Bumps on every change (the UI compares it instead of listening to signals).
    pub fn revision(&self) -> u64 { self.revision }
    fn mark_dirty(&mut self) { self.dirty = true; self.revision += 1; }
    fn bump(&mut self) { self.revision += 1; }

    // ---- the file ----
    /// QCView `newProject`: the pool and selection go, the bin shells stay (only their ids reset); a fresh uuid and dates.
    pub fn new_project(&mut self) {
        self.pool.clear(); self.last_removal.clear(); for bn in &mut self.bins { bn.item_ids.clear(); }
        self.active_item_id.clear(); self.b_source_media_id.clear();
        self.uuid = new_uuid(); self.file_path.clear(); self.name = "Untitled".into(); self.created_at = now_iso_utc(); self.modified_at = self.created_at.clone();
        self.dirty = false; self.bump();
    }
    pub fn save_project(&mut self, path: &str) -> Result<(), String> {
        let stamp = now_iso_utc();
        io::save(&self.snapshot(), path, &stamp)?;
        self.file_path = path.to_string(); self.modified_at = stamp; self.dirty = false; self.bump();
        Ok(())
    }
    /// Open; on failure the manager is left as a clean new project (QCView's contract). Returns the probes to schedule.
    pub fn open_project(&mut self, path: &str) -> Result<Vec<Probe>, String> {
        match io::load(path) {
            Ok(file) => { let probes = self.apply_loaded(file); self.file_path = path.to_string(); self.bump(); Ok(probes) }
            Err(e) => { self.new_project(); Err(e) }
        }
    }
    fn snapshot(&self) -> ProjectFile { ProjectFile { uuid: self.uuid.clone(), name: self.name.clone(), created_at: self.created_at.clone(), modified_at: self.modified_at.clone(), media: self.pool.clone(), bins: self.bins.clone(), active_item_id: self.active_item_id.clone(), b_source_media_id: self.b_source_media_id.clone() } }
    fn apply_loaded(&mut self, f: ProjectFile) -> Vec<Probe> {
        self.pool = f.media; self.bins = f.bins; self.active_item_id = f.active_item_id; self.b_source_media_id = f.b_source_media_id; self.last_removal.clear();
        self.uuid = if f.uuid.is_empty() { new_uuid() } else { f.uuid }; self.name = if f.name.is_empty() { "Untitled".into() } else { f.name };
        self.created_at = if f.created_at.is_empty() { now_iso_utc() } else { f.created_at }; self.modified_at = if f.modified_at.is_empty() { self.created_at.clone() } else { f.modified_at };
        self.dirty = false;
        if !self.active_item_id.is_empty() && self.item(&self.active_item_id.clone()).is_none() { log::warn!("ProjectManager: saved active id missing from pool; clearing"); self.active_item_id.clear(); }
        // the migration probes: header-only, never a full re-extract that could clobber good cache on an offline volume — except the old ProRes RAW caches
        let mut probes = vec![];
        for it in &self.pool {
            if it.media_type != MediaType::Video || !it.video.loaded { continue }
            if it.video.rotation_deg < 0 { probes.push(Probe::Rotation(it.id.clone(), it.path.clone())); }
            if it.video.container_format.is_empty() { probes.push(Probe::Container(it.id.clone(), it.path.clone())); }
            if it.video.video_codec == "prores_raw" && it.video.pixel_format.is_empty() { probes.push(Probe::FullVideo(it.id.clone(), it.path.clone())); }
        }
        probes
    }

    // ---- adds ----
    fn route(&mut self, id: &str, t: MediaType) { let t = if t == MediaType::ImageSequence { MediaType::Image } else { t }; if let Some(bn) = self.bins.iter_mut().find(|bn| bn.accepts == t) { bn.item_ids.push(id.to_string()); } }
    fn note_recent(&mut self, p: &str) { self.recent.retain(|r| r != p); self.recent.insert(0, p.to_string()); }
    /// QCView `addMediaFile`: the extension gate, Recents bump (also on a dedupe hit), images through the sequence
    /// detector, dedupe by absolute path, the default name = the file name, routed to its bin. Returns the item id
    /// (existing on a dedupe hit) and the probes to schedule.
    pub fn add_media_file(&mut self, raw_path: &str) -> (Option<String>, Vec<Probe>) {
        let p = Path::new(raw_path);
        if raw_path.is_empty() || !p.exists() { log::warn!("ProjectManager: addMediaFile — '{raw_path}' does not exist"); return (None, vec![]) }
        if !extensions::is_supported_media_file(raw_path) { log::warn!("ProjectManager: addMediaFile — '{raw_path}' refused ({})", if p.is_dir() { "a folder" } else { "not a media file type" }); return (None, vec![]) }
        let abs = std::fs::canonicalize(p).map(|a| a.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| raw_path.replace('\\', "/"));
        self.note_recent(&abs);
        if extensions::kind_for_path(&abs) == extensions::Kind::Image {
            if let Some(seq) = min_paths::sequence::detect_on_disk(Path::new(&abs)) {
                if let Some(e) = self.pool.iter().find(|e| e.media_type == MediaType::ImageSequence && e.image_seq.directory == seq.directory && e.image_seq.pattern == seq.pattern) { return (Some(e.id.clone()), vec![]) }
                return (Some(self.add_sequence(&abs, seq, 24.0)), vec![])
            }
            return (Some(self.add_single_image_as_sequence(&abs, 24.0)), vec![])
        }
        if let Some(e) = self.pool.iter().find(|e| e.path == abs) {
            // a re-add is the moment to backfill a pre-field cache (header probes only)
            let mut probes = vec![];
            if e.media_type == MediaType::Video && e.video.loaded && e.video.container_format.is_empty() { probes.push(Probe::Container(e.id.clone(), e.path.clone())); }
            if e.media_type == MediaType::Video && e.video.loaded && e.video.video_codec == "prores_raw" && e.video.pixel_format.is_empty() { probes.push(Probe::FullVideo(e.id.clone(), e.path.clone())); }
            return (Some(e.id.clone()), probes)
        }
        let t = match extensions::kind_for_path(&abs) { extensions::Kind::Audio => MediaType::Audio, extensions::Kind::Image => MediaType::Image, _ => MediaType::Video };
        let item = MediaItem { id: new_uuid(), name: Path::new(&abs).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), path: abs.clone(), media_type: t, ..Default::default() };
        let id = item.id.clone();
        self.pool.push(item); self.route(&id, t); self.mark_dirty();
        let probes = if matches!(t, MediaType::Video | MediaType::Audio) { vec![Probe::FullVideo(id.clone(), abs.clone()), Probe::Adobe(id.clone(), abs)] } else { vec![] };
        (Some(id), probes)
    }
    fn add_sequence(&mut self, abs_first: &str, seq: min_paths::sequence::Sequence, fps: f64) -> String {
        let fps = if fps > 0.0 { fps } else { 24.0 };
        let frame_count = seq.end - seq.start + 1;
        let mut item = MediaItem { id: new_uuid(), name: format!("{} [{:0w$}-{:0w$}]", seq.base_label, seq.start, seq.end, w = seq.padding), path: seq.frame_path(seq.start), media_type: MediaType::ImageSequence, duration: frame_count as f64 / fps, has_audio: false, ..Default::default() };
        let _ = abs_first;
        item.image_seq = ImageSequenceData { directory: seq.directory.clone(), pattern: seq.pattern.clone(), frame_count, start_frame: seq.start, end_frame: seq.end, frame_rate: fps, duration: item.duration, format: seq.ext.to_uppercase(), missing_frames: seq.missing.clone(), ..Default::default() };
        let id = item.id.clone();
        self.pool.push(item); self.route(&id, MediaType::ImageSequence); self.mark_dirty();
        id
    }
    /// A still = a 1-frame ImageSequence (it rides the native loaders); the literal name is the pattern with `%` doubled.
    pub fn add_single_image_as_sequence(&mut self, abs_path: &str, fps: f64) -> String {
        let p = Path::new(abs_path);
        let dir = p.parent().map(|d| d.to_string_lossy().replace('\\', "/")).unwrap_or_default();
        let pattern = p.file_name().map(|n| n.to_string_lossy().replace('%', "%%")).unwrap_or_default();
        if let Some(e) = self.pool.iter().find(|e| e.media_type == MediaType::ImageSequence && e.image_seq.directory == dir && e.image_seq.pattern == pattern) { return e.id.clone() }
        let fps = if fps > 0.0 { fps } else { 24.0 };
        let mut item = MediaItem { id: new_uuid(), name: p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), path: abs_path.to_string(), media_type: MediaType::ImageSequence, duration: 1.0 / fps, has_audio: false, ..Default::default() };
        item.image_seq = ImageSequenceData { directory: dir, pattern, frame_count: 1, start_frame: 0, end_frame: 0, frame_rate: fps, duration: item.duration, format: p.extension().map(|e| e.to_string_lossy().to_uppercase()).unwrap_or_default(), ..Default::default() };
        let id = item.id.clone();
        self.pool.push(item); self.route(&id, MediaType::ImageSequence); self.mark_dirty();
        id
    }
    /// QCView `addLiveStream`: dedupe by exact URL; the name = the given one, else host:port, else the URL.
    pub fn add_live_stream(&mut self, url: &str, name: &str) -> Option<String> {
        let trimmed = url.trim();
        if !trimmed.contains("://") { log::warn!("ProjectManager::addLiveStream: not a URL: '{trimmed}'"); return None }
        self.note_recent(trimmed);
        if let Some(e) = self.pool.iter().find(|e| e.media_type == MediaType::LiveStream && e.path == trimmed) { return Some(e.id.clone()) }
        let display = if !name.trim().is_empty() { name.trim().to_string() } else { let rest = &trimmed[trimmed.find("://").unwrap() + 3..]; let authority = rest.split('/').next().unwrap_or(""); if authority.is_empty() { trimmed.to_string() } else { authority.to_string() } };
        let item = MediaItem { id: new_uuid(), name: display, path: trimmed.to_string(), media_type: MediaType::LiveStream, ..Default::default() };
        let id = item.id.clone();
        self.pool.push(item); self.route(&id, MediaType::LiveStream); self.mark_dirty();
        Some(id)
    }
    /// QCView `createPlaylist`: the clips land in the standard bins through `add_media_file` (the playlist references, never
    /// owns); audio-only files are skipped; an empty name → `Playlist-yyyyMMdd-HHmmss` (local time).
    pub fn create_playlist(&mut self, paths: &[String], name: &str, fps: f64, canvas_w: i32, canvas_h: i32, gap_frames: i32, utc_offset_secs: i64) -> (String, Vec<Probe>) {
        let mut item = MediaItem { id: new_uuid(), media_type: MediaType::Playlist, name: if name.is_empty() { format!("Playlist-{}", stamp_local(utc_offset_secs)) } else { name.to_string() }, ..Default::default() };
        item.playlist = PlaylistData { master_fps: if fps > 0.0 { fps } else { 24.0 }, canvas_width: if canvas_w > 0 { canvas_w } else { 1920 }, canvas_height: if canvas_h > 0 { canvas_h } else { 1080 }, default_gap_frames: if gap_frames >= 0 { gap_frames } else { 10 }, loop_: false, items: vec![] };
        let mut probes = vec![];
        for p in paths {
            let (id, pr) = self.add_media_file(p); probes.extend(pr);
            let Some(id) = id else { continue };
            if self.item(&id).is_some_and(|m| m.media_type == MediaType::Audio) { log::warn!("createPlaylist: audio media not allowed in playlists — skipping '{p}'"); continue }
            item.playlist.items.push(PlaylistEntry { media_id: id, range: InOutRange::default() });
        }
        let id = item.id.clone();
        self.pool.push(item); self.route(&id, MediaType::Playlist); self.mark_dirty();
        (id, probes)
    }
    /// QCView `renameMediaItem`: empty names and unknown ids are refused; the same name is a no-op that still succeeds.
    pub fn rename_media_item(&mut self, id: &str, new_name: &str) -> bool {
        if id.is_empty() || new_name.is_empty() { return false }
        let Some(m) = self.item_mut(id) else { return false };
        if m.name == new_name { return true }
        m.name = new_name.to_string(); self.mark_dirty(); true
    }
    /// `Playlist_NN`: one past the highest existing number, two digits.
    pub fn next_playlist_auto_name(&self) -> String {
        let max = self.pool.iter().filter(|m| m.media_type == MediaType::Playlist).filter_map(|m| m.name.strip_prefix("Playlist_").and_then(|n| n.parse::<i32>().ok())).max().unwrap_or(0);
        format!("Playlist_{:02}", max + 1)
    }
    pub fn add_dual_pair(&mut self, name: &str, data: DualPairData) -> String {
        let item = MediaItem { id: new_uuid(), name: if name.is_empty() { "Untitled Dual View".into() } else { name.to_string() }, media_type: MediaType::DualPair, dual_pair: data, ..Default::default() };
        let id = item.id.clone();
        self.pool.push(item); self.route(&id, MediaType::DualPair); self.mark_dirty();
        id
    }

    // ---- remove / restore ----
    /// Batch remove with a one-step restore (`restore_last_removed`): each snapshot's indices are valid in the state
    /// just before its own removal. Returns how many went.
    pub fn remove_media_items(&mut self, ids: &[String]) -> usize {
        let mut batch = vec![];
        for id in ids {
            let Some(idx) = self.pool.iter().position(|m| &m.id == id) else { continue };
            let bin_positions = self.bins.iter().enumerate().filter_map(|(b, bn)| bn.item_ids.iter().position(|x| x == id).map(|pos| (b, pos))).collect();
            let item = self.pool.remove(idx);
            for bn in &mut self.bins { bn.item_ids.retain(|x| x != id); }
            if self.active_item_id == *id { self.active_item_id.clear(); }
            if self.b_source_media_id == *id { self.b_source_media_id.clear(); }
            batch.push(RemovedItemSnapshot { item, pool_index: idx, bin_positions });
        }
        if batch.is_empty() { return 0 }
        let n = batch.len(); self.last_removal = batch; self.mark_dirty(); n
    }
    pub fn remove_media_item(&mut self, id: &str) -> bool { self.remove_media_items(&[id.to_string()]) > 0 }
    /// Unwinds the last batch back to front so every item lands exactly where it was. Returns the restored names.
    pub fn restore_last_removed(&mut self) -> Vec<String> {
        if self.last_removal.is_empty() { return vec![] }
        let batch = std::mem::take(&mut self.last_removal);
        let mut names = vec![];
        for snap in batch.into_iter().rev() {
            if self.pool.iter().any(|m| m.id == snap.item.id) { continue }
            let pos = snap.pool_index.min(self.pool.len());
            names.insert(0, snap.item.name.clone());
            let id = snap.item.id.clone();
            self.pool.insert(pos, snap.item);
            for (b, p) in snap.bin_positions { if let Some(bn) = self.bins.get_mut(b) { let p = p.min(bn.item_ids.len()); bn.item_ids.insert(p, id.clone()); } }
        }
        if !names.is_empty() { self.mark_dirty(); }
        names
    }

    // ---- selection + per-clip settings ----
    pub fn set_active_item(&mut self, id: &str) -> bool { if id == self.active_item_id { return false } if self.item(id).is_none() { log::warn!("ProjectManager: setActiveItem — unknown id '{id}'"); return false } self.active_item_id = id.to_string(); if !id.is_empty() { let p = self.item(id).map(|m| m.path.clone()).unwrap_or_default(); if !p.is_empty() { self.note_recent(&p); } } self.bump(); true }
    pub fn set_b_source_media_id(&mut self, id: &str) -> bool { if id == self.b_source_media_id { return false } if !id.is_empty() && self.item(id).is_none() { return false } self.b_source_media_id = id.to_string(); self.mark_dirty(); true }
    pub fn set_bin_open(&mut self, bin: usize, open: bool) { if let Some(bn) = self.bins.get_mut(bin) { if bn.is_open != open { bn.is_open = open; self.mark_dirty(); } } }
    fn edit(&mut self, id: &str, f: impl FnOnce(&mut MediaItem) -> bool) -> bool { let changed = self.item_mut(id).map(f).unwrap_or(false); if changed { self.mark_dirty(); } changed }
    pub fn set_video_range_override(&mut self, id: &str, r: VideoRange) -> bool { self.edit(id, |m| if m.video_range_override != r { m.video_range_override = r; true } else { false }) }
    pub fn set_transfer_override(&mut self, id: &str, t: TransferOverride) -> bool { self.edit(id, |m| if m.transfer_override != t { m.transfer_override = t; true } else { false }) }
    /// QCView `setPixelAspect(item, mode, num, den)`: Custom keeps the given ratio; the others keep the stored custom ratio.
    pub fn set_pixel_aspect(&mut self, id: &str, mode: PixelAspectMode, num: i32, den: i32) -> bool { self.edit(id, |m| { let (n, d) = if mode == PixelAspectMode::Custom && num > 0 && den > 0 { (num, den) } else { (m.custom_par_num, m.custom_par_den) }; if m.pixel_aspect_mode != mode || m.custom_par_num != n || m.custom_par_den != d { m.pixel_aspect_mode = mode; m.custom_par_num = n; m.custom_par_den = d; true } else { false } }) }
    pub fn set_rotation_override(&mut self, id: &str, deg: i32) -> bool { let deg = if [0, 90, 180, 270].contains(&deg) { deg } else { -1 }; self.edit(id, |m| if m.rotation_override != deg { m.rotation_override = deg; true } else { false }) }
    pub fn set_audio_routing_mode(&mut self, id: &str, mode: AudioRoutingMode) -> bool { self.edit(id, |m| if m.audio_routing_mode != mode { m.audio_routing_mode = mode; true } else { false }) }
    pub fn set_in_out(&mut self, id: &str, r: InOutRange) -> bool { self.edit(id, |m| if m.range != r { m.range = r; true } else { false }) }
    pub fn set_load_failed(&mut self, id: &str, failed: bool) { if let Some(m) = self.item_mut(id) { if m.load_failed != failed { m.load_failed = failed; self.bump(); } } }
    pub fn set_duration_and_audio(&mut self, id: &str, duration: f64, has_audio: bool) -> bool { self.edit(id, |m| if m.duration != duration || m.has_audio != has_audio { m.duration = duration; m.has_audio = has_audio; true } else { false }) }

    // ---- the metadata service's results ----
    /// A full extraction landed: the whole struct (QCView `onMetadataReady`).
    pub fn on_video_metadata(&mut self, id: &str, v: VideoMetadata) { let changed = self.item_mut(id).map(|m| { m.video = v; true }).unwrap_or(false); if changed { self.mark_dirty(); } }
    pub fn on_adobe_metadata(&mut self, id: &str, a: AdobeMetadata) { let changed = self.item_mut(id).map(|m| { m.adobe = a; true }).unwrap_or(false); if changed { self.mark_dirty(); } }
    /// A header-only rotation probe: patches the one field; deliberately NOT dirty (a migration on open must not read as an edit).
    pub fn on_rotation_probed(&mut self, id: &str, deg: i32) { if deg < 0 { return } if let Some(m) = self.item_mut(id) { if m.video.rotation_deg != deg { m.video.rotation_deg = deg; self.bump(); } } }
    /// A header-only container probe: only the probe's fields, never the full struct, so cached stream / colour / audio data survives.
    pub fn on_container_probed(&mut self, id: &str, probe: &VideoMetadata) {
        if !probe.loaded { return }
        let Some(m) = self.item_mut(id) else { return };
        let mut changed = false;
        let mut patch = |dst: &mut String, src: &str| { if !src.is_empty() && dst != src { *dst = src.to_string(); changed = true; } };
        patch(&mut m.video.container_format, &probe.container_format); patch(&mut m.video.mxf_operational_pattern, &probe.mxf_operational_pattern); patch(&mut m.video.encoder_tool, &probe.encoder_tool); patch(&mut m.video.codec_profile, &probe.codec_profile);
        if probe.container_bitrate > 0 && m.video.container_bitrate != probe.container_bitrate { m.video.container_bitrate = probe.container_bitrate; changed = true; }
        if probe.video_bitrate > 0 && m.video.video_bitrate != probe.video_bitrate { m.video.video_bitrate = probe.video_bitrate; changed = true; }
        if changed { self.bump(); }
    }
}
