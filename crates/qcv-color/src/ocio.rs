//! OCIO in-process (canon rows 183 / 198 / 200 / 204): OpenColorIO 2.6 builds the chain and generates its shader; the
//! shader runs unchanged as a native pass (MSL 2.0 on Metal, HLSL SM 5.0 on DX12) through wgpu's passthrough modules —
//! S1 step 2's route A, exact to OCIO's CPU processor. One display, built in code from the config's own views
//! (`src/ocio.cpp`): the user picks config · input · look · view only.
//!
//! The wrapping is QCView's (metal_ocio_renderer.mm / d3d11_ocio_renderer.cpp, as S1 reproduced it): src texture,
//! dst storage texture, one shared linear+clamp sampler, the LUT textures — wgpu numbers each resource class in
//! bind-group-entry order, so on Metal the slots fall where QCView's kernel declares them and on DX12 the HLSL gets
//! explicit registers.
use min_present::{ColorPass, Space, Transfer};
use std::{borrow::Cow, ffi::{c_char, c_int, c_long, c_uint, CStr, CString}, sync::Arc};

#[repr(C)] struct RawConfig { _p: [u8; 0] }
#[repr(C)] struct RawChain { _p: [u8; 0] }
unsafe extern "C" {
    fn oc_version() -> *const c_char;
    fn oc_config_open(path: *const c_char, err: *mut c_char, n: usize) -> *mut RawConfig;
    fn oc_config_free(c: *mut RawConfig);
    fn oc_config_warning(c: *const RawConfig) -> *const c_char;
    fn oc_config_role(c: *const RawConfig, role: *const c_char) -> *const c_char;
    fn oc_config_count(c: *const RawConfig, what: c_int) -> c_int;
    fn oc_config_item(c: *const RawConfig, what: c_int, i: c_int) -> *const c_char;
    fn oc_config_family(c: *const RawConfig, cs: *const c_char) -> *const c_char;
    fn oc_config_encoding(c: *const RawConfig, cs: *const c_char) -> *const c_char;
    fn oc_chain_build(c: *const RawConfig, input: *const c_char, look: *const c_char, view: *const c_char, scene_lut: *const c_char, ccc_id: *const c_char, lang: c_int, fn_name: *const c_char, err: *mut c_char, n: usize) -> *mut RawChain;
    fn oc_chain_free(c: *mut RawChain);
    fn oc_chain_build_scope(c: *const RawConfig, input: *const c_char, lang: c_int, side: *mut c_int, err: *mut c_char, n: usize) -> *mut RawChain;
    fn oc_chain_build_split(c: *const RawConfig, input: *const c_char, look: *const c_char, view: *const c_char, scene_lut: *const c_char, ccc_id: *const c_char, lang: c_int, pre: *mut *mut RawChain, post: *mut *mut RawChain, err: *mut c_char, n: usize) -> c_int;
    fn oc_chain_shader(c: *const RawChain) -> *const c_char;
    fn oc_chain_num_uniforms(c: *const RawChain) -> c_uint;
    fn oc_chain_num_textures(c: *const RawChain) -> c_uint;
    fn oc_chain_texture(c: *const RawChain, i: c_uint, name: *mut *const c_char, w: *mut c_uint, h: *mut c_uint, rgb: *mut c_int, dim: *mut c_int, values: *mut *const f32) -> c_int;
    fn oc_chain_num_3d(c: *const RawChain) -> c_uint;
    fn oc_chain_texture3d(c: *const RawChain, i: c_uint, name: *mut *const c_char, edge: *mut c_uint, values: *mut *const f32) -> c_int;
    fn oc_chain_apply_rgba(c: *const RawChain, px: *mut f32, w: c_long, h: c_long);
}

pub fn version() -> String { unsafe { CStr::from_ptr(oc_version()) }.to_string_lossy().into_owned() }
fn s(p: *const c_char) -> String { unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned() }
fn cstr(v: &str) -> CString { CString::new(v).unwrap_or_default() }

/// A loaded config with the "Linear Screen" display added. OCIO's config is immutable once built; shared freely.
pub struct Config { raw: *mut RawConfig, pub path: String }
unsafe impl Send for Config {}
unsafe impl Sync for Config {}
impl Drop for Config { fn drop(&mut self) { unsafe { oc_config_free(self.raw) } } }

impl Config {
    pub fn open(path: &str) -> Result<Arc<Config>, String> {
        let mut err = vec![0u8; 4096];
        let raw = unsafe { oc_config_open(cstr(path).as_ptr(), err.as_mut_ptr() as *mut c_char, err.len()) };
        if raw.is_null() { return Err(CStr::from_bytes_until_nul(&err).map(|c| c.to_string_lossy().into_owned()).unwrap_or_default()) }
        let w = s(unsafe { oc_config_warning(raw) });
        if !w.is_empty() { log::warn!("OCIO config {path}: {w}"); }
        Ok(Arc::new(Config { raw, path: path.to_string() }))
    }
    fn list(&self, what: i32) -> Vec<String> { unsafe { (0..oc_config_count(self.raw, what)).map(|i| s(oc_config_item(self.raw, what, i))).collect() } }
    /// The input colourspaces (every non-data colourspace but our display's).
    pub fn inputs(&self) -> Vec<String> { self.list(0) }
    pub fn looks(&self) -> Vec<String> { self.list(1) }
    /// The views on the one display, the sRGB display's first.
    pub fn views(&self) -> Vec<String> { self.list(2) }
    /// A role's colourspace ("scene_linear" …), "" when the config has none.
    pub fn role(&self, role: &str) -> String { s(unsafe { oc_config_role(self.raw, cstr(role).as_ptr()) }) }
    pub fn family(&self, cs: &str) -> String { s(unsafe { oc_config_family(self.raw, cstr(cs).as_ptr()) }) }
    pub fn encoding(&self, cs: &str) -> String { s(unsafe { oc_config_encoding(self.raw, cstr(cs).as_ptr()) }) }
}

/// What a chain is: the user's choices (the config is the `Config` it is built against). `scene_lut` = a LUT / CDL file's
/// path (`.cube .3dl .csp .cc .ccc .cdl`, "" = none), applied to the values as they are after the Look — no conversion
/// either side (QCView's code; its "scene-linear" wording was wrong, spec step6-luts B1); `ccc_id` = the correction inside a
/// `.ccc` / `.cdl` (its id or an index; "" = the first).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Spec { pub input: String, pub look: String, pub view: String, pub scene_lut: String, pub ccc_id: String }

/// One built chain: OCIO's processor, its generated shader and LUTs, the CPU processor (the ground truth).
pub struct Chain { raw: *mut RawChain, _config: Arc<Config> }
unsafe impl Send for Chain {}
impl Drop for Chain { fn drop(&mut self) { unsafe { oc_chain_free(self.raw) } } }

/// One OCIO LUT as OCIO declared it (1D / 2D / 3D; RGB or red only).
pub struct Lut { pub name: String, pub w: u32, pub h: u32, pub rgb: bool, pub dim: u32, pub values: Vec<f32> }

impl Chain {
    pub fn build(config: &Arc<Config>, spec: &Spec) -> Result<Chain, String> {
        let mut err = vec![0u8; 4096];
        let lang = if cfg!(windows) { 1 } else { 0 };
        let raw = unsafe { oc_chain_build(config.raw, cstr(&spec.input).as_ptr(), cstr(&spec.look).as_ptr(), cstr(&spec.view).as_ptr(), cstr(&spec.scene_lut).as_ptr(), cstr(&spec.ccc_id).as_ptr(), lang, c"OCIODisplay".as_ptr(), err.as_mut_ptr() as *mut c_char, err.len()) };
        if raw.is_null() { return Err(CStr::from_bytes_until_nul(&err).map(|c| c.to_string_lossy().into_owned()).unwrap_or_default()) }
        Ok(Chain { raw, _config: config.clone() })
    }
    /// QCView's split around the Knee: (pre → the interchange role, post → the view, the side).
    pub fn build_split(config: &Arc<Config>, spec: &Spec) -> Result<(Chain, Chain, crate::Side), String> {
        let mut err = vec![0u8; 4096];
        let (mut pre, mut post) = (std::ptr::null_mut(), std::ptr::null_mut());
        let lang = if cfg!(windows) { 1 } else { 0 };
        let side = unsafe { oc_chain_build_split(config.raw, cstr(&spec.input).as_ptr(), cstr(&spec.look).as_ptr(), cstr(&spec.view).as_ptr(), cstr(&spec.scene_lut).as_ptr(), cstr(&spec.ccc_id).as_ptr(), lang, &mut pre, &mut post, err.as_mut_ptr() as *mut c_char, err.len()) };
        if side == 0 { return Err(CStr::from_bytes_until_nul(&err).map(|c| c.to_string_lossy().into_owned()).unwrap_or_default()) }
        Ok((Chain { raw: pre, _config: config.clone() }, Chain { raw: post, _config: config.clone() }, if side == 1 { crate::Side::Scene } else { crate::Side::Display }))
    }
    /// QCView's OCIOScope: the input → the interchange role, and the side (for the matrix to Rec.2020).
    pub fn build_scope(config: &Arc<Config>, input: &str) -> Result<(Chain, crate::Side), String> {
        let mut err = vec![0u8; 4096]; let mut side = 0;
        let raw = unsafe { oc_chain_build_scope(config.raw, cstr(input).as_ptr(), if cfg!(windows) { 1 } else { 0 }, &mut side, err.as_mut_ptr() as *mut c_char, err.len()) };
        if raw.is_null() { return Err(CStr::from_bytes_until_nul(&err).map(|c| c.to_string_lossy().into_owned()).unwrap_or_default()) }
        Ok((Chain { raw, _config: config.clone() }, if side == 1 { crate::Side::Scene } else { crate::Side::Display }))
    }
    pub fn shader(&self) -> String { s(unsafe { oc_chain_shader(self.raw) }) }
    pub fn num_uniforms(&self) -> u32 { unsafe { oc_chain_num_uniforms(self.raw) } }
    /// The LUT inventory: 1D / 2D first (registration order), then 3D — the order QCView declares the kernel arguments in.
    pub fn luts(&self) -> Vec<Lut> {
        let mut out = vec![];
        unsafe {
            for i in 0..oc_chain_num_textures(self.raw) {
                let (mut name, mut w, mut h, mut rgb, mut dim, mut vals) = (std::ptr::null(), 0, 0, 0, 0, std::ptr::null());
                if oc_chain_texture(self.raw, i, &mut name, &mut w, &mut h, &mut rgb, &mut dim, &mut vals) == 0 { continue }
                let h = if dim == 1 { 1 } else { h.max(1) };
                let n = (w.max(1) * h) as usize * if rgb == 1 { 3 } else { 1 };
                out.push(Lut { name: s(name), w: w.max(1), h, rgb: rgb == 1, dim: dim as u32, values: std::slice::from_raw_parts(vals, n).to_vec() });
            }
            for i in 0..oc_chain_num_3d(self.raw) {
                let (mut name, mut e, mut vals) = (std::ptr::null(), 0, std::ptr::null());
                if oc_chain_texture3d(self.raw, i, &mut name, &mut e, &mut vals) == 0 { continue }
                out.push(Lut { name: s(name), w: e, h: e, rgb: true, dim: 3, values: std::slice::from_raw_parts(vals, (e * e * e) as usize * 3).to_vec() });
            }
        }
        out
    }
    /// OCIO's CPU processor over packed RGBA f32, in place.
    pub fn apply_cpu(&self, px: &mut [f32]) { let n = px.len() / 4; unsafe { oc_chain_apply_rgba(self.raw, px.as_mut_ptr(), n as c_long, 1) } }
}

fn reg_index(name: &str) -> u32 { name.rsplit('_').next().and_then(|s| s.parse().ok()).unwrap_or(0) }

fn lut_texture(device: &wgpu::Device, queue: &wgpu::Queue, l: &Lut) -> wgpu::Texture {
    let (format, bpp) = if l.rgb { (wgpu::TextureFormat::Rgba32Float, 16) } else { (wgpu::TextureFormat::R32Float, 4) };
    let data: Vec<f32> = if l.rgb { l.values.chunks(3).flat_map(|c| [c[0], c[1], c[2], 1.0]).collect() } else { l.values.clone() };
    let (dimension, depth) = match l.dim { 1 => (wgpu::TextureDimension::D1, 1), 2 => (wgpu::TextureDimension::D2, 1), _ => (wgpu::TextureDimension::D3, l.w) };
    let size = wgpu::Extent3d { width: l.w, height: l.h, depth_or_array_layers: depth };
    let tex = device.create_texture(&wgpu::TextureDescriptor { label: Some(&l.name), size, mip_level_count: 1, sample_count: 1, dimension, format, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] });
    queue.write_texture(tex.as_image_copy(), bytemuck::cast_slice(&data), wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(l.w * bpp), rows_per_image: Some(l.h) }, size);
    tex
}

/// The kernel source around OCIO's shader: QCView's compute wrapper for this backend.
fn kernel(shader: &str, luts: &[Lut], func: &str) -> String {
    if cfg!(windows) {
        // DX12: OCIO's HLSL declares its LUT textures + samplers as globals; wgpu numbers each register class in bind-group-entry
        // order in space0 (SRV t0 = src, t1.. = LUTs; UAV u0 = dst; sampler s0) — explicit registers, every sampler the shared one.
        let mut body = String::new(); let mut samplers: Vec<String> = vec![];
        for line in shader.lines() {
            let t = line.trim();
            if let Some(n) = t.strip_prefix("SamplerState") { samplers.push(n.trim().trim_end_matches(';').trim().to_string()); continue }
            let mut l = line.to_string();
            for (i, lu) in luts.iter().enumerate() { let decl = format!(" {};", lu.name); if t.starts_with("Texture") && t.ends_with(decl.trim_start()) { l = l.replace(&decl, &format!(" {} : register(t{});", lu.name, 1 + i)); } }
            body.push_str(&l); body.push('\n');
        }
        for sm in &samplers { body = body.replace(sm.as_str(), "src_smp"); }
        format!("Texture2D<float4> src : register(t0);\nRWTexture2D<float4> dst : register(u0);\nSamplerState src_smp : register(s0);\n\n{body}\n[numthreads(16, 16, 1)]\nvoid ocio_apply(uint3 gid : SV_DispatchThreadID)\n{{\n    uint w, h; dst.GetDimensions(w, h);\n    if (gid.x >= w || gid.y >= h) return;\n    float4 color = src.Load(int3(gid.xy, 0));\n    color = {func}(color);\n    dst[gid.xy] = color;\n}}\n")
    } else {
        let mut args = String::new(); let (mut call3, mut call2) = (vec![], vec![]);
        for (i, l) in luts.iter().enumerate() {
            let ty = match l.dim { 1 => "texture1d", 2 => "texture2d", _ => "texture3d" };
            args += &format!("    {ty}<float, access::sample> {} [[texture({})]],\n", l.name, 2 + i);
            if l.dim == 3 { call3.push((reg_index(&l.name), l.name.clone())) } else { call2.push((reg_index(&l.name), l.name.clone())) }
        }
        call3.sort_by_key(|c| c.0); call2.sort_by_key(|c| c.0);
        let call: String = call3.iter().chain(call2.iter()).map(|(_, n)| format!("{n}, src_smp, ")).collect();
        format!("#include <metal_stdlib>\nusing namespace metal;\n\n{shader}\nkernel void ocio_apply(\n    texture2d<float, access::sample> src [[texture(0)]],\n    texture2d<float, access::write> dst [[texture(1)]],\n    sampler src_smp [[sampler(0)]],\n{args}    uint2 gid [[thread_position_in_grid]])\n{{\n    if (gid.x >= dst.get_width() || gid.y >= dst.get_height()) return;\n    float4 color = src.read(gid);\n    color = {func}({call}color);\n    dst.write(color, gid);\n}}\n")
    }
}

/// One OCIO processor as a native compute pass (its shader in QCView's wrapper, its LUTs as textures).
struct Native { pipeline: wgpu::ComputePipeline, layout: wgpu::BindGroupLayout, luts: Vec<wgpu::TextureView>, _textures: Vec<wgpu::Texture> }

impl Native {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, chain: &Chain, func: &str) -> Result<Native, String> {
        if chain.num_uniforms() > 0 { log::warn!("OCIO chain has {} dynamic-property uniforms — not bound yet", chain.num_uniforms()); }
        let luts = chain.luts();
        let src = kernel(&chain.shader(), &luts, func);
        let mut entries = vec![
            wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format: wgpu::TextureFormat::Rgba16Float, view_dimension: wgpu::TextureViewDimension::D2 }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
        ];
        let filterable = device.features().contains(wgpu::Features::FLOAT32_FILTERABLE);
        for (i, l) in luts.iter().enumerate() {
            let vd = match l.dim { 1 => wgpu::TextureViewDimension::D1, 2 => wgpu::TextureViewDimension::D2, _ => wgpu::TextureViewDimension::D3 };
            entries.push(wgpu::BindGroupLayoutEntry { binding: 3 + i as u32, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable }, view_dimension: vd, multisampled: false }, count: None });
        }
        let textures: Vec<wgpu::Texture> = luts.iter().map(|l| lut_texture(device, queue, l)).collect();
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let entry_points = Cow::Owned(vec![wgpu::PassthroughShaderEntryPoint { name: "ocio_apply".into(), workgroup_size: (16, 16, 1) }]);
        // SAFETY: OCIO's own generated shader inside QCView's shipping wrapper; the layout below is exactly what it declares.
        let module = unsafe { device.create_shader_module_passthrough(if cfg!(windows) {
            wgpu::ShaderModuleDescriptorPassthrough { label: Some("ocio-hlsl"), entry_points, hlsl: Some(Cow::Owned(src.clone())), ..Default::default() }
        } else {
            wgpu::ShaderModuleDescriptorPassthrough { label: Some("ocio-msl"), entry_points, msl: Some(Cow::Owned(src.clone())), ..Default::default() }
        }) };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("ocio"), entries: &entries });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("ocio"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some("ocio"), layout: Some(&pl), module: &module, entry_point: Some("ocio_apply"), compilation_options: Default::default(), cache: None });
        if let Some(e) = pollster::block_on(scope.pop()) {
            let dump = std::env::temp_dir().join(if cfg!(windows) { "qcv-ocio-kernel.hlsl" } else { "qcv-ocio-kernel.metal" });
            std::fs::write(&dump, &src).ok();
            return Err(format!("OCIO kernel failed (dumped to {}): {e}", dump.display()));
        }
        Ok(Native { pipeline, layout, luts: textures.iter().map(|t| t.create_view(&Default::default())).collect(), _textures: textures })
    }
    fn run(&self, device: &wgpu::Device, sampler: &wgpu::Sampler, enc: &mut wgpu::CommandEncoder, src: &wgpu::Texture, dst: &wgpu::Texture) {
        let sv = src.create_view(&Default::default()); let dv = dst.create_view(&Default::default());
        let mut e = vec![wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&sv) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&dv) }, wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) }];
        for (i, v) in self.luts.iter().enumerate() { e.push(wgpu::BindGroupEntry { binding: 3 + i as u32, resource: wgpu::BindingResource::TextureView(v) }); }
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("ocio"), layout: &self.layout, entries: &e });
        let size = dst.size();
        let mut p = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("ocio"), timestamp_writes: None });
        p.set_pipeline(&self.pipeline); p.set_bind_group(0, &bind, &[]);
        p.dispatch_workgroups(size.width.div_ceil(16), size.height.div_ceil(16), 1);
    }
}

enum Mode {
    One(Native),
    /// QCView's split: OCIO pre → the Knee (interchange, linear) → OCIO post; two working textures between.
    Split { pre: Native, knee: crate::KneeStage, post: Native, side: crate::Side, tex: Option<(wgpu::Texture, wgpu::Texture, (u32, u32))> },
}

/// An SDR view targets 100 nits for the Knee (QCView: `displayIsSdr`); with one linear display the view says it.
pub fn view_is_sdr(view: &str) -> bool { !view.contains("HDR") }

/// OCIO in the stage-2 slot: the source's R'G'B' in (the input colourspace says what they are), linear Rec.709 out.
pub struct OcioPass {
    device: wgpu::Device,
    sampler: wgpu::Sampler,
    mode: Mode,
    /// the waveform's tap (QCView's OCIOScope): the native pass, its matrix to Rec.2020, on, its texture
    scope: Option<(Native, [f32; 9])>, scope_on: bool, scope_tex: Option<wgpu::Texture>,
    pub spec: Spec,
    pub knee: crate::Knee,
    pub build_ms: f64,
}

impl OcioPass {
    /// Build the chain (split around the Knee when it is on), its LUT textures and the native pipelines. Run it on a worker:
    /// OCIO + the backend compile take tens to hundreds of ms. The device must have `PASSTHROUGH_SHADERS`.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, config: &Arc<Config>, spec: Spec, knee: crate::Knee) -> Result<OcioPass, String> {
        let t0 = std::time::Instant::now();
        if !device.features().contains(wgpu::Features::PASSTHROUGH_SHADERS) { return Err("the device has no passthrough shaders".into()) }
        let mode = if knee.enabled {
            match Chain::build_split(config, &spec) {
                Ok((pre, post, side)) => {
                    let k = crate::KneeStage::new(device, queue);
                    k.set(&knee, side, view_is_sdr(&spec.view), 1.0);
                    Mode::Split { pre: Native::new(device, queue, &pre, "OCIOPre")?, knee: k, post: Native::new(device, queue, &post, "OCIOPost")?, side, tex: None }
                }
                Err(e) => { log::warn!("OCIO: no Knee for this chain ({e}); running it whole"); Mode::One(Native::new(device, queue, &Chain::build(config, &spec)?, "OCIODisplay")?) }
            }
        } else { Mode::One(Native::new(device, queue, &Chain::build(config, &spec)?, "OCIODisplay")?) };
        let scope = match Chain::build_scope(config, &spec.input) {
            Ok((c, side)) => Native::new(device, queue, &c, "OCIOScope").ok().map(|n| (n, crate::side_matrices(side).0)),
            Err(e) => { log::info!("OCIO: no scope tap for '{}' ({e})", spec.input); None }
        };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { label: Some("ocio-linear-clamp"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let build_ms = t0.elapsed().as_secs_f64() * 1000.0;
        log::info!("OCIO {}: '{}' → '{}'{}{} built in {build_ms:.0} ms", version(), spec.input, spec.view, if spec.look.is_empty() { String::new() } else { format!(" + look '{}'", spec.look) }, if matches!(mode, Mode::Split { .. }) { " + Knee (split)" } else { "" });
        Ok(OcioPass { device: device.clone(), sampler, mode, scope, scope_on: false, scope_tex: None, spec, knee, build_ms })
    }
    /// New Knee numbers on a split chain: a buffer write, no rebuild. Turning the Knee on or off needs a new pass (`new`).
    pub fn set_knee(&mut self, k: crate::Knee) -> bool {
        match &self.mode { Mode::Split { knee, side, .. } if k.enabled => { knee.set(&k, *side, view_is_sdr(&self.spec.view), 1.0); self.knee = k; true } _ => false }
    }
    pub fn is_split(&self) -> bool { matches!(self.mode, Mode::Split { .. }) }
}

impl ColorPass for OcioPass {
    fn input(&self) -> Space { Space::Encoded(Transfer::Bt1886) }
    fn output(&self) -> Space { Space::Linear }
    fn set_scope_tap(&mut self, on: bool) { self.scope_on = on; }
    fn scope_tap(&self) -> Option<(wgpu::Texture, [f32; 9])> { if !self.scope_on { return None } Some((self.scope_tex.clone()?, self.scope.as_ref()?.1)) }
    fn run(&mut self, enc: &mut wgpu::CommandEncoder, src: &wgpu::Texture, dst: &wgpu::Texture) {
        if self.scope_on && let Some((n, _)) = self.scope.as_ref() {
            if self.scope_tex.as_ref().map(|t| (t.width(), t.height())) != Some((src.width(), src.height())) { self.scope_tex = Some(crate::scope_texture(&self.device, src.width(), src.height())); }
            n.run(&self.device, &self.sampler, enc, src, self.scope_tex.as_ref().unwrap());
        }
        match &mut self.mode {
            Mode::One(n) => n.run(&self.device, &self.sampler, enc, src, dst),
            Mode::Split { pre, knee, post, tex, .. } => {
                let size = (dst.width(), dst.height());
                if tex.as_ref().map(|t| t.2) != Some(size) {
                    let mk = |l: &str| self.device.create_texture(&wgpu::TextureDescriptor { label: Some(l), size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba16Float, usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] });
                    *tex = Some((mk("ocio-interchange"), mk("ocio-kneed"), size));
                }
                let (a, b, _) = tex.as_ref().unwrap();
                pre.run(&self.device, &self.sampler, enc, src, a);
                knee.run(enc, a, b);
                post.run(&self.device, &self.sampler, enc, b, dst);
            }
        }
    }
}
