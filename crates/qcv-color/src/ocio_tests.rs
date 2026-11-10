//! OCIO on the GPU against OCIO's own CPU processor, on the three bundled configs (row 198), through the one display.
//! Configs: `QCV_OCIO_CONFIGS` (a folder holding Blender5.2/ ACES_1.3/ ACES_2.0/), else the app's own copy
//! (`apps/qcv/assets/OCIO`, moved here from QCView-Player 2026-11-09). Skips when absent.
use super::ocio::*;

pub fn configs_dir() -> Option<std::path::PathBuf> {
    let p = std::env::var_os("QCV_OCIO_CONFIGS").map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/qcv/assets/OCIO"));
    p.join("Blender5.2/config.ocio").exists().then_some(p)
}

fn open(name: &str) -> Option<std::sync::Arc<Config>> {
    let d = configs_dir()?;
    Some(Config::open(d.join(name).join("config.ocio").to_str().unwrap()).unwrap_or_else(|e| panic!("{name}: {e}")))
}

fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
    let inst = min_present::Presenter::instance();   // the app's backend: DX12 on Windows (the default picks Vulkan there)
    let a = pollster::block_on(inst.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
    let f = a.features() & (wgpu::Features::PASSTHROUGH_SHADERS | wgpu::Features::FLOAT32_FILTERABLE);
    if !f.contains(wgpu::Features::PASSTHROUGH_SHADERS) { return None }
    pollster::block_on(a.request_device(&wgpu::DeviceDescriptor { required_features: f, ..Default::default() })).ok()
}

#[test]
fn the_one_display_carries_every_view_and_a_true_linear_encoding() {
    let Some(b) = open("Blender5.2") else { eprintln!("no configs — skipped"); return };
    let v = b.views();
    for want in ["Standard", "AgX", "ACES 2.0 - HDR 1000 nits", "Raw"] { assert!(v.iter().any(|x| x == want), "Blender 5.2 views {v:?}"); }
    // Standard on linear Rec.709 = no change: the display colourspace's XYZ → Rec.709 matrix is the inverse of the config's
    let c = Chain::build(&b, &Spec { input: "Linear Rec.709".into(), view: "Standard".into(), ..Default::default() }).unwrap();
    let mut px = vec![0.18f32, 0.18, 0.18, 1.0, 0.9, 0.2, 0.05, 1.0, 0.02, 0.5, 0.8, 1.0];
    let want = px.clone();
    c.apply_cpu(&mut px);
    for (a, w) in px.iter().zip(&want) { assert!((a - w).abs() < 2e-4, "Standard on linear Rec.709: {px:?} vs {want:?}"); }
    for name in ["ACES_1.3", "ACES_2.0"] {
        let Some(c) = open(name) else { continue };
        eprintln!("{name}: {} inputs, {} looks, {} views: {:?}", c.inputs().len(), c.looks().len(), c.views().len(), c.views());
        assert!(c.views().iter().any(|x| x.contains("SDR")) && c.views().iter().any(|x| x.contains("HDR")), "{name}: {:?}", c.views());
    }
}

#[test]
fn native_shaders_match_the_cpu_processor() {
    let Some((dev, q)) = gpu() else { eprintln!("no GPU with passthrough — skipped"); return };
    let Some(blender) = open("Blender5.2") else { eprintln!("no configs — skipped"); return };
    let aces2 = open("ACES_2.0").unwrap();
    let aces13 = open("ACES_1.3").unwrap();
    let pick = |c: &Config, needle: &str| c.views().into_iter().find(|v| v.contains(needle)).unwrap_or_else(|| panic!("no view with {needle:?} in {:?}", c.views()));
    let inp = |c: &Config, needle: &str| c.inputs().into_iter().find(|v| v == needle || v.contains(needle)).unwrap_or_else(|| panic!("no input {needle:?}"));
    let cases: Vec<(&std::sync::Arc<Config>, Spec, bool)> = vec![
        (&blender, Spec { input: "Rec.1886".into(), view: "Standard".into(), ..Default::default() }, false),
        (&blender, Spec { input: "Linear Rec.709".into(), view: "AgX".into(), ..Default::default() }, true),
        (&blender, Spec { input: "Linear Rec.709".into(), view: "ACES 2.0 - HDR 1000 nits".into(), ..Default::default() }, true),
        (&aces2, Spec { input: inp(&aces2, "ACEScg"), view: pick(&aces2, "SDR 100 nits (Rec.709)"), ..Default::default() }, true),
        (&aces2, Spec { input: inp(&aces2, "ACEScg"), view: pick(&aces2, "HDR 1000 nits (P3 D65)"), ..Default::default() }, true),
        (&aces13, Spec { input: inp(&aces13, "ACEScg"), view: aces13.views().into_iter().find(|v| v != "Raw").unwrap(), ..Default::default() }, true),
    ];
    let mut worst = 0.0f32;
    for (cfg, spec, hdr) in cases {
        let mut px: Vec<[f32; 3]> = (0..=32).map(|i| { let x = i as f32 / 32.0; [x, x, x] }).collect();
        px.extend([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.9, 0.05, 0.02], [0.18, 0.18, 0.18], [0.5, 0.4, 0.3]]);
        if hdr { for k in [2.0f32, 8.0, 40.0] { px.push([k, k, k]); px.push([k, k * 0.2, 0.0]); } }
        let mut pass = OcioPass::new(&dev, &q, cfg, spec.clone(), crate::Knee::default()).unwrap_or_else(|e| panic!("{spec:?}: {e}"));
        let got = crate::tests::run_gpu_pass(&dev, &q, &mut pass, &px);
        let chain = Chain::build(cfg, &spec).unwrap();
        let mut cpu: Vec<f32> = px.iter().flat_map(|p| [p[0], p[1], p[2], 1.0]).collect();
        chain.apply_cpu(&mut cpu);
        for (i, g) in got.iter().enumerate() {
            for c in 0..3 {
                let w = cpu[i * 4 + c];
                assert!(!g[c].is_nan(), "{spec:?}: NaN for {:?}", px[i]);
                let err = (g[c] - w).abs() / w.abs().max(0.05);
                worst = worst.max(err);
                assert!(err < 6e-3, "{} / {spec:?}: {:?} → GPU {g:?} vs CPU {:?}", cfg.path, px[i], &cpu[i * 4..i * 4 + 3]);
            }
        }
        eprintln!("{} → {}: built in {:.0} ms", spec.input, spec.view, pass.build_ms);
    }
    eprintln!("OCIO native vs CPU processor: worst relative error {worst:.2e} (f16 output)");
}

#[test]
fn the_knee_split_matches_the_cpu() {
    let Some((dev, q)) = gpu() else { eprintln!("no GPU with passthrough — skipped"); return };
    let Some(blender) = open("Blender5.2") else { eprintln!("no configs — skipped"); return };
    let aces2 = open("ACES_2.0").unwrap();
    let hdr_view = aces2.views().into_iter().find(|v| v.contains("HDR 1000 nits (P3 D65)")).unwrap();
    let cg = aces2.inputs().into_iter().find(|v| v.contains("ACEScg")).unwrap();
    let cases = [
        (&blender, Spec { input: "Linear Rec.709".into(), view: "Standard".into(), ..Default::default() }, crate::Knee { enabled: true, source_nits: 1000.0, target_nits: 1000.0, start: -1.0 }),
        (&blender, Spec { input: "Linear Rec.709".into(), view: "Standard".into(), ..Default::default() }, crate::Knee { enabled: true, source_nits: 2000.0, target_nits: 1000.0, start: 0.6 }),
        (&aces2, Spec { input: cg, view: hdr_view.clone(), ..Default::default() }, crate::Knee { enabled: true, source_nits: 4000.0, target_nits: 1000.0, start: -1.0 }),
    ];
    let mut worst = 0.0f32;
    for (cfg, spec, knee) in cases {
        let mut px: Vec<[f32; 3]> = (0..=24).map(|i| { let x = (i as f32 / 4.0).exp2() / 64.0; [x, x, x] }).collect();   // 1/64 .. 64 (≈ 6400 nits)
        px.extend([[20.0, 4.0, 1.0], [1.0, 30.0, 2.0], [0.5, 0.5, 9.0]]);
        let mut pass = OcioPass::new(&dev, &q, cfg, spec.clone(), knee).unwrap_or_else(|e| panic!("{spec:?}: {e}"));
        assert!(pass.is_split(), "{spec:?} did not split");
        let got = crate::tests::run_gpu_pass(&dev, &q, &mut pass, &px);
        let (pre, post, side) = Chain::build_split(cfg, &spec).unwrap();
        let mut cpu: Vec<f32> = px.iter().flat_map(|p| [p[0], p[1], p[2], 1.0]).collect();
        pre.apply_cpu(&mut cpu);
        for c in cpu.chunks_mut(4) { let k = crate::knee_cpu(&knee, side, view_is_sdr(&spec.view), [c[0], c[1], c[2]]); c[..3].copy_from_slice(&k); }
        post.apply_cpu(&mut cpu);
        // the knee must have bitten: the brightest grey through the Knee is darker than without it
        let mut plain = vec![64.0f32, 64.0, 64.0, 1.0]; Chain::build(cfg, &spec).unwrap().apply_cpu(&mut plain);
        assert!(cpu[24 * 4] < plain[0] * 0.95, "{spec:?}: the Knee did nothing ({} vs {})", cpu[24 * 4], plain[0]);
        for (i, g) in got.iter().enumerate() {
            for c in 0..3 {
                let w = cpu[i * 4 + c];
                assert!(!g[c].is_nan(), "{spec:?}: NaN for {:?}", px[i]);
                // two extra f16 hand-offs (pre → knee → post; row 202: half float, exactness not a gate). A saturated highlight's
                // small channel comes out of the interchange → Rec.2020 matrix as a difference of large f16 values, so its error is
                // absolute (≈ 1e-3 of the pixel's level), not relative: measure against the pixel's brightest channel too.
                let level = cpu[i * 4..i * 4 + 3].iter().fold(0.0f32, |a, b| a.max(b.abs()));
                let err = (g[c] - w).abs() / w.abs().max(0.05).max(level * 0.1);
                worst = worst.max(err);
                assert!(err < 8e-3, "{spec:?} {knee:?}: {:?} → GPU {g:?} vs CPU {:?}", px[i], &cpu[i * 4..i * 4 + 3]);
            }
        }
        eprintln!("{} → {} with {:?}: split built in {:.0} ms", spec.input, spec.view, knee, pass.build_ms);
    }
    eprintln!("OCIO + Knee split vs CPU: worst relative error {worst:.2e}");
}

/// A scratch folder holding the test's LUT files: a 2³ `.cube` that halves every value, and a `.ccc` with two corrections
/// ("double" first, then "half").
fn lut_files(tag: &str) -> (std::path::PathBuf, String, String) {
    let dir = std::env::temp_dir().join(format!("qcv-ocio-luts-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cube = dir.join("half.cube");
    let mut s = String::from("TITLE \"half\"\nLUT_3D_SIZE 2\n");
    for b in 0..2 { for g in 0..2 { for r in 0..2 { s += &format!("{} {} {}\n", r as f32 * 0.5, g as f32 * 0.5, b as f32 * 0.5); } } }
    std::fs::write(&cube, s).unwrap();
    let ccc = dir.join("two.ccc");
    let cc = |id: &str, k: f32| format!("<ColorCorrection id=\"{id}\"><SOPNode><Slope>{k} {k} {k}</Slope><Offset>0 0 0</Offset><Power>1 1 1</Power></SOPNode><SatNode><Saturation>1</Saturation></SatNode></ColorCorrection>");
    std::fs::write(&ccc, format!("<ColorCorrectionCollection xmlns=\"urn:ASC:CDL:v1.2\">{}{}</ColorCorrectionCollection>", cc("double", 2.0), cc("half", 0.5))).unwrap();
    (dir, cube.to_string_lossy().into_owned(), ccc.to_string_lossy().into_owned())
}

#[test]
fn a_scene_lut_and_a_correction_id_act_on_the_inputs_values() {
    let Some(b) = open("Blender5.2") else { eprintln!("no configs — skipped"); return };
    let (dir, cube, ccc) = lut_files("cpu");
    let base = Spec { input: "Rec.1886".into(), view: "Standard".into(), ..Default::default() };
    let run = |spec: &Spec, px: &[f32]| { let mut v = px.to_vec(); Chain::build(&b, spec).unwrap_or_else(|e| panic!("{spec:?}: {e}")).apply_cpu(&mut v); v };
    let px = [0.8f32, 0.4, 0.2, 1.0, 0.5, 0.5, 0.5, 1.0];
    let scaled = |k: f32| -> Vec<f32> { px.chunks(4).flat_map(|c| [c[0] * k, c[1] * k, c[2] * k, c[3]]).collect() };
    let close = |a: &[f32], w: &[f32], what: &str| for (x, y) in a.iter().zip(w) { assert!((x - y).abs() < 2e-3, "{what}: {a:?} vs {w:?}"); };
    // the file acts on the Input's own values, before the view: the LUT'd picture = the plain chain on the scaled values
    close(&run(&Spec { scene_lut: cube.clone(), ..base.clone() }, &px), &run(&base, &scaled(0.5)), "a .cube that halves");
    // a .ccc: no id = the first correction; an id picks its own; an index works too (QCView: \"its id, or an index\")
    close(&run(&Spec { scene_lut: ccc.clone(), ..base.clone() }, &scaled(0.25)), &run(&base, &scaled(0.5)), "no id = the first (double)");
    close(&run(&Spec { scene_lut: ccc.clone(), ccc_id: "half".into(), ..base.clone() }, &px), &run(&base, &scaled(0.5)), "id half");
    close(&run(&Spec { scene_lut: ccc.clone(), ccc_id: "1".into(), ..base.clone() }, &px), &run(&base, &scaled(0.5)), "index 1 = half");
    // what cannot load says so: an unknown id, a missing file — never a silent chain without the LUT
    assert!(Chain::build(&b, &Spec { scene_lut: ccc.clone(), ccc_id: "nope".into(), ..base.clone() }).is_err(), "an unknown Correction ID builds");
    assert!(Chain::build(&b, &Spec { scene_lut: dir.join("gone.cube").to_string_lossy().into_owned(), ..base.clone() }).is_err(), "a missing LUT builds");
    // and with the Knee on, the split's pre half carries it
    let (pre, _, _) = Chain::build_split(&b, &Spec { input: "Linear Rec.709".into(), view: "Standard".into(), scene_lut: ccc.clone(), ccc_id: "half".into(), ..Default::default() }).unwrap();
    let (plain, _, _) = Chain::build_split(&b, &Spec { input: "Linear Rec.709".into(), view: "Standard".into(), ..Default::default() }).unwrap();
    let (mut a, mut w) = (px.to_vec(), scaled(0.5)); pre.apply_cpu(&mut a); plain.apply_cpu(&mut w);
    close(&a, &w, "the split's pre half");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_scene_lut_on_the_gpu_matches_the_cpu() {
    let Some((dev, q)) = gpu() else { eprintln!("no GPU with passthrough — skipped"); return };
    let Some(blender) = open("Blender5.2") else { eprintln!("no configs — skipped"); return };
    let (dir, cube, ccc) = lut_files("gpu");
    for spec in [Spec { input: "Rec.1886".into(), view: "Standard".into(), scene_lut: cube, ..Default::default() }, Spec { input: "Rec.1886".into(), view: "Standard".into(), scene_lut: ccc, ccc_id: "half".into(), ..Default::default() }] {
        let mut px: Vec<[f32; 3]> = (0..=32).map(|i| { let x = i as f32 / 32.0; [x, x, x] }).collect();
        px.extend([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.9, 0.05, 0.02], [0.5, 0.4, 0.3]]);
        let mut pass = OcioPass::new(&dev, &q, &blender, spec.clone(), crate::Knee::default()).unwrap_or_else(|e| panic!("{spec:?}: {e}"));
        let got = crate::tests::run_gpu_pass(&dev, &q, &mut pass, &px);
        let mut cpu: Vec<f32> = px.iter().flat_map(|p| [p[0], p[1], p[2], 1.0]).collect();
        Chain::build(&blender, &spec).unwrap().apply_cpu(&mut cpu);
        for (i, g) in got.iter().enumerate() { for c in 0..3 {
            let w = cpu[i * 4 + c]; let err = (g[c] - w).abs() / w.abs().max(0.05);
            assert!(err < 6e-3, "{spec:?}: {:?} → GPU {g:?} vs CPU {:?}", px[i], &cpu[i * 4..i * 4 + 3]);
        } }
        eprintln!("{} + {}: built in {:.0} ms", spec.input, spec.scene_lut.rsplit('/').next().unwrap_or(""), pass.build_ms);
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_exported_cube_read_back_as_a_scene_lut_is_the_chain() {
    // Export LUT (canon row 257): the cube maps the Input's code values to the chain's SDR picture, sRGB-encoded. Read back by
    // OCIO as a Scene LUT in front of an identity view (Standard on linear Rec.709), it must give what the chain gives.
    let Some(b) = open("Blender5.2") else { eprintln!("no configs — skipped"); return };
    let dir = std::env::temp_dir().join(format!("qcv-ocio-bake-{}", std::process::id())); std::fs::create_dir_all(&dir).unwrap();
    let srgb = |v: f32| if v <= 0.0031308 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    for (case, (spec, knee)) in [
        (Spec { input: "Rec.1886".into(), view: "Standard".into(), ..Default::default() }, crate::Knee::default()),
        (Spec { input: "Rec.1886".into(), view: "AgX".into(), ..Default::default() }, crate::Knee::default()),
        (Spec { input: "Rec.1886".into(), view: "Standard".into(), ..Default::default() }, crate::Knee { enabled: true, source_nits: 400.0, target_nits: 100.0, start: -1.0 }),
    ].into_iter().enumerate() {
        let t0 = std::time::Instant::now();
        let (rgb, skipped) = crate::bake::bake_ocio(&b, &spec, knee, crate::bake::CUBE_SIZE).unwrap();
        let ms = t0.elapsed().as_secs_f64() * 1000.0;
        assert!(!skipped, "{spec:?}: the Knee was left out");
        let path = dir.join(format!("out{case}.cube"));   // a name of its own: OCIO caches a LUT file by its path
        crate::bake::write_atomic(&path, &crate::bake::cube_text("OCIO Transform LUT", crate::bake::CUBE_SIZE, &rgb)).unwrap();
        let back = Chain::build(&b, &Spec { input: "Linear Rec.709".into(), view: "Standard".into(), scene_lut: path.to_string_lossy().into_owned(), ..Default::default() }).unwrap();
        let px = [0.1f32, 0.1, 0.1, 1.0, 0.5, 0.5, 0.5, 1.0, 0.9, 0.3, 0.2, 1.0, 0.25, 0.75, 0.5, 1.0];
        let mut got = px.to_vec(); back.apply_cpu(&mut got);
        // the chain itself, with the same Knee
        let mut want = px.to_vec();
        if knee.enabled { let (pre, post, side) = Chain::build_split(&b, &spec).unwrap(); pre.apply_cpu(&mut want); for c in want.chunks_mut(4) { let k = crate::knee_cpu(&knee, side, view_is_sdr(&spec.view), [c[0], c[1], c[2]]); c[..3].copy_from_slice(&k); } post.apply_cpu(&mut want); }
        else { Chain::build(&b, &spec).unwrap().apply_cpu(&mut want); }
        for (i, (g, w)) in got.iter().zip(&want).enumerate() { if i % 4 == 3 { continue } assert!((g - srgb(w.max(0.0))).abs() < 4e-3, "{spec:?} {knee:?}: {got:?} vs sRGB of {want:?}"); }
        eprintln!("{} → {} (knee {}): 65³ baked in {ms:.0} ms", spec.input, spec.view, knee.enabled);
    }
    let _ = std::fs::remove_dir_all(dir);
}
