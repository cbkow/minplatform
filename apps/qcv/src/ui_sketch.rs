//! The bottom bands' backdrop: the status strip and the transport row share one layer-1 fill with a hairline on top
//! (the scrubber paints its own band; the wells and buttons are nodes — `status_strip.rs`, `transport_bar.rs`).
use kurbo::Rect;
use min_theme::tokens as t;
use min_ui::paint;
use vello::Scene;

pub fn bands(scene: &mut Scene, w: u32, h: u32, scale: f64) {
    scene.reset();
    let (w, h) = (w as f64, h as f64);
    let scrub_h = t::SCRUBBER_HEIGHT as f64 * scale;
    let top = h - scrub_h - (t::STATUS_STRIP_HEIGHT + t::TRANSPORT_BAR_HEIGHT) as f64 * scale;
    paint::rect(scene, Rect::new(0.0, top, w, h - scrub_h), t::BAND_BACKGROUND_COLOR);
    let hair = t::DIVIDER_THICKNESS as f64 * scale;
    paint::hline(scene, 0.0, w, top, hair, t::DIVIDER_COLOR);
    paint::hline(scene, 0.0, w, top + t::STATUS_STRIP_HEIGHT as f64 * scale, hair, t::DIVIDER_COLOR);
}
