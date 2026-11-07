//! The Inspector in the right rail (QCView's `InspectorPanel.qml` sections, the ones this player can fill today: File,
//! Video, Audio, Timecode — Geometry, Color, Linked Projects and Playlist Contents arrive with their features). Each
//! section is a flat header and label / value rows: label in the subdued colour at 12, value right-aligned at 12 too
//! (§25.4's reference puts values at 14; the user found that too large here), the value in the mono face where it is a number.
use kurbo::Rect;
use min_theme::tokens as t;
use min_ui::{paint, Event, Family, Fonts, Section};
use qcv_player::Transport;
use std::time::Instant;
use vello::Scene;

struct Group { section: Section, rows: Vec<(String, String, bool)>, row_rects: Vec<Rect> }

pub struct InspectorPanel { groups: Vec<Group>, path_seen: String, size_bytes: Option<u64>, height: f64 }

fn fmt_bytes(b: u64) -> String { if b >= 1 << 30 { format!("{:.2} GB", b as f64 / (1u64 << 30) as f64) } else if b >= 1 << 20 { format!("{:.1} MB", b as f64 / (1u64 << 20) as f64) } else { format!("{} KB", b / 1024) } }
fn fmt_rate(n: i32, d: i32) -> String { if d == 0 { return "—".into() } let r = n as f64 / d as f64; if (r - r.round()).abs() < 1e-6 { format!("{} fps", r.round() as i64) } else { format!("{r:.3} fps") } }
fn fmt_dur(s: f64) -> String { let ms = (s * 1000.0).round() as i64; format!("{:02}:{:02}:{:02}.{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000) }

impl InspectorPanel {
    pub fn new() -> InspectorPanel {
        let g = |title| Group { section: Section::new(title, true), rows: vec![], row_rects: vec![] };
        InspectorPanel { groups: vec![g("File"), g("Video"), g("Audio"), g("Timecode")], path_seen: String::new(), size_bytes: None, height: 0.0 }
    }
    /// Refill from the Transport (cheap: strings only; the file size is read once per path).
    pub fn sync(&mut self, p: Option<&Transport>) {
        let Some(p) = p else { for g in &mut self.groups { g.rows.clear(); } return };
        let path = p.path().to_string();
        if path != self.path_seen { self.path_seen = path.clone(); self.size_bytes = std::fs::metadata(&path).ok().map(|m| m.len()); }
        let i = p.info();
        let pb = std::path::Path::new(&path);
        let name = pb.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.clone());
        let folder = pb.parent().map(|d| d.to_string_lossy().to_string()).unwrap_or_default();
        let mut file = vec![("Name".to_string(), name, false), ("Folder".to_string(), folder, false)];
        if let Some(b) = self.size_bytes { file.push(("Size".into(), fmt_bytes(b), true)); }
        file.push(("Duration".into(), fmt_dur(i.duration_secs), true));
        let res = if i.meta.sar != (1, 1) && i.meta.sar.1 != 0 { format!("{} × {}  (SAR {}:{})", i.width, i.height, i.meta.sar.0, i.meta.sar.1) } else { format!("{} × {}", i.width, i.height) };
        let mut video = vec![
            ("Codec".to_string(), i.codec_name.clone(), false),
            ("Resolution".into(), res, true),
            ("Frame rate".into(), fmt_rate(i.frame_rate.0, i.frame_rate.1), true),
            ("Frames".into(), i.frame_count.to_string(), true),
            ("Pixel format".into(), i.pix_fmt.clone(), true),
            ("Bit depth".into(), format!("{}-bit", i.meta.bit_depth), true),
            ("Range".into(), format!("{:?}", i.meta.range), false),
            ("Matrix".into(), format!("{:?}", i.meta.matrix), false),
            ("Transfer".into(), format!("{:?}", i.meta.transfer), false),
            ("Primaries".into(), format!("{:?}", i.meta.primaries), false),
            ("Decode".into(), if i.hw_accel.is_empty() { "software".to_string() } else { i.hw_accel.clone() }, false),
            ("GOP".into(), if i.intra_only { "intra-only" } else { "inter" }.to_string(), false),
        ];
        if i.meta.rotation != 0 { video.push(("Rotation".into(), format!("{}°", i.meta.rotation), true)); }
        let audio = if p.audio.has_audio() {
            vec![("Channels".to_string(), p.audio.source_channels().to_string(), true), ("Layout".into(), p.audio.layout_name(), false), ("Device latency".into(), format!("{:.1} ms", p.audio.device_latency_ms()), true), ("Volume".into(), format!("{} %{}", (p.audio.volume() * 100.0).round() as i32, if p.audio.is_muted() { "  (muted)" } else { "" }), true)]
        } else { vec![("Audio".to_string(), "none".to_string(), false)] };
        let tc = vec![("Source".to_string(), i.source_timecode.clone().unwrap_or_else(|| "none".into()), true), ("Drop frame".into(), if i.drop_frame { "yes" } else { "no" }.to_string(), false)];
        for (g, rows) in self.groups.iter_mut().zip([file, video, audio, tc]) { g.rows = rows; }
    }
    pub fn layout(&mut self, fonts: &mut Fonts, x0: f64, x1: f64, y: f64, scale: f64) -> f64 {
        let hh = Section::height(scale); let pad = t::SPACING_100 as f64 * scale; let gap = t::RAIL_ROW_GAP as f64 * scale;
        let row_h = t::COMPONENT_HEIGHT_75 as f64 * scale;
        let mut cy = y;
        let _ = fonts;
        for g in &mut self.groups {
            g.section.rect = Rect::new(x0, cy, x1, cy + hh); cy += hh;
            g.row_rects.clear();
            if g.section.expanded {
                cy += pad / 2.0;
                for _ in &g.rows { g.row_rects.push(Rect::new(x0 + pad, cy, x1 - pad, cy + row_h)); cy += row_h + gap; }
                cy += pad;
            }
        }
        self.height = cy - y;
        self.height
    }
    pub fn event(&mut self, e: &Event, now: Instant) -> bool {
        let mut dirty = false;
        for g in &mut self.groups { let (_, d) = g.section.event(e, now); dirty |= d; }
        dirty
    }
    pub fn animating(&self, now: Instant) -> bool { self.groups.iter().any(|g| g.section.animating(now)) }
    pub fn paint(&self, scene: &mut Scene, fonts: &mut Fonts, scale: f64, now: Instant) {
        for (gi, g) in self.groups.iter().enumerate() {
            g.section.paint(scene, fonts, scale, now, gi == 0);
            if !g.section.expanded { continue }
            for ((label, value, mono), r) in g.rows.iter().zip(&g.row_rects) {
                let l = fonts.layout(label, Family::Sans, t::RAIL_LABEL_SIZE * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_VALUE_COLOR));
                min_ui::fonts::draw_layout(scene, &l, kurbo::Affine::translate((r.x0, r.y0 + (r.height() - l.height() as f64) / 2.0)));
                let avail = (r.width() - l.width() as f64 - t::SPACING_200 as f64 * scale).max(40.0);
                let fam = if *mono { Family::Mono } else { Family::Sans }; let size = if *mono { t::CODE_SIZE_S } else { t::BODY_SIZE_S };   // 12, with the labels (the user 2026-11-07: 14 "too large of a font" for the values)
                let mut v = fonts.layout(value, fam, size * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_LABEL_COLOR));
                if v.width() as f64 > avail {
                    // elide from the left for paths, from the right otherwise (QCView: folders show their tail)
                    let chars: Vec<char> = value.chars().collect(); let mut lo = 0usize; let mut hi = chars.len();
                    while lo < hi { let mid = (lo + hi).div_ceil(2); let s: String = if label == "Folder" { format!("…{}", chars[chars.len() - mid..].iter().collect::<String>()) } else { format!("{}…", chars[..mid].iter().collect::<String>()) }; let t2 = fonts.layout(&s, fam, size * scale as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::RAIL_LABEL_COLOR)); if t2.width() as f64 <= avail { lo = mid; v = t2; } else { hi = mid - 1; } }
                }
                min_ui::fonts::draw_layout(scene, &v, kurbo::Affine::translate((r.x1 - v.width() as f64, r.y0 + (r.height() - v.height() as f64) / 2.0)));
            }
        }
    }
}
