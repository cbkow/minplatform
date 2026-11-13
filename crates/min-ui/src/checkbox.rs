//! A checkbox (minNotes' package / Collect Media window, canon row 332): Spectrum 2's 14-px box, radius 4 - empty
//! with a 2-px border, or filled with the tick (`Some(true)`) or the dash of a group that is partly chosen (`None`).
//! Drawn, not held: the list that owns it knows its state and where it is.
use crate::{fonts::draw_layout, paint, Fonts};
use kurbo::{Affine, Rect};
use min_theme::tokens as t;
use vello::Scene;

/// The box's side, px.
pub fn size(scale: f64) -> f64 { t::SWITCH_TRACK_HEIGHT as f64 * scale }

/// `state`: Some(on) or None for a mixed group. `r` is the box (a square of [`size`]).
pub fn paint_box(scene: &mut Scene, fonts: &mut Fonts, r: Rect, state: Option<bool>, enabled: bool, hover: bool, scale: f64) {
    let rad = t::SPACING_75 as f64 * scale;
    let edge = if !enabled { t::SWITCH_TRACK_COLOR_DISABLED } else if hover { t::NEUTRAL_CONTENT_COLOR_HOVER } else if state == Some(false) { t::NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT } else { t::NEUTRAL_CONTENT_COLOR_DEFAULT };
    paint::rounded(scene, r, rad, edge);
    if state == Some(false) || !enabled { let b = t::SPACING_50 as f64 * scale; paint::rounded(scene, r.inset(-b), (rad - b).max(0.0), t::DIALOG_BACKGROUND_COLOR); if state == Some(false) { return } }
    let ink = if enabled { t::NEUTRAL_CONTENT_COLOR_INVERTED } else { t::SWITCH_HANDLE_COLOR_DISABLED };
    if let Some(g) = fonts.icon(if state.is_none() { "minus" } else { "check" }, (r.width() * 0.8) as f32, paint::color(ink)) {
        draw_layout(scene, &g, Affine::translate((r.center().x - g.width() as f64 / 2.0, r.center().y - g.height() as f64 / 2.0)));
    }
}
