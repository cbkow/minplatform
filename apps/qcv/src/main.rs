//! `qcv` — Phase 2 step 1: open a file (or an image-sequence frame) → the fused presenter at the display's cadence.
//! `qcv <file> [--sw] [--out auto|sdr|edr|pq] [--white N] [--seconds N] [--stats] [--snap-out path@N] [--verify-pq]
//!      [--identity-chain] [--refresh ms] [--screen i]`. Space = play / pause · F = fullscreen · Esc = quit · drop a file to open it.
use winit31 as winit;
use min_present::{Output, Presenter, Snapshot};
use min_theme::tokens as t;
use qcv_player::{HwDevice, Transport, TransportOptions, Source, Action, Playback};
use min_present::dual::CompareMode;
/// QCView's `ViewportOverlay` catalog context (dual view's strings, canon row 195).
fn trv(s: &str) -> &str { min_i18n::trc("ViewportOverlay", s) }
use std::{collections::HashMap, path::Path, sync::Arc, time::Instant};
use winit::application::ApplicationHandler;
use winit::data_transfer::TypeHint;
use winit::event::{ElementState, WindowEvent};
use winit::event_loop::{ActiveEventLoop, AsyncRequestSerial, ControlFlow, DndAction, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::monitor::Fullscreen;
use winit::window::{Window, WindowAttributes, WindowId};
use winit::dpi::{LogicalSize, PhysicalPosition};
#[cfg(target_os = "macos")] mod mac;

fn pct(v: &mut Vec<f32>, p: f32) -> f32 { if v.is_empty() { return f32::NAN } v.sort_by(|a, b| a.partial_cmp(b).unwrap()); v[((v.len() - 1) as f32 * p / 100.0).round() as usize] }

struct Opts { /// `--dual <B>`: the launch file is A, this is B (canon row 211; the strip's chips are step 3)
    dual: Option<String>, snap_display: Option<(String, u32)>, cut: Option<String>, hover: Option<u32>, seek_gap_ms: u64, seeks: Option<u32>, scrub: Option<(i32, i32, f32)>, scrub_hw: bool, headless: Option<u32>, looping: bool, file: Option<String>, /// every positional path (the window adds them all, loads the first — like a drop)
    files: Vec<String>, sw: bool, out: Output, white: f32, seconds: f32, stats: bool, snap_out: Option<(String, u32)>, verify_pq: bool, identity_chain: bool, refresh_ms: f32, screen: Option<usize>, threads: i32, autoplay: bool }

mod ui_sketch;
mod transport_bar;
mod status_strip;
mod hover_thumbs;
mod settings;
mod settings_panel;
mod inspector_panel;
mod os_actions;
mod menu;
mod single_instance;
mod logging;
mod background_panel;
mod compact_strip;
mod project_panel;
mod playlist;
mod clip_bar;
mod clip_popover;
mod i18n;
mod color;
mod color_model;
mod color_drawer;
mod color_columns;
mod sdr_white;
mod dual_controls;
mod dual_bars;
mod guides;
mod guides_panel;
mod annotate;
mod notes_drawer;
mod capture;
mod thumb_color;
mod notes_export;
mod shortcuts;

/// QCView's catalog contexts for the shell's words (canon row 195): its toasts (WindowManager) and its window (Main).
fn trw(s: &'static str) -> &'static str { min_i18n::trc("qcv::WindowManager", s) }
fn trm(s: &'static str) -> &'static str { min_i18n::trc("Main", s) }
use min_i18n::fill;

struct Gpu { window: Arc<Box<dyn Window>>, presenter: Presenter, /// the minColor pass building off-thread (canon row 204)
    mincolor_rx: Option<std::sync::mpsc::Receiver<Box<dyn min_present::ColorPass + Send>>>, /// the Color drawer (canon row 199)
    drawer: color_drawer::ColorDrawer, /// dual view's controls on the status strip + the Save dialog + the seam's grab (row 173)
    dualc: dual_controls::DualControls, dual_dialog: min_ui::Dialog, seam_hot: bool, seam_drag: bool, /// the annotation tool's button is down
    annot_down: bool, /// dual's A / B lanes (row 212) + the side the precision popover edits
    dual_bars: dual_bars::DualBars, popover_side: Option<qcv_player::Side>, /// the Notes drawer (row 215; the colour drawer's slot)
    notes: notes_drawer::NotesDrawer, notes_current: Option<String>, /// the Inspector's subject last synced (slot, item): the Transfer chips refresh on a change
    insp_seen: (usize, String), /// the pointer's last movement (the floating tabs fade when it parks over the picture)
    last_move: Instant, /// the SDR white gain's last check (row 184; re-read every 2 s: the window moves, the slider changes)
    sdr_checked: Option<Instant>, /// the waveform's peak for the loaded clip (QCView: reset on every load)
    /// each picture's peak since its clip loaded (0 = the single view / dual A, 1 = dual B)
    clip_peak: [Option<[f32; 2]>; 2], ui: Option<min_ui::UiLayer>, fonts: min_ui::Fonts, ui_ms: Vec<f32>, ui_shown_frame: Option<(i32, i32)>, bar: transport_bar::TransportBar, strip: status_strip::StatusStrip, scrubber: min_ui::Scrubber, thumbs: hover_thumbs::HoverThumbs, /// dual's B side (A uses `thumbs`)
    thumbs_b: hover_thumbs::HoverThumbs, pointer: (f64, f64), ui_dirty: bool, /// the sequence cache's band status last painted (behind, ahead, failed, wraps)
    seq_status: Option<(i32, i32, usize, bool)>,
    /// Fullscreen: the picture alone; the bands slide in (1) from the bottom edge while the pointer rests there (canon §25 row 171).
    fullscreen: bool, bands: min_ui::anim::Ramp, bands_leave: Option<Instant>,
    /// the playlist's clip bar (canon row 167), shown while a playlist is loaded
    clip_bar: clip_bar::ClipBar, clip_popover: clip_popover::ClipPopover,
    /// QCView's single outcome voice (Toast.qml)
    toast: min_ui::Toast,
    /// Compact mode (canon row 172; QCView's chromeless + CompactStrip): the picture and one 32-px band, no title bar.
    compact: bool, compact_strip: compact_strip::CompactStrip, pinned: bool, /// View ▸ Status Bar (Ctrl+5) / Minimal Mode hid the strip
    strip_hidden: bool,
    waveform: qcv_player::WaveformProbeEngine, wave_seen: u64, wave_req: (String, f64, f64, usize), wave_peaks: Vec<(f32, f32)>,
    /// The rails (canon §16.3 + the 2026-11-07 rails ruling): left = Settings (QCView's section), right = the Inspector; both hidden in fullscreen.
    left: min_ui::Rail, right: min_ui::Rail, settings_panel: settings_panel::SettingsPanel, background_panel: background_panel::BackgroundPanel, guides_panel: guides_panel::GuidesPanel, /// the shown safety guide, parsed once per choice
    guide: Option<(String, guides::Guide)>, /// the annotated thumbnail's offscreen vello layer (built on first use)
    thumb_layer: Option<min_ui::UiLayer>, project_panel: project_panel::ProjectPanel, title_shown: String, inspector: inspector_panel::InspectorPanel, /// the Keyboard Shortcuts card under it (row 218) and a pending Ctrl+/ scroll
    shortcuts: shortcuts::ShortcutsPanel, shortcuts_reveal: bool, /// the language generation the labels were built in (row 197's live switching)
    lang_gen: u64, content_set: Option<(u32, u32, u32, u32)>, waveform_enabled: bool, cursor: winit::cursor::CursorIcon,
    /// QCView's in-window menu bar (Windows; row 194) + its About modal
    menu_bar: Option<menu::InWindowMenu>, about: min_ui::Dialog }

struct App {
    o: Opts, gpu: Option<Gpu>, player: Option<Transport>, /// dual view (row 211): set ⇒ `player` is None and the presenter's sides draw A / B
    dual: Option<qcv_player::DualTransport>, hw: Option<HwDevice>, hw_intra: Option<HwDevice>, source_label: String, source_path: String,
    t0: Option<Instant>, t_open: Option<Instant>, first_presented: bool,
    scrub_last_req: Option<(i32, Instant)>,
    /// the loaded playlist's timeline model + its undo (qcv-timeline's Editor); None when a single file is loaded
    pl_editor: Option<qcv_timeline::Editor>,
    /// QCView reportUnsupportedMedia: one report per item per load (the camera-named line may replace a generic one once)
    unsupported_toast: Option<(String, bool)>,
    last_tick: Instant,
    presents: Vec<f32>, last_present: Option<Instant>, show_ms: Vec<f32>, last_report: Instant, shown_in_window: u32,
    fetches: HashMap<AsyncRequestSerial, DropTarget>, /// the last OS-drag position over the window (physical px)
    drag_pos: Option<(f64, f64)>, mods: qcv_player::Mods, finished: bool, frames_presented: u64, edr_seen: f64, /// the window's screen shows HDR (None = unknown) — the Color drawer's "highlights clip" pill (row 222)
    pub(crate) screen_hdr: Option<bool>, snapped: bool, verified: bool,
    settings: min_settings::Settings,
    /// qcv-meta's service: the extractor + the XMP reader run off-thread when a file opens; the Inspector drains the results.
    meta: qcv_meta::MetadataService,
    /// QCView's project (row 141): the media pool, the bins, the per-clip choices; `loaded_item` = the media id in the player.
    project: qcv_project::ProjectManager, loaded_item: String, /// the saved Dual View this session came from (QCView `activeDualViewId`: Update overwrites it)
    active_dual_view: String, /// dual view's edit model: tracks A / B at the master rate (row 212); None outside dual
    dual_editor: Option<qcv_timeline::Editor>, /// the loaded clip's notes + the drawing tools (row 215)
    annot: annotate::Annotations,
    /// the native menu bar (row 143)
    #[cfg(target_os = "macos")] menu: Option<menu::AppMenu>,
    /// Alt went down with nothing else since: its release alone focuses the in-window bar (Windows' Alt tap)
    alt_clean: bool,
    /// the in-window bar's choices, run from `about_to_wait` (Quit needs the event loop)
    menu_queue: Vec<menu::Cmd>,
    /// single instance (row 143): what later launches forwarded
    single_rx: Option<std::sync::mpsc::Receiver<Vec<String>>>,
    /// colour (canon rows 198–208): the user's presets; the shared minColor pass (and its build); what the slot holds; an OCIO build in flight
    colors: color_model::Library, /// per colour SLOT (0 = the single view / dual A, 1 = dual B): the minColor pass, its build in flight
    /// screenshots being encoded on workers (row 216): the format's name on success
    capture_rx: Vec<std::sync::mpsc::Receiver<Option<&'static str>>>,
    /// the thumbnails' look: the clip's chain on our own small engine (row 216)
    thumb_color: [thumb_color::ThumbColor; 2],
    /// notes exports on workers (row 217): the written path, or the error
    export_rx: Vec<std::sync::mpsc::Receiver<Result<String, String>>>,
    mincolor: [Option<std::rc::Rc<std::cell::RefCell<qcv_color::MinColorPass>>>; 2], mincolor_ready: [Option<std::sync::mpsc::Receiver<qcv_color::MinColorPass>>; 2],
    /// the OCIO configs the drawer has opened, by key
    ocio_configs: HashMap<String, std::sync::Arc<qcv_color::ocio::Config>>,
    color_bound: [Option<String>; 2], ocio_rx: [Option<(String, std::sync::mpsc::Receiver<Box<dyn min_present::ColorPass + Send>>)>; 2],
}

impl App {
    /// A path from the command line, a drop or the Open dialog: QCView's `addMediaFile` (the extension gate, dedupe,
    /// routing to its bin, the metadata requests) and then the load of that item. Returns the item id.
    fn drop_target(&self, x: f64, y: f64) -> Option<DropTarget> {
        let g = self.gpu.as_ref()?;
        if g.fullscreen || g.compact { return Some(DropTarget::Viewport) }
        if g.left.hit(x, y) { return Some(DropTarget::Bin) }
        if g.right.hit(x, y) { return None }
        if self.pl_editor.is_none() { if let Some(s) = g.dualc.chip_at(x, y) { return Some(DropTarget::Side(s)) } }
        if self.pl_editor.is_some() && (g.clip_bar.hit(x, y) || g.scrubber.hit(x, y)) { return Some(DropTarget::Playlist(x.round() as i32)) }
        let (cx, cy, cw, ch) = g.presenter.engine.content_rect()?;
        let inside = x >= cx as f64 && x < (cx + cw) as f64 && y >= cy as f64 && y < (cy + ch) as f64;
        if inside && self.dual.is_some() { return Some(DropTarget::Side(self.side_at(x))) }
        inside.then_some(DropTarget::Viewport)
    }
    fn add_and_load(&mut self, path: &str, load: bool) -> Option<String> {
        let (id, probes) = self.project.add_media_file(path);
        self.schedule_probes(probes);
        let id = id?;
        // QCView `populateExrImportMetadata`: a new EXR sequence's layers + compression from its first frame's header
        if let Some(m) = self.project.item(&id) && m.media_type == qcv_project::MediaType::ImageSequence && m.image_seq.format == "EXR" && m.image_seq.available_layers.is_empty() {
            let first = m.path.clone();
            let layers = qcv_player::min_image::exr::layers(&first).unwrap_or_default();
            log::info!("project: EXR '{first}' layers ({}): [{}]", layers.len(), layers.join(", "));
            let comp = qcv_player::min_image::exr::compression(&first).unwrap_or("");
            self.project.set_exr_import_metadata(&id, layers, comp);
        }
        self.note_recent_media(path);
        if load { self.load_item(&id); }
        Some(id)
    }
    fn schedule_probes(&self, probes: Vec<qcv_project::Probe>) {
        for p in probes { match p {
            qcv_project::Probe::Rotation(id, path) => self.meta.request_rotation_probe(&id, &path),
            qcv_project::Probe::Container(id, path) => self.meta.request_container_probe(&id, &path),
            qcv_project::Probe::FullVideo(id, path) => self.meta.request_video_extraction(&id, &path),
            qcv_project::Probe::Adobe(id, path) => self.meta.request_adobe_extraction(&id, &path),
        } }
    }
    /// QCView's `loadRequested` handler: close the current media, open by type, then apply the item's per-clip choices
    /// (range → the decoders, aspect + rotation → the presenter, routing → the audio). Sequences, playlists, dual
    /// views and live streams arrive with their steps (2c, Phase 4): they select but do not load yet.
    fn load_item(&mut self, id: &str) {
        let Some(item) = self.project.item(id).cloned() else { return };
        if item.media_type == qcv_project::MediaType::DualPair { self.load_dual_view(id); return }
        // in dual (QCView `loadRequested`): a video / sequence becomes A in place; anything else falls back to the single view
        if self.dual.is_some() {
            if matches!(item.media_type, qcv_project::MediaType::Video | qcv_project::MediaType::ImageSequence) {
                let info = self.dual_media(id);
                let ok = self.dual.as_mut().map(|d| d.set_side(qcv_player::Side::A, info).is_ok()).unwrap_or(false);
                if ok { self.dual_side_changed(qcv_player::Side::A); }
                self.project.set_load_failed(id, !ok);
                if ok { self.project.set_active_item(id); self.loaded_item = id.to_string(); if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; } }
                return
            }
            self.leave_dual();
        }
        self.project.set_active_item(id);
        match item.media_type {
            qcv_project::MediaType::Video | qcv_project::MediaType::Audio => {}
            qcv_project::MediaType::Playlist => { let ok = self.load_playlist(id); self.project.set_load_failed(id, !ok); if ok { self.loaded_item = id.to_string(); } if let Some(g) = self.gpu.as_mut() { g.content_set = None; g.ui_dirty = true; } return }
            qcv_project::MediaType::ImageSequence => {}
            other => { log::info!("load: {other:?} items load with their step; selected only"); return }
        }
        let ok = if item.media_type == qcv_project::MediaType::ImageSequence {
            // QCView `loadImageSequence`: the item's own pattern, frame range and rate (the detector's, at add) — a still is a 1-frame sequence
            let q = &item.image_seq;
            let pattern = std::path::Path::new(&q.directory).join(&q.pattern).to_string_lossy().into_owned();
            let label = format!("{} ({} frames{})", item.name, q.frame_count, if q.missing_frames.is_empty() { String::new() } else { format!(", {} missing", q.missing_frames.len()) });
            self.open_source_layer(Source::Sequence { pattern, rate: qcv_timeline::Rate::from_f64(if q.frame_rate > 0.0 { q.frame_rate } else { 24.0 }), start: q.start_frame as i64, end: q.end_frame as i64, label }, &item.path, &q.layer)
        } else { self.open(&item.path) };
        self.project.set_load_failed(id, !ok);
        if !ok { self.report_unsupported(id); return }
        if self.unsupported_toast.as_ref().is_some_and(|(i, _)| i != id) { self.unsupported_toast = None; }
        self.loaded_item = id.to_string();
        if let Some(p) = self.player.as_mut() {
            p.set_range_override(match item.video_range_override { qcv_project::VideoRange::Full => qcv_player::RangeOverride::Full, qcv_project::VideoRange::Limited => qcv_player::RangeOverride::Limited, _ => qcv_player::RangeOverride::Auto });
            if let Some(s) = p.decoder().as_sequence() { s.cache().set_stride(item.image_seq.cache_stride); }   // QCView `startImageSequence` applies the item's stride
            p.audio.set_routing_mode(match item.audio_routing_mode { qcv_project::AudioRoutingMode::Downmix5_1 => qcv_player::RoutingMode::Downmix5_1, qcv_project::AudioRoutingMode::Stereo7_8 => qcv_player::RoutingMode::Stereo7_8, _ => qcv_player::RoutingMode::Auto });
            // QCView's handler ends with seekToFrame(0): the first frame the decoder produced carried Auto, and the player opens paused — re-decode it with the item's range in force
            p.seek_to_frame(0);
            // QCView `startImageSequence`: the detector's gaps seed the failed set, so the timeline's red ticks show before any IO
            if let Some(s) = p.decoder().as_sequence() { s.cache().set_known_missing(&item.image_seq.missing_frames); }
        }
        if let Some(g) = self.gpu.as_mut() {
            let par = match item.pixel_aspect_mode { qcv_project::PixelAspectMode::Square => min_present::engine::Par::Square, qcv_project::PixelAspectMode::Detected => min_present::engine::Par::Detected, qcv_project::PixelAspectMode::Custom => min_present::engine::Par::Custom(item.custom_par_num.max(1) as u32, item.custom_par_den.max(1) as u32) };
            let rotation = if item.rotation_override >= 0 { Some(item.rotation_override as u16) } else { None };
            g.presenter.engine.set_transfer_override(transfer_choice(item.transfer_override)); g.inspector.set_transfer(item.transfer_override);
            g.presenter.engine.set_geometry(min_present::engine::Geometry { par, rotation }); g.content_set = None; g.ui_dirty = true;
        }
        self.apply_color();   // the clip's chain (or the project default) into the slot (row 205)
        if let Some(g) = self.gpu.as_mut() { g.clip_peak = [None, None]; g.presenter.engine.reset_scope_peaks(); }   // QCView: the peak readout resets per load
    }
    fn note_recent_media(&mut self, path: &str) { let mut list: Vec<String> = self.settings.string(settings::k::RECENT_MEDIA).lines().filter(|l| !l.is_empty()).map(str::to_string).collect(); list.retain(|p| p != path); list.insert(0, path.to_string()); list.truncate(10); self.settings.set_str(settings::k::RECENT_MEDIA, &list.join("\n")); }
    fn note_recent_project(&mut self, path: &str) { let mut list: Vec<String> = self.settings.string(settings::k::RECENT_PROJECTS).lines().filter(|l| !l.is_empty()).map(str::to_string).collect(); list.retain(|p| p != path); list.insert(0, path.to_string()); list.truncate(10); self.settings.set_str(settings::k::RECENT_PROJECTS, &list.join("\n")); }
    /// QCView's media file filters (`mediaext::nameFilters`).
    fn toast(&mut self, text: &str, kind: min_ui::ToastKind) { if let Some(g) = self.gpu.as_mut() { g.toast.show(text, kind, Instant::now()); g.ui_dirty = true; } }
    fn toast_action(&mut self, text: &str, kind: min_ui::ToastKind, label: &str, id: &str) { if let Some(g) = self.gpu.as_mut() { g.toast.show_action(text, kind, label, id, Instant::now()); g.ui_dirty = true; } }
    /// QCView `invokeToastAction`.
    fn toast_action_invoked(&mut self, id: &str) {
        if id == "undo-media-delete" {
            let names = self.project.restore_last_removed();
            match names.len() { 0 => self.toast(trw("Nothing to restore"), min_ui::ToastKind::Notice), 1 => self.toast(&fill(trw("Restored %1"), &[&names[0]]), min_ui::ToastKind::Positive), n => self.toast(&fill(trw("Restored %1 items"), &[&n]), min_ui::ToastKind::Positive) }
        }
    }
    /// QCView `admitMediaPaths`: the file-type gate on every way media comes in (open, drops, the command line). Refused paths
    /// are toasted once — one line for a single path, a summary by suffix for a batch. (Windows shortcuts resolve in QCView;
    /// `.lnk` resolution is still owed here and they are refused like any unknown suffix.)
    fn admit_media_paths(&mut self, paths: &[String]) -> Vec<String> {
        let mut admitted = vec![]; let mut refused: Vec<String> = vec![]; let mut dead_shortcut = false;
        for raw in paths {
            if raw.is_empty() { continue }
            // a Windows shortcut stands in for its target (QCView: the admitted list carries the target); a dead one has its own toast
            let p = match os_actions::resolve_shortcut(raw) { Some(t) => t, None => { refused.push(raw.clone()); dead_shortcut = true; continue } };
            if p.contains("://") || qcv_project::extensions::is_supported_media_file(&p) { admitted.push(p) } else { refused.push(p) }
        }
        if refused.is_empty() { return admitted }
        let name = |p: &str| std::path::Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.to_string());
        if refused.len() == 1 && paths.len() == 1 {
            let p = &refused[0];
            let msg = if dead_shortcut { fill(trw("%1 points to a file that isn’t there any more."), &[&name(p)]) } else if std::path::Path::new(p).is_dir() { fill(trw("%1 is a folder — drop a file, or any frame of an image sequence."), &[&name(p)]) } else { fill(trw("%1 isn’t a file type QCView opens."), &[&name(p)]) };
            self.toast(&msg, min_ui::ToastKind::Notice);
        } else {
            let mut kinds: Vec<String> = vec![];
            for p in &refused { let k = if std::path::Path::new(p).is_dir() { trw("folder").to_string() } else { match qcv_project::extensions::suffix_lower(p) { s if s.is_empty() => trw("no extension").into(), s => format!(".{s}") } }; if !kinds.contains(&k) { kinds.push(k); } }
            let mut shown = kinds.iter().take(4).cloned().collect::<Vec<_>>().join(", "); if kinds.len() > 4 { shown.push_str(" …"); }
            self.toast(&fill(trw("%1 of %2 files skipped — not file types QCView opens (%3)."), &[&refused.len(), &paths.len(), &shown]), min_ui::ToastKind::Notice);
        }
        admitted
    }
    /// QCView `reportUnsupportedMedia`: the row badge + one toast per item per load (ARRIRAW gets the camera-named line).
    fn report_unsupported(&mut self, id: &str) {
        self.project.set_load_failed(id, true);
        let Some(item) = self.project.item(id).cloned() else { return };
        let v = &item.video;
        let named = v.camera_vendor.eq_ignore_ascii_case("ARRI") || v.camera_model.to_uppercase().starts_with("ALEXA") || v.camera_model.to_uppercase().starts_with("AMIRA");
        if self.unsupported_toast.as_ref().is_some_and(|(i, n)| i == id && (*n || !named)) { return }
        self.unsupported_toast = Some((id.to_string(), named));
        let name = std::path::Path::new(&item.path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let text = if named { let cam = if v.camera_model.is_empty() { "ARRIRAW".to_string() } else { format!("{} ARRIRAW", v.camera_model) }; fill(trw("%1 — %2 can’t be played here: un-debayered camera raw. Review the ProRes / dailies, or convert in DaVinci Resolve or the ARRI Reference Tool."), &[&cam, &name]) }
                   else { fill(trw("%1 can’t be played here: unsupported format."), &[&name]) };
        self.toast(&text, min_ui::ToastKind::Notice);
    }
    fn open_media_dialog(&mut self) {
        let all: Vec<&str> = qcv_project::extensions::VIDEO.iter().chain(qcv_project::extensions::AUDIO).chain(qcv_project::extensions::IMAGE).copied().collect();
        let picked = rfd::FileDialog::new().set_title(trm("Open Media")).add_filter(trm("All media"), &all).add_filter(trm("Video files"), qcv_project::extensions::VIDEO).add_filter(trm("Audio files"), qcv_project::extensions::AUDIO).add_filter(trm("Image / sequence files"), qcv_project::extensions::IMAGE).pick_files();
        let Some(paths) = picked else { return };
        let paths: Vec<String> = paths.iter().map(|p| p.to_string_lossy().to_string()).collect();
        let mut first = true;
        for p in self.admit_media_paths(&paths) { if self.add_and_load(&p, first).is_some() { first = false; } }
    }
    fn open_project_dialog(&mut self) { if let Some(p) = rfd::FileDialog::new().set_title(trm("Open Project")).add_filter(trm("QCView Project"), &["qcvproj"]).pick_file() { self.open_project(&p.to_string_lossy()); } }
    fn open_project(&mut self, path: &str) {
        match self.project.open_project(path) {
            Ok(probes) => { self.settings.set_bool(settings::k::COLOR_ENABLED, false); self.color_on_for_project(); self.schedule_probes(probes); self.note_recent_project(path); self.player = None; self.loaded_item.clear(); let active = self.project.active_item_id().to_string(); if !active.is_empty() { self.load_item(&active); } if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; } }
            Err(e) => log::warn!("{e}"),
        }
    }
    /// QCView `projectSave`: to the file it came from, else Save As.
    fn save_project(&mut self) { if self.project.file_path().is_empty() { self.save_project_as(); } else { let p = self.project.file_path().to_string(); match self.project.save_project(&p) { Ok(()) => self.note_recent_project(&p), Err(e) => log::warn!("{e}") } } }
    fn save_project_as(&mut self) {
        let suggested = if self.project.file_path().is_empty() { format!("{}.qcvproj", self.project.name()) } else { Path::new(self.project.file_path()).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default() };
        if let Some(p) = rfd::FileDialog::new().set_title(trm("Save Project As")).add_filter(trm("QCView Project"), &["qcvproj"]).set_file_name(&suggested).save_file() {
            let mut p = p.to_string_lossy().to_string(); if !p.to_lowercase().ends_with(".qcvproj") { p.push_str(".qcvproj"); }
            match self.project.save_project(&p) { Ok(()) => self.note_recent_project(&p), Err(e) => log::warn!("{e}") }
        }
    }
    fn new_project(&mut self) { self.settings.set_bool(settings::k::COLOR_ENABLED, false); self.project.new_project(); self.player = None; self.loaded_item.clear(); if let Some(g) = self.gpu.as_mut() { g.presenter.engine.set_geometry(Default::default()); g.ui_dirty = true; } self.apply_color(); }
    /// Open the file in the Transport (the raw open; the project paths above call it). True when it opened.
    /// The transport options every open uses — QCView's performance settings apply at open: threads, hardware decode (its
    /// row is Windows-only), the scrub cache budget and the inter-codec hw-scrub gate.
    fn transport_options(&self) -> TransportOptions {
        let st = &self.settings;
        let hw_ok = !cfg!(windows) || st.bool(settings::k::HW_DECODE);
        let hw = if self.o.sw || !hw_ok { None } else { self.hw.clone() };
        let threads = if self.o.threads != 0 { self.o.threads } else { st.int(settings::k::FFMPEG_THREADS) as i32 };
        let scrub = Some(qcv_player::ScrubOptions { budget_bytes: (st.int(settings::k::SCRUB_CACHE_MB).max(1) as usize) << 20, hw_inter_scrub: st.bool(settings::k::HW_SCRUB_INTER), stats: *TRACE_SCRUB, ..Default::default() });
        let loop_enabled = st.bool(settings::k::LOOP_ENABLED) || self.o.looping;   // the store's loop (QCView default ON); `--loop` forces it for the harness
        let seq = qcv_player::image_sequence_cache::StreamingConfig::from_settings(st.int(settings::k::IMAGE_SEQ_THREADS) as i32, st.int(settings::k::IMAGE_SEQ_READ_AHEAD) as i32);   // QCView reads them at each sequence load
        TransportOptions { hw, hw_intra: if self.o.sw { None } else { self.hw_intra.clone() }, threads, loop_enabled, refresh_ms: self.o.refresh_ms, scrub, seq, uploader: self.gpu.as_ref().map(|g| g.presenter.engine.uploader()), ..Default::default() }
    }
    // ---- dual view (canon rows 173 / 211) ---------------------------------------------------------------------------
    /// `--dual <B>` (the harness / a manual launch): B joins the project as its B source, then QCV_DUAL_MODE's mode (side by side).
    fn enter_dual(&mut self, b: &str) {
        let admitted = self.admit_media_paths(&[b.to_string()]);
        if let Some(p) = admitted.first() { if let Some(id) = self.add_and_load(p, false) { self.project.set_b_source_media_id(&id); } }
        let mode = match std::env::var("QCV_DUAL_MODE").as_deref() { Ok("split") => CompareMode::Split, Ok("diff") => CompareMode::Difference, _ => CompareMode::SideBySide };
        self.set_compare_mode(mode);
        if self.o.autoplay { if let Some(d) = self.dual.as_mut() { d.play(); } }
    }
    /// A project item as a dual side: a video probed, a sequence from the item (its own rate — row 211 b); anything else is not a side.
    fn dual_media(&self, id: &str) -> Option<qcv_timeline::MediaInfo> {
        let m = self.project.item(id)?;
        match m.media_type {
            qcv_project::MediaType::Video => qcv_player::dual::media_info(&m.path).ok().map(|mut i| { i.id = id.to_string(); i.name = m.name.clone(); i }),
            qcv_project::MediaType::ImageSequence => {
                let q = &m.image_seq;
                let pattern = std::path::Path::new(&q.directory).join(&q.pattern).to_string_lossy().into_owned();
                Some(qcv_timeline::MediaInfo { id: id.to_string(), name: m.name.clone(), path: m.path.clone(), kind: qcv_timeline::ClipMediaKind::ImageSequence, rate: qcv_timeline::Rate::from_f64(if q.frame_rate > 0.0 { q.frame_rate } else { 24.0 }), duration: q.frame_count as i64, width: q.width.max(0) as u32, height: q.height.max(0) as u32, has_audio: false, sequence_pattern: pattern, sequence_start_frame: q.start_frame as i64, sequence_end_frame: q.end_frame as i64, sequence_exr_layer: q.layer.clone(), sequence_missing: q.missing_frames.clone() })
            }
            _ => None,
        }
    }
    /// QCView `setCompositorMode`: Single leaves dual; any other mode enters it (A = the loaded clip, B = the project's B
    /// source — either may be empty: that side is blank and a drop target) or just switches the presenter's mode.
    fn set_compare_mode(&mut self, mode: CompareMode) {
        if mode == CompareMode::Single { self.leave_dual(); return }
        if self.gpu.is_none() { return }
        if self.dual.is_none() {
            if self.pl_editor.is_some() { return }   // a playlist: QCView hides the modes (dualUnavailable)
            let a = (!self.loaded_item.is_empty()).then(|| self.dual_media(&self.loaded_item.clone())).flatten();
            let b_id = self.project.b_source_media_id().to_string();
            let b = (!b_id.is_empty()).then(|| self.dual_media(&b_id)).flatten();
            let sec = self.player.as_ref().map(|p| p.current_frame() as f64 / p.rate().as_f64().max(1e-9)).unwrap_or(0.0);
            match qcv_player::DualTransport::open(a, b, self.transport_options()) {
                Ok(mut d) => {
                    if let Some(p) = self.player.take() { p.retire(); }
                    // each side its own audio player (row 211 step 5): A heard, B muted by default (QCView's DualAudioMixer)
                    if std::env::var_os("QCV_NO_AUDIO").is_none() { for side in [qcv_player::Side::A, qcv_player::Side::B] { if let Ok(au) = qcv_player::AudioPlayer::new() { d.set_side_audio(side, Box::new(au)); } } }
                    let st = &self.settings; let (vol, mute, sync, scrub) = (st.float(settings::k::VOLUME) as f32, st.bool(settings::k::MUTED), st.int(settings::k::DUAL_SYNC_OFFSET_MS) as i32, st.bool(settings::k::SCRUB_MUTED));
                    d.for_each_audio(|au| { au.set_volume(vol); au.set_muted(mute); au.set_sync_offset_ms(sync); au.set_scrub_muted(scrub); });
                    let f = (sec * d.rate().as_f64()).round() as i32; d.seek_to_frame(f);   // the single view's time carries into A
                    println!("dual: master {:.3} fps, {} frames", d.rate().as_f64(), d.frame_count());
                    let mut ed = qcv_timeline::Editor::default(); ed.load_dual(d.media(qcv_player::Side::A), d.media(qcv_player::Side::B), d.rate());
                    self.dual_editor = Some(ed);
                    if let Some(g) = self.gpu.as_mut() { g.dual_bars.selected = None; g.dual_bars.target = qcv_player::Side::B; if self.mincolor[1].is_none() && self.mincolor_ready[1].is_none() && !color::harness_active() { self.mincolor_ready[1] = Some(color::mincolor_ahead(&g.presenter.engine.device, &g.presenter.engine.queue)); } }
                    self.color_bound = [None, None]; self.ocio_rx = [None, None];
                    self.dual = Some(d); self.close_notes(); self.annot.load(None);   // annotations: single view only (row 215)
                    if let Some(g) = self.gpu.as_mut() { g.presenter.engine.set_dual(true); }
                }
                Err(e) => { println!("dual: {e}"); self.toast(&fill(trw("Couldn’t open: %1"), &[&e]), min_ui::ToastKind::Negative); return }
            }
        }
        if let Some(g) = self.gpu.as_mut() { if let Some(ed) = g.presenter.engine.dual_mut() { ed.mode = mode; } g.presenter.engine.relayout_dual(); g.ui_dirty = true; }
        for side in [qcv_player::Side::A, qcv_player::Side::B] { self.apply_item_to_side(side); }
        self.apply_color();   // each side's own chain on its engine (row 205)
    }
    /// QCView's Single: the dual session ends, A reloads as the single view at the same time.
    fn leave_dual(&mut self) {
        let Some(d) = self.dual.take() else { return };
        let sec = d.current_frame() as f64 / d.rate().as_f64().max(1e-9);
        d.retire();
        self.active_dual_view.clear(); self.dual_editor = None; self.color_bound = [None, None]; self.ocio_rx = [None, None];
        if let Some(g) = self.gpu.as_mut() { if g.popover_side.take().is_some() { g.clip_popover.close(); } }
        if let Some(g) = self.gpu.as_mut() { g.presenter.engine.set_dual(false); g.presenter.engine.set_color_pass(None); g.seam_hot = false; g.seam_drag = false; g.ui_dirty = true; }   // the single view rebinds its chain on reload
        let id = self.loaded_item.clone();
        if !id.is_empty() { self.load_item(&id); if let Some(p) = self.player.as_mut() { let f = (sec * p.rate().as_f64()).round() as i32; p.seek_to_frame(f); } }
    }
    /// QCView `setBSource` (in place here — row 211 c): audio-only refused.
    fn set_b_source(&mut self, id: &str) -> bool {
        let info = self.dual_media(id);
        if info.is_none() { self.toast(trv("Couldn't load that file as Source B"), min_ui::ToastKind::Notice); return false }
        self.project.set_b_source_media_id(id);
        if let Some(d) = self.dual.as_mut() { if let Err(e) = d.set_side(qcv_player::Side::B, info) { println!("dual B: {e}"); self.toast(trv("Couldn't load that file as Source B"), min_ui::ToastKind::Notice); return false } }
        self.dual_side_changed(qcv_player::Side::B);
        if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; }
        true
    }
    /// QCView `clearBSource`: B blank, dual stays.
    fn clear_b_source(&mut self) {
        self.project.set_b_source_media_id("");
        if let Some(d) = self.dual.as_mut() { let _ = d.set_side(qcv_player::Side::B, None); }
        self.dual_side_changed(qcv_player::Side::B);
        if let Some(g) = self.gpu.as_mut() { if let Some(ed) = g.presenter.engine.dual_mut() { ed.b.clear_frame(); } g.ui_dirty = true; }
    }
    /// A drop on a side (a chip, or the picture by mode): B takes the first path, the rest go to the bins; A loads the first.
    fn drop_on_side(&mut self, side: qcv_player::Side, paths: Vec<String>) {
        let mut first = true;
        for p in paths {
            if first && side == qcv_player::Side::B { if let Some(id) = self.add_and_load(&p, false) { self.set_b_source(&id); first = false; } continue }
            if self.add_and_load(&p, first).is_some() { first = false; }
        }
    }
    /// Which side a point in the picture belongs to (QCView `dropMediaAt`: SBS halves, the wipe's seam, Single / Difference = A).
    fn side_at(&self, x: f64) -> qcv_player::Side {
        let Some(g) = self.gpu.as_ref() else { return qcv_player::Side::A };
        let Some(ed) = g.presenter.engine.dual() else { return qcv_player::Side::A };
        let (cx, _, cw, _) = g.presenter.engine.content_rect().unwrap_or((0, 0, 1, 1));
        let b = match ed.mode { CompareMode::SideBySide => x >= cx as f64 + cw as f64 / 2.0, CompareMode::Split => x >= ed.seam_x(), _ => false };
        if b { qcv_player::Side::B } else { qcv_player::Side::A }
    }
    /// QCView `loadDualView`: A, then B, side by side, the saved in / out; Update now overwrites this view.
    fn load_dual_view(&mut self, id: &str) {
        let Some(item) = self.project.item(id).cloned() else { return };
        let dp = item.dual_pair;
        self.project.set_b_source_media_id(&dp.media_id_b);
        if self.dual.is_some() {
            let (a, b) = (self.dual_media(&dp.media_id_a), self.dual_media(&dp.media_id_b));
            self.loaded_item = dp.media_id_a.clone(); self.project.set_active_item(&dp.media_id_a);
            if let Some(d) = self.dual.as_mut() { let _ = d.set_side(qcv_player::Side::A, a); let _ = d.set_side(qcv_player::Side::B, b); }
            self.dual_side_changed(qcv_player::Side::A); self.dual_side_changed(qcv_player::Side::B);
            self.set_compare_mode(CompareMode::SideBySide);
        } else {
            if self.project.item(&dp.media_id_a).is_some() { self.loaded_item = dp.media_id_a.clone(); self.project.set_active_item(&dp.media_id_a); }
            self.set_compare_mode(CompareMode::SideBySide);
        }
        // QCView `loadDualView`: the saved clip maps replace each non-empty track (empty = the full clip, as QCView skips them)
        if let (Some(d), Some(ed)) = (self.dual.as_mut(), self.dual_editor.as_mut()) {
            let rate = ed.timeline.rate; let mut any = false;
            for (tr, maps) in [(qcv_timeline::TRACK_A, &dp.clips_a), (qcv_timeline::TRACK_B, &dp.clips_b)] { if !maps.is_empty() { ed.set_track_clips(tr, maps.iter().filter_map(|v| clip_from_qcview(v, rate)).collect()); any = true; } }
            if any { d.set_timeline(&ed.timeline, true); }
            d.set_in_out(dp.in_point, dp.out_point);
        }
        self.active_dual_view = id.to_string();
    }
    fn dual_pair_data(&self) -> Option<qcv_project::DualPairData> {
        let d = self.dual.as_ref()?;
        let fps = d.rate().as_f64();
        // QCView `saveCurrentDualView`: each track's clips as clip maps (seconds), so QCView opens the view with the same edits
        let maps = |tr: &str| self.dual_editor.as_ref().and_then(|ed| ed.timeline.track(tr).map(|t| t.clips.iter().filter(|c| !c.is_gap).map(|c| clip_to_qcview(c, ed.timeline.rate)).collect())).unwrap_or_default();
        Some(qcv_project::DualPairData { media_id_a: self.loaded_item.clone(), media_id_b: self.project.b_source_media_id().to_string(), master_fps: fps, master_duration: d.frame_count() as f64 / fps.max(1e-9), clips_a: maps(qcv_timeline::TRACK_A), clips_b: maps(qcv_timeline::TRACK_B), in_point: d.in_point(), out_point: d.out_point() })
    }
    /// Ctrl+4 / the transport's note button (QCView's Notes panel): the drawer opens in the colour drawer's slot (closing Color),
    /// arming Freehand; closing it leaves annotation mode. Single view on a loaded clip only (row 215).
    fn toggle_notes(&mut self) {
        let open = self.gpu.as_ref().is_some_and(|g| g.notes.open);
        if open { self.close_notes(); return }
        if self.dual.is_some() || self.pl_editor.is_some() || self.player.is_none() || self.annot.media.is_none() { return }
        let now = Instant::now();
        if self.gpu.as_ref().is_some_and(|g| g.drawer.open) { self.color_cmd(color_drawer::Cmd::Toggle); }
        self.annot.active = true; self.annot.tool = annotate::Tool::Freehand;
        if let Some(c) = qcv_notes_color(&self.settings.string(settings::k::ANNOTATION_COLOR)) { self.annot.color = c; }
        if let Some(g) = self.gpu.as_mut() { g.notes.set_open(true, now); g.ui_dirty = true; }
    }
    pub(crate) fn close_notes(&mut self) {
        self.annot.active = false; self.annot.escape(); self.annot.selected = None;
        let pending: Vec<notes_drawer::Cmd> = self.gpu.as_mut().map(|g| g.notes.blur()).unwrap_or_default();   // a note being typed commits first
        for c in pending { if let notes_drawer::Cmd::Text(k, t) = c { self.annot.set_text(&k, &t); } }
        if let Some(g) = self.gpu.as_mut() { if g.notes.open { g.notes.set_open(false, Instant::now()); } g.ui_dirty = true; if g.cursor == winit::cursor::CursorIcon::Crosshair { g.cursor = winit::cursor::CursorIcon::Default; g.window.set_cursor(winit::cursor::Cursor::Icon(g.cursor)); } }
    }
    fn notes_cmd(&mut self, c: notes_drawer::Cmd) {
        use notes_drawer::Cmd as N;
        let (frame, fps) = self.player.as_ref().map(|p| (p.current_frame(), p.rate().as_f64())).unwrap_or((0, 24.0));
        match c {
            N::Export(f) => self.export_notes(f),
            N::AddNote => { if let Some(c) = self.annot.add_note(frame, fps) { self.after_annotation(c); } }
            N::Seek(f) => { if let Some(p) = self.player.as_mut() { p.seek_to_frame(f as i32); } }
            N::Text(k, t) => self.annot.set_text(&k, &t),
            N::Addressed(k, on) => self.annot.set_addressed(&k, on),
            N::Delete(k) => self.annot.delete_note(&k),
            // QCView: clicking the armed tool again disarms it (annotation mode off, the panel stays)
            N::Tool(t) => { match t { Some(t) => { self.annot.tool = t; self.annot.active = true; } None => { self.annot.active = false; self.annot.escape(); } } self.annot.selected = None; }
            N::Color(h) => { if let Some(c) = qcv_notes_color(&h) { self.annot.color = c; self.settings.set_str(settings::k::ANNOTATION_COLOR, &h); } }
            N::Width(w) => self.annot.width = w,
            N::Height(h) => { self.settings.set_float(settings::k::NOTES_DRAWER_HEIGHT, h); }
        }
        if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; }
    }
    /// After a committed edit: a new note's CLEAN frame (once — the picture alone, so no stroke in it), then the annotated
    /// thumbnail redrawn through the SAME geometry as the screen (vello, offscreen) — both QCView needs to show the note.
    fn after_annotation(&mut self, c: annotate::Committed) {
        if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; }
        let Some(media) = self.annot.media.clone() else { return };
        let dir = qcv_notes::io::images_dir(&qcv_notes::io::notes_dir(&media)); let _ = std::fs::create_dir_all(&dir);
        let clean = dir.join(qcv_notes::io::image_filename(&c.key)); let annotated = dir.join(qcv_notes::io::annotated_filename(&c.key));
        // the clean frame: a CAPTURE (row 216 — the clip's chain SDR-retargeted, the viewer aids, pixel aspect + rotation, no
        // strokes, no guide), captured once and never overwritten (QCView); the aids and the chain recorded on the note with it
        let current = self.annot.note_at_frame(self.player.as_ref().map(|p| p.current_frame()).unwrap_or(-1)).is_some_and(|n| n.timecode == c.key);
        if c.new_note && current && !clean.exists() {
            if let Some((px, w, h)) = self.capture_picture(false) {
                if let Err(e) = min_image::write::png8(&clean.to_string_lossy(), &min_present::capture::srgb8(&px), w, h) { log::warn!("note thumbnail: {e}"); }
                let (viewer, ocio) = self.note_tags(); self.annot.set_tags(&c.key, &viewer, &ocio);
            }
        }
        // the annotated one: the clean PNG at the display aspect + the strokes (none left → the file goes, QCView's fallback is the clean one)
        let Some(note) = self.annot.notes.iter().find(|n| n.timecode == c.key).cloned() else { let _ = std::fs::remove_file(&annotated); return };
        let ss = qcv_notes::strokes::parse(note.annotation.as_ref());
        if ss.is_empty() { let _ = std::fs::remove_file(&annotated); return }
        let Ok(img) = image_rgba8(&clean) else { return };
        let Some(g) = self.gpu.as_mut() else { return };
        let (vw, vh) = img.1;
        // the display aspect: a capture is already at it; an older clean frame (QCView's, at the source size) is widened to it
        let (dw, dh) = g.presenter.engine.display_dims();
        let w = if dw > 0 && dh > 0 { ((vh as f64) * dw as f64 / dh as f64).round().max(1.0) as u32 } else { vw.max(1) };
        let h = vh.max(1);
        let dev = g.presenter.engine.device.clone(); let q = g.presenter.engine.queue.clone();
        if g.thumb_layer.is_none() { g.thumb_layer = min_ui::UiLayer::new(&dev, w, h).ok(); }
        let Some(layer) = g.thumb_layer.as_mut() else { return };
        if layer.size() != (w, h) { layer.resize(&dev, w, h); }
        let scene = layer.scene_mut(); scene.reset();
        min_ui::paint::image(scene, &min_ui::paint::image_rgba8(vw, vh, img.0), kurbo::Rect::new(0.0, 0.0, w as f64, h as f64));
        let geo = annotate::Geo { rect: [0.0, 0.0, w as f64, h as f64], px_per_src: w as f64 / dw.max(1) as f64 };
        for s in &ss { annotate::draw(scene, s, &geo); }
        layer.render(&dev, &q);
        let px = min_present::readback::read_rgba8(&dev, &q, layer.texture(), w, h);
        write_png_rgba8(&annotated.to_string_lossy(), &px, w, h);
    }
    /// A side's clip's own choices (QCView applies them per load): Range → its decoders, Aspect / Rotation / Transfer → its
    /// engine, Mix → its audio; and its metadata requested (the Inspector shows the target side's).
    fn apply_item_to_side(&mut self, side: qcv_player::Side) {
        let slot = side as usize; let id = self.slot_item(slot);
        let Some(item) = self.project.item(&id).cloned() else { return };
        if let Some(t) = self.dual.as_mut().and_then(|d| d.side_mut(side)) { t.set_range_override(range_of(&item)); t.audio.set_routing_mode(routing_of(&item)); }
        if let Some(g) = self.gpu.as_mut() { if let Some(en) = color::slot_engine(g, slot, true) { en.set_geometry(min_present::engine::Geometry { par: par_of(&item), rotation: rotation_of(&item) }); en.set_transfer_override(transfer_choice(item.transfer_override)); } g.content_set = None; g.insp_seen = (9, String::new()); }
        if !item.path.is_empty() { self.meta.request_video_extraction(&id, &item.path); self.meta.request_adobe_extraction(&id, &item.path); }
    }
    /// A side's media changed in place: its track becomes the full clip; the master rate re-times the other side's edits.
    fn dual_side_changed(&mut self, side: qcv_player::Side) {
        let (Some(d), Some(ed)) = (self.dual.as_mut(), self.dual_editor.as_mut()) else { return };
        if ed.timeline.rate != d.rate() { ed.retime_master(d.rate()); }
        ed.set_track_media(dual_bars::track(side), d.media(side));
        d.set_timeline(&ed.timeline, false);
        if let Some(g) = self.gpu.as_mut() { if g.dual_bars.selected.as_ref().is_some_and(|(s, _)| *s == side) { g.dual_bars.selected = None; } g.ui_dirty = true; }
        self.apply_item_to_side(side);
        if let Some(g) = self.gpu.as_mut() { g.clip_peak[side as usize] = None; if let Some(d) = g.presenter.engine.dual_mut() { if side == qcv_player::Side::A { d.a.reset_scope_peaks(); } else { d.b.reset_scope_peaks(); } } }   // a side's readout resets with its clip
        self.apply_color();
    }
    /// An edit landed in the dual editor: the transport follows (a split / delete reopens the sides).
    fn after_dual_edit(&mut self, structural: bool) {
        if let (Some(d), Some(ed)) = (self.dual.as_mut(), self.dual_editor.as_mut()) {
            // a growth to the left moved every clip right: the playhead moves with them (it stays on the same picture)
            let grow = std::mem::take(&mut ed.last_grow); let f = d.current_frame() + grow as i32;
            d.set_timeline(&ed.timeline, structural);
            if grow > 0 { d.seek_to_frame(f); }
        }
        if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; }
    }
    fn dual_lane_cmd(&mut self, c: dual_bars::LaneCmd) {
        use dual_bars::{track, LaneCmd};
        let Some(ed) = self.dual_editor.as_mut() else { return };
        let changed = match c {
            LaneCmd::Seek(f) => { if let Some(d) = self.dual.as_mut() { d.seek_to_frame(f as i32); } false }
            LaneCmd::Select(sel) => { if let Some(g) = self.gpu.as_mut() { if sel.is_some() { g.project_panel.selected.clear(); } g.dual_bars.selected = sel; } false }
            LaneCmd::Target(s) => { if let Some(g) = self.gpu.as_mut() { g.dual_bars.target = s; } false }
            LaneCmd::TrimHead { side, clip, new_start, new_source_in } => ed.trim_clip_head(track(side), &clip, new_start, new_source_in),
            LaneCmd::TrimTail { side, clip, new_source_out } => ed.trim_clip_tail(track(side), &clip, new_source_out),
            LaneCmd::Slide { side, clip, delta, carry } => ed.shift_clips(track(side), &clip, delta, carry) != 0,
            LaneCmd::Slip { side, clip, new_in, new_out } => ed.slip_clip(track(side), &clip, new_in, new_out),
            LaneCmd::Precision(side, id) => {
                if let (Some(g), Some(c)) = (self.gpu.as_mut(), ed.timeline.track(track(side)).and_then(|t| t.clip(&id)).cloned()) {
                    let st = scrub_state(pb(&self.dual, &self.player), self.player.as_ref()); let scale = g.window.scale_factor(); let (w, h) = g.window.surface_size().into();
                    let lane_h = g.dual_bars.rect.height() / 2.0; let y0 = g.dual_bars.rect.y0 + if side == qcv_player::Side::B { lane_h } else { 0.0 };
                    let anchor = kurbo::Rect::new(g.scrubber.x_of_frame(c.start, &st, scale), y0, g.scrubber.x_of_frame(c.end(), &st, scale), y0 + lane_h);
                    g.clip_popover.open(&c, ed.timeline.rate, anchor, &mut g.fonts, kurbo::Rect::new(0.0, 0.0, w, h), scale); g.popover_side = Some(side); set_ime(&**g.window, true);
                }
                false
            }
        };
        if changed { self.after_dual_edit(false); }
        if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; }
    }
    /// The clip the dual keys act on (row 212): the selected clip, else the target side's clip under the playhead — or, for
    /// a head trim from a gap, the next clip; for a tail trim, the previous one.
    fn dual_key_clip(&self, prefer: i32) -> Option<(qcv_player::Side, qcv_timeline::Clip)> {
        let (g, ed, d) = (self.gpu.as_ref()?, self.dual_editor.as_ref()?, self.dual.as_ref()?);
        if let Some((s, id)) = &g.dual_bars.selected { if let Some(c) = ed.timeline.track(dual_bars::track(*s)).and_then(|t| t.clip(id)) { return Some((*s, c.clone())) } }
        let side = g.dual_bars.target; let p = d.current_frame() as i64;
        let mut clips: Vec<&qcv_timeline::Clip> = ed.timeline.track(dual_bars::track(side))?.clips.iter().filter(|c| !c.is_gap).collect(); clips.sort_by_key(|c| c.start);
        clips.iter().find(|c| c.contains(p)).or_else(|| if prefer > 0 { clips.iter().find(|c| c.start > p) } else if prefer < 0 { clips.iter().rev().find(|c| c.end() <= p) } else { None }).map(|c| (side, (*c).clone()))
    }
    /// Dual's keys (row 212): `[` / `]` trim to the playhead, ⌘K cuts the target side, Alt+← / → slides (Shift ×10),
    /// Delete leaves a gap / Shift+Delete closes it. Returns true when the key was dual's.
    fn dual_key(&mut self, key: &winit::keyboard::Key) -> bool {
        use winit::keyboard::{Key, NamedKey};
        if self.dual.is_none() || self.dual_editor.is_none() { return false }
        let m = self.mods; let p = self.dual.as_ref().unwrap().current_frame() as i64;
        let ch = |c: &str| matches!(key, Key::Character(k) if k.as_str() == c);
        let (edit, structural) = if !m.ctrl && !m.alt && ch("[") {
            let Some((s, c)) = self.dual_key_clip(1) else { return true };
            let rate = self.dual_editor.as_ref().unwrap().timeline.rate;
            let inn = c.source_in + qcv_timeline::rescale(p - c.start, rate, c.source_rate);
            (p < c.end() && inn >= 0 && self.dual_editor.as_mut().unwrap().trim_clip_head(dual_bars::track(s), &c.id, p, inn), false)
        } else if !m.ctrl && !m.alt && ch("]") {
            let Some((s, c)) = self.dual_key_clip(-1) else { return true };
            let rate = self.dual_editor.as_ref().unwrap().timeline.rate;
            let out = c.source_in + qcv_timeline::rescale(p + 1 - c.start, rate, c.source_rate);   // the playhead's frame stays: the clip's last
            (p >= c.start && out > c.source_in && self.dual_editor.as_mut().unwrap().trim_clip_tail(dual_bars::track(s), &c.id, out), false)
        } else if m.ctrl && !m.shift && (ch("k") || ch("K")) {
            let side = self.gpu.as_ref().map(|g| g.dual_bars.target).unwrap_or(qcv_player::Side::B);
            (self.dual_editor.as_mut().unwrap().split_clip_at(dual_bars::track(side), p), true)
        } else if m.alt && !m.ctrl && matches!(key, Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowRight)) {
            let Some((s, c)) = self.dual_key_clip(0) else { return true };
            let n = if m.shift { 10 } else { 1 } * if matches!(key, Key::Named(NamedKey::ArrowLeft)) { -1 } else { 1 };
            (self.dual_editor.as_mut().unwrap().shift_clips(dual_bars::track(s), &c.id, n, true) != 0, false)
        } else if !m.ctrl && !m.alt && matches!(key, Key::Named(NamedKey::Delete | NamedKey::Backspace)) {
            let Some((s, id)) = self.gpu.as_ref().and_then(|g| g.dual_bars.selected.clone()) else { return false };   // no dual selection: the bins' Delete
            let ed = self.dual_editor.as_mut().unwrap();
            let ok = if m.shift { ed.delete_and_close(dual_bars::track(s), &id) } else { ed.delete_clip(dual_bars::track(s), &id) };
            if ok { if let Some(g) = self.gpu.as_mut() { g.dual_bars.selected = None; } }
            (ok, true)
        } else { return false };
        if edit { self.after_dual_edit(structural); }
        if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; }
        true
    }
    fn dual_cmd(&mut self, c: dual_controls::Cmd) {
        use dual_controls::Cmd;
        match c {
            Cmd::Mode(m) => self.set_compare_mode(m),
            Cmd::ClearB => self.clear_b_source(),
            Cmd::Target(sd) => if let Some(g) = self.gpu.as_mut() { g.dual_bars.target = sd; g.ui_dirty = true; },
            Cmd::ToggleSideMute(sd) => { if let Some(d) = self.dual.as_mut() { let m = !d.side_muted(sd); d.set_side_muted(sd, m); } if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; } }
            Cmd::Seam(v) => if let Some(g) = self.gpu.as_mut() { if let Some(ed) = g.presenter.engine.dual_mut() { ed.seam = v.clamp(0.0, 1.0); } g.ui_dirty = true; },
            Cmd::Gain(v) => if let Some(g) = self.gpu.as_mut() { if let Some(ed) = g.presenter.engine.dual_mut() { ed.gain = v.clamp(1.0, dual_controls::GAIN_MAX); } g.ui_dirty = true; },
            Cmd::Save => {
                // QCView `autoSaveName`: "<A> vs <B>", "(none)" for an empty side
                let name_of = |id: &str| self.project.item(id).map(|m| m.name.clone()).unwrap_or_else(|| trv("(none)").to_string());
                let name = format!("{} vs {}", name_of(&self.loaded_item.clone()), name_of(self.project.b_source_media_id()));
                if let Some(g) = self.gpu.as_mut() { g.dual_dialog.show_with(&name, &mut g.fonts); set_ime(&**g.window, true); g.ui_dirty = true; }
            }
            Cmd::Update => {
                let Some(data) = self.dual_pair_data() else { return };
                let id = self.active_dual_view.clone();
                if !id.is_empty() && self.project.item(&id).is_some() { self.project.set_dual_pair(&id, data); self.toast(trv("Dual view updated"), min_ui::ToastKind::Positive); }
                else { self.toast(trv("Couldn't update dual view"), min_ui::ToastKind::Negative); }
            }
        }
    }
    /// The Save dialog closed with button `i` (1 = Save).
    fn dual_dialog_done(&mut self, i: usize) {
        let Some(g) = self.gpu.as_mut() else { return };
        set_ime(&**g.window, false); g.ui_dirty = true;
        if i != 1 { return }
        let name = g.dual_dialog.value();
        let Some(data) = self.dual_pair_data() else { self.toast(trv("Couldn't save dual view"), min_ui::ToastKind::Negative); return };
        let id = self.project.add_dual_pair(&name, data);
        self.active_dual_view = id;
        self.toast(&fill(trv("Dual view saved: %1"), &[&name]), min_ui::ToastKind::Positive);
    }
    fn open(&mut self, path: &str) -> bool { self.open_source(Source::from_path(path, None), path) }
    fn open_source(&mut self, src: Source, path: &str) -> bool { self.open_source_layer(src, path, "") }
    fn open_source_layer(&mut self, src: Source, path: &str, layer: &str) -> bool {
        if self.gpu.is_none() { return false }
        let t = Instant::now();
        match Transport::open(&src, TransportOptions { seq_layer: layer.to_string(), ..self.transport_options() }) {
            Ok(p) => { self.pl_editor = None; self.adopt(p, src.display(), path, t); true }
            Err(e) => { println!("open {path}: {e}"); false }
        }
    }
    /// A freshly opened transport becomes the player: audio, QCView's paused open, the readouts, the metadata requests.
    fn adopt(&mut self, mut p: Transport, label: String, path: &str, t: Instant) {
        let st = &self.settings;
        // audio first, so the harness's play below starts it too (the player's hooks run inside Transport::play)
        if std::env::var_os("QCV_NO_AUDIO").is_none() { match qcv_player::AudioPlayer::new() { Ok(a) => { p.set_audio(Box::new(a)); println!("audio: {} (device latency {:.1} ms)", if p.audio.has_audio() { "open (CoreAudio default output)" } else { "no audio stream" }, p.audio.device_latency_ms()); } Err(e) => println!("audio: {e}") } }
        if p.audio.has_audio() { p.audio.set_volume(st.float(settings::k::VOLUME) as f32); p.audio.set_muted(st.bool(settings::k::MUTED)); p.audio.set_sync_offset_ms(st.int(settings::k::SYNC_OFFSET_MS) as i32); p.audio.set_scrub_muted(st.bool(settings::k::SCRUB_MUTED)); }
        if self.o.autoplay { p.play(); }   // QCView opens PAUSED (the waveform probe and the thumbnail strip run before the first play); `--play` is the harness's flag
        println!("opened {label} — {}  (open took {:.0} ms)", describe(&p), t.elapsed().as_secs_f32() * 1000.0);
        let g = self.gpu.as_mut().unwrap();
        self.source_label = label; self.source_path = path.to_string(); g.thumbs.on_open(&p); g.presenter.engine.set_geometry(Default::default()); g.presenter.engine.set_transfer_override(Default::default()); g.inspector.set_transfer(Default::default());
        // QCView: both metadata requests at add; keyed by the media id when the file is in the project (the manager caches the result), else by path
        let key = self.project.pool().iter().find(|m| m.path == path).map(|m| m.id.clone()).unwrap_or_else(|| path.to_string());
        if !path.is_empty() { self.meta.request_video_extraction(&key, path); self.meta.request_adobe_extraction(&key, path); }
        self.player = Some(p); self.t_open = Some(t); self.first_presented = false;
        self.annot.load(if path.is_empty() || path.contains("://") { None } else { Some(std::path::Path::new(path)) });   // the clip's notes; undo starts over (row 215)
        g.title_shown.clear();
    }
    /// QCView `startPlaylist`: the entries resolved against the pool into a timeline, opened paused on its first clip; an
    /// empty playlist is loaded with no player (its bar says "Drop media here").
    fn load_playlist(&mut self, id: &str) -> bool {
        let Some(item) = self.project.item(id).cloned() else { return false };
        let ed = playlist::editor_for(&self.project, &item);
        let has_clips = ed.timeline.tracks.first().is_some_and(|t| t.clips.iter().any(|c| !c.is_gap));
        self.player = None;
        if has_clips {
            let t = Instant::now();
            match Transport::open_playlist(ed.timeline.clone(), self.transport_options()) {
                Ok(p) => self.adopt(p, item.name.clone(), "", t),
                Err(e) => { println!("open playlist '{}': {e}", item.name); return false }
            }
        } else { self.source_label = item.name.clone(); self.source_path.clear(); if let Some(g) = self.gpu.as_mut() { g.title_shown.clear(); } }
        println!("playlist '{}': {} clip(s), {} frames", item.name, ed.timeline.tracks.first().map(|t| t.clips.iter().filter(|c| !c.is_gap).count()).unwrap_or(0), ed.timeline.duration);
        self.pl_editor = Some(ed);
        true
    }
    fn tick(&mut self) {
        self.poll_color();
        self.poll_capture();
        let thumb_gen = [self.thumb_color_tick(0), if self.dual.is_some() { self.thumb_color_tick(1) } else { 0 }];
        self.poll_export();
        self.ensure_color_config();
        let color_view = self.color_view();
        if let Some(g) = self.gpu.as_mut() && let Some(rx) = &g.mincolor_rx && let Ok(pass) = rx.try_recv() {
            g.presenter.engine.set_color_pass(Some(pass)); g.mincolor_rx = None; log::info!("colour chain bound to the slot");
        }
        self.last_tick = Instant::now();
        let Some(g) = self.gpu.as_mut() else { return };
        let now = Instant::now();
        tab_presence(g, now);   // a parked pointer fades the tabs with no event to say so
        // the window's screen, every 2 s (canon rows 184 + 220, Windows): HDR on → the fp16 scRGB surface with SDR white lifted to the
        // user's SDR brightness; HDR off → the 10-bit sRGB surface (gain 1). The chain renders as chosen on both — above SDR white
        // clips on an SDR monitor and the user dials it in with the Highlight Knee (the user 2026-11-09: no auto tone mapping).
        if g.presenter.info().output == Output::Edr && g.sdr_checked.is_none_or(|t| now.duration_since(t).as_secs_f32() >= 2.0) {
            g.sdr_checked = Some(now);
            if cfg!(windows) {
                let name = g.window.current_monitor().and_then(|m| m.name().map(|n| n.to_string())).unwrap_or_default();
                let (gain, hdr) = sdr_white::screen_for(&name);
                if let Some(hdr) = hdr && g.presenter.set_sdr_surface(!hdr) { g.ui_dirty = true; }
                if hdr != self.screen_hdr { self.screen_hdr = hdr; g.ui_dirty = true; }
                let gain = if g.presenter.sdr_surface() { 1.0 } else { gain };
                if (gain - g.presenter.engine.output_gain()).abs() > 1e-3 { log::info!("SDR white on {name}: {:.0} nits (gain {gain:.2})", gain * 80.0); g.presenter.engine.set_output_gain(gain); }
            } else if let Some(h) = g.presenter.window_screen_headroom() {
                // macOS: read only — EDR stays as it is (row 220's ruling); the screen's headroom feeds the drawer's pill (row 222)
                let hdr = Some(h > 1.0 + 1e-3); if hdr != self.screen_hdr { self.screen_hdr = hdr; g.ui_dirty = true; }
            }
        }
        let mut shown_new = false; let mut reopen = false;
        if let Some(p) = self.player.as_mut() {
            if *TRACE_SCRUB { if let Some(sd) = p.scrub() { if let Some(rx) = sd.served.clone() { while let Ok(sv) = rx.try_recv() { println!("scrub: served {} at {:.1} ms — request → ready {:.1} ms, decoded {}, hit {}, seeked {}", sv.target, trace_ms(), (sv.done - sv.issued).as_secs_f32() * 1000.0, sv.decoded, sv.hit, sv.seeked); } } } }
            let idx = if *TRACE_SCRUB { Some(p.frame_index().clone()) } else { None };
            let scrub_tick = *TRACE_SCRUB && p.is_scrubbing();
            if scrub_tick { println!("scrub: tick at {:.1} ms — queued {} (front external {}), seek in flight {}, decoder playing {}", trace_ms(), p.decoder().queued(), p.decoder().front_is_external(), p.seek_in_flight(), p.decoder().is_playing()); }
            let ticked = p.tick(now);
            if scrub_tick { println!("scrub: tick → {}", ticked.as_ref().map(|d| if d.frame_no < 0 { "a preview".to_string() } else { format!("stream frame {}", d.frame_no) }).unwrap_or_else(|| "nothing".to_string())); }
            if let Some(d) = ticked {
                if let (true, Some(idx)) = (d.frame_no < 0, idx.as_ref()) { let shown_f = idx.frame_for_pts(d.pts); let (lf, since) = self.scrub_last_req.map(|(f, t)| (f, t.elapsed().as_secs_f32() * 1000.0)).unwrap_or((-1, 0.0)); println!("scrub: shown preview frame {shown_f} at {:.1} ms — latest request {lf} ({} frames behind, {since:.1} ms after it)", trace_ms(), lf - shown_f); }
                match g.presenter.engine.show(&d.frame) {
                    Ok(()) => { shown_new = true; self.show_ms.push(g.presenter.engine.last_show_ms); self.shown_in_window += 1; }
                    Err(e) => { println!("show: {e} — reopening in software"); reopen = true; }
                }
            }
        }
        if let (Some(d), Some(ed)) = (self.dual.as_mut(), g.presenter.engine.dual_mut()) {
            let t_tick = Instant::now();
            let u = d.tick(now);
            if std::env::var_os("QCV_TRACE_DUAL").is_some() { let fa = d.side(qcv_player::Side::A).and_then(|t| t.current()).map(|h| h.frame_no); let fb = d.side(qcv_player::Side::B).and_then(|t| t.current()).map(|h| h.frame_no); println!("dual {:.1} ms: tick {:.2} ms master {} playing {} inflight {} in {} out {} | A {:?} {fa:?} | B {:?} {fb:?}", trace_ms(), t_tick.elapsed().as_secs_f64() * 1000.0, d.current_frame(), d.is_playing(), d.seek_in_flight(), d.in_point(), d.out_point(), u[0], u[1]); }
            for (i, side) in [(0usize, qcv_player::Side::A), (1, qcv_player::Side::B)] {
                let eng = if i == 0 { &mut ed.a } else { &mut ed.b };
                match u[i] {
                    qcv_player::SideUpdate::New => if let Some(h) = d.side(side).and_then(|t| t.current()) { match eng.show(&h.frame) { Ok(()) => shown_new = true, Err(e) => println!("dual {side:?}: show: {e}") } },
                    qcv_player::SideUpdate::Blank => { eng.clear_frame(); shown_new = true; }
                    qcv_player::SideUpdate::Same => {}
                }
            }
        }
        if reopen { self.o.sw = true; self.player = None; let path = self.source_path.clone(); let _ = self.open(&path); return }
        if !shown_new { if let Some(p) = self.player.as_mut() { p.idle_refresh(); } }
        // the notes sidecar rewritten by another app (minNotes, a second QCView): reload (canon §4.5's watcher)
        // (only with the Notes panel open — the user 2026-11-09 — and paused: a stat on a slow mount must never hitch playback;
        // opening the panel polls on its first tick, so edits made while it was shut land then)
        if g.notes.open && !self.player.as_ref().is_some_and(|p| p.is_playing()) && self.annot.poll_disk(Instant::now()) { g.ui_dirty = true; }
        // a language switch (row 197): relabel once per change
        if g.lang_gen != min_i18n::generation() { g.lang_gen = min_i18n::generation(); relabel(g); #[cfg(target_os = "macos")] { let m = menu::AppMenu::new(); m.menu.init_for_nsapp(); self.menu = Some(m); } }
        if let Some(u) = g.ui.as_mut() {
            // the bands repaint when the playhead moves (single: once its first frame landed; dual: the master's frame)
            let frame = match &self.dual { Some(d) => Some((d.current_frame(), d.frame_count(), d.rate().as_f64())), None => self.player.as_ref().and_then(|p| p.current().map(|_| (p.current_frame(), p.frame_count(), p.rate().as_f64()))) };
            let key = frame.map(|(c, n, _)| (c, n));
            g.bar.sync(pb(&self.dual, &self.player), now);
            // the chosen safety guide, parsed once per choice (row 215)
            { let want = self.settings.string(settings::k::SAFETY_GUIDE); if g.guide.as_ref().map(|x| x.0.as_str()) != Some(want.as_str()) { g.guide = (!want.is_empty()).then(|| crate::guides::GUIDES.iter().find(|x| x.0 == want).map(|x| (want.clone(), crate::guides::parse(x.1)))).flatten(); g.ui_dirty = true; } }
            { // dual view's controls (row 173): the loaded clip as A, the project's B source while dual is on
                let name = |id: &str| (!id.is_empty()).then(|| self.project.item(id).map(|m| m.name.clone())).flatten();
                let a = name(&self.loaded_item);
                let b = self.dual.as_ref().filter(|d| d.media(qcv_player::Side::B).is_some()).and_then(|_| name(self.project.b_source_media_id()));
                let (mode, seam, gain) = g.presenter.engine.dual().map(|d| (d.mode, d.seam, d.gain)).unwrap_or((CompareMode::Single, 0.5, 1.0));
                if g.dualc.sync(self.dual.is_some(), self.pl_editor.is_none(), mode, a, b, seam, gain, !self.active_dual_view.is_empty()) { g.ui_dirty = true; }
                if g.dualc.set_target(g.dual_bars.target) { g.ui_dirty = true; }
                let extra = match (self.dual.as_ref(), self.dual_editor.as_ref()) { (Some(d), Some(ed)) => g.dual_bars.offset_text(&ed.timeline, d.current_frame() as i64).unwrap_or_default(), _ => String::new() };
                if extra != g.strip.extra { g.strip.extra = extra; g.ui_dirty = true; }
                let aud = |s: qcv_player::Side| self.dual.as_ref().filter(|d| d.side_has_audio(s)).map(|d| d.side_muted(s));
                if g.dualc.set_audio(aud(qcv_player::Side::A), aud(qcv_player::Side::B)) { g.ui_dirty = true; }
                if g.dualc.tick(now) || g.dualc.animating(now) { g.ui_dirty = true; }
            }
            if g.bar.tick(now) || g.strip.animating(now) || g.strip.tick(now) || g.toast.tick(now) || g.project_panel.tick(now) { g.ui_dirty = true; }
            let (w, h) = u.size(); let scale = g.window.scale_factor();
            // the sequence cache's band + failed ticks: repaint when they change (QCView polls the cache at 33 ms, emits on change)
            let seq_status = self.player.as_ref().and_then(|p| p.decoder().as_sequence().map(|s| { let c = s.cache(); (c.buffered_behind_coverage(), c.buffered_ahead_coverage(), c.failed_count(), c.looping()) }));
            if seq_status != g.seq_status {
                if seq_status.map(|s| s.2) != g.seq_status.map(|s| s.2) { g.scrubber.failed = self.player.as_ref().map(sequence_failed_on_timeline).unwrap_or_default(); }
                g.scrubber.cache_wraps = seq_status.is_some_and(|s| s.3);
                g.seq_status = seq_status; g.ui_dirty = true;
            }
            let st = scrub_state(pb(&self.dual, &self.player), self.player.as_ref());
            let bar_h = if g.compact { 0.0 } else if self.pl_editor.is_some() { clip_bar::ClipBar::height(scale) } else if self.dual.is_some() { dual_bars::DualBars::height(scale) } else { 0.0 };   // +28 a playlist, +24 dual (row 169)   // +20 for a playlist (row 169)
            let strip_h = if g.strip_hidden { 0.0 } else { t::STATUS_STRIP_HEIGHT as f64 * scale };
            let bands_h = if g.compact { t::SCRUBBER_HEIGHT as f64 * scale } else { strip_h + (t::TRANSPORT_BAR_HEIGHT + t::SCRUBBER_HEIGHT) as f64 * scale + bar_h };
            if g.fullscreen {
                // the pointer at the bottom edge (or on the shown bands) reveals; leaving starts the hide delay
                let (px, py) = g.pointer;
                let shown_rect_top = h as f64 - bands_h * g.bands.value(now) as f64;
                let near = px >= 0.0 && (py >= h as f64 - t::BANDS_REVEAL_ZONE as f64 * scale || (g.bands.target() > 0.5 && py >= shown_rect_top));
                if near { g.bands_leave = None; g.bands.set_target(1.0, now); }
                else if g.bands.target() > 0.5 { match g.bands_leave { None => g.bands_leave = Some(now), Some(t0) => if now.duration_since(t0).as_millis() as u32 >= t::BANDS_HIDE_DELAY { g.bands.set_target(0.0, now); g.bands_leave = None; } } }
            }
            if g.bands.animating(now) { g.ui_dirty = true; }
            let slide = bands_h * (1.0 - g.bands.value(now) as f64);
            let scrub_y = h as f64 - t::SCRUBBER_HEIGHT as f64 * scale - bar_h + slide;
            // the rails span the viewport row (QCView's layout: the bands run full width beneath); none in fullscreen
            let rails_bottom = if g.compact { scrub_y } else { scrub_y - t::TRANSPORT_BAR_HEIGHT as f64 * scale - strip_h };
            let chromeless = g.fullscreen || g.compact;   // QCView: fullscreen or compact hides every rail and band
            // the Color drawer (row 199) sits on the bands and pushes the rails' bottom (and the picture) up
            let notes_h = if chromeless { 0.0 } else { g.notes.visible_height(now, scale) };
            let drawer_h = if chromeless { 0.0 } else { g.drawer.visible_height(now, scale) } + notes_h;
            if g.notes.animating(now) || g.notes.tick(now) { g.ui_dirty = true; }
            let rails_bottom = rails_bottom - drawer_h;
            if g.drawer.animating(now) || g.drawer.tick(now) { g.ui_dirty = true; }
            // QCView's in-window menu bar across the top (Windows); chromeless collapses it to zero, as QCView's `menuBar.height` did
            let menu_h = if chromeless || g.menu_bar.is_none() { 0.0 } else { (t::MENU_BAR_HEIGHT as f64 * scale).round() };
            if let Some(mb) = g.menu_bar.as_mut() { if chromeless { if mb.bar.cancel() { g.ui_dirty = true; } } mb.bar.layout(&mut g.fonts, 0.0, 0.0, w as f64, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), scale); if chromeless { mb.bar.rect = kurbo::Rect::ZERO; } }
            let (lw, rw) = if chromeless { (0.0, 0.0) } else {
                g.left.layout(0.0, menu_h, rails_bottom, scale, now); g.right.layout(w as f64, menu_h, rails_bottom, scale, now);
                (g.left.visible_width(now, scale), g.right.visible_width(now, scale))
            };
            if g.left.animating(now) || g.right.animating(now) || g.left.tick(now) || g.right.tick(now) { g.ui_dirty = true; }
            let content = if g.fullscreen { (0, 0, w, h) } else { let x0 = lw.round() as u32; let x1 = (w as f64 - rw).round() as u32; let y0 = menu_h as u32; (x0, y0, x1.saturating_sub(x0).max(1), ((rails_bottom - menu_h).round() as u32).max(1)) };
            if g.content_set != Some(content) { g.content_set = Some(content); g.presenter.engine.set_content_rect(Some(content)); g.ui_dirty = true; }
            if chromeless { g.drawer.rect = kurbo::Rect::ZERO; } else {
                if g.drawer.sync(color_view, now) { g.ui_dirty = true; }
                let cr = kurbo::Rect::new(content.0 as f64, content.1 as f64, (content.0 + content.2) as f64, (content.1 + content.3) as f64);
                g.drawer.notes_open = g.notes.open;
                g.drawer.layout(&mut g.fonts, cr, 0.0, rails_bottom, w as f64, scale, now);
                // the Notes drawer: its cards from the clip's notes (the annotated thumbnail when there is one), the current note, the tools
                if g.notes.open || g.notes.visible_height(now, scale) > 0.0 {
                    let dir = self.annot.media.as_ref().map(|m| qcv_notes::io::images_dir(&qcv_notes::io::notes_dir(m)));
                    let cards: Vec<notes_drawer::Card> = self.annot.notes.iter().map(|n| { let thumb = dir.as_ref().and_then(|d| { let a = d.join(qcv_notes::io::annotated_filename(&n.timecode)); let c = d.join(qcv_notes::io::image_filename(&n.timecode)); if a.exists() { Some(a) } else if c.exists() { Some(c) } else { None } }); notes_drawer::Card { key: n.timecode.clone(), frame: n.frame, text: n.text.clone(), addressed: n.addressed, thumb, viewer: !n.viewer.is_empty(), ocio: !n.ocio.is_empty() } }).collect();
                    let frame = self.player.as_ref().map(|p| p.current_frame()).unwrap_or(-1);
                    let current = self.annot.note_at_frame(frame).map(|n| n.timecode.clone());
                    let clip = self.annot.media.as_ref().and_then(|m| m.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                    let moved = current != g.notes_current; g.notes_current = current.clone();
                    if g.notes.sync(&clip, cards, current, self.annot.active.then_some(self.annot.tool), &self.settings.string(settings::k::ANNOTATION_COLOR), self.annot.width) { g.ui_dirty = true; }
                    if moved { g.notes.reveal_current(); }
                    let notes_y = rails_bottom + g.drawer.visible_height(now, scale);
                    g.notes.layout(&mut g.fonts, 0.0, notes_y, w as f64, scale, now);
                }
            }
            // the waveform (row 203): the presenter draws its trace into the drawer's card while it shows
            g.presenter.engine.set_scope(if chromeless || g.fullscreen { None } else { g.drawer.scope() });
            // the readout: the newest frame's peaks and the clip's maximum since it loaded
            // dual: each side's own (its engine taps its own waveform), QCView's A / B readout lines
            let scope_on = g.drawer.scope().is_some();
            let side_peaks = if scope_on { g.presenter.engine.dual_mut().map(|d| [d.a.scope_peaks(), d.b.scope_peaks()]) } else { None };
            let p = if scope_on && side_peaks.is_none() { g.presenter.engine.scope_peaks() } else { None };
            let mut fold = |i: usize, p: Option<[f32; 2]>| { if let Some(p) = p { let m = g.clip_peak[i].get_or_insert([0.0, 0.0]); m[0] = m[0].max(p[0]); m[1] = m[1].max(p[1]); } p.zip(g.clip_peak[i]) };
            let (single, dual) = match side_peaks { Some([a, b]) => (None, Some([fold(0, a), fold(1, b)])), None => (fold(0, p), None) };
            if g.drawer.set_peaks(single) | g.drawer.set_peaks_dual(dual) { g.ui_dirty = true; }
            // the panels: the settings mirror the store; the inspector mirrors the Transport (+ the metadata service's results, drained every tick)
            while let Some(ev) = self.meta.try_recv() {
                let path_of = |id: &str, project: &qcv_project::ProjectManager| project.item(id).map(|m| m.path.clone()).unwrap_or_else(|| id.to_string());
                match ev {
                    qcv_meta::MetadataEvent::Video(id, m) => { let path = path_of(&id, &self.project); g.inspector.set_video_meta(&path, m.clone()); if self.project.item(&id).is_some() { self.project.on_video_metadata(&id, m); } }
                    qcv_meta::MetadataEvent::Adobe(id, m) => { let path = path_of(&id, &self.project); g.inspector.set_adobe_meta(&path, m.clone()); if self.project.item(&id).is_some() { self.project.on_adobe_metadata(&id, m); } }
                    qcv_meta::MetadataEvent::Rotation(id, deg) => self.project.on_rotation_probed(&id, deg),
                    qcv_meta::MetadataEvent::Container(id, m) => self.project.on_container_probed(&id, &m),
                }
                g.ui_dirty = true;
            }
            // the window title: QCView's "clip — project" with a dot while unsaved
            { let title = match self.player.as_ref() { Some(_) if !self.source_label.is_empty() => format!("qcv — {} — {}{}", self.source_label, self.project.name(), if self.project.dirty() { " •" } else { "" }), _ => format!("qcv — {}{}", self.project.name(), if self.project.dirty() { " •" } else { "" }) }; if title != g.title_shown { g.window.set_title(&title); g.title_shown = title; } }
            g.settings_panel.sync(&self.settings, now);
            if g.settings_panel.tick(now) || g.inspector.tick(now) || g.inspector.animating(now) || g.shortcuts.animating(now) || g.background_panel.animating(now) || g.background_panel.tooltips.tick(now) || g.guides_panel.animating(now) || g.project_panel.animating(now) { g.ui_dirty = true; }
            if !chromeless {
                let lc = g.left.content_rect();
                if g.project_panel.sync(&self.project, &self.loaded_item) { g.ui_dirty = true; }
                // the loaded playlist's tree: its non-gap clips, the playing one, the count + canvas
                // (from the playlist editor's timeline — it exists for an empty playlist too, the drop target the rows need)
                let tree = self.pl_editor.as_ref().filter(|ed| ed.playlist_media_item_id == self.loaded_item).map(|ed| {
                    let clips: Vec<&qcv_timeline::Clip> = ed.timeline.tracks.first().map(|t| t.clips.iter().filter(|c| !c.is_gap).collect()).unwrap_or_default();
                    let playing = self.player.as_ref().filter(|p| p.is_playlist()).and_then(|p| p.timeline.tracks.first().and_then(|t| t.clips.get(p.active_clip_index())).map(|c| c.id.clone()));
                    let current = playing.and_then(|id| clips.iter().position(|c| c.id == id));
                    let kind = |k: qcv_timeline::ClipMediaKind| match k { qcv_timeline::ClipMediaKind::ImageSequence => qcv_project::MediaType::ImageSequence, qcv_timeline::ClipMediaKind::Audio => qcv_project::MediaType::Audio, _ => qcv_project::MediaType::Video };
                    let n = clips.len();
                    project_panel::TreeModel { playlist: self.loaded_item.clone(), clips: clips.iter().map(|c| (c.id.clone(), c.name.clone(), kind(c.media_kind))).collect(), current, info: format!("{n} clip{} · {}×{}", if n == 1 { "" } else { "s" }, ed.timeline.canvas_width, ed.timeline.canvas_height) }
                });
                if g.project_panel.set_tree(tree) { g.ui_dirty = true; }
                if let Some(id) = g.project_panel.pending_rename.take() { if g.project_panel.begin_rename(&id, &mut g.fonts) { set_ime(&**g.window, true); g.ui_dirty = true; } }
                let ph = g.project_panel.layout(&mut g.fonts, lc.x0, lc.x1, lc.y0, scale);
                g.background_panel.sync(self.settings.int(settings::k::BACKGROUND_MODE) as u8);
                { let st = &self.settings; g.guides_panel.sync(&st.string(settings::k::SAFETY_GUIDE), st.float(settings::k::SAFETY_OPACITY) as f32, st.float(settings::k::SAFETY_WIDTH) as f32, &st.string(settings::k::SAFETY_COLOR)); }
                let gh = g.guides_panel.layout(&mut g.fonts, lc.x0, lc.x1, lc.y0 + ph, scale);   // QCView's order: Media, Safety Guides, Background, Settings
                let bh = g.background_panel.layout(&mut g.fonts, lc.x0, lc.x1, lc.y0 + ph + gh, scale);
                let lh = g.settings_panel.layout(&mut g.fonts, lc.x0, lc.x1, lc.y0 + ph + gh + bh, scale); g.left.content_height = ph + gh + bh + lh;
                let islot = if self.dual.is_some() && g.dual_bars.target == qcv_player::Side::B { 1 } else { 0 };
                { let en: &min_present::engine::Engine = match g.presenter.engine.dual() { Some(d) if self.dual.is_some() => if islot == 1 { &d.b } else { &d.a }, _ => &g.presenter.engine }; let (geo, dims) = (en.geometry(), en.display_dims()); g.inspector.set_geometry(geo, dims); }
                if g.inspector.set_side(self.dual.is_some().then(|| if islot == 1 { qcv_player::Side::B } else { qcv_player::Side::A })) { g.ui_dirty = true; }
                let iid = if islot == 1 { self.project.b_source_media_id().to_string() } else { self.loaded_item.clone() };
                if g.insp_seen != (islot, iid.clone()) { if let Some(m) = self.project.item(&iid) { g.inspector.set_transfer(m.transfer_override); } g.insp_seen = (islot, iid); }
                // the EXR layer tiles: the loaded sequence item's layers (the project's), its current layer, its first frame
                let (ls, cur, first) = self.project.item(&self.loaded_item).filter(|m| m.media_type == qcv_project::MediaType::ImageSequence && self.player.as_ref().is_some_and(|p| p.decoder().as_sequence().is_some())).map(|m| (m.image_seq.available_layers.clone(), m.image_seq.layer.clone(), m.path.clone())).unwrap_or_default();
                g.inspector.set_seq_layers(ls, &cur, &first);
                if g.inspector.sync(match self.dual.as_ref() { Some(d) => d.side(if islot == 1 { qcv_player::Side::B } else { qcv_player::Side::A }), None => self.player.as_ref() }) { g.ui_dirty = true; }
                { let tc = &mut self.thumb_color[islot.min(1)]; if g.inspector.apply_look(thumb_gen[islot.min(1)], &mut |px| tc.convert(px)) { g.ui_dirty = true; } }
                let rc = g.right.content_rect(); let rh = g.inspector.layout(&mut g.fonts, rc.x0, rc.x1, rc.y0, scale);
                let sh = g.shortcuts.layout(&mut g.fonts, rc.x0, rc.x1, rc.y0 + rh, scale); g.right.content_height = rh + sh;
                // Ctrl+/ (QCView revealSection): the card's header to the top, 8 px of air, as far as the content allows
                if std::mem::take(&mut g.shortcuts_reveal) { let max = (g.right.content_height - g.right.body_height()).max(0.0); g.right.scroll = (rh - 8.0 * scale).clamp(0.0, max); g.ui_dirty = true; }
            }
            // the bar's volume / mute / loop and the keys write the store back (QCView persists them on change)
            if let Some(p) = pb(&self.dual, &self.player) { if p.has_audio() { self.settings.set_float(settings::k::VOLUME, p.volume() as f64); self.settings.set_bool(settings::k::MUTED, p.is_muted()); } }
            if let Some(p) = self.player.as_ref() { self.settings.set_bool(settings::k::LOOP_ENABLED, p.loop_enabled()); }
            if !chromeless { self.settings.set_int(settings::k::LEFT_RAIL_WIDTH, g.left.width.round() as i64); self.settings.set_int(settings::k::RIGHT_RAIL_WIDTH, g.right.width.round() as i64); self.settings.set_bool(settings::k::LEFT_RAIL_COLLAPSED, g.left.collapsed); self.settings.set_bool(settings::k::RIGHT_RAIL_COLLAPSED, g.right.collapsed); }
            if let Err(e) = self.settings.save_if_dirty() { log::warn!("settings: {e}"); }
            g.scrubber.compact = g.compact;
            if g.compact { g.strip.sync(pb(&self.dual, &self.player)); let tc = g.strip.timecode_text().to_string(); g.compact_strip.sync(pb(&self.dual, &self.player), &tc, g.pinned, now); g.compact_strip.layout(&mut g.fonts, w as f64, scrub_y, scale); let sr = g.compact_strip.scrub_rect; g.scrubber.layout(sr, scale); if g.compact_strip.tick(now) { g.ui_dirty = true; } }
            else { g.scrubber.layout(kurbo::Rect::new(0.0, scrub_y, w as f64, scrub_y + t::SCRUBBER_HEIGHT as f64 * scale), scale); let lanes = if bar_h > 0.0 { kurbo::Rect::new(0.0, scrub_y + t::SCRUBBER_HEIGHT as f64 * scale, w as f64, scrub_y + t::SCRUBBER_HEIGHT as f64 * scale + bar_h) } else { kurbo::Rect::ZERO }; g.clip_bar.rect = if self.pl_editor.is_some() { lanes } else { kurbo::Rect::ZERO }; g.dual_bars.rect = if self.dual.is_some() { lanes } else { kurbo::Rect::ZERO }; }
            if g.scrubber.follow(&st, scale) { g.ui_dirty = true; }
            // the waveform: ask the probe engine for the visible window at column resolution; repaint as peaks land
            if let Some(p) = self.player.as_ref() { if p.audio.has_audio() && g.waveform_enabled {
                g.waveform.set_playback_active(p.is_playing() || p.is_scrubbing());
                let (t0, t1) = g.scrubber.visible_window(&st); let cols = g.scrubber.track_columns();
                let req = (p.path().to_string(), t0, t1, cols);
                if req != g.wave_req { g.wave_req = req.clone(); g.waveform.request_window(&req.0, t0, t1, cols); g.ui_dirty = true; }
                let wgen = g.waveform.data_generation();
                if wgen != g.wave_seen || g.ui_dirty { g.wave_seen = wgen; g.wave_peaks = g.waveform.sample_columns(&req.0, t0, t1, cols).into_iter().map(|c| if c.valid { (c.mn, c.mx) } else { (0.0, 0.0) }).collect(); g.ui_dirty = true; }
            } else if !g.wave_peaks.is_empty() {
                // a clip without audio (or the probe switched off): QCView's waveform loader goes inactive and draws nothing — never the previous clip's peaks
                g.wave_peaks.clear(); g.wave_req = (String::new(), 0.0, 0.0, 0); g.ui_dirty = true;
            } }
            let [tc0, tc1] = &mut self.thumb_color;
            if let Some(d) = self.dual.as_mut() {
                // dual (row 170: a pair tracking the pointer, A above B): each side's frame under the master hover through its
                // own edits; nothing requested while both sides are busy playing / scrubbing, and no strip precache
                let hv = g.scrubber.hover_frame.filter(|_| !d.is_playing() && !d.is_scrubbing());
                for (side, th, tc, look_gen) in [(qcv_player::Side::A, &mut g.thumbs, &mut *tc0, thumb_gen[0]), (qcv_player::Side::B, &mut g.thumbs_b, &mut *tc1, thumb_gen[1])] {
                    let f = hv.and_then(|m| d.side_frame_of(side, m)).map(|x| x as i32);
                    if let Some(t) = d.side(side) { th.follow(&t.thumb_key()); }
                    if th.tick(d.side_mut(side), f, now, (look_gen, &mut |px| tc.convert(px))) { g.ui_dirty = true; }
                }
            } else {
                if g.thumbs.tick(self.player.as_mut(), g.scrubber.hover_frame, now, (thumb_gen[0], &mut |px| tc0.convert(px))) { g.ui_dirty = true; }
                if g.thumbs_b.is_shown() { g.thumbs_b.tick(None, None, now, (0, &mut |_| None)); g.ui_dirty = true; }
            }
            let _ = frame;
            if u.is_dirty() || g.ui_dirty || key != g.ui_shown_frame {
                let bands_y = scrub_y - t::TRANSPORT_BAR_HEIGHT as f64 * scale;
                g.bar.layout(&mut g.fonts, w as f64, bands_y, scale);
                g.strip.sync(pb(&self.dual, &self.player));
                g.strip.layout(w as f64, bands_y - t::STATUS_STRIP_HEIGHT as f64 * scale, scale);
                { let sr = g.strip.rect; let fw = g.strip.fps_width(&mut g.fonts, scale); let gap = t::SPACING_300 as f64 * scale;
                  let ew = g.strip.extra_width(&mut g.fonts, scale);
                  g.dualc.layout(&mut g.fonts, g.strip.wells_right() + ew + gap, sr.x1 - t::SPACING_100 as f64 * scale - fw - if fw > 0.0 { gap } else { 0.0 }, sr.y0, sr.height(), scale); }
                // the seam (Split wipe: grey, white under the pointer / while dragged) and side-by-side's fixed grey centre line (QCView's compositor)
                let seam_line = g.presenter.engine.dual().and_then(|d| match d.mode { CompareMode::Split => Some((d.seam_x(), g.seam_hot || g.seam_drag)), CompareMode::SideBySide => g.presenter.engine.content_rect().map(|(cx, _, cw, _)| (cx as f64 + (cw / 2) as f64, false)), _ => None });
                let content = g.presenter.engine.content_rect().map(|(x, y, cw, ch)| kurbo::Rect::new(x as f64, y as f64, (x + cw) as f64, (y + ch) as f64)).unwrap_or(kurbo::Rect::new(0.0, 0.0, w as f64, h as f64));
                let scene = u.scene_mut();
                if g.compact {
                    scene.reset();
                    g.scrubber.paint(scene, &mut g.fonts, scale, &st, None);
                    g.compact_strip.paint(scene, &mut g.fonts, scale, now);
                    if self.dual.is_some() { let hb = g.thumbs_b.shown_height(content); let gap = if hb > 0.0 { hb + t::SPACING_100 as f64 * scale } else { 0.0 }; g.thumbs_b.paint(scene, content, scale, Some(g.pointer.0)); g.thumbs.paint_lifted(scene, content, scale, Some(g.pointer.0), gap); } else { g.thumbs.paint(scene, content, scale, Some(g.pointer.0)); }
                    g.compact_strip.paint_tooltip(scene, &mut g.fonts, scale, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), now);
                    g.ui_shown_frame = key; g.ui_dirty = false;
                } else {
                ui_sketch::bands(scene, w, h, scale, slide, bar_h);
                if !g.fullscreen {
                    for (rail, is_left) in [(&g.left, true), (&g.right, false)] {
                        rail.paint_background(scene, now);
                        if rail.rect.width() > 0.0 {
                            rail.push_clip(scene);
                            if is_left { g.project_panel.paint(scene, &mut g.fonts, scale, now, true); g.guides_panel.paint(scene, &mut g.fonts, scale, now, false); g.background_panel.paint(scene, &mut g.fonts, scale, now, false); g.settings_panel.paint(scene, &mut g.fonts, scale, now, false); } else { g.inspector.paint(scene, &mut g.fonts, scale, now); g.shortcuts.paint(scene, &mut g.fonts, scale, now, false); }
                            rail.pop_clip(scene);
                        }
                        rail.paint_foreground(scene, &mut g.fonts, scale, now);
                    }
                }
                // the frame's annotations, the live stroke, the selection (row 215: while annotating, single view)
                if self.annot.active && self.dual.is_none() { if let (Some(p), Some(geo)) = (self.player.as_ref(), annot_geo(&g.presenter.engine)) { self.annot.paint(scene, p.current_frame(), &geo); } }
                // the safety guide over each picture (row 215): side by side → both sides; wipe / difference / single → A (QCView)
                if let Some((_, gd)) = g.guide.as_ref() {
                    let st = &self.settings;
                    let (w_, op) = (st.float(settings::k::SAFETY_WIDTH), st.float(settings::k::SAFETY_OPACITY) as f32);
                    let c = min_ui::color_picker::parse_hex(&st.string(settings::k::SAFETY_COLOR)).unwrap_or([1.0, 1.0, 0.0]);
                    let col = peniko::Color::new([c[0], c[1], c[2], op.clamp(0.0, 1.0)]);
                    let places: Vec<_> = match g.presenter.engine.dual() { Some(d) => { let mut v = vec![d.a.picture_placement()]; if d.mode == CompareMode::SideBySide { v.push(d.b.picture_placement()); } v } None => vec![g.presenter.engine.picture_placement()] };
                    let content = g.presenter.engine.content_rect();
                    for pl in places.into_iter().flatten() { guides::paint(scene, gd, guides::transform(gd.view, pl.1, pl.0, pl.2), w_ * scale, col); }
                    if g.presenter.engine.picture_placement().is_none() && g.presenter.engine.dual().is_none() { if let Some((cx, cy, cw, ch)) = content { let r = [cx as f64, cy as f64, cw as f64, ch as f64]; guides::paint(scene, gd, guides::transform(gd.view, gd.view, r, 0), w_ * scale, col); } }   // no media: the whole viewport (QCView)
                }
                if let (Some((sx, hot)), Some((_, cy, _, ch))) = (seam_line, g.presenter.engine.content_rect()) { let lw = (2.0 * scale).round().max(1.0); let c = if hot { min_theme::Color::rgba(1.0, 1.0, 1.0, 1.0) } else { min_theme::Color::rgba(0.4, 0.4, 0.4, 1.0) }; min_ui::paint::rect(scene, kurbo::Rect::new(sx - lw / 2.0, cy as f64, sx + lw / 2.0, (cy + ch) as f64), c); }
                if !g.fullscreen { g.drawer.paint(scene, &mut g.fonts, now); g.notes.paint(scene, &mut g.fonts, now); g.drawer.paint_tab(scene, &mut g.fonts, now); }
                if !g.fullscreen { if let Some(mb) = g.menu_bar.as_ref() { mb.bar.paint(scene, &mut g.fonts, scale); } }
                if !g.strip_hidden { g.strip.paint(scene, &mut g.fonts, scale, now); g.dualc.paint(scene, &mut g.fonts, scale, now); }
                g.bar.paint(scene, &mut g.fonts, scale, now);
                g.scrubber.paint(scene, &mut g.fonts, scale, &st, if g.wave_peaks.is_empty() { None } else { Some(&g.wave_peaks) });
                if let Some(ed) = self.pl_editor.as_ref() { g.clip_bar.paint(scene, &mut g.fonts, &ed.timeline, &g.scrubber, &st, scale); }
                if let (Some(ed), true) = (self.dual_editor.as_ref(), self.dual.is_some()) { g.dual_bars.paint(scene, &mut g.fonts, &ed.timeline, &g.scrubber, &st, scale); }
                if self.dual.is_some() { let hb = g.thumbs_b.shown_height(content); let gap = if hb > 0.0 { hb + t::SPACING_100 as f64 * scale } else { 0.0 }; g.thumbs_b.paint(scene, content, scale, Some(g.pointer.0)); g.thumbs.paint_lifted(scene, content, scale, Some(g.pointer.0), gap); } else { g.thumbs.paint(scene, content, scale, Some(g.pointer.0)); }
                g.strip.paint_popover(scene, &mut g.fonts, scale);
                g.bar.paint_tooltip(scene, &mut g.fonts, scale, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), now);
                g.strip.paint_tooltip(scene, &mut g.fonts, scale, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), now);
                if !g.strip_hidden { g.dualc.paint_tooltip(scene, &mut g.fonts, scale, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), now); }
                if !g.fullscreen { g.left.paint_tooltip(scene, &mut g.fonts, scale, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), now); g.right.paint_tooltip(scene, &mut g.fonts, scale, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), now); }
                if !g.fullscreen { g.settings_panel.paint_overlays(scene, &mut g.fonts, scale, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), now); g.inspector.paint_overlays(scene, &mut g.fonts, scale, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), now); g.background_panel.tooltips.paint(scene, &mut g.fonts, scale, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), now); }
                if !g.fullscreen { g.guides_panel.paint_menu(scene, &mut g.fonts, scale); }
                if !g.fullscreen { g.project_panel.paint_menu(scene, &mut g.fonts, scale, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), now); }
                g.clip_popover.paint(scene, &mut g.fonts, scale);
                if !g.fullscreen { g.drawer.paint_overlays(scene, &mut g.fonts, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), now); g.notes.paint_overlays(scene, &mut g.fonts, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), now); }
                g.toast.paint(scene, &mut g.fonts, content, scale, now);
                if let Some(mb) = g.menu_bar.as_ref() { mb.bar.paint_menus(scene, &mut g.fonts, scale); }
                if g.about.open { let b = kurbo::Rect::new(0.0, 0.0, w as f64, h as f64); g.about.layout(&mut g.fonts, b, scale); g.about.paint(scene, &mut g.fonts, scale, b, now); }
                if g.dual_dialog.open { let b = kurbo::Rect::new(0.0, 0.0, w as f64, h as f64); g.dual_dialog.layout(&mut g.fonts, b, scale); g.dual_dialog.paint(scene, &mut g.fonts, scale, b, now); }
                g.ui_shown_frame = key; g.ui_dirty = false;
                }
            }
            if let Some(ms) = u.render(&g.presenter.engine.device, &g.presenter.engine.queue) { g.ui_ms.push(ms); }
        }
        let presented = g.presenter.draw(|| g.window.pre_present_notify());
        if presented {
            self.frames_presented += 1;
            if let Some(lp) = self.last_present { self.presents.push((now - lp).as_secs_f32() * 1000.0); }
            self.last_present = Some(now);
            if shown_new && !self.first_presented { self.first_presented = true; if let Some(t) = self.t_open { println!("open → first frame presented: {:.0} ms", t.elapsed().as_secs_f32() * 1000.0); } }
        } else { std::thread::sleep(std::time::Duration::from_millis(16)); }
        if shown_new { self.after_show(); }
        if self.o.stats && now.duration_since(self.last_report).as_secs_f32() >= 2.0 {
            let dt = now.duration_since(self.last_report).as_secs_f32();
            let edr = Presenter::edr_headroom(); let edr_max = edr.iter().cloned().fold(0.0, f64::max); if edr_max > self.edr_seen { self.edr_seen = edr_max; }
            let (lat_p95, dropped, late) = self.player.as_mut().map(|p| (pct(&mut p.stats.lateness_ms.clone(), 95.0), p.stats.dropped, p.stats.late)).unwrap_or((f32::NAN, 0, 0));
            let g = self.gpu.as_mut().unwrap();
            let ui = if g.ui_ms.is_empty() { String::new() } else { format!("  ui renders {} ms p50 {:.2} max {:.2}", g.ui_ms.len(), pct(&mut g.ui_ms, 50.0), pct(&mut g.ui_ms, 100.0)) };
            if let Some(p) = self.player.as_mut() { if p.audio.has_audio() { let mut d = p.audio.take_drift(); if let Some(a) = p.audio.stats() { if !d.is_empty() { println!("        audio drift ms p50 {:+.1} p95 {:+.1} max {:+.1} (n={})  re-seeks {} holds {} skips {} underruns {} fades {} callbacks {} contended {}", pct(&mut d, 50.0), pct(&mut d.iter().map(|v| v.abs()).collect::<Vec<_>>(), 95.0), d.iter().fold(0.0f32, |m, v| m.max(v.abs())), d.len(), a.reseeks, a.holds, a.skips, a.underruns, a.fades, a.callbacks, a.contended); } } } }
            println!("{:>5.1} s  shown {:>5.1} fps  show ms p50 {:.2} max {:.2}  present ms p50 {:.1} p95 {:.1} max {:.1}  lateness p95 {:.1}  dropped {dropped} late {late}  EDR now {:?}{ui}",
                self.t0.map(|t| t.elapsed().as_secs_f32()).unwrap_or(0.0), self.shown_in_window as f32 / dt, pct(&mut self.show_ms, 50.0), pct(&mut self.show_ms, 100.0), pct(&mut self.presents, 50.0), pct(&mut self.presents, 95.0), pct(&mut self.presents, 100.0), lat_p95, edr);
            g.ui_ms.clear();
            self.shown_in_window = 0; self.show_ms.clear(); self.presents.clear(); self.last_report = now;
        }
    }
    /// One pointer event through the UI's router: the transport bar's commands go the keys' way (`Transport::apply`).
    fn pointer(&mut self, e: min_ui::Event) {
        let now = Instant::now();
        let Some(g) = self.gpu.as_mut() else { return };
        let (w, h) = g.ui.as_ref().map(|u| u.size()).unwrap_or((1, 1)); let scale = g.window.scale_factor();
        if let Some((x, y)) = e.pos() { g.pointer = (x, y); } else { g.pointer = (-1.0, -1.0); }
        if matches!(e, min_ui::Event::Move { .. } | min_ui::Event::Down { .. }) { g.last_move = now; }
        tab_presence(g, now);
        let bounds = kurbo::Rect::new(0.0, 0.0, w as f64, h as f64);
        if matches!(e, min_ui::Event::Down { .. }) { self.alt_clean = false; }
        // QCView's About modal takes everything while open
        if g.dual_dialog.open { let d1 = g.dual_dialog.pointer(&e, &mut g.fonts); let (b, d2) = g.dual_dialog.event(&e, now); if d1 || d2 { g.ui_dirty = true; } if let Some(i) = b { self.dual_dialog_done(i); } return }
        if g.about.open { let (b, _) = g.about.event(&e, now); match b { Some(0) => { let _ = os_actions::open("https://qcview.app/"); g.about.cancel(); } Some(_) => g.about.cancel(), None => {} } g.ui_dirty = true; return }
        // QCView's in-window menu bar (row 194): an open menu takes everything (its click-eater), the strip its own presses;
        // a band node's grab (a scrub released over the bar) is never the bar's
        if !g.compact && !g.fullscreen && !(matches!(e, min_ui::Event::Move { .. } | min_ui::Event::Up { .. }) && (g.scrubber.is_pressed() || g.bar.is_pressed() || g.clip_bar.is_pressed() || g.dualc.dragging() || g.seam_drag || g.dual_bars.is_pressed())) {
            if let Some(mb) = g.menu_bar.as_mut() {
                let (id, d, consumed) = mb.bar.event(&e, &mut g.fonts); if d { g.ui_dirty = true; }
                if let Some(c) = id.and_then(|i| mb.cmd(i)) { self.menu_queue.push(c); }
                if consumed { if g.cursor != winit::cursor::CursorIcon::Default { g.cursor = winit::cursor::CursorIcon::Default; g.window.set_cursor(winit::cursor::Cursor::Icon(g.cursor)); } return }
            }
        }
        if g.compact {
            let (cmds, dirty, consumed) = g.compact_strip.event(&e, now); if dirty { g.ui_dirty = true; }
            let want = if st_hit(self.player.as_ref(), &g.scrubber, e.pos()) && !consumed { winit::cursor::CursorIcon::Pointer } else { winit::cursor::CursorIcon::Default };
            if want != g.cursor { g.cursor = want; g.window.set_cursor(winit::cursor::Cursor::Icon(want)); }
            for c in cmds { match c {
                compact_strip::Cmd::Act(a) => { if let Some(p) = pb_mut(&mut self.dual, &mut self.player) { p.apply(a, now); } }
                compact_strip::Cmd::Pin => self.toggle_pin(),
                compact_strip::Cmd::Exit => { self.toggle_compact(); return }
                compact_strip::Cmd::DragWindow => { if let Some(g) = self.gpu.as_ref() { if let Err(e) = g.window.drag_window() { log::debug!("drag_window: {e}"); } } }
            } }
            let Some(g) = self.gpu.as_mut() else { return };
            if consumed && !(g.scrubber.is_pressed() && matches!(e, min_ui::Event::Move { .. } | min_ui::Event::Up { .. })) { return }
            let st = scrub_state(pb(&self.dual, &self.player), self.player.as_ref());
            let (scmds, sd) = g.scrubber.event(&e, now, &st, scale); if sd { g.ui_dirty = true; }
            if let Some(p) = pb_mut(&mut self.dual, &mut self.player) { for c in scmds { match c { min_ui::ScrubCmd::Begin => { if *TRACE_SCRUB { println!("scrub: begin at {:.1} ms", trace_ms()); } p.begin_scrub() } min_ui::ScrubCmd::To(f) => { if *TRACE_SCRUB { println!("scrub: request {f} at {:.1} ms", trace_ms()); self.scrub_last_req = Some((f, Instant::now())); } p.scrub_to_frame(f) } min_ui::ScrubCmd::End => { if *TRACE_SCRUB { println!("scrub: end at {:.1} ms", trace_ms()); } p.end_scrub() } } } }
            return
        }
        // THE POINTER GRAB: a press on a band node (the scrubber, a transport button, the volume slider) owns Move / Up until the
        // release, wherever the pointer went — Qt's MouseArea grab, which QCView relied on. Without it a release with the pointer
        // over a rail was swallowed by the hit-tested routing below: the scrubber stayed pressed and every later move dragged the
        // playhead (to the end of the clip when the mouse went right), a held fast-forward never saw its release-on-leave and
        // shuttled to the end — the user's "scrubbing overruns until the end" (2026-11-07 night), and the parked Windows
        // "clicks lost after leaving and returning" is the same swallow.
        // the toast: a press ON it dismisses (its action button fires); presses elsewhere go on
        { let (id, d, consumed) = g.toast.event(&e); if d { g.ui_dirty = true; } if let Some(id) = id { self.toast_action_invoked(&id); return } if consumed { return } }
        let Some(g) = self.gpu.as_mut() else { return };
        // the clip popover takes everything while open; a press outside commits it
        if g.clip_popover.is_open() {
            let (c, d, consumed) = g.clip_popover.event(&e, &mut g.fonts); if d { g.ui_dirty = true; }
            if !g.clip_popover.is_open() { set_ime(&**g.window, false); }
            let closed_now = !c.is_empty() || !g.clip_popover.is_open();
            for c in c { self.bar_cmd(c); }
            if consumed || closed_now { return }
        }
        let Some(g) = self.gpu.as_mut() else { return };
        let grabbed = matches!(e, min_ui::Event::Move { .. } | min_ui::Event::Up { .. }) && (g.scrubber.is_pressed() || g.bar.is_pressed() || g.clip_bar.is_pressed() || g.dualc.dragging() || g.seam_drag || g.dual_bars.is_pressed());
        // an open rail popup (a Settings picker such as Language, the Guide picker, the Media context menu, a settings dialog) owns
        // the pointer ahead of the drawers: its menu can hang over the Notes / Color drawer, which took the clicks and hovers
        // underneath it (the Windows walk 2026-11-09, window-geometry dependent)
        let rail_pop = !g.fullscreen && (g.settings_panel.is_modal() || g.settings_panel.menu_open() || g.guides_panel.menu_open() || g.project_panel.menu_open());
        // the Notes drawer (row 215): its own presses (and its slider / picker drags)
        if !g.fullscreen && !g.compact && !grabbed && !rail_pop && g.notes.rect.height() > 1.0 {
            let had = g.notes.has_focus();
            let (cmds, d, consumed) = g.notes.event(&e, now, &mut g.fonts); if d { g.ui_dirty = true; }
            let want = if g.notes.splitter_hovered() { winit::cursor::CursorIcon::RowResize } else { winit::cursor::CursorIcon::Default };
            if (g.notes.splitter_hovered() || g.cursor == winit::cursor::CursorIcon::RowResize) && want != g.cursor { g.cursor = want; g.window.set_cursor(winit::cursor::Cursor::Icon(want)); }
            if g.notes.has_focus() != had { set_ime(&**g.window, g.notes.has_focus()); }
            let _ = g; for c in cmds { self.notes_cmd(c); }
            if consumed { return }
        }
        let g = self.gpu.as_mut().unwrap();
        // the Color drawer (row 199): its menu / naming popover take everything; its tab over the picture; its own presses
        if !g.fullscreen && !g.compact && !grabbed && !rail_pop {
            let mut cmds = vec![];
            if matches!(e, min_ui::Event::Down { .. }) && g.drawer.has_focus() && !e.pos().is_some_and(|(x, y)| g.drawer.rect.contains(kurbo::Point::new(x, y))) { cmds.extend(g.drawer.blur()); }
            let modal = g.drawer.is_modal(); let was_typing = g.drawer.is_typing();
            let (more, d, consumed) = g.drawer.event(&e, now, &mut g.fonts, bounds); cmds.extend(more);
            if d { g.ui_dirty = true; }
            let typing = g.drawer.is_typing(); if typing != was_typing { set_ime(&**g.window, typing); }
            if g.drawer.splitter_hovered() { if g.cursor != winit::cursor::CursorIcon::RowResize { g.cursor = winit::cursor::CursorIcon::RowResize; g.window.set_cursor(winit::cursor::Cursor::Icon(g.cursor)); } } else if g.cursor == winit::cursor::CursorIcon::RowResize { g.cursor = winit::cursor::CursorIcon::Default; g.window.set_cursor(winit::cursor::Cursor::Icon(g.cursor)); }
            for c in cmds { self.color_cmd(c); }
            if consumed || modal { return }
        }
        let Some(g) = self.gpu.as_mut() else { return };
        // the rails first (a modal dialog or an open menu in the settings panel takes everything)
        if !g.fullscreen && !grabbed {
            let had_focus = g.settings_panel.has_focus();
            // ONE OWNER while a rail popup is open (the walk 2026-11-08: the Guide menu clicked through to the Background tiles): a
            // settings picker / dialog, the Guide picker, the Media context menu — only that panel sees the pointer, wherever it is
            let settings_pop = g.settings_panel.is_modal() || g.settings_panel.menu_open(); let guides_pop = g.guides_panel.menu_open(); let project_pop = g.project_panel.menu_open();
            let modal = settings_pop || guides_pop;
            let lr = g.left.event(&e, now); let rr = g.right.event(&e, now);
            if lr.dirty || rr.dirty { g.ui_dirty = true; }
            let over_left = e.pos().is_some_and(|(x, y)| g.left.hit(x, y)); let over_right = e.pos().is_some_and(|(x, y)| g.right.hit(x, y));
            let want = if g.drawer.splitter_hovered() { winit::cursor::CursorIcon::RowResize } else if g.left.splitter_hovered() || g.right.splitter_hovered() { winit::cursor::CursorIcon::ColResize } else if st_hit(self.player.as_ref(), &g.scrubber, e.pos()) { winit::cursor::CursorIcon::Pointer } else { winit::cursor::CursorIcon::Default };
            if want != g.cursor { g.cursor = want; g.window.set_cursor(winit::cursor::Cursor::Icon(want)); }
            let mut cmds = vec![]; let mut pcmds: Vec<project_panel::Cmd> = vec![]; let mods = self.mods;
            // a press on a rail's own control (the collapse arrow, the scrollbar) is the rail's, not the panel's
            let rail_ctl = matches!(e, min_ui::Event::Down { .. }) && e.pos().is_some_and(|(x, y)| g.left.over_control(x, y, now) || g.right.over_control(x, y, now));
            if matches!(e, min_ui::Event::Down { .. }) && !over_right && g.shortcuts.has_focus() { g.shortcuts.blur(); set_ime(&**g.window, false); g.ui_dirty = true; }
            if matches!(e, min_ui::Event::Down { .. }) && !over_right && g.inspector.has_focus() { let cmds = g.inspector.blur(); set_ime(&**g.window, false); g.ui_dirty = true; for c in cmds { if let inspector_panel::Cmd::Par(par) = c { { let sl = insp_slot(&self.dual, g); let dual_on = self.dual.is_some(); if let Some(en) = color::slot_engine(g, sl, dual_on) { let mut geo = en.geometry(); geo.par = par; en.set_geometry(geo); } } g.content_set = None; } } }
            let pp_modal = g.project_panel.menu_open() || g.project_panel.is_editing() || g.project_panel.is_dragging(); let pp_modal_editing = g.project_panel.is_editing();
            if !rail_ctl && (modal || pp_modal || over_left || matches!(e, min_ui::Event::Leave | min_ui::Event::Up { .. }) || g.settings_panel.has_focus()) {
                // a press outside the settings' fields commits a typed number
                if matches!(e, min_ui::Event::Down { .. }) && !over_left && had_focus { cmds.extend(g.settings_panel.blur(&mut self.settings)); }
                if !guides_pop && !project_pop { let (d, _c, more) = g.settings_panel.event(&e, now, &mut g.fonts, &mut self.settings, bounds, scale); if d { g.ui_dirty = true; } cmds.extend(more); }
                if !settings_pop && !project_pop {
                    let had = g.guides_panel.has_focus();
                    if matches!(e, min_ui::Event::Down { .. }) && had && !over_left { for c in g.guides_panel.blur() { guide_setting(&mut self.settings, c, &mut cmds); } }
                    let (gc, d) = g.guides_panel.event(&e, now, &mut g.fonts, bounds, scale); if d { g.ui_dirty = true; }
                    for c in gc { guide_setting(&mut self.settings, c, &mut cmds); }
                    if g.guides_panel.has_focus() != had { set_ime(&**g.window, g.guides_panel.has_focus()); }
                }
                if !settings_pop && !guides_pop && !project_pop && !g.guides_panel.menu_open() { let (chosen, d) = g.background_panel.event(&e, now); if d { g.ui_dirty = true; } if let Some(m) = chosen { if self.settings.set_int(settings::k::BACKGROUND_MODE, m as i64) { cmds.push(settings_panel::Cmd::Changed(settings::k::BACKGROUND_MODE)); } } }
                if !settings_pop && !guides_pop && !g.guides_panel.menu_open() { let (p, d) = g.project_panel.event(&e, now, mods.ctrl, mods.shift, &mut g.fonts, bounds, scale); if d { g.ui_dirty = true; } if g.project_panel.is_editing() != pp_modal_editing { set_ime(&**g.window, g.project_panel.is_editing()); } pcmds.extend(p); }
            }
            if g.settings_panel.has_focus() != had_focus { set_ime(&**g.window, g.settings_panel.has_focus()); }
            if !rail_ctl && (over_right || matches!(e, min_ui::Event::Leave | min_ui::Event::Up { .. })) {
                let had_sc = g.shortcuts.has_focus(); if g.shortcuts.event(&e, now, &mut g.fonts) { g.ui_dirty = true; } if g.shortcuts.has_focus() != had_sc { set_ime(&**g.window, g.shortcuts.has_focus()); }
                let had = g.inspector.has_focus();
                let (icmds, d) = g.inspector.event(&e, now, &mut g.fonts); if d { g.ui_dirty = true; }
                if g.inspector.has_focus() != had { set_ime(&**g.window, g.inspector.has_focus()); }
                // the Inspector's subject (the walk 2026-11-08: "that makes sense for the inspector too"): dual's target side, else the single view
                let slot = insp_slot(&self.dual, g); let dual_on = self.dual.is_some();
                let target_id = if slot == 1 { self.project.b_source_media_id().to_string() } else { self.loaded_item.clone() };
                for c in icmds { match c {
                    // QCView: the timeline retunes live (`TimelineController::setFrameRate`) and the item keeps the rate (`setImageSeqFrameRate`)
                    // QCView `setImageSeqLayer`: the cache reloads at the layer, the item keeps it
                    // QCView copyTextToClipboard / revealInFileManager / openExternalPath
                    inspector_panel::Cmd::Copy(text) => { match os_actions::copy(&text) { Ok(()) => g.toast.show(trw("Copied to the clipboard"), min_ui::ToastKind::Positive, now), Err(e) => g.toast.show(&fill(trw("Couldn’t copy: %1"), &[&e]), min_ui::ToastKind::Negative, now) } g.ui_dirty = true; }
                    inspector_panel::Cmd::Reveal(p) => { if let Err(e) = os_actions::reveal(&p) { log::warn!("reveal: {e}"); } }
                    inspector_panel::Cmd::Open(p) => { if let Err(e) = os_actions::open(&p) { log::warn!("open: {e}"); } }
                    inspector_panel::Cmd::Layer(l) => { if let Some(p) = self.player.as_mut() { p.set_sequence_layer(&l); } if !self.loaded_item.is_empty() { let id = self.loaded_item.clone(); self.project.set_sequence_layer(&id, &l); } g.ui_dirty = true; }
                    inspector_panel::Cmd::FrameRate(fps) => { if let Some(p) = self.player.as_mut() { p.set_sequence_rate(qcv_timeline::Rate::from_f64(fps)); } if !self.loaded_item.is_empty() { let id = self.loaded_item.clone(); self.project.set_image_seq_frame_rate(&id, fps); } g.ui_dirty = true; }
                    // QCView `setImageSeqCacheStride`: the live cache + the item's session value
                    inspector_panel::Cmd::Stride(s) => { if let Some(c) = self.player.as_ref().and_then(|p| p.decoder().as_sequence()) { c.cache().set_stride(s); } if !self.loaded_item.is_empty() { let id = self.loaded_item.clone(); self.project.set_image_seq_cache_stride(&id, s); } g.ui_dirty = true; }
                    inspector_panel::Cmd::Range(r) => { if let Some(p) = insp_transport(&mut self.dual, &mut self.player, slot) { p.set_range_override(r); if !p.is_playing() { let f = p.current_frame(); p.seek_to_frame(f); } } if !target_id.is_empty() { let id = target_id.clone(); self.project.set_video_range_override(&id, match r { qcv_player::RangeOverride::Full => qcv_project::VideoRange::Full, qcv_player::RangeOverride::Limited => qcv_project::VideoRange::Limited, _ => qcv_project::VideoRange::Auto }); } }   // QCView: "applies on the next seek or play" — paused, re-decode the frame on screen
                    inspector_panel::Cmd::Transfer(t) => { if let Some(en) = color::slot_engine(g, slot, dual_on) { en.set_transfer_override(transfer_choice(t)); } g.content_set = None; if !target_id.is_empty() { let id = target_id.clone(); self.project.set_transfer_override(&id, t); } }
                    inspector_panel::Cmd::Routing(m) => { if let Some(p) = insp_transport(&mut self.dual, &mut self.player, slot) { p.audio.set_routing_mode(m); } if !target_id.is_empty() { let id = target_id.clone(); self.project.set_audio_routing_mode(&id, match m { qcv_player::RoutingMode::Downmix5_1 => qcv_project::AudioRoutingMode::Downmix5_1, qcv_player::RoutingMode::Stereo7_8 => qcv_project::AudioRoutingMode::Stereo7_8, _ => qcv_project::AudioRoutingMode::Auto }); } }
                    inspector_panel::Cmd::Origin(o) => { g.strip.origin = Some(o); g.strip.sync(pb(&self.dual, &self.player)); }
                    inspector_panel::Cmd::Par(par) => { { let sl = insp_slot(&self.dual, g); let dual_on = self.dual.is_some(); if let Some(en) = color::slot_engine(g, sl, dual_on) { let mut geo = en.geometry(); geo.par = par; en.set_geometry(geo); } } g.content_set = None; if !target_id.is_empty() { let id = target_id.clone(); let (mode, n, d) = match par { min_present::engine::Par::Square => (qcv_project::PixelAspectMode::Square, 1, 1), min_present::engine::Par::Detected => (qcv_project::PixelAspectMode::Detected, 1, 1), min_present::engine::Par::Custom(n, d) => (qcv_project::PixelAspectMode::Custom, n as i32, d as i32) }; self.project.set_pixel_aspect(&id, mode, n, d); } }
                    inspector_panel::Cmd::Rotation(r) => { if let Some(en) = color::slot_engine(g, slot, dual_on) { let mut geo = en.geometry(); geo.rotation = r; en.set_geometry(geo); } g.content_set = None; if !target_id.is_empty() { let id = target_id.clone(); self.project.set_rotation_override(&id, r.map(|d| d as i32).unwrap_or(-1)); } }
                } }
            }
            let dragging = g.project_panel.is_dragging(); let ptr = e.pos();
            let _ = g; for c in cmds { self.apply_setting(c); } for c in pcmds { self.project_cmd(c); }
            if self.pl_editor.is_some() { let hint = match (dragging, ptr) { (true, Some((x, y))) => match self.drop_target(x, y) { Some(DropTarget::Playlist(_)) => self.playlist_drop_index(x), _ => None }, _ => None }; if let Some(g) = self.gpu.as_mut() { if g.clip_bar.set_drop_hint(hint) { g.ui_dirty = true; } } }
            let g = self.gpu.as_mut().unwrap();
            if modal || pp_modal || g.project_panel.menu_open() || lr.consumed || rr.consumed || over_left || over_right { if !matches!(e, min_ui::Event::Leave) { return } }
            let _ = g;
        }
        let g = self.gpu.as_mut().unwrap();
        // dual view's controls (row 173) — they own the pointer while their slider drags
        if !g.strip_hidden && !g.compact && !g.fullscreen && (!grabbed || g.dualc.dragging()) {
            let (dc, dd, consumed) = g.dualc.event(&e, now, scale);
            if dd { g.ui_dirty = true; }
            if consumed || !dc.is_empty() { let _ = g; for c in dc { self.dual_cmd(c); } return }
        }
        // annotating (row 215): the active tool owns presses on the picture (and the drag that follows, wherever it goes)
        if self.annot.active && self.dual.is_none() && !grabbed {
            if let (Some(p), Some(geo)) = (self.player.as_ref(), annot_geo(&g.presenter.engine)) {
                let (frame, fps) = (p.current_frame(), p.rate().as_f64());
                let content = g.presenter.engine.content_rect().map(|(x, y, w, h)| kurbo::Rect::new(x as f64, y as f64, (x + w) as f64, (y + h) as f64));
                let in_view = e.pos().is_some_and(|(x, y)| content.is_some_and(|c| c.contains(kurbo::Point::new(x, y))));
                let mut done = None;
                let used = match e {
                    min_ui::Event::Down { x, y, button: min_ui::Button::Primary } if in_view => { if p.is_playing() && self.annot.tool == annotate::Tool::Select { } self.annot.press(geo.to_norm(x, y), frame, &geo); true }
                    min_ui::Event::Move { x, y } if self.annot.drawing() || (self.annot.tool == annotate::Tool::Eraser && g.annot_down) => { done = self.annot.motion(geo.to_norm(x, y), frame, fps, &geo, g.annot_down); true }
                    min_ui::Event::Up { x, y, button: min_ui::Button::Primary } if self.annot.drawing() || g.annot_down => { done = self.annot.release(geo.to_norm(x, y), frame, fps, &geo); true }
                    _ => false,
                };
                if let min_ui::Event::Down { button: min_ui::Button::Primary, .. } = e { if in_view { g.annot_down = true; } }
                if let min_ui::Event::Up { .. } = e { g.annot_down = false; }
                if in_view && g.cursor != winit::cursor::CursorIcon::Crosshair && self.annot.tool != annotate::Tool::Select { g.cursor = winit::cursor::CursorIcon::Crosshair; g.window.set_cursor(winit::cursor::Cursor::Icon(g.cursor)); }
                if used { g.ui_dirty = true; let _ = g; if let Some(c) = done { self.after_annotation(c); } return }
            }
        }
        let g = self.gpu.as_mut().unwrap();
        // the Split wipe's seam on the picture: a 14-px grab strip, the resize cursor, press snaps, drag follows (QCView kSeamGrabWidth)
        if let (Some(sx), Some((cx, cy, cw, ch))) = (g.presenter.engine.dual().filter(|d| d.mode == CompareMode::Split).map(|d| d.seam_x()), g.presenter.engine.content_rect()) {
            let in_pic = |x: f64, y: f64| x >= cx as f64 && x < (cx + cw) as f64 && y >= cy as f64 && y < (cy + ch) as f64;
            let near = |x: f64, y: f64| in_pic(x, y) && (x - sx).abs() <= 7.0 * scale;
            let set = |g: &mut Gpu, x: f64| { if let Some(ed) = g.presenter.engine.dual_mut() { ed.seam = (((x - cx as f64) / cw.max(1) as f64) as f32).clamp(0.0, 1.0); } g.ui_dirty = true; };
            let used = match e {
                min_ui::Event::Move { x, .. } if g.seam_drag => { set(g, x); true }
                min_ui::Event::Down { x, y, button: min_ui::Button::Primary } if near(x, y) && !grabbed => { g.seam_drag = true; set(g, x); true }
                min_ui::Event::Up { .. } if g.seam_drag => { g.seam_drag = false; g.ui_dirty = true; true }
                _ => false,
            };
            let hot = e.pos().is_some_and(|(x, y)| near(x, y)) && !matches!(e, min_ui::Event::Leave);
            if hot != g.seam_hot { g.seam_hot = hot; g.ui_dirty = true; }
            if hot || g.seam_drag { if g.cursor != winit::cursor::CursorIcon::ColResize { g.cursor = winit::cursor::CursorIcon::ColResize; g.window.set_cursor(winit::cursor::Cursor::Icon(g.cursor)); } }
            if used || (hot && !matches!(e, min_ui::Event::Leave)) { return }
        } else if g.seam_hot || g.seam_drag { g.seam_hot = false; g.seam_drag = false; g.ui_dirty = true; }
        let was_open = g.strip.is_open();
        let (sd, consumed) = if grabbed || g.strip_hidden { (false, false) } else { g.strip.event(&e, now, &mut g.fonts, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), scale) };
        if sd { g.ui_dirty = true; }
        if g.strip.is_open() != was_open { set_ime(&**g.window, g.strip.is_open()); }
        if consumed { return }
        let (cmds, dirty) = g.bar.event(&e, now);
        if dirty { g.ui_dirty = true; }
        let st = scrub_state(pb(&self.dual, &self.player), self.player.as_ref());
        // the clip bar (a playlist): its gestures commit timeline edits; while one is pressed it owns the pointer
        let mut bcmds = vec![];
        if let Some(ed) = self.pl_editor.as_ref() { if g.clip_bar.is_pressed() || e.pos().is_some_and(|(x, y)| g.clip_bar.hit(x, y)) || matches!(e, min_ui::Event::Leave) {
            let (c, d) = g.clip_bar.event(&e, now, self.mods.ctrl, &ed.timeline, &g.scrubber, &st, scale); if d { g.ui_dirty = true; } bcmds = c;
            // the edit scrub: the frame under the trim edge / the slipped content in the viewport while the gesture drags
            if let (Some((clip, src)), Some(p)) = (g.clip_bar.edit_preview(&ed.timeline, &g.scrubber, &st, scale), self.player.as_mut()) { if matches!(e, min_ui::Event::Move { .. }) { p.edit_preview(&clip, src); } }
            let want = match g.clip_bar.hover_zone() { Some(clip_bar::Zone::Head) | Some(clip_bar::Zone::Tail) => winit::cursor::CursorIcon::ColResize, Some(clip_bar::Zone::Body) => winit::cursor::CursorIcon::Grab, None => winit::cursor::CursorIcon::Default };
            if want != g.cursor { g.cursor = want; g.window.set_cursor(winit::cursor::Cursor::Icon(want)); }
        } }
        if matches!(e, min_ui::Event::Up { .. }) { if let Some(p) = self.player.as_mut() { p.end_edit_preview(); } }
        if !bcmds.is_empty() || g.clip_bar.is_pressed() { for c in bcmds { self.bar_cmd(c); } return }
        // dual's lanes (row 212): Shift moves a clip alone, Ctrl / Cmd slips; while one is pressed they own the pointer
        let mut lcmds = vec![];
        if let (Some(ed), true) = (self.dual_editor.as_ref(), self.dual.is_some()) { if g.dual_bars.is_pressed() || e.pos().is_some_and(|(x, y)| g.dual_bars.hit(x, y)) || matches!(e, min_ui::Event::Leave) {
            let (c, d) = g.dual_bars.event(&e, now, self.mods.ctrl, self.mods.shift, &ed.timeline, &g.scrubber, &st, scale); if d { g.ui_dirty = true; } lcmds = c;
            // the edit scrub: the frame at the trim edge / the slipped content on that side while the gesture drags
            if let (Some((side, clip, src)), Some(d)) = (g.dual_bars.edit_preview(&ed.timeline, &g.scrubber, &st, scale), self.dual.as_mut()) { if matches!(e, min_ui::Event::Move { .. }) { d.edit_preview(side, &clip, src); } }
            let want = match g.dual_bars.hover_zone() { Some(dual_bars::Zone::Head) | Some(dual_bars::Zone::Tail) => winit::cursor::CursorIcon::ColResize, Some(dual_bars::Zone::Body) => winit::cursor::CursorIcon::Grab, None => winit::cursor::CursorIcon::Default };
            if want != g.cursor { g.cursor = want; g.window.set_cursor(winit::cursor::Cursor::Icon(want)); }
        } }
        if matches!(e, min_ui::Event::Up { .. }) { if let Some(d) = self.dual.as_mut() { if d.edit_previewing() { d.end_edit_preview(); } } }
        if !lcmds.is_empty() || g.dual_bars.is_pressed() { for c in lcmds { self.dual_lane_cmd(c); } return }
        let g = self.gpu.as_mut().unwrap();
        let (scmds, sd) = g.scrubber.event(&e, now, &st, scale);
        if sd { g.ui_dirty = true; }
        if let Some(p) = pb_mut(&mut self.dual, &mut self.player) { for c in scmds { match c { min_ui::ScrubCmd::Begin => { if *TRACE_SCRUB { println!("scrub: begin at {:.1} ms", trace_ms()); } p.begin_scrub() } min_ui::ScrubCmd::To(f) => { if *TRACE_SCRUB { println!("scrub: request {f} at {:.1} ms", trace_ms()); self.scrub_last_req = Some((f, Instant::now())); } p.scrub_to_frame(f) } min_ui::ScrubCmd::End => { if *TRACE_SCRUB { println!("scrub: end at {:.1} ms", trace_ms()); } p.end_scrub() } } } }
        if g.fullscreen { let want = if st.frames > 0 && e.pos().is_some_and(|(x, y)| g.scrubber.hit(x, y)) { winit::cursor::CursorIcon::Pointer } else { winit::cursor::CursorIcon::Default }; if want != g.cursor { g.cursor = want; g.window.set_cursor(winit::cursor::Cursor::Icon(want)); } }
        for c in cmds {
            match c {
                transport_bar::Cmd::Act(a) => {
                    let handled = pb_mut(&mut self.dual, &mut self.player).map(|p| p.apply(a, now)).unwrap_or(false);
                    if !handled && a == Action::ToggleFullscreen { self.toggle_fullscreen(); return }
                    else if !handled && a == Action::CycleBackground { self.cycle_background(); }
                    else if !handled && (a == Action::Screenshot || a == Action::ScreenshotToClipboard) { self.screenshot(if a == Action::Screenshot { capture::Dest::File } else { capture::Dest::Clipboard }); }
                    else if !handled { log::info!("{a:?}: nothing to apply it to"); }
                }
                transport_bar::Cmd::Open => self.open_media_dialog(),
                transport_bar::Cmd::Pin => self.toggle_pin(),
                transport_bar::Cmd::ToggleCompact => { self.toggle_compact(); return }
                transport_bar::Cmd::RevealBackground => self.reveal(Reveal::Background),
                transport_bar::Cmd::RevealGuides => self.reveal(Reveal::Guides),
                transport_bar::Cmd::ToggleColor => self.color_cmd(color_drawer::Cmd::Toggle),
                transport_bar::Cmd::ToggleNotes => self.toggle_notes(),
                transport_bar::Cmd::Volume(v) => { if let Some(p) = self.player.as_mut() { p.audio.set_volume(v); } if let Some(d) = self.dual.as_mut() { d.for_each_audio(|au| au.set_volume(v)); } }
            }
        }
    }
    /// A settings change took: push it to whatever it drives (QCView's `WindowManager` setters, one by one).
    fn apply_setting(&mut self, c: settings_panel::Cmd) {
        use settings::k;
        let keys: Vec<&'static str> = match c { settings_panel::Cmd::Changed(k) => vec![k], settings_panel::Cmd::ResetAll => vec![k::TIMELINE_HOVER_THUMBS, k::TIMELINE_WAVEFORMS, k::VOLUME, k::MUTED, k::SYNC_OFFSET_MS, k::SCRUB_MUTED, k::LOOP_ENABLED, k::LEFT_RAIL_WIDTH, k::RIGHT_RAIL_WIDTH, k::LEFT_RAIL_COLLAPSED, k::RIGHT_RAIL_COLLAPSED, k::BACKGROUND_MODE, k::ALWAYS_ON_TOP] };
        let now = Instant::now();
        for key in keys {
            if key == k::LANGUAGE { i18n::apply(&self.settings.string(k::LANGUAGE)); }   // the panels relabel on the next tick (QCView: "Applies at once")
            let Some(g) = self.gpu.as_mut() else { return };
            match key {
                k::TIMELINE_HOVER_THUMBS => { g.thumbs.enabled = self.settings.bool(key); g.ui_dirty = true; }
                k::TIMELINE_WAVEFORMS => { g.waveform_enabled = self.settings.bool(key); if !g.waveform_enabled { g.wave_peaks.clear(); g.wave_req = (String::new(), 0.0, 0.0, 0); } g.ui_dirty = true; }
                k::VOLUME => { let v = self.settings.float(key) as f32; if let Some(p) = self.player.as_mut() { p.audio.set_volume(v); } if let Some(d) = self.dual.as_mut() { d.for_each_audio(|au| au.set_volume(v)); } }
                k::MUTED => { let m = self.settings.bool(key); if let Some(p) = self.player.as_mut() { p.audio.set_muted(m); } if let Some(d) = self.dual.as_mut() { d.for_each_audio(|au| au.set_muted(m)); } }
                k::DUAL_SYNC_OFFSET_MS => if let Some(d) = self.dual.as_mut() { let ms = self.settings.int(key) as i32; d.for_each_audio(|au| au.set_sync_offset_ms(ms)); },
                k::SYNC_OFFSET_MS => if let Some(p) = self.player.as_mut() { p.audio.set_sync_offset_ms(self.settings.int(key) as i32); },
                k::SCRUB_MUTED => { let m = self.settings.bool(key); if let Some(p) = self.player.as_mut() { p.audio.set_scrub_muted(m); } if let Some(d) = self.dual.as_mut() { d.for_each_audio(|au| au.set_scrub_muted(m)); } }
                k::LOOP_ENABLED => if let Some(p) = self.player.as_mut() { p.set_loop_enabled(self.settings.bool(key)); },
                k::LEFT_RAIL_WIDTH => { g.left.width = self.settings.int(key) as f64; g.ui_dirty = true; }
                k::RIGHT_RAIL_WIDTH => { g.right.width = self.settings.int(key) as f64; g.ui_dirty = true; }
                k::LEFT_RAIL_COLLAPSED => { g.left.set_collapsed(self.settings.bool(key), now); g.ui_dirty = true; }
                k::RIGHT_RAIL_COLLAPSED => { g.right.set_collapsed(self.settings.bool(key), now); g.ui_dirty = true; }
                k::SAFETY_COLOR | k::SAFETY_OPACITY | k::SAFETY_WIDTH | k::SAFETY_GUIDE => { g.ui_dirty = true; }
                k::BACKGROUND_MODE => { g.presenter.engine.set_background(self.settings.int(key) as u8); g.ui_dirty = true; }
                k::ALWAYS_ON_TOP => { let _ = g; self.apply_pin(); }
                _ => {}   // applies at the next open (performance/*) or has no consumer yet
            }
        }
    }
    /// QCView createPlaylistFromSelection / createEmptyPlaylistAndRename: auto-named Playlist_NN at 24 fps 1920×1080, gap 0;
    /// the new row is selected and its rename opens (QCView also activates it — that waits for playlist loading in the shell).
    fn create_playlist(&mut self, paths: &[String]) {
        let name = self.project.next_playlist_auto_name();
        let (id, probes) = self.project.create_playlist(paths, &name, 24.0, 1920, 1080, 0, 0);
        self.schedule_probes(probes);
        self.load_item(&id);   // QCView: setActiveItem — the new playlist loads (empty: no player, the bar invites a drop)
        if let Some(g) = self.gpu.as_mut() { g.project_panel.selected = vec![id.clone()]; g.project_panel.pending_rename = Some(id); g.ui_dirty = true; }
    }
    /// A clip-bar command: seeks / selection, or one undoable timeline edit followed by the sync.
    fn bar_cmd(&mut self, c: clip_bar::BarCmd) {
        use clip_bar::{BarCmd, TRACK};
        // dual: the precision popover edits the clip on the lane that opened it
        if self.dual.is_some() {
            let side = self.gpu.as_ref().and_then(|g| g.popover_side).unwrap_or(qcv_player::Side::B);
            let lc = match c { BarCmd::TrimHead { clip, new_start, new_source_in } => Some(dual_bars::LaneCmd::TrimHead { side, clip, new_start, new_source_in }), BarCmd::TrimTail { clip, new_source_out } => Some(dual_bars::LaneCmd::TrimTail { side, clip, new_source_out }), BarCmd::Slip { clip, new_in, new_out } => Some(dual_bars::LaneCmd::Slip { side, clip, new_in, new_out }), BarCmd::Seek(f) => Some(dual_bars::LaneCmd::Seek(f)), _ => None };
            if let Some(lc) = lc { self.dual_lane_cmd(lc); }
            return
        }
        let Some(ed) = self.pl_editor.as_mut() else { return };
        let (changed, structural) = match c {
            BarCmd::Seek(f) => { if let Some(p) = self.player.as_mut() { let fps = p.rate().as_f64(); if fps > 0.0 { p.seek_to_time(f as f64 / fps); } } (false, false) }
            BarCmd::Select(id) => { if id.is_some() { if let Some(g) = self.gpu.as_mut() { g.project_panel.selected.clear(); } } (false, false) }   // one selection at a time: the bar's or the bin's (QCView's mutual exclusion)
            BarCmd::TrimHead { clip, new_start, new_source_in } => (ed.trim_clip_head(TRACK, &clip, new_start, new_source_in), false),
            BarCmd::TrimTail { clip, new_source_out } => (ed.trim_clip_tail(TRACK, &clip, new_source_out), false),
            BarCmd::Slip { clip, new_in, new_out } => (ed.slip_clip(TRACK, &clip, new_in, new_out), false),
            BarCmd::Move { clip, index } => (ed.move_clip_to_index(TRACK, &clip, index), true),
            BarCmd::Precision(id) => {
                if let (Some(g), Some(c)) = (self.gpu.as_mut(), ed.timeline.tracks.first().and_then(|t| t.clips.iter().find(|c| c.id == id)).cloned()) {
                    let st = scrub_state(pb(&self.dual, &self.player), self.player.as_ref()); let scale = g.window.scale_factor(); let (w, h) = g.window.surface_size().into();
                    let anchor = kurbo::Rect::new(g.scrubber.x_of_frame(c.start, &st, scale), g.clip_bar.rect.y0, g.scrubber.x_of_frame(c.start + c.duration, &st, scale), g.clip_bar.rect.y1);
                    g.clip_popover.open(&c, ed.timeline.rate, anchor, &mut g.fonts, kurbo::Rect::new(0.0, 0.0, w, h), scale); set_ime(&**g.window, true);
                }
                (false, false)
            }
        };
        if changed { self.after_playlist_edit(structural); }
        if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; }
    }
    /// After a timeline edit: the project item takes the new order + trims (QCView writes them back; the dirty dot), and the
    /// transport plays the new timeline — trims / slips re-seat in place; a reorder / insert / delete / split rebuilds the
    /// playlist transport at the same position and play state (the slots are indexed by clip).
    fn after_playlist_edit(&mut self, structural: bool) {
        let Some(ed) = self.pl_editor.as_ref() else { return };
        let tl = ed.timeline.clone();
        let id = ed.playlist_media_item_id.clone();
        self.project.set_playlist_items(&id, playlist::entries_from(&tl));
        let has_clips = tl.tracks.first().is_some_and(|t| t.clips.iter().any(|c| !c.is_gap));
        if !has_clips { self.player = None; return }
        match self.player.as_mut() {
            Some(p) if !structural => p.set_timeline(tl),
            _ => {
                let (pos, playing) = self.player.as_ref().map(|p| (p.current_frame(), p.is_playing())).unwrap_or((0, false));
                let t = Instant::now();
                match Transport::open_playlist(tl, self.transport_options()) {
                    Ok(p) => { let name = self.source_label.clone(); self.adopt(p, name, "", t); if let Some(p) = self.player.as_mut() { let fps = p.rate().as_f64(); if fps > 0.0 { p.seek_to_time(pos.min(p.frame_count() - 1).max(0) as f64 / fps); } if playing { p.play(); } } }
                    Err(e) => println!("playlist rebuild: {e}"),
                }
            }
        }
    }
    /// The playlist's insert point for a drop at x: the clip edge within 12 px (QCView `snapTolPx`), else the end.
    fn playlist_drop_index(&self, x: f64) -> Option<(usize, f64)> {
        let (ed, g) = (self.pl_editor.as_ref()?, self.gpu.as_ref()?);
        let st = scrub_state(pb(&self.dual, &self.player), self.player.as_ref()); let scale = g.window.scale_factor();
        let tol = (12.0 * scale * g.scrubber.frames_per_px(&st)).ceil().max(1.0) as i64;
        let f = if self.player.is_some() { g.scrubber.frame_at_x(x, &st, scale) as i64 } else { 0 };
        let clips = ed.timeline.tracks.first().map(|t| t.clips.len()).unwrap_or(0);
        Some(clip_bar::ClipBar::insert_index(&ed.timeline, f, Some(tol), &g.scrubber, &st, scale).unwrap_or((clips, g.scrubber.x_of_frame(ed.timeline.duration, &st, scale))))
    }
    /// QCView's playlist drop: each item (already in the project) inserted at successive indices from the drop edge; audio is
    /// refused (`createPlaylist` / `insertClipAtIndex` rule).
    fn playlist_insert(&mut self, media_ids: &[String], x: f64) {
        let Some((idx, _)) = self.playlist_drop_index(x) else { return };
        self.playlist_insert_at(media_ids, idx);
    }
    /// Insert the media ids at clip-array index `idx` (one undoable insert each, in order; audio / playlists / dual views refused).
    fn playlist_insert_at(&mut self, media_ids: &[String], mut idx: usize) {
        let infos: Vec<_> = media_ids.iter().filter_map(|id| self.project.item(id)).filter(|m| { let ok = !matches!(m.media_type, qcv_project::MediaType::Audio | qcv_project::MediaType::Playlist | qcv_project::MediaType::DualPair); if !ok { log::info!("playlist drop: '{}' refused (audio / playlist / dual view)", m.name); } ok }).map(playlist::media_info).collect();
        let Some(ed) = self.pl_editor.as_mut() else { return };
        let mut n = 0;
        for info in &infos { if ed.insert_clip_at_index(clip_bar::TRACK, idx, info, Default::default()).is_some() { idx += 1; n += 1; } }
        if n > 0 { self.after_playlist_edit(true); }
        if let Some(g) = self.gpu.as_mut() { g.clip_bar.set_drop_hint(None); g.ui_dirty = true; }
    }
    fn project_cmd(&mut self, c: project_panel::Cmd) {
        match c {
            project_panel::Cmd::Load(id) => self.load_item(&id),
            // QCView `seekToPlaylistItem`: the n-th clip's start on the playlist timeline
            // a tree reorder = the clip bar's move: insertion slot `to` among the playlist's clips → the clip array's index
            project_panel::Cmd::MoveClip { clip, to } => {
                let idx = self.pl_editor.as_ref().and_then(|ed| ed.timeline.tracks.first()).map(|t| { let real: Vec<usize> = t.clips.iter().enumerate().filter(|(_, c)| !c.is_gap).map(|(k, _)| k).collect(); real.get(to).copied().unwrap_or(t.clips.len()) });
                if let Some(index) = idx { self.bar_cmd(clip_bar::BarCmd::Move { clip, index }); }
            }
            project_panel::Cmd::SeekClip(i) => { if let Some(p) = self.player.as_mut() { let start = p.timeline.tracks.first().and_then(|t| t.clips.iter().filter(|c| !c.is_gap).nth(i)).map(|c| c.start); let fps = p.rate().as_f64(); if let Some(s) = start && fps > 0.0 { p.seek_to_time(s as f64 / fps); } } }
            project_panel::Cmd::Remove(ids) => { let loaded_gone = ids.iter().any(|i| *i == self.loaded_item); let first_name = ids.first().and_then(|i| self.project.item(i)).map(|m| m.name.clone()).unwrap_or_default(); let n = self.project.remove_media_items(&ids); if n > 0 { self.toast_action(&if n == 1 { fill(trw("Removed %1"), &[&first_name]) } else { fill(trw("Removed %1 items"), &[&n]) }, min_ui::ToastKind::Notice, trw("Undo"), "undo-media-delete"); } if n > 0 && loaded_gone { self.player = None; self.loaded_item.clear(); } if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; } }
            project_panel::Cmd::AddMedia => self.open_media_dialog(),
            // QCView createPlaylistFromSelection / createEmptyPlaylistAndRename: auto-named Playlist_NN at 24 fps 1920×1080, gap 0;
            // the new row is selected and its rename opens (activation waits for playlist loading in the shell)
            project_panel::Cmd::CreatePlaylist(ids) => { let paths: Vec<String> = ids.iter().filter_map(|i| self.project.item(i).map(|m| m.path.clone())).collect(); self.create_playlist(&paths) }
            project_panel::Cmd::NewPlaylist => self.create_playlist(&[]),
            // rows released over the viewport load the first (QCView: the bin's drag → the viewport DropArea → openMediaPaths; the rows
            // are already in the project, so this is the load); over the bin / the bands / outside the window nothing happens
            // rows released on the loaded playlist's tree (ours): insert at its line, or append on the playlist's row
            project_panel::Cmd::DropRows(ids, x, y) if self.gpu.as_ref().and_then(|g| g.project_panel.tree_drop_slot(x, y)).is_some() => {
                let to = self.gpu.as_ref().and_then(|g| g.project_panel.tree_drop_slot(x, y)).unwrap_or(0);
                let idx = self.pl_editor.as_ref().and_then(|ed| ed.timeline.tracks.first()).map(|t| { let real: Vec<usize> = t.clips.iter().enumerate().filter(|(_, c)| !c.is_gap).map(|(k, _)| k).collect(); real.get(to).copied().unwrap_or(t.clips.len()) });
                if let Some(i) = idx { self.playlist_insert_at(&ids, i); }
            }
            project_panel::Cmd::DropRows(ids, x, y) => { if let Some(DropTarget::Side(sd)) = self.drop_target(x, y) { if let Some(first) = ids.first().cloned() { if sd == qcv_player::Side::B { self.set_b_source(&first); } else { self.load_item(&first); } } } else if let Some(DropTarget::Playlist(_)) = self.drop_target(x, y) { self.playlist_insert(&ids, x); } else if self.drop_target(x, y) == Some(DropTarget::Viewport) { if let Some(first) = ids.first() { self.load_item(first); } } if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; } }
            project_panel::Cmd::Rename(id, name) => { if self.project.rename_media_item(&id, &name) { if id == self.loaded_item { self.source_label = name; } } if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; } }
        }
    }
    /// Compact mode (QCView's `toggleCompact` / `exitCompact`): the window drops its title bar, every rail and band goes, one
    /// 32-px strip stays; fullscreen is left first (QCView refuses the window-style change while fullscreen).
    /// A key → the in-window bar's view of it (None = not the bar's). Tracks Windows' Alt tap: Alt down and up with nothing
    /// between. Letters come from the physical key (Alt+F on any layout's F position, as Qt's mnemonics on Windows).
    fn bar_key(&mut self, event: &winit::event::KeyEvent, pressed: bool) -> Option<min_ui::BarKey> {
        use winit::keyboard::PhysicalKey;
        let active = self.gpu.as_ref().and_then(|g| g.menu_bar.as_ref()).is_some_and(|m| m.bar.is_active());
        self.gpu.as_ref()?.menu_bar.as_ref()?;
        if matches!(event.logical_key, Key::Named(NamedKey::Alt)) {
            if pressed { if !event.repeat { self.alt_clean = true; } return None }
            let tap = std::mem::take(&mut self.alt_clean); return if tap && !self.mods.ctrl && !self.mods.shift { Some(min_ui::BarKey::AltTap) } else { None };
        }
        if !pressed { return None }
        self.alt_clean = false;
        if matches!(event.logical_key, Key::Named(NamedKey::F10)) && !self.mods.ctrl && !self.mods.shift { return Some(min_ui::BarKey::F10) }
        let letter = match event.physical_key { PhysicalKey::Code(c) => { let n = format!("{c:?}"); n.strip_prefix("Key").filter(|l| l.len() == 1).and_then(|l| l.chars().next()).map(|ch| ch.to_ascii_lowercase()) } _ => None };
        if self.mods.alt && !self.mods.ctrl { if let Some(ch) = letter { return Some(min_ui::BarKey::Alt(ch)) } }
        if !active { return None }
        Some(match &event.logical_key {
            Key::Named(NamedKey::Escape) => min_ui::BarKey::Named(min_ui::Named::Escape),
            Key::Named(NamedKey::Enter) => min_ui::BarKey::Named(min_ui::Named::Enter),
            Key::Named(NamedKey::ArrowUp) => min_ui::BarKey::Named(min_ui::Named::Up),
            Key::Named(NamedKey::ArrowDown) => min_ui::BarKey::Named(min_ui::Named::Down),
            Key::Named(NamedKey::ArrowLeft) => min_ui::BarKey::Named(min_ui::Named::Left),
            Key::Named(NamedKey::ArrowRight) => min_ui::BarKey::Named(min_ui::Named::Right),
            Key::Character(c) if c == " " => min_ui::BarKey::Space,
            Key::Named(NamedKey::Tab) => min_ui::BarKey::Named(min_ui::Named::Tab),
            _ => match letter { Some(ch) => min_ui::BarKey::Char(ch), None => min_ui::BarKey::Other },   // anything else: swallowed while the bar holds the keyboard
        })
    }
    /// A menu command (QCView's `onTriggered`s); the keys that share them call the same functions.
    fn menu_cmd(&mut self, c: menu::Cmd, el: &dyn ActiveEventLoop) {
        log::info!("menu: {c:?}");
        use menu::Cmd as M;
        let now = Instant::now();
        match c {
            M::OpenMedia => self.open_media_dialog(),
            M::NewProject => self.new_project(),
            M::OpenProject => self.open_project_dialog(),
            M::SaveProject => self.save_project(),
            M::SaveProjectAs => self.save_project_as(),
            M::RecentMedia(p) => { if std::path::Path::new(&p).exists() { for a in self.admit_media_paths(&[p]) { self.add_and_load(&a, true); } } else { self.toast(trw("That file is no longer there"), min_ui::ToastKind::Notice); } }
            M::RecentProject(p) => { if std::path::Path::new(&p).exists() { self.open_project(&p); } else { self.toast(trw("That project is no longer there"), min_ui::ToastKind::Notice); } }
            M::ClearRecentMedia => { self.settings.set_str(settings::k::RECENT_MEDIA, ""); }
            M::ClearRecentProjects => { self.settings.set_str(settings::k::RECENT_PROJECTS, ""); }
            M::Quit => { self.finish(); el.exit(); }
            M::Minimal => self.view_minimal(),
            M::AllPanels => self.view_all_panels(),
            M::Compact => self.toggle_compact(),
            M::Pin => self.toggle_pin(),
            M::ZoomIn | M::ZoomOut => { if let Some(g) = self.gpu.as_mut() { let st = scrub_state(pb(&self.dual, &self.player), self.player.as_ref()); let scale = g.window.scale_factor(); let x = g.scrubber.playhead_x(&st, scale); if g.scrubber.wheel(x, if c == M::ZoomOut { -600.0 } else { 600.0 }, false, &st, scale) { g.ui_dirty = true; } } }
            M::Fullscreen => self.toggle_fullscreen(),
            M::DefaultFrame => {
                // QCView `resetWindowFrame`: fullscreen left first, then the first-run size centred on this screen
                if self.gpu.as_ref().is_some_and(|g| g.fullscreen) { self.toggle_fullscreen(); }
                if let Some(g) = self.gpu.as_ref() {
                    let size = winit::dpi::LogicalSize::new(1280.0, 720.0);
                    let _ = g.window.request_surface_size(size.into());
                    if let Some(mon) = g.window.current_monitor() && let (Some(mp), Some(ms)) = (mon.position(), mon.current_video_mode().map(|v| v.size())) {
                        let sc = g.window.scale_factor(); let (w, h) = (1280.0 * sc, 720.0 * sc);
                        g.window.set_outer_position(winit::dpi::PhysicalPosition::new(mp.x + ((ms.width as f64 - w) / 2.0) as i32, mp.y + ((ms.height as f64 - h) / 2.0) as i32).into());
                    }
                }
            }
            M::LeftRail | M::RightRail => { if let Some(g) = self.gpu.as_mut() { if !g.fullscreen { if c == M::LeftRail { g.left.toggle(now); } else { g.right.toggle(now); } g.ui_dirty = true; } } }
            M::StatusBar => self.toggle_status_bar(),
            M::Background => self.reveal(Reveal::Background),
            M::Shortcuts => self.reveal_shortcuts(),
            M::Notes => self.toggle_notes(),
            M::SafetyGuides => self.reveal(Reveal::Guides),
            M::ColorPanel => { if self.gpu.as_ref().is_some_and(|g| !g.fullscreen && !g.compact) { self.color_cmd(color_drawer::Cmd::Toggle); } }
            M::Settings => self.reveal(Reveal::Settings),
            M::CopyProjectLink => self.copy_project_link(),
            M::Docs => { let _ = os_actions::open("https://qcview.app/"); }
            // QCView's own About modal (Windows; macOS shows the platform panel from the app menu)
            M::About => { if let Some(g) = self.gpu.as_mut() { if let Some(mb) = g.menu_bar.as_mut() { mb.bar.cancel(); } g.about.show(); } }
        }
        if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; }
    }
    /// QCView `dispatchOpenArgsOnPrimary` (+ raising the window): `qcview://` links through `openProjectLink`, `.qcvproj` opens,
    /// other URLs are live streams (not landed: a toast), files are added as a drop is — the first one loads.
    fn open_forwarded(&mut self, args: Vec<String>) {
        if let Some(g) = self.gpu.as_ref() { g.window.set_minimized(false); g.window.focus_window(); }
        let mut files = vec![];
        for a in args {
            if a.get(..9).is_some_and(|p| p.eq_ignore_ascii_case("qcview://")) { self.open_project_link(&a); }
            else if a.contains("://") { self.toast(trw("Live streams arrive with the stream step"), min_ui::ToastKind::Notice); }
            else if a.to_lowercase().ends_with(".qcvproj") { self.open_project(&a); }
            else { files.push(a); }
        }
        let mut first = true;
        for p in self.admit_media_paths(&files) { if self.add_and_load(&p, first).is_some() { first = false; } }
    }
    /// QCView `openProjectLink`: `qcview://<uuid>[/<mediaId>]` → the RECENT project whose JSON `uuid` matches (QCView: links
    /// resolve only to projects in this user's Recents), opened, the media item loaded. Stream links wait for live streams.
    fn open_project_link(&mut self, uri: &str) -> bool {
        match min_paths::links::parse_qcview_link(uri) {
            Some(min_paths::links::QcviewLink::Project { uuid, media_id }) => {
                let recents: Vec<String> = self.settings.string(settings::k::RECENT_PROJECTS).lines().filter(|l| !l.is_empty()).map(str::to_string).collect();
                let hit = recents.into_iter().find(|p| std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()).and_then(|v| v.get("uuid").and_then(|u| u.as_str()).map(|u| u == uuid)).unwrap_or(false));
                let Some(path) = hit else { self.toast("That project isn’t in your recent projects", min_ui::ToastKind::Notice); log::warn!("openProjectLink: no recent project with uuid {uuid}"); return false };
                self.open_project(&path);
                if let Some(m) = media_id && self.project.item(&m).is_some() { self.load_item(&m); }
                true
            }
            Some(min_paths::links::QcviewLink::Stream { .. }) => { self.toast(trw("Live streams arrive with the stream step"), min_ui::ToastKind::Notice); false }
            None => { log::warn!("openProjectLink: not a qcview:// link"); false }
        }
    }
    /// QCView `copyProjectLinkToClipboard` → `buildProjectLink`: `qcview://<project uuid>/<loaded media id>`.
    fn copy_project_link(&mut self) {
        match min_paths::links::qcview_project_link(self.project.uuid(), &self.loaded_item) {
            Some(link) => { match os_actions::copy(&link) { Ok(()) => self.toast(trw("Project link copied"), min_ui::ToastKind::Positive), Err(e) => self.toast(&fill(trw("Couldn’t copy: %1"), &[&e]), min_ui::ToastKind::Negative) } }
            None => self.toast(trw("Load an item to copy its link"), min_ui::ToastKind::Notice),
        }
    }
    /// QCView `viewMinimal`: both rails collapsed (their reopen tabs stay), the status strip hidden.
    fn view_minimal(&mut self) { if let Some(g) = self.gpu.as_mut() { if !g.fullscreen && !g.compact { let now = Instant::now(); g.left.set_collapsed(true, now); g.right.set_collapsed(true, now); g.strip_hidden = true; g.ui_dirty = true; } } }
    /// QCView `viewAllPanels` → `viewDefault`: the rails open, the strip back.
    fn view_all_panels(&mut self) { if let Some(g) = self.gpu.as_mut() { if !g.fullscreen && !g.compact { let now = Instant::now(); g.left.set_collapsed(false, now); g.right.set_collapsed(false, now); g.strip_hidden = false; g.ui_dirty = true; } } }
    /// View ▸ Status Bar (Ctrl+5).
    fn toggle_status_bar(&mut self) { if let Some(g) = self.gpu.as_mut() { if !g.fullscreen && !g.compact { g.strip_hidden = !g.strip_hidden; log::info!("status bar {}", if g.strip_hidden { "hidden" } else { "shown" }); g.content_set = None; g.ui_dirty = true; } } }
    fn toggle_compact(&mut self) {
        if self.gpu.as_ref().is_some_and(|g| g.fullscreen) { self.toggle_fullscreen(); }
        let Some(g) = self.gpu.as_mut() else { return };
        g.compact = !g.compact;
        g.window.set_decorations(!g.compact);
        g.content_set = None; g.ui_dirty = true;
        if g.compact { g.strip.close(); g.settings_panel.blur(&mut self.settings); }
        let s = g.window.surface_size(); let sc = g.window.scale_factor();
        g.presenter.engine.set_content_rect(Some(content_rect(s.width, s.height, sc)));
    }
    /// QCView's `cycleBackgroundMode` (B): the next of the four fills.
    fn cycle_background(&mut self) { let m = (self.settings.int(settings::k::BACKGROUND_MODE) + 1) % 4; self.settings.set_int(settings::k::BACKGROUND_MODE, m); self.apply_setting(settings_panel::Cmd::Changed(settings::k::BACKGROUND_MODE)); }
    /// Always on Top (QCView's pin; `ui/alwaysOnTop`): the OS window level.
    fn toggle_pin(&mut self) { let on = !self.settings.bool(settings::k::ALWAYS_ON_TOP); self.settings.set_bool(settings::k::ALWAYS_ON_TOP, on); self.apply_pin(); }
    fn apply_pin(&mut self) {
        let on = self.settings.bool(settings::k::ALWAYS_ON_TOP);
        let Some(g) = self.gpu.as_mut() else { return };
        g.pinned = on; g.bar.set_pinned(on);
        g.window.set_window_level(if on { winit::window::WindowLevel::AlwaysOnTop } else { winit::window::WindowLevel::Normal });
        g.ui_dirty = true;
    }
    /// QCView's `revealSection`: open the left rail, expand that section alone, scroll it to the top.
    fn reveal(&mut self, which: Reveal) {
        let Some(g) = self.gpu.as_mut() else { return };
        let now = Instant::now();
        g.left.set_collapsed(false, now);
        g.background_panel.section.expanded = which == Reveal::Background;
        g.guides_panel.section.expanded = which == Reveal::Guides;
        g.settings_panel.section.expanded = which == Reveal::Settings;
        g.left.scroll = 0.0;   // Background sits first; with Settings alone expanded its header is at the top too
        g.ui_dirty = true;
    }
    /// QCView's `RightRail.revealSection("shortcuts")`: open the right rail, expand the card, scroll its header to the top.
    fn reveal_shortcuts(&mut self) {
        let Some(g) = self.gpu.as_mut() else { return };
        if g.fullscreen || g.compact { return }
        g.right.set_collapsed(false, Instant::now()); g.shortcuts.section.expanded = true; g.shortcuts_reveal = true; g.ui_dirty = true;
    }
    fn after_show(&mut self) {
        let frame_no = self.player.as_ref().and_then(|p| p.current()).map(|h| h.frame_no).unwrap_or(-1);
        let g = self.gpu.as_mut().unwrap();
        if let Some((path, n)) = &self.o.snap_out { if frame_no == *n as i32 && !self.snapped { self.snapped = true; match g.presenter.engine.snapshot(Snapshot::Encoded) { Ok((px, w, h)) => { write_png_rgb8(path, &px, w, h); println!("snap-out written: {path} ({w}x{h}, display-encoded 8-bit, frame {n})"); } Err(e) => println!("snap-out: {e}") } } }
        if self.o.verify_pq && frame_no == 24 && !self.verified { self.verified = true; self.verify_pq(); }
    }
    /// S1's check for the synthetic PQ ramp fixture (rows < H/2: grey ramp code = x/(W-1); then bands at 0.5081 / 0.7518 / 0.9026).
    fn verify_pq(&mut self) {
        let g = self.gpu.as_mut().unwrap();
        let Ok((px, w, h)) = g.presenter.engine.snapshot(Snapshot::Linear) else { return };
        let pq = |c: f64| { let (m1, m2, c1, c2, c3) = (0.1593017578125, 78.84375, 0.8359375, 18.8515625, 18.6875); let e = c.powf(1.0 / m2); ((e - c1).max(0.0) / (c2 - c3 * e)).powf(1.0 / m1) * 10000.0 };
        let white = self.o.white as f64; let row = (h / 4) as usize;
        let (mut worst, mut worst_x, mut over1) = (0f64, 0usize, 0u32);
        for x in 0..w as usize {
            let code = x as f64 / (w - 1) as f64; let exp = pq(code) / white; let got = px[(row * w as usize + x) * 4] as f64;
            if got > 1.0 { over1 += 1 }
            if exp > 0.001 { let rel = ((got - exp) / exp).abs(); if rel > worst { worst = rel; worst_x = x; } }
        }
        let bands: Vec<String> = [0.5081f64, 0.7518, 0.9026].iter().enumerate().map(|(i, code)| { let y = (h / 2 + (h / 6) * i as u32 + h / 12) as usize; let got = px[(y * w as usize + w as usize / 2) * 4] as f64; format!("band {code:.4} → expect {:.4} ({:.0} nit) got {:.4} ({:.0} nit)", pq(*code) / white, pq(*code), got, got * white) }).collect();
        println!("verify-pq (row {row}, {} nits = 1.0): worst relative error {:.3} % at x={worst_x} (code {:.3}); {} of {} samples above 1.0 (EDR range)\n  {}", self.o.white, worst * 100.0, worst_x as f64 / (w - 1) as f64, over1, w, bands.join("\n  "));
    }
    /// Enter / leave fullscreen (QCView's F): the content rect becomes the whole window, the bands hide until hovered.
    fn toggle_fullscreen(&mut self) {
        let Some(g) = self.gpu.as_mut() else { return };
        let fs = g.window.fullscreen();
        let entering = fs.is_none();
        g.window.set_fullscreen(if entering { Some(Fullscreen::Borderless(None)) } else { None });
        g.fullscreen = entering;
        let now = Instant::now();
        g.bands.set_target(if entering { 0.0 } else { 1.0 }, now); g.bands_leave = None;
        let s = g.window.surface_size(); let sc = g.window.scale_factor();
        g.content_set = None;
        g.presenter.engine.set_content_rect(Some(if entering { (0, 0, s.width, s.height) } else { content_rect(s.width, s.height, sc) }));
        g.ui_dirty = true;
    }
    fn finish(&mut self) {
        i18n::write_misses();
        if let Some(g) = self.gpu.as_mut() {
            g.thumbs.shutdown(); g.thumbs_b.shutdown();
            if !g.fullscreen {
                let maximized = g.window.is_maximized();
                self.settings.set_bool(settings::k::WINDOW_MAXIMIZED, maximized);
                if !maximized { if let Ok(p) = g.window.outer_position() { self.settings.set_int(settings::k::WINDOW_X, p.x as i64); self.settings.set_int(settings::k::WINDOW_Y, p.y as i64); } let s = g.window.surface_size(); self.settings.set_int(settings::k::WINDOW_WIDTH, s.width as i64); self.settings.set_int(settings::k::WINDOW_HEIGHT, s.height as i64); }
            }
        }
        if let Err(e) = self.settings.save_if_dirty() { log::warn!("settings: {e}"); }
        if self.finished { return } self.finished = true;
        let wall = self.t0.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0);
        if let Some(p) = self.player.as_mut() {
            let s = &mut p.stats;
            println!("play: shown {} frames, dropped {}, late (> 1 refresh) {}, black refreshes {}, loops {}; lateness ms vs the clock p50 {:.1} p95 {:.1} max {:.1} min {:.1}; cadence (refreshes per frame → count) {:?}; decode ms p50 {:.1} max {:.1}{}",
                s.shown, s.dropped, s.late, s.black, s.loops, pct(&mut s.lateness_ms, 50.0), pct(&mut s.lateness_ms, 95.0), pct(&mut s.lateness_ms, 100.0), pct(&mut s.lateness_ms, 0.0), s.cadence, pct(&mut s.decode_ms, 50.0), pct(&mut s.decode_ms, 100.0),
                if s.seek_land_ms.is_empty() { String::new() } else { format!("; stream seeks landed ms p50 {:.1} p95 {:.1} (n={})", pct(&mut s.seek_land_ms, 50.0), pct(&mut s.seek_land_ms, 95.0), s.seek_land_ms.len()) });
        }
        if let Some(p) = self.player.as_ref() { if let Some(idx) = p.decoder().frame_index() { println!("frame index: tier 2 {:?} ({} frames, {} keyframes); decode errors {}{}", idx.scan_state(), idx.scan_frames_built(), idx.keyframe_count(), p.decoder().decode_error_count(), if p.decoder().decode_error_count() > 0 { format!(" (last: {})", p.decoder().last_decode_error()) } else { String::new() }); } }
        let cpu = qcv_player::process_cpu_seconds();
        if wall > 0.0 { println!("process CPU time {cpu:.2} s over {wall:.1} s = {:.0} % of one core (decode threads included); max RSS {:.0} MB; frames presented {}; EDR headroom seen {:.1}", 100.0 * cpu / wall, qcv_player::max_rss_mb(), self.frames_presented, self.edr_seen); }
        if let Some(g) = self.gpu.as_ref() { let r = g.presenter.layer_report(); if !r.is_empty() { println!("{r}"); } }
        // the teardown, timed stage by stage (the user 2026-11-07: "it takes a bit to close" on Windows). Order: the player
        // (its decoders hold hw frames), then the hw device context WHILE the GPU device is alive, then the presenter, then the window.
        let t_all = Instant::now();
        let t = Instant::now(); if let Some(mut p) = self.player.take() { p.audio.close(); let audio_ms = t.elapsed().as_secs_f32() * 1000.0; let t2 = Instant::now(); drop(p); println!("close: audio {audio_ms:.0} ms · player (decoders, scrub session, index) {:.0} ms", t2.elapsed().as_secs_f32() * 1000.0); }
        eprintln!("close: dropping the hw device context…");
        let t = Instant::now(); let had_hw = self.hw.take().is_some(); println!("close: hw device {} {:.0} ms", if had_hw { "dropped" } else { "none" }, t.elapsed().as_secs_f32() * 1000.0);
        if let Some(g) = self.gpu.take() {
            let t = Instant::now(); drop(g.waveform); let wave_ms = t.elapsed().as_secs_f32() * 1000.0;
            let t = Instant::now(); drop(g.thumbs); let thumbs_ms = t.elapsed().as_secs_f32() * 1000.0;
            eprintln!("close: dropping the presenter…");
            let t = Instant::now(); drop(g.ui); drop(g.presenter); let gpu_ms = t.elapsed().as_secs_f32() * 1000.0;
            let t = Instant::now(); drop(g.window); let win_ms = t.elapsed().as_secs_f32() * 1000.0;
            println!("close: waveform {wave_ms:.0} ms · thumbnails {thumbs_ms:.0} ms · gpu {gpu_ms:.0} ms · window {win_ms:.0} ms");
        }
        println!("close: total {:.0} ms · wall {}", t_all.elapsed().as_secs_f32() * 1000.0, unix_ms());
    }
}

fn dual_dialog() -> min_ui::Dialog { min_ui::Dialog::new(trv("Save dual view"), "", vec![(trv("Cancel"), min_ui::Variant::Secondary), (trv("Save"), min_ui::Variant::Primary)]).with_field(trv("Name"), "") }
fn about_dialog() -> min_ui::Dialog { min_ui::Dialog::new("QCView", &format!("{}\n\n{}", fill(trm("Version %1"), &[&env!("CARGO_PKG_VERSION")]), trm("Color-accurate review player.")), vec![(trm("Docs"), min_ui::Variant::Secondary), (trm("Close"), min_ui::Variant::Primary)]) }

/// A language switch (canon row 197, QCView's `retranslate`): every panel's construction-time labels in the new language;
/// what they show each frame (values, rows built at sync) follows on its own. An open dialog keeps its words until it closes.
fn relabel(g: &mut Gpu) {
    g.settings_panel.relabel(); g.background_panel.relabel(); g.guides_panel.relabel(); g.project_panel.relabel();
    g.inspector.relabel(); g.shortcuts.relabel(); g.bar.relabel(); g.strip.relabel(); g.compact_strip.relabel();
    g.dualc.relabel(); g.clip_popover.relabel(); g.drawer.relabel(); g.notes.relabel();
    if !g.about.open { g.about = about_dialog(); }
    if !g.dual_dialog.open { g.dual_dialog = dual_dialog(); }
    if g.menu_bar.is_some() { g.menu_bar = Some(menu::InWindowMenu::new(&mut g.fonts)); }
    g.ui_dirty = true;
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Reveal { Background, Guides, Settings }

fn describe(p: &Transport) -> String {
    let i = p.info();
    format!("{} {}x{} {} fps ({} frames) {} {:?} {:?} {:?} {:?} {}-bit  decode {}{}", i.codec_short, i.width, i.height, i.rate(), i.frame_count, i.pix_fmt, i.meta.matrix, i.meta.transfer, i.meta.primaries, i.meta.range, i.meta.bit_depth,
        if i.hw_accel.is_empty() { "software + upload".to_string() } else { format!("{} (zero-copy when the layout is bridged)", i.hw_accel) }, i.source_timecode.as_ref().map(|t| format!("  TC {t}")).unwrap_or_default())
}

/// IME on while a text node has focus (winit 0.31's request API; the deprecated `set_ime_allowed` is gone).
fn set_ime(window: &dyn Window, on: bool) {
    use winit::window::{ImeCapabilities, ImeEnableRequest, ImeRequest, ImeRequestData};
    let req = if on { match ImeEnableRequest::new(ImeCapabilities::new(), ImeRequestData::default()) { Some(r) => ImeRequest::Enable(r), None => return } } else { ImeRequest::Disable };
    if let Err(e) = window.request_ime_update(req) { log::debug!("ime {on}: {e}"); }
}

/// winit's key to the UI's (S3's table; the strip and the settings fields share it).
/// The floating tabs (the rails' and the drawer's, the user 2026-11-08): each shows while the pointer is over the picture or
/// its own panel, and all fade once the pointer has parked over the picture for the bands' hide delay (~2 s) — unless it
/// rests on a tab. Outside the window (or without focus, which the shell reports as a leave) they are gone.
fn tab_presence(g: &mut Gpu, now: Instant) {
    let (px, py) = g.pointer;
    let inside = px >= 0.0 && g.window.has_focus();
    let pt = kurbo::Point::new(px, py);
    let view = g.presenter.engine.content_rect().map(|(x, y, w, h)| kurbo::Rect::new(x as f64, y as f64, (x + w) as f64, (y + h) as f64)).unwrap_or(kurbo::Rect::ZERO);
    let in_view = inside && view.contains(pt);
    let on_tab = g.left.hovered() && !g.left.hit(px, py) || g.right.hovered() && !g.right.hit(px, py) || g.drawer.tab_hovered();
    let parked = in_view && !on_tab && now.duration_since(g.last_move).as_millis() as u32 >= t::BANDS_HIDE_DELAY;
    let show = |own: bool| inside && (in_view || own) && !parked;
    let (l, r, d) = (show(g.left.hit(px, py)), show(g.right.hit(px, py)), show(g.drawer.rect.contains(pt)));
    g.left.set_pointer_present(l, now); g.right.set_pointer_present(r, now); g.drawer.set_pointer_present(d, now);
}

fn to_key_event(event: &winit::event::KeyEvent, mods: qcv_player::Mods) -> min_ui::KeyEvent {
    let key = match &event.logical_key {
        Key::Named(NamedKey::Escape) => min_ui::Key::Named(min_ui::Named::Escape), Key::Named(NamedKey::Enter) => min_ui::Key::Named(min_ui::Named::Enter), Key::Named(NamedKey::Tab) => min_ui::Key::Named(min_ui::Named::Tab),
        Key::Named(NamedKey::Backspace) => min_ui::Key::Named(min_ui::Named::Backspace), Key::Named(NamedKey::Delete) => min_ui::Key::Named(min_ui::Named::Delete),
        Key::Named(NamedKey::ArrowLeft) => min_ui::Key::Named(min_ui::Named::Left), Key::Named(NamedKey::ArrowRight) => min_ui::Key::Named(min_ui::Named::Right),
        Key::Named(NamedKey::ArrowUp) => min_ui::Key::Named(min_ui::Named::Up), Key::Named(NamedKey::ArrowDown) => min_ui::Key::Named(min_ui::Named::Down),
        Key::Named(NamedKey::Home) => min_ui::Key::Named(min_ui::Named::Home), Key::Named(NamedKey::End) => min_ui::Key::Named(min_ui::Named::End),
        Key::Character(c) => min_ui::Key::Char(c.to_string()), _ => min_ui::Key::Char(String::new()),
    };
    min_ui::KeyEvent { key, text: event.text.as_ref().map(|t| t.to_string()), shift: mods.shift, cmd: mods.ctrl }
}
fn st_hit(p: Option<&Transport>, sc: &min_ui::Scrubber, pos: Option<(f64, f64)>) -> bool { p.is_some_and(|p| p.frame_count() > 0) && pos.is_some_and(|(x, y)| sc.hit(x, y)) }

/// The active image sequence's failed / missing frames as TIMELINE frames (a playlist clip: offset by its start).
fn sequence_failed_on_timeline(p: &Transport) -> Vec<i32> {
    let Some(seq) = p.decoder().as_sequence() else { return vec![] };
    let slot = p.active_slot();
    let start = p.timeline.tracks.first().and_then(|t| t.clips.get(slot.clip)).map(|c| c.start).unwrap_or(0);
    let (lo, hi) = (slot.src_in, slot.src_out);
    seq.cache().failed_frames().into_iter().filter(|&f| !p.is_playlist() || (f as i64 >= lo && (f as i64) < hi)).map(|f| if p.is_playlist() { (start + f as i64 - lo) as i32 } else { f }).collect()
}

/// The scrubber's view of the Transport.
/// The loaded playhead, single or dual (the bands read either through `Playback`).
fn pb<'a>(dual: &'a Option<qcv_player::DualTransport>, player: &'a Option<Transport>) -> Option<&'a dyn Playback> { match dual { Some(d) => Some(d), None => player.as_ref().map(|p| p as &dyn Playback) } }
fn pb_mut<'a>(dual: &'a mut Option<qcv_player::DualTransport>, player: &'a mut Option<Transport>) -> Option<&'a mut dyn Playback> { match dual { Some(d) => Some(d), None => player.as_mut().map(|p| p as &mut dyn Playback) } }
fn scrub_state(p: Option<&dyn Playback>, single: Option<&Transport>) -> min_ui::ScrubState {
    let Some(p) = p else { return Default::default() };
    let cached = single.and_then(|p| p.decoder().as_sequence()).map(|s| (s.cache().buffered_behind_coverage(), s.cache().buffered_ahead_coverage()));   // the band: sequences only (QCView's `currentMediaKind === 1`)
    min_ui::ScrubState { frames: p.frame_count(), fps: p.rate().as_f64(), current: p.current_frame(), in_point: p.in_point(), out_point: p.out_point(), playing: p.is_playing(), scrubbing: p.is_scrubbing(), cached }
}

/// The video's rect: everything above the three bottom bands (canon §25 row 169).
fn content_rect(w: u32, h: u32, scale: f64) -> (u32, u32, u32, u32) {
    let bands = ((t::STATUS_STRIP_HEIGHT + t::TRANSPORT_BAR_HEIGHT + t::SCRUBBER_HEIGHT) as f64 * scale).round() as u32;
    (0, 0, w, h.saturating_sub(bands).max(1))
}

/// The Inspector's subject: dual's target side (slot 1 = B), else the single view (0).
fn insp_slot(dual: &Option<qcv_player::DualTransport>, g: &Gpu) -> usize { if dual.is_some() && g.dual_bars.target == qcv_player::Side::B { 1 } else { 0 } }
fn insp_transport<'a>(dual: &'a mut Option<qcv_player::DualTransport>, player: &'a mut Option<Transport>, slot: usize) -> Option<&'a mut Transport> { match dual { Some(d) => d.side_mut(if slot == 1 { qcv_player::Side::B } else { qcv_player::Side::A }), None => player.as_mut() } }
/// An item's per-clip choices as the player / engine take them (QCView's `loadRequested` applies them).
fn range_of(m: &qcv_project::MediaItem) -> qcv_player::RangeOverride { match m.video_range_override { qcv_project::VideoRange::Full => qcv_player::RangeOverride::Full, qcv_project::VideoRange::Limited => qcv_player::RangeOverride::Limited, _ => qcv_player::RangeOverride::Auto } }
fn routing_of(m: &qcv_project::MediaItem) -> qcv_player::RoutingMode { match m.audio_routing_mode { qcv_project::AudioRoutingMode::Downmix5_1 => qcv_player::RoutingMode::Downmix5_1, qcv_project::AudioRoutingMode::Stereo7_8 => qcv_player::RoutingMode::Stereo7_8, _ => qcv_player::RoutingMode::Auto } }
fn par_of(m: &qcv_project::MediaItem) -> min_present::engine::Par { match m.pixel_aspect_mode { qcv_project::PixelAspectMode::Square => min_present::engine::Par::Square, qcv_project::PixelAspectMode::Detected => min_present::engine::Par::Detected, qcv_project::PixelAspectMode::Custom => min_present::engine::Par::Custom(m.custom_par_num.max(1) as u32, m.custom_par_den.max(1) as u32) } }
fn rotation_of(m: &qcv_project::MediaItem) -> Option<u16> { if m.rotation_override >= 0 { Some(m.rotation_override as u16) } else { None } }
/// A Safety Guides change into the store (the Changed command repaints / reparses).
fn guide_setting(st: &mut min_settings::Settings, c: guides_panel::Cmd, cmds: &mut Vec<settings_panel::Cmd>) {
    use guides_panel::Cmd as G;
    let (key, ch) = match c { G::Guide(n) => (settings::k::SAFETY_GUIDE, st.set_str(settings::k::SAFETY_GUIDE, &n)), G::Opacity(v) => (settings::k::SAFETY_OPACITY, st.set_float(settings::k::SAFETY_OPACITY, v as f64)), G::Width(v) => (settings::k::SAFETY_WIDTH, st.set_float(settings::k::SAFETY_WIDTH, v as f64)), G::Color(h) => (settings::k::SAFETY_COLOR, st.set_str(settings::k::SAFETY_COLOR, &h)) };
    if ch { cmds.push(settings_panel::Cmd::Changed(key)); }
}
/// QCView's `TransferOverride` → the engine's choice.
fn transfer_choice(t: qcv_project::TransferOverride) -> min_present::engine::TransferChoice {
    use min_present::engine::TransferChoice as C; use qcv_project::TransferOverride as T;
    match t { T::Auto => C::Auto, T::SdrRec709 => C::Sdr709, T::Pq2020 => C::Pq2020, T::PqP3D65 => C::PqP3, T::Hlg => C::Hlg, T::Linear => C::Linear }
}
fn srgb_oetf(x: f32) -> f32 { let x = x.clamp(0.0, 1.0); if x <= 0.003_130_8 { 12.92 * x } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 } }
/// The display snapshot as the panel would show it: a linear-light surface is encoded with the sRGB curve (what the OS does), an
/// encoded surface is written as is.
/// "#rrggbb" → an opaque sRGB colour.
fn qcv_notes_color(h: &str) -> Option<[f32; 4]> { min_ui::color_picker::parse_hex(h).map(|c| [c[0], c[1], c[2], 1.0]) }
/// Where the single view's picture is, for the annotation tools: its displayed rect and window px per source px.
fn annot_geo(e: &min_present::engine::Engine) -> Option<annotate::Geo> {
    let (rect, _, _) = e.picture_placement()?; let (dw, _) = e.display_dims();
    Some(annotate::Geo { rect, px_per_src: rect[2] / dw.max(1) as f64 })
}
/// A PNG as RGBA8 (the clean thumbnail, read back for the annotated one).
pub(crate) fn image_rgba8(path: &std::path::Path) -> Result<(Vec<u8>, (u32, u32)), String> {
    let d = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).map_err(|e| e.to_string())?));
    let mut r = d.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; r.output_buffer_size()]; let info = r.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let px: Vec<u8> = match info.color_type { png::ColorType::Rgb => buf[..info.buffer_size()].chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(), png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(), _ => return Err("png colour type".into()) };
    Ok((px, (info.width, info.height)))
}
/// RGBA8 → an RGB PNG (QCView's thumbnails are opaque).
fn write_png_rgba8(path: &str, px: &[u8], w: u32, h: u32) {
    let rgb: Vec<u8> = px.chunks(4).flat_map(|c| [c[0], c[1], c[2]]).collect();
    let Ok(file) = std::fs::File::create(path) else { log::warn!("cannot write {path}"); return };
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h); enc.set_color(png::ColorType::Rgb); enc.set_depth(png::BitDepth::Eight);
    if let Ok(mut wr) = enc.write_header() { let _ = wr.write_image_data(&rgb); }
}
fn write_png_display(path: &str, px: &[f32], w: u32, h: u32, linear_surface: bool) {
    if !linear_surface { return write_png_rgb8(path, px, w, h) }
    let enc: Vec<f32> = px.iter().enumerate().map(|(i, v)| if i % 4 == 3 { *v } else { srgb_oetf(*v) }).collect();
    write_png_rgb8(path, &enc, w, h)
}
/// Where an OS drop lands (QCView's DropAreas): the bin ADDS only (`LeftRail.qml` dropZone → `addMediaPaths`, "drop into the
/// viewport is the explicit load-now gesture"); the viewport ADDS and LOADS the first (`dropMediaAt` → `openMediaPaths`). The
/// bands and the Inspector have no drop target — the OS shows a refused drop there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DropTarget { Bin, Viewport, /// dual view: a chip, or the picture by mode (QCView `dropMediaAt` / the chips' DropAreas)
    Side(qcv_player::Side), /// a playlist is loaded and the drop is over the scrubber or the clip bar: insert at x (QCView's playlistDropArea)
    Playlist(i32) }
/// QCView's `clipToMap` (a saved Dual View's `clipsA` / `clipsB`): times in SECONDS at the master rate / the source fps.
fn clip_to_qcview(c: &qcv_timeline::Clip, master: qcv_timeline::Rate) -> serde_json::Value {
    let sf = c.source_rate.as_f64().max(1e-9); let mf = master.as_f64().max(1e-9);
    serde_json::json!({ "id": c.id, "name": c.name, "startTime": c.start as f64 / mf, "duration": c.duration as f64 / mf, "sourceIn": c.source_in as f64 / sf, "sourceOut": c.source_out as f64 / sf, "isGap": c.is_gap,
        "mediaKind": match c.media_kind { qcv_timeline::ClipMediaKind::Video => 0, qcv_timeline::ClipMediaKind::ImageSequence => 1, qcv_timeline::ClipMediaKind::Audio => 2 },
        "mediaPath": c.media_path, "mediaItemId": c.media_item_id, "sourceFps": c.source_rate.as_f64(), "sourceDuration": c.source_duration as f64 / sf, "hasAudio": c.has_audio })
}
/// QCView's `clipFromMap`: back to frames (the master rate for positions, the clip's fps for its source window).
fn clip_from_qcview(v: &serde_json::Value, master: qcv_timeline::Rate) -> Option<qcv_timeline::Clip> {
    let o = v.as_object()?; let f = |k: &str| o.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0); let st = |k: &str| o.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string();
    let sr = qcv_timeline::Rate::from_f64(if f("sourceFps") > 0.0 { f("sourceFps") } else { master.as_f64() }); let (sf, mf) = (sr.as_f64(), master.as_f64());
    let kind = match o.get("mediaKind").and_then(|x| x.as_i64()).unwrap_or(0) { 1 => qcv_timeline::ClipMediaKind::ImageSequence, 2 => qcv_timeline::ClipMediaKind::Audio, _ => qcv_timeline::ClipMediaKind::Video };
    Some(qcv_timeline::Clip { id: if st("id").is_empty() { qcv_timeline::fresh_id() } else { st("id") }, name: st("name"), start: (f("startTime") * mf).round() as i64, duration: (f("duration") * mf).round() as i64, source_in: (f("sourceIn") * sf).round() as i64, source_out: (f("sourceOut") * sf).round() as i64, is_gap: o.get("isGap").and_then(|x| x.as_bool()).unwrap_or(false), media_kind: kind, media_path: st("mediaPath"), media_item_id: st("mediaItemId"), source_rate: sr, source_duration: (f("sourceDuration") * sf).round() as i64, has_audio: o.get("hasAudio").and_then(|x| x.as_bool()).unwrap_or(false), ..qcv_timeline::Clip::empty() })
}
fn write_png_rgb8(path: &str, px: &[f32], w: u32, h: u32) {
    let bytes: Vec<u8> = px.chunks(4).flat_map(|c| [(c[0].clamp(0.0, 1.0) * 255.0 + 0.5) as u8, (c[1].clamp(0.0, 1.0) * 255.0 + 0.5) as u8, (c[2].clamp(0.0, 1.0) * 255.0 + 0.5) as u8]).collect();
    let Ok(file) = std::fs::File::create(path) else { println!("cannot write {path}"); return };
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h); enc.set_color(png::ColorType::Rgb); enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&bytes).unwrap();
}

impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, el: &dyn ActiveEventLoop) {
        if self.gpu.is_some() { return }
        for (i, m) in el.available_monitors().enumerate() { println!("monitor {i}: {:?} at {:?} scale {}", m.name(), m.position(), m.scale_factor()); }
        let mut attrs = WindowAttributes::default().with_title("qcv").with_surface_size(LogicalSize::new(1280.0, 720.0));
        // QCView's `window/*` restore: the saved physical size and position when there is one (first run: 1280 × 720, the OS places it)
        { let st = &self.settings; use settings::k;
          if st.is_set(k::WINDOW_WIDTH) && st.int(k::WINDOW_WIDTH) >= 320 && st.int(k::WINDOW_HEIGHT) >= 200 { attrs = attrs.with_surface_size(winit::dpi::PhysicalSize::new(st.int(k::WINDOW_WIDTH) as u32, st.int(k::WINDOW_HEIGHT) as u32)); }
          if st.is_set(k::WINDOW_X) && st.is_set(k::WINDOW_Y) && self.o.screen.is_none() { attrs = attrs.with_position(PhysicalPosition::new(st.int(k::WINDOW_X) as i32, st.int(k::WINDOW_Y) as i32)); }
          if st.bool(k::WINDOW_MAXIMIZED) { attrs = attrs.with_maximized(true); } }
        if let Some(i) = self.o.screen { if let Some(m) = el.available_monitors().nth(i) { if let Some(p) = m.position() { attrs = attrs.with_position(PhysicalPosition::new(p.x + 100, p.y + 100)); } } }
        let window: Arc<Box<dyn Window>> = Arc::new(el.create_window(attrs).expect("window"));
        let instance = Presenter::instance();
        let surface = instance.create_surface(window.clone()).expect("surface");
        let size = window.surface_size();
        #[cfg(target_os = "macos")]
        let ns_view = { use raw_window_handle::{HasWindowHandle, RawWindowHandle}; match window.window_handle().unwrap().as_raw() { RawWindowHandle::AppKit(h) => Some(h.ns_view.as_ptr()), _ => None } };
        #[cfg(not(target_os = "macos"))]
        let ns_view = None;
        let mut presenter = Presenter::new(&instance, surface, (size.width, size.height), self.o.out, ns_view).expect("presenter");
        let mut d = presenter.engine.display(); d.white = self.o.white; presenter.engine.set_display(d);
        presenter.engine.set_content_rect(Some(content_rect(size.width, size.height, window.scale_factor())));
        presenter.engine.set_background(self.settings.int(settings::k::BACKGROUND_MODE) as u8); presenter.engine.set_ui_scale(window.scale_factor() as f32);
        if self.o.identity_chain { presenter.engine.set_color_pass(Some(Box::new(min_present::IdentityColorPass))); }
        let mincolor_rx = color::wanted_ahead(&presenter.engine.device, &presenter.engine.queue);
        if !color::harness_active() { self.mincolor_ready[0] = Some(color::mincolor_ahead(&presenter.engine.device, &presenter.engine.queue)); }
        let i = presenter.info();
        println!("presenter: {} ({}) → {:?} {:?} colour space {:?}{}; features {:?}; scale {}", i.adapter, i.backend, i.output, i.format, i.color_space, if i.layer_workaround { " (PQ set on the layer by name)" } else { "" }, i.features, window.scale_factor());
        if !i.present_modes.is_empty() { println!("present modes {:?}", i.present_modes); }
        let r = presenter.layer_report(); if !r.is_empty() { println!("{r}"); }
        #[cfg(windows)] let dev = presenter.engine.dx12_device_raw();
        #[cfg(not(windows))] let dev = None;
        self.hw = if self.o.sw { None } else { HwDevice::platform(dev) };
        self.hw_intra = if self.o.sw { None } else { HwDevice::vulkan(dev) };   // Windows: ProRes / FFV1 / APV on FFmpeg's Vulkan device (S6); None elsewhere
        if let Some(h) = &self.hw { println!("hardware decode: {}", h.name()); } else if !self.o.sw { println!("hardware decode unavailable — software"); }
        #[cfg(target_os = "macos")] { mac::menu_bar(); mac::activate(); }
        window.focus_window();
        println!("window: surface {:?} scale {} visible {:?} monitor {:?}", window.surface_size(), window.scale_factor(), window.is_visible(), window.current_monitor().and_then(|m| m.name().map(|n| n.to_string())));
        window.request_redraw();
        // the rails from the store (QCView `ui.layout/*`; "Always open in minimal mode" collapses both at launch)
        let (left, right, waveform_enabled, thumbs_enabled) = { let st = &self.settings; use settings::k; let minimal = st.bool(k::ALWAYS_OPEN_MINIMAL);
            let mut l = min_ui::Rail::new(min_ui::Side::Left, st.int(k::LEFT_RAIL_WIDTH) as f64, minimal || st.bool(k::LEFT_RAIL_COLLAPSED)); l.settle();
            let mut r = min_ui::Rail::new(min_ui::Side::Right, st.int(k::RIGHT_RAIL_WIDTH) as f64, minimal || st.bool(k::RIGHT_RAIL_COLLAPSED)); r.settle();
            l.shortcut = if cfg!(target_os = "macos") { "⌘1" } else { "Ctrl+1" }.into(); r.shortcut = if cfg!(target_os = "macos") { "⌘2" } else { "Ctrl+2" }.into();
            (l, r, st.bool(k::TIMELINE_WAVEFORMS), st.bool(k::TIMELINE_HOVER_THUMBS)) };
        // the UI layer (stage 4): at the surface's physical size, on the presenter's device; `QCV_NO_UI=1` leaves it out for A/B
        let mut ui = if std::env::var_os("QCV_NO_UI").is_some() { None } else { min_ui::UiLayer::new(&presenter.engine.device, size.width, size.height).map_err(|e| println!("ui layer: {e}")).ok() };
        if let Some(u) = ui.as_mut() { presenter.engine.set_overlay(Some(u.view())); }
        let wf = (match self.settings.int(settings::k::WAVEFORM_SCALE) { 0 => Some(false), 1 => Some(true), _ => None }, self.settings.int(settings::k::WAVEFORM_PEAK) as f32, self.settings.bool(settings::k::WAVEFORM_OPEN));
        let drawer = color_drawer::ColorDrawer::new(self.settings.bool(settings::k::COLOR_DRAWER_OPEN), self.settings.int(settings::k::COLOR_DRAWER_HEIGHT) as f64, wf);
        self.gpu = Some(Gpu { window, presenter, mincolor_rx, drawer, dualc: dual_controls::DualControls::new(), dual_bars: dual_bars::DualBars::new(), popover_side: None, notes: { let mut n = notes_drawer::NotesDrawer::new(); n.height = self.settings.float(settings::k::NOTES_DRAWER_HEIGHT).clamp(notes_drawer::HEIGHT_MIN, notes_drawer::HEIGHT_MAX); n }, notes_current: None, insp_seen: (0, String::new()), dual_dialog: dual_dialog(), seam_hot: false, seam_drag: false, annot_down: false, last_move: Instant::now(), sdr_checked: None, clip_peak: [None, None], ui, clip_bar: clip_bar::ClipBar::new(), clip_popover: clip_popover::ClipPopover::new(), toast: min_ui::Toast::new(), fonts: min_ui::Fonts::new(), ui_ms: vec![], ui_shown_frame: None, bar: transport_bar::TransportBar::new(), strip: status_strip::StatusStrip::new(), scrubber: min_ui::Scrubber::new(), thumbs_b: hover_thumbs::HoverThumbs::new(), thumbs: hover_thumbs::HoverThumbs::new(), pointer: (-1.0, -1.0), ui_dirty: true, seq_status: None, fullscreen: false, bands: min_ui::anim::Ramp::new(1.0, 0) /* the bands POP in fullscreen, as the rails do (the user 2026-11-07: "it's simpler"); the 160-ms slide is retired */, bands_leave: None, waveform: qcv_player::WaveformProbeEngine::new(), wave_seen: 0, wave_req: (String::new(), 0.0, 0.0, 0), wave_peaks: vec![],
            compact: false, compact_strip: compact_strip::CompactStrip::new(), pinned: false, strip_hidden: false, left, right, settings_panel: settings_panel::SettingsPanel::new(), background_panel: background_panel::BackgroundPanel::new(), guides_panel: guides_panel::GuidesPanel::new(), guide: None, thumb_layer: None, project_panel: project_panel::ProjectPanel::new(), title_shown: String::new(), inspector: inspector_panel::InspectorPanel::new(), shortcuts: shortcuts::ShortcutsPanel::new(), shortcuts_reveal: false, lang_gen: min_i18n::generation(), content_set: None, waveform_enabled, cursor: winit::cursor::CursorIcon::Default, menu_bar: None,
            about: about_dialog() });
        if let Some(g) = self.gpu.as_mut() { let l = |c: min_theme::Color| { let v = c.linear(); [v[0], v[1], v[2]] }; g.presenter.engine.set_side_tints(l(t::SIDE_A_COLOR), l(t::SIDE_B_COLOR)); }
        // the scrubber's camera insets 16 px at both ends (the walk, 2026-11-08): past macOS's rounded bottom corners, so the
        // timeline's ends keep their trim zones reachable, and room for the dual lanes' A / B row labels on every OS
        if let Some(g) = self.gpu.as_mut() { g.scrubber.end_inset = dual_bars::LABEL_W; }   // the waveform's A / B (row 173)
        if let Some(g) = self.gpu.as_mut() { g.thumbs.enabled = thumbs_enabled; }
        self.apply_pin();
        // the menu bar (QCView's MenuBar, row 143): the app's native menu on macOS; QCView's in-window bar on Windows (row 194)
        #[cfg(target_os = "macos")] { let m = menu::AppMenu::new(); m.menu.init_for_nsapp(); self.menu = Some(m); }
        if menu::InWindowMenu::wanted() { if let Some(g) = self.gpu.as_mut() { g.menu_bar = Some(menu::InWindowMenu::new(&mut g.fonts)); } }
        self.t0 = Some(Instant::now()); self.last_report = Instant::now();
        if let Some(f) = self.o.file.clone() { if f.to_lowercase().ends_with(".qcvproj") { self.open_project(&f); } else { let paths = if self.o.files.is_empty() { vec![f.clone()] } else { self.o.files.clone() }; let mut first = true; for p in self.admit_media_paths(&paths) { if self.add_and_load(&p, first).is_some() { first = false; } } } }
        if let Some(b) = self.o.dual.clone() { self.enter_dual(&b); }
        if std::env::var_os("QCV_OPEN_NOTES").is_some() { self.toggle_notes(); }   // harness: the Notes drawer at launch
    }
    fn about_to_wait(&mut self, el: &dyn ActiveEventLoop) {
        if self.o.seconds > 0.0 { if let Some(t0) = self.t0 { let dl = t0 + std::time::Duration::from_secs_f32(self.o.seconds); if Instant::now() >= dl { self.finish(); el.exit(); return } el.set_control_flow(ControlFlow::WaitUntil(dl)); return } }
        // THE HEARTBEAT: the app's tick runs from RedrawRequested, re-requested after each tick. On Windows that chain died at the
        // first 6.7K open (2026-11-07: the stats stopped, no tick ran while input kept arriving — the whole UI froze until a drag
        // or resize); winit's own idiom is to request the redraw from here. Wake at least every refresh and ask again if the last
        // tick is older than one — a duplicate request coalesces, a dropped one no longer freezes the app.
        // single instance: a later launch's files / links, the window raised (QCView `dispatchOpenArgsOnPrimary`)
        let forwarded: Vec<Vec<String>> = self.single_rx.as_ref().map(|rx| rx.try_iter().collect()).unwrap_or_default();
        for args in forwarded { self.open_forwarded(args); }
        // the menu bar: its commands, then its live parts (the Recent submenus, the two checkmarks)
        for c in std::mem::take(&mut self.menu_queue) { self.menu_cmd(c, el); }
        let list = |k| self.settings.string(k).lines().filter(|l| !l.is_empty()).map(str::to_string).collect::<Vec<_>>();
        let (rm, rp) = (list(settings::k::RECENT_MEDIA), list(settings::k::RECENT_PROJECTS));
        #[cfg(target_os = "macos")] {
            while let Ok(ev) = muda::MenuEvent::receiver().try_recv() { if let Some(c) = self.menu.as_ref().and_then(|m| m.cmd(&ev.id)) { self.menu_cmd(c, el); } }
            if let Some(m) = self.menu.as_mut() { m.set_recents(&rm, &rp); if let Some(g) = self.gpu.as_ref() { m.set_checks(g.compact, g.pinned); } }
        }
        if let Some(g) = self.gpu.as_mut() { let (c, p) = (g.compact, g.pinned); if let Some(mb) = g.menu_bar.as_mut() { mb.update(&rm, &rp, c, p, &mut g.fonts); g.ui_dirty = true; } }
        let now = Instant::now();
        if let Some(g) = self.gpu.as_ref() { if now.duration_since(self.last_tick) >= std::time::Duration::from_millis(16) { g.window.request_redraw(); } }
        el.set_control_flow(ControlFlow::WaitUntil(now + std::time::Duration::from_millis(16)));
    }
    fn window_event(&mut self, el: &dyn ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        // `QCV_TRACE_INPUT=1`: every pointer / focus event as the window layer delivers it (the Windows leave-and-return click report, 2026-11-07)
        if *TRACE_INPUT { match &event { WindowEvent::PointerMoved { .. } | WindowEvent::PointerButton { .. } | WindowEvent::PointerLeft { .. } | WindowEvent::PointerEntered { .. } | WindowEvent::Focused(_) | WindowEvent::MouseWheel { .. } => println!("input: {event:?}"), _ => {} } }
        match event {
            WindowEvent::CloseRequested => { self.finish(); el.exit(); }
            WindowEvent::Focused(f) => {
                log::info!("focused {f}");
                let mut pending: Vec<settings_panel::Cmd> = vec![]; let mut pending_pp: Vec<project_panel::Cmd> = vec![];
                if let Some(g) = self.gpu.as_mut() {
                    let now = Instant::now();
                    if !f {
                        self.alt_clean = false;
                        if g.strip.is_open() { g.strip.close(); }
                        if let Some(mb) = g.menu_bar.as_mut() { mb.bar.cancel(); }   // a menu never outlives the window's focus (Qt closes its popups)
                        // the window lost focus: every typed field commits (QCView's editingFinished)
                        pending = g.settings_panel.blur(&mut self.settings);
                        if let Some(c) = g.project_panel.commit_rename() { pending_pp.push(c); } set_ime(&**g.window, false);
                        for c in g.inspector.blur() { if let inspector_panel::Cmd::Par(par) = c { { let sl = insp_slot(&self.dual, g); let dual_on = self.dual.is_some(); if let Some(en) = color::slot_engine(g, sl, dual_on) { let mut geo = en.geometry(); geo.par = par; en.set_geometry(geo); } } g.content_set = None; } }
                        set_ime(&**g.window, false);
                    }
                    tab_presence(g, now);
                    g.ui_dirty = true;
                }
                for c in pending { self.apply_setting(c); } for c in pending_pp { self.project_cmd(c); }
            }
            WindowEvent::Ime(ime) => { if let Some(g) = self.gpu.as_mut() { let e = match ime { winit::event::Ime::Preedit(t, c) => Some(min_ui::ImeEvent::Preedit(t, c)), winit::event::Ime::Commit(t) => Some(min_ui::ImeEvent::Commit(t)), _ => None }; if let Some(e) = e { if g.dual_dialog.open { g.dual_dialog.ime(&e, &mut g.fonts); g.ui_dirty = true; } else if g.notes.has_focus() { g.notes.ime(&e, &mut g.fonts); g.ui_dirty = true; } else if g.guides_panel.has_focus() { g.guides_panel.ime(&e, &mut g.fonts); g.ui_dirty = true; } else if g.clip_popover.is_open() { g.clip_popover.ime(&e, &mut g.fonts); } else if g.drawer.is_typing() { g.drawer.ime(&e, &mut g.fonts); } else if g.project_panel.is_editing() { g.project_panel.ime(&e, &mut g.fonts); } else if g.settings_panel.has_focus() { g.settings_panel.ime(&e, &mut g.fonts); } else if g.inspector.has_focus() { g.inspector.ime(&e, &mut g.fonts); } else if g.shortcuts.has_focus() { g.shortcuts.ime(&e, &mut g.fonts); } else { g.strip.ime(&e, &mut g.fonts); } g.ui_dirty = true; } } }
            WindowEvent::SurfaceResized(s) => if let Some(g) = self.gpu.as_mut() { g.presenter.resize(s.width, s.height); let sc = g.window.scale_factor(); g.presenter.engine.set_ui_scale(sc as f32); g.content_set = None; g.presenter.engine.set_content_rect(Some(if g.fullscreen { (0, 0, s.width, s.height) } else { content_rect(s.width, s.height, sc) })); if let Some(u) = g.ui.as_mut() { u.resize(&g.presenter.engine.device, s.width, s.height); g.presenter.engine.set_overlay(Some(u.view())); } g.window.request_redraw(); },
            WindowEvent::RedrawRequested => { self.tick(); if let Some(g) = &self.gpu { g.window.request_redraw(); } }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                // the Save dual view dialog takes every key while open (Enter = Save, Escape = Cancel)
                if let Some(g) = self.gpu.as_mut() { if g.dual_dialog.open {
                    if pressed { let k = to_key_event(&event, self.mods); let r = g.dual_dialog.key(&k, &mut g.fonts); g.ui_dirty = true; if let Some(i) = r { self.dual_dialog_done(i); } }
                    return
                } }
                // QCView's About modal: Escape / Enter close it; it takes every key while open
                if let Some(g) = self.gpu.as_mut() { if g.about.open { if pressed && matches!(event.logical_key, Key::Named(NamedKey::Escape | NamedKey::Enter)) { g.about.cancel(); g.ui_dirty = true; } return } }
                // the in-window menu bar's keys (Windows' rules, row 194): Alt tapped alone / F10 focus it, Alt+letter opens a menu,
                // and while it is active the arrows / Enter / Space / Escape / letters are its
                if let Some(k) = self.bar_key(&event, pressed) {
                    if let Some(g) = self.gpu.as_mut() { if !g.fullscreen && !g.compact { if let Some(mb) = g.menu_bar.as_mut() {
                        let (id, d, consumed) = mb.bar.key(&k, &mut g.fonts); if d { g.ui_dirty = true; }
                        if let Some(c) = id.and_then(|i| mb.cmd(i)) { self.menu_queue.push(c); }
                        if consumed { return }
                    } } }
                }
                if pressed { if let Some(g) = self.gpu.as_mut() { if g.clip_popover.is_open() {
                    let k = to_key_event(&event, self.mods);
                    let c = g.clip_popover.key(&k, &mut g.fonts); g.ui_dirty = true;
                    if !g.clip_popover.is_open() { set_ime(&**g.window, false); }
                    for c in c { self.bar_cmd(c); }
                    return
                } } }
                if pressed { if let Some(g) = self.gpu.as_mut() { if !g.fullscreen && !g.compact && g.project_panel.is_editing() {
                    let k = to_key_event(&event, self.mods);
                    let c = g.project_panel.key(&k, &mut g.fonts); g.ui_dirty = true;
                    if !g.project_panel.is_editing() { set_ime(&**g.window, false); }
                    if let Some(c) = c { self.project_cmd(c); }
                    return
                } } }
                // the Keyboard Shortcuts filter takes the keys while focused (row 218)
                if pressed { if let Some(g) = self.gpu.as_mut() { if !g.fullscreen && !g.compact && g.shortcuts.has_focus() {
                    let k = to_key_event(&event, self.mods); g.shortcuts.key(&k, &mut g.fonts); g.ui_dirty = true;
                    if !g.shortcuts.has_focus() { set_ime(&**g.window, false); }
                    return
                } } }
                // QCView's rail shortcuts: F2 renames the single selected row; Ctrl/Cmd+A selects every source row (no text field focused)
                if pressed && !event.repeat { if let Some(g) = self.gpu.as_mut() { if !g.fullscreen && !g.compact && !g.settings_panel.has_focus() && !g.inspector.has_focus() && !g.strip.is_open() {
                    if matches!(event.logical_key, Key::Named(NamedKey::F2)) { if g.project_panel.rename_selection(&mut g.fonts) { set_ime(&**g.window, true); g.ui_dirty = true; } return }
                    if self.mods.ctrl && !self.mods.shift && !self.mods.alt && matches!(&event.logical_key, Key::Character(c) if c == "a" || c == "A") { if g.project_panel.select_all_media() { g.ui_dirty = true; } return }
                } } }
                if pressed { if let Some(g) = self.gpu.as_mut() { if !g.fullscreen && !g.compact && (g.drawer.has_focus() || g.drawer.is_modal()) {
                    let k = to_key_event(&event, self.mods);
                    let (cmds, used) = g.drawer.key(&k, &mut g.fonts); g.ui_dirty = true;
                    if !g.drawer.is_modal() { set_ime(&**g.window, false); }
                    for c in cmds { self.color_cmd(c); }
                    if used { return }
                } } }
                if pressed { if let Some(g) = self.gpu.as_mut() { if !g.fullscreen && !g.compact && g.inspector.has_focus() {
                    let k = to_key_event(&event, self.mods);
                    let cmds = g.inspector.key(&k, &mut g.fonts); g.ui_dirty = true;
                    if !g.inspector.has_focus() { set_ime(&**g.window, false); }
                    for c in cmds { if let inspector_panel::Cmd::Par(par) = c { { let sl = insp_slot(&self.dual, g); let dual_on = self.dual.is_some(); if let Some(en) = color::slot_engine(g, sl, dual_on) { let mut geo = en.geometry(); geo.par = par; en.set_geometry(geo); } } g.content_set = None; } }
                    return
                } } }
                // the Notes drawer's text / hex fields
                if pressed { if let Some(g) = self.gpu.as_mut() { if !g.fullscreen && !g.compact && g.notes.has_focus() {
                    let k = to_key_event(&event, self.mods);
                    let cmds = g.notes.key(&k, &mut g.fonts); g.ui_dirty = true;
                    if !g.notes.has_focus() { set_ime(&**g.window, false); }
                    for c in cmds { self.notes_cmd(c); }
                    return
                } } }
                // the Safety Guides' Hex field
                if pressed { if let Some(g) = self.gpu.as_mut() { if !g.fullscreen && !g.compact && g.guides_panel.has_focus() {
                    let k = to_key_event(&event, self.mods);
                    let gc = g.guides_panel.key(&k, &mut g.fonts); g.ui_dirty = true;
                    if !g.guides_panel.has_focus() { set_ime(&**g.window, false); }
                    let mut cmds = vec![]; for c in gc { guide_setting(&mut self.settings, c, &mut cmds); } for c in cmds { self.apply_setting(c); }
                    return
                } } }
                if pressed { if let Some(g) = self.gpu.as_mut() { if !g.fullscreen && !g.compact && (g.settings_panel.has_focus() || g.settings_panel.is_modal() || g.settings_panel.menu_open()) {
                    let k = to_key_event(&event, self.mods);
                    let had_focus = g.settings_panel.has_focus();
                    let (handled, cmds) = g.settings_panel.key(&k, &mut g.fonts, &mut self.settings);
                    g.ui_dirty = true;
                    if g.settings_panel.has_focus() != had_focus { set_ime(&**g.window, g.settings_panel.has_focus()); }
                    for c in cmds { self.apply_setting(c); }
                    if handled { return }
                } } }
                // QCView: Ctrl+Shift+C toggles Compact Mode; Ctrl+Alt+T the pin
                if pressed && self.mods.ctrl && !event.repeat { if let Key::Character(c) = &event.logical_key { if self.mods.shift && (c == "c" || c == "C") { self.toggle_compact(); return } if self.mods.alt && (c == "t" || c == "T" || c == "†") { self.toggle_pin(); return } } }
                if pressed && self.mods.ctrl && !self.mods.shift && !self.mods.alt && !event.repeat { if let Key::Character(c) = &event.logical_key {
                    let now = Instant::now();
                    match c.as_str() {
                        "0" => { self.view_minimal(); return }   // Minimal Mode
                        "9" => { self.view_all_panels(); return }  // Show All Panels
                        "5" => { self.toggle_status_bar(); return }   // View ▸ Status Bar
                        "3" => { if self.gpu.as_ref().is_some_and(|g| !g.fullscreen && !g.compact) { self.color_cmd(color_drawer::Cmd::Toggle); } return }   // View ▸ Color Panel (row 199)
                        "l" | "L" => { if let Some(p) = pb_mut(&mut self.dual, &mut self.player) { p.apply(Action::ToggleLoop, now); } return }
                        "q" | "Q" => { self.finish(); el.exit(); return }
                        "=" | "+" | "-" | "_" => { if let Some(g) = self.gpu.as_mut() { let st = scrub_state(pb(&self.dual, &self.player), self.player.as_ref()); let scale = g.window.scale_factor(); let x = g.scrubber.playhead_x(&st, scale); if g.scrubber.wheel(x, if c == "-" || c == "_" { -600.0 } else { 600.0 }, false, &st, scale) { g.ui_dirty = true; } } return }
                        _ => {}
                    }
                } }
                // QCView's File menu: Ctrl+O Open Media…, Ctrl+Shift+O Open Project…, Ctrl+S Save, Ctrl+Shift+S Save As…
                if pressed && self.mods.ctrl && !self.mods.alt && !event.repeat { if let Key::Character(c) = &event.logical_key { match (c.to_lowercase().as_str(), self.mods.shift) {
                    ("o", false) => { self.open_media_dialog(); return } ("o", true) => { self.open_project_dialog(); return }
                    ("s", false) => { self.save_project(); return } ("s", true) => { self.save_project_as(); return }
                    ("n", false) => { self.new_project(); return }   // QCView's New Project has no shortcut (a menu item); Ctrl+N stands in until the menu bar exists
                    _ => {}
                } } }
                // dual view's edit keys (row 212) — no text field has the keys by here
                if pressed && self.dual.is_some() { let typing = self.gpu.as_ref().is_some_and(|g| g.settings_panel.has_focus() || g.inspector.has_focus() || g.project_panel.is_editing() || g.strip.is_open() || g.drawer.has_focus()); if !typing && self.dual_key(&event.logical_key) { return } }
                // a loaded playlist: Delete / Backspace removes the selected clip (no ripple close, QCView), Ctrl/Cmd+K splits at the playhead
                if pressed && !event.repeat && self.pl_editor.is_some() { if let Some(g) = self.gpu.as_mut() { if !g.settings_panel.has_focus() && !g.inspector.has_focus() && !g.project_panel.is_editing() && !g.strip.is_open() {
                    let del = matches!(event.logical_key, Key::Named(NamedKey::Delete) | Key::Named(NamedKey::Backspace));
                    let split = self.mods.ctrl && !self.mods.shift && matches!(&event.logical_key, Key::Character(c) if c == "k" || c == "K");
                    if del { if let Some(id) = g.clip_bar.selected.take() { if self.pl_editor.as_mut().unwrap().delete_clip(clip_bar::TRACK, &id) { self.after_playlist_edit(true); } if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; } return } }
                    if split { let at = self.player.as_ref().map(|p| p.current_frame() as i64).unwrap_or(0); if self.pl_editor.as_mut().unwrap().split_clip_at(clip_bar::TRACK, at) { self.after_playlist_edit(true); } if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; } return }
                } } }
                // Delete / Backspace: remove the bin selection (QCView's Delete menu item); Ctrl+Z restores the last batch while the annotation undo has no owner
                if pressed && !event.repeat && matches!(event.logical_key, Key::Named(NamedKey::Delete) | Key::Named(NamedKey::Backspace)) { if let Some(g) = self.gpu.as_mut() { if !g.fullscreen && !g.compact && !g.settings_panel.has_focus() && !g.inspector.has_focus() { if let Some(c) = g.project_panel.remove_selection() { self.project_cmd(c); return } } } }
                // QCView's Tools ▸ Settings… (Ctrl+,)
                if pressed && self.mods.ctrl && !event.repeat && matches!(&event.logical_key, Key::Character(c) if c == ",") { self.reveal(Reveal::Settings); return }
                // QCView's Tools ▸ Keyboard Shortcuts (Ctrl+/): the right rail, the card expanded, scrolled into view
                if pressed && self.mods.ctrl && !event.repeat && matches!(&event.logical_key, Key::Character(c) if c == "/") { self.reveal_shortcuts(); return }
                // QCView's View menu: Ctrl+1 / Ctrl+2 toggle the rails
                if pressed && self.mods.ctrl && !event.repeat { if let Key::Character(c) = &event.logical_key { if c == "1" || c == "2" { if let Some(g) = self.gpu.as_mut() { if !g.fullscreen { let now = Instant::now(); if c == "1" { g.left.toggle(now); } else { g.right.toggle(now); } g.ui_dirty = true; } } return } } }
                if let Some(g) = self.gpu.as_mut() { if g.strip.is_open() {
                    if pressed {
                        let k = to_key_event(&event, self.mods);
                        let commit = g.strip.key(&k, &mut g.fonts);
                        g.ui_dirty = true;
                        if !g.strip.is_open() { set_ime(&**g.window, false); }
                        match commit { Some(status_strip::Commit::Frame(f)) => { if let Some(p) = pb_mut(&mut self.dual, &mut self.player) { p.seek_to_frame(f); } } Some(status_strip::Commit::Time(sec)) => { if let Some(p) = pb_mut(&mut self.dual, &mut self.player) { let f = (sec * p.rate().as_f64()).round() as i32; p.seek_to_frame(f); } } None => {} }
                    }
                    return
                } }
                // annotating (row 215): Esc cancels the stroke (else clears the selection), Delete removes the selected stroke, Ctrl+4 the mode
                if pressed && self.annot.active {
                    let frame = self.player.as_ref().map(|p| p.current_frame()).unwrap_or(0);
                    if matches!(event.logical_key, Key::Named(NamedKey::Escape)) && self.annot.escape() { if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; } return }
                    if self.annot.tool == annotate::Tool::Select && matches!(event.logical_key, Key::Named(NamedKey::Delete | NamedKey::Backspace)) { if let Some(c) = self.annot.delete_selected(frame) { self.after_annotation(c); } return }
                }
                if pressed && !event.repeat && self.mods.ctrl && !self.mods.shift && matches!(&event.logical_key, Key::Character(c) if c == "4") { self.toggle_notes(); return }
                if pressed && matches!(event.logical_key, Key::Named(NamedKey::Escape)) { if self.gpu.as_ref().is_some_and(|g| g.compact) { self.toggle_compact(); return } if self.gpu.as_ref().is_some_and(|g| g.fullscreen) { self.toggle_fullscreen(); return } self.finish(); el.exit(); return }
                let key = match &event.logical_key {
                    Key::Character(c) if c == " " => Some(qcv_player::Key::Space),
                    Key::Character(c) => c.chars().next().map(qcv_player::Key::Char),
                    Key::Named(NamedKey::ArrowLeft) => Some(qcv_player::Key::Left), Key::Named(NamedKey::ArrowRight) => Some(qcv_player::Key::Right),
                    Key::Named(NamedKey::ArrowUp) => Some(qcv_player::Key::Up), Key::Named(NamedKey::ArrowDown) => Some(qcv_player::Key::Down), Key::Named(NamedKey::Home) => Some(qcv_player::Key::Home), Key::Named(NamedKey::End) => Some(qcv_player::Key::End),
                    _ => None,
                };
                let m = self.mods;
                if let Some(a) = key.and_then(|k| qcv_player::map_key(k, m, pressed, event.repeat)) {
                    let handled = pb_mut(&mut self.dual, &mut self.player).map(|p| p.apply(a, Instant::now())).unwrap_or(false);
                    if handled { if let Some(p) = &self.player { println!("{a:?} → {} at frame {}{}", if p.is_playing() { "playing" } else { "paused" }, p.current_frame(), if p.is_fast_seeking() { " (shuttle)" } else { "" }); } }
                    else if a == Action::ToggleFullscreen { self.toggle_fullscreen(); }
                    else if a == Action::CycleBackground { self.cycle_background(); }
                    else if a == Action::Screenshot || a == Action::ScreenshotToClipboard { if !event.repeat { self.screenshot(if a == Action::Screenshot { capture::Dest::File } else { capture::Dest::Clipboard }); } }
                    else if (a == Action::AnnotationUndo || a == Action::AnnotationRedo) && self.annot.active && self.dual.is_none() {
                        let c = if a == Action::AnnotationUndo { self.annot.undo() } else { self.annot.redo() };
                        if let Some(c) = c { self.after_annotation(c); }
                    }
                    else if (a == Action::AnnotationUndo || a == Action::AnnotationRedo) && self.dual_editor.is_some() && self.dual.is_some() {
                        let ed = self.dual_editor.as_mut().unwrap(); let ok = if a == Action::AnnotationUndo { ed.undo_edit() } else { ed.redo_edit() };
                        if ok { self.after_dual_edit(true); }
                    }
                    else if (a == Action::AnnotationUndo || a == Action::AnnotationRedo) && self.pl_editor.is_some() {
                        // a loaded playlist owns Ctrl/Cmd+Z (QCView: the timeline's undoEdit / redoEdit in an editable mode)
                        let ed = self.pl_editor.as_mut().unwrap(); let ok = if a == Action::AnnotationUndo { ed.undo_edit() } else { ed.redo_edit() };
                        if ok { self.after_playlist_edit(true); } if let Some(g) = self.gpu.as_mut() { g.ui_dirty = true; }
                    }
                    // (the bin-restore stand-in on Ctrl+Z retired 2026-11-08: the removal toast's Undo is QCView's way)
                }
            }
            WindowEvent::PointerMoved { position, .. } => self.pointer(min_ui::Event::Move { x: position.x, y: position.y }),
            WindowEvent::PointerLeft { .. } => self.pointer(min_ui::Event::Leave),
            WindowEvent::MouseWheel { delta, .. } => {
                let alt = self.mods.alt;
                // Qt angle units (120 per notch): lines → notches; pixel deltas (trackpads) at 2 units per px; Alt+wheel folds x into y (QCView's Windows rule)
                let dy = match delta { winit::event::MouseScrollDelta::LineDelta(x, y) => (if alt && y == 0.0 { x } else { y }) as f64 * 120.0, winit::event::MouseScrollDelta::PixelDelta(p) => (if alt && p.y == 0.0 { p.x } else { p.y }) * 2.0, _ => 0.0 };
                if let Some(g) = self.gpu.as_mut() { let (x, y) = g.pointer; let now = Instant::now(); let scale = g.window.scale_factor();
                    let chromeless = g.fullscreen || g.compact;
                    // the drawer: a sideways swipe or Shift + wheel scrolls its row of columns; else its lists / columns scroll
                    let dx = match delta { winit::event::MouseScrollDelta::LineDelta(x, _) => x as f64 * 120.0, winit::event::MouseScrollDelta::PixelDelta(p) => p.x * 2.0, _ => 0.0 };
                    let (hx, vy) = if self.mods.shift && dx == 0.0 { (dy, 0.0) } else { (dx, dy) };
                    if !chromeless && hx.abs() > vy.abs() && g.drawer.hwheel(x, y, -hx / 120.0 * 48.0 * scale) { g.ui_dirty = true; return }
                    if !chromeless && g.drawer.wheel(x, y, -vy / 120.0 * 48.0 * scale) { g.ui_dirty = true; return }
                    if !chromeless && g.notes.wheel(x, y, -(if hx.abs() > vy.abs() { hx } else { vy }) / 120.0 * 48.0 * scale, vy.abs() >= hx.abs(), &mut g.fonts) { g.ui_dirty = true; return }
                    if !chromeless && g.left.hit(x, y) {
                        if let Some(cmds) = g.settings_panel.wheel(x, y, dy, &mut self.settings) { g.ui_dirty = true; for c in cmds { self.apply_setting(c); } return }
                        if g.left.wheel(-dy / 120.0 * 48.0 * scale, now) { g.ui_dirty = true; } return
                    }
                    if !chromeless && g.right.hit(x, y) { if g.right.wheel(-dy / 120.0 * 48.0 * scale, now) { g.ui_dirty = true; } return }
                    if g.scrubber.hit(x, y) { let st = scrub_state(pb(&self.dual, &self.player), self.player.as_ref()); let scale = g.window.scale_factor(); if g.scrubber.wheel(x, dy, alt, &st, scale) { g.ui_dirty = true; } } }
            }
            WindowEvent::PointerButton { state, position, button, .. } => {
                let b = match button.mouse_button() { Some(winit::event::MouseButton::Left) => min_ui::Button::Primary, Some(winit::event::MouseButton::Right) => min_ui::Button::Secondary, Some(winit::event::MouseButton::Middle) => min_ui::Button::Middle, _ => return };
                let (x, y) = (position.x, position.y);
                self.pointer(if state == ElementState::Pressed { min_ui::Event::Down { x, y, button: b } } else { min_ui::Event::Up { x, y, button: b } });
            }
            WindowEvent::ModifiersChanged(m) => { let st = m.state(); self.mods = qcv_player::Mods { shift: st.shift_key(), ctrl: st.control_key() || st.meta_key(), alt: st.alt_key(), keypad: false }; if let Some(g) = self.gpu.as_mut() { g.bar.set_shift(self.mods.shift); } }
            WindowEvent::DragEntered { id: tid, position } => {
                self.drag_pos = position.map(|p| (p.x, p.y));
                let ok = el.data_transfer(tid).map(|dt| dt.has_type(&TypeHint::UriList)).unwrap_or(false) && self.drag_pos.is_none_or(|(x, y)| self.drop_target(x, y).is_some());
                el.set_valid_dnd_actions(tid, if ok { &[DndAction::Copy] } else { &[] }).ok();
            }
            // the cursor says refused over the bands / the Inspector (QCView: no DropArea there)
            WindowEvent::DragPosition { id: tid, position, .. } => {
                self.drag_pos = Some((position.x, position.y));
                let target = self.drop_target(position.x, position.y);
                let hint = match target { Some(DropTarget::Playlist(x)) => self.playlist_drop_index(x as f64), _ => None };
                let side = match target { Some(DropTarget::Side(sd)) => Some(sd), _ => None };
                if let Some(g) = self.gpu.as_mut() { if g.dualc.set_drop_side(side) { g.ui_dirty = true; } }
                if let Some(g) = self.gpu.as_mut() { if g.clip_bar.set_drop_hint(hint) { g.ui_dirty = true; } }
                let ok = el.data_transfer(tid).map(|dt| dt.has_type(&TypeHint::UriList)).unwrap_or(false) && self.drop_target(position.x, position.y).is_some();
                el.set_valid_dnd_actions(tid, if ok { &[DndAction::Copy] } else { &[] }).ok();
            }
            WindowEvent::DragLeft { .. } => { self.drag_pos = None; if let Some(g) = self.gpu.as_mut() { if g.clip_bar.set_drop_hint(None) { g.ui_dirty = true; } if g.dualc.set_drop_side(None) { g.ui_dirty = true; } } }
            WindowEvent::DragDropped { id: tid, .. } => {
                // no position from the OS at all: the viewport, as before
                let target = match self.drag_pos { Some((x, y)) => self.drop_target(x, y), None => Some(DropTarget::Viewport) };
                self.drag_pos = None;
                if let Some(g) = self.gpu.as_mut() { if g.dualc.set_drop_side(None) { g.ui_dirty = true; } }
                if let Some(t) = target { if let Ok(s) = el.fetch_data_transfer(tid, &TypeHint::UriList) { self.fetches.insert(s, t); } }
            }
            WindowEvent::DataTransferReceived { serial, value, .. } => {
                if let Some(target) = self.fetches.remove(&serial) { if let Ok(uris) = value.try_as_uris() { let raw: Vec<String> = uris.iter().filter_map(|u| min_paths::url::file_url_to_path(u)).collect(); let projects: Vec<String> = raw.iter().filter(|p| p.to_lowercase().ends_with(".qcvproj")).cloned().collect(); let media: Vec<String> = raw.iter().filter(|p| !p.to_lowercase().ends_with(".qcvproj")).cloned().collect(); let admitted = if media.is_empty() { vec![] } else { self.admit_media_paths(&media) }; for p in &projects { self.open_project(p); } if let DropTarget::Side(sd) = target { self.drop_on_side(sd, admitted); return } if let DropTarget::Playlist(x) = target { let mut ids = vec![]; for p in admitted.iter().cloned() { if let Some(id) = self.add_and_load(&p, false) { ids.push(id); } } self.playlist_insert(&ids, x as f64); return } let mut first = target == DropTarget::Viewport && projects.is_empty(); for p in admitted { if self.add_and_load(&p, first).is_some() { first = false; } } } }
            }
            _ => {}
        }
    }
}

static TRACE_INPUT: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| std::env::var_os("QCV_TRACE_INPUT").is_some());
/// `QCV_TRACE_SCRUB=1`: every scrub request (target, wall ms), every served preview (target, request → ready ms, decoded, hit) and every
/// preview SHOWN (frame, how far behind the latest request) — the windowed app's own scrub latency, which the headless bench cannot see.
static TRACE_SCRUB: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| std::env::var_os("QCV_TRACE_SCRUB").is_some());
fn trace_ms() -> f64 { static T0: std::sync::LazyLock<Instant> = std::sync::LazyLock::new(Instant::now); T0.elapsed().as_secs_f64() * 1000.0 }

fn main() {
    #[cfg(target_os = "macos")] menu::AppMenu::quiet_appkit_extras();   // before NSApp launches: AppKit reads these at launch
    let args: Vec<String> = std::env::args().skip(1).collect();
    // the window logs to a file too (QCView's installFileLogger, after the single-instance handoff below); the harness modes stderr
    let harness = args.iter().any(|a| ["--headless", "--ui-bench", "--audio-bench"].contains(&a.as_str()));
    if harness { env_logger::init(); }
    i18n::install();   // canon row 195: before any panel builds its labels
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned());
    let has = |k: &str| args.iter().any(|a| a == k);
    let file = { let mut f = None; let mut skip = false; for (i, a) in args.iter().enumerate() { if skip { skip = false; continue } if a.starts_with("--") { skip = ["--dual", "--out", "--white", "--seconds", "--snap-out", "--snap-display", "--refresh", "--screen", "--threads", "--headless", "--seeks", "--scrub", "--seek-gap", "--hover", "--cut", "--ui-bench", "--ui-snap", "--ui-size", "--audio-bench"].contains(&a.as_str()); continue } let _ = i; if f.is_none() { f = Some(a.clone()); } } f };
    let files: Vec<String> = { let mut v = vec![]; let mut skip = false; for a in args.iter() { if skip { skip = false; continue } if a.starts_with("--") { skip = ["--dual", "--out", "--white", "--seconds", "--snap-out", "--snap-display", "--refresh", "--screen", "--threads", "--headless", "--seeks", "--scrub", "--seek-gap", "--hover", "--cut", "--ui-bench", "--ui-snap", "--ui-size", "--audio-bench"].contains(&a.as_str()); continue } v.push(a.clone()); } v };
    let out = match arg("--out").as_deref() { Some("sdr") => Output::Sdr, Some("edr") => Output::Edr, Some("pq") => Output::Pq, _ => Output::Auto };
    let snap_out = arg("--snap-out").map(|s| { let (p, n) = s.rsplit_once('@').unwrap_or((&s, "24")); (p.to_string(), n.parse().unwrap_or(24)) });
    let snap_display = arg("--snap-display").map(|s| { let (p, n) = s.rsplit_once('@').unwrap_or((&s, "24")); (p.to_string(), n.parse().unwrap_or(24)) });
    let scrub = arg("--scrub").and_then(|v| { let (r, ms) = v.rsplit_once(':')?; let (a, b) = r.split_once('-')?; Some((a.parse().ok()?, b.parse().ok()?, ms.parse().ok()?)) });
    let o = Opts { dual: arg("--dual"), snap_display, cut: arg("--cut"), hover: arg("--hover").and_then(|v| v.parse().ok()), seek_gap_ms: arg("--seek-gap").and_then(|v| v.parse().ok()).unwrap_or(0), seeks: arg("--seeks").and_then(|v| v.parse().ok()), scrub, scrub_hw: has("--scrub-hw"), headless: arg("--headless").and_then(|v| v.parse().ok()), looping: has("--loop"), file, files, sw: has("--sw"), out, white: arg("--white").and_then(|v| v.parse().ok()).unwrap_or(100.0), seconds: arg("--seconds").and_then(|v| v.parse().ok()).unwrap_or(0.0), stats: has("--stats"), snap_out, verify_pq: has("--verify-pq"), identity_chain: has("--identity-chain"), refresh_ms: arg("--refresh").and_then(|v| v.parse().ok()).unwrap_or(1000.0 / 60.0), screen: arg("--screen").and_then(|v| v.parse().ok()), threads: arg("--threads").and_then(|v| v.parse().ok()).unwrap_or(0), autoplay: has("--play") };
    if let Some(secs) = arg("--audio-bench").and_then(|v| v.parse().ok()) { run_audio_bench(o, secs); return }
    if let Some(n) = arg("--ui-bench").and_then(|v| v.parse().ok()) { run_ui_bench(o, n, arg("--ui-snap"), arg("--ui-size").and_then(|v| v.split_once('x').and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?))))); i18n::write_misses(); return }
    if o.headless.is_some() { run_headless(o); return }
    // QCView's single-instance handoff: a running window takes this launch's arguments (absolute: its working folder differs)
    let multi = std::env::var_os("QCV_MULTI_INSTANCE").is_some();
    let fwd: Vec<String> = o.files.iter().map(|a| if a.contains("://") { a.clone() } else { std::fs::canonicalize(a).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| a.clone()) }).collect();
    if !multi && single_instance::forward(&fwd) { println!("qcv: handed {} argument(s) to the running window", fwd.len()); return }
    let _ = logging::install();
    let single_rx = if multi { None } else { single_instance::listen() };
    let el = EventLoop::new().unwrap();
    let settings = settings::open();
    i18n::apply(&settings.string(settings::k::LANGUAGE));   // row 197: the picker's language (QCV_LANG wins), before the window builds its panels
    let mut settings = settings;
    settings.set_bool(settings::k::COLOR_ENABLED, false);   // colour starts off at every launch; a project with assigned chains turns it on
    let colors = color_model::Library::load(settings.path());
    let app = App { thumb_color: Default::default(), capture_rx: vec![], export_rx: vec![], dual: None, active_dual_view: String::new(), dual_editor: None, annot: annotate::Annotations::new(), colors, ocio_configs: HashMap::new(), mincolor: [None, None], mincolor_ready: [None, None], color_bound: [None, None], ocio_rx: [None, None], single_rx, #[cfg(target_os = "macos")] menu: None, alt_clean: false, menu_queue: vec![], unsupported_toast: None, pl_editor: None, drag_pos: None, hw_intra: None, last_tick: Instant::now(), scrub_last_req: None, o, gpu: None, player: None, hw: None, settings, meta: qcv_meta::MetadataService::new(2), project: qcv_project::ProjectManager::new(), loaded_item: String::new(), source_label: String::new(), source_path: String::new(), t0: None, t_open: None, first_presented: false, presents: vec![], last_present: None, show_ms: vec![], last_report: Instant::now(), shown_in_window: 0, fetches: HashMap::new(), mods: qcv_player::Mods::default(), finished: false, frames_presented: 0, edr_seen: 0.0, screen_hdr: None, snapped: false, verified: false };
    el.run_app(app).unwrap();
    println!("main: event loop returned · wall {}", unix_ms());   // anything after this is the process's own exit (DLL detach, driver teardown)
}
fn unix_ms() -> u128 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0) }

/// `--headless N`: no window — decode N frames through the player, show each on a headless engine (the same zero-copy
/// import and stage graph), write `--snap-out` / run `--verify-pq`. The evidence that needs no display.
/// `--audio-bench SECONDS file`: no window, no speaker — the Transport plays through a wall-paced silent device; the
/// servo's drift is sampled every refresh. Then a seek to one third and the time it takes the drift to re-enter ±20 ms,
/// then a hold/release cycle through pause → play. The step-2b bar: drift settles inside the quiet band and stays,
/// no re-seeks after the first second, no underruns at 1×.
fn run_audio_bench(o: Opts, seconds: f32) {
    let Some(file) = o.file.clone() else { println!("--audio-bench needs a file"); return };
    let src = Source::from_path(&file, None);
    let mut p = match Transport::open(&src, TransportOptions { hw: None, threads: o.threads, loop_enabled: true, scrub: None, ..Default::default() }) { Ok(p) => p, Err(e) => { println!("open {file}: {e}"); return } };
    let a = match qcv_player::AudioPlayer::headless() { Ok(a) => a, Err(e) => { println!("audio: {e}"); return } };
    p.set_audio(Box::new(a));
    println!("opened {} — {}; audio: {}", src.display(), describe(&p), if p.audio.has_audio() { "open" } else { "NONE — the bench needs a file with an audio stream" });
    if !p.audio.has_audio() { return }
    let refresh = std::time::Duration::from_secs_f32(o.refresh_ms / 1000.0);
    let run = |p: &mut Transport, secs: f32, label: &str| {
        let t0 = Instant::now(); let mut drift: Vec<(f32, f32)> = vec![];
        while t0.elapsed().as_secs_f32() < secs {
            let now = Instant::now(); let _ = p.tick(now); if p.current().is_none() { p.idle_refresh(); }
            for d in p.audio.take_drift() { drift.push((t0.elapsed().as_secs_f32(), d)); }
            std::thread::sleep(refresh);
        }
        let settle = drift.iter().find(|(_, d)| d.abs() <= 20.0).map(|(t, _)| *t);
        let after: Vec<f32> = drift.iter().filter(|(t, _)| settle.is_some_and(|s| *t >= s + 0.5)).map(|(_, d)| *d).collect();
        let mut abs: Vec<f32> = after.iter().map(|v| v.abs()).collect();
        let mut signed: Vec<f32> = after.clone();
        let a = p.audio.stats().unwrap_or_default();
        println!("{label}: {} drift samples; first inside ±20 ms at {}; after settling: drift p50 {:+.1} |drift| p50 {:.1} p95 {:.1} max {:.1} ms (n={}); re-seeks {} holds {} skips {} underruns {} starved {} fades {} callbacks {} audible {} contended {}",
            drift.len(), settle.map(|s| format!("{s:.2} s")).unwrap_or("never".into()), pct(&mut signed, 50.0), pct(&mut abs, 50.0), pct(&mut abs, 95.0), pct(&mut abs, 100.0), after.len(), a.reseeks, a.holds, a.skips, a.underruns, a.starved, a.fades, a.callbacks, a.audible, a.contended);
    };
    p.play();
    run(&mut p, seconds, "play from 0");
    let third = p.frame_count() / 3; p.seek_to_frame(third);
    run(&mut p, (seconds / 2.0).max(3.0), &format!("seek to frame {third} while playing"));
    p.pause(); std::thread::sleep(std::time::Duration::from_millis(300)); p.tick(Instant::now()); p.play();
    run(&mut p, (seconds / 2.0).max(3.0), "pause 300 ms → play (hold + release)");
    // the shuttle: hold fast-forward for 1.5 s (2× → 4× → 8×), then rewind for 1 s; the grains feed the callback instead of the decoder
    let t0 = Instant::now(); p.start_fast_seek(1, t0); let mut calls0 = p.audio.stats().map(|a| a.callbacks).unwrap_or(0);
    while t0.elapsed().as_secs_f32() < 1.5 { let now = Instant::now(); let _ = p.tick(now); std::thread::sleep(refresh); }
    let active_ff = p.audio.shuttle_active(); p.stop_fast_seek();
    let t1 = Instant::now(); p.start_fast_seek(-1, t1);
    while t1.elapsed().as_secs_f32() < 1.0 { let now = Instant::now(); let _ = p.tick(now); std::thread::sleep(refresh); }
    let active_rw = p.audio.shuttle_active(); p.stop_fast_seek();
    let calls1 = p.audio.stats().map(|a| a.callbacks).unwrap_or(0); calls0 = calls1 - calls0;
    println!("shuttle: ff 1.5 s (active {active_ff}) + rw 1 s (active {active_rw}): {calls0} device callbacks fed by grains; after release the player is {} at frame {}", if p.is_playing() { "playing" } else { "paused" }, p.current_frame());
    if !p.is_playing() { p.play(); }
    run(&mut p, 3.0, "play on after the shuttle");
    // review speeds: 2× then 0.5× through the WSOLA stage (constant pitch), then back to 1×
    p.set_review_speed(2.0); run(&mut p, 4.0, "review speed 2× (tempo)");
    p.set_review_speed(0.5); run(&mut p, 3.0, "review speed 0.5× (tempo)");
    p.set_review_speed(1.0); run(&mut p, 3.0, "back to 1×");
    println!("source: {} channels ({})", p.audio.source_channels(), p.audio.layout_name());
    println!("process CPU {:.0} % of one core over the run; max RSS {:.0} MB", 0.0, qcv_player::max_rss_mb());
}

/// `--ui-bench N [--ui-snap PATH] [file]`: no window — the UI layer at 2560×1440 (scale 2) over a headless engine; the
/// token sketch is rebuilt + rendered N times at a moving frame (scene-build ms and vello render ms apart), then the
/// composite (over the file's first frame when one is given) is written as an sRGB PNG. The piece-6 harness: the bar is
/// ≤ 1 ms of CPU when the layer changes, nothing when it doesn't.
fn run_ui_bench(o: Opts, n: u32, snap: Option<String>, size: Option<(u32, u32)>) {
    use min_present::engine::{DisplayMode, Engine, OutMode};
    let (w, h) = size.unwrap_or((2560u32, 1440u32));
    let scale = 2.0f64;
    let mut engine = Engine::headless(wgpu::TextureFormat::Rgba16Float).expect("headless engine");
    engine.set_display(DisplayMode { out: OutMode::SdrEncode, white: o.white });
    engine.set_viewport(w, h);
    let mut frame_info: Option<(i32, i32, f64)> = None;
    let mut player: Option<Transport> = None;
    if let Some(file) = o.file.clone() {
        let src = Source::from_path(&file, None);
        match Transport::open(&src, TransportOptions { hw: None, threads: o.threads, loop_enabled: true, scrub: None, ..Default::default() }) {
            Ok(mut p) => {
                p.play();
                let deadline = Instant::now() + std::time::Duration::from_secs(10);
                while p.current().is_none() && Instant::now() < deadline { if let Some(d) = p.tick(Instant::now()) { engine.show(&d.frame).expect("show"); break } p.idle_refresh(); std::thread::sleep(std::time::Duration::from_micros(500)); }
                p.pause();
                frame_info = Some((p.current_frame(), p.frame_count(), p.rate().as_f64()));
                println!("opened {} — {}", src.display(), describe(&p));
                player = Some(p);
            }
            Err(e) => println!("open {file}: {e} — the sketch over black"),
        }
    }
    let t = Instant::now();
    let mut fonts = min_ui::Fonts::new();
    println!("fonts registered in {:.1} ms", t.elapsed().as_secs_f32() * 1000.0);
    let mut ui = min_ui::UiLayer::new(&engine.device, w, h).expect("ui layer");
    engine.set_overlay(Some(ui.view()));
    // the transport bar in every state the snap can show: hover (step forward, with its tooltip), pressed (set in),
    // selected (loop), disabled (the unbacked buttons)
    engine.set_content_rect(Some(content_rect(w, h, scale)));
    let mut strip = status_strip::StatusStrip::new();
    let mut bar = transport_bar::TransportBar::new();
    let bands_y = h as f64 - (t::STATUS_STRIP_HEIGHT + t::TRANSPORT_BAR_HEIGHT + t::SCRUBBER_HEIGHT) as f64 * scale + t::STATUS_STRIP_HEIGHT as f64 * scale;
    let t0 = Instant::now();
    // the rails: the left open on Settings (a switch hovered, a number field focused), the right open on the Inspector so the snap shows both
    let mut store = min_settings::Settings::in_memory(&settings::SCHEMA);
    let mut left = min_ui::Rail::new(min_ui::Side::Left, 280.0, false); left.settle();
    let mut right = min_ui::Rail::new(min_ui::Side::Right, 320.0, false); right.settle();
    let rails_bottom = bands_y - t::STATUS_STRIP_HEIGHT as f64 * scale;
    // `QCV_UI_SNAP_COLOR=1`: the Color drawer open (row 199) on a sample state — a user preset, the chain edited (Custom), a clip that owns it
    let mut drawer = std::env::var_os("QCV_UI_SNAP_COLOR").map(|_| { let mut d = color_drawer::ColorDrawer::new(true, 320.0, (None, 1000.0, true)); d.set_pointer_present(true, t0); d });
    let rails_bottom = rails_bottom - drawer.as_ref().map(|d| d.visible_height(t0, scale)).unwrap_or(0.0);
    // `QCV_UI_SNAP_MENU=1`: the in-window menu bar (row 194) on top, File open, the Recent Media submenu cascaded
    let mut menu_bar = std::env::var_os("QCV_UI_SNAP_MENU").map(|_| menu::InWindowMenu::new(&mut fonts));
    let menu_h = if menu_bar.is_some() { (t::MENU_BAR_HEIGHT as f64 * scale).round() } else { 0.0 };
    left.layout(0.0, menu_h, rails_bottom, scale, t0); right.layout(w as f64, menu_h, rails_bottom, scale, t0);
    engine.set_content_rect(Some((left.visible_width(t0, scale) as u32, menu_h as u32, (w as f64 - left.visible_width(t0, scale) - right.visible_width(t0, scale)) as u32, (rails_bottom - menu_h) as u32)));
    if let Some(mb) = menu_bar.as_mut() {
        mb.update(&["/media/review_v003.mov".into(), "/media/plate_0001.exr".into()], &["/projects/review.qcvproj".into()], false, true, &mut fonts);
        mb.bar.layout(&mut fonts, 0.0, 0.0, w as f64, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), scale);
        let r = mb.bar.rect; let x = r.x0 + 30.0 * scale;   // the File title
        mb.bar.event(&min_ui::Event::Down { x, y: r.center().y, button: min_ui::Button::Primary }, &mut fonts);
        mb.bar.event(&min_ui::Event::Up { x, y: r.center().y, button: min_ui::Button::Primary }, &mut fonts);
        // the sixth row (Open Recent Media): rows are 32 px under the popover's padding
        let y = r.y1 + (t::POPOVER_PADDING as f64 + 5.5 * t::MENU_ITEM_HEIGHT as f64) * scale;
        mb.bar.event(&min_ui::Event::Move { x: x + 40.0 * scale, y }, &mut fonts);
    }
    let mut panel = settings_panel::SettingsPanel::new(); panel.sync(&store, t0);
    let mut project = qcv_project::ProjectManager::new(); let mut pp = project_panel::ProjectPanel::new();
    let loaded = if let Some(p) = player.as_ref() { project.add_media_file(p.path()).0.unwrap_or_default() } else { String::new() };
    // `QCV_UI_SNAP_TREE=1`: a playlist of three clips loaded, its tree open on the second (the snapshot of the Media tree)
    let loaded = if std::env::var_os("QCV_UI_SNAP_TREE").is_some() {
        let pid = project.create_playlist(&[], "Review_01", 24.0, 1920, 1080, 0, 0).0;
        pp.set_tree(Some(project_panel::TreeModel { playlist: pid.clone(), clips: vec![("a".into(), "shot_010_v003.mov".into(), qcv_project::MediaType::Video), ("b".into(), "render_[1001-1048]".into(), qcv_project::MediaType::ImageSequence), ("c".into(), "shot_020_comp_v012_with_a_long_name.mov".into(), qcv_project::MediaType::Video)], current: Some(1), info: "3 clips · 1920×1080".into() }));
        pid
    } else { loaded };
    let _ = pp.sync(&project, &loaded);
    let mut bgp = background_panel::BackgroundPanel::new(); bgp.sync(2); engine.set_background(2); engine.set_ui_scale(scale as f32);
    // `QCV_UI_SNAP_RELABEL=<code>`: the panels built in the launch language, then the live switch (row 197) — what the Language picker does
    if let Ok(code) = std::env::var("QCV_UI_SNAP_RELABEL") { min_i18n::set_language(&code); panel.relabel(); panel.sync(&store, t0); pp.relabel(); let _ = pp.sync(&project, &loaded); bgp.relabel(); bar.relabel(); strip.relabel(); println!("relabel: {} (generation {})", min_i18n::language(), min_i18n::generation()); }
    let lc = left.content_rect(); let pph = pp.layout(&mut fonts, lc.x0, lc.x1, lc.y0, scale); let bh = bgp.layout(&mut fonts, lc.x0, lc.x1, lc.y0 + pph, scale); let ph = panel.layout(&mut fonts, lc.x0, lc.x1, lc.y0 + pph + bh, scale); left.content_height = pph + bh + ph;
    // the tree drag, scripted: clip 0 dragged below the last row → MoveClip to slot 3; then pressed + dragged again for the snap
    if std::env::var_os("QCV_UI_SNAP_TREE").is_some() && let (Some(r0), Some(r2)) = (pp.tree_rows().first().copied(), pp.tree_rows().last().copied()) {
        let b = kurbo::Rect::new(0.0, 0.0, w as f64, h as f64); let ev = |pp: &mut project_panel::ProjectPanel, fonts: &mut min_ui::Fonts, e: min_ui::Event| pp.event(&e, t0, false, false, fonts, b, scale).0;
        let (cx, y0, y1) = (r0.center().x, r0.center().y, r2.y1 + 2.0 * scale);
        ev(&mut pp, &mut fonts, min_ui::Event::Down { x: cx, y: y0, button: min_ui::Button::Primary }); ev(&mut pp, &mut fonts, min_ui::Event::Move { x: cx, y: y1 });
        println!("tree drag: {:?}", ev(&mut pp, &mut fonts, min_ui::Event::Up { x: cx, y: y1, button: min_ui::Button::Primary }));
        // a bin row dragged onto the tree between clips 0 and 1 → slot 1, a DropRows the shell inserts
        if let Some((_, vr)) = pp.row_rects().into_iter().find(|(n, _)| n.ends_with(".mov")) {
            let yb = r0.y1 + t::RAIL_ROW_GAP as f64 * scale / 2.0;
            ev(&mut pp, &mut fonts, min_ui::Event::Down { x: vr.center().x, y: vr.center().y, button: min_ui::Button::Primary });
            ev(&mut pp, &mut fonts, min_ui::Event::Move { x: vr.center().x, y: vr.center().y + 20.0 * scale }); ev(&mut pp, &mut fonts, min_ui::Event::Move { x: cx, y: yb });
            println!("bin drop over the tree: slot {:?}, {:?}", pp.tree_drop_slot(cx, yb), ev(&mut pp, &mut fonts, min_ui::Event::Up { x: cx, y: yb, button: min_ui::Button::Primary }));
        }
        ev(&mut pp, &mut fonts, min_ui::Event::Down { x: cx, y: y0, button: min_ui::Button::Primary }); ev(&mut pp, &mut fonts, min_ui::Event::Move { x: cx + 30.0 * scale, y: (r2.center().y + r2.y1) / 2.0 + 4.0 * scale });
    }
    let mut inspector = inspector_panel::InspectorPanel::new(); let _ = inspector.sync(player.as_ref());
    if let Some(p) = player.as_ref() { let path = p.path().to_string(); inspector.set_video_meta(&path, qcv_meta::extract(&path)); inspector.set_adobe_meta(&path, qcv_meta::xmp::read_adobe_metadata(&path)); let _ = inspector.sync(player.as_ref()); }
    std::thread::sleep(std::time::Duration::from_millis(400)); let _ = inspector.sync(player.as_ref());   // the hero thumbnail's worker lands
    // an EXR sequence: the layer tiles as the project would fill them (discovery on the first frame), thumbnails awaited
    if let Some(c) = player.as_ref().and_then(|p| p.decoder().as_sequence()).map(|s| s.cache()) {
        let first = c.frame_path(0);
        let layers = if first.to_lowercase().ends_with(".exr") { qcv_player::min_image::exr::layers(&first).unwrap_or_default() } else { vec![] };
        if !layers.is_empty() { let cur = std::env::var("QCV_SNAP_LAYER").unwrap_or_else(|_| layers[0].clone()); inspector.set_seq_layers(layers, &cur, &first); std::thread::sleep(std::time::Duration::from_millis(800)); let _ = inspector.sync(player.as_ref()); }
    }
    let rc = right.content_rect(); let ih = inspector.layout(&mut fonts, rc.x0, rc.x1, rc.y0, scale); right.content_height = ih;
    // `QCV_UI_SNAP_SHORTCUTS=1` (or a filter text): the Keyboard Shortcuts card expanded at the top of the right rail (row 218)
    let mut sc_panel = std::env::var("QCV_UI_SNAP_SHORTCUTS").ok().map(|q| { let mut sp = shortcuts::ShortcutsPanel::new(); sp.section.expanded = true; if q != "1" { sp.set_filter(&q); } sp });
    if let Some(sp) = sc_panel.as_mut() { let sh = sp.layout(&mut fonts, rc.x0, rc.x1, rc.y0 + ih, scale); right.content_height = ih + sh; right.wheel(ih - 8.0 * scale, t0); right.layout(w as f64, menu_h, rails_bottom, scale, t0); let rc = right.content_rect(); let ih = inspector.layout(&mut fonts, rc.x0, rc.x1, rc.y0, scale); sp.layout(&mut fonts, rc.x0, rc.x1, rc.y0 + ih, scale); }
    // the snap shows the Inspector's lower half (Color / Audio / Timecode with their pickers): scroll the rail
    if std::env::var_os("QCV_SNAP_ROTATE").is_some() { engine.set_geometry(min_present::engine::Geometry { par: min_present::engine::Par::Square, rotation: Some(90) }); inspector.set_geometry(engine.geometry(), engine.display_dims()); let _ = inspector.sync(player.as_ref()); let rc = right.content_rect(); inspector.layout(&mut fonts, rc.x0, rc.x1, rc.y0, scale); }
    if std::env::var_os("QCV_SNAP_SCROLL").is_some() { right.wheel(ih, t0); right.layout(w as f64, 0.0, rails_bottom, scale, t0); let rc = right.content_rect(); inspector.layout(&mut fonts, rc.x0, rc.x1, rc.y0, scale); }
    { let bounds = kurbo::Rect::new(0.0, 0.0, w as f64, h as f64);
      // hover the "Timeline hover thumbnails" switch's label (its help tooltip) and press the scrub-cache field
      let _ = panel.event(&min_ui::Event::Move { x: lc.x0 + 20.0 * scale, y: lc.y0 + 330.0 * scale }, t0, &mut fonts, &mut store, bounds, scale);
      let _ = panel.event(&min_ui::Event::Down { x: lc.x1 - 60.0 * scale, y: lc.y0 + 215.0 * scale, button: min_ui::Button::Primary }, t0, &mut fonts, &mut store, bounds, scale); }
    println!("settings panel: {} px tall at {scale}× ({} rows); inspector {} px", ph.round(), settings::SCHEMA.keys.len(), ih.round());
    // probe: the Range picker's Full chip → the Transport's override (the user 2026-11-07: "Range is def not affecting image")
    if let Some(p) = player.as_mut() {
        let before = engine.render_offscreen(w, h).expect("composite");
        if let Some(r) = inspector.chip_rect("Range", 1) {
            let c = r.center();
            let _ = inspector.event(&min_ui::Event::Down { x: c.x, y: c.y, button: min_ui::Button::Primary }, t0, &mut fonts);
            let (cmds, _) = inspector.event(&min_ui::Event::Up { x: c.x, y: c.y, button: min_ui::Button::Primary }, t0, &mut fonts);
            println!("range probe: chip at {:?} → {:?}", r, cmds);
            for c in cmds { if let inspector_panel::Cmd::Range(ro) = c { p.set_range_override(ro); } }
            println!("range probe: transport override now {:?}", p.range_override());
            // re-decode the current frame and compare a mid-grey pixel
            let f = p.current_frame(); p.seek_to_frame(f);
            let deadline = Instant::now() + std::time::Duration::from_secs(3); let mut shown = false;
            while Instant::now() < deadline { if let Some(d) = p.tick(Instant::now()) { engine.show(&d.frame).expect("show"); shown = true; break } p.idle_refresh(); std::thread::sleep(std::time::Duration::from_millis(2)); }
            let after = engine.render_offscreen(w, h).expect("composite");
            let idx = ((h / 2) as usize * w as usize + (w / 2) as usize) * 4;
            println!("range probe: re-decoded {shown}; centre pixel before {:.4} after {:.4} (meta range of the shown frame: {:?})", before[idx], after[idx], p.current().map(|h| h.frame.meta.range));
        } else { println!("range probe: no Range chips in the panel"); }
    }
    bar.sync(player.as_ref().map(|p| p as &dyn Playback), t0);
    bar.layout(&mut fonts, w as f64, bands_y, scale);
    let (hover, press) = (bar.rect_of("caret-right").unwrap().center(), bar.rect_of("sign-out").unwrap().center());
    bar.event(&min_ui::Event::Down { x: press.x, y: press.y, button: min_ui::Button::Primary }, t0);
    bar.event(&min_ui::Event::Move { x: hover.x, y: hover.y }, t0);
    let shown_at = t0 + std::time::Duration::from_millis(t::TOOLTIP_DELAY as u64 + 100);   // the hover ramp done, the tooltip up
    bar.sync(player.as_ref().map(|p| p as &dyn Playback), shown_at); bar.tick(shown_at);
    strip.sync(player.as_ref().map(|p| p as &dyn Playback));
    strip.layout(w as f64, bands_y - t::STATUS_STRIP_HEIGHT as f64 * scale, scale);
    // `QCV_UI_SNAP_DUAL=1`: dual view's controls in the strip (side by side, A and B named) — the row's weight against the transport
    let mut dualc = std::env::var_os("QCV_UI_SNAP_DUAL").map(|_| { let mut d = dual_controls::DualControls::new(); d.sync(true, true, CompareMode::SideBySide, Some("review_v003.mov".into()), Some("review_v004.mov".into()), 0.5, 1.0, false);
        let sr = strip.rect; let fw = strip.fps_width(&mut fonts, scale); let gap = t::SPACING_300 as f64 * scale; let ew = strip.extra_width(&mut fonts, scale);
        d.layout(&mut fonts, strip.wells_right() + ew + gap, sr.x1 - t::SPACING_100 as f64 * scale - fw - if fw > 0.0 { gap } else { 0.0 }, sr.y0, sr.height(), scale); d });
    strip.stage(status_strip::Mode::Timecode, "1000", &mut fonts, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), scale);
    let mut scrubber = min_ui::Scrubber::new();
    let mut thumbs = hover_thumbs::HoverThumbs::new();
    // an image sequence: let the cache fill so the band + failed ticks are in the snap
    if let Some(p) = player.as_ref() && p.decoder().as_sequence().is_some() { std::thread::sleep(std::time::Duration::from_millis(600)); scrubber.failed = sequence_failed_on_timeline(p); scrubber.cache_wraps = p.decoder().as_sequence().unwrap().cache().looping(); }
    let st = scrub_state(player.as_ref().map(|p| p as &dyn Playback), player.as_ref());
    scrubber.layout(kurbo::Rect::new(0.0, h as f64 - t::SCRUBBER_HEIGHT as f64 * scale, w as f64, h as f64), scale);
    if std::env::var_os("QCV_UI_SNAP_FIT").is_none() { scrubber.wheel(w as f64 * 0.4, 600.0, false, &st, scale); }   // zoomed in about 40 % — frame ticks + the extent marker (`QCV_UI_SNAP_FIT=1`: the whole timeline)
    scrubber.event(&min_ui::Event::Move { x: w as f64 * 0.62, y: h as f64 - 10.0 }, t0, &st, scale);
    if let Some(p) = player.as_mut() { thumbs.on_open(p); let deadline = Instant::now() + std::time::Duration::from_millis(1500); while Instant::now() < deadline { thumbs.tick(Some(p), scrubber.hover_frame, Instant::now(), (0, &mut |_| None)); if thumbs_shown(&thumbs) { break } std::thread::sleep(std::time::Duration::from_millis(20)); } }
    let content = engine.content_rect().map(|(x, y, cw, ch)| kurbo::Rect::new(x as f64, y as f64, (x + cw) as f64, (y + ch) as f64)).unwrap();
    let mut wave_peaks: Vec<(f32, f32)> = vec![];
    if let Some(p) = player.as_ref() {
        let eng = qcv_player::WaveformProbeEngine::new();
        let (t0w, t1w) = scrubber.visible_window(&st); let cols = scrubber.track_columns();
        eng.request_window(p.path(), t0w, t1w, cols);
        let deadline = Instant::now() + std::time::Duration::from_millis(3000); let mut last = 0;
        while Instant::now() < deadline { let g = eng.data_generation(); if g != last { last = g; let c = eng.sample_columns(p.path(), t0w, t1w, cols); if c.iter().filter(|c| c.valid).count() * 10 >= cols * 9 { wave_peaks = c.into_iter().map(|c| (c.mn, c.mx)).collect(); break } } std::thread::sleep(std::time::Duration::from_millis(30)); }
        println!("waveform: {} of {} columns probed in time", wave_peaks.len(), cols);
    }
    let (mut build_ms, mut render_ms) = (vec![], vec![]);
    for i in 0..n.max(1) {
        let fi = frame_info.map(|(_, c, f)| ((i as i32 * 7) % c.max(1), c, f)).or(Some((i as i32, 240, 24.0)));
        let t = Instant::now();
        let scene = ui.scene_mut();
        let _ = fi;
        ui_sketch::bands(scene, w, h, scale, 0.0, 0.0);
        for (rail, is_left) in [(&left, true), (&right, false)] { rail.paint_background(scene, shown_at); rail.push_clip(scene); if is_left { pp.paint(scene, &mut fonts, scale, shown_at, true); bgp.paint(scene, &mut fonts, scale, shown_at, false); panel.paint(scene, &mut fonts, scale, shown_at, false); } else { inspector.paint(scene, &mut fonts, scale, shown_at); if let Some(sp) = sc_panel.as_mut() { sp.paint(scene, &mut fonts, scale, shown_at, false); } } rail.pop_clip(scene); rail.paint_foreground(scene, &mut fonts, scale, shown_at); }
        pp.paint_menu(scene, &mut fonts, scale, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), shown_at);   // the drag ghost / insertion line / menu, over the rails
        strip.paint(scene, &mut fonts, scale, shown_at);
        if let Some(d) = dualc.as_mut() { d.paint(scene, &mut fonts, scale, shown_at); }
        bar.paint(scene, &mut fonts, scale, shown_at);
        scrubber.paint(scene, &mut fonts, scale, &st, if wave_peaks.is_empty() { None } else { Some(&wave_peaks) });
        thumbs.paint(scene, content, scale, Some(w as f64 * 0.62));
        strip.paint_popover(scene, &mut fonts, scale);
        bar.paint_tooltip(scene, &mut fonts, scale, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), shown_at);
        panel.tick(shown_at); panel.paint_overlays(scene, &mut fonts, scale, kurbo::Rect::new(0.0, 0.0, w as f64, h as f64), shown_at);
        if let Some(d) = drawer.as_mut() {
            let mut presets = color_model::builtins();
            let mine = color_model::Stored { chain: color_model::ClipChain::MinColor(qcv_color::Chain { in_gamut: 3, in_transfer: 14, knee_on: 1, ..Default::default() }), preset: "Client PQ review".into() };
            presets.push(color_model::Preset { name: "Client PQ review".into(), stored: mine.clone(), builtin: false });
            let edited = color_model::Stored { chain: color_model::ClipChain::MinColor(qcv_color::Chain { in_gamut: 3, in_transfer: 14, knee_on: 1, knee_tgt: 200.0, agx_on: 1, ..Default::default() }), preset: "Client PQ review".into() };
            // `QCV_UI_SNAP_COLOR=ocio`: an OCIO chain on the bundled ACES 2.0 config
            let (stored, ocio) = if std::env::var("QCV_UI_SNAP_COLOR").as_deref() == Ok("ocio") {
                let cfg = qcv_color::ocio::Config::open(&color::ocio_config_path("ACES_2.0")).expect("ACES 2.0 config");
                let o = color_model::OcioChain { config: "ACES_2.0".into(), input: cfg.inputs().into_iter().find(|i| i.contains("ACEScg")).unwrap_or_default(), look: String::new(), view: cfg.views().into_iter().find(|v| v.contains("SDR")).unwrap_or_default(), scene_lut: String::new(), knee: qcv_color::Knee { enabled: true, source_nits: 1000.0, target_nits: 1000.0, start: -1.0 } };
                (color_model::Stored { chain: color_model::ClipChain::Ocio(o), preset: String::new() }, Some(color_drawer::OcioLists { configs: color::BUNDLED.iter().map(|(k, l)| (k.to_string(), l.to_string())).collect(), inputs: cfg.inputs(), looks: cfg.looks(), views: cfg.views() }))
            } else { (edited, None) };
            d.sync(color_drawer::View { target: "HDR_Chart_P3-D65_PQ.mov".into(), side: None, has_clip: true, own: true, stored, custom: true, on: true, presets, file_peak: (1000, 1000.0), ocio, hdr_source: true, clip_warn: std::env::var_os("QCV_UI_SNAP_CLIP").is_some() }, t0);
            d.layout(&mut fonts, content, 0.0, rails_bottom, w as f64, scale, t0);
            engine.set_scope(d.scope());
            d.paint(scene, &mut fonts, shown_at); d.paint_tab(scene, &mut fonts, shown_at); d.paint_overlays(scene, &mut fonts, kurbo::Rect::new(0.0, 0.0, 1e5, 1e5), shown_at);
        }
        if let Some(mb) = menu_bar.as_ref() { mb.bar.paint(scene, &mut fonts, scale); mb.bar.paint_menus(scene, &mut fonts, scale); }
        build_ms.push(t.elapsed().as_secs_f32() * 1000.0);
        if let Some(ms) = ui.render(&engine.device, &engine.queue) { render_ms.push(ms); }
        let _ = engine.device.poll(wgpu::PollType::Wait { submission_index: None, timeout: None });
    }
    assert!(ui.render(&engine.device, &engine.queue).is_none(), "a clean layer renders nothing");
    println!("ui layer {w}x{h} @{scale}: {} builds — scene build ms p50 {:.2} p95 {:.2} max {:.2}; vello render (CPU, encode+submit) ms p50 {:.2} p95 {:.2} max {:.2}; clean layer: no render",
        n, pct(&mut build_ms, 50.0), pct(&mut build_ms, 95.0), pct(&mut build_ms, 100.0), pct(&mut render_ms, 50.0), pct(&mut render_ms, 95.0), pct(&mut render_ms, 100.0));
    let t = Instant::now();
    let px = engine.render_offscreen(w, h).expect("composite");
    println!("composite {w}x{h} render + readback {:.1} ms", t.elapsed().as_secs_f32() * 1000.0);
    if let Some(path) = snap { write_png_rgb8(&path, &px, w, h); println!("ui snap written: {path}"); }
    println!("hover thumbnail staged: {}", if thumbs_shown(&thumbs) { "shown" } else { "none" });
    thumbs.shutdown();
    println!("max RSS {:.0} MB", qcv_player::max_rss_mb());
}
fn thumbs_shown(t: &hover_thumbs::HoverThumbs) -> bool { t.is_shown() }

fn run_headless(o: Opts) {
    if o.cut.is_some() { return run_cut_bench(o) }
    if o.hover.is_some() { return run_hover_bench(o) }
    if o.seeks.is_some() || o.scrub.is_some() { return run_scrub_bench(o) }
    use min_present::engine::{DisplayMode, Engine, OutMode};
    let n = o.headless.unwrap_or(1);
    let Some(file) = o.file.clone() else { println!("--headless needs a file"); return };
    let mut engine = Engine::headless(wgpu::TextureFormat::Rgba16Float).expect("headless engine");
    engine.set_display(DisplayMode { out: OutMode::Linear, white: o.white });
    if o.identity_chain { engine.set_color_pass(Some(Box::new(min_present::IdentityColorPass))); }
    if let Some(rx) = color::wanted_ahead(&engine.device, &engine.queue) { let t = Instant::now(); match rx.recv() { Ok(p) => { engine.set_color_pass(Some(p)); println!("headless: colour chain {}{} bound ({:.0} ms build)", std::env::var("QCV_OCIO").unwrap_or_default(), std::env::var("QCV_MINCOLOR").unwrap_or_default(), t.elapsed().as_secs_f64() * 1000.0); } Err(_) => println!("headless: colour chain FAILED (see the log)") } }
    #[cfg(windows)] let dev = engine.dx12_device_raw();
    #[cfg(not(windows))] let dev = None;
    let hw = if o.sw { None } else { HwDevice::platform(dev) }; let hw_intra = if o.sw { None } else { HwDevice::vulkan(dev) };
    println!("headless engine: zero-copy {} · decode {}", engine.zero_copy_available(), hw.as_ref().map(|h| h.name()).unwrap_or("software"));
    let src = Source::from_path(&file, None);
    let t = Instant::now();
    let mut p = match Transport::open(&src, TransportOptions { hw, hw_intra, threads: o.threads, loop_enabled: false, scrub: None, uploader: Some(engine.uploader()), ..Default::default() }) { Ok(p) => p, Err(e) => { println!("open {file}: {e}"); return } };
    println!("opened {} — {}  (open took {:.0} ms)", src.display(), describe(&p), t.elapsed().as_secs_f32() * 1000.0);
    p.play();
    let mut first: Option<f32> = None; let mut show_ms = vec![];
    let deadline = Instant::now() + std::time::Duration::from_secs(60);
    while p.current().map(|h| h.frame_no + 1).unwrap_or(0) < n as i32 && !p.at_end() && Instant::now() < deadline {
        let now = Instant::now();
        let Some(d) = p.tick(now) else { p.idle_refresh(); std::thread::sleep(std::time::Duration::from_micros(500)); continue };
        match engine.show(&d.frame) { Ok(()) => { show_ms.push(engine.last_show_ms); if first.is_none() { first = Some(t.elapsed().as_secs_f32() * 1000.0); } } Err(e) => { println!("show: {e}"); return } }
        let shown = d.frame_no as u32;
        if let Some((path, k)) = &o.snap_display { if shown == *k { match engine.snapshot(Snapshot::Display) { Ok((px, w, h)) => { write_png_display(path, &px, w, h, true); println!("snap-display written: {path} ({w}x{h}, the display path, linear surface encoded sRGB, frame {k})"); } Err(e) => println!("snap-display: {e}") } } }
        if let Some((path, k)) = &o.snap_out { if shown == *k { match engine.snapshot(Snapshot::Encoded) { Ok((px, w, h)) => { write_png_rgb8(path, &px, w, h); println!("snap-out written: {path} ({w}x{h}, display-encoded 8-bit, frame {k})"); } Err(e) => println!("snap-out: {e}") } } }
        if o.verify_pq && shown == 24 {
            let Ok((px, w, h)) = engine.snapshot(Snapshot::Linear) else { continue };
            let pq = |c: f64| { let (m1, m2, c1, c2, c3) = (0.1593017578125, 78.84375, 0.8359375, 18.8515625, 18.6875); let e = c.powf(1.0 / m2); ((e - c1).max(0.0) / (c2 - c3 * e)).powf(1.0 / m1) * 10000.0 };
            let white = o.white as f64; let row = (h / 4) as usize;
            let (mut worst, mut worst_x, mut over1) = (0f64, 0usize, 0u32);
            for x in 0..w as usize { let code = x as f64 / (w - 1) as f64; let exp = pq(code) / white; let got = px[(row * w as usize + x) * 4] as f64; if got > 1.0 { over1 += 1 } if exp > 0.001 { let rel = ((got - exp) / exp).abs(); if rel > worst { worst = rel; worst_x = x; } } }
            let bands: Vec<String> = [0.5081f64, 0.7518, 0.9026].iter().enumerate().map(|(i, code)| { let y = (h / 2 + (h / 6) * i as u32 + h / 12) as usize; let got = px[(y * w as usize + w as usize / 2) * 4] as f64; format!("band {code:.4} → expect {:.4} ({:.0} nit) got {:.4} ({:.0} nit)", pq(*code) / white, pq(*code), got, got * white) }).collect();
            println!("verify-pq (row {row}, {} nits = 1.0): worst relative error {:.3} % at x={worst_x} (code {:.3}); {} of {} samples above 1.0 (EDR range)\n  {}", o.white, worst * 100.0, worst_x as f64 / (w - 1) as f64, over1, w, bands.join("\n  "));
        }
    }
    // the FrameIndex report: tier 2 vs tier 1 on every frame, the keyframe table, the read-ahead pages touched
    if let Some(idx) = p.decoder().frame_index() {
        let t1 = Instant::now(); while idx.scan_state() == qcv_player::frame_index::ScanState::Building && t1.elapsed().as_secs() < 30 { std::thread::sleep(std::time::Duration::from_millis(20)); }
        let n = idx.total_frames();
        let (mut mism, mut worst) = (0, 0i64);
        if idx.has_exact_mapping() {
            let f1 = qcv_player::FrameIndex::new(idx.time_base().0, idx.time_base().1, idx.frame_rate().0, idx.frame_rate().1, n, idx.is_intra_only());
            for k in 0..n { let d = (idx.pts_for_frame(k) - f1.pts_for_frame(k)).abs(); if d != 0 { mism += 1; worst = worst.max(d); } }
        }
        println!("frame index: tier 2 {:?} ({} frames built), keyframes {}, tier-1 formula disagrees on {mism} of {n} frames (worst {worst} ticks); keyframe before frame {} = pts {}; read-ahead pages touched {}",
            idx.scan_state(), idx.scan_frames_built(), idx.keyframe_count(), n / 2, idx.keyframe_pts_before(n / 2), qcv_player::read_ahead::ReadAhead::instance().touched_pages.load(std::sync::atomic::Ordering::Relaxed));
    }
    if let Some(sq) = p.decoder().as_sequence() { println!("sequence: {} of the frames shown arrived uploaded ahead (GPU upload thread)", sq.gpu_uploads()); }
    let s = &mut p.stats;
    println!("headless: {} frames shown (open → first frame on the engine {:.0} ms), show ms p50 {:.2} max {:.2}, decode ms p50 {:.1} max {:.1}, dropped {}; CPU {:.0} % of one core; max RSS {:.0} MB",
        s.shown, first.unwrap_or(f32::NAN), pct(&mut show_ms, 50.0), pct(&mut show_ms, 100.0), pct(&mut s.decode_ms, 50.0), pct(&mut s.decode_ms, 100.0), s.dropped, 100.0 * qcv_player::process_cpu_seconds() / t.elapsed().as_secs_f64(), qcv_player::max_rss_mb());
}

/// `--seeks N` / `--scrub a-b:ms` (S2's battery, now against the PORTED scrub decoder): the streaming decoder opens the
/// file and pauses; the scrub decoder opens its own session and publishes into the stream's slot; the headless engine
/// shows what lands. Seeks: N random targets, one at a time (request → served → shown). Scrub: a pointer sweeping a → b,
/// one request per refresh interval for 2 s, the slot drained each refresh (request → ready, hits, frames behind the
/// pointer). `--scrub-hw` lifts the intra-only hwaccel gate (QCView's `performance/hwScrubInterCodecs`; the default on Windows).
fn run_scrub_bench(o: Opts) {
    use min_present::engine::{DisplayMode, Engine, OutMode};
    use qcv_player::ScrubOptions;
    let Some(file) = o.file.clone() else { println!("--seeks / --scrub need a file"); return };
    let mut engine = Engine::headless(wgpu::TextureFormat::Rgba16Float).expect("headless engine");
    engine.set_display(DisplayMode { out: OutMode::Linear, white: o.white });
    #[cfg(windows)] let dev = engine.dx12_device_raw();
    #[cfg(not(windows))] let dev = None;
    let hw = if o.sw { None } else { HwDevice::platform(dev) }; let hw_intra = if o.sw { None } else { HwDevice::vulkan(dev) };
    let src = Source::from_path(&file, None);
    let t_open = Instant::now();
    let sopts = ScrubOptions { hw: hw.clone(), hw_intra: hw_intra.clone(), hw_inter_scrub: o.scrub_hw || cfg!(windows), stats: true, threads: std::env::var("QCV_SCRUB_THREADS").ok().and_then(|v| v.parse().ok()).unwrap_or(0), budget_bytes: std::env::var("QCV_SCRUB_CACHE_MB").ok().and_then(|v| v.parse::<usize>().ok()).unwrap_or(512) << 20, ..Default::default() };
    let seq = qcv_player::image_sequence_cache::StreamingConfig::from_settings(std::env::var("QCV_SEQ_THREADS").ok().and_then(|v| v.parse().ok()).unwrap_or(16), std::env::var("QCV_SEQ_AHEAD").ok().and_then(|v| v.parse().ok()).unwrap_or(72));
    let mut p = match Transport::open(&src, TransportOptions { hw: hw.clone(), hw_intra: hw_intra.clone(), threads: o.threads, loop_enabled: false, scrub: Some(sopts), seq, ..Default::default() }) { Ok(p) => p, Err(e) => { println!("open {file}: {e}"); return } };
    let index = p.frame_index().clone();
    let total = index.total_frames();
    if p.decoder().as_sequence().is_some() { return run_sequence_bench(o, p, &mut engine, total, t_open) }   // the engine stays here: the hw devices built on its D3D12 device must drop first
    let (hw_active, intra_direct, served) = match p.scrub() { Some(sd) => (sd.hw_active, sd.intra_direct, sd.served.clone().unwrap()), None => { println!("no scrub decoder"); return } };
    println!("opened {} — {}; scrub decoder: {} (intra-direct {}), transport opened in {:.0} ms", src.display(), describe(&p), if hw_active { p.scrub().map(|sd| sd.hw_name()).unwrap_or("hw") } else { "software" }, intra_direct, t_open.elapsed().as_secs_f32() * 1000.0);
    // the open's own first frame lands through the transport's seek gate (paused at open)
    { let t = Instant::now(); while p.seek_in_flight() && t.elapsed().as_secs() < 10 { if let Some(h) = p.tick(Instant::now()) { engine.show(&h.frame).ok(); } else { std::thread::sleep(std::time::Duration::from_micros(200)); } } }
    if let Some(ms) = std::env::var("QCV_SCRUB_SETTLE_MS").ok().and_then(|v| v.parse::<u64>().ok()) { std::thread::sleep(std::time::Duration::from_millis(ms)); }   // measurement: let the index scan / warmer finish first
    // the first frame through the scrub decoder (the H.264 open carried from step 1)
    let t0 = Instant::now(); p.scrub().unwrap().request_frame(0);
    match served.recv_timeout(std::time::Duration::from_secs(10)) { Ok(s) => { let h = p.decoder().fetch_latest(); if let Some(h) = &h { engine.show(&h.frame).ok(); } println!("first frame via the scrub decoder: ready {:.1} ms, shown {:.1} ms (decoded {}, seeked {})", (s.done - s.issued).as_secs_f32() * 1000.0, t0.elapsed().as_secs_f32() * 1000.0, s.decoded, s.seeked); } Err(_) => { println!("first frame: no answer in 10 s"); return } }
    let mut rng = 0x9E3779B97F4A7C15u64; let mut next = || { rng ^= rng << 13; rng ^= rng >> 7; rng ^= rng << 17; rng };
    if let Some(n) = o.seeks {
        let (mut ready, mut shown, mut fwd, mut back, mut hits) = (vec![], vec![], vec![], vec![], 0u32);
        let mut last = 0i32;
        for _ in 0..n {
            let target = (next() % total.max(1) as u64) as i32;
            if target == last { continue }
            if o.seek_gap_ms > 0 { std::thread::sleep(std::time::Duration::from_millis(o.seek_gap_ms)); }   // a user's seeks are spaced; the decoder's in-flight frame threads drain between them
            let t = Instant::now();
            p.seek_to_frame(target);                            // the stream's own seek (QCView's seekToFrame)
            p.scrub().unwrap().request_frame(target);    // the scrub decoder answers first
            let Ok(s) = served.recv_timeout(std::time::Duration::from_secs(10)) else { println!("seek {target}: no answer in 10 s"); break };
            let r = (s.done - s.issued).as_secs_f32() * 1000.0;
            let mut sh = f32::NAN;
            while p.seek_in_flight() && t.elapsed().as_secs() < 10 { if let Some(h) = p.tick(Instant::now()) { engine.show(&h.frame).ok(); if h.frame_no < 0 && sh.is_nan() { sh = t.elapsed().as_secs_f32() * 1000.0; } } else { std::thread::sleep(std::time::Duration::from_micros(200)); } }
            if let Some(h) = p.current() { if h.frame_no >= 0 && h.frame_no != target { println!("  MISMATCH: asked {target} got {}", h.frame_no); } }
            if sh.is_nan() { sh = t.elapsed().as_secs_f32() * 1000.0; }
            if std::env::var_os("QCV_SCRUB_ROWS").is_some() { println!("  seek → {target}: ready {r:.1} ms, decoded {} (keyframe before = frame {}), seeked {}, hit {}", s.decoded, index.frame_for_pts(index.keyframe_pts_before(target)), s.seeked, s.hit); }
            ready.push(r); shown.push(sh); if s.hit { hits += 1 } if target > last { fwd.push(r) } else { back.push(r) }
            last = target;
        }
        let land = &mut p.stats.seek_land_ms;
        println!("seeks ({} random, {} frames): scrub request → ready ms p50 {:.1} p95 {:.1} max {:.1}; → shown p50 {:.1} p95 {:.1}; forward p50 {:.1} p95 {:.1} (n={}), backward p50 {:.1} p95 {:.1} (n={}); cache hits {hits}; the stream's own seek landed ms p50 {:.1} p95 {:.1} max {:.1}",
            ready.len(), total, pct(&mut ready, 50.0), pct(&mut ready, 95.0), pct(&mut ready, 100.0), pct(&mut shown, 50.0), pct(&mut shown, 95.0), pct(&mut fwd, 50.0), pct(&mut fwd, 95.0), fwd.len(), pct(&mut back, 50.0), pct(&mut back, 95.0), back.len(), pct(land, 50.0), pct(land, 95.0), pct(land, 100.0));
    }
    if let Some((a, b, ms)) = o.scrub {
        let (a, b) = (a.clamp(0, total - 1), b.clamp(0, total - 1));
        if (b - a).abs() < 10 { println!("scrub: the clip has {total} frames — the sweep {a}→{b} is too short to measure"); }
        // position on the sweep's start first (a user's drag begins where the playhead is); the sweep measures the drag, not the jump to it
        p.begin_scrub(); p.scrub_to_frame(a); while let Ok(s) = served.recv_timeout(std::time::Duration::from_secs(10)) { if s.target == a { break } }
        if let Some(h) = p.tick(Instant::now()) { engine.show(&h.frame).ok(); }
        let period = std::time::Duration::from_secs_f32(ms / 1000.0);
        let sweep = std::time::Duration::from_secs(2); let t = Instant::now();
        let (mut ready, mut behind, mut reqs, mut hits, mut shown_n, mut retargets) = (vec![], vec![], 0u32, 0u32, 0u32, 0u32);
        let mut stream_shown = 0u32;   // frames the STREAM decoder slipped onto the screen mid-drag: must be 0 (the 2026-11-07 "plays toward the pointer" bug)
        let mut tick = t;
        while t.elapsed() < sweep {
            let frac = (t.elapsed().as_secs_f32() / sweep.as_secs_f32()).min(1.0);
            let pointer = a + ((b - a) as f32 * frac).round() as i32;
            p.scrub_to_frame(pointer); reqs += 1;
            tick += period; let now = Instant::now(); if tick > now { std::thread::sleep(tick - now); }
            while let Ok(s) = served.try_recv() { ready.push((s.done - s.issued).as_secs_f32() * 1000.0); if s.hit { hits += 1 } if s.retargeted { retargets += 1 } }
            if let Some(h) = p.tick(Instant::now()) { engine.show(&h.frame).ok(); shown_n += 1; if h.frame_no >= 0 { stream_shown += 1; } }
            behind.push((pointer - p.scrub().unwrap().last_shown()).abs() as f32);
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
        while let Ok(s) = served.try_recv() { ready.push((s.done - s.issued).as_secs_f32() * 1000.0); if s.hit { hits += 1 } }
        println!("scrub {a}→{b} @ {ms:.1} ms: {reqs} requests → {} served ({hits} cache hits, {retargets} retargets), {shown_n} shown ({stream_shown} of them STREAM frames — must be 0); request → ready ms p50 {:.1} p95 {:.1} max {:.1}; frames behind the pointer p50 {:.0} p95 {:.0} max {:.0}",
            ready.len(), pct(&mut ready, 50.0), pct(&mut ready, 95.0), pct(&mut ready, 100.0), pct(&mut behind, 50.0), pct(&mut behind, 95.0), pct(&mut behind, 100.0));
        if stream_shown > 0 { println!("FAIL: {stream_shown} stream frames reached the screen during the scrub — the streaming decoder was not paused"); }
    }
    println!("CPU {:.0} % of one core over the run; max RSS {:.0} MB", 100.0 * qcv_player::process_cpu_seconds() / t_open.elapsed().as_secs_f64(), qcv_player::max_rss_mb());
    p.end_scrub();
}

/// `--seeks` / `--scrub` on an image sequence (step 2c): the cache has no scrub decoder — a seek is `updatePlayhead(f, force)`
/// and lands when the frame is cached; a drag moves the playhead without forcing. `QCV_SEQ_AHEAD` shrinks the window
/// (24 = QCView's minimum) so random seeks leave it.
fn run_sequence_bench(o: Opts, mut p: Transport, engine: &mut min_present::engine::Engine, total: i32, t_open: Instant) {
    let c = p.decoder().as_sequence().unwrap().cache();
    println!("opened {} — sequence cache: {} IO threads, window {} ahead / {} behind; transport opened in {:.0} ms", describe(&p), c.config().threads, c.config().read_ahead, c.config().read_behind, t_open.elapsed().as_secs_f32() * 1000.0);
    let land = |p: &mut Transport, engine: &mut min_present::engine::Engine| { let t = Instant::now(); while p.seek_in_flight() && t.elapsed().as_secs() < 10 { if let Some(h) = p.tick(Instant::now()) { engine.show(&h.frame).ok(); } else { std::thread::sleep(std::time::Duration::from_micros(200)); } } t.elapsed().as_secs_f32() * 1000.0 };
    println!("first frame landed in {:.1} ms", land(&mut p, engine));
    let mut rng = 0x9E3779B97F4A7C15u64; let mut next = || { rng ^= rng << 13; rng ^= rng >> 7; rng ^= rng << 17; rng };
    if let Some(n) = o.seeks {
        let (mut cold, mut warm) = (vec![], vec![]);
        for _ in 0..n {
            let target = (next() % total.max(1) as u64) as i32;
            let cached = p.decoder().as_sequence().unwrap().cache().frame(target).is_some();
            if o.seek_gap_ms > 0 { std::thread::sleep(std::time::Duration::from_millis(o.seek_gap_ms)); }
            p.seek_to_frame(target);
            let ms = land(&mut p, engine);
            if let Some(h) = p.current() && h.frame_no != target { println!("  MISMATCH: asked {target} got {}", h.frame_no); }
            if cached { warm.push(ms) } else { cold.push(ms) }
        }
        println!("seeks ({} random, {total} frames): cold n={} landed ms p50 {:.1} p95 {:.1} max {:.1}; cached n={} p50 {:.2} max {:.2}", cold.len() + warm.len(), cold.len(), pct(&mut cold, 50.0), pct(&mut cold, 95.0), pct(&mut cold, 100.0), warm.len(), pct(&mut warm, 50.0), pct(&mut warm, 100.0));
    }
    if let Some((a, b, ms)) = o.scrub {
        let (a, b) = (a.clamp(0, total - 1), b.clamp(0, total - 1));
        p.seek_to_frame(a); land(&mut p, engine);
        p.begin_scrub();
        let period = std::time::Duration::from_secs_f32(ms / 1000.0);
        let sweep = std::time::Duration::from_secs(2); let t = Instant::now();
        let (mut behind, mut reqs, mut shown) = (vec![], 0u32, 0u32);
        let mut tick = t;
        while t.elapsed() < sweep {
            let frac = (t.elapsed().as_secs_f32() / sweep.as_secs_f32()).min(1.0);
            let pointer = a + ((b - a) as f32 * frac).round() as i32;
            p.scrub_to_frame(pointer); reqs += 1;
            tick += period; let now = Instant::now(); if tick > now { std::thread::sleep(tick - now); }
            if let Some(h) = p.tick(Instant::now()) { engine.show(&h.frame).ok(); shown += 1; }
            behind.push((pointer - p.current().map(|h| h.frame_no).unwrap_or(a)).abs() as f32);
        }
        p.end_scrub(); land(&mut p, engine);
        println!("scrub {a}→{b} @ {ms:.1} ms: {reqs} moves, {shown} frames shown; frames behind the pointer p50 {:.0} p95 {:.0} max {:.0}; landed on {:?}", pct(&mut behind, 50.0), pct(&mut behind, 95.0), pct(&mut behind, 100.0), p.current().map(|h| h.frame_no));
    }
    println!("CPU {:.0} % of one core over the run; max RSS {:.0} MB", 100.0 * qcv_player::process_cpu_seconds() / t_open.elapsed().as_secs_f64(), qcv_player::max_rss_mb());
}

/// `--hover N`: the hover-thumbnail cache (QCView's `TimelineThumbnailCache` + `VideoImageLoader`) — N random hover frames
/// at the panel's 16 ms request throttle, the drain every 33 ms (textures made on the headless engine); then the same
/// frames again (hits), then a prefetch of the whole file at stride 6. The bar: a hover answers within one refresh once
/// cached, and a miss lands in the time one thumbnail decode takes.
fn cache_size_for(_c: &qcv_player::TimelineThumbnailCache, qcview: bool) -> usize { if qcview { qcv_player::thumbnail_cache::QCVIEW_CACHE_SIZE } else { qcv_player::ThumbConfig::default().cache_size } }
fn run_hover_bench(o: Opts) {
    use min_present::engine::Engine;
    use qcv_player::{TimelineThumbnailCache, ThumbConfig, VideoThumbLoader, thumbnail_cache::{HOVER_REQUEST_THROTTLE_MS, UPLOAD_DRAIN_MS}};
    let n = o.hover.unwrap_or(20);
    let Some(file) = o.file.clone() else { println!("--hover needs a file"); return };
    let engine = Engine::headless(wgpu::TextureFormat::Rgba16Float).expect("headless engine");
    let src = Source::from_path(&file, None);
    let mut p = match Transport::open(&src, TransportOptions { hw: None, threads: o.threads, loop_enabled: false, ..Default::default() }) { Ok(p) => p, Err(e) => { println!("open {file}: {e}"); return } };
    let total = p.frame_count();
    let qcview_budget = std::env::var_os("QCV_HOVER_QCVIEW").is_some();   // QCView's 100-entry budget, no strip, no peek: the comparison run
    println!("opened {} — {}", src.display(), describe(&p));
    let textures: std::rc::Rc<std::cell::RefCell<std::collections::HashMap<u64, wgpu::Texture>>> = Default::default();
    let released = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let (tx, rl) = (textures.clone(), released.clone());
    let mut cache = TimelineThumbnailCache::new(ThumbConfig { cache_size: if qcview_budget { qcv_player::thumbnail_cache::QCVIEW_CACHE_SIZE } else { ThumbConfig::default().cache_size }, ..Default::default() }, VideoThumbLoader::any_factory(), Box::new(move |h| { tx.borrow_mut().remove(&h); rl.set(rl.get() + 1); }));
    let mut next_handle = 0u64;
    let mut adopt = |key: &qcv_player::ThumbKey, px: &qcv_player::ThumbPixels| -> u64 {
        let _ = key;
        let tex = engine.device.create_texture(&wgpu::TextureDescriptor { label: Some("thumb"), size: wgpu::Extent3d { width: px.width, height: px.height, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba8Unorm, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] });
        engine.queue.write_texture(wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, &px.rgba8(), wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(px.width * 4), rows_per_image: Some(px.height) }, wgpu::Extent3d { width: px.width, height: px.height, depth_or_array_layers: 1 });
        engine.queue.submit(std::iter::empty()); let _ = engine.device.poll(wgpu::PollType::Poll);
        next_handle += 1; textures.borrow_mut().insert(next_handle, tex); next_handle
    };
    let mut rng = 0x9E3779B97F4A7C15u64; let mut next = || { rng ^= rng << 13; rng ^= rng >> 7; rng ^= rng << 17; rng };
    let frames: Vec<i32> = (0..n).map(|_| (next() % total.max(1) as u64) as i32).collect();
    let (mut land_ms, mut sizes) = (vec![], (0u32, 0u32));
    let t_all = Instant::now();
    for &f in &frames {
        let t = Instant::now();
        let e = cache.get_thumbnail(&file, f, true);
        if e.texture != 0 && cache.stats().total_cached > 0 && land_ms.is_empty() { /* a fallback answered */ }
        let mut landed = e.texture != 0 && false;
        let mut next_drain = t;
        while !landed && t.elapsed().as_secs() < 10 {
            if Instant::now() >= next_drain { cache.process_pending_uploads(&mut adopt); next_drain += std::time::Duration::from_millis(UPLOAD_DRAIN_MS); }
            let e = cache.get_thumbnail(&file, f, false);
            if e.texture != 0 { landed = true; sizes = (e.width, e.height); land_ms.push(t.elapsed().as_secs_f32() * 1000.0); }
            else { std::thread::sleep(std::time::Duration::from_millis(1)); }
        }
        std::thread::sleep(std::time::Duration::from_millis(HOVER_REQUEST_THROTTLE_MS));
    }
    let fill = t_all.elapsed().as_secs_f32();
    let mut hit_ms = vec![];
    for &f in &frames { let t = Instant::now(); let e = cache.get_thumbnail(&file, f, true); hit_ms.push(t.elapsed().as_secs_f32() * 1000.0); if e.texture == 0 { println!("  miss on a re-hover of frame {f}"); } }
    let s = cache.stats();
    println!("hover: {} random frames: miss → thumbnail landed ms p50 {:.1} p95 {:.1} max {:.1} (thumb {}x{}); re-hover hit ms p50 {:.3} max {:.3}; cached {} of {}; hits {} misses {}; the walk took {:.1} s",
        frames.len(), pct(&mut land_ms, 50.0), pct(&mut land_ms, 95.0), pct(&mut land_ms, 100.0), sizes.0, sizes.1, pct(&mut hit_ms, 50.0), pct(&mut hit_ms, 100.0), s.total_cached, cache_size_for(&cache, qcview_budget), s.cache_hits, s.cache_misses, fill);
    // the strip on open (frames / 400, floor 6), then a MOUSE SWEEP: the hover crosses the whole timeline in 2 s at the
    // 16 ms throttle; every position asks for its thumbnail through the Transport (GOP-cache peek → cache → nearest cached)
    cache.clear();
    let t = Instant::now();
    if !qcview_budget { cache.precache_strip(&file, total, 0); }
    let wanted = if qcview_budget { 0 } else { ((total as i64 + (total as i64 / 400).max(6) - 1) / (total as i64 / 400).max(6)) as usize };
    let mut landed = 0usize;
    while !qcview_budget && cache.stats().pending_requests > 0 && t.elapsed().as_secs() < 180 { landed += cache.process_pending_uploads(&mut adopt); std::thread::sleep(std::time::Duration::from_millis(UPLOAD_DRAIN_MS)); }
    landed += cache.process_pending_uploads(&mut adopt);
    println!("strip: {landed} of {wanted} thumbnails in {:.1} s ({:.0} ms each across 3 workers); cache holds {} of {}", t.elapsed().as_secs_f32(), t.elapsed().as_secs_f32() * 1000.0 / landed.max(1) as f32, cache.stats().total_cached, cache_size_for(&cache, qcview_budget));
    let _ = &p;
    let (mut answered, mut exact, mut nothing, mut off_by, mut last_f) = (0u32, 0u32, 0u32, vec![], 0i32);
    let t = Instant::now(); let sweep = std::time::Duration::from_secs(2); let mut next_drain = t;
    while t.elapsed() < sweep {
        let frac = t.elapsed().as_secs_f32() / sweep.as_secs_f32(); let f = ((total - 1) as f32 * frac) as i32; last_f = f;
        if Instant::now() >= next_drain { cache.process_pending_uploads(&mut adopt); next_drain += std::time::Duration::from_millis(UPLOAD_DRAIN_MS); }
        let (e, peeked) = p.hover_thumbnail(&mut cache, f, 320);
        if peeked { cache.process_pending_uploads(&mut adopt); }
        let (e, _) = if peeked { p.hover_thumbnail(&mut cache, f, 320) } else { (e, false) };
        if e.texture != 0 { answered += 1; if cache.contains(&file, f) { exact += 1; } else { off_by.push(cache.nearest_cached_distance(&file, f).unwrap_or(0) as f32); } } else { nothing += 1; }
        std::thread::sleep(std::time::Duration::from_millis(HOVER_REQUEST_THROTTLE_MS));
    }
    // the mouse rests on the last position: how long until the exact frame lands?
    let t2 = Instant::now(); let mut rest_ms = f32::NAN;
    while t2.elapsed().as_secs() < 10 { cache.process_pending_uploads(&mut adopt); if cache.contains(&file, last_f) { rest_ms = t2.elapsed().as_secs_f32() * 1000.0; break } std::thread::sleep(std::time::Duration::from_millis(1)); }
    let s = cache.stats();
    println!("sweep: {} hovers over 2 s — answered with a picture {answered} (exact {exact}, nearest cached {} at p50 {:.0} / p95 {:.0} / max {:.0} frames away), nothing {nothing}; the exact frame landed {rest_ms:.0} ms after the mouse rested; backlog left {} requests; cache holds {} textures",
        answered + nothing, answered - exact, pct(&mut off_by, 50.0), pct(&mut off_by, 95.0), pct(&mut off_by, 100.0), s.pending_requests, s.total_cached);
    // a drag across frames 100–190 (the scrub decoder's GOP cache fills along it on inter codecs), then hovers over the same
    // span: the GOP-cache peek answers exactly and at once where the cache holds the frame
    if total > 200 {
        p.begin_scrub(); let t = Instant::now(); let served = p.scrub().and_then(|s| s.served.clone());
        for f in (100..190).step_by(2) { p.scrub_to_frame(f); if let Some(r) = &served { let _ = r.recv_timeout(std::time::Duration::from_millis(500)); } let _ = p.tick(Instant::now()); }
        p.end_scrub(); while p.seek_in_flight() && t.elapsed().as_secs() < 5 { let _ = p.tick(Instant::now()); std::thread::sleep(std::time::Duration::from_micros(200)); }
        let (mut peek_ms, mut exact_peek, mut via_loader) = (vec![], 0u32, 0u32);
        for f in (101..189).step_by(3) {
            let t = Instant::now(); let (e, peeked) = p.hover_thumbnail(&mut cache, f, 320);
            if peeked { cache.process_pending_uploads(&mut adopt); let (e2, _) = p.hover_thumbnail(&mut cache, f, 320); if e2.texture != 0 && cache.contains(&file, f) { exact_peek += 1; peek_ms.push(t.elapsed().as_secs_f32() * 1000.0); } }
            else { via_loader += 1; let _ = e; }
        }
        println!("after a drag over 100–190: {} hovers on that span — exact through the GOP-cache peek {exact_peek} (p50 {:.1} ms, max {:.1}), to the loader {via_loader} ({})", exact_peek + via_loader, pct(&mut peek_ms, 50.0), pct(&mut peek_ms, 100.0), if p.scrub().map(|s| s.intra_direct).unwrap_or(true) { "an intra codec: the scrub decoder caches nothing, by design" } else { "frames the GOP cache did not hold" });
    }
    // playing: one worker
    cache.notify_playback_state(true);
    let t = Instant::now(); cache.precache_range(&file, 0, (total - 1).min(120), 0, 1);
    while cache.stats().pending_requests > 0 && t.elapsed().as_secs() < 60 { cache.process_pending_uploads(&mut adopt); std::thread::sleep(std::time::Duration::from_millis(UPLOAD_DRAIN_MS)); }
    println!("playing (one worker): 121 frames at stride 1 in {:.1} s; peak RSS {:.0} MB", t.elapsed().as_secs_f32(), qcv_player::max_rss_mb());
    cache.shutdown();
}

/// `--cut a.mov@0-48,b.mp4@100-148[,…]`: a playlist through the Transport's two slots, played headless at a 60 Hz refresh
/// cadence from the start to the end; the cut's evidence is the cadence / black / drop accounting around every
/// boundary (S2's bar: no black refresh, no duplicate frame at the cut).
fn run_cut_bench(o: Opts) {
    use min_present::engine::Engine;
    let spec = o.cut.clone().unwrap();
    let mut specs = vec![];
    for part in spec.split(',') {
        let (path, trim) = match part.rsplit_once('@') { Some((p, r)) if r.contains('-') => { let (a, b) = r.split_once('-').unwrap(); (p.to_string(), Some((a.parse::<i64>().unwrap_or(0), b.parse::<i64>().unwrap_or(0)))) } _ => (part.to_string(), None) };
        specs.push((path, trim));
    }
    let mut engine = Engine::headless(wgpu::TextureFormat::Rgba16Float).expect("headless engine");
    #[cfg(windows)] let dev = engine.dx12_device_raw();
    #[cfg(not(windows))] let dev = None;
    let hw = if o.sw { None } else { HwDevice::platform(dev) }; let hw_intra = if o.sw { None } else { HwDevice::vulkan(dev) };
    let tl = match qcv_player::transport::playlist_from_specs(&specs) { Ok(t) => t, Err(e) => { println!("cut: {e}"); return } };
    let boundaries = tl.boundaries(); let total = tl.duration;
    let t0 = Instant::now();
    let mut p = match Transport::open_playlist(tl, TransportOptions { hw, hw_intra, threads: o.threads, loop_enabled: o.looping, scrub: None, ..Default::default() }) { Ok(p) => p, Err(e) => { println!("open playlist: {e}"); return } };
    println!("playlist: {} clips, {} timeline frames at {} fps, boundaries at {:?}; opened in {:.0} ms", specs.len(), total, p.rate(), boundaries, t0.elapsed().as_secs_f32() * 1000.0);
    // let the first frame land, then play at a 60 Hz refresh cadence
    { let t = Instant::now(); while p.seek_in_flight() && t.elapsed().as_secs() < 10 { if let Some(h) = p.tick(Instant::now()) { engine.show(&h.frame).ok(); } else { std::thread::sleep(std::time::Duration::from_micros(200)); } } }
    p.play();
    let period = std::time::Duration::from_secs_f32(o.refresh_ms / 1000.0);
    let mut next = Instant::now(); let mut shown_at: Vec<(i64, u32)> = vec![];   // (timeline frame shown, refresh index)
    let mut refresh = 0u32; let deadline = Instant::now() + std::time::Duration::from_secs(120);
    let loops_wanted = if o.looping { 1 } else { 0 };
    let (mut last_loops, mut wrap_at, mut wraps) = (0u32, None::<(u32, i64)>, vec![]);
    while !p.at_end() && Instant::now() < deadline && p.stats.loops <= loops_wanted {
        let now = Instant::now();
        match p.tick(now) { Some(h) => { engine.show(&h.frame).ok(); shown_at.push((p.current_frame() as i64, refresh)); if let Some((r0, f0)) = wrap_at.take() { wraps.push(format!("wrap {}: frame {f0} at refresh {r0} → frame {} at refresh {refresh} ({} refreshes; the landing {:.1} ms)", p.stats.loops, p.current_frame(), refresh - r0, p.stats.seek_land_ms.last().copied().unwrap_or(f32::NAN))); } } None => p.idle_refresh() }
        if p.stats.loops != last_loops { last_loops = p.stats.loops; wrap_at = Some((refresh, shown_at.last().map(|x| x.0).unwrap_or(-1))); }
        if o.looping && p.stats.loops > loops_wanted { break }
        refresh += 1; next += period; let n2 = Instant::now(); if next > n2 { std::thread::sleep(next - n2); }
    }
    let s = &mut p.stats;
    println!("cut: shown {} frames over {refresh} refreshes, dropped {}, late {}, black refreshes {}, loops {}; lateness ms p50 {:.1} p95 {:.1} max {:.1}; cadence {:?}; swaps prewarmed {} / opened {}; CPU {:.0} %; RSS {:.0} MB",
        s.shown, s.dropped, s.late, s.black, s.loops, pct(&mut s.lateness_ms, 50.0), pct(&mut s.lateness_ms, 95.0), pct(&mut s.lateness_ms, 100.0), s.cadence, s.swaps_prewarmed, s.swaps_opened, 100.0 * qcv_player::process_cpu_seconds() / t0.elapsed().as_secs_f64(), qcv_player::max_rss_mb());
    // around every boundary: the refresh gaps between the frames shown just before and after the cut
    for w in &wraps { println!("  {w}"); }
    // random seeks across the playlist through the 50 ms debounce (applyPlaylistSeek: clip resolve + swap + the stream's landing)
    if !o.looping && total > 1 {
        p.pause();
        let mut rng = 0x9E3779B97F4A7C15u64; let mut next = || { rng ^= rng << 13; rng ^= rng >> 7; rng ^= rng << 17; rng };
        let (mut land, mut cross) = (vec![], 0u32);
        for _ in 0..12 {
            let target = (next() % total as u64) as i32; let from_clip = p.active_clip_index();
            let t = Instant::now(); p.seek_to_frame(target);
            while (p.seek_in_flight() || t.elapsed().as_millis() < 60) && t.elapsed().as_secs() < 10 { if let Some(h) = p.tick(Instant::now()) { engine.show(&h.frame).ok(); } else { std::thread::sleep(std::time::Duration::from_micros(200)); } }
            let landed = t.elapsed().as_secs_f32() * 1000.0; land.push(landed); if p.active_clip_index() != from_clip { cross += 1; }
            if p.current_frame() != target { println!("  seek → {target}: landed on timeline frame {} (clip {})", p.current_frame(), p.active_clip_index()); }
        }
        let s = &p.stats;
        println!("playlist seeks: 12 random, {cross} cross-clip; request → landed ms (incl. the 50 ms debounce) p50 {:.1} p95 {:.1} max {:.1}; swaps prewarmed {} / opened {}", pct(&mut land, 50.0), pct(&mut land, 95.0), pct(&mut land, 100.0), s.swaps_prewarmed, s.swaps_opened);
    }
    for b in &boundaries {
        let after = shown_at.iter().find(|(f, _)| *f >= *b); let before = after.and_then(|(_, ra)| shown_at.iter().rev().find(|(f, r)| *f < *b && r < ra));
        match (before, after) { (Some((fb, rb)), Some((fa, ra))) => println!("  boundary {b}: frame {fb} at refresh {rb} → frame {fa} at refresh {ra} ({} refreshes apart; a 2/3 cadence = gapless, no duplicate)", ra - rb), _ => println!("  boundary {b}: not crossed") }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_dual_clip_survives_qcviews_clip_map() {
        // QCView writes seconds; a 24-fps clip on a 30-fps master: start 45 (1.5 s), duration 60, source 36..84 at 24
        let c = qcv_timeline::Clip { id: "x".into(), name: "b".into(), start: 45, duration: 60, source_in: 36, source_out: 84, media_path: "/m/b.mov".into(), media_item_id: "m".into(), source_rate: qcv_timeline::Rate::new(24, 1), source_duration: 240, ..qcv_timeline::Clip::empty() };
        let v = clip_to_qcview(&c, qcv_timeline::Rate::new(30, 1));
        assert_eq!(v["startTime"].as_f64(), Some(1.5)); assert_eq!(v["sourceIn"].as_f64(), Some(1.5)); assert_eq!(v["sourceFps"].as_f64(), Some(24.0));
        let back = clip_from_qcview(&v, qcv_timeline::Rate::new(30, 1)).unwrap();
        assert_eq!((back.start, back.duration, back.source_in, back.source_out, back.source_duration, back.source_rate), (45, 60, 36, 84, 240, qcv_timeline::Rate::new(24, 1)));
    }
}
