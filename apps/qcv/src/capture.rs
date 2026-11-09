//! Captures (canon row 216, ruled 2026-11-08): T = the review picture to the Desktop as
//! `qcview-screenshot-yyyy-MM-dd_HH-mm-ss.(png|jpg|tif)` per `export/screenshotFormat` (PNG 8-bit · JPEG q95 · TIFF 16-bit —
//! the user 2026-11-08: "give them 16 bit tiffs"); Ctrl/Cmd+T = to the clipboard (8-bit). The picture is the screen's: the
//! clip's tags + overrides, its chain RE-TARGETED to SDR when the view targets HDR (QCView's `setSdrCapture`: minColor at
//! peak 100, OCIO the nearest SDR view), the viewer aids, pixel aspect + rotation applied; an HDR source in bypass is
//! tone-mapped by BT.2390 (never raw code values); annotations baked; safety guides only with the "include guides" setting
//! (off by default). The encode + write runs on a worker (QCView's QtConcurrent route); the toast reports back.
use crate::{annotate, color, guides, settings::k, trw};
use qcv_color::ocio::view_is_sdr;
use std::{path::PathBuf, sync::mpsc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dest { File, Clipboard }

/// What a capture swapped out to re-target its chain to SDR, put back after.
enum Restore { None, MinColor(qcv_color::Chain), Ocio(Option<Box<dyn min_present::ColorPass>>) }

/// The user's Desktop (QCView: QStandardPaths::DesktopLocation).
pub fn desktop_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::Shell::{FOLDERID_Desktop, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
        let p = SHGetKnownFolderPath(&FOLDERID_Desktop, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().ok(); windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const _));
        return s.map(PathBuf::from).filter(|d| d.is_dir())
    }
    #[cfg(not(windows))]
    { std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Desktop")).filter(|d| d.is_dir()) }
}

/// `export/screenshotFormat` → (QCView's token, the extension, the toast's format name).
fn format(token: &str) -> (&'static str, &'static str, &'static str) {
    match token { "jpeg" => ("jpeg", "jpg", "JPEG"), "tiff" => ("tiff", "tif", "TIFF"), _ => ("png", "png", "PNG") }
}

/// QCView's nearest SDR view for an HDR one (`sdrCaptureDisplayView`, over our one display's views): the Blender name without
/// its tier, the ACES "SDR 100 nits" sibling keeping the D60 choice, any SDR view of the family, else the first view.
pub fn sdr_view(views: &[String], src: &str) -> Option<String> {
    if view_is_sdr(src) { return None }
    let sdr: Vec<&String> = views.iter().filter(|v| view_is_sdr(v)).collect();
    let mut base = src.replace(" (No Tonemap)", "");
    for tier in [" - HDR", " - SDR"] { if let Some(at) = base.find(tier) && at > 0 { base.truncate(at); } }
    if sdr.iter().any(|v| **v == base) { return Some(base) }
    let d60 = src.contains("D60");
    let family = src.split(' ').next().unwrap_or("");
    let prefix = match src.find("HDR ") { Some(i) if src[i + 4..].split(' ').next().is_some_and(|n| n.parse::<u32>().is_ok()) && src[i..].contains(" nits") => format!("{}SDR 100 nits", &src[..i]), _ => src.to_string() };
    for pass in 0..2 {
        let mut fallback = None;
        for v in &sdr {
            let m = if pass == 0 { prefix != src && v.starts_with(&prefix) } else { !family.is_empty() && v.starts_with(family) && v.contains("SDR") };
            if !m { continue }
            if v.contains("D60") == d60 { return Some((*v).clone()) }
            fallback.get_or_insert((*v).clone());
        }
        if fallback.is_some() { return fallback }
    }
    sdr.first().map(|v| (*v).clone()).or_else(|| views.first().cloned())
}

fn lin(c: u8) -> f32 { let v = c as f32 / 255.0; if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) } }

impl crate::App {
    /// T / Ctrl+T and the bar's two buttons.
    pub(crate) fn screenshot(&mut self, dest: Dest) {
        let Some(img) = self.capture_picture() else { return };
        let (px, w, h) = img;
        match dest {
            Dest::Clipboard => {
                let rgb = min_present::capture::srgb8(&px);
                let rgba: Vec<u8> = rgb.chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect();
                let ok = arboard::Clipboard::new().and_then(|mut c| c.set_image(arboard::ImageData { width: w as usize, height: h as usize, bytes: rgba.into() }));
                match ok {
                    Ok(()) => { log::info!("screenshot {w}x{h} copied to clipboard"); self.toast(trw("Screenshot copied to clipboard"), min_ui::ToastKind::Positive) }
                    Err(e) => { log::warn!("screenshot to clipboard: {e}"); self.toast(trw("Screenshot export failed"), min_ui::ToastKind::Negative) }
                }
            }
            Dest::File => {
                let Some(desk) = desktop_dir() else { log::warn!("screenshot: no Desktop folder"); self.toast(trw("Couldn't resolve the Desktop folder"), min_ui::ToastKind::Negative); return };
                let (token, ext, name) = format(&self.settings.string(k::SCREENSHOT_FORMAT));
                let stamp = jiff::Zoned::now().strftime("%Y-%m-%d_%H-%M-%S").to_string();
                let path = desk.join(format!("qcview-screenshot-{stamp}.{ext}")).to_string_lossy().into_owned();
                let (tx, rx) = mpsc::channel();
                std::thread::Builder::new().name("screenshot".into()).spawn(move || {
                    let r = match token {
                        "jpeg" => min_image::write::jpeg8(&path, &min_present::capture::srgb8(&px), w, h, 95),
                        "tiff" => min_image::write::tiff16(&path, &min_present::capture::srgb16(&px), w, h),
                        _ => min_image::write::png8(&path, &min_present::capture::srgb8(&px), w, h),
                    };
                    match &r { Ok(()) => log::info!("screenshot saved to {path}"), Err(e) => log::warn!("screenshot save failed: {path}: {e}") }
                    let _ = tx.send(r.is_ok().then_some(name));
                }).ok();
                self.capture_rx.push(rx);
            }
        }
    }
    /// The workers' results landing (every tick): QCView's short toast, the format confirming the writer.
    pub(crate) fn poll_capture(&mut self) {
        let mut done = vec![];
        self.capture_rx.retain(|rx| match rx.try_recv() { Ok(r) => { done.push(r); false } Err(mpsc::TryRecvError::Empty) => true, Err(_) => false });
        for r in done { match r { Some(name) => { let t = trw("Screenshot saved (%1)").replace("%1", name); self.toast(&t, min_ui::ToastKind::Positive) } None => self.toast(trw("Screenshot export failed"), min_ui::ToastKind::Negative) } }
    }
    /// The review picture as SDR-ready linear RGB (1.0 = SDR white) with the strokes (and the guide, when asked) on it.
    pub(crate) fn capture_picture(&mut self) -> Option<(Vec<f32>, u32, u32)> {
        let dual = self.dual.is_some();
        let frame = self.player.as_ref().map(|p| p.current_frame()).unwrap_or(-1);
        let restore = self.sdr_retarget(0);
        let got = { let g = self.gpu.as_mut()?; color::slot_engine(g, 0, dual).map(|en| (en.bypass_hdr_peak(), en.picture_placement(), en.capture(true))) };
        self.sdr_restore(0, restore);
        let Some((peak, place, Ok((mut px, w, h)))) = got else {
            log::warn!("screenshot capture returned empty"); self.toast(trw("Nothing to capture — load media first"), min_ui::ToastKind::Notice); return None
        };
        min_present::capture::to_sdr(&mut px, peak);
        for v in px.iter_mut() { *v = v.clamp(0.0, 1.0); }
        // the overlay: the note's strokes at this frame (single view, QCView), the guide when the setting asks
        let strokes = if dual { vec![] } else { self.annot.strokes_at(frame) };
        let guide = self.settings.bool(k::SCREENSHOT_GUIDES).then(|| self.gpu.as_ref().and_then(|g| g.guide.as_ref().map(|(_, gd)| gd.clone()))).flatten();
        if strokes.is_empty() && guide.is_none() { return Some((px, w, h)) }
        let (gw, gop, gcol) = { let st = &self.settings; (st.float(k::SAFETY_WIDTH), st.float(k::SAFETY_OPACITY) as f32, min_ui::color_picker::parse_hex(&st.string(k::SAFETY_COLOR)).unwrap_or([1.0, 1.0, 0.0])) };
        let g = self.gpu.as_mut()?;
        let (dev, q) = (g.presenter.engine.device.clone(), g.presenter.engine.queue.clone());
        if g.thumb_layer.is_none() { g.thumb_layer = min_ui::UiLayer::new(&dev, w, h).ok(); }
        let layer = g.thumb_layer.as_mut()?;
        if layer.size() != (w, h) { layer.resize(&dev, w, h); }
        let scene = layer.scene_mut(); scene.reset();
        let geo = annotate::Geo { rect: [0.0, 0.0, w as f64, h as f64], px_per_src: 1.0 };
        for s in &strokes { annotate::draw(scene, s, &geo); }
        if let Some(gd) = &guide {
            // the screen's line width at a 1080-line picture, scaled with the capture
            let k_ = (h as f64 / 1080.0).max(1.0);
            let (src, quarters) = place.map(|p| (p.1, p.2)).unwrap_or(((w as f64, h as f64), 0));
            guides::paint(scene, gd, guides::transform(gd.view, src, [0.0, 0.0, w as f64, h as f64], quarters), gw * k_, peniko::Color::new([gcol[0], gcol[1], gcol[2], gop.clamp(0.0, 1.0)]));
        }
        layer.render(&dev, &q);
        let ov = min_present::readback::read_rgba8(&dev, &q, layer.texture(), w, h);
        // vello writes sRGB-encoded straight alpha: over the picture in linear light
        for (c, o) in px.chunks_mut(3).zip(ov.chunks(4)) {
            if o[3] == 0 { continue }
            let a = o[3] as f32 / 255.0;
            for i in 0..3 { c[i] = c[i] * (1.0 - a) + lin(o[i]) * a; }
        }
        Some((px, w, h))
    }
    /// Re-target the slot's chain to SDR for the capture (QCView's `setSdrCapture`).
    fn sdr_retarget(&mut self, slot: usize) -> Restore {
        if !self.settings.bool(k::COLOR_ENABLED) || self.color_bound[slot].is_none() { return Restore::None }
        let (_, stored, _) = self.chain_of(&self.slot_item(slot));
        match stored.chain {
            crate::color_model::ClipChain::MinColor(_) => {
                let Some(rc) = self.mincolor[slot].clone() else { return Restore::None };
                let old = *rc.borrow().chain();
                let mut n = old; n.peak_nits = 100.0; n.agx_peak = 100.0;
                if n == old { return Restore::None }
                rc.borrow_mut().set_chain(n);
                Restore::MinColor(old)
            }
            crate::color_model::ClipChain::Ocio(o) => {
                let Some(cfg) = self.ocio_config(&o.config) else { return Restore::None };
                let Some(view) = sdr_view(&cfg.views(), &o.view) else { return Restore::None };
                let dual = self.dual.is_some();
                let Some(g) = self.gpu.as_mut() else { return Restore::None };
                let Some(en) = color::slot_engine(g, slot, dual) else { return Restore::None };
                let spec = qcv_color::ocio::Spec { input: o.input, look: o.look, view: view.clone(), scene_lut: o.scene_lut };
                match qcv_color::ocio::OcioPass::new(&en.device, &en.queue, &cfg, spec, o.knee) {
                    Ok(p) => { log::info!("screenshot: OCIO view '{}' → '{view}' for the capture", o.view); let old = en.take_color_pass(); en.set_color_pass(Some(Box::new(p))); Restore::Ocio(old) }
                    Err(e) => { log::warn!("screenshot: no SDR twin for '{}' ({e}); capturing the HDR view clipped", o.view); Restore::None }
                }
            }
        }
    }
    fn sdr_restore(&mut self, slot: usize, r: Restore) {
        match r {
            Restore::None => {}
            Restore::MinColor(c) => { if let Some(rc) = &self.mincolor[slot] { rc.borrow_mut().set_chain(c); } }
            Restore::Ocio(old) => { let dual = self.dual.is_some(); if let Some(g) = self.gpu.as_mut() && let Some(en) = color::slot_engine(g, slot, dual) { en.set_color_pass(old); } }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::sdr_view;
    #[test]
    fn the_nearest_sdr_view_is_qcviews() {
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let blender = v(&["Standard", "AgX", "ACES 2.0", "ACES 2.0 - HDR 1000 nits", "Standard (No Tonemap) - HDR"]);
        assert_eq!(sdr_view(&blender, "AgX"), None, "an SDR view is captured as is");
        assert_eq!(sdr_view(&blender, "ACES 2.0 - HDR 1000 nits").as_deref(), Some("ACES 2.0"));
        let aces = v(&["ACES 2.0 - SDR 100 nits (Rec.709)", "ACES 2.0 - SDR 100 nits (Rec.709 D60 in Rec.709 D65)", "ACES 2.0 - HDR 1000 nits (P3 D65)", "ACES 2.0 - HDR 1000 nits (P3 D60 in P3 D65)"]);
        assert_eq!(sdr_view(&aces, "ACES 2.0 - HDR 1000 nits (P3 D65)").as_deref(), Some("ACES 2.0 - SDR 100 nits (Rec.709)"));
        assert_eq!(sdr_view(&aces, "ACES 2.0 - HDR 1000 nits (P3 D60 in P3 D65)").as_deref(), Some("ACES 2.0 - SDR 100 nits (Rec.709 D60 in Rec.709 D65)"));
    }
}
