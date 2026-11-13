//! A progress bar (minNotes' package / Collect Media window, canon row 332): a pill track with a fill from its left
//! end for a known fraction. Spectrum 2's bar in the family's monochrome: the track is the switch's off track, the fill
//! the neutral content colour; a finished one takes the positive colour, a failed one the negative.
use crate::paint;
use kurbo::Rect;
use min_theme::tokens as t;
use vello::Scene;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone { Neutral, Positive, Negative }

/// The bar's height, px.
pub fn height(scale: f64) -> f64 { t::SPACING_75 as f64 * scale }

/// `fraction` 0..=1 of `r` is filled (a started bar shows at least its own height, so a beginning is seen).
pub fn bar(scene: &mut Scene, r: Rect, fraction: f64, tone: Tone) {
    let h = r.height(); paint::rounded(scene, r, h / 2.0, t::SWITCH_TRACK_COLOR_OFF);
    let f = fraction.clamp(0.0, 1.0); if f <= 0.0 { return }
    let w = (r.width() * f).max(h).min(r.width());
    paint::rounded(scene, Rect::new(r.x0, r.y0, r.x0 + w, r.y1), h / 2.0, match tone { Tone::Neutral => t::NEUTRAL_CONTENT_COLOR_DEFAULT, Tone::Positive => t::POSITIVE_VISUAL_COLOR, Tone::Negative => t::NEGATIVE_VISUAL_COLOR });
}
