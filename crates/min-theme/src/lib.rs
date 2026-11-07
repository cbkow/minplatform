//! `min-theme` — the family's design tokens as data (canon `docs/rust-wgpu-port.md` §25, rows 160–169).
//!
//! The source of truth is `tokens/*.json` in Spectrum 2's token shape: a global colour ramp (`color-palette.json`),
//! the alias layer components speak in (`color-aliases.json`), the layout / type scales (`layout.json`,
//! `typography.json`) and the component tokens (`component.json`). `build.rs` resolves every reference and generates
//! one typed `pub const` per token into [`tokens`], plus [`tokens::ALL`] for tooling. Dark only, by product direction.
//!
//! Colours are sRGB-encoded, straight (un-premultiplied) alpha, 0–1 floats — what vello / peniko take directly;
//! [`Color::linear`] gives the linear-light value for a wgpu pass that wants it.
#![forbid(unsafe_code)]

/// An sRGB-encoded colour with straight alpha, components 0–1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color { pub r: f32, pub g: f32, pub b: f32, pub a: f32 }

impl Color {
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Color { Color { r, g, b, a } }
    /// The same colour with its alpha replaced (the hover-ramp twin rule: idle = hover at alpha 0).
    pub const fn with_alpha(self, a: f32) -> Color { Color { a, ..self } }
    /// 8-bit sRGB components (alpha included), for bitmaps and the Qt generator.
    pub fn to_rgba8(self) -> [u8; 4] { [q8(self.r), q8(self.g), q8(self.b), q8(self.a)] }
    /// `#rrggbb` or `#rrggbbaa` (the latter only when alpha < 1).
    pub fn to_hex(self) -> String {
        let [r, g, b, a] = self.to_rgba8();
        if a == 255 { format!("#{r:02x}{g:02x}{b:02x}") } else { format!("#{r:02x}{g:02x}{b:02x}{a:02x}") }
    }
    /// Linear-light RGB (alpha unchanged), for a shader that composites in linear.
    pub fn linear(self) -> [f32; 4] { [srgb_to_linear(self.r), srgb_to_linear(self.g), srgb_to_linear(self.b), self.a] }
    /// WCAG relative luminance (alpha ignored).
    pub fn luminance(self) -> f32 { let l = self.linear(); 0.2126 * l[0] + 0.7152 * l[1] + 0.0722 * l[2] }
    /// WCAG contrast ratio against `other` (≥ 1; alpha ignored — both are taken as opaque).
    pub fn contrast(self, other: Color) -> f32 {
        let (a, b) = (self.luminance(), other.luminance());
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }
}

fn q8(x: f32) -> u8 { (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8 }
fn srgb_to_linear(c: f32) -> f32 { if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) } }

/// One layer of a drop shadow (offsets, blur and spread in px).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow { pub x: f32, pub y: f32, pub blur: f32, pub spread: f32, pub color: Color }

/// A token's value, typed by the shape of its data.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Token {
    Color(Color),
    /// A length in logical pixels.
    Px(f32),
    /// A bare number (a ratio such as `corner-radius-full` = 0.5 of the short side, a weight, a line height).
    Num(f64),
    /// A duration in milliseconds.
    Ms(u32),
    /// A length in em.
    Em(f32),
    Str(&'static str),
    Shadow(&'static [Shadow]),
}

/// The generated tokens: one `pub const` per data name (`corner-radius-100` → `CORNER_RADIUS_100`), and `ALL`.
pub mod tokens {
    use super::{Color, Shadow, Token};
    include!(concat!(env!("OUT_DIR"), "/theme.rs"));

    /// A token by its data name.
    pub fn get(name: &str) -> Option<Token> { ALL.binary_search_by(|(n, _)| n.cmp(&name)).ok().map(|i| ALL[i].1) }
}

/// The Phosphor icon table: slug → the glyph in `Phosphor.ttf` (QCView's table, 1 512 names).
pub mod icons {
    include!(concat!(env!("OUT_DIR"), "/icons.rs"));

    /// The glyph for a Phosphor slug (`"play"`, `"skip-back"`, …), or `None` for an unknown name — QCView renders
    /// an unknown name as a blank gap on purpose, so a typo is obvious in review; callers keep that rule.
    pub fn glyph(slug: &str) -> Option<char> { PHOSPHOR.binary_search_by(|(n, _)| n.cmp(&slug)).ok().map(|i| PHOSPHOR[i].1) }
}

/// The bundled font files (feature `fonts`, on by default): Geist Sans (OFL 1.1), Geist Mono (OFL 1.1), Phosphor (MIT).
/// Family names as the fonts declare them; a consumer registers the bytes with its text stack and asks for these names.
#[cfg(feature = "fonts")]
pub mod fonts {
    pub const FAMILY_SANS: &str = "Geist";
    pub const FAMILY_MONO: &str = "Geist Mono";
    pub const FAMILY_ICON: &str = "Phosphor";
    pub const GEIST_REGULAR: &[u8] = include_bytes!("../assets/fonts/Geist-Regular.ttf");
    pub const GEIST_BOLD: &[u8] = include_bytes!("../assets/fonts/Geist-Bold.ttf");
    pub const GEIST_ITALIC: &[u8] = include_bytes!("../assets/fonts/Geist-Italic.ttf");
    pub const GEIST_MONO_REGULAR: &[u8] = include_bytes!("../assets/fonts/GeistMono-Regular.ttf");
    pub const GEIST_MONO_BOLD: &[u8] = include_bytes!("../assets/fonts/GeistMono-Bold.ttf");
    pub const PHOSPHOR: &[u8] = include_bytes!("../assets/fonts/Phosphor.ttf");
    /// Every bundled face, for a loop that registers them all.
    pub const ALL: &[(&str, &[u8])] = &[
        ("Geist-Regular", GEIST_REGULAR), ("Geist-Bold", GEIST_BOLD), ("Geist-Italic", GEIST_ITALIC),
        ("GeistMono-Regular", GEIST_MONO_REGULAR), ("GeistMono-Bold", GEIST_MONO_BOLD), ("Phosphor", PHOSPHOR),
    ];
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokens::*;

    #[test]
    fn contrast_roles_hold_on_every_layer() {
        // Canon §25 row 163: content ≥ 7:1 and subdued ≥ 4.5:1 on all three layers; disabled is allowed to fail.
        for bg in [BACKGROUND_BASE_COLOR, BACKGROUND_LAYER_1_COLOR, BACKGROUND_LAYER_2_COLOR] {
            assert!(NEUTRAL_CONTENT_COLOR_DEFAULT.contrast(bg) >= 7.0, "content on {}: {}", bg.to_hex(), NEUTRAL_CONTENT_COLOR_DEFAULT.contrast(bg));
            assert!(NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT.contrast(bg) >= 4.5, "subdued on {}: {}", bg.to_hex(), NEUTRAL_SUBDUED_CONTENT_COLOR_DEFAULT.contrast(bg));
        }
        assert!(DISABLED_CONTENT_COLOR.contrast(BACKGROUND_BASE_COLOR) < 3.0);
    }

    #[test]
    fn accent_and_fills_carry_their_text() {
        // Row 162: white on the accent fill meets 4.5:1 (cobalt did not, 3.6:1); inverted text on a selected neutral fill is strong.
        assert!(ACCENT_CONTENT_COLOR_ON_FILL.contrast(ACCENT_BACKGROUND_COLOR_DEFAULT) >= 4.5);
        // the deliberate-grey selected state and Spectrum's softer tooltip still carry their text at AA (4.5:1)
        assert!(ACTION_BUTTON_CONTENT_COLOR_SELECTED.contrast(ACTION_BUTTON_BACKGROUND_COLOR_SELECTED) >= 4.5);
        assert!(ACTION_BUTTON_CONTENT_COLOR_SELECTED.contrast(ACTION_BUTTON_BACKGROUND_COLOR_SELECTED_DOWN) >= 4.5);
        assert!(TOOLTIP_CONTENT_COLOR.contrast(TOOLTIP_BACKGROUND_COLOR) >= 4.5);
        assert!(BUTTON_NEGATIVE_CONTENT_COLOR.contrast(BUTTON_NEGATIVE_BACKGROUND_COLOR) >= 4.0);
    }

    #[test]
    fn aliases_resolved_and_alpha_extension_applied() {
        assert_eq!(ACTION_BUTTON_HEIGHT_M, 32.0);
        assert_eq!(CORNER_RADIUS_MEDIUM_DEFAULT, 8.0);
        assert_eq!(HOVER_BACKGROUND_COLOR_IDLE, HOVER_BACKGROUND_COLOR.with_alpha(0.0));
        assert_eq!(SCRUBBER_WAVEFORM_COLOR.a, 0.12);
        assert_eq!(SCRUBBER_WAVEFORM_COLOR.with_alpha(1.0), GRAY_800);
        assert_eq!(tokens::get("spacing-100"), Some(Token::Px(8.0)));
        assert_eq!(tokens::get("no-such-token"), None);
        assert_eq!(POPOVER_SHADOW.len(), 3);
        assert_eq!(ANIMATION_DURATION_HOVER, 100);
        assert_eq!(DETAIL_LETTER_SPACING, 0.06);
        assert_eq!(FONT_FAMILY_MONO, "Geist Mono");
        assert!(ALL.windows(2).all(|w| w[0].0 < w[1].0), "ALL is sorted");
    }

    #[test]
    fn scales_are_spectrum_2_desktop() {
        assert_eq!([COMPONENT_HEIGHT_50, COMPONENT_HEIGHT_75, COMPONENT_HEIGHT_100, COMPONENT_HEIGHT_200], [20.0, 24.0, 32.0, 40.0]);
        assert_eq!([CORNER_RADIUS_SMALL_DEFAULT, CORNER_RADIUS_MEDIUM_DEFAULT, CORNER_RADIUS_LARGE_DEFAULT, CORNER_RADIUS_EXTRA_LARGE_DEFAULT], [4.0, 8.0, 10.0, 16.0]);
        assert_eq!(CORNER_RADIUS_FULL, 0.5);
        assert_eq!([SPACING_50, SPACING_75, SPACING_85, SPACING_100, SPACING_200, SPACING_300, SPACING_400, SPACING_500], [2.0, 4.0, 6.0, 8.0, 12.0, 16.0, 24.0, 32.0]);
        assert_eq!([BODY_SIZE_S, BODY_SIZE_M, BODY_SIZE_L, HEADING_SIZE_M], [12.0, 14.0, 16.0, 18.0]);
        assert_eq!(ICON_SIZE_TOOLBAR, 20.0);
        assert_eq!([WORKFLOW_ICON_SIZE_75, WORKFLOW_ICON_SIZE_100, WORKFLOW_ICON_SIZE_200, WORKFLOW_ICON_SIZE_300], [16.0, 20.0, 22.0, 26.0]);
        assert_eq!((ACTION_GROUP_GAP, ACTION_GROUP_GAP_COMPACT, ACTION_GROUP_CLUSTER_GAP), (8.0, 0.0, 16.0));
    }

    #[test]
    fn colour_maths() {
        assert_eq!(GRAY_25.to_hex(), "#111111");
        assert_eq!(BLUE_800.to_rgba8(), [64, 105, 253, 255]);
        assert_eq!(Color::rgba(1.0, 1.0, 1.0, 1.0).contrast(Color::rgba(0.0, 0.0, 0.0, 1.0)).round(), 21.0);
        assert!((Color::rgba(0.5, 0.5, 0.5, 1.0).linear()[0] - 0.2140).abs() < 1e-3);
    }

    #[test]
    fn phosphor_table() {
        assert_eq!(icons::PHOSPHOR.len(), 1512);
        assert!(icons::glyph("play").is_some());
        assert!(icons::glyph("skip-back").is_some());
        assert!(icons::glyph("no-such-icon").is_none());
        assert!(icons::PHOSPHOR.iter().all(|(_, c)| ('\u{E000}'..='\u{F8FF}').contains(c)), "private-use glyphs");
    }

    #[cfg(feature = "fonts")]
    #[test]
    fn fonts_are_truetype() {
        for (name, bytes) in fonts::ALL { assert_eq!(&bytes[..4], &[0, 1, 0, 0], "{name} is a TrueType face"); }
    }
}
