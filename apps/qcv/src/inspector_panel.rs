//! The Inspector in the right rail — QCView's `InspectorPanel.qml` carried row for row (the user 2026-11-07: "ensure we
//! are faithfully porting over what we had for videos, some of those fields were specific user requests"): File ·
//! Video · Geometry · Color · Audio · Timecode, the labels verbatim, the notes beside the values (QCView's `KvRow.note`),
//! the HDR row in the warn colour. The metadata is `qcv-meta`'s extractor + XMP reader (the same code QCView runs),
//! delivered by its service when the file opens; the decoder's facts fill in until then. Three of QCView's pickers are
//! here because the player backs them: Color ▸ Range (Auto / Full / Limited → the decoders' override), Audio ▸ Mix
//! (Auto / 5.1 → 2 / 7-8 → the routing matrix), Timecode ▸ Origin (From start / Embedded / QT Start / QT TC / XMP Alt →
//! the strip's base). Geometry's Aspect + Rotation pickers and Color's Transfer picker wait for their engine stages
//! (written in the Owed list). Linked Projects / Playlist Contents arrive with the project crate.
use kurbo::Rect;
use min_theme::tokens as t;
use min_ui::{paint, ActionButton, ActionGroup, Event, Family, FieldResponse, Fonts, ImeEvent, KeyEvent, Response, Section, TextField};
use qcv_meta::{AdobeMetadata, VideoMetadata};
use min_present::engine::{Geometry, Par};
use qcv_player::{audio::RoutingMode, RangeOverride, Transport};
use qcv_project::TransferOverride;
use std::time::Instant;
use min_i18n::fill as tf;
use vello::Scene;

#[derive(Clone, Debug, PartialEq)]
pub enum Cmd { /// the image-sequence card's chips (QCView `ImageSequenceInspector`)
    FrameRate(f64), Stride(i32), /// an EXR layer tile
    Layer(String), /// the path actions (QCView's File card + Linked Projects): copy the text, reveal / open the path
    Copy(String), Reveal(String), Open(String), Range(RangeOverride), Transfer(TransferOverride), Routing(RoutingMode), Origin(String), Par(Par), Rotation(Option<u16>) }

struct Row { label: String, value: String, note: String, mono: bool, warn: bool }
/// QCView's `KvChipRow`: a label and a fused row of choices, one selected.
struct Chips { /// the English label: what a click dispatches on, whatever the language
    id: String, label: String, group: ActionGroup, keys: Vec<String>, selected: usize, rect: Rect, /// the fused rows' rects (one per wrapped line) for the backing
    rows: Vec<Rect> }
/// QCView's Custom aspect fields: Pixel aspect (a scalar, clamped 0.05–20, rationalised at 1e4) and Display aspect (a single
/// decimal ": 1"; PAR = DAR × storageH / storageW, rationalised at 1e3) — either round-trips to the same ratio.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FieldKind { Par, Dar }
struct Field { label: String, kind: FieldKind, field: TextField, suffix: String, rect: Rect }
/// QCView's EXR layer grid: a tile per layer (the first frame at that layer + the name), two columns.
struct Layers { names: Vec<String>, current: String, tiles: Vec<Rect>, aspect: f64, hover: Option<usize> }
/// A row of quiet action buttons (QCView's FlatButtons under a path), each with what it does.
struct Actions { buttons: Vec<(ActionButton, Cmd)> }
enum Item { Row(Row), Chips(Chips), Field(Field), Meters, Layers(Layers), Actions(Actions), /// QCView's hero thumbnail (the type glyph)
    Hero(&'static str), /// a small subdued caption (a link's app)
    Caption(String), /// a full-width mono path, elided in the middle
    PathLine(String) }
/// `shown` = QCView's card visibility (the video cards hide for an image sequence; the sequence card shows only for one)
struct Group { section: Section, items: Vec<Item>, rects: Vec<Rect>, shown: bool }
/// QCView's preset rates ("the 8 SMPTE / broadcast standards cover ~95% of source material")
const SEQ_RATES: [&str; 8] = ["23.976", "24", "25", "29.97", "30", "50", "59.94", "60"];

pub struct InspectorPanel {
    groups: Vec<Group>,
    path: String, size_bytes: Option<u64>,
    vmeta: Option<VideoMetadata>, ameta: Option<AdobeMetadata>,
    /// results keyed by the path they describe — kept until the panel is on that path (they land before the first layout at startup)
    pending_v: Option<(String, VideoMetadata)>, pending_a: Option<(String, AdobeMetadata)>,
    peaks: [f32; 16], channels: usize,
    range: RangeOverride, transfer: TransferOverride, routing: RoutingMode, origin: Option<String>,
    /// the presenter's geometry choices + the dims it fits with (the Display row tracks what the viewport shows)
    geometry: Geometry, display_dims: (u32, u32),
    /// the stored dims (the DAR ↔ PAR conversion needs them)
    storage: (u32, u32),
    height: f64, dirty_model: bool,
    /// the loaded sequence's live state: (stride, rate) rebuild the card; (ahead, behind) update its rows in place
    seq_shape: Option<(i32, f64)>, seq_fill: (i32, i32),
    /// the loaded EXR sequence's layers (the project item's), the current one, its first frame — and the tiles' thumbnails,
    /// decoded off-thread per (frame, layer) (QCView's `image://thumb/<first>?layer=`)
    seq_layers: (Vec<String>, String, String), layer_thumbs: std::collections::HashMap<String, peniko::ImageBrush>,
    layer_rx: Option<std::sync::mpsc::Receiver<(String, String, Option<qcv_player::ThumbPixels>)>>,
    tooltips: min_ui::Tooltips,
    /// the hero thumbnail: its key (path + layer), the picture once decoded, the worker's channel
    hero_key: String, hero: Option<peniko::ImageBrush>, hero_rx: Option<std::sync::mpsc::Receiver<(String, Option<qcv_player::ThumbPixels>)>>,
}

const LABEL_W: f64 = 96.0;
/// QCView's catalog contexts (canon row 195): the Inspector's own words, and the sequence card's (they differ for "Range").
/// The row / chip / action builders below translate the English they are handed; a label already translated passes
/// through unchanged (no catalog holds a translation as an English key).
fn tr(s: &str) -> &str { min_i18n::trc("InspectorPanel", s) }
fn trs(s: &str) -> &str { min_i18n::trc("ImageSequenceInspector", s) }
/// Quiet small buttons with icon + label; a disabled one carries why in its tooltip.
fn actions(v: Vec<(&'static str, &str, bool, &str, Cmd)>) -> Item {
    Item::Actions(Actions { buttons: v.into_iter().map(|(icon, label, on, why, cmd)| { let mut b = ActionButton::new(icon, tr(if on { label } else { why })).size_s().enabled(on); b.label = Some(tr(label).to_string()); (b, cmd) }).collect() })
}   // QCView's labelColWidth (logical)

/// QCView's `KvChipRow` is a Flow: the chips wrap. Here: on the label's line when they fit beside it, else on their own
/// line(s) below it, each line a fused run (radius on the run's ends). Returns the item's height.
fn layout_chips(c: &mut Chips, fonts: &mut Fonts, x0: f64, x1: f64, y: f64, scale: f64) -> f64 {
    let r = t::ACTION_BUTTON_CORNER_RADIUS_S as f64 * scale; let bh = t::ACTION_BUTTON_HEIGHT_S as f64 * scale; let line_gap = t::SPACING_75 as f64 * scale;
    let widths: Vec<f64> = c.group.buttons.iter().map(|b| b.measure(fonts, scale).0).collect();
    let total: f64 = widths.iter().sum();
    let label_w = LABEL_W * scale * 0.6;
    let (start_x, mut cy, avail) = if total <= x1 - x0 - label_w - t::SPACING_200 as f64 * scale { (x1 - total, y, x1 - x0) } else { (x0, y + bh + line_gap, x1 - x0) };
    c.rows.clear();
    let mut i = 0; let n = c.group.buttons.len();
    while i < n {
        // the run that fits this line (at least one chip)
        let mut j = i; let mut run_w = 0.0;
        while j < n && (j == i || run_w + widths[j] <= avail) { run_w += widths[j]; j += 1; }
        let rx = if cy == y { start_x } else { x0 };
        let mut cx = rx;
        for k in i..j {
            let b = &mut c.group.buttons[k];
            b.rect = Rect::new(cx, cy, cx + widths[k], cy + bh);
            b.corners = min_ui::action_button::Corners { tl: if k == i { r } else { 0.0 }, bl: if k == i { r } else { 0.0 }, tr: if k + 1 == j { r } else { 0.0 }, br: if k + 1 == j { r } else { 0.0 } };
            cx += widths[k];
        }
        c.rows.push(Rect::new(rx, cy, cx, cy + bh));
        cy += bh + line_gap; i = j;
    }
    c.group.rect = Rect::new(x0, y, x1, cy - line_gap);
    c.rect = c.group.rect;
    c.group.rect.height()
}

fn fmt_bytes(b: u64) -> String { if b >= 1 << 30 { format!("{:.2} GB", b as f64 / (1u64 << 30) as f64) } else if b >= 1 << 20 { format!("{:.1} MB", b as f64 / (1u64 << 20) as f64) } else { format!("{} KB", b / 1024) } }
fn fmt_dur(s: f64) -> String { let ms = (s * 1000.0).round() as i64; format!("{:02}:{:02}:{:02}.{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000) }
/// QCView `formatBitrate`.
fn fmt_bitrate(bps: i64) -> String { if bps <= 0 { String::new() } else if bps >= 1_000_000_000 { format!("{:.2} Gb/s", bps as f64 / 1e9) } else if bps >= 1_000_000 { format!("{:.1} Mb/s", bps as f64 / 1e6) } else if bps >= 1000 { format!("{:.0} kb/s", bps as f64 / 1e3) } else { format!("{bps} b/s") } }

impl InspectorPanel {
    pub fn new() -> InspectorPanel {
        let g = |title| Group { section: Section::new(tr(title), true), items: vec![], rects: vec![], shown: true };
        InspectorPanel { groups: vec![g("File"), g("Video"), g("Geometry"), g("Color"), g("Audio"), g("Timecode"), g("Linked Projects"), g("Image Sequence")], seq_shape: None, seq_fill: (-1, -1), seq_layers: Default::default(), layer_thumbs: Default::default(), layer_rx: None, tooltips: min_ui::Tooltips::default(), hero_key: String::new(), hero: None, hero_rx: None, path: String::new(), size_bytes: None, vmeta: None, ameta: None, pending_v: None, pending_a: None, peaks: [0.0; 16], channels: 0,
            range: RangeOverride::Auto, transfer: TransferOverride::Auto, routing: RoutingMode::Auto, origin: None, geometry: Geometry::default(), display_dims: (0, 0), storage: (0, 0), height: 0.0, dirty_model: true }
    }
    /// The extractor's result for `path` (the service delivers it after the open).
    pub fn set_video_meta(&mut self, path: &str, m: VideoMetadata) { self.pending_v = Some((path.to_string(), m)); self.dirty_model = true; }
    pub fn set_adobe_meta(&mut self, path: &str, m: AdobeMetadata) { self.pending_a = Some((path.to_string(), m)); self.dirty_model = true; }
    /// The presenter's geometry state (for the pickers' selection and the Display row).
    pub fn set_geometry(&mut self, g: Geometry, display_dims: (u32, u32)) { if g != self.geometry || display_dims != self.display_dims { self.geometry = g; self.display_dims = display_dims; self.dirty_model = true; } }
    /// Refill from the Transport when something it shows changed; the meters read every frame.
    /// The loaded clip's Transfer override (engine-level; the shell sets it from the project item and from the chips).
    pub fn set_transfer(&mut self, t: TransferOverride) { if t != self.transfer { self.transfer = t; self.dirty_model = true; } }
    /// The loaded sequence's EXR layers (empty = not an EXR sequence), the current layer and the first frame's path; a new
    /// first frame starts the tiles' thumbnails.
    pub fn set_seq_layers(&mut self, layers: Vec<String>, current: &str, first_frame: &str) {
        if self.seq_layers.0 == layers && self.seq_layers.1 == current && self.seq_layers.2 == first_frame { return }
        if self.seq_layers.2 != first_frame || self.seq_layers.0 != layers {
            self.layer_thumbs.clear(); self.layer_rx = None;
            if !layers.is_empty() && !first_frame.is_empty() {
                let (tx, rx) = std::sync::mpsc::channel(); self.layer_rx = Some(rx);
                let (names, first) = (layers.clone(), first_frame.to_string());
                std::thread::Builder::new().name("qcv-layer-thumbs".into()).spawn(move || {
                    for n in names { let px = qcv_player::min_image::load_thumbnail(&first, &n, 240).ok().and_then(|img| qcv_player::thumbnail_cache::image_thumb(&img, 240)); if tx.send((first.clone(), n, px)).is_err() { return } }
                }).ok();
            }
        }
        self.seq_layers = (layers, current.to_string(), first_frame.to_string()); self.dirty_model = true;
    }
    pub fn sync(&mut self, p: Option<&Transport>) -> bool {
        let Some(p) = p else { if !self.path.is_empty() { self.path.clear(); self.vmeta = None; self.ameta = None; for g in &mut self.groups { g.items.clear(); } self.dirty_model = false; return true } return false };
        let path = p.path().to_string();
        if path != self.path { self.path = path.clone(); self.size_bytes = std::fs::metadata(&path).ok().map(|m| m.len()); self.vmeta = None; self.ameta = None; self.dirty_model = true; }
        let (range, routing) = (p.range_override(), p.audio.routing_mode());
        if range != self.range || routing != self.routing { self.range = range; self.routing = routing; self.dirty_model = true; }
        let mut dirty = false;
        // QCView's hero: a video's MID-clip frame (first frames are routinely black leaders), a sequence's first frame at its layer
        let (key, src, frame, layer) = match p.decoder().as_sequence() {
            Some(s) => { let c = s.cache(); let f = c.frame_path(0); (format!("{f}?layer={}", c.layer()), f, 0, c.layer()) }
            None => (path.clone(), path.clone(), (p.frame_count() / 2).max(0), String::new()),
        };
        if key != self.hero_key {
            self.hero_key = key.clone(); self.hero = None; dirty = true;
            let (tx, rx) = std::sync::mpsc::channel(); self.hero_rx = Some(rx);
            let seq = p.decoder().as_sequence().is_some();
            std::thread::Builder::new().name("qcv-hero".into()).spawn(move || {
                use qcv_player::ThumbLoader;
                let px = if seq { qcv_player::min_image::load_thumbnail(&src, &layer, 480).ok().and_then(|img| qcv_player::thumbnail_cache::image_thumb(&img, 480)) }
                         else { qcv_player::VideoThumbLoader::open(&src).and_then(|mut l| l.load_thumbnail(frame, 480)) };
                let _ = tx.send((key, px));
            }).ok();
        }
        if let Some(rx) = &self.hero_rx && let Ok((k, px)) = rx.try_recv() { if k == self.hero_key { self.hero = px.map(|px| paint::image_rgba8(px.width, px.height, px.rgba)); dirty = true; } self.hero_rx = None; }
        if let Some(rx) = &self.layer_rx { while let Ok((first, n, px)) = rx.try_recv() { if first == self.seq_layers.2 && let Some(px) = px { self.layer_thumbs.insert(n, paint::image_rgba8(px.width, px.height, px.rgba)); dirty = true; } } }
        if p.audio.has_audio() {
            let pk = p.audio.peak_levels(); let ch = p.audio.source_channels().min(16);
            if ch != self.channels { self.channels = ch; self.dirty_model = true; }
            if (0..ch).any(|i| (pk[i] - self.peaks[i]).abs() > 0.01) { self.peaks = pk; dirty = true; }
        } else if self.channels != 0 { self.channels = 0; self.dirty_model = true; }
        let seq = p.decoder().as_sequence().map(|s| { let c = s.cache(); ((c.stride(), p.rate().as_f64()), (c.buffered_ahead(), c.buffered_behind())) });
        if seq.map(|s| s.0) != self.seq_shape { self.seq_shape = seq.map(|s| s.0); self.dirty_model = true; }
        if let Some((_, fill)) = seq && fill != self.seq_fill {
            self.seq_fill = fill;
            if !self.dirty_model { if let Some(c) = p.decoder().as_sequence().map(|s| s.cache().config()) { for it in self.groups[7].items.iter_mut() { if let Item::Row(r) = it {
                if r.label == trs("Cache ahead") { r.value = tf(trs("%1 / %2 frames"), &[&fill.0, &c.read_ahead]); dirty = true; }
                if r.label == trs("Cache behind") { r.value = tf(trs("%1 / %2 frames"), &[&fill.1, &c.read_behind]); dirty = true; }
            } } } }
        }
        if self.pending_v.as_ref().is_some_and(|(k, _)| *k == self.path) { self.vmeta = self.pending_v.take().map(|(_, m)| m); self.dirty_model = true; }
        if self.pending_a.as_ref().is_some_and(|(k, _)| *k == self.path) { self.ameta = self.pending_a.take().map(|(_, m)| m); self.dirty_model = true; }
        if self.dirty_model { self.rebuild(p); self.dirty_model = false; dirty = true; }
        dirty
    }
    fn rebuild(&mut self, p: &Transport) {
        let i = p.info(); let v = self.vmeta.as_ref(); let a = self.ameta.as_ref();
        let row = |label: &str, value: String, mono: bool| Item::Row(Row { label: tr(label).into(), value, note: String::new(), mono, warn: false });
        let row_n = |label: &str, value: String, note: String, mono: bool| Item::Row(Row { label: tr(label).into(), value, note, mono, warn: false });
        let chips = |label: &str, opts: &[(&str, &str)], selected_key: &str| {
            let buttons = opts.iter().map(|(_, l)| { let mut b = ActionButton::new("", tr(l)).size_s(); b.label = Some(tr(l).to_string()); b }).collect();
            let keys: Vec<String> = opts.iter().map(|(k, _)| k.to_string()).collect();
            let selected = keys.iter().position(|k| k == selected_key).unwrap_or(0);
            Item::Chips(Chips { id: label.to_string(), label: tr(label).into(), group: ActionGroup::new(buttons).compact(), keys, selected, rect: Rect::ZERO, rows: vec![] })
        };
        let pb = std::path::Path::new(&self.path);
        let name = pb.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| self.path.clone());
        let folder = pb.parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default();
        // ---- File ----
        let seq = p.decoder().as_sequence();
        let mut file = vec![Item::Hero(if seq.is_some() { "film-strip" } else { "video" }), row("Name", name, false), row("Folder", folder, false)];
        if let Some(b) = self.size_bytes { file.push(row("Size", fmt_bytes(b), true)); }
        // QCView's File card actions: Copy path · Reveal (a sequence: its first frame; Reveal off when the file is not reachable)
        let real = seq.map(|s| s.cache().frame_path(0)).unwrap_or_else(|| self.path.clone());
        let reachable = std::path::Path::new(&real).exists();
        // a sequence's duration is its frames at the item's rate (the extractor reads one frame at FFmpeg's default 25)
        file.push(row("Duration", fmt_dur(if seq.is_some() { i.duration_secs } else { v.map(|m| m.duration).filter(|d| *d > 0.0).unwrap_or(i.duration_secs) }), true));
        if let Some(m) = v {
            if !m.container_format.is_empty() { file.push(row("Container", if m.mxf_operational_pattern.is_empty() { m.container_format.clone() } else { format!("{} · {}", m.container_format, m.mxf_operational_pattern) }, false)); }
            if !m.encoder_tool.is_empty() { file.push(row("Encoder", m.encoder_tool.clone(), false)); }
        }
        file.push(actions(vec![("copy", "Copy path", true, "", Cmd::Copy(crate::os_actions::native(&real))), ("folder-simple", "Reveal", reachable, "Not reachable from this machine", Cmd::Reveal(real.clone()))]));   // the card's actions, last
        // ---- Video ----
        let (w, h) = v.map(|m| (m.width as u32, m.height as u32)).filter(|(w, _)| *w > 0).unwrap_or((i.width, i.height));
        let fps = v.map(|m| m.frame_rate).filter(|f| *f > 0.0).unwrap_or(i.rate().as_f64());
        let mut video = vec![
            row("Resolution", tf(tr("%1 × %2"), &[&w, &h]), true),
            row("Frame rate", format!("{fps:.3} fps"), true),
            row("Total frames", v.map(|m| m.total_frames).filter(|n| *n > 0).unwrap_or(i.frame_count).to_string(), true),
            row("Codec", v.map(|m| m.video_codec.clone()).filter(|c| !c.is_empty()).unwrap_or_else(|| i.codec_name.clone()), false),
        ];
        if let Some(m) = v {
            if !m.codec_profile.is_empty() { video.push(row("Profile", m.codec_profile.clone(), false)); }
            if m.video_bitrate > 0 || m.container_bitrate > 0 {
                let (value, note) = if m.video_bitrate > 0 { (fmt_bitrate(m.video_bitrate), if m.container_bitrate > 0 { tf(tr("file avg %1"), &[&fmt_bitrate(m.container_bitrate)]) } else { String::new() }) } else { (fmt_bitrate(m.container_bitrate), tr("file avg").to_string()) };
                video.push(row_n("Bitrate", value, note, true));
            }
            video.push(row_n("Pixel format", m.pixel_format.clone(), if m.is_rgb { tr("RGB").into() } else { String::new() }, true));
            video.push(row("Bit depth", if m.has_alpha { tf(tr("%1 bit + alpha"), &[&m.bit_depth]) } else { tf(tr("%1 bit"), &[&m.bit_depth]) }, true));
            if m.is_hdr_content { video.push(Item::Row(Row { label: tr("HDR").into(), value: tf(tr("Yes — %1"), &[&m.color_transfer]), note: String::new(), mono: false, warn: true })); }
        } else {
            video.push(row("Pixel format", i.pix_fmt.clone(), true));
            video.push(row("Bit depth", tf(tr("%1 bit"), &[&i.meta.bit_depth]), true));
        }
        video.push(row("Decode", if i.hw_accel.is_empty() { tr("software").to_string() } else { i.hw_accel.clone() }, false));   // ours: the decode path (not in QCView's panel)
        // ---- Geometry (the read-outs; the Aspect + Rotation pickers wait for their engine stages) ----
        let (sn, sd, anam) = v.map(|m| (m.sar_num, m.sar_den, m.is_anamorphic)).unwrap_or((i.meta.sar.0 as i32, i.meta.sar.1 as i32, i.meta.sar != (1, 1)));
        let rot = v.map(|m| m.rotation_deg).unwrap_or(i.meta.rotation as i32);
        // QCView's Aspect pill: Square (stored pixels untouched, the default) / Detected n:d / Custom (the custom fields wait)
        let detected_label = if anam && sd > 0 { tf(tr("Detected %1:%2"), &[&sn, &sd]) } else { tr("Detected").to_string() };
        self.storage = (w, h);
        let mut geometry = vec![chips("Aspect", &[("square", "Square"), ("detected", detected_label.as_str()), ("custom", "Custom")], match self.geometry.par { Par::Square => "square", Par::Detected => "detected", Par::Custom(..) => "custom" })];
        if let Par::Custom(n, d) = self.geometry.par {
            let par = if d > 0 { n as f64 / d as f64 } else { 1.0 }; let dar = if h > 0 && d > 0 { (w as f64 * n as f64) / (h as f64 * d as f64) } else { 0.0 };
            let mk = |label: &str, kind, text: String, suffix: &str| { let mut f = TextField::new(Family::Mono, t::CODE_SIZE_S); f.set_text(&text); Item::Field(Field { label: label.into(), kind, field: f, suffix: suffix.into(), rect: Rect::ZERO }) };
            geometry.push(mk("Pixel aspect", FieldKind::Par, format!("{par:.4}"), ""));
            geometry.push(mk("Display aspect", FieldKind::Dar, if dar > 0.0 { format!("{dar:.4}") } else { String::new() }, ": 1"));
        }
        geometry.push(row("Detected", if anam && sd > 0 { tf(tr("%1:%2 (non-square)"), &[&sn, &sd]) } else { tr("1:1 (square)").into() }, true));
        let (dw, dh) = if self.display_dims.0 > 0 { self.display_dims } else { (w, h) };
        if dw > 0 && dh > 0 { geometry.push(row("Display", format!("{:.3}:1   ({dw} × {dh})", dw as f64 / dh as f64), true)); }   // what the viewport shows (rotation swapped, PAR applied)
        // QCView's Rotation pill: Auto labels itself with the detected display matrix
        let auto_label = if rot > 0 { tf(tr("Auto (%1°)"), &[&rot]) } else { tr("Auto").to_string() };
        geometry.push(chips("Rotation", &[("auto", auto_label.as_str()), ("0", "0°"), ("90", "90°"), ("180", "180°"), ("270", "270°")], match self.geometry.rotation { None => "auto", Some(0) => "0", Some(90) => "90", Some(180) => "180", Some(270) => "270", _ => "auto" }));
        // ---- Color ----
        let mut color = vec![];
        if let Some(m) = v {
            if !m.colorspace.is_empty() { color.push(row("Colorspace", m.colorspace.clone(), false)); }
            if !m.color_primaries.is_empty() { color.push(row("Primaries", m.color_primaries.clone(), false)); }
            if !m.color_transfer.is_empty() { color.push(row("Transfer", m.color_transfer.clone(), false)); }
            if m.max_cll > 0 || m.mastering_max_nits > 0.0 {
                let mut parts = vec![]; if m.max_cll > 0 { parts.push(tf(tr("MaxCLL %1"), &[&m.max_cll])); } if m.max_fall > 0 { parts.push(tf(tr("MaxFALL %1"), &[&m.max_fall])); } if m.mastering_max_nits > 0.0 { parts.push(tf(tr("mastering %1 nits"), &[&(m.mastering_max_nits.round() as i64)])); }
                color.push(row("HDR metadata", parts.join(", "), true));
            }
            if !m.nclc_tag.is_empty() { color.push(row("NCLC tag", m.nclc_tag.clone(), true)); }
            let (value, note) = if !m.container_range_tag.is_empty() { (m.container_range_tag.clone(), String::new()) } else if !m.color_range.is_empty() { (tr("untagged").to_string(), tf(tr("decoder: %1"), &[&m.color_range])) } else { (tr("untagged").to_string(), tr(if m.is_rgb { "Auto assumes full" } else { "Auto assumes limited" }).to_string()) };
            color.push(row_n("Range tag", value, note, false));
        } else {
            color.push(row("Colorspace", format!("{:?}", i.meta.matrix), false)); color.push(row("Primaries", format!("{:?}", i.meta.primaries), false)); color.push(row("Transfer", format!("{:?}", i.meta.transfer), false));
        }
        // the Range picker: Auto labels itself with the detected range (QCView's rule)
        let detected = v.map(|m| if m.color_range == "full" { "Auto (Full)" } else if m.color_range == "limited" { "Auto (Limited)" } else if m.container_range_tag == "full" { "Auto (Full)" } else if m.container_range_tag == "limited" { "Auto (Limited)" } else if m.is_rgb { "Auto (Full, assumed)" } else { "Auto (Limited, assumed)" }).unwrap_or("Auto");
        color.push(chips("Range", &[("auto", detected), ("full", "Full"), ("limited", "Limited")], match self.range { RangeOverride::Auto => "auto", RangeOverride::Full => "full", RangeOverride::Limited => "limited" }));
        // the Transfer override (QCView InspectorPanel.qml "Transfer / encoding override"): how the clip is READ when its tags are
        // missing or wrong (an untagged PQ export); Auto labels itself with the tags' resolution. A decode fact, not a curve choice —
        // sRGB 2.2 vs BT.1886 2.4 is the chain's Input (canon §18.6).
        let detected_t = v.map(|m| { let t = m.color_transfer.to_lowercase(); let p = m.color_primaries.to_lowercase();
            if t == "smpte2084" || t.contains("pq") { if p.contains("432") || p.contains("431") || p.contains("p3") { "Auto (PQ P3)" } else { "Auto (PQ 2020)" } }
            else if t.contains("arib") || t.contains("hlg") { "Auto (HLG)" }
            else if t.is_empty() || t == "unknown" || t == "unspecified" { "Auto (SDR, untagged)" } else { "Auto (SDR)" } }).unwrap_or("Auto");
        color.push(chips("Transfer", &[("auto", detected_t), ("sdr", "SDR 709"), ("pq2020", "PQ 2020"), ("pqp3", "PQ P3"), ("hlg", "HLG"), ("linear", "Linear")],
            match self.transfer { TransferOverride::Auto => "auto", TransferOverride::SdrRec709 => "sdr", TransferOverride::Pq2020 => "pq2020", TransferOverride::PqP3D65 => "pqp3", TransferOverride::Hlg => "hlg", TransferOverride::Linear => "linear" }));
        // ---- Audio ----
        let mut audio = vec![];
        let has_audio = p.audio.has_audio() || v.is_some_and(|m| !m.audio_codec.is_empty());
        if has_audio {
            if let Some(m) = v { if !m.audio_codec.is_empty() { audio.push(row("Codec", m.audio_codec.clone(), false)); } if m.audio_sample_rate > 0 { audio.push(row("Sample rate", tf(tr("%1 Hz"), &[&m.audio_sample_rate]), true)); } }
            let ch = v.map(|m| m.audio_channels as usize).filter(|c| *c > 0).unwrap_or(p.audio.source_channels());
            audio.push(row("Channels", match ch { 2 => tf(tr("%1 (stereo)"), &[&2]), 1 => tf(tr("%1 (mono)"), &[&1]), n => n.to_string() }, true));
            let layout = v.map(|m| m.audio_channel_layout_name.clone()).filter(|l| !l.is_empty()).unwrap_or_else(|| p.audio.layout_name());
            if !layout.is_empty() { audio.push(row("Layout", layout, false)); }
            if p.audio.has_audio() {
                let mut opts: Vec<(&str, &str)> = vec![("auto", "Auto")]; if ch >= 6 { opts.push(("5.1", "5.1 → 2")); } if ch >= 8 { opts.push(("7-8", "7-8")); }
                if opts.len() > 1 { audio.push(chips("Mix", &opts, match self.routing { RoutingMode::Auto => "auto", RoutingMode::Downmix5_1 => "5.1", RoutingMode::Stereo7_8 => "7-8" })); }
                audio.push(Item::Meters);
                audio.push(row("Device latency", format!("{:.1} ms", p.audio.device_latency_ms()), true));   // ours
            }
        } else { audio.push(row("Audio", tr("none").into(), false)); }
        // ---- Timecode ----
        let mut tc = vec![];
        let embedded = v.map(|m| m.embedded_timecode.clone()).filter(|s| !s.is_empty()).or_else(|| i.source_timecode.clone());
        let mut origins: Vec<(String, &str)> = vec![(String::new(), "From start")];
        if let Some(e) = &embedded { origins.push((e.clone(), "Embedded")); }
        if let Some(am) = a { if !am.qt_start_timecode.is_empty() { origins.push((am.qt_start_timecode.clone(), "QT Start")); } if !am.qt_timecode.is_empty() { origins.push((am.qt_timecode.clone(), "QT TC")); } if !am.xmp_alt_timecode.is_empty() { origins.push((am.xmp_alt_timecode.clone(), "XMP Alt")); } }
        if origins.len() > 1 {
            let sel = self.origin.clone().unwrap_or_else(|| embedded.clone().unwrap_or_default());
            let opts: Vec<(&str, &str)> = origins.iter().map(|(k, l)| (k.as_str(), *l)).collect();
            tc.push(chips("Origin", &opts, &sel));
        }
        if let Some(e) = &embedded {
            let fmt = qcv_meta::TimecodeFormatter::new(i.frame_rate.0, i.frame_rate.1, i.drop_frame);
            let note = if fmt.is_valid() { tf(tr("(frame %1)"), &[&fmt.parse(e)]) } else { String::new() };
            tc.push(row_n("Source", e.clone(), note, true));
        }
        if let Some(am) = a {
            for (label, val) in [("QT Start", &am.qt_start_timecode), ("QT TimeCode", &am.qt_timecode), ("XMP Alt", &am.xmp_alt_timecode)] { if !val.is_empty() { tc.push(row(label, val.clone(), true)); } }
            for (label, val) in [("Created", &am.qt_creation_date), ("Media Created", &am.qt_media_create_date)] { if !val.is_empty() { tc.push(row(label, val.clone(), false)); } }
        }
        tc.push(row("Drop frame", tr(if i.drop_frame { "yes" } else { "no" }).to_string(), false));   // ours
        // ---- Linked Projects (QCView's card: After Effects · Premiere (Mac) · Premiere (Windows); Open · Copy · Reveal; ours: Open /
        // Reveal off with a tooltip when the path is not reachable from this machine — cross-OS mapping waits for its table) ----
        let mut linked = vec![];
        if let Some(am) = a {
            for (label, path) in [("After Effects", &am.ae_project_path), ("Premiere (Mac)", &am.premiere_mac_path), ("Premiere (Windows)", &am.premiere_win_path)] {
                if path.is_empty() { continue }
                let there = std::path::Path::new(path.as_str()).exists();
                linked.push(Item::Caption(tr(label).into()));
                linked.push(Item::PathLine(crate::os_actions::native(path)));
                linked.push(actions(vec![("arrow-square-out", "Open", there, "Not reachable from this machine", Cmd::Open(path.clone())), ("copy", "Copy", true, "", Cmd::Copy(crate::os_actions::native(path))), ("folder-simple", "Reveal", there, "Not reachable from this machine", Cmd::Reveal(path.clone()))]));
            }
        }
        // ---- Image Sequence (QCView `ImageSequenceInspector`; the EXR layer list and Compression wait for piece 5) ----
        let mut sq = vec![];
        if let Some(s) = seq {
            let c = s.cache(); let cfg = c.config();
            let fps = p.rate().as_f64();
            let rate_opts: Vec<(&str, &str)> = SEQ_RATES.iter().map(|r| (*r, *r)).collect();
            let mut rates = chips("Frame rate", &rate_opts, "");
            if let Item::Chips(ch) = &mut rates { ch.selected = SEQ_RATES.iter().position(|r| (r.parse::<f64>().unwrap() - fps).abs() < 0.01).unwrap_or(usize::MAX); }
            sq.push(rates);
            let is_exr = i.codec_short == "exr";
            let detected_t = if is_exr { "Auto (Linear, EXR)" } else { "Auto (sRGB)" };
            sq.push(chips("Transfer", &[("auto", trs(detected_t)), ("sdr", "SDR 709"), ("pq2020", "PQ 2020"), ("pqp3", "PQ P3"), ("hlg", "HLG"), ("linear", "Linear")],
                match self.transfer { TransferOverride::Auto => "auto", TransferOverride::SdrRec709 => "sdr", TransferOverride::Pq2020 => "pq2020", TransferOverride::PqP3D65 => "pqp3", TransferOverride::Hlg => "hlg", TransferOverride::Linear => "linear" }));
            // QCView's readout: Range (first – last file number, the count), Pattern, Resolution, Duration
            let n = c.frame_count();
            sq.push(row(trs("Range"), tf(trs("%1 – %2 (%3 frames)"), &[&c.start_frame(), &(c.start_frame() + n - 1), &n]), true));
            sq.push(row(trs("Pattern"), c.pattern().to_string(), true));
            sq.push(row(trs("Resolution"), tf(trs("%1 × %2"), &[&c.width(), &c.height()]), true));
            if let Some(cm) = is_exr.then(|| qcv_player::min_image::exr::compression(&self.seq_layers.2)).flatten() { sq.push(row(trs("Compression"), cm.to_string(), true)); }
            sq.push(row(trs("Duration"), format!("{:.2} s", i.duration_secs), true));
            let stride = c.stride().to_string();
            sq.push(chips("Cache stride", &[("1", "1x"), ("2", "2x"), ("3", "3x"), ("4", "4x")], &stride));
            sq.push(row(trs("Cache ahead"), tf(trs("%1 / %2 frames"), &[&c.buffered_ahead(), &cfg.read_ahead]), true));
            sq.push(row(trs("Cache behind"), tf(trs("%1 / %2 frames"), &[&c.buffered_behind(), &cfg.read_behind]), true));
            // QCView's layer list, anchored at the bottom (multi-AOV renders ship dozens; cryptomatte filtered at discovery)
            if is_exr && !self.seq_layers.0.is_empty() { sq.push(Item::Layers(Layers { names: self.seq_layers.0.clone(), current: self.seq_layers.1.clone(), tiles: vec![], aspect: if c.width() > 0 { c.height() as f64 / c.width() as f64 } else { 9.0 / 16.0 }, hover: None })); }
        }
        // QCView: Video / Geometry / Color need `videoLoaded` (never for a sequence); Timecode needs a line; the sequence card needs a sequence
        let is_seq = seq.is_some();
        for (gi, g) in self.groups.iter_mut().enumerate() { g.shown = match gi { 0 => true, 6 => !linked.is_empty(), 7 => is_seq, _ => !is_seq }; }
        for (g, items) in self.groups.iter_mut().zip([file, video, geometry, color, audio, tc, linked, sq]) { g.items = items; }
        for g in self.groups.iter_mut() { for it in g.items.iter_mut() { if let Item::Chips(c) = it { let sel = c.selected; for (j, b) in c.group.buttons.iter_mut().enumerate() { b.selected = j == sel; } } } }
    }
    pub fn layout(&mut self, fonts: &mut Fonts, x0: f64, x1: f64, y: f64, scale: f64) -> f64 {
        let hh = Section::height(scale); let pad = t::SPACING_100 as f64 * scale; let gap = t::RAIL_ROW_GAP as f64 * scale;
        let row_h = t::COMPONENT_HEIGHT_75 as f64 * scale;
        let mut cy = y;
        for g in &mut self.groups {
            if !g.shown { g.section.rect = Rect::ZERO; g.rects.clear(); continue }
            g.section.rect = Rect::new(x0, cy, x1, cy + hh); cy += hh;
            g.rects.clear();
            if !g.section.expanded || g.items.is_empty() { continue }
            cy += pad / 2.0;
            for it in g.items.iter_mut() {
                let h = match it {
                    Item::Row(_) | Item::PathLine(_) => row_h,
                    Item::Chips(c) => { let h = layout_chips(c, fonts, x0 + pad, x1 - pad, cy, scale); h + gap }
                    Item::Field(f) => { let fw = 90.0 * scale; let fh = t::FIELD_HEIGHT_S as f64 * scale; let sw = if f.suffix.is_empty() { 0.0 } else { fonts.layout(&f.suffix, Family::Mono, t::CODE_SIZE_S * scale as f32, t::FONT_WEIGHT_REGULAR as f32, peniko::Color::WHITE).width() as f64 + t::SPACING_75 as f64 * scale }; let x1f = x1 - pad - sw; f.rect = Rect::new(x1f - fw, cy + (row_h - fh) / 2.0, x1f, cy + (row_h + fh) / 2.0); f.field.layout(fonts, f.rect, scale); row_h }
                    Item::Meters => 6.0 * scale * self.channels.max(1) as f64 + gap,
                    Item::Actions(a) => {
                        let mut x = x0 + pad; let bh = t::ACTION_BUTTON_HEIGHT_S as f64 * scale;
                        for (b, _) in a.buttons.iter_mut() { let (w, _) = b.measure(fonts, scale); b.rect = Rect::new(x, cy, x + w, cy + bh); x += w + t::SPACING_100 as f64 * scale; }
                        bh
                    }
                    Item::Caption(_) => t::RAIL_GROUP_HEADER_SIZE as f64 * scale * 1.5,
                    Item::Hero(_) => ((x1 - x0 - 2.0 * pad) * 9.0 / 16.0).round(),
                    Item::Layers(l) => {
                        let (lx0, lx1) = (x0 + pad, x1 - pad); let tg = t::SPACING_100 as f64 * scale; let ip = t::SPACING_85 as f64 * scale;
                        let top = t::RAIL_GROUP_GAP as f64 * scale - gap;   // the caption is a sub-header: the group gap above it
                        let cap = top + t::RAIL_GROUP_HEADER_SIZE as f64 * scale * 1.4 + tg;
                        let tw = ((lx1 - lx0 - tg) / 2.0).max(1.0); let th = ip + (tw - 2.0 * ip) * l.aspect + t::SPACING_75 as f64 * scale + 16.0 * scale + ip;
                        l.tiles = (0..l.names.len()).map(|i| { let (c, r) = ((i % 2) as f64, (i / 2) as f64); let x = lx0 + c * (tw + tg); let y = cy + cap + r * (th + tg); Rect::new(x, y, x + tw, y + th) }).collect();
                        let rows = l.names.len().div_ceil(2) as f64;
                        cap + rows * th + (rows - 1.0).max(0.0) * tg + gap
                    }
                };
                g.rects.push(Rect::new(x0 + pad, cy, x1 - pad, cy + h)); cy += h + gap;
            }
            cy += t::RAIL_SECTION_GAP as f64 * scale - gap;
        }
        self.height = cy - y;
        self.height
    }
    fn commit_field(storage: (u32, u32), f: &mut Field) -> Option<Cmd> {
        let v: f64 = f.field.text().trim().parse().ok()?;
        if !(v > 0.0) { return None }
        let v = v.clamp(0.05, 20.0);
        let (w, h) = storage;
        match f.kind {
            FieldKind::Par => Some(Cmd::Par(Par::Custom((v * 10000.0).round() as u32, 10000))),
            FieldKind::Dar => if w > 0 && h > 0 { Some(Cmd::Par(Par::Custom((v * h as f64 * 1000.0).round() as u32, w * 1000))) } else { None },
        }
    }
    pub fn has_focus(&self) -> bool { self.groups.iter().flat_map(|g| g.items.iter()).any(|it| matches!(it, Item::Field(f) if f.field.focused)) }
    /// Keys to the focused field; Enter commits, Escape reverts (the model rebuilds from the presenter's state).
    pub fn key(&mut self, k: &KeyEvent, fonts: &mut Fonts) -> Vec<Cmd> {
        let storage = self.storage; let mut cmds = vec![];
        for g in &mut self.groups { for it in g.items.iter_mut() { if let Item::Field(f) = it { if f.field.focused {
            match f.field.key(k, fonts) { FieldResponse::Accepted => { f.field.focused = false; if let Some(c) = Self::commit_field(storage, f) { cmds.push(c); } self.dirty_model = true; } FieldResponse::Cancelled => { f.field.focused = false; self.dirty_model = true; } _ => {} }
        } } } }
        cmds
    }
    pub fn ime(&mut self, e: &ImeEvent, fonts: &mut Fonts) { for g in &mut self.groups { for it in g.items.iter_mut() { if let Item::Field(f) = it { if f.field.focused { f.field.ime(e, fonts); } } } } }
    /// Focus lost (a press elsewhere, the window): commit a typed value.
    pub fn blur(&mut self) -> Vec<Cmd> {
        let storage = self.storage; let mut cmds = vec![];
        for g in &mut self.groups { for it in g.items.iter_mut() { if let Item::Field(f) = it { if f.field.focused { f.field.focused = false; if let Some(c) = Self::commit_field(storage, f) { cmds.push(c); } self.dirty_model = true; } } } }
        cmds
    }
    /// Returns (commands, redraw needed).
    pub fn event(&mut self, e: &Event, now: Instant, fonts: &mut Fonts) -> (Vec<Cmd>, bool) {
        let mut dirty = false; let mut cmds = vec![];
        // a press outside a focused field commits it
        if let Event::Down { x, y, .. } = *e { let on_field = self.groups.iter().flat_map(|g| g.items.iter()).any(|it| matches!(it, Item::Field(f) if f.field.hit(x, y))); if !on_field && self.has_focus() { cmds.extend(self.blur()); dirty = true; } }
        // a press on one field commits any other (focus moves field to field without Enter)
        if let Event::Down { x, y, .. } = *e { let storage = self.storage; for g in &mut self.groups { for it in g.items.iter_mut() { if let Item::Field(f) = it { if f.field.focused && !f.field.hit(x, y) { f.field.focused = false; if let Some(c) = Self::commit_field(storage, f) { cmds.push(c); } self.dirty_model = true; dirty = true; } } } } }
        for g in &mut self.groups { if g.shown && g.section.expanded { for it in g.items.iter_mut() { if let Item::Field(f) = it { let was = f.field.focused; if f.field.pointer(e, fonts) { dirty = true; } if f.field.focused && !was { f.field.select_all(fonts); } } } } }
        for g in &mut self.groups {
            if !g.shown { continue }
            let (_, d) = g.section.event(e, now); dirty |= d;
            if !g.section.expanded { continue }
            for it in g.items.iter_mut() { if let Item::Actions(a) = it { for (b, c) in a.buttons.iter_mut() {
                let r = b.event(e, now); if r != Response::None { dirty = true; }
                if r == Response::Clicked && b.enabled { cmds.push(c.clone()); }
            } } }
            for it in g.items.iter_mut() { if let Item::Layers(l) = it {
                match *e {
                    Event::Move { x, y } => { let h = l.tiles.iter().position(|r| r.contains(kurbo::Point::new(x, y))); if h != l.hover { l.hover = h; dirty = true; } }
                    Event::Down { x, y, .. } => { if let Some(i) = l.tiles.iter().position(|r| r.contains(kurbo::Point::new(x, y))) { if l.names[i] != l.current { l.current = l.names[i].clone(); cmds.push(Cmd::Layer(l.current.clone())); } dirty = true; } }
                    _ => {}
                }
            } }
            for it in g.items.iter_mut() { if let Item::Chips(c) = it {
                for (i, r) in c.group.event(e, now) { dirty = true; if r == Response::Clicked && i != c.selected {
                    c.selected = i;
                    let key = c.keys[i].as_str();
                    match c.id.as_str() {
                        "Frame rate" => { if let Ok(f) = key.parse::<f64>() { cmds.push(Cmd::FrameRate(f)); } }
                        "Cache stride" => { if let Ok(s) = key.parse::<i32>() { cmds.push(Cmd::Stride(s)); } }
                        "Range" => cmds.push(Cmd::Range(match key { "full" => RangeOverride::Full, "limited" => RangeOverride::Limited, _ => RangeOverride::Auto })),
                        "Transfer" => { let t = match key { "sdr" => TransferOverride::SdrRec709, "pq2020" => TransferOverride::Pq2020, "pqp3" => TransferOverride::PqP3D65, "hlg" => TransferOverride::Hlg, "linear" => TransferOverride::Linear, _ => TransferOverride::Auto }; self.transfer = t; cmds.push(Cmd::Transfer(t)); }
                        "Mix" => cmds.push(Cmd::Routing(match key { "5.1" => RoutingMode::Downmix5_1, "7-8" => RoutingMode::Stereo7_8, _ => RoutingMode::Auto })),
                        "Origin" => { self.origin = Some(key.to_string()); cmds.push(Cmd::Origin(key.to_string())); }
                        "Aspect" => cmds.push(Cmd::Par(match key { "detected" => Par::Detected, "custom" => match self.geometry.par { Par::Custom(n, d) => Par::Custom(n, d), _ => Par::Custom(1, 1) }, _ => Par::Square })),
                        "Rotation" => cmds.push(Cmd::Rotation(match key { "0" => Some(0), "90" => Some(90), "180" => Some(180), "270" => Some(270), _ => None })),
                        _ => {}
                    }
                } }
                for (j, b) in c.group.buttons.iter_mut().enumerate() { b.selected = j == c.selected; }
            } }
        }
        // the action buttons' tooltips (a disabled one says why)
        let over = e.pos().filter(|_| !matches!(e, Event::Leave)).and_then(|(x, y)| self.groups.iter().filter(|g| g.shown && g.section.expanded).flat_map(|g| g.items.iter()).find_map(|it| match it {
            Item::Actions(a) => a.buttons.iter().enumerate().find(|(_, (b, _))| b.hit(x, y) && !b.enabled).map(|(i, (b, _))| (900 + i as u64, b.rect, b.tooltip.clone())), _ => None }));
        if self.tooltips.hover(over.as_ref().map(|(i, r, s)| (*i, *r, s.as_str())), now) { dirty = true; }
        if matches!(e, Event::Down { .. }) { self.tooltips.pressed(now); }
        (cmds, dirty)
    }
    /// The tooltip clock (true = redraw).
    pub fn tick(&mut self, now: Instant) -> bool { self.tooltips.tick(now) }
    /// Tooltips, over every panel.
    pub fn paint_overlays(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, bounds: Rect, now: Instant) { self.tooltips.paint(scene, fonts, scale, bounds, now); }
    /// The harness: a chip's rect by its row label and index.
    pub fn chip_rect(&self, label: &str, i: usize) -> Option<Rect> { self.groups.iter().flat_map(|g| g.items.iter()).find_map(|it| match it { Item::Chips(c) if c.id == label => c.group.buttons.get(i).map(|b| b.rect), _ => None }) }
    pub fn animating(&self, now: Instant) -> bool { self.groups.iter().any(|g| g.section.animating(now) || g.items.iter().any(|it| matches!(it, Item::Chips(c) if c.group.animating(now)))) }
    pub fn paint(&mut self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, now: Instant) {
        for g in self.groups.iter_mut() { if g.shown && g.section.expanded { for it in g.items.iter_mut() { if let Item::Field(f) = it { f.field.paint(scene, fonts, scale); } } } }
        for (gi, g) in self.groups.iter().enumerate() {
            if !g.shown { continue }
            let bottom = if g.section.expanded { g.rects.last().map(|r| r.y1 + t::SPACING_100 as f64 * scale).unwrap_or(g.section.rect.y1) } else { g.section.rect.y1 };
            min_ui::section::paint_tile(scene, Rect::new(g.section.rect.x0, g.section.rect.y0, g.section.rect.x1, bottom), scale);
            g.section.paint(scene, fonts, scale, now, gi == 0);
            if !g.section.expanded { continue }
            for (it, r) in g.items.iter().zip(&g.rects) {
                match it {
                    Item::Row(row) => {
                        let l = fonts.layout(&row.label, Family::Sans, t::RAIL_LABEL_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_VALUE_COLOR));
                        min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((r.x0, r.y0 + (r.height() - l.height() as f64) / 2.0)));
                        let label_w = (l.width() as f64).max(LABEL_W * scale * 0.6);
                        let avail = (r.width() - label_w - t::SPACING_200 as f64 * scale).max(40.0);
                        let fam = if row.mono { Family::Mono } else { Family::Sans }; let size = if row.mono { t::CODE_SIZE_S } else { t::BODY_SIZE_S };
                        let vc = if row.warn { t::NOTICE_VISUAL_COLOR } else { t::RAIL_LABEL_COLOR };
                        let mut v = fonts.layout(&row.value, fam, size * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(vc));
                        let note_w = if row.note.is_empty() { 0.0 } else { fonts.layout(&row.note, Family::Sans, t::DETAIL_SIZE_S * scale as f32, t::FONT_WEIGHT_REGULAR as f32, peniko::Color::WHITE).width() as f64 + t::SPACING_100 as f64 * scale };
                        let avail_v = (avail - note_w).max(30.0);
                        if v.width() as f64 > avail_v {
                            // elide from the left for paths, from the right otherwise (QCView: folders show their tail)
                            let chars: Vec<char> = row.value.chars().collect(); let mut lo = 0usize; let mut hi = chars.len();
                            while lo < hi { let mid = (lo + hi).div_ceil(2); let s: String = if row.label == tr("Folder") { format!("…{}", chars[chars.len() - mid..].iter().collect::<String>()) } else { format!("{}…", chars[..mid].iter().collect::<String>()) }; let t2 = fonts.layout(&s, fam, size * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(vc)); if t2.width() as f64 <= avail_v { lo = mid; v = t2; } else { hi = mid - 1; } }
                        }
                        let vx = r.x1 - v.width() as f64;
                        min_ui::fonts::draw_layout(scene, &v, kurbo::Affine::translate((vx, r.y0 + (r.height() - v.height() as f64) / 2.0)));
                        if !row.note.is_empty() { let n = fonts.layout(&row.note, Family::Sans, t::DETAIL_SIZE_S * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::DISABLED_CONTENT_COLOR)); min_ui::fonts::draw_layout(scene, &n, kurbo::Affine::translate((vx - t::SPACING_100 as f64 * scale - n.width() as f64, r.y0 + (r.height() - n.height() as f64) / 2.0))); }
                    }
                    Item::Chips(c) => {
                        let l = fonts.layout(&c.label, Family::Sans, t::RAIL_LABEL_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_VALUE_COLOR));
                        let bh = t::ACTION_BUTTON_HEIGHT_S as f64 * scale;
                        min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((r.x0, c.group.rect.y0 + (bh - l.height() as f64) / 2.0)));
                        for row in &c.rows { paint::rounded(scene, *row, t::ACTION_BUTTON_CORNER_RADIUS_S as f64 * scale, min_ui::section::card_color()); }
                        for b in &c.group.buttons { b.paint(scene, fonts, scale, now); }
                    }
                    Item::Field(f) => {
                        let l = fonts.layout(&f.label, Family::Sans, t::RAIL_LABEL_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_VALUE_COLOR));
                        min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((r.x0, r.y0 + (r.height() - l.height() as f64) / 2.0)));
                        // the field paints its own well; the editor's `paint` wants &mut, so the panel's paint takes the item through a cell below
                        if !f.suffix.is_empty() { let sfx = fonts.layout(&f.suffix, Family::Mono, t::CODE_SIZE_S * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_VALUE_COLOR)); min_ui::fonts::draw_layout(scene, &sfx, kurbo::Affine::translate((f.rect.x1 + t::SPACING_75 as f64 * scale, r.y0 + (r.height() - sfx.height() as f64) / 2.0))); }
                    }
                    Item::Actions(a) => { for (b, _) in &a.buttons { b.paint(scene, fonts, scale, now); } }
                    Item::Hero(icon) => {
                        // QCView's tile: 16:9 recess, the picture fitted, its type badge bottom-right; no picture yet = the type glyph
                        let rad = t::CORNER_RADIUS_SMALL_DEFAULT as f64 * scale;
                        paint::rounded(scene, *r, rad, t::FIELD_BACKGROUND_COLOR);
                        match &self.hero {
                            Some(b) => {
                                let (bw, bh) = (b.image.width as f64, b.image.height as f64); let s = (r.width() / bw).min(r.height() / bh); let (w, h) = (bw * s, bh * s);
                                paint::image(scene, b, Rect::new(r.center().x - w / 2.0, r.center().y - h / 2.0, r.center().x + w / 2.0, r.center().y + h / 2.0));
                                let bs = 22.0 * scale; let br = Rect::new(r.x1 - 4.0 * scale - bs, r.y1 - 4.0 * scale - bs, r.x1 - 4.0 * scale, r.y1 - 4.0 * scale);
                                paint::rounded(scene, br, rad, min_theme::Color { r: 0.0, g: 0.0, b: 0.0, a: 0.55 });
                                if let Some(ic) = fonts.icon(icon, 14.0 * scale as f32, paint::color(t::GRAY_900)) { min_ui::fonts::draw_layout(scene, &ic, kurbo::Affine::translate((br.center().x - ic.width() as f64 / 2.0, br.center().y - ic.height() as f64 / 2.0))); }
                            }
                            None => if let Some(ic) = fonts.icon(icon, t::ICON_SIZE_LARGE * scale as f32, paint::color(t::RAIL_GROUP_HEADER_COLOR)) { min_ui::fonts::draw_layout(scene, &ic, kurbo::Affine::translate((r.center().x - ic.width() as f64 / 2.0, r.center().y - ic.height() as f64 / 2.0))); },
                        }
                    }
                    Item::Caption(c) => {
                        let l = fonts.layout(c, Family::Sans, t::RAIL_GROUP_HEADER_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_VALUE_COLOR));
                        min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((r.x0, r.y1 - l.height() as f64)));
                    }
                    Item::PathLine(pth) => {
                        let (size, c) = (t::CODE_SIZE_S * scale as f32, paint::color(t::RAIL_LABEL_COLOR));
                        let mut l = fonts.layout(pth, Family::Mono, size, t::FONT_WEIGHT_REGULAR as f32, c);
                        if l.width() as f64 > r.width() {
                            // QCView's ElideMiddle: the head (the volume) and the tail (the project's name) both stay
                            let chars: Vec<char> = pth.chars().collect(); let mut keep = chars.len();
                            while keep > 4 { keep -= 1; let head = keep / 3; let tail = keep - head; let s = format!("{}…{}", chars[..head].iter().collect::<String>(), chars[chars.len() - tail..].iter().collect::<String>()); l = fonts.layout(&s, Family::Mono, size, t::FONT_WEIGHT_REGULAR as f32, c); if l.width() as f64 <= r.width() { break } }
                        }
                        min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((r.x0, r.y0 + (r.height() - l.height() as f64) / 2.0)));
                    }
                    Item::Layers(l) => {
                        let cap = fonts.detail(trs("Layer"), t::RAIL_GROUP_HEADER_SIZE * scale as f32, paint::color(t::RAIL_GROUP_HEADER_COLOR));
                        let top = t::RAIL_GROUP_GAP as f64 * scale - t::RAIL_ROW_GAP as f64 * scale;
                        min_ui::fonts::draw_layout(scene, &cap, kurbo::Affine::translate((r.x0, r.y0 + top)));
                        let ip = t::SPACING_85 as f64 * scale; let rad = t::ACTION_BUTTON_CORNER_RADIUS_S as f64 * scale;
                        for (i, (name, tr)) in l.names.iter().zip(&l.tiles).enumerate() {
                            // selection = the backing one grey step up (the user 2026-11-08: a fill, not QCView's accent border); hover half way
                            let cur = *name == l.current;
                            let bg = if cur { t::GRAY_300 } else if l.hover == Some(i) { t::GRAY_200 } else { min_ui::section::card_color() };
                            paint::rounded(scene, *tr, rad, bg);
                            let img_r = Rect::new(tr.x0 + ip, tr.y0 + ip, tr.x1 - ip, tr.y0 + ip + (tr.width() - 2.0 * ip) * l.aspect);
                            match self.layer_thumbs.get(name) {
                                Some(b) => { let (bw, bh) = (b.image.width as f64, b.image.height as f64); let s = (img_r.width() / bw).min(img_r.height() / bh); let (w, h) = (bw * s, bh * s); paint::image(scene, b, Rect::new(img_r.center().x - w / 2.0, img_r.center().y - h / 2.0, img_r.center().x + w / 2.0, img_r.center().y + h / 2.0)); }
                                None => paint::rounded(scene, img_r, rad / 2.0, t::FIELD_BACKGROUND_COLOR),
                            }
                            let lab = fonts.layout_width(name, Family::Mono, t::CODE_SIZE_S * scale as f32 * 0.9, t::FONT_WEIGHT_REGULAR as f32, paint::color(if cur { t::GRAY_900 } else { t::RAIL_VALUE_COLOR }), Some((tr.width() - 2.0 * ip) as f32));
                            min_ui::fonts::draw_layout(scene, &lab, kurbo::Affine::translate((tr.center().x - lab.width() as f64 / 2.0, img_r.y1 + t::SPACING_75 as f64 * scale)));
                        }
                    }
                    Item::Meters => {
                        // QCView's per-channel peak meters: one 4-px bar per source channel, the track in the card colour, the fill in the content grey, over 0.9 in the notice colour
                        let bar_h = 4.0 * scale; let step = 6.0 * scale; let x0 = r.x0 + LABEL_W * scale * 0.6;
                        let l = fonts.layout(tr("Meters"), Family::Sans, t::RAIL_LABEL_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_VALUE_COLOR));
                        min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((r.x0, r.y0)));
                        for ch in 0..self.channels {
                            let y = r.y0 + ch as f64 * step; let w = r.x1 - x0;
                            paint::rounded(scene, Rect::new(x0, y, r.x1, y + bar_h), bar_h / 2.0, min_ui::section::card_color());
                            let lvl = self.peaks[ch].clamp(0.0, 1.0) as f64;
                            if lvl > 0.0 { paint::rounded(scene, Rect::new(x0, y, x0 + w * lvl, y + bar_h), bar_h / 2.0, if lvl > 0.9 { t::NOTICE_VISUAL_COLOR } else { t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT }); }
                        }
                    }
                }
            }
        }
    }
}
