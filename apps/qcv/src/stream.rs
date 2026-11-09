//! The STREAM output in the app (canon row 239, `docs/phase2/step4-stream-card.md`; QCView `OutputBus`'s stream parts): the
//! settings (`output/stream/*`, QCView's keys and defaults), Start / Stop, the session's status for the card. The picture
//! comes from the outputs' publish pass (`outputs.rs`): the composite at the stream's raster with the layers, the SDR wire
//! texture, the NV12 pass into the encoder's own surface, handed over when the GPU is done — no CPU copy. The audio is the
//! outputs' tap. A session never starts by itself at launch (QCView), except through the dev aid `QCV_STREAM_SPIKE`
//! (QCView's: a URL, or empty = the saved settings).
use crate::settings::k;
use qcv_stream::sink::{self, State, Status, StreamConfig, StreamSink};
use qcv_stream::Profile;

#[derive(Default)]
pub struct Stream {
    pub sink: Option<StreamSink>, /// why the last session ended or would not start
    pub error: Option<String>, /// the outputs' stamp of the picture last handed to the encoder
    pub published: Option<u64>, pub nv12: Option<qcv_stream::nv12::Nv12Pass>, spike_done: bool,
}
impl Stream {
    pub fn running(&self) -> bool { self.sink.is_some() }
    pub fn status(&self) -> Option<Status> { self.sink.as_ref().map(|s| s.status()) }
}

/// QCView `fpsToRational` for the fixed choices; "auto" → (0, 1).
fn fps_of(mode: &str) -> (u32, u32) { match mode.trim().parse::<f64>() { Ok(f) if f > 0.0 => (f.round() as u32, 1), _ => (0, 1) } }
/// A source rate as a rational (QCView `fpsToRational`: the NTSC rates exact).
pub(crate) fn rational(fps: f64) -> (u32, u32) {
    for (r, n) in [(23.976, 24000), (29.97, 30000), (59.94, 60000), (47.952, 48000), (119.88, 120000)] { if (fps - r).abs() < 0.01 { return (n, 1001) } }
    if fps > 0.0 && (fps - fps.round()).abs() < 1e-3 { return (fps.round() as u32, 1) }
    if fps > 0.0 { ((fps * 1000.0).round() as u32, 1000) } else { (0, 0) }
}

impl crate::App {
    /// The session's config from the settings (QCView `startStream`: the clamps, the resolution names, the key's place).
    pub(crate) fn stream_config(&self) -> StreamConfig {
        let st = &self.settings;
        let (width, height) = match st.string(k::STREAM_RESOLUTION).as_str() { "720p" => (1280, 720), "540p" => (960, 540), "2160p" => (3840, 2160), _ => (1920, 1080) };
        let (url, stream_id) = sink::apply_key(&st.string(k::STREAM_URL), &st.string(k::STREAM_KEY));
        let (fps_num, fps_den) = fps_of(&st.string(k::STREAM_FPS_MODE));
        StreamConfig { url, width, height, bitrate_kbps: st.int(k::STREAM_BITRATE).clamp(500, 50000) as u32, profile: if st.string(k::STREAM_PROFILE) == "main" { Profile::Main } else { Profile::Baseline },
            srt_latency_ms: st.int(k::STREAM_SRT_LATENCY).clamp(20, 5000) as u32, stream_id, passphrase: std::env::var("QCV_STREAM_SPIKE_PASSPHRASE").unwrap_or_default(), fps_num, fps_den, audio_kbps: st.int(k::STREAM_AUDIO).max(0) as u32 }
    }
    pub(crate) fn stream_start(&mut self) {
        if self.outputs.stream.running() { return }
        let cfg = self.stream_config();
        self.stream_start_with(cfg);
    }
    fn stream_start_with(&mut self, cfg: StreamConfig) {
        let src = rational(self.output_transport().2);
        match StreamSink::start(cfg, src) {
            Ok(s) => {
                self.outputs.stream.sink = Some(s); self.outputs.stream.error = None; self.outputs.stream.published = None;   // the current picture goes out at once (ruling 7)
                self.settings.set_bool(k::OUTPUT_FEED_MUTED, false);   // ruling 13: a stream starts with sound
                self.output_tap_sync();
            }
            Err(e) => { log::warn!("stream: {e}"); self.outputs.stream.error = Some(e); }
        }
        if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; }
    }
    /// Stop: returns at once (ruling 8); a user's Stop clears the error.
    pub(crate) fn stream_stop(&mut self) {
        if let Some(mut s) = self.outputs.stream.sink.take() { s.stop(); }
        self.outputs.stream.error = None; self.outputs.stream.published = None;
        self.output_tap_sync();
        if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; }
    }
    /// Each tick: the source's rate for a session still waiting, a session that ended by itself, the dev aid.
    pub(crate) fn stream_tick(&mut self) {
        if !self.outputs.stream.spike_done && self.gpu.is_some() {
            self.outputs.stream.spike_done = true;
            if let Ok(url) = std::env::var("QCV_STREAM_SPIKE") { let mut cfg = self.stream_config(); if !url.is_empty() { cfg.url = url; } self.stream_start_with(cfg); }
        }
        let src = rational(self.output_transport().2);
        let Some(s) = self.outputs.stream.sink.as_ref() else { return };
        s.set_source_fps(src);
        let st = s.status();
        if st.failed { self.outputs.stream.error = Some(match st.state { State::Failed(e) => e, _ => String::new() }); self.outputs.stream.sink = None; self.output_tap_sync(); if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; } }
    }
}
