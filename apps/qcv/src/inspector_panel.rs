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
use min_ui::{paint, ActionButton, ActionGroup, Event, Family, Fonts, Response, Section};
use qcv_meta::{AdobeMetadata, VideoMetadata};
use qcv_player::{audio::RoutingMode, RangeOverride, Transport};
use std::time::Instant;
use vello::Scene;

#[derive(Clone, Debug, PartialEq)]
pub enum Cmd { Range(RangeOverride), Routing(RoutingMode), Origin(String) }

struct Row { label: String, value: String, note: String, mono: bool, warn: bool }
/// QCView's `KvChipRow`: a label and a fused row of choices, one selected.
struct Chips { label: String, group: ActionGroup, keys: Vec<String>, selected: usize, rect: Rect }
enum Item { Row(Row), Chips(Chips), Meters }
struct Group { section: Section, items: Vec<Item>, rects: Vec<Rect> }

pub struct InspectorPanel {
    groups: Vec<Group>,
    path: String, size_bytes: Option<u64>,
    vmeta: Option<VideoMetadata>, ameta: Option<AdobeMetadata>,
    peaks: [f32; 16], channels: usize,
    range: RangeOverride, routing: RoutingMode, origin: Option<String>,
    height: f64, dirty_model: bool,
}

const LABEL_W: f64 = 96.0;   // QCView's labelColWidth (logical)

fn fmt_bytes(b: u64) -> String { if b >= 1 << 30 { format!("{:.2} GB", b as f64 / (1u64 << 30) as f64) } else if b >= 1 << 20 { format!("{:.1} MB", b as f64 / (1u64 << 20) as f64) } else { format!("{} KB", b / 1024) } }
fn fmt_dur(s: f64) -> String { let ms = (s * 1000.0).round() as i64; format!("{:02}:{:02}:{:02}.{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000) }
/// QCView `formatBitrate`.
fn fmt_bitrate(bps: i64) -> String { if bps <= 0 { String::new() } else if bps >= 1_000_000_000 { format!("{:.2} Gb/s", bps as f64 / 1e9) } else if bps >= 1_000_000 { format!("{:.1} Mb/s", bps as f64 / 1e6) } else if bps >= 1000 { format!("{:.0} kb/s", bps as f64 / 1e3) } else { format!("{bps} b/s") } }

impl InspectorPanel {
    pub fn new() -> InspectorPanel {
        let g = |title| Group { section: Section::new(title, true), items: vec![], rects: vec![] };
        InspectorPanel { groups: vec![g("File"), g("Video"), g("Geometry"), g("Color"), g("Audio"), g("Timecode")], path: String::new(), size_bytes: None, vmeta: None, ameta: None, peaks: [0.0; 16], channels: 0,
            range: RangeOverride::Auto, routing: RoutingMode::Auto, origin: None, height: 0.0, dirty_model: true }
    }
    /// The extractor's result for `path` (the service delivers it after the open).
    pub fn set_video_meta(&mut self, path: &str, m: VideoMetadata) { if path == self.path { self.vmeta = Some(m); self.dirty_model = true; } }
    pub fn set_adobe_meta(&mut self, path: &str, m: AdobeMetadata) { if path == self.path { self.ameta = Some(m); self.dirty_model = true; } }
    /// Refill from the Transport when something it shows changed; the meters read every frame.
    pub fn sync(&mut self, p: Option<&Transport>) -> bool {
        let Some(p) = p else { if !self.path.is_empty() { self.path.clear(); self.vmeta = None; self.ameta = None; for g in &mut self.groups { g.items.clear(); } self.dirty_model = false; return true } return false };
        let path = p.path().to_string();
        if path != self.path { self.path = path.clone(); self.size_bytes = std::fs::metadata(&path).ok().map(|m| m.len()); self.vmeta = None; self.ameta = None; self.dirty_model = true; }
        let (range, routing) = (p.range_override(), p.audio.routing_mode());
        if range != self.range || routing != self.routing { self.range = range; self.routing = routing; self.dirty_model = true; }
        let mut dirty = false;
        if p.audio.has_audio() {
            let pk = p.audio.peak_levels(); let ch = p.audio.source_channels().min(16);
            if ch != self.channels { self.channels = ch; self.dirty_model = true; }
            if (0..ch).any(|i| (pk[i] - self.peaks[i]).abs() > 0.01) { self.peaks = pk; dirty = true; }
        } else if self.channels != 0 { self.channels = 0; self.dirty_model = true; }
        if self.dirty_model { self.rebuild(p); self.dirty_model = false; dirty = true; }
        dirty
    }
    fn rebuild(&mut self, p: &Transport) {
        let i = p.info(); let v = self.vmeta.as_ref(); let a = self.ameta.as_ref();
        let row = |label: &str, value: String, mono: bool| Item::Row(Row { label: label.into(), value, note: String::new(), mono, warn: false });
        let row_n = |label: &str, value: String, note: String, mono: bool| Item::Row(Row { label: label.into(), value, note, mono, warn: false });
        let chips = |label: &str, opts: &[(&str, &str)], selected_key: &str| {
            let buttons = opts.iter().map(|(_, l)| { let mut b = ActionButton::new("", l).size_s(); b.label = Some(l.to_string()); b }).collect();
            let keys: Vec<String> = opts.iter().map(|(k, _)| k.to_string()).collect();
            let selected = keys.iter().position(|k| k == selected_key).unwrap_or(0);
            Item::Chips(Chips { label: label.into(), group: ActionGroup::new(buttons).compact(), keys, selected, rect: Rect::ZERO })
        };
        let pb = std::path::Path::new(&self.path);
        let name = pb.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| self.path.clone());
        let folder = pb.parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default();
        // ---- File ----
        let mut file = vec![row("Name", name, false), row("Folder", folder, false)];
        if let Some(b) = self.size_bytes { file.push(row("Size", fmt_bytes(b), true)); }
        file.push(row("Duration", fmt_dur(v.map(|m| m.duration).filter(|d| *d > 0.0).unwrap_or(i.duration_secs)), true));
        if let Some(m) = v {
            if !m.container_format.is_empty() { file.push(row("Container", if m.mxf_operational_pattern.is_empty() { m.container_format.clone() } else { format!("{} · {}", m.container_format, m.mxf_operational_pattern) }, false)); }
            if !m.encoder_tool.is_empty() { file.push(row("Encoder", m.encoder_tool.clone(), false)); }
        }
        // ---- Video ----
        let (w, h) = v.map(|m| (m.width as u32, m.height as u32)).filter(|(w, _)| *w > 0).unwrap_or((i.width, i.height));
        let fps = v.map(|m| m.frame_rate).filter(|f| *f > 0.0).unwrap_or(i.rate().as_f64());
        let mut video = vec![
            row("Resolution", format!("{w} × {h}"), true),
            row("Frame rate", format!("{fps:.3} fps"), true),
            row("Total frames", v.map(|m| m.total_frames).filter(|n| *n > 0).unwrap_or(i.frame_count).to_string(), true),
            row("Codec", v.map(|m| m.video_codec.clone()).filter(|c| !c.is_empty()).unwrap_or_else(|| i.codec_name.clone()), false),
        ];
        if let Some(m) = v {
            if !m.codec_profile.is_empty() { video.push(row("Profile", m.codec_profile.clone(), false)); }
            if m.video_bitrate > 0 || m.container_bitrate > 0 {
                let (value, note) = if m.video_bitrate > 0 { (fmt_bitrate(m.video_bitrate), if m.container_bitrate > 0 { format!("file avg {}", fmt_bitrate(m.container_bitrate)) } else { String::new() }) } else { (fmt_bitrate(m.container_bitrate), "file avg".to_string()) };
                video.push(row_n("Bitrate", value, note, true));
            }
            video.push(row_n("Pixel format", m.pixel_format.clone(), if m.is_rgb { "RGB".into() } else { String::new() }, true));
            video.push(row("Bit depth", if m.has_alpha { format!("{} bit + alpha", m.bit_depth) } else { format!("{} bit", m.bit_depth) }, true));
            if m.is_hdr_content { video.push(Item::Row(Row { label: "HDR".into(), value: format!("Yes — {}", m.color_transfer), note: String::new(), mono: false, warn: true })); }
        } else {
            video.push(row("Pixel format", i.pix_fmt.clone(), true));
            video.push(row("Bit depth", format!("{} bit", i.meta.bit_depth), true));
        }
        video.push(row("Decode", if i.hw_accel.is_empty() { "software".to_string() } else { i.hw_accel.clone() }, false));   // ours: the decode path (not in QCView's panel)
        // ---- Geometry (the read-outs; the Aspect + Rotation pickers wait for their engine stages) ----
        let (sn, sd, anam) = v.map(|m| (m.sar_num, m.sar_den, m.is_anamorphic)).unwrap_or((i.meta.sar.0 as i32, i.meta.sar.1 as i32, i.meta.sar != (1, 1)));
        let rot = v.map(|m| m.rotation_deg).unwrap_or(i.meta.rotation as i32);
        let mut geometry = vec![row("Detected", if anam && sd > 0 { format!("{sn}:{sd} (non-square)") } else { "1:1 (square)".into() }, true)];
        if w > 0 && h > 0 {
            // QCView: Square renders stored pixels untouched (the default); the Display row shows the effective aspect + dims, swapped for 90 / 270
            let swap = rot == 90 || rot == 270;
            let (dw, dh) = if swap { (h, w) } else { (w, h) };
            geometry.push(row("Display", format!("{:.3}:1   ({dw} × {dh})", dw as f64 / dh as f64), true));
        }
        geometry.push(row("Rotation", if rot > 0 { format!("Auto ({rot}°)") } else { "Auto".into() }, false));
        // ---- Color ----
        let mut color = vec![];
        if let Some(m) = v {
            if !m.colorspace.is_empty() { color.push(row("Colorspace", m.colorspace.clone(), false)); }
            if !m.color_primaries.is_empty() { color.push(row("Primaries", m.color_primaries.clone(), false)); }
            if !m.color_transfer.is_empty() { color.push(row("Transfer", m.color_transfer.clone(), false)); }
            if m.max_cll > 0 || m.mastering_max_nits > 0.0 {
                let mut parts = vec![]; if m.max_cll > 0 { parts.push(format!("MaxCLL {}", m.max_cll)); } if m.max_fall > 0 { parts.push(format!("MaxFALL {}", m.max_fall)); } if m.mastering_max_nits > 0.0 { parts.push(format!("mastering {} nits", m.mastering_max_nits.round() as i64)); }
                color.push(row("HDR metadata", parts.join(", "), true));
            }
            if !m.nclc_tag.is_empty() { color.push(row("NCLC tag", m.nclc_tag.clone(), true)); }
            let (value, note) = if !m.container_range_tag.is_empty() { (m.container_range_tag.clone(), String::new()) } else if !m.color_range.is_empty() { ("untagged".to_string(), format!("decoder: {}", m.color_range)) } else { ("untagged".to_string(), if m.is_rgb { "Auto assumes full" } else { "Auto assumes limited" }.to_string()) };
            color.push(row_n("Range tag", value, note, false));
        } else {
            color.push(row("Colorspace", format!("{:?}", i.meta.matrix), false)); color.push(row("Primaries", format!("{:?}", i.meta.primaries), false)); color.push(row("Transfer", format!("{:?}", i.meta.transfer), false));
        }
        // the Range picker: Auto labels itself with the detected range (QCView's rule)
        let detected = v.map(|m| if m.color_range == "full" { "Auto (Full)" } else if m.color_range == "limited" { "Auto (Limited)" } else if m.container_range_tag == "full" { "Auto (Full)" } else if m.container_range_tag == "limited" { "Auto (Limited)" } else if m.is_rgb { "Auto (Full, assumed)" } else { "Auto (Limited, assumed)" }).unwrap_or("Auto");
        color.push(chips("Range", &[("auto", detected), ("full", "Full"), ("limited", "Limited")], match self.range { RangeOverride::Auto => "auto", RangeOverride::Full => "full", RangeOverride::Limited => "limited" }));
        // ---- Audio ----
        let mut audio = vec![];
        let has_audio = p.audio.has_audio() || v.is_some_and(|m| !m.audio_codec.is_empty());
        if has_audio {
            if let Some(m) = v { if !m.audio_codec.is_empty() { audio.push(row("Codec", m.audio_codec.clone(), false)); } if m.audio_sample_rate > 0 { audio.push(row("Sample rate", format!("{} Hz", m.audio_sample_rate), true)); } }
            let ch = v.map(|m| m.audio_channels as usize).filter(|c| *c > 0).unwrap_or(p.audio.source_channels());
            audio.push(row("Channels", match ch { 2 => "2 (stereo)".to_string(), 1 => "1 (mono)".to_string(), n => n.to_string() }, true));
            let layout = v.map(|m| m.audio_channel_layout_name.clone()).filter(|l| !l.is_empty()).unwrap_or_else(|| p.audio.layout_name());
            if !layout.is_empty() { audio.push(row("Layout", layout, false)); }
            if p.audio.has_audio() {
                let mut opts: Vec<(&str, &str)> = vec![("auto", "Auto")]; if ch >= 6 { opts.push(("5.1", "5.1 → 2")); } if ch >= 8 { opts.push(("7-8", "7-8")); }
                if opts.len() > 1 { audio.push(chips("Mix", &opts, match self.routing { RoutingMode::Auto => "auto", RoutingMode::Downmix5_1 => "5.1", RoutingMode::Stereo7_8 => "7-8" })); }
                audio.push(Item::Meters);
                audio.push(row("Device latency", format!("{:.1} ms", p.audio.device_latency_ms()), true));   // ours
            }
        } else { audio.push(row("Audio", "none".into(), false)); }
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
            let note = if fmt.is_valid() { format!("(frame {})", fmt.parse(e)) } else { String::new() };
            tc.push(row_n("Source", e.clone(), note, true));
        }
        if let Some(am) = a {
            for (label, val) in [("QT Start", &am.qt_start_timecode), ("QT TimeCode", &am.qt_timecode), ("XMP Alt", &am.xmp_alt_timecode)] { if !val.is_empty() { tc.push(row(label, val.clone(), true)); } }
            for (label, val) in [("Created", &am.qt_creation_date), ("Media Created", &am.qt_media_create_date)] { if !val.is_empty() { tc.push(row(label, val.clone(), false)); } }
        }
        tc.push(row("Drop frame", if i.drop_frame { "yes" } else { "no" }.to_string(), false));   // ours
        for (g, items) in self.groups.iter_mut().zip([file, video, geometry, color, audio, tc]) { g.items = items; }
    }
    pub fn layout(&mut self, fonts: &mut Fonts, x0: f64, x1: f64, y: f64, scale: f64) -> f64 {
        let hh = Section::height(scale); let pad = t::SPACING_100 as f64 * scale; let gap = t::RAIL_ROW_GAP as f64 * scale;
        let row_h = t::COMPONENT_HEIGHT_75 as f64 * scale;
        let mut cy = y;
        for g in &mut self.groups {
            g.section.rect = Rect::new(x0, cy, x1, cy + hh); cy += hh;
            g.rects.clear();
            if !g.section.expanded || g.items.is_empty() { continue }
            cy += pad / 2.0;
            for it in g.items.iter_mut() {
                let h = match it {
                    Item::Row(_) => row_h,
                    Item::Chips(c) => { let gw = c.group.layout(fonts, 0.0, 0.0, scale); let cx = (x1 - pad - gw).max(x0 + pad + LABEL_W * scale); c.group.layout(fonts, cx, cy, scale); c.rect = Rect::new(x0 + pad, cy, x1 - pad, cy + c.group.rect.height()); c.group.rect.height() + gap }
                    Item::Meters => 6.0 * scale * self.channels.max(1) as f64 + gap,
                };
                g.rects.push(Rect::new(x0 + pad, cy, x1 - pad, cy + h)); cy += h + gap;
            }
            cy += pad;
        }
        self.height = cy - y;
        self.height
    }
    /// Returns (commands, redraw needed).
    pub fn event(&mut self, e: &Event, now: Instant) -> (Vec<Cmd>, bool) {
        let mut dirty = false; let mut cmds = vec![];
        for g in &mut self.groups {
            let (_, d) = g.section.event(e, now); dirty |= d;
            if !g.section.expanded { continue }
            for it in g.items.iter_mut() { if let Item::Chips(c) = it {
                for (i, r) in c.group.event(e, now) { dirty = true; if r == Response::Clicked && i != c.selected {
                    c.selected = i;
                    let key = c.keys[i].as_str();
                    match c.label.as_str() {
                        "Range" => cmds.push(Cmd::Range(match key { "full" => RangeOverride::Full, "limited" => RangeOverride::Limited, _ => RangeOverride::Auto })),
                        "Mix" => cmds.push(Cmd::Routing(match key { "5.1" => RoutingMode::Downmix5_1, "7-8" => RoutingMode::Stereo7_8, _ => RoutingMode::Auto })),
                        "Origin" => { self.origin = Some(key.to_string()); cmds.push(Cmd::Origin(key.to_string())); }
                        _ => {}
                    }
                } }
                for (j, b) in c.group.buttons.iter_mut().enumerate() { b.selected = j == c.selected; }
            } }
        }
        (cmds, dirty)
    }
    pub fn animating(&self, now: Instant) -> bool { self.groups.iter().any(|g| g.section.animating(now) || g.items.iter().any(|it| matches!(it, Item::Chips(c) if c.group.animating(now)))) }
    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, now: Instant) {
        for (gi, g) in self.groups.iter().enumerate() {
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
                            while lo < hi { let mid = (lo + hi).div_ceil(2); let s: String = if row.label == "Folder" { format!("…{}", chars[chars.len() - mid..].iter().collect::<String>()) } else { format!("{}…", chars[..mid].iter().collect::<String>()) }; let t2 = fonts.layout(&s, fam, size * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(vc)); if t2.width() as f64 <= avail_v { lo = mid; v = t2; } else { hi = mid - 1; } }
                        }
                        let vx = r.x1 - v.width() as f64;
                        min_ui::fonts::draw_layout(scene, &v, kurbo::Affine::translate((vx, r.y0 + (r.height() - v.height() as f64) / 2.0)));
                        if !row.note.is_empty() { let n = fonts.layout(&row.note, Family::Sans, t::DETAIL_SIZE_S * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::DISABLED_CONTENT_COLOR)); min_ui::fonts::draw_layout(scene, &n, kurbo::Affine::translate((vx - t::SPACING_100 as f64 * scale - n.width() as f64, r.y0 + (r.height() - n.height() as f64) / 2.0))); }
                    }
                    Item::Chips(c) => {
                        let l = fonts.layout(&c.label, Family::Sans, t::RAIL_LABEL_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_VALUE_COLOR));
                        min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((r.x0, c.group.rect.y0 + (c.group.rect.height() - l.height() as f64) / 2.0)));
                        paint::rounded(scene, c.group.rect, t::ACTION_BUTTON_CORNER_RADIUS_S as f64 * scale, t::RAIL_CARD_BACKGROUND_COLOR);
                        c.group.paint(scene, fonts, scale, now);
                    }
                    Item::Meters => {
                        // QCView's per-channel peak meters: one 4-px bar per source channel, the track in the card colour, the fill in the content grey, over 0.9 in the notice colour
                        let bar_h = 4.0 * scale; let step = 6.0 * scale; let x0 = r.x0 + LABEL_W * scale * 0.6;
                        let l = fonts.layout("Meters", Family::Sans, t::RAIL_LABEL_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_VALUE_COLOR));
                        min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((r.x0, r.y0)));
                        for ch in 0..self.channels {
                            let y = r.y0 + ch as f64 * step; let w = r.x1 - x0;
                            paint::rounded(scene, Rect::new(x0, y, r.x1, y + bar_h), bar_h / 2.0, t::RAIL_CARD_BACKGROUND_COLOR);
                            let lvl = self.peaks[ch].clamp(0.0, 1.0) as f64;
                            if lvl > 0.0 { paint::rounded(scene, Rect::new(x0, y, x0 + w * lvl, y + bar_h), bar_h / 2.0, if lvl > 0.9 { t::NOTICE_VISUAL_COLOR } else { t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT }); }
                        }
                    }
                }
            }
        }
    }
}
