//! The colour chain as the app stores it (canon rows 205–208, ruled 2026-11-08): ONE whole chain per clip (Input, Look /
//! Scene LUT, Knee, Rendering), a project default for clips without one, presets as starting points in the user's
//! library. Stored in the per-clip maps `.qcvproj` already carries (`minColorClip` / `ocioClip`): QCView's own keys
//! (`input`, `knee` for minColor; `input`, `look`, `sceneLut`, `knee` for OCIO) keep their meaning, ours are added
//! (`agx`, `output` = minColorChain::toVariant's; `engine`, `config`, `view`, `preset`), so QCView still reads the clip side.
use qcv_color::{Chain, Knee};
use serde_json::{json, Map, Value};

#[derive(Clone, Debug, PartialEq)]
pub struct OcioChain { pub config: String, pub input: String, pub look: String, pub view: String, pub scene_lut: String, pub knee: Knee }

#[derive(Clone, Debug, PartialEq)]
pub enum ClipChain { MinColor(Chain), Ocio(OcioChain) }

impl ClipChain {
    pub fn is_ocio(&self) -> bool { matches!(self, ClipChain::Ocio(_)) }
}

/// A chain plus the preset it came from (shown as "Custom (from …)" once edited).
#[derive(Clone, Debug, PartialEq)]
pub struct Stored { pub chain: ClipChain, pub preset: String }

fn f(m: &Map<String, Value>, k: &str, d: f32) -> f32 { m.get(k).and_then(Value::as_f64).map(|v| v as f32).unwrap_or(d) }
fn i(m: &Map<String, Value>, k: &str, d: i32) -> i32 { m.get(k).and_then(|v| v.as_i64().or_else(|| v.as_bool().map(|b| b as i64))).map(|v| v as i32).unwrap_or(d) }
fn b(m: &Map<String, Value>, k: &str, d: bool) -> bool { m.get(k).and_then(|v| v.as_bool().or_else(|| v.as_i64().map(|n| n != 0))).unwrap_or(d) }
fn o<'a>(m: &'a Map<String, Value>, k: &str) -> Map<String, Value> { m.get(k).and_then(Value::as_object).cloned().unwrap_or_default() }
fn s(m: &Map<String, Value>, k: &str) -> String { m.get(k).and_then(Value::as_str).unwrap_or_default().to_string() }

fn knee_to(k: &Knee) -> Value { json!({ "enabled": k.enabled, "sourceNits": k.source_nits as f64, "targetNits": k.target_nits as f64, "start": k.start as f64 }) }
fn knee_from(m: &Map<String, Value>, d: Knee) -> Knee { Knee { enabled: b(m, "enabled", d.enabled), source_nits: f(m, "sourceNits", d.source_nits), target_nits: f(m, "targetNits", d.target_nits), start: f(m, "start", d.start) } }

/// minColor's map: QCView's MinColorChain::toVariant keys exactly.
pub fn mc_to_map(c: &Chain) -> Map<String, Value> {
    let v = json!({
        "input": { "gamut": c.in_gamut, "transfer": c.in_transfer, "limited": c.in_limited != 0 },
        "knee": { "enabled": c.knee_on != 0, "sourceNits": c.knee_src as f64, "targetNits": c.knee_tgt as f64, "start": c.knee_start as f64 },
        "agx": { "enabled": c.agx_on != 0, "target": c.agx_target, "peak": c.agx_peak as f64, "whiteEv": c.agx_white_ev as f64, "blackEv": c.agx_black_ev as f64, "contrast": c.agx_contrast as f64, "toe": c.agx_toe as f64, "shoulder": c.agx_shoulder as f64, "hueRestore": c.agx_hue_restore as f64, "hdrPurity": c.agx_hdr_purity as f64, "outset": c.agx_outset as f64 },
        "output": { "openDrt": c.open_drt != 0, "look": c.look, "tonescale": c.tonescale, "cwp": c.cwp, "cwpLimit": c.cwp_limit as f64, "surround": c.surround, "peakNits": c.peak_nits as f64, "greyBoost": c.grey_boost as f64, "hdrPurity": c.hdr_purity as f64, "greyNits": c.grey_nits as f64 },
    });
    v.as_object().cloned().unwrap()
}
/// Read a minColor map over `base` (QCView's clip pins carry only `input` / `knee`; the rest follows the base).
pub fn mc_from_map(m: &Map<String, Value>, base: Chain) -> Chain {
    let mut c = base;
    let inp = o(m, "input"); if !inp.is_empty() { c.in_gamut = i(&inp, "gamut", c.in_gamut); c.in_transfer = i(&inp, "transfer", c.in_transfer); c.in_limited = b(&inp, "limited", false) as i32; }
    let kn = o(m, "knee"); if !kn.is_empty() { c.knee_on = b(&kn, "enabled", false) as i32; c.knee_src = f(&kn, "sourceNits", c.knee_src); c.knee_tgt = f(&kn, "targetNits", c.knee_tgt); c.knee_start = f(&kn, "start", c.knee_start); }
    let ax = o(m, "agx"); if !ax.is_empty() { c.agx_on = b(&ax, "enabled", false) as i32; c.agx_target = i(&ax, "target", c.agx_target); c.agx_peak = f(&ax, "peak", c.agx_peak); c.agx_white_ev = f(&ax, "whiteEv", c.agx_white_ev); c.agx_black_ev = f(&ax, "blackEv", c.agx_black_ev); c.agx_contrast = f(&ax, "contrast", c.agx_contrast); c.agx_toe = f(&ax, "toe", c.agx_toe); c.agx_shoulder = f(&ax, "shoulder", c.agx_shoulder); c.agx_hue_restore = f(&ax, "hueRestore", c.agx_hue_restore); c.agx_hdr_purity = f(&ax, "hdrPurity", c.agx_hdr_purity); c.agx_outset = f(&ax, "outset", c.agx_outset); }
    let out = o(m, "output"); if !out.is_empty() { c.open_drt = b(&out, "openDrt", false) as i32; c.look = i(&out, "look", c.look); c.tonescale = i(&out, "tonescale", c.tonescale); c.cwp = i(&out, "cwp", c.cwp); c.cwp_limit = f(&out, "cwpLimit", c.cwp_limit); c.surround = i(&out, "surround", c.surround); c.peak_nits = f(&out, "peakNits", c.peak_nits); c.grey_boost = f(&out, "greyBoost", c.grey_boost); c.hdr_purity = f(&out, "hdrPurity", c.hdr_purity); c.grey_nits = f(&out, "greyNits", c.grey_nits); }
    c
}

fn ocio_to_map(c: &OcioChain) -> Map<String, Value> {
    let v = json!({ "engine": "ocio", "config": c.config, "input": c.input, "look": c.look, "view": c.view, "sceneLut": { "path": c.scene_lut, "cccId": "" }, "knee": knee_to(&c.knee) });
    v.as_object().cloned().unwrap()
}
fn ocio_from_map(m: &Map<String, Value>) -> Option<OcioChain> {
    let view = s(m, "view"); if view.is_empty() { return None }
    Some(OcioChain { config: s(m, "config"), input: s(m, "input"), look: s(m, "look"), view, scene_lut: s(&o(m, "sceneLut"), "path"), knee: knee_from(&o(m, "knee"), Knee::default()) })
}

impl Stored {
    /// One map (presets, the project default): `engine` says which shape follows.
    pub fn to_map(&self) -> Map<String, Value> {
        let mut m = match &self.chain { ClipChain::MinColor(c) => { let mut m = mc_to_map(c); m.insert("engine".into(), "minColor".into()); m } ClipChain::Ocio(c) => ocio_to_map(c) };
        if !self.preset.is_empty() { m.insert("preset".into(), self.preset.clone().into()); }
        m
    }
    pub fn from_map(m: &Map<String, Value>) -> Option<Stored> {
        if m.is_empty() { return None }
        let chain = if s(m, "engine") == "ocio" { ClipChain::Ocio(ocio_from_map(m)?) } else { ClipChain::MinColor(mc_from_map(m, Chain::default())) };
        Some(Stored { chain, preset: s(m, "preset") })
    }
    /// The clip's two maps: the engine's own map carries the chain; the other keeps its QCView pins minus our marker.
    pub fn to_clip_maps(&self, old_ocio: &Map<String, Value>, old_mc: &Map<String, Value>) -> (Map<String, Value>, Map<String, Value>) {
        match &self.chain {
            ClipChain::MinColor(c) => { let mut mc = mc_to_map(c); if !self.preset.is_empty() { mc.insert("preset".into(), self.preset.clone().into()); } let mut oc = old_ocio.clone(); oc.remove("engine"); (oc, mc) }
            ClipChain::Ocio(c) => { let mut oc = ocio_to_map(c); if !self.preset.is_empty() { oc.insert("preset".into(), self.preset.clone().into()); } (oc, old_mc.clone()) }
        }
    }
    /// A clip's own chain from its maps; None = it follows the project default. QCView's minColor pins (input / knee only)
    /// read over the default's minColor chain.
    pub fn from_clip_maps(ocio: &Map<String, Value>, mc: &Map<String, Value>, default: &Stored) -> Option<Stored> {
        if s(ocio, "engine") == "ocio" { return ocio_from_map(ocio).map(|c| Stored { chain: ClipChain::Ocio(c), preset: s(ocio, "preset") }) }
        if !mc.is_empty() {
            let base = match &default.chain { ClipChain::MinColor(c) => *c, _ => Chain::default() };
            return Some(Stored { chain: ClipChain::MinColor(mc_from_map(mc, base)), preset: s(mc, "preset") })
        }
        None
    }
}

/// minColor's `DRT_IN_*` / `DRT_OETF_*` indices the built-ins use (the vendored opendrt_params.h).
mod id { pub const ACES2065: i32 = 1; pub const ACESCG: i32 = 2; pub const P3D65: i32 = 3; pub const REC2020: i32 = 4; pub const REC709: i32 = 5;
         pub const LINEAR: i32 = 0; pub const REC1886: i32 = 10; pub const SRGB: i32 = 11; pub const PQ: i32 = 14; }

#[derive(Clone, Debug, PartialEq)]
pub struct Preset { pub name: String, pub stored: Stored, pub builtin: bool }

/// The built-ins (row 198: every built-in is minColor): QCView's minColor seeds with the per-output triples collapsed (one
/// surface), plus the two display-referred readings a review meets most (Rec.709 video, sRGB).
pub fn builtins() -> Vec<Preset> {
    let d = Chain::default();
    let mk = |name: &str, c: Chain| Preset { name: name.into(), stored: Stored { chain: ClipChain::MinColor(c), preset: name.into() }, builtin: true };
    vec![
        mk("Rec.709 video", Chain { in_gamut: id::REC709, in_transfer: id::REC1886, ..d }),
        mk("sRGB", Chain { in_gamut: id::REC709, in_transfer: id::SRGB, ..d }),
        // the ACES pair next: more common in review than the linear working spaces (the user 2026-11-08)
        mk("ACEScg", Chain { in_gamut: id::ACESCG, in_transfer: id::LINEAR, ..d }),
        mk("ACES 2065-1", Chain { in_gamut: id::ACES2065, in_transfer: id::LINEAR, ..d }),
        mk("Linear Rec.709", Chain { in_gamut: id::REC709, in_transfer: id::LINEAR, ..d }),
        mk("Linear Rec.709 · AgX", Chain { in_gamut: id::REC709, in_transfer: id::LINEAR, agx_on: 1, ..d }),
        mk("Linear Rec.2020", Chain { in_gamut: id::REC2020, in_transfer: id::LINEAR, ..d }),
        mk("Linear Rec.2020 · AgX", Chain { in_gamut: id::REC2020, in_transfer: id::LINEAR, agx_on: 1, ..d }),
        mk("P3-D65 PQ master", Chain { in_gamut: id::P3D65, in_transfer: id::PQ, ..d }),
    ]
}

/// The project default when the project has none: QCView's minColor default (Rec.709 / Rec.1886).
pub fn fallback_default() -> Stored { builtins().remove(0).stored }

/// The user's presets (row 208): `color-presets.json` beside the settings file.
pub struct Library { path: Option<std::path::PathBuf>, pub user: Vec<Preset> }

impl Library {
    pub fn load(settings_path: Option<&std::path::Path>) -> Library {
        let path = settings_path.and_then(|p| p.parent()).map(|d| d.join("color-presets.json"));
        let user = path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.get("presets").and_then(Value::as_array).cloned()).unwrap_or_default().iter().filter_map(|p| {
                let m = p.as_object()?; let name = s(m, "name"); if name.is_empty() { return None }
                Some(Preset { name, stored: Stored::from_map(&o(m, "chain"))?, builtin: false })
            }).collect();
        Library { path, user }
    }
    pub fn save(&self) {
        let Some(p) = &self.path else { return };
        let v = json!({ "presets": self.user.iter().map(|p| json!({ "name": p.name, "chain": Value::Object(Stored { preset: String::new(), ..p.stored.clone() }.to_map()) })).collect::<Vec<_>>() });
        if let Err(e) = std::fs::write(p, serde_json::to_string_pretty(&v).unwrap_or_default()) { log::warn!("colour presets: {e}"); }
    }
    /// Built-ins first, then the user's.
    pub fn all(&self) -> Vec<Preset> { let mut v = builtins(); v.extend(self.user.iter().cloned()); v }
    pub fn find(&self, name: &str) -> Option<Preset> { self.all().into_iter().find(|p| p.name == name) }
    /// A name not yet taken ("Name", "Name 2", …).
    pub fn unique(&self, want: &str) -> String { let all = self.all(); let mut n = want.trim().to_string(); if n.is_empty() { n = "Preset".into(); } let base = n.clone(); let mut k = 2; while all.iter().any(|p| p.name == n) { n = format!("{base} {k}"); k += 1; } n }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chains_round_trip_and_read_qcview_pins() {
        let agx = builtins().into_iter().find(|p| p.name == "Linear Rec.709 · AgX").unwrap().stored;
        assert_eq!(Stored::from_map(&agx.to_map()), Some(agx.clone()));
        let (oc, mc) = agx.to_clip_maps(&Map::new(), &Map::new());
        assert_eq!(Stored::from_clip_maps(&oc, &mc, &fallback_default()), Some(agx));
        // a QCView pin: input only (P3-D65 / PQ) over the default's chain
        let pin: Map<String, Value> = serde_json::from_str(r#"{"input":{"gamut":3,"transfer":14,"limited":false}}"#).unwrap();
        let Some(Stored { chain: ClipChain::MinColor(c), .. }) = Stored::from_clip_maps(&Map::new(), &pin, &fallback_default()) else { panic!() };
        assert_eq!((c.in_gamut, c.in_transfer, c.agx_on), (3, 14, 0));
        let ocio = Stored { chain: ClipChain::Ocio(OcioChain { config: "Blender5.2".into(), input: "Rec.1886".into(), look: String::new(), view: "AgX".into(), scene_lut: String::new(), knee: Knee::default() }), preset: "Mine".into() };
        let (oc, mc) = ocio.to_clip_maps(&Map::new(), &pin);
        assert_eq!(mc, pin, "the minColor pins survive an OCIO chain");
        assert_eq!(Stored::from_clip_maps(&oc, &mc, &fallback_default()), Some(ocio));
        assert_eq!(Stored::from_clip_maps(&Map::new(), &Map::new(), &fallback_default()), None, "no maps = follows the default");
    }
}
