//! OCIO on the GPU against OCIO's own CPU processor, on the three bundled configs (row 198), through the one display.
//! Configs: `QCV_OCIO_CONFIGS` (a folder holding Blender5.2/ ACES_1.3/ ACES_2.0/), else QCView's checkout beside this
//! repo (`../QCView-Player/assets/OCIO`) — where they ship from is a packaging question. Skips when absent.
use super::ocio::*;

pub fn configs_dir() -> Option<std::path::PathBuf> {
    let p = std::env::var_os("QCV_OCIO_CONFIGS").map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../QCView-Player/assets/OCIO"));
    p.join("Blender5.2/config.ocio").exists().then_some(p)
}

fn open(name: &str) -> Option<std::sync::Arc<Config>> {
    let d = configs_dir()?;
    Some(Config::open(d.join(name).join("config.ocio").to_str().unwrap()).unwrap_or_else(|e| panic!("{name}: {e}")))
}

fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
    let inst = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
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
        let mut pass = OcioPass::new(&dev, &q, cfg, spec.clone()).unwrap_or_else(|e| panic!("{spec:?}: {e}"));
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
