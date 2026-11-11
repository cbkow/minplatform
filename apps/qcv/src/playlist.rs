//! A project playlist ↔ the timeline the transport plays (QCView's `startPlaylist` + `TimelineController::loadPlaylist`, and
//! the write-back its edits make). The project stores entries as (media id, in / out in SOURCE SECONDS, -1 = untrimmed); the
//! timeline works in frames on the master rate. Entries whose media has left the pool are skipped (QCView logs them).
use qcv_project::{InOutRange as PRange, MediaItem, MediaType, PlaylistEntry, ProjectManager};
use qcv_timeline::{ClipMediaKind, Editor, InOutRange, MediaInfo, Rate, Timeline};

/// The source facts a clip needs: the project's metadata when the service has delivered it, else a synchronous probe (QCView
/// reflows on `binsChanged` instead; a probe at load keeps the first build right).
pub fn media_info(m: &MediaItem) -> MediaInfo {
    let mut probed_audio = false;
    let (kind, mut fps, mut frames, mut w, mut h) = match m.media_type {
        MediaType::ImageSequence => (ClipMediaKind::ImageSequence, m.image_seq.frame_rate, m.image_seq.frame_count as i64, m.image_seq.width, m.image_seq.height),
        MediaType::Audio => (ClipMediaKind::Audio, 0.0, 0, 0, 0),
        _ => (ClipMediaKind::Video, m.video.frame_rate, m.video.total_frames as i64, m.video.width, m.video.height),
    };
    if kind == ClipMediaKind::Video && (fps <= 0.0 || frames <= 0) {
        let v = qcv_meta::extract(&m.path);
        if v.loaded { fps = v.frame_rate; frames = v.total_frames as i64; w = v.width; h = v.height; probed_audio = v.audio_channels > 0; }   // the item's `has_audio` is not known yet either (the playbar's waveform reads the clip's)
        else { log::warn!("playlist: no metadata for '{}' — the clip is empty until it arrives", m.name); }
    }
    let rate = if fps > 0.0 { Rate::from_f64(fps) } else { Rate::new(24, 1) };
    // an audio clip (canon row 278): its duration at the audio mode's 24 fps — the item's, or a probe before the metadata arrives
    let secs = if kind == ClipMediaKind::Audio && m.duration <= 0.0 { qcv_meta::extract(&m.path).duration } else { m.duration };
    if frames <= 0 && secs > 0.0 { frames = (secs * rate.as_f64()).round() as i64; }
    MediaInfo { id: m.id.clone(), name: m.name.clone(), path: m.path.clone(), kind, rate, duration: frames.max(0), width: w.max(0) as u32, height: h.max(0) as u32, has_audio: m.has_audio || m.video.audio_channels > 0 || probed_audio || kind == ClipMediaKind::Audio,   // the item's own flag is set only once it has been opened; the extractor's channel count is known from the add
        sequence_pattern: if m.media_type == MediaType::ImageSequence { std::path::Path::new(&m.image_seq.directory).join(&m.image_seq.pattern).to_string_lossy().into_owned() } else { String::new() },   // QCView opens a sequence clip from its item's directory + pattern (`startImageSequence`)
        sequence_start_frame: m.image_seq.start_frame as i64, sequence_end_frame: m.image_seq.end_frame as i64,
        // the item's EXR layer and gaps on disk travel with the clip (QCView's `startImageSequence` reads them off the item)
        sequence_exr_layer: if m.media_type == MediaType::ImageSequence { m.image_seq.layer.clone() } else { String::new() }, sequence_missing: if m.media_type == MediaType::ImageSequence { m.image_seq.missing_frames.clone() } else { vec![] } }
}

/// QCView's trim: -1 = untrimmed; seconds → source frames on the clip's own rate.
fn trim_frames(r: PRange, rate: Rate) -> InOutRange {
    let f = |s: f64| (s >= 0.0).then(|| (s * rate.as_f64()).round() as i64);
    InOutRange { in_point: f(r.in_point), out_point: f(r.out_point) }
}

/// Build the editor for playlist `item` (QCView `startPlaylist`): the entries resolved against the pool, in order.
pub fn editor_for(pm: &ProjectManager, item: &MediaItem) -> Editor {
    let mut infos = vec![]; let mut trims = vec![]; let mut missing = 0;
    for e in &item.playlist.items {
        let Some(m) = pm.item(&e.media_id) else { missing += 1; continue };
        let info = media_info(m); trims.push(trim_frames(e.range, info.rate)); infos.push(info);
    }
    if missing > 0 { log::warn!("playlist '{}': {missing} entries missing from the pool (likely removed)", item.name); }
    let pl = &item.playlist;
    let mut ed = Editor::default();
    ed.load_playlist(&item.name, &infos, &trims, Rate::from_f64(if pl.master_fps > 0.0 { pl.master_fps } else { 24.0 }), pl.default_gap_frames.max(0) as i64, (pl.canvas_width.max(0) as u32, pl.canvas_height.max(0) as u32), &item.id);
    ed
}

/// The write-back after an edit: the timeline's non-gap clips in order, trims back to source seconds (-1 when untrimmed).
pub fn entries_from(t: &Timeline) -> Vec<PlaylistEntry> {
    let Some(track) = t.tracks.first() else { return vec![] };
    track.clips.iter().filter(|c| !c.is_gap && !c.media_item_id.is_empty()).map(|c| {
        let r = c.source_rate.as_f64().max(1e-9);
        let untrimmed = c.source_in <= 0 && (c.source_duration <= 0 || c.source_out >= c.source_duration);
        PlaylistEntry { media_id: c.media_item_id.clone(), range: if untrimmed { PRange::default() } else { PRange { in_point: c.source_in as f64 / r, out_point: c.source_out as f64 / r } } }
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trims_round_trip_through_seconds_and_untrimmed_stays_minus_one() {
        let rate = Rate::new(24, 1);
        let t = trim_frames(PRange { in_point: 1.0, out_point: 2.5 }, rate);
        assert_eq!((t.in_point, t.out_point), (Some(24), Some(60)));
        assert_eq!(trim_frames(PRange::default(), rate).in_point, None);
    }
    #[test]
    fn a_project_playlist_becomes_a_gapless_timeline_and_writes_back_unchanged() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        let (a, b) = (root.join("prores_1080.mov"), root.join("prores4444_1080.mov"));
        if !a.exists() || !b.exists() { eprintln!("fixtures absent — skipped"); return }
        let mut pm = ProjectManager::new();
        let (id, _) = pm.create_playlist(&[a.to_string_lossy().into(), b.to_string_lossy().into()], "Playlist_01", 24.0, 1920, 1080, 0, 0);
        // trim the second entry to 1.0 s .. 2.0 s (QCView stores seconds)
        let mut items = pm.item(&id).unwrap().playlist.items.clone(); items[1].range = PRange { in_point: 1.0, out_point: 2.0 };
        pm.set_playlist_items(&id, items.clone());
        let ed = editor_for(&pm, pm.item(&id).unwrap());
        let clips: Vec<_> = ed.timeline.tracks[0].clips.iter().filter(|c| !c.is_gap).collect();
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[1].start, clips[0].duration, "gapless at gap 0");
        assert!(clips[0].duration > 0 && clips[1].duration > 20 && clips[1].duration < 30, "the trim lands as ~1 s of frames: {}", clips[1].duration);
        let back = entries_from(&ed.timeline);
        assert_eq!(back[0].range, PRange::default(), "untrimmed stays -1");
        assert!((back[1].range.in_point - 1.0).abs() < 0.05 && (back[1].range.out_point - 2.0).abs() < 0.05, "{:?}", back[1].range);
    }
}
