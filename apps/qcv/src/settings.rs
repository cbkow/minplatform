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
    /// QCView `liveSyncOffsetKey`: one A/V offset per host application (`audio/liveSyncOffsetMs/<host>`, premiere when none)
    pub const LIVE_SYNC_OFFSET_MS: &str = "audio/liveSyncOffsetMs/premiere";
    pub const LIVE_SYNC_OFFSET_MS_AE: &str = "audio/liveSyncOffsetMs/ae";
    pub const LIVE_SYNC_OFFSET_MS_PROBE: &str = "audio/liveSyncOffsetMs/probe";
    pub fn live_sync_key(host: &str) -> &'static str { match host { "ae" => LIVE_SYNC_OFFSET_MS_AE, "probe" => LIVE_SYNC_OFFSET_MS_PROBE, _ => LIVE_SYNC_OFFSET_MS } }
    pub const LIVE_BUFFER_MS: &str = "audio/liveBufferMs";
    pub const BLENDER_SESSIONS: &str = "bridge/blenderSessions";
    /// QCView's Output section (row 227 step 3): the feed mute, and per shipped sink (ndi, aja) the enabled flag + its settings
    pub const OUTPUT_FEED_MUTED: &str = "output/feedMuted";
    /// ours (row 246): the Color panel's Video toggle — the chain's SDR picture as an unmanaged Rec.709 desktop shows it
    pub const VIDEO_EMULATION: &str = "color/videoEmulation";
    /// QCView's Guides / Notes chips (row 238): the safety guides and the note strokes drawn into the outputs that ask
    pub const OUTPUT_GUIDES: &str = "output/includeGuides";
    pub const OUTPUT_NOTES: &str = "output/includeAnnotations";
    /// `output/enabled/<id>` and `output/<id>/<field>` for the two shipped sinks (the schema is static; another id keeps its
    /// settings in memory only)
    pub fn output_key(id: &str, field: &str) -> Option<&'static str> {
        Some(match (id, field) {
            ("ndi", "enabled") => "output/enabled/ndi", ("aja", "enabled") => "output/enabled/aja",
            ("ndi", "raster") => "output/ndi/raster", ("ndi", "transfer") => "output/ndi/transfer", ("ndi", "primaries") => "output/ndi/primaries", ("ndi", "peakNits") => "output/ndi/peakNits", ("ndi", "minNits") => "output/ndi/minNits", ("ndi", "maxCll") => "output/ndi/maxCll", ("ndi", "maxFall") => "output/ndi/maxFall",
            ("aja", "raster") => "output/aja/raster", ("aja", "transfer") => "output/aja/transfer", ("aja", "primaries") => "output/aja/primaries", ("aja", "peakNits") => "output/aja/peakNits", ("aja", "minNits") => "output/aja/minNits", ("aja", "maxCll") => "output/aja/maxCll", ("aja", "maxFall") => "output/aja/maxFall",
            _ => return None,
        })
    }
    pub const BACKGROUND_MODE: &str = "display/backgroundMode";
    /// QCView `safety/colorHex`; the guide, opacity and width persist too (row 215)
    pub const SAFETY_COLOR: &str = "safety/colorHex";
    /// QCView `annotation/colorHex` (the Notes drawer's colour)
    pub const ANNOTATION_COLOR: &str = "annotation/colorHex";
    /// the Notes drawer's height (its own range, row 215)
    pub const NOTES_DRAWER_HEIGHT: &str = "notes/drawerHeight";
    pub const SAFETY_GUIDE: &str = "safety/guide";
    pub const SAFETY_OPACITY: &str = "safety/opacity";
    pub const SAFETY_WIDTH: &str = "safety/width";
    pub const BRIGHTNESS: &str = "display/brightness";
    pub const SCREENSHOT_FORMAT: &str = "export/screenshotFormat";
    /// ours (row 216): safety guides drawn into screenshots, off by default (QCView never drew them)
    pub const SCREENSHOT_GUIDES: &str = "export/screenshotGuides";
    pub const LOOP_ENABLED: &str = "playback/loopEnabled";
    pub const ALWAYS_ON_TOP: &str = "ui/alwaysOnTop";
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
    Key::int(k::DUAL_SYNC_OFFSET_MS, if cfg!(target_os = "macos") { 17 } else { 15 }, -100, 100),   // QCView `dualAudioSyncOffsetMsDefault`
    Key::bool(k::LIVE_PLAYDOWN, false),
    Key::int(k::LIVE_SYNC_OFFSET_MS, 0, -200, 200), Key::int(k::LIVE_SYNC_OFFSET_MS_AE, 0, -200, 200), Key::int(k::LIVE_SYNC_OFFSET_MS_PROBE, 0, -200, 200),
    Key::int(k::LIVE_BUFFER_MS, 60, 20, 200),
    Key::bool(k::BLENDER_SESSIONS, false),
    Key::bool(k::VIDEO_EMULATION, false),
    Key::bool(k::OUTPUT_GUIDES, true), Key::bool(k::OUTPUT_NOTES, true),   // QCView: both on
    Key::bool(k::OUTPUT_FEED_MUTED, true),   // the user 2026-11-09: the feed starts muted (QCView: on)
    Key::bool("output/enabled/ndi", false), Key::int("output/ndi/raster", 0, 0, 2), Key::int("output/ndi/transfer", 1, 1, 3), Key::int("output/ndi/primaries", 1, 1, 2), Key::float("output/ndi/peakNits", 1000.0, 1.0, 10000.0), Key::float("output/ndi/minNits", 0.005, 0.0, 1000.0), Key::int("output/ndi/maxCll", 1000, 0, 10000), Key::int("output/ndi/maxFall", 400, 0, 10000),
    Key::bool("output/enabled/aja", false), Key::int("output/aja/raster", 0, 0, 2), Key::int("output/aja/transfer", 1, 1, 3), Key::int("output/aja/primaries", 1, 1, 2), Key::float("output/aja/peakNits", 1000.0, 1.0, 10000.0), Key::float("output/aja/minNits", 0.005, 0.0, 1000.0), Key::int("output/aja/maxCll", 1000, 0, 10000), Key::int("output/aja/maxFall", 400, 0, 10000),
    Key::int(k::BACKGROUND_MODE, 0, 0, 3),
    Key::float(k::BRIGHTNESS, 1.0, 0.0, 4.0),
    Key::str(k::SCREENSHOT_FORMAT, "png"), Key::bool(k::SCREENSHOT_GUIDES, false),
    Key::str(k::SAFETY_COLOR, "#ffff00"), Key::str(k::ANNOTATION_COLOR, "#ff0000"), Key::float(k::NOTES_DRAWER_HEIGHT, 340.0, 320.0, 720.0), Key::str(k::SAFETY_GUIDE, ""), Key::float(k::SAFETY_OPACITY, 0.7, 0.0, 1.0), Key::float(k::SAFETY_WIDTH, 2.0, 1.0, 8.0),
    Key::bool(k::LOOP_ENABLED, true),
    Key::bool(k::ALWAYS_ON_TOP, false),
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
