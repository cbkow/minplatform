//! A live host feed in the single view (canon rows 227–228, step 1a): QCView's Connect to After Effects / Premiere Pro, the
//! feed on the render tick, and the rule that ends it — the feed lives exactly as long as its item is the loaded one (every
//! way out — another item, a file or project opened, New Project, dual view, the item removed — changes the loaded item).
use crate::{transfer_choice, trw};
use qcv_player::host_feed::HostFeed;
use qcv_player::stream_feed::StreamFeed;
use std::sync::Arc;

/// A live item's source: a QCBridge host over shared memory (step 1), or a network stream (step 2a: SRT / any FFmpeg URL).
pub enum Feed { Host(HostFeed), Stream(StreamFeed) }
impl Feed {
    pub fn tick(&mut self, now: Instant, dev: Option<&wgpu::Device>) -> Option<Arc<min_present::Frame>> { match self { Feed::Host(h) => h.tick(now, dev).map(Arc::new), Feed::Stream(s) => s.tick() } }
    pub fn facts(&self) -> qcv_player::host_feed::Facts { match self { Feed::Host(h) => h.facts(), Feed::Stream(s) => s.facts() } }
    pub fn republish(&mut self) { match self { Feed::Host(h) => h.republish(), Feed::Stream(s) => s.republish() } }
    pub fn live_seconds(&self) -> f64 { match self { Feed::Host(h) => h.live_seconds(), Feed::Stream(s) => s.live_seconds() } }
    pub fn host_timecode(&self) -> String { match self { Feed::Host(h) => h.host_timecode(), Feed::Stream(_) => String::new() } }
    pub fn host_fps(&self) -> Option<f64> { match self { Feed::Host(h) => h.host_fps(), Feed::Stream(s) => s.fps() } }
    pub fn set_video_hold_ms(&mut self, ms: i32) { if let Feed::Host(h) = self { h.set_video_hold_ms(ms); } }
    /// The host application's name for the details ("After Effects"), "" for a stream.
    pub fn app(&self) -> &'static str { match self { Feed::Host(h) => h.host().app(), Feed::Stream(_) => "" } }
    /// The host's settings key ("ae" / "premiere" / "probe"), "" for a stream.
    pub fn key(&self) -> &'static str { match self { Feed::Host(h) => h.host().key(), Feed::Stream(_) => "" } }
    pub fn is_stream(&self) -> bool { matches!(self, Feed::Stream(_)) }
}
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

pub struct Live { pub review: Option<Review>, /// a saved note's frame being decoded off the UI thread (its key)
    pub review_rx: Option<(String, std::sync::mpsc::Receiver<Option<min_present::Frame>>)>, pub feed: Feed, pub item: String, pub last_facts: Option<qcv_player::host_feed::Facts>,
    /// the QCAA segment's player (QCView: the live AudioPlayer): it plays as packets arrive; Listen decides if it is heard
    pub audio: Option<qcv_player::audio::AudioPlayer>, /// its reader (the Inspector's Audio facts)
    pub audio_src: Option<std::sync::Arc<qcv_player::audio::LiveAudioSource>> }

impl crate::App {
    /// QCView `connectHostBridge`: the host's item ("QCBridge After Effects" in the Live bin, deduped by URL), made active.
    pub(crate) fn connect_host(&mut self, host: Host) {
        let Some(id) = self.project.add_live_stream(host.url(), host.label()) else { return };
        self.load_item(&id);
    }
    /// A live item's feed (and, for a host, its audio player), made WITHOUT touching the view: the single view adopts it
    /// (`start_live`), or a dual side does (canon row 258). None = refused (a Blender listener while the input is off).
    pub(crate) fn make_live(&mut self, id: &str, item: &qcv_project::MediaItem) -> Option<Live> {
        let Some(host) = Host::parse(&item.path) else { return self.make_stream_live(id, item) };
        let sync_key = crate::settings::k::live_sync_key(host.key());
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
                audio_src = Some(au.open_live(&host.audio_ring_name()));
                Some(au)
            }
            Err(e) => { log::warn!("live audio: {e}"); None }
        };
        let mut feed = Feed::Host(HostFeed::new(host)); feed.set_video_hold_ms(qcv_player::audio::live::video_hold_ms());
        log::info!("live: {} ({})", item.name, host.url());
        Some(Live { review: None, review_rx: None, feed, item: id.to_string(), last_facts: None, audio, audio_src })
    }
    /// QCView `startLiveStream` for a network URL (`LiveStreamDecoder`): the decoder on its own thread, no audio (QCView
    /// discards a stream's audio), the hardware decoder when the setting allows it (QCView's macOS ignored the setting).
    fn make_stream_live(&mut self, id: &str, item: &qcv_project::MediaItem) -> Option<Live> {
        // a LISTENER the Blender card brought is a Blender clip: it listens only while the Blender input is on (QCView's gate —
        // which failed silently, and held every listener URL). One the user opened this session listens regardless (row 243:
        // the walk 2026-11-09 — QCView as the receiver for its own Stream card).
        if item.path.contains("mode=listener") && !self.blender.enabled && !self.blender.user_listeners.contains(item.path.trim()) {
            self.toast(trw("Turn on File ▸ Connect to Blender to listen for this Blender session"), min_ui::ToastKind::Notice); return None
        }
        let hw = if self.settings.bool(crate::settings::k::HW_DECODE) { self.hw.clone() } else { None };
        let feed = Feed::Stream(StreamFeed::open(&item.path, hw));
        log::info!("live: {} ({})", item.name, qcv_player::stream_feed::redact(&item.path));
        Some(Live { review: None, review_rx: None, feed, item: id.to_string(), last_facts: None, audio: None, audio_src: None })
    }
    /// QCView `startLiveStream`: a `qcbae://` item reads its host's shared memory; any other URL opens as a network stream.
    /// The feed becomes the single view's.
    pub(crate) fn start_live(&mut self, id: &str, item: &qcv_project::MediaItem) -> bool {
        let Some(l) = self.make_live(id, item) else { return false };
        self.adopt_live(l, item);
        true
    }
    /// The single view takes a feed — a new one, or a dual side's coming back with no reconnect (row 258, R8): the file
    /// transport stops, the feed owns the picture, the item's choices reach the screen.
    pub(crate) fn adopt_live(&mut self, mut l: Live, item: &qcv_project::MediaItem) {
        self.player = None;
        if let Some(host) = Host::parse(&item.path) { let sync_key = crate::settings::k::live_sync_key(host.key()); if let Some(g) = self.gpu.as_mut() { g.settings_panel.set_live_sync_key(sync_key); g.inspector.set_routing(crate::routing_of(item)); } }
        if let Some(au) = l.audio.as_mut() { au.set_live_playdown(self.settings.bool(crate::settings::k::LIVE_PLAYDOWN)); }
        l.review = None; l.review_rx = None;
        self.loaded_item = l.item.clone();
        self.live = Some(l);
        self.source_path.clear(); self.source_label = item.name.clone();   // the screenshot's name (QCView: the stream's name)
        if let Some(g) = self.gpu.as_mut() {
            g.presenter.engine.clear_frame();
            g.presenter.engine.set_transfer_override(transfer_choice(item.transfer_override)); g.inspector.set_transfer(item.transfer_override);
            g.presenter.engine.set_geometry(min_present::engine::Geometry::default()); g.content_set = None; g.ui_dirty = true;
        }
        self.apply_color();
        self.sync_stream_notes();
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
            let f = &*f;
            match g.presenter.engine.show(&f) { Ok(()) => g.ui_dirty = true, Err(e) => log::warn!("live: show: {e}") }
        }
        let facts = l.feed.facts();
        if l.last_facts.as_ref() != Some(&facts) { l.last_facts = Some(facts); g.ui_dirty = true; }
        // a stream's colour tags onto its item (QCView `pushLiveTags`: session facts, no dirty) — the Auto label reads them
        if let Feed::Stream(s) = &l.feed { let (t, p, m) = s.tags(); let id = l.item.clone(); if self.project.set_live_video_tags(&id, &t, &p, &m) { if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; } } }
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
        if !self.annot.notes.iter().any(|n| n.timecode == key) { return }
        if self.live.is_none() { return }
        // the decode + conversion on a worker (the newest note's pixels from memory); the frame shows when it lands (`review_poll`)
        let png = self.annot.dir().map(|d| qcv_notes::io::images_dir(&d).join(qcv_notes::io::image_filename(key)));
        let mem = self.note_img.as_ref().filter(|(k, _, _)| k == key).map(|(_, px, wh)| (px.clone(), *wh));
        if mem.is_none() && !png.as_ref().is_some_and(|p| p.exists()) { self.toast(trw("This note has no captured frame"), min_ui::ToastKind::Notice); return }
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new().name("review-frame".into()).spawn(move || {
            let src = match mem { Some((px, wh)) => Some((px, wh)), None => png.and_then(|p| crate::image_rgba8(&p).ok()).map(|(px, wh)| (std::sync::Arc::new(px), wh)) };
            let _ = tx.send(src.map(|(px, (w, h))| {
                // 8-bit sRGB → the packed half-float layout the feed uses (as stored: Transfer sRGB, straight alpha)
                let mut half = Vec::with_capacity(px.len() * 2);
                for v in px.iter() { half.extend_from_slice(&half::f16::from_f32(*v as f32 / 255.0).to_le_bytes()); }
                min_present::Frame { width: w, height: h, pixels: min_present::Pixels::Planar { planes: vec![min_present::Plane::owned(half, w, h, w * 8)], layout: min_present::PlanarLayout::RgbaF16 { premultiplied: false } }, meta: min_present::ColorMeta { transfer: min_present::Transfer::Srgb, range: min_present::Range::Full, bit_depth: 8, ..Default::default() } }
            }));
        }).ok();
        if let Some(l) = self.live.as_mut() { l.review_rx = Some((key.to_string(), rx)); }
    }
    /// A saved note's frame landed: on the viewport, Review on that note (QCView `enterReview`'s second half).
    pub(crate) fn review_poll(&mut self) {
        let Some(l) = self.live.as_mut() else { return };
        let Some((key, rx)) = l.review_rx.as_ref() else { return };
        let r = match rx.try_recv() { Ok(r) => r, Err(std::sync::mpsc::TryRecvError::Empty) => return, Err(_) => None };
        let key = key.clone(); l.review_rx = None;
        let Some(frame) = r else { self.toast(trw("This note has no captured frame"), min_ui::ToastKind::Notice); return };
        let Some(note) = self.annot.notes.iter().find(|n| n.timecode == key).cloned() else { return };
        if let Some(g) = self.gpu.as_mut() { if let Err(e) = g.presenter.engine.show(&frame) { log::warn!("review: {e}"); return } g.ui_dirty = true; }
        let key = key.as_str();
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
        if project.is_empty() { self.annot.load(None); self.annot.active = false; return }
        let dir = qcv_notes::io::stream_dir(std::path::Path::new(&project), &item.name, &item.id);
        let header = qcv_notes::io::stream_header(&dir, &item.name, &item.id, &item.path);
        self.annot.load_stream(dir, header);
        // the panel already open (on an unsaved project, a reopened one): drawing — and the notes' strokes on the picture — follow
        // the new target (they stayed off until the next tool click)
        if self.gpu.as_ref().is_some_and(|g| g.notes.open) { self.annot.active = self.annot.has_target(); }
        if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; }
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
