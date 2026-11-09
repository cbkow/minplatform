//! Text: parley over the faces `min-theme` bundles — Geist Sans, Geist Mono, Phosphor (canon §25 row 161).
use parley::{Alignment, AlignmentOptions, FontContext, FontFamily, FontFamilyName, FontWeight, Layout, LayoutContext, LineHeight, PositionedLayoutItem, StyleProperty};
use peniko::{Blob, Brush, Color};
use std::{borrow::Cow, sync::Arc};

/// A shaped text layout as the panels hold it.
pub type TextLayout = Layout<Brush>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family { Sans, Mono, Icon }

pub struct Fonts {
    pub font_cx: FontContext,
    pub layout_cx: LayoutContext<Brush>,
    sans: String, mono: String, icon: String,
}

impl Fonts {
    /// Registers the bundled faces; the family names are read back from the registration (they match
    /// `min_theme::fonts::FAMILY_*`, verified in the crate's tests).
    pub fn new() -> Fonts {
        let mut font_cx = FontContext::new();
        let mut names: Vec<String> = vec![];
        for (_, bytes) in min_theme::fonts::ALL {
            for (id, _) in font_cx.collection.register_fonts(Blob::new(Arc::new(*bytes)), None) {
                if let Some(n) = font_cx.collection.family_name(id) { let n = n.to_string(); if !names.contains(&n) { names.push(n); } }
            }
        }
        let pick = |want: &str| names.iter().find(|n| n.as_str() == want).cloned().unwrap_or_else(|| { log::warn!("min-ui: family {want:?} not among the registered {names:?}"); want.to_string() });
        let (sans, mono, icon) = (pick(min_theme::fonts::FAMILY_SANS), pick(min_theme::fonts::FAMILY_MONO), pick(min_theme::fonts::FAMILY_ICON));
        Fonts { font_cx, layout_cx: LayoutContext::new(), sans, mono, icon }
    }
    pub fn family_name(&self, f: Family) -> &str { match f { Family::Sans => &self.sans, Family::Mono => &self.mono, Family::Icon => &self.icon } }

    /// A single-style layout: `size_px` is in physical pixels (pass logical × scale), `weight` 400 / 700, no wrapping.
    pub fn layout(&mut self, text: &str, family: Family, size_px: f32, weight: f32, color: Color) -> Layout<Brush> {
        self.layout_width(text, family, size_px, weight, color, None)
    }
    /// As `layout`, wrapping at `max_width` physical pixels when given.
    pub fn layout_width(&mut self, text: &str, family: Family, size_px: f32, weight: f32, color: Color, max_width: Option<f32>) -> Layout<Brush> {
        let name = self.family_name(family).to_string();
        // Geist has no ⌘ / ⌥ (they drew as boxes in tooltips and the shortcuts card): those two come from Phosphor's
        // `command` / `option`, swapped in as ranges of the icon family
        let (text, marks) = mac_glyphs(text, family);
        let text = text.as_str();
        let icon = self.icon.clone();
        let mut b = self.layout_cx.ranged_builder(&mut self.font_cx, text, 1.0, true);
        b.push_default(StyleProperty::FontFamily(FontFamily::Single(FontFamilyName::Named(Cow::Owned(name)))));
        b.push_default(StyleProperty::FontSize(size_px));
        b.push_default(StyleProperty::FontWeight(FontWeight::new(weight)));
        b.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(min_theme::tokens::UI_LINE_HEIGHT as f32)));
        b.push_default(StyleProperty::Brush(Brush::Solid(color)));
        b.push_default(StyleProperty::Locale(ui_locale()));
        for r in marks { b.push(StyleProperty::FontFamily(FontFamily::Single(FontFamilyName::Named(Cow::Owned(icon.clone())))), r.clone()); b.push(StyleProperty::FontWeight(FontWeight::new(400.0)), r); }
        let mut layout = b.build(text);
        layout.break_all_lines(max_width);
        layout.align(Alignment::Start, AlignmentOptions::default());
        layout
    }
    /// As `layout`, cut with "…" to fit `max_w` physical px (a long translation in a fixed control: the walk 2026-11-09,
    /// German "Dunkles Schachbrett" ran out of its chip). The whole text when it fits.
    pub fn layout_fit(&mut self, text: &str, family: Family, size_px: f32, weight: f32, color: Color, max_w: f64) -> Layout<Brush> {
        let full = self.layout(text, family, size_px, weight, color);
        if full.width() as f64 <= max_w || max_w <= 0.0 { return full }
        let chars: Vec<char> = text.chars().collect();
        let (mut lo, mut hi) = (0usize, chars.len());
        while lo < hi { let mid = (lo + hi).div_ceil(2); let s: String = chars[..mid].iter().collect::<String>().trim_end().to_string() + "…"; if self.layout(&s, family, size_px, weight, color).width() as f64 <= max_w { lo = mid; } else { hi = mid - 1; } }
        let s: String = chars[..lo].iter().collect::<String>().trim_end().to_string() + "…";
        self.layout(&s, family, size_px, weight, color)
    }
    /// A tracked-capitals "detail" layout (Spectrum 2's detail style; the go-to popover's title): bold, `letter_spacing` in em.
    pub fn detail(&mut self, text: &str, size_px: f32, color: Color) -> Layout<Brush> {
        let upper = text.to_uppercase();
        let name = self.sans.clone();
        let mut b = self.layout_cx.ranged_builder(&mut self.font_cx, &upper, 1.0, true);
        b.push_default(StyleProperty::FontFamily(FontFamily::Single(FontFamilyName::Named(Cow::Owned(name)))));
        b.push_default(StyleProperty::FontSize(size_px));
        b.push_default(StyleProperty::FontWeight(FontWeight::new(min_theme::tokens::FONT_WEIGHT_BOLD as f32)));
        b.push_default(StyleProperty::LetterSpacing(size_px * min_theme::tokens::DETAIL_LETTER_SPACING));
        b.push_default(StyleProperty::Locale(ui_locale()));
        b.push_default(StyleProperty::LineHeight(LineHeight::FontSizeRelative(min_theme::tokens::UI_LINE_HEIGHT as f32)));
        b.push_default(StyleProperty::Brush(Brush::Solid(color)));
        let mut layout = b.build(&upper);
        layout.break_all_lines(None);
        layout.align(Alignment::Start, AlignmentOptions::default());
        layout
    }
    /// A Phosphor glyph as a layout (`None` for an unknown slug — QCView's blank-gap rule, the caller draws nothing).
    pub fn icon(&mut self, slug: &str, size_px: f32, color: Color) -> Option<Layout<Brush>> {
        let ch = min_theme::icons::glyph(slug)?;
        Some(self.layout(&ch.to_string(), Family::Icon, size_px, 400.0, color))
    }
}

/// The interface language as the text's locale: the system fallback for Han characters follows it (rhombus 2026-11-09:
/// without it Windows drew Simplified Chinese from a Japanese face — 设 / 项 / 显 came out as boxes).
pub fn ui_locale() -> Option<parley::Language> {
    let code = min_i18n::language();
    if code == "en" { return None }
    parley::Language::parse(&code.replace('_', "-")).ok()
}

/// `text` with ⌘ / ⌥ swapped for Phosphor's glyphs, and the byte ranges they occupy (none for the icon family).
fn mac_glyphs(text: &str, family: Family) -> (String, Vec<std::ops::Range<usize>>) {
    if family == Family::Icon || !text.contains(['⌘', '⌥']) { return (text.to_string(), vec![]) }
    let mut out = String::with_capacity(text.len()); let mut marks = vec![];
    for ch in text.chars() {
        let sub = match ch { '⌘' => min_theme::icons::glyph("command"), '⌥' => min_theme::icons::glyph("option"), _ => None };
        match sub { Some(g) => { let at = out.len(); out.push(g); marks.push(at..out.len()); } None => out.push(ch) }
    }
    (out, marks)
}

impl Default for Fonts { fn default() -> Self { Fonts::new() } }

/// Draw a layout's glyph runs into `scene` with its origin at `transform` (S3's loop, hinting on).
pub fn draw_layout(scene: &mut vello::Scene, layout: &Layout<Brush>, transform: kurbo::Affine) {
    for line in layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(gr) = item else { continue };
            let run = gr.run(); let style = gr.style();
            let coords: Vec<i16> = run.normalized_coords().iter().map(|c| c.to_bits()).collect();
            scene.draw_glyphs(run.font()).font_size(run.font_size()).normalized_coords(&coords).brush(&style.brush).hint(true).transform(transform)
                .draw(peniko::Fill::NonZero, gr.positioned_glyphs().map(|g| vello::Glyph { id: g.id.into(), x: g.x, y: g.y }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bundled_families_register_under_their_names() {
        let mut f = Fonts::new();
        assert_eq!(f.family_name(Family::Sans), min_theme::fonts::FAMILY_SANS);
        assert_eq!(f.family_name(Family::Mono), min_theme::fonts::FAMILY_MONO);
        assert_eq!(f.family_name(Family::Icon), min_theme::fonts::FAMILY_ICON);
        let l = f.layout("Frame 12 / 240", Family::Mono, 12.0, 400.0, Color::WHITE);
        assert!(l.width() > 60.0 && l.height() > 12.0, "{} × {}", l.width(), l.height());
        assert!(f.icon("play", 20.0, Color::WHITE).is_some());
        assert!(f.icon("no-such-icon", 20.0, Color::WHITE).is_none());
    }
}
