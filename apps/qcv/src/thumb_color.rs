//! Thumbnails through the screen's pipeline (canon row 216: "ONE pipeline for captures AND thumbnails … the clip's own
//! chain (SDR-retargeted when the view targets HDR) … on the GPU … THUMBNAILS never [the viewer aids]"). The loaders give
//! R'G'B' (swscale with the frame's matrix + range); a small engine of our own on the presenter's device shows it as planar
//! RGB tagged with the clip's transfer + primaries, runs a chain built like the slot's (its own pass: the slot's is busy on
//! screen), captures, BT.2390 for an HDR source in bypass, sRGB 8-bit. `generation` moves when the look changes, so the hover
//! cache re-renders what it shows (and nothing it does not).
use crate::color_model::ClipChain;
use min_present::{engine::Engine, ColorPass, Frame, Pixels, PlanarLayout, Plane};
use qcv_player::{ThumbPixels, ThumbSamples};
use std::sync::mpsc;

#[derive(Default)]
pub struct ThumbColor {
    engine: Option<Engine>,
    /// the look the engine has (chain + tags); `generation` = its generation
    key: String, pub generation: u64,
    pending: Option<(String, mpsc::Receiver<Box<dyn ColorPass + Send>>)>,
    meta: Option<min_present::ColorMeta>,
}

impl ThumbColor {
    /// R'G'B' thumbnail → the clip's look, sRGB 8-bit RGBA (None: no engine yet / the GPU said no).
    pub fn convert(&mut self, px: &ThumbPixels) -> Option<peniko::ImageBrush> {
        let en = self.engine.as_mut()?;
        let (w, h) = (px.width, px.height);
        if w == 0 || h == 0 { return None }
        // the source's own samples as planar G, B, R at 16 bits (code values, or half floats for EXR) — row 216
        let (data, float) = match &px.samples { ThumbSamples::Code16(v) => (v, false), ThumbSamples::Half(v) => (v, true) };
        let pitch = (w * 2).div_ceil(256) * 256;
        let mut planes = [vec![0u8; (pitch * h) as usize], vec![0u8; (pitch * h) as usize], vec![0u8; (pitch * h) as usize]];
        for y in 0..h { for x in 0..w {
            let i = ((y * w + x) * 3) as usize; let o = (y * pitch + x * 2) as usize;
            for (pl, c) in [(0usize, 1usize), (1, 2), (2, 0)] { planes[pl][o..o + 2].copy_from_slice(&data[i + c].to_ne_bytes()); }
        } }
        let meta = self.meta.unwrap_or_default();
        let m = min_present::ColorMeta { matrix: min_present::Matrix::Identity, range: min_present::Range::Full, chroma: min_present::Chroma::Rgb, bit_depth: 16, rotation: 0, sar: (1, 1), range_forced: false, ..meta };
        let [g, b, r] = planes;
        let frame = Frame { width: w, height: h, pixels: Pixels::Planar { planes: vec![Plane::owned(g, w, h, pitch), Plane::owned(b, w, h, pitch), Plane::owned(r, w, h, pitch)], layout: PlanarLayout::Rgb { bytes_per_sample: 2, code_bits: 16, float, alpha: false, legal_expand: false } }, meta: m };
        en.show(&frame).ok()?;
        let peak = en.bypass_hdr_peak();
        let (mut lin, cw, ch) = en.capture(false).ok()?;
        min_present::capture::to_sdr(&mut lin, peak);
        let rgb = min_present::capture::srgb8(&lin);
        let rgba: Vec<u8> = rgb.chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect();
        Some(min_ui::paint::image_rgba8(cw, ch, rgba))
    }
}

impl crate::App {
    /// Per tick: keep the thumbnail look in step with slot 0's (colour on/off, its chain, the clip's tags). Returns the
    /// generation the hover cache should be at.
    pub(crate) fn thumb_color_tick(&mut self, slot: usize) -> u64 {
        let on = self.settings.bool(crate::settings::k::COLOR_ENABLED) && self.color_bound[slot].is_some();
        let (_, stored, _) = self.chain_of(&self.slot_item(slot));
        let meta = self.gpu.as_ref().and_then(|g| if self.dual.is_some() { g.presenter.engine.dual().and_then(|d| if slot == 1 { d.b.shown_meta() } else { d.a.shown_meta() }) } else if slot == 0 { g.presenter.engine.shown_meta() } else { None });
        let chain_key = if !on { "bypass".to_string() } else { match &stored.chain { ClipChain::MinColor(c) => format!("mc|{c:?}"), ClipChain::Ocio(o) => format!("ocio|{}|{}|{}|{}|{}|{:?}", o.config, o.input, o.look, o.view, o.scene_lut, o.knee) } };
        let Some(g) = self.gpu.as_mut() else { return self.thumb_color[slot].generation };
        // the Video emulation as the slot's picture has it on screen (canon row 262: thumbnails carry it too)
        let emu = on && crate::color::slot_engine(g, slot, self.dual.is_some()).is_some_and(|e| e.video_emulation());
        let key = format!("{chain_key}|{:?}|{emu}", meta.map(|m| (m.transfer, m.primaries)));
        let tc = &mut self.thumb_color[slot];
        if tc.engine.is_none() { tc.engine = Some(Engine::new(g.presenter.engine.device.clone(), g.presenter.engine.queue.clone(), wgpu::TextureFormat::Rgba16Float)); }
        // a pass landing
        if let Some((k, rx)) = &tc.pending && let Ok(p) = rx.try_recv() {
            if *k == key { tc.engine.as_mut().unwrap().set_color_pass(Some(p)); tc.generation += 1; }
            tc.pending = None;
        }
        if key == tc.key { return tc.generation }
        tc.key = key.clone(); tc.meta = meta; tc.generation += 1; tc.pending = None;
        let en = tc.engine.as_mut().unwrap();
        en.set_video_emulation(emu);
        en.set_color_pass(None);   // bypass meanwhile (and for good when colour is off)
        if !on { return tc.generation }
        let (d, q) = (en.device.clone(), en.queue.clone());
        let (tx, rx) = mpsc::channel();
        match stored.chain {
            ClipChain::MinColor(c) => {
                // the slot's chain re-targeted to SDR, as a capture is (thumbnails are SDR pictures)
                let mut c = c; c.peak_nits = 100.0; c.agx_peak = 100.0;
                std::thread::Builder::new().name("thumb-mincolor".into()).spawn(move || { let _ = tx.send(Box::new(qcv_color::MinColorPass::new(&d, &q, c)) as Box<dyn ColorPass + Send>); }).ok();
            }
            ClipChain::Ocio(o) => {
                let cfg = crate::color::ocio_config_path(&o.config);
                std::thread::Builder::new().name("thumb-ocio".into()).spawn(move || {
                    let Ok(c) = qcv_color::ocio::Config::open(&cfg) else { return };
                    let view = crate::capture::sdr_view(&c.views(), &o.view).unwrap_or(o.view.clone());
                    match qcv_color::ocio::OcioPass::new(&d, &q, &c, qcv_color::ocio::Spec { input: o.input, look: o.look, view, scene_lut: o.scene_lut }, o.knee) { Ok(p) => { let _ = tx.send(Box::new(p) as Box<dyn ColorPass + Send>); } Err(e) => log::warn!("thumbnail OCIO: {e}") }
                }).ok();
            }
        }
        tc.pending = Some((key, rx));
        tc.generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_bypass_thumbnail_is_as_stored() {
        let Ok(en) = Engine::headless(wgpu::TextureFormat::Rgba16Float) else { eprintln!("no GPU — skipped"); return };
        let mut tc = ThumbColor { engine: Some(en), meta: Some(min_present::ColorMeta { transfer: min_present::Transfer::Bt1886, ..Default::default() }), ..Default::default() };
        let rgba: Vec<u8> = (0..32 * 18).flat_map(|i| { let v = (i % 32 * 8) as u8; [v, 255 - v, 128, 255] }).collect();
        let img = tc.convert(&ThumbPixels::from_rgba8(32, 18, &rgba)).unwrap();
        if std::env::var_os("QCV_THUMB_TIMING").is_some() {
            let big: Vec<u8> = (0..320 * 180).flat_map(|i| [(i % 251) as u8, (i % 199) as u8, 128, 255]).collect();
            let px = ThumbPixels::from_rgba8(320, 180, &big); let _ = tc.convert(&px);
            let t = std::time::Instant::now(); for _ in 0..20 { let _ = tc.convert(&px); }
            eprintln!("look stage, 320×180: {:.2} ms per thumbnail", t.elapsed().as_secs_f64() * 1000.0 / 20.0);
        }
        let out = img.image.data.data();
        let worst = rgba.iter().zip(out.iter()).enumerate().filter(|(i, _)| i % 4 != 3).map(|(_, (a, b))| (*a as i32 - *b as i32).abs()).max().unwrap();
        assert!(worst <= 2, "code values round-trip within 2: worst {worst}");
    }
    #[test]
    fn a_linear_exr_thumbnail_reads_like_the_viewport() {
        let Ok(en) = Engine::headless(wgpu::TextureFormat::Rgba16Float) else { eprintln!("no GPU — skipped"); return };
        let mut tc = ThumbColor { engine: Some(en), meta: Some(min_present::ColorMeta { transfer: min_present::Transfer::Linear, ..Default::default() }), ..Default::default() };
        // a linear ramp, deep shadows included (8-bit linear would have flattened them)
        let v: Vec<u16> = (0..32 * 18).flat_map(|i| { let x = (i % 32) as f32 / 31.0; let l = x * x * x; [half::f16::from_f32(l).to_bits(), half::f16::from_f32(l * 0.5).to_bits(), half::f16::from_f32(0.18).to_bits()] }).collect();
        let px = ThumbPixels { width: 32, height: 18, samples: ThumbSamples::Half(v) };
        let img = tc.convert(&px).unwrap();
        let (out, want) = (img.image.data.data(), px.rgba8());
        let worst = want.iter().zip(out.iter()).enumerate().filter(|(i, _)| i % 4 != 3).map(|(_, (a, b))| (*a as i32 - *b as i32).abs()).max().unwrap();
        assert!(worst <= 2, "linear → the sRGB encode, as the viewport's bypass: worst {worst}");
        let distinct: std::collections::HashSet<u8> = out.chunks(4).take(32).map(|c| c[0]).collect();
        assert!(distinct.len() >= 28, "the shadows keep their steps: {} distinct of 32", distinct.len());
    }
}
