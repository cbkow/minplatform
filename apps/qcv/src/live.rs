//! A live host feed in the single view (canon rows 227–228, step 1a): QCView's Connect to After Effects / Premiere Pro, the
//! feed on the render tick, and the rule that ends it — the feed lives exactly as long as its item is the loaded one (every
//! way out — another item, a file or project opened, New Project, dual view, the item removed — changes the loaded item).
use crate::{transfer_choice, trw};
use qcv_player::host_feed::HostFeed;
use std::time::Instant;
use qcv_ring::url::Host;

pub struct Live { pub feed: HostFeed, pub item: String, pub last_facts: Option<qcv_player::host_feed::Facts>,
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
        self.live = Some(Live { feed, item: id.to_string(), last_facts: None, audio, audio_src });
        self.loaded_item = id.to_string();
        self.source_path.clear(); self.source_label = item.name.clone();   // the screenshot's name (QCView: the stream's name)
        if let Some(g) = self.gpu.as_mut() {
            g.presenter.engine.clear_frame();
            g.presenter.engine.set_transfer_override(transfer_choice(item.transfer_override)); g.inspector.set_transfer(item.transfer_override);
            g.presenter.engine.set_geometry(min_present::engine::Geometry::default()); g.content_set = None; g.ui_dirty = true;
        }
        self.apply_color();
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
        if let Some(f) = l.feed.tick(now, dev.as_ref()) {
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
            Cmd::Color => self.color_cmd(crate::color_drawer::Cmd::Toggle),
            Cmd::Notes => {}   // step 1b (Review)
            Cmd::Shot(d) => self.screenshot(d),
        }
    }
    /// QCView `stopLiveStream`: the feed closes and no stale frame survives.
    pub(crate) fn stop_live(&mut self) {
        if self.live.take().is_none() { return }
        if let Some(g) = self.gpu.as_mut() { g.presenter.engine.clear_frame(); g.ui_dirty = true; }
        log::info!("live: stopped");
    }
}
