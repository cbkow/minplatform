//! QCView's colour engines for min-present's stage-2 slot (canon §18; rows 198–204, ruled 2026-11-08).
//!
//! minColor — the default engine — is the vendored minColorAE core (`core/`, GPL-3.0-only, see `core/VENDORED.md`):
//! the GPU kernel is the core through its own GLSL shim, turned into WGSL by naga at build time, so the maths are never
//! translated by hand; the host derive is the same core in C++ (`src/ffi.cpp` = QCView's `mincolor::resolve` without Qt).
//! One compute pass at the video's size, every control a uniform: a change never recompiles (row 204).
//!
//! The display is fixed (rows 179 / 181): the linear Rec.709 hand-off for the one extended-linear surface, 1.0 = 100
//! nits. Rendering (un-tone-mapped · OpenDRT · AgX) and the Knee stay; the Display choice is gone.
use min_present::{ColorPass, Space, Transfer};

/// One side's chain (QCView's `MinColorChain`: Input · Knee · AgX · Output), flattened for the C side.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Chain {
    /// Input: the file's encoding (`DRT_IN_*` gamut, `DRT_OETF_*` transfer, limited-range codes).
    pub in_gamut: i32, pub in_transfer: i32, pub in_limited: i32,
    /// The Highlight Knee: source / target peak nits, start (< 0 = BT.2390's).
    pub knee_on: i32, pub knee_src: f32, pub knee_tgt: f32, pub knee_start: f32,
    /// AgX: target gamut (0 Rec.2020 · 1 Rec.709 · 2 P3-D65), peak nits and the curve.
    pub agx_on: i32, pub agx_target: i32, pub agx_peak: f32, pub agx_white_ev: f32, pub agx_black_ev: f32, pub agx_contrast: f32, pub agx_toe: f32, pub agx_shoulder: f32, pub agx_hue_restore: f32, pub agx_hdr_purity: f32, pub agx_outset: f32,
    /// Rendering: OpenDRT (else un-tone-mapped), its look / tonescale / creative white, surround, peak nits, grey boost, purity, grey nits.
    pub open_drt: i32, pub look: i32, pub tonescale: i32, pub cwp: i32, pub cwp_limit: f32, pub surround: i32, pub peak_nits: f32, pub grey_boost: f32, pub hdr_purity: f32, pub grey_nits: f32,
}

/// `DRT_IN_*` / `DRT_OETF_*` indices the app names (the vendored `opendrt_params.h`).
pub mod ids {
    pub const IN_REC2020: i32 = 4;
    pub const IN_REC709: i32 = 5;
    pub const OETF_LINEAR: i32 = 0;
    pub const OETF_REC1886: i32 = 10;
}

impl Default for Chain {
    /// QCView's defaults (`mincolor_chain.h`): Rec.709 / Rec.1886 in, knee and AgX off, un-tone-mapped at 100 nits.
    fn default() -> Chain {
        Chain { in_gamut: ids::IN_REC709, in_transfer: ids::OETF_REC1886, in_limited: 0,
            knee_on: 0, knee_src: 1000.0, knee_tgt: 100.0, knee_start: -1.0,
            agx_on: 0, agx_target: 1, agx_peak: 100.0, agx_white_ev: 6.5, agx_black_ev: -10.0, agx_contrast: 2.4, agx_toe: 1.5, agx_shoulder: 1.5, agx_hue_restore: 0.6, agx_hdr_purity: 0.5, agx_outset: 1.0,
            open_drt: 0, look: 0, tonescale: 0, cwp: 0, cwp_limit: 0.25, surround: 0, peak_nits: 100.0, grey_boost: 0.13, hdr_purity: 0.5, grey_nits: 10.0 }
    }
}

unsafe extern "C" {
    fn mc_gpu_size() -> usize;
    fn mc_chain_size() -> usize;
    fn mc_resolve(c: *const Chain, capture: i32, g: *mut u8);
    fn mc_apply(g: *const u8, rgb: *mut f32);
    fn mc_look_count() -> i32;
    fn mc_look_name(i: i32) -> *const std::ffi::c_char;
    fn mc_in_gamut_count() -> i32;
    fn mc_in_gamut_name(i: i32) -> *const std::ffi::c_char;
    fn mc_in_transfer_count() -> i32;
    fn mc_in_transfer_name(i: i32) -> *const std::ffi::c_char;
}

/// The derived GPU blocks: `DrtParams` in · `DrtParams` out · `DrtAgxParams` · flags, the bytes the kernel binds.
#[derive(Clone, PartialEq)]
pub struct Blocks(Vec<u8>);
const PARAMS: usize = 124 * 4;
const AGX: usize = 84 * 4;
const FLAGS: usize = 16;
const SPANS: [(usize, usize); 4] = [(0, PARAMS), (PARAMS, PARAMS), (2 * PARAMS, AGX), (2 * PARAMS + AGX, FLAGS)];

impl Blocks {
    pub fn bytes(&self) -> &[u8] { &self.0 }
}

/// Derive the chain's blocks. `capture`: the sRGB 2.2 display at peak 100 (QCView's SDR capture rule) instead of the
/// screen's linear hand-off.
pub fn resolve(chain: &Chain, capture: bool) -> Blocks {
    unsafe {
        assert_eq!(mc_chain_size(), std::mem::size_of::<Chain>(), "Chain layout drifted from ffi.cpp");
        let n = mc_gpu_size();
        assert_eq!(n, 2 * PARAMS + AGX + FLAGS, "the core's block sizes changed");
        let mut v = vec![0u8; n];
        mc_resolve(chain, capture as i32, v.as_mut_ptr());
        Blocks(v)
    }
}

/// The CPU reference of one pixel (the core in C++): the file's R'G'B' in, linear Rec.709 out (1.0 = 100 nits).
pub fn apply_cpu(b: &Blocks, rgb: [f32; 3]) -> [f32; 3] {
    let mut v = rgb;
    unsafe { mc_apply(b.0.as_ptr(), v.as_mut_ptr()); }
    v
}

/// The OpenDRT looks, by index (the vendored `kLooks`).
pub fn look_names() -> Vec<String> {
    unsafe { (0..mc_look_count()).map(|i| std::ffi::CStr::from_ptr(mc_look_name(i)).to_string_lossy().into_owned()).collect() }
}

/// The Input gamuts and transfers, by `DRT_IN_*` / `DRT_OETF_*` index (the non-inverse transfers only).
pub fn input_gamut_names() -> Vec<String> { unsafe { (0..mc_in_gamut_count()).map(|i| std::ffi::CStr::from_ptr(mc_in_gamut_name(i)).to_string_lossy().into_owned()).collect() } }
pub fn input_transfer_names() -> Vec<String> { unsafe { (0..mc_in_transfer_count()).map(|i| std::ffi::CStr::from_ptr(mc_in_transfer_name(i)).to_string_lossy().into_owned()).collect() } }

/// The generated kernel (the core through naga), for tests and inspection.
pub const KERNEL_WGSL: &str = include_str!(concat!(env!("OUT_DIR"), "/mincolor.wgsl"));

/// minColor in the stage-2 slot: reads the source's R'G'B' (stage 1 after matrix + range only — QCView hands the chain
/// the same), writes linear Rec.709 at 1.0 = 100 nits.
pub struct MinColorPass {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    ubuf: wgpu::Buffer,
    sampler: wgpu::Sampler,
    chain: Chain,
    blocks: Option<Blocks>,
}

/// The four blocks' offsets in the one read-only storage buffer (256-aligned for every backend; std430 = the C layout).
const SLOT: u64 = 512;

impl MinColorPass {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, chain: Chain) -> MinColorPass {
        let t0 = std::time::Instant::now();
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("minColor"), source: wgpu::ShaderSource::Wgsl(KERNEL_WGSL.into()) });
        let u = |b: u32, size: usize| wgpu::BindGroupLayoutEntry { binding: b, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: wgpu::BufferSize::new(size as u64) }, count: None };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("minColor"), entries: &[
            u(0, PARAMS), u(1, PARAMS), u(2, AGX), u(3, FLAGS),
            wgpu::BindGroupLayoutEntry { binding: 4, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 5, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering), count: None },
            wgpu::BindGroupLayoutEntry { binding: 6, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format: wgpu::TextureFormat::Rgba16Float, view_dimension: wgpu::TextureViewDimension::D2 }, count: None },
        ] });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("minColor"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some("minColor"), layout: Some(&pl), module: &module, entry_point: Some("main"), compilation_options: Default::default(), cache: None });
        let ubuf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("minColor blocks"), size: 3 * SLOT + FLAGS as u64, usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { label: Some("minColor"), ..Default::default() });
        log::info!("minColor kernel ready ({:.0} ms)", t0.elapsed().as_secs_f64() * 1000.0);
        MinColorPass { device: device.clone(), queue: queue.clone(), pipeline, layout, ubuf, sampler, chain, blocks: None }
    }
    pub fn chain(&self) -> &Chain { &self.chain }
    /// A new chain: only the uniform bytes change (written before the next run).
    pub fn set_chain(&mut self, chain: Chain) { if chain != self.chain { self.chain = chain; self.blocks = None; } }
}

impl ColorPass for MinColorPass {
    fn input(&self) -> Space { Space::Encoded(Transfer::Bt1886) }
    fn output(&self) -> Space { Space::Linear }
    fn run(&mut self, enc: &mut wgpu::CommandEncoder, src: &wgpu::Texture, dst: &wgpu::Texture) {
        if self.blocks.is_none() {
            let b = resolve(&self.chain, false);
            for (i, (o, n)) in SPANS.iter().enumerate() { self.queue.write_buffer(&self.ubuf, i as u64 * SLOT, &b.0[*o..o + n]); }
            self.blocks = Some(b);
        }
        let sv = src.create_view(&Default::default()); let dv = dst.create_view(&Default::default());
        let ub = |i: u64, n: usize| wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &self.ubuf, offset: i * SLOT, size: wgpu::BufferSize::new(n as u64) });
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("minColor"), layout: &self.layout, entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: ub(0, PARAMS) }, wgpu::BindGroupEntry { binding: 1, resource: ub(1, PARAMS) },
            wgpu::BindGroupEntry { binding: 2, resource: ub(2, AGX) }, wgpu::BindGroupEntry { binding: 3, resource: ub(3, FLAGS) },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&sv) },
            wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&dv) },
        ] });
        let size = dst.size();
        let mut p = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("minColor"), timestamp_writes: None });
        p.set_pipeline(&self.pipeline); p.set_bind_group(0, &bind, &[]);
        p.dispatch_workgroups(size.width.div_ceil(8), size.height.div_ceil(8), 1);
    }
}

#[cfg(test)]
mod tests;
