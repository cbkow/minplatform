//! A SKETCH of the bottom bands drawn straight from `min-theme`'s tokens — the scrubber (the status strip and the transport
//! row are real nodes: `status_strip.rs`, `transport_bar.rs`) (canon §25 rows 166 / 169) — to prove the UI layer end to end: fonts, glyphs, rounded fills, hairlines,
//! the faded waveform, the composite over video at SDR white. Replaced by `min-ui`'s nodes in piece 6b; nothing here
//! is interactive, and every number is a token so the look already is the ruled one.
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use min_ui::{paint, Family, Fonts};
use vello::Scene;

/// `frame` = (current, count, fps) when media is open.
pub fn draw(scene: &mut Scene, fonts: &mut Fonts, w: u32, h: u32, scale: f64, frame: Option<(i32, i32, f64)>) {
    scene.reset();
    let s = scale;
    let (w, h) = (w as f64, h as f64);
    let (status_h, transport_h, scrub_h) = (t::STATUS_STRIP_HEIGHT as f64 * s, t::TRANSPORT_BAR_HEIGHT as f64 * s, t::SCRUBBER_HEIGHT as f64 * s);
    let bands_top = h - status_h - transport_h - scrub_h;
    paint::rect(scene, Rect::new(0.0, bands_top, w, h), t::BAND_BACKGROUND_COLOR);
    let hair = t::DIVIDER_THICKNESS as f64 * s;
    let pad = t::SPACING_100 as f64 * s;

    // ---- status strip: the band's top hairline only — the wells are `status_strip.rs`'s nodes ----
    let y0 = bands_top;
    paint::hline(scene, 0.0, w, y0, hair, t::DIVIDER_COLOR);
    let (cur, count, fps) = frame.unwrap_or((0, 0, 0.0));

    // ---- transport row: the band + hairline only — the buttons are `transport_bar.rs`'s nodes ----
    let y1 = y0 + status_h;
    paint::hline(scene, 0.0, w, y1, hair, t::DIVIDER_COLOR);

    // ---- scrubber: ticks + labels, the faded waveform fill, in / out, the playhead ----
    let y2 = y1 + transport_h;
    paint::hline(scene, 0.0, w, y2, hair, t::DIVIDER_COLOR);
    let (sx0, sx1) = (pad, w - pad);
    let span = sx1 - sx0;
    let dur_s = if count > 0 && fps > 0.0 { count as f64 / fps } else { 60.0 };
    let pps = span / dur_s;
    // the waveform as the band's quiet background: a synthetic envelope until piece 6c reads the probe memo
    let wave_h = scrub_h * t::SCRUBBER_WAVEFORM_HEIGHT_RATIO;
    let mut path = kurbo::BezPath::new();
    path.move_to((sx0, y2 + scrub_h));
    let n = (span / (2.0 * s)).max(2.0) as usize;
    for i in 0..=n {
        let u = i as f64 / n as f64; let x = sx0 + u * span;
        let env = (0.55 + 0.45 * (u * 37.0).sin() * (u * 5.3).cos()).abs() * (1.0 - (u * 2.0 - 1.0).powi(8));
        path.line_to((x, y2 + scrub_h - env * wave_h));
    }
    path.line_to((sx1, y2 + scrub_h)); path.close_path();
    scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, paint::color(t::SCRUBBER_WAVEFORM_COLOR), None, &path);
    // ticks: the largest interval that keeps ≥ tick-min-spacing px (QCView's rule); frame ticks when a frame is ≥ 6 px
    let candidates = [1.0, 5.0, 10.0, 30.0, 60.0, 300.0, 600.0];
    let tick_s = candidates.iter().cloned().find(|c| c * pps >= t::SCRUBBER_TICK_MIN_SPACING as f64 * s).unwrap_or(600.0);
    let mut ts = 0.0;
    while ts <= dur_s + 1e-9 {
        let x = sx0 + ts * pps;
        paint::rect(scene, Rect::new(x, y2 + hair, x + hair, y2 + hair + t::SCRUBBER_TICK_MAJOR_LENGTH as f64 * s), t::SCRUBBER_TICK_COLOR);
        let l = fonts.layout(&ruler_label(ts), Family::Mono, t::SCRUBBER_TICK_LABEL_SIZE * s as f32, t::FONT_WEIGHT_REGULAR as f32, paint::color(t::SCRUBBER_TICK_LABEL_COLOR));
        min_ui::fonts::draw_layout(scene, &l, Affine::translate((x + t::SPACING_75 as f64 * s, y2 + t::SPACING_50 as f64 * s)));
        ts += tick_s;
    }
    if fps > 0.0 && pps / fps >= t::SCRUBBER_FRAME_TICK_MIN_PX_PER_FRAME as f64 * s {
        let mut f = 0.0; while f < dur_s * fps { let x = sx0 + f / fps * pps; paint::rect(scene, Rect::new(x, y2 + hair, x + hair, y2 + hair + t::SCRUBBER_TICK_MINOR_LENGTH as f64 * s), t::SCRUBBER_TICK_COLOR); f += 1.0; }
    }
    // a sample in / out zone at 20 %–55 % (the flags: positive / notice)
    let (ix, ox) = (sx0 + 0.20 * span, sx0 + 0.55 * span);
    paint::rect(scene, Rect::new(ix, y2 + hair, ox, y2 + scrub_h), t::SCRUBBER_IN_OUT_ZONE_COLOR);
    paint::rect(scene, Rect::new(ix, y2 + hair, ix + hair, y2 + scrub_h), t::SCRUBBER_IN_POINT_COLOR);
    paint::rect(scene, Rect::new(ox - hair, y2 + hair, ox, y2 + scrub_h), t::SCRUBBER_OUT_POINT_COLOR);
    let (fw, fh) = (t::SCRUBBER_FLAG_WIDTH as f64 * s, t::SCRUBBER_FLAG_HEIGHT as f64 * s);
    paint::rect(scene, Rect::new(ix, y2 + scrub_h - fh, ix + fw, y2 + scrub_h), t::SCRUBBER_IN_POINT_COLOR);
    paint::rect(scene, Rect::new(ox - fw, y2 + scrub_h - fh, ox, y2 + scrub_h), t::SCRUBBER_OUT_POINT_COLOR);
    // the cache underline (2 px) and the playhead
    paint::rect(scene, Rect::new(sx0, y2 + scrub_h - t::SCRUBBER_CACHE_BAND_THICKNESS as f64 * s, sx0 + 0.35 * span, y2 + scrub_h), t::SCRUBBER_CACHE_AHEAD_COLOR);
    let px = if count > 0 { sx0 + cur as f64 / count as f64 * span } else { sx0 + 0.30 * span };
    paint::rect(scene, Rect::new(px, y2 + hair, px + t::SCRUBBER_PLAYHEAD_WIDTH as f64 * s, y2 + scrub_h), t::SCRUBBER_PLAYHEAD_COLOR);
}

fn ruler_label(secs: f64) -> String { let m = (secs / 60.0).floor() as i64; let s = secs - m as f64 * 60.0; if secs.fract() > 1e-6 { format!("{m}:{s:04.1}") } else { format!("{m}:{:02}", s as i64) } }
