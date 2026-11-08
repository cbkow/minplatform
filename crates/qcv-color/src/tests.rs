//! The kernel through naga against the C++ core, pixel for pixel, on the chains QCView ships and on the inputs that
//! broke Metal's fast math in QCView (saturated primaries, out-of-range codes): the row-204 check for minColor.
use super::*;

fn f16_to_f32(h: u16) -> f32 {
    let s = (h >> 15) as u32; let e = ((h >> 10) & 0x1f) as u32; let m = (h & 0x3ff) as u32;
    let bits = if e == 0 { if m == 0 { s << 31 } else { let mut e2 = 127 - 15 + 1; let mut m2 = m; while m2 & 0x400 == 0 { m2 <<= 1; e2 -= 1; } (s << 31) | (e2 << 23) | ((m2 & 0x3ff) << 13) } }
        else if e == 31 { (s << 31) | 0x7f80_0000 | (m << 13) } else { (s << 31) | ((e + 127 - 15) << 23) | (m << 13) };
    f32::from_bits(bits)
}

fn gpu() -> Option<(wgpu::Device, wgpu::Queue)> {
    let inst = min_present::Presenter::instance();   // the app's backend: DX12 on Windows (the default picks Vulkan there)
    let a = pollster::block_on(inst.request_adapter(&wgpu::RequestAdapterOptions::default())).ok()?;
    pollster::block_on(a.request_device(&wgpu::DeviceDescriptor::default())).ok()
}

/// Run `pass` over `px` (one row) on the GPU; the f16 results.
fn run_gpu(dev: &wgpu::Device, q: &wgpu::Queue, pass: &mut MinColorPass, px: &[[f32; 3]]) -> Vec<[f32; 3]> { run_gpu_pass(dev, q, pass, px) }
pub(crate) fn run_gpu_pass(dev: &wgpu::Device, q: &wgpu::Queue, pass: &mut dyn ColorPass, px: &[[f32; 3]]) -> Vec<[f32; 3]> {
    let w = px.len() as u32;
    let size = wgpu::Extent3d { width: w, height: 1, depth_or_array_layers: 1 };
    let src = dev.create_texture(&wgpu::TextureDescriptor { label: None, size, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba32Float, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] });
    let dst = dev.create_texture(&wgpu::TextureDescriptor { label: None, size, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba16Float, usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC, view_formats: &[] });
    let data: Vec<f32> = px.iter().flat_map(|p| [p[0], p[1], p[2], 1.0]).collect();
    q.write_texture(src.as_image_copy(), bytemuck::cast_slice(&data), wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 16), rows_per_image: None }, size);
    let bpr = (w * 8).div_ceil(256) * 256;
    let buf = dev.create_buffer(&wgpu::BufferDescriptor { label: None, size: bpr as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
    let mut enc = dev.create_command_encoder(&Default::default());
    pass.run(&mut enc, &src, &dst);
    enc.copy_texture_to_buffer(dst.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bpr), rows_per_image: None } }, size);
    q.submit([enc.finish()]);
    buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    dev.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let v = buf.slice(..).get_mapped_range().unwrap();
    let h: &[u16] = bytemuck::cast_slice(&v[..(w * 8) as usize]);
    h.chunks(4).map(|c| [f16_to_f32(c[0]), f16_to_f32(c[1]), f16_to_f32(c[2])]).collect()
}

fn inputs(hdr: bool) -> Vec<[f32; 3]> {
    let mut v = vec![];
    for i in 0..=32 { let x = i as f32 / 32.0; v.push([x, x, x]); }
    for p in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 1.0, 0.0], [0.0, 1.0, 1.0], [1.0, 0.0, 1.0], [0.9, 0.05, 0.02], [0.02, 0.8, 0.1], [0.1, 0.1, 0.95], [1.2, -0.05, 0.3], [-0.02, 0.5, 1.1], [0.18, 0.18, 0.18], [0.5, 0.4, 0.3]] { v.push(p); }
    if hdr { for k in [2.0f32, 5.0, 10.0, 40.0, 100.0] { v.push([k, k, k]); v.push([k, k * 0.1, 0.0]); v.push([0.0, k * 0.3, k]); } }
    v
}

fn transfer(name: &str) -> i32 { input_transfer_names().iter().position(|n| n.contains(name)).unwrap_or_else(|| panic!("no transfer {name}: {:?}", input_transfer_names())) as i32 }
fn gamut(name: &str) -> i32 { input_gamut_names().iter().position(|n| n.contains(name)).unwrap_or_else(|| panic!("no gamut {name}: {:?}", input_gamut_names())) as i32 }

#[test]
fn kernel_matches_the_cpp_core() {
    let Some((dev, q)) = gpu() else { eprintln!("no GPU — skipped"); return };
    let lin = transfer("Linear");
    let mut chains: Vec<(&str, Chain, bool)> = vec![
        ("default Rec.709 / Rec.1886, un-tone-mapped", Chain::default(), false),
        ("linear Rec.709 + AgX", Chain { in_transfer: lin, agx_on: 1, ..Chain::default() }, true),
        ("linear Rec.2020 + OpenDRT 1000 nits", Chain { in_gamut: ids::IN_REC2020, in_transfer: lin, open_drt: 1, peak_nits: 1000.0, ..Chain::default() }, true),
        ("linear Rec.709 + Knee 1000 → 100", Chain { in_transfer: lin, knee_on: 1, ..Chain::default() }, true),
    ];
    if let Some(pq) = input_transfer_names().iter().position(|n| n.contains("PQ")) { chains.push(("P3 PQ + OpenDRT 100", Chain { in_gamut: gamut("P3"), in_transfer: pq as i32, open_drt: 1, ..Chain::default() }, false)); }
    for look in 1..look_names().len().min(4) as i32 { chains.push(("OpenDRT look", Chain { in_transfer: lin, open_drt: 1, look, ..Chain::default() }, true)); }
    let t0 = std::time::Instant::now();
    let _warm = MinColorPass::new(&dev, &q, Chain::default());
    eprintln!("minColor pipeline build (first, cold): {:.0} ms", t0.elapsed().as_secs_f64() * 1000.0);
    let mut worst = 0.0f32;
    for (name, chain, hdr) in chains {
        let mut pass = MinColorPass::new(&dev, &q, chain);
        let px = inputs(hdr);
        let got = run_gpu(&dev, &q, &mut pass, &px);
        let b = resolve(&chain, false);
        for (p, g) in px.iter().zip(&got) {
            let want = apply_cpu(&b, *p);
            for c in 0..3 {
                assert!(!(g[c].is_nan() && want[c].is_finite()), "{name}: NaN on the GPU for {p:?} (CPU {want:?}) — fast math?");
                if !want[c].is_finite() { continue }
                let err = (g[c] - want[c]).abs() / want[c].abs().max(0.05);
                worst = worst.max(err);
                assert!(err < 4e-3, "{name}: {p:?} → GPU {g:?} vs CPU {want:?}");
            }
        }
    }
    eprintln!("minColor kernel vs C++ core: worst relative error {worst:.2e} (f16 output)");
}

#[test]
fn names_and_defaults() {
    assert!(look_names().len() >= 5, "{:?}", look_names());
    assert_eq!(input_transfer_names()[ids::OETF_REC1886 as usize].contains("1886"), true, "{:?}", input_transfer_names());
    assert!(input_gamut_names()[ids::IN_REC709 as usize].contains("709"), "{:?}", input_gamut_names());
    // the default chain is the identity-ish conversion: Rec.1886 0.5 → 0.5^2.4 at 1.0 = 100 nits
    let y = apply_cpu(&resolve(&Chain::default(), false), [0.5, 0.5, 0.5]);
    assert!((y[0] - 0.5f32.powf(2.4)).abs() < 2e-3, "{y:?}");
}

#[test]
fn the_scope_tap_is_the_input_stage() {
    let Some((dev, q)) = gpu() else { eprintln!("no GPU — skipped"); return };
    // linear Rec.709 in, AgX on (the view must NOT reach the tap): the tap = the input in linear Rec.2020
    let mut pass = MinColorPass::new(&dev, &q, Chain { in_transfer: ids::OETF_LINEAR, agx_on: 1, ..Chain::default() });
    pass.set_scope_tap(true);
    let px = [[0.5f32, 0.5, 0.5], [1.0, 0.0, 0.0], [4.0, 4.0, 4.0]];
    let _ = run_gpu_pass(&dev, &q, &mut pass, &px);
    let (tap, m) = pass.scope_tap().expect("a tap");
    assert_eq!(m, IDENTITY3);
    let v = min_present::readback::read_rgba16f(&dev, &q, &tap, 3, 1);
    let want = [[0.5, 0.5, 0.5], [0.6274, 0.0691, 0.0164], [4.0, 4.0, 4.0]];
    for (i, w) in want.iter().enumerate() { for c in 0..3 { assert!((v[i * 4 + c] - w[c]).abs() < 4e-3 * w[c].max(0.1), "pixel {i}: tap {:?} vs {w:?}", &v[i * 4..i * 4 + 3]); } }
}
