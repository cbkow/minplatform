//! The player's settings schema (canon row 142; the 2026-11-07 rails ruling): QCView's keys VERBATIM — the same
//! `group/name` spellings, the same defaults, the same ranges — so a preference means the same thing in both players.
//! Keys whose feature has not landed here are in the schema already (the panel shows them greyed): the shape is final.
//! One deviation, written: `performance/hwScrubInterCodecs` keeps QCView's default (off, "experimental") on both OSes
//! where `ScrubOptions::default()` had turned it on for Windows — the D3D12VA scrub check (row 149) is still owed.
use min_settings::{Key, Schema, Settings};

pub const APP: &str = "qcv";
pub const ENV: &str = "QCV";

pub mod k {
    pub const VOLUME: &str = "audio/volume";
    pub const MUTED: &str = "audio/muted";
    pub const SCRUB_MUTED: &str = "audio/scrubMuted";
    pub const SYNC_OFFSET_MS: &str = "audio/syncOffsetMs";
    pub const DUAL_SYNC_OFFSET_MS: &str = "audio/dualSyncOffsetMs";
    pub const LIVE_PLAYDOWN: &str = "audio/livePlaydown";
    pub const LIVE_SYNC_OFFSET_MS: &str = "audio/liveSyncOffsetMs/premiere";
    pub const LIVE_BUFFER_MS: &str = "audio/liveBufferMs";
    pub const BLENDER_SESSIONS: &str = "bridge/blenderSessions";
    pub const BACKGROUND_MODE: &str = "display/backgroundMode";
    pub const BRIGHTNESS: &str = "display/brightness";
    pub const SCREENSHOT_FORMAT: &str = "export/screenshotFormat";
    pub const LOOP_ENABLED: &str = "playback/loopEnabled";
    pub const ALWAYS_ON_TOP: &str = "ui/alwaysOnTop";
    pub const ALWAYS_OPEN_MINIMAL: &str = "ui/alwaysOpenMinimal";
    pub const DRAG_VIEWPORT_MOVES_WINDOW: &str = "ui/dragViewportMovesWindow";
    pub const LANGUAGE: &str = "ui/language";
    pub const TIMELINE_HOVER_THUMBS: &str = "ui/timelineHoverThumbs";
    pub const TIMELINE_WAVEFORMS: &str = "ui/timelineWaveforms";
    pub const AUTO_UPDATE_CHECKS: &str = "ui/autoUpdateChecks";
    pub const IMAGE_SEQ_THREADS: &str = "performance/imageSeqThreads";
    pub const IMAGE_SEQ_READ_AHEAD: &str = "performance/imageSeqReadAhead";
    pub const FFMPEG_THREADS: &str = "performance/ffmpegThreads";
    pub const HW_DECODE: &str = "performance/hardwareDecodeEnabled";
    pub const SCRUB_CACHE_MB: &str = "performance/scrubCacheMB";
    pub const HW_SCRUB_INTER: &str = "performance/hwScrubInterCodecs";
    pub const LEFT_RAIL_WIDTH: &str = "ui.layout/leftRailWidth";
    pub const RIGHT_RAIL_WIDTH: &str = "ui.layout/rightRailWidth";
    pub const LEFT_RAIL_COLLAPSED: &str = "ui.layout/leftRailCollapsed";
    /// ours (canon rows 199 / 205): the colour chain on or off (QCView's one On / Off switch), the Color drawer open + its height
    pub const COLOR_ENABLED: &str = "color/enabled";
    pub const COLOR_DRAWER_OPEN: &str = "ui.layout/colorDrawerOpen";
    pub const COLOR_DRAWER_HEIGHT: &str = "ui.layout/colorDrawerHeight";
    /// the waveform as the user left it: scale (−1 = follow the source, 0 SDR, 1 HDR), the HDR top in nits, open / collapsed
    pub const WAVEFORM_SCALE: &str = "ui.layout/waveformScale";
    pub const WAVEFORM_PEAK: &str = "ui.layout/waveformPeakNits";
    pub const WAVEFORM_OPEN: &str = "ui.layout/waveformOpen";
    pub const RIGHT_RAIL_COLLAPSED: &str = "ui.layout/rightRailCollapsed";
    pub const WINDOW_X: &str = "window/x";
    pub const WINDOW_Y: &str = "window/y";
    pub const WINDOW_WIDTH: &str = "window/width";
    pub const WINDOW_HEIGHT: &str = "window/height";
    pub const WINDOW_MAXIMIZED: &str = "window/maximized";
    /// QCView `recents/media` + `recents/projects`: most-recent-first, capped at 10 (stored newline-joined; QSettings held a list)
    pub const RECENT_MEDIA: &str = "recents/media";
    pub const RECENT_PROJECTS: &str = "recents/projects";
}

static KEYS: &[Key] = &[
    Key::float(k::VOLUME, 1.0, 0.0, 1.0),
    Key::bool(k::MUTED, false),
    Key::bool(k::SCRUB_MUTED, false),
    Key::int(k::SYNC_OFFSET_MS, 0, -100, 100),
    Key::int(k::DUAL_SYNC_OFFSET_MS, 0, -100, 100),
    Key::bool(k::LIVE_PLAYDOWN, false),
    Key::int(k::LIVE_SYNC_OFFSET_MS, 0, -200, 200),
    Key::int(k::LIVE_BUFFER_MS, 60, 20, 200),
    Key::bool(k::BLENDER_SESSIONS, false),
    Key::int(k::BACKGROUND_MODE, 0, 0, 3),
    Key::float(k::BRIGHTNESS, 1.0, 0.0, 4.0),
    Key::str(k::SCREENSHOT_FORMAT, "png"),
    Key::bool(k::LOOP_ENABLED, true),
    Key::bool(k::ALWAYS_ON_TOP, false),
    Key::bool(k::ALWAYS_OPEN_MINIMAL, false),
    Key::bool(k::DRAG_VIEWPORT_MOVES_WINDOW, true),
    Key::str(k::LANGUAGE, ""),
    Key::bool(k::TIMELINE_HOVER_THUMBS, true),
    Key::bool(k::TIMELINE_WAVEFORMS, true),
    Key::bool(k::AUTO_UPDATE_CHECKS, true),
    Key::int(k::IMAGE_SEQ_THREADS, 16, 1, 64),
    Key::int(k::IMAGE_SEQ_READ_AHEAD, 72, 24, 240),
    Key::int(k::FFMPEG_THREADS, 0, 0, 32),
    Key::bool(k::HW_DECODE, true),
    Key::int(k::SCRUB_CACHE_MB, 512, 128, 4096),
    Key::bool(k::HW_SCRUB_INTER, false),
    Key::bool(k::COLOR_ENABLED, false),
    Key::bool(k::COLOR_DRAWER_OPEN, false),
    Key::int(k::COLOR_DRAWER_HEIGHT, 320, 200, 600),
    Key::int(k::WAVEFORM_SCALE, -1, -1, 1),
    Key::int(k::WAVEFORM_PEAK, 1000, 300, 4000),
    Key::bool(k::WAVEFORM_OPEN, true),
    Key::int(k::LEFT_RAIL_WIDTH, 280, 200, 600),
    Key::int(k::RIGHT_RAIL_WIDTH, 320, 200, 600),
    Key::bool(k::LEFT_RAIL_COLLAPSED, false),
    Key::bool(k::RIGHT_RAIL_COLLAPSED, true),   // the 2026-11-07 ruling: the right rail starts collapsed (QCView: open)
    Key::int(k::WINDOW_X, i64::MIN / 4, i64::MIN / 4, i64::MAX / 4),
    Key::int(k::WINDOW_Y, i64::MIN / 4, i64::MIN / 4, i64::MAX / 4),
    Key::int(k::WINDOW_WIDTH, 0, 0, 32768),
    Key::int(k::WINDOW_HEIGHT, 0, 0, 32768),
    Key::bool(k::WINDOW_MAXIMIZED, false),
    Key::str(k::RECENT_MEDIA, ""),
    Key::str(k::RECENT_PROJECTS, ""),
];

pub static SCHEMA: Schema = Schema { keys: KEYS };

pub fn open() -> Settings {
    let s = Settings::open(APP, ENV, &SCHEMA);
    match s.path() { Some(p) => log::info!("settings: {}", p.display()), None => log::info!("settings: no store (in memory)") }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_schema_is_qcviews() {
        let s = Settings::in_memory(&SCHEMA);
        assert!(s.bool(k::LOOP_ENABLED) && s.bool(k::TIMELINE_HOVER_THUMBS) && s.bool(k::TIMELINE_WAVEFORMS) && !s.bool(k::SCRUB_MUTED));
        assert_eq!((s.int(k::FFMPEG_THREADS), s.int(k::SCRUB_CACHE_MB), s.int(k::IMAGE_SEQ_THREADS), s.int(k::IMAGE_SEQ_READ_AHEAD), s.int(k::LIVE_BUFFER_MS)), (0, 512, 16, 72, 60));
        assert_eq!((s.int(k::LEFT_RAIL_WIDTH), s.int(k::RIGHT_RAIL_WIDTH)), (280, 320));
        assert!(!s.bool(k::LEFT_RAIL_COLLAPSED) && s.bool(k::RIGHT_RAIL_COLLAPSED));
        assert_eq!(s.string(k::SCREENSHOT_FORMAT), "png");
        assert!(!s.bool(k::HW_SCRUB_INTER), "QCView's default, both OSes");
    }
}
