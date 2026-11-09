//! A live host feed in the single view (canon rows 227–228, step 1a): QCView's Connect to After Effects / Premiere Pro, the
//! feed on the render tick, and the rule that ends it — the feed lives exactly as long as its item is the loaded one (every
//! way out — another item, a file or project opened, New Project, dual view, the item removed — changes the loaded item).
use crate::{transfer_choice, trw};
use qcv_player::host_feed::HostFeed;
use std::time::Instant;
use qcv_ring::url::Host;

/// QCView's LIVE / REVIEW (plan B2; canon row 228 1b): the feed frozen on the viewport. `stamp` = the note key minted at the
/// freeze (`yyyyMMdd-HHmmss-zzz`, local) that the first note takes; `note` = the stream note shown (None = a transient freeze).
/// `captured_at` / `host_tc` / `secs` = the freeze's fields for the note that takes it (QCView `currentNoteKey`: ISO with the
/// UTC offset, the host's timecode, seconds since the item opened); `stored` = the viewport shows a SAVED note's PNG (display-
/// referred: the chain is off for it, QCView's bypass), not the held float frame.
pub struct Review { pub label: String, pub stamp: String, pub note: Option<String>, pub captured_at: String, pub host_tc: String, pub secs: f64, pub stored: bool }

/// The note a drawing / Add note targets now (QCView `currentNoteKey`): a live item's = the Review note or the freeze's stamp
/// (None while live — the callers freeze first); a file's = the playhead's. Free of `App` so a caller holding the GPU can ask.
pub(crate) fn note_cur(live: Option<&Live>, player: Option<&qcv_player::Transport>, notes: &[qcv_notes::Note], stream_name: &str) -> Option<crate::annotate::Cur> {
    use crate::annotate::{Cur, StreamTag};
    if let Some(l) = live {
        let r = l.review.as_ref()?;
        if let Some(k) = &r.note { let n = notes.iter().find(|n| &n.timecode == k)?; return Some(Cur { key: k.clone(), secs: n.timestamp_seconds, frame: 0, stream: Some(StreamTag { captured_at: n.captured_at.clone(), host_timecode: n.host_timecode.clone(), name: n.stream.clone() }) }) }
        return Some(Cur { key: r.stamp.clone(), secs: r.secs, frame: 0, stream: Some(StreamTag { captured_at: r.captured_at.clone(), host_timecode: r.host_tc.clone(), name: stream_name.to_string() }) })
    }
    player.map(|p| Cur::file(p.current_frame(), p.rate().as_f64()))
}
/// QCView `reviewLabelFor`: "HH:mm:ss" from `captured_at`, else the key's "yyyyMMdd-HHmmss-zzz", else the key.
pub(crate) fn review_label(captured_at: &str, key: &str) -> String {
    if captured_at.len() >= 19 { return captured_at[11..19].to_string() }
    if key.len() >= 15 && key.as_bytes()[8] == b'-' { return format!("{}:{}:{}", &key[9..11], &key[11..13], &key[13..15]) }
    key.to_string()
}
/// The card's capture label (QCView `captureLabel`: "HH:mm:ss.zzz").
pub(crate) fn capture_label(captured_at: &str, key: &str) -> String { if captured_at.len() >= 23 { captured_at[11..23].to_string() } else { review_label(captured_at, key) } }

pub struct Live { pub review: Option<Review>, pub feed: HostFeed, pub item: String, pub last_facts: Option<qcv_player::host_feed::Facts>,
    /// the QCAA segment's player (QCView: the live AudioPlayer): it plays as packets arrive; Listen decides if it is heard
    pub audio: Option<qcv_player::audio::AudioPlayer>, /// its reader (the Inspector's Audio facts)
    pub audio_src: Option<std::sync::Arc<qcv_player::audio::LiveAudioSource>> }

impl crate::App {
    /// QCView `connectHostBridge`: the host's item ("QCBridge After Effects" in the Live bin, deduped by URL), made active.
    pub(crate) fn connect_host(&mut self, host: Host) {
        let Some(id) = self.project.add_live_stream(host.url(), host.label()) else { return };
        self.load_item(&id);
    }
    /// QCView `startLiveStream` for a `qcbae://` item; other URLs (SRT) arrive with step 2.
    pub(crate) fn start_live(&mut self, id: &str, item: &qcv_project::MediaItem) -> bool {
        let Some(host) = Host::parse(&item.path) else { self.toast(trw("Live streams arrive with the stream step"), min_ui::ToastKind::Notice); return false };
        self.player = None;   // the file transport stops; the feed owns the picture
        let sync_key = crate::settings::k::live_sync_key(host.key());
        if let Some(g) = self.gpu.as_mut() { g.settings_panel.set_live_sync_key(sync_key); }
        let mut audio_src = None;
        let audio = match qcv_player::audio::AudioPlayer::new() {
            Ok(mut au) => {
                use crate::settings::k;
                qcv_player::audio::live::set_global_buffer_ms(self.settings.int(k::LIVE_BUFFER_MS) as i32);
                qcv_player::audio::live::set_global_sync_offset_ms(self.settings.int(sync_key) as i32);
                qcv_player::audio::live::set_global_scrub_mute(self.settings.bool(k::SCRUB_MUTED));
                au.set_volume(self.settings.float(k::VOLUME) as f32); au.set_muted(self.settings.bool(k::MUTED));
                au.set_live_playdown(self.settings.bool(k::LIVE_PLAYDOWN));
                au.set_routing_mode(crate::routing_of(item));   // the item's Mix pill (QCView: the clip's routing mode)
                if let Some(g) = self.gpu.as_mut() { g.inspector.set_routing(crate::routing_of(item)); }
                audio_src = Some(au.open_live(&host.audio_ring_name()));
                Some(au)
            }
            Err(e) => { log::warn!("live audio: {e}"); None }
        };
        let mut feed = HostFeed::new(host); feed.set_video_hold_ms(qcv_player::audio::live::video_hold_ms());
        self.live = Some(Live { review: None, feed, item: id.to_string(), last_facts: None, audio, audio_src });
        self.loaded_item = id.to_string();
        self.source_path.clear(); self.source_label = item.name.clone();   // the screenshot's name (QCView: the stream's name)
        if let Some(g) = self.gpu.as_mut() {
            g.presenter.engine.clear_frame();
            g.presenter.engine.set_transfer_override(transfer_choice(item.transfer_override)); g.inspector.set_transfer(item.transfer_override);
            g.presenter.engine.set_geometry(min_present::engine::Geometry::default()); g.content_set = None; g.ui_dirty = true;
        }
        self.apply_color();
        self.sync_stream_notes();
        log::info!("live: {} ({})", item.name, host.url());
        true
    }
    /// Per tick: the feed's newest frame to the screen; its facts changing redraws the strip. Ends the feed when its item is
    /// no longer the loaded one.
    pub(crate) fn live_tick(&mut self, now: Instant) {
        let Some(l) = self.live.as_ref() else { return };
        if l.item != self.loaded_item { self.stop_live(); return }
        let Some(g) = self.gpu.as_mut() else { return };
        let l = self.live.as_mut().unwrap();
        // the Mac wraps the ring slot for the GPU (no copy); Windows copies it once (S4)
        let dev = if cfg!(target_os = "macos") { Some(g.presenter.engine.device.clone()) } else { None };
        // in Review the feed keeps receiving but nothing reaches the screen (QCView: publish off)
        if let Some(f) = l.feed.tick(now, dev.as_ref()) && l.review.is_none() {
            match g.presenter.engine.show(&f) { Ok(()) => g.ui_dirty = true, Err(e) => log::warn!("live: show: {e}") }
        }
        let facts = l.feed.facts();
        if l.last_facts.as_ref() != Some(&facts) { l.last_facts = Some(facts); g.ui_dirty = true; }
    }
    /// The live row's buttons (QCView's LiveStrip): Transfer as the Inspector applies it, the panels, the screenshots.
    pub(crate) fn live_cmd(&mut self, c: crate::live_strip::Cmd) {
        use crate::live_strip::Cmd;
        match c {
            Cmd::Transfer(t) => {
                let Some(id) = self.live.as_ref().map(|l| l.item.clone()) else { return };
                if let Some(g) = self.gpu.as_mut() { g.presenter.engine.set_transfer_override(transfer_choice(t)); g.inspector.set_transfer(t); g.content_set = None; g.ui_dirty = true; }
                self.project.set_transfer_override(&id, t);
            }
            Cmd::Listen => { use crate::settings::k; let on = !self.settings.bool(k::LIVE_PLAYDOWN); self.settings.set_bool(k::LIVE_PLAYDOWN, on); if let Some(au) = self.live.as_mut().and_then(|l| l.audio.as_mut()) { au.set_live_playdown(on); } if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; } }
            Cmd::Color => self.color_cmd(crate::color_drawer::Cmd::Toggle),
            Cmd::Notes => self.toggle_notes(),
            Cmd::Shot(d) => self.screenshot(d),
        }
    }
    /// QCView `freezeLive`: the shown frame held on the viewport (fix 6: the float frame, the chain still applies), a fresh
    /// note key minted, the label = the local time "HH:mm:ss". Nothing on screen yet: QCView's toast, still live.
    pub(crate) fn freeze_live(&mut self) {
        let Some(l) = self.live.as_ref() else { return };
        if l.review.is_some() { return }
        let held = l.feed.facts().has_frame && self.gpu.as_mut().is_some_and(|g| g.presenter.engine.hold_shown());
        if !held { self.toast(trw("No frame to freeze yet"), min_ui::ToastKind::Notice); return }
        let now = jiff::Zoned::now();
        let stamp = now.strftime("%Y%m%d-%H%M%S-%3f").to_string();
        let label = now.strftime("%H:%M:%S").to_string();
        let captured_at = now.strftime("%Y-%m-%dT%H:%M:%S.%3f%:z").to_string();
        log::info!("live: REVIEW (freeze {stamp})");
        if let Some(l) = self.live.as_mut() { let (host_tc, secs) = (l.feed.host_timecode(), l.feed.live_seconds()); l.review = Some(Review { label, stamp, note: None, captured_at, host_tc, secs, stored: false }); }
        if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; }
    }
    /// QCView `exitReview`: back to live — the host's newest frame shows again even when it is paused (the republish); a freeze
    /// that never became a note is dropped.
    pub(crate) fn exit_review(&mut self) -> bool {
        let Some(l) = self.live.as_mut() else { return false };
        let Some(r) = l.review.take() else { return false };
        l.feed.republish();
        if r.stored { self.apply_color(); }   // the chain back on for the live picture
        log::info!("live: back to LIVE");
        if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; }
        true
    }
    /// Space (and K / W / S) on a live item: freeze, or back to live (QCView `togglePlayback` on a live single item).
    pub(crate) fn toggle_review(&mut self) { if self.live.as_ref().is_some_and(|l| l.review.is_some()) { self.exit_review(); } else { self.freeze_live(); } }
    /// QCView `enterReview(timecode)`: a stream note's saved frame on the viewport — its clean PNG (display-referred, so the
    /// chain is off for it: QCView's bypass); the note's strokes overlay it. No PNG: QCView's toast.
    pub(crate) fn enter_review(&mut self, key: &str) {
        let Some(note) = self.annot.notes.iter().find(|n| n.timecode == key).cloned() else { return };
        if self.live.is_none() { return }
        let png = self.annot.dir().map(|d| qcv_notes::io::images_dir(&d).join(qcv_notes::io::image_filename(key)));
        let Some((px, (w, h))) = png.and_then(|p| crate::image_rgba8(&p).ok()) else { self.toast(trw("This note has no captured frame"), min_ui::ToastKind::Notice); return };
        // 8-bit sRGB → the packed half-float layout the feed uses (as stored: Transfer sRGB, straight alpha)
        let half: Vec<u8> = px.iter().flat_map(|v| half::f16::from_f32(*v as f32 / 255.0).to_le_bytes()).collect();
        let frame = min_present::Frame { width: w, height: h, pixels: min_present::Pixels::Planar { planes: vec![min_present::Plane::owned(half, w, h, w * 8)], layout: min_present::PlanarLayout::RgbaF16 { premultiplied: false } }, meta: min_present::ColorMeta { transfer: min_present::Transfer::Srgb, range: min_present::Range::Full, bit_depth: 8, ..Default::default() } };
        if let Some(g) = self.gpu.as_mut() { if let Err(e) = g.presenter.engine.show(&frame) { log::warn!("review: {e}"); return } g.ui_dirty = true; }
        let label = review_label(&note.captured_at, key);
        if let Some(l) = self.live.as_mut() {
            let stamp = l.review.as_ref().map(|r| r.stamp.clone()).unwrap_or_default();
            l.review = Some(Review { label, stamp, note: Some(key.to_string()), captured_at: note.captured_at.clone(), host_tc: note.host_timecode.clone(), secs: note.timestamp_seconds, stored: true });
        }
        self.apply_color();   // off while a saved note shows
        log::info!("live: REVIEW of note {key}");
    }
    /// ← / → and Q / E in Review (QCView `stepReviewNote`): the previous / next stream note by key; from a transient freeze, ←
    /// goes to the newest note and → does nothing.
    pub(crate) fn step_review(&mut self, dir: i32) {
        let Some(r) = self.live.as_ref().and_then(|l| l.review.as_ref()) else { return };
        let keys: Vec<String> = self.annot.notes.iter().map(|n| n.timecode.clone()).collect();
        let next = match &r.note {
            None => if dir < 0 { keys.last().cloned() } else { None },
            Some(k) => { let i = keys.iter().position(|x| x == k); i.and_then(|i| if dir < 0 { i.checked_sub(1) } else { Some(i + 1) }).and_then(|j| keys.get(j).cloned()) }
        };
        if let Some(k) = next { self.enter_review(&k); }
    }
    /// The first note on a transient freeze takes it (QCView `adoptReviewNote`).
    pub(crate) fn adopt_review_note(&mut self, key: &str) {
        if let Some(r) = self.live.as_mut().and_then(|l| l.review.as_mut()) && r.note.is_none() && r.stamp == key { r.note = Some(key.to_string()); }
    }
    /// The Review note is gone (deleted / undone): leave Review (QCView kept showing it and then minted malformed notes — row 228 1b).
    pub(crate) fn review_note_check(&mut self) {
        let gone = self.live.as_ref().and_then(|l| l.review.as_ref()).and_then(|r| r.note.clone()).is_some_and(|k| !self.annot.notes.iter().any(|n| n.timecode == k));
        if gone { self.exit_review(); }
    }
    /// QCView `syncAnnotationManagerToActiveMedia`'s stream branch: a saved project → the item's folder under its `.qcview/streams`
    /// (found again by the id after a rename; created by the first note); never saved → no notes, the panel's Save tile.
    pub(crate) fn sync_stream_notes(&mut self) {
        let Some(l) = self.live.as_ref() else { return };
        let Some(item) = self.project.item(&l.item).cloned() else { return };
        let project = self.project.file_path().to_string();
        if project.is_empty() { self.annot.load(None); return }
        let dir = qcv_notes::io::stream_dir(std::path::Path::new(&project), &item.name, &item.id);
        let header = qcv_notes::io::stream_header(&dir, &item.name, &item.id, &item.path);
        self.annot.load_stream(dir, header);
    }
    /// QCView `stopLiveStream`: the feed closes and no stale frame survives.
    pub(crate) fn stop_live(&mut self) {
        if self.live.take().is_none() { return }
        if let Some(g) = self.gpu.as_mut() { g.presenter.engine.clear_frame(); g.ui_dirty = true; }
        log::info!("live: stopped");
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_note_stamp_is_qcviews() {
        // QCView `currentNoteKey` for a stream: local time "yyyyMMdd-HHmmss-zzz" (sorts chronologically as a string)
        let t: jiff::Zoned = "2026-11-09T14:32:05.123-05:00[America/New_York]".parse().unwrap();
        assert_eq!(t.strftime("%Y%m%d-%H%M%S-%3f").to_string(), "20261109-143205-123");
        assert_eq!(t.strftime("%H:%M:%S").to_string(), "14:32:05");
    }
}
