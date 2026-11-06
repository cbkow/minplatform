//! S1 step 2 — OCIO on wgpu. Route A (`native`): OCIO's generated MSL 2.0 through wgpu's PASSTHROUGH shader module, wrapped in
//! QCView's compute kernel (src / dst / one shared linear+clamp sampler / the LUTs), LUTs as ordinary wgpu textures — the
//! Metal slot order falls out of wgpu's bind-group-entry order (per resource class, sequential). Route B (`lut`): the CPU
//! processor baked on an edge³ lattice → ONE WGSL compute stage. Ground truth = the CPU processor on the same encoded pixels.
//! GPU time of the colour pass via timestamp queries. Dies with the spike; canon §18.
use std::{borrow::Cow, ffi::{c_char, c_int, c_long, c_uint, CStr, CString}};
use crate::rb::{read_buffer, read_rgba16f, f2h};

#[repr(C)] pub struct Raw { _p: [u8; 0] }
unsafe extern "C" {
    fn ocio_version() -> *const c_char;
    fn ocio_chain_open(config: *const c_char, input_cs: *const c_char, display: *const c_char, view: *const c_char, lang: c_int, fn_name: *const c_char, err: *mut c_char, err_len: usize) -> *mut Raw;
    fn ocio_chain_close(c: *mut Raw);
    fn ocio_chain_shader(c: *const Raw) -> *const c_char;
    fn ocio_chain_num_uniforms(c: *const Raw) -> c_uint;
    fn ocio_chain_num_textures(c: *const Raw) -> c_uint;
    fn ocio_chain_texture(c: *const Raw, i: c_uint, name: *mut *const c_char, w: *mut c_uint, h: *mut c_uint, rgb: *mut c_int, dim: *mut c_int, values: *mut *const f32) -> c_int;
    fn ocio_chain_num_3d(c: *const Raw) -> c_uint;
    fn ocio_chain_texture3d(c: *const Raw, i: c_uint, name: *mut *const c_char, edge: *mut c_uint, values: *mut *const f32) -> c_int;
    fn ocio_chain_apply_rgba(c: *const Raw, px: *mut f32, w: c_long, h: c_long);
    fn ocio_chain_bake3d(c: *const Raw, edge: c_int, out: *mut f32);
}

pub fn version() -> String { unsafe { CStr::from_ptr(ocio_version()) }.to_string_lossy().into_owned() }

#[derive(Clone, Debug)]
pub struct Opts { pub config: String, pub cs: String, pub display: String, pub view: String, pub route: Route, pub eotf: u32, pub verify: bool, pub edge: u32, pub gpu_ts: bool }
#[derive(Clone, Copy, Debug, PartialEq)] pub enum Route { Native, Lut }

pub struct Chain { raw: *mut Raw }
impl Drop for Chain { fn drop(&mut self) { unsafe { ocio_chain_close(self.raw) } } }
pub struct LutSrc { pub name: String, pub w: u32, pub h: u32, pub rgb: bool, pub dim: u32, pub values: Vec<f32> }   // dim 1 | 2 | 3

impl Chain {
    pub fn open(o: &Opts, hlsl: bool) -> anyhow::Result<Chain> {
        let c = |s: &str| CString::new(s).unwrap();
        let mut err = vec![0u8; 4096];
        let raw = unsafe { ocio_chain_open(c(&o.config).as_ptr(), c(&o.cs).as_ptr(), c(&o.display).as_ptr(), c(&o.view).as_ptr(), hlsl as c_int, c("OCIODisplay").as_ptr(), err.as_mut_ptr() as *mut c_char, err.len()) };
        if raw.is_null() { anyhow::bail!("OCIO: {}", CStr::from_bytes_until_nul(&err).unwrap().to_string_lossy()); }
        Ok(Chain { raw })
    }
    pub fn shader(&self) -> String { unsafe { CStr::from_ptr(ocio_chain_shader(self.raw)) }.to_string_lossy().into_owned() }
    pub fn num_uniforms(&self) -> u32 { unsafe { ocio_chain_num_uniforms(self.raw) } }
    /// OCIO's LUT inventory: 1D/2D first (registration order), then 3D — the order QCView declares the kernel arguments in.
    pub fn luts(&self) -> Vec<LutSrc> {
        let mut out = vec![];
        unsafe {
            for i in 0..ocio_chain_num_textures(self.raw) {
                let (mut name, mut w, mut h, mut rgb, mut dim, mut vals) = (std::ptr::null(), 0, 0, 0, 0, std::ptr::null());
                if ocio_chain_texture(self.raw, i, &mut name, &mut w, &mut h, &mut rgb, &mut dim, &mut vals) == 0 { continue }
                let h = if dim == 1 { 1 } else { h.max(1) };
                let n = (w.max(1) * h) as usize * if rgb == 1 { 3 } else { 1 };
                out.push(LutSrc { name: CStr::from_ptr(name).to_string_lossy().into_owned(), w: w.max(1), h, rgb: rgb == 1, dim: dim as u32, values: std::slice::from_raw_parts(vals, n).to_vec() });
            }
            for i in 0..ocio_chain_num_3d(self.raw) {
                let (mut name, mut e, mut vals) = (std::ptr::null(), 0, std::ptr::null());
                if ocio_chain_texture3d(self.raw, i, &mut name, &mut e, &mut vals) == 0 { continue }
                out.push(LutSrc { name: CStr::from_ptr(name).to_string_lossy().into_owned(), w: e, h: e, rgb: true, dim: 3, values: std::slice::from_raw_parts(vals, (e * e * e) as usize * 3).to_vec() });
            }
        }
        out
    }
    pub fn apply_rgba(&self, px: &mut [f32], w: u32, h: u32) { assert_eq!(px.len(), (w * h * 4) as usize); unsafe { ocio_chain_apply_rgba(self.raw, px.as_mut_ptr(), w as c_long, h as c_long) } }
    pub fn bake3d(&self, edge: u32) -> Vec<f32> { let mut v = vec![0f32; (edge * edge * edge * 4) as usize]; unsafe { ocio_chain_bake3d(self.raw, edge as c_int, v.as_mut_ptr()) }; v }
}

fn reg_index(name: &str) -> u32 { name.rsplit('_').next().and_then(|s| s.parse().ok()).unwrap_or(0) }

/// Upload one OCIO LUT as a wgpu texture (RGB packed to RGBA32F; red-only stays R32F), 1D / 2D / 3D as OCIO declared it.
fn lut_texture(device: &wgpu::Device, queue: &wgpu::Queue, l: &LutSrc) -> wgpu::Texture {
    let (format, bpp) = if l.rgb { (wgpu::TextureFormat::Rgba32Float, 16) } else { (wgpu::TextureFormat::R32Float, 4) };
    let data: Vec<f32> = if l.rgb { l.values.chunks(3).flat_map(|c| [c[0], c[1], c[2], 1.0]).collect() } else { l.values.clone() };
    let (dimension, depth) = match l.dim { 1 => (wgpu::TextureDimension::D1, 1), 2 => (wgpu::TextureDimension::D2, 1), _ => (wgpu::TextureDimension::D3, l.w) };
    let size = wgpu::Extent3d { width: l.w, height: l.h, depth_or_array_layers: depth };
    let tex = device.create_texture(&wgpu::TextureDescriptor { label: Some(&l.name), size, mip_level_count: 1, sample_count: 1, dimension, format, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] });
    queue.write_texture(wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, bytemuck::cast_slice(&data),
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(l.w * bpp), rows_per_image: Some(l.h) }, size);
    tex
}

enum Pass { Native { pipeline: wgpu::ComputePipeline, bind: wgpu::BindGroup, _luts: Vec<wgpu::Texture> }, Lut { pipeline: wgpu::ComputePipeline, bind: wgpu::BindGroup, _lut: wgpu::Texture, _ubuf: wgpu::Buffer } }

pub struct Stage {
    pub chain: Chain, pub opts: Opts, pub size: (u32, u32),
    pub inter: wgpu::Texture, pub inter_view: wgpu::TextureView, pub out: wgpu::Texture,
    pass: Pass,
    blit_layout: wgpu::BindGroupLayout, blit_shader: wgpu::ShaderModule, pub blit_pipeline: wgpu::RenderPipeline, pub blit_bind: wgpu::BindGroup, blit_ubuf: wgpu::Buffer,
    qs: wgpu::QuerySet, qbuf: wgpu::Buffer, pub runs: u32, period: f32, pub encode_ms: Vec<f32>,
}

const QSLOTS: u32 = 1024;   // timestamp pairs kept (each resolved into its own 256-byte slot)

impl Stage {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, opts: Opts, size: (u32, u32), surface_format: wgpu::TextureFormat, window: (u32, u32), f32_filterable: bool) -> anyhow::Result<Stage> {
        let t0 = std::time::Instant::now();
        let chain = Chain::open(&opts, cfg!(windows))?;   // MSL 2.0 on Metal, HLSL SM 5.0 on DX12
        let shader = chain.shader();
        let luts = chain.luts();
        println!("OCIO {}: chain '{}' → '{}' / '{}' built in {:.0} ms: shader {} lines, {} uniforms, LUTs: {}", version(), opts.cs, opts.display, opts.view, t0.elapsed().as_secs_f32() * 1000.0, shader.lines().count(), chain.num_uniforms(),
            luts.iter().map(|l| format!("{} {}D {}x{}{} {}", l.name, l.dim, l.w, l.h, if l.dim == 3 { format!("x{}", l.w) } else { String::new() }, if l.rgb { "rgb" } else { "r" })).collect::<Vec<_>>().join(", "));
        if chain.num_uniforms() > 0 { println!("NOTE: the chain has dynamic-property uniforms — not bound by this spike (QCView binds none either)"); }
        let tex2d = |label: &str, usage| device.create_texture(&wgpu::TextureDescriptor { label: Some(label), size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba16Float, usage, view_formats: &[] });
        let inter = tex2d("ocio-in (encoded RGB)", wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC);
        let out = tex2d("ocio-out (display-encoded)", wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC);
        let inter_view = inter.create_view(&Default::default());
        let out_view = out.create_view(&Default::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { label: Some("ocio-linear-clamp"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let src_entry = wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None };
        let dst_entry = wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format: wgpu::TextureFormat::Rgba16Float, view_dimension: wgpu::TextureViewDimension::D2 }, count: None };
        let smp_entry = wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None };
        let t1 = std::time::Instant::now();
        let pass = match opts.route {
            Route::Native => {
                // QCView's wrapper: src texture(0), dst texture(1), src_smp sampler(0), LUTs texture(2..) — wgpu's Metal backend
                // numbers each resource class sequentially in bind-group-entry order, so entries 0,1,2,3.. land exactly there.
                let mut args = String::new();
                let (mut call3, mut call2) = (vec![], vec![]);
                let mut entries = vec![src_entry, dst_entry, smp_entry];
                let mut textures = vec![];
                for (i, l) in luts.iter().enumerate() {
                    let (ty, vd) = match l.dim { 1 => ("texture1d", wgpu::TextureViewDimension::D1), 2 => ("texture2d", wgpu::TextureViewDimension::D2), _ => ("texture3d", wgpu::TextureViewDimension::D3) };
                    args += &format!("    {ty}<float, access::sample> {} [[texture({})]],\n", l.name, 2 + i);
                    if l.dim == 3 { call3.push((reg_index(&l.name), l.name.clone())) } else { call2.push((reg_index(&l.name), l.name.clone())) }
                    entries.push(wgpu::BindGroupLayoutEntry { binding: 3 + i as u32, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: f32_filterable }, view_dimension: vd, multisampled: false }, count: None });
                    textures.push(lut_texture(device, queue, l));
                }
                call3.sort_by_key(|c| c.0); call2.sort_by_key(|c| c.0);
                let call: String = call3.iter().chain(call2.iter()).map(|(_, n)| format!("{n}, src_smp, ")).collect();
                #[cfg(not(windows))]
                let (kernel, dump) = (format!("#include <metal_stdlib>\nusing namespace metal;\n\n{shader}\nkernel void ocio_apply(\n    texture2d<float, access::sample> src [[texture(0)]],\n    texture2d<float, access::write> dst [[texture(1)]],\n    sampler src_smp [[sampler(0)]],\n{args}    uint2 gid [[thread_position_in_grid]])\n{{\n    if (gid.x >= dst.get_width() || gid.y >= dst.get_height()) return;\n    float4 color = src.read(gid);\n    color = OCIODisplay({call}color);\n    dst.write(color, gid);\n}}\n"), concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/spikes/s1/out/ocio-kernel.metal"));
                #[cfg(windows)]
                let (kernel, dump) = {
                    // DX12: OCIO's HLSL SM5 declares its LUT textures + samplers as globals and OCIODisplay(inPixel) reads them. wgpu's DX12
                    // backend numbers each register class in bind-group-entry order in space0 (SRV t0 = src, t1.. = LUTs; UAV u0 = dst;
                    // sampler s0), so the declarations get explicit registers and every OCIO sampler becomes the one shared src_smp.
                    let _ = (&args, &call);
                    let mut body = String::new();
                    let mut samplers: Vec<String> = vec![];
                    for line in shader.lines() {
                        let t = line.trim();
                        if t.starts_with("SamplerState") { if let Some(n) = t.strip_prefix("SamplerState").map(|s| s.trim().trim_end_matches(';').trim().to_string()) { samplers.push(n); } continue; }
                        let mut l = line.to_string();
                        for (i, lu) in luts.iter().enumerate() { let decl = format!(" {};", lu.name); if t.starts_with("Texture") && t.ends_with(decl.trim_start()) { l = l.replace(&decl, &format!(" {} : register(t{});", lu.name, 1 + i)); } }
                        body.push_str(&l); body.push('\n');
                    }
                    for s in &samplers { body = body.replace(s.as_str(), "src_smp"); }
                    (format!("Texture2D<float4> src : register(t0);\nRWTexture2D<float4> dst : register(u0);\nSamplerState src_smp : register(s0);\n\n{body}\n[numthreads(16, 16, 1)]\nvoid ocio_apply(uint3 gid : SV_DispatchThreadID)\n{{\n    uint w, h; dst.GetDimensions(w, h);\n    if (gid.x >= w || gid.y >= h) return;\n    float4 color = src.Load(int3(gid.xy, 0));\n    color = OCIODisplay(color);\n    dst[gid.xy] = color;\n}}\n"), concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/spikes/s1/out/ocio-kernel.hlsl"))
                };
                std::fs::create_dir_all(std::path::Path::new(dump).parent().unwrap()).ok();
                std::fs::write(dump, &kernel).ok();
                let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
                // SAFETY: the kernel is OCIO's own MSL / HLSL plus QCView's shipping wrapper; the bind group layout below is what it declares.
                let entry_points = Cow::Owned(vec![wgpu::PassthroughShaderEntryPoint { name: "ocio_apply".into(), workgroup_size: (16, 16, 1) }]);
                #[cfg(not(windows))]
                let module = unsafe { device.create_shader_module_passthrough(wgpu::ShaderModuleDescriptorPassthrough { label: Some("ocio-msl"), entry_points, msl: Some(Cow::Owned(kernel)), ..Default::default() }) };
                #[cfg(windows)]
                let module = unsafe { device.create_shader_module_passthrough(wgpu::ShaderModuleDescriptorPassthrough { label: Some("ocio-hlsl"), entry_points, hlsl: Some(Cow::Owned(kernel)), ..Default::default() }) };
                let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("ocio-native"), entries: &entries });
                let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
                let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some("ocio-native"), layout: Some(&pl), module: &module, entry_point: Some("ocio_apply"), compilation_options: Default::default(), cache: None });
                if let Some(e) = pollster::block_on(scope.pop()) { anyhow::bail!("passthrough shader failed (kernel dumped to {dump}): {e}"); }
                let views: Vec<_> = textures.iter().map(|t| t.create_view(&Default::default())).collect();
                let mut bes = vec![wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&inter_view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&out_view) }, wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) }];
                for (i, v) in views.iter().enumerate() { bes.push(wgpu::BindGroupEntry { binding: 3 + i as u32, resource: wgpu::BindingResource::TextureView(v) }); }
                let bind = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("ocio-native"), layout: &layout, entries: &bes });
                println!("route A (native passthrough): {} compiled + pipeline built in {:.0} ms, {} LUT textures bound", if cfg!(windows) { "HLSL" } else { "MSL" }, t1.elapsed().as_secs_f32() * 1000.0, textures.len());
                Pass::Native { pipeline, bind, _luts: textures }
            }
            Route::Lut => {
                let edge = opts.edge;
                let tb = std::time::Instant::now();
                let baked = chain.bake3d(edge);
                let bake_ms = tb.elapsed().as_secs_f32() * 1000.0;
                let half: Vec<u16> = baked.iter().map(|&f| f2h(f)).collect();
                let lut = device.create_texture(&wgpu::TextureDescriptor { label: Some("ocio-baked-3d"), size: wgpu::Extent3d { width: edge, height: edge, depth_or_array_layers: edge }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D3, format: wgpu::TextureFormat::Rgba16Float, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] });
                queue.write_texture(wgpu::TexelCopyTextureInfo { texture: &lut, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, bytemuck::cast_slice(&half), wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(edge * 8), rows_per_image: Some(edge) }, wgpu::Extent3d { width: edge, height: edge, depth_or_array_layers: edge });
                let ubuf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("lut-p"), size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
                queue.write_buffer(&ubuf, 0, bytemuck::cast_slice(&[edge as f32, 0.0, 0.0, 0.0]));
                let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("lut3d"), source: wgpu::ShaderSource::Wgsl(include_str!("lut3d.wgsl").into()) });
                let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("ocio-lut"), entries: &[src_entry, dst_entry, smp_entry,
                    wgpu::BindGroupLayoutEntry { binding: 3, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D3, multisampled: false }, count: None },
                    wgpu::BindGroupLayoutEntry { binding: 4, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None }] });
                let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
                let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some("ocio-lut"), layout: Some(&pl), module: &module, entry_point: Some("main"), compilation_options: Default::default(), cache: None });
                let lut_view = lut.create_view(&Default::default());
                let bind = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("ocio-lut"), layout: &layout, entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&inter_view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&out_view) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&sampler) }, wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&lut_view) }, wgpu::BindGroupEntry { binding: 4, resource: ubuf.as_entire_binding() }] });
                println!("route B (baked LUT {edge}³ Rgba16Float): CPU bake {bake_ms:.0} ms, stage built in {:.0} ms", t1.elapsed().as_secs_f32() * 1000.0);
                Pass::Lut { pipeline, bind, _lut: lut, _ubuf: ubuf }
            }
        };
        // the blit: post-OCIO → surface (fit + EOTF)
        let blit_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("fit_blit"), source: wgpu::ShaderSource::Wgsl(include_str!("fit_blit.wgsl").into()) });
        let blit_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("blit"), entries: &[
            wgpu::BindGroupLayoutEntry { binding: 0, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None },
            wgpu::BindGroupLayoutEntry { binding: 2, visibility: wgpu::ShaderStages::VERTEX_FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None }] });
        let blit_ubuf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("blit-u"), size: 16, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST, mapped_at_creation: false });
        let blit_pipeline = crate::make_pipeline(device, &blit_shader, &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&blit_layout)], immediate_size: 0 }), surface_format);
        let blit_bind = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("blit"), layout: &blit_layout, entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::Sampler(&sampler) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&out_view) }, wgpu::BindGroupEntry { binding: 2, resource: blit_ubuf.as_entire_binding() }] });
        let qs = device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("ocio-ts"), ty: wgpu::QueryType::Timestamp, count: QSLOTS * 2 });
        let qbuf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("ocio-ts"), size: (QSLOTS * 256) as u64, usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false });
        let period = queue.get_timestamp_period();
        let mut st = Stage { chain, opts, size, inter, inter_view, out, pass, blit_layout, blit_shader, blit_pipeline, blit_bind, blit_ubuf, qs, qbuf, runs: 0, period, encode_ms: vec![] };
        st.set_fit(queue, window);
        Ok(st)
    }

    pub fn set_fit(&mut self, queue: &wgpu::Queue, window: (u32, u32)) {
        let (vw, vh, ww, wh) = (self.size.0 as f32, self.size.1 as f32, window.0 as f32, window.1 as f32);
        let fit = if vw / vh > ww / wh { [1.0, (ww / wh) / (vw / vh)] } else { [(vw / vh) / (ww / wh), 1.0] };
        queue.write_buffer(&self.blit_ubuf, 0, bytemuck::cast_slice(&[fit[0], fit[1], self.opts.eotf as f32, 0.0]));
    }

    /// The colour pass: inter → out, timestamped.
    pub fn run(&mut self, enc: &mut wgpu::CommandEncoder) {
        let t = std::time::Instant::now();
        let slot = self.runs % QSLOTS;
        {
            let tw = self.opts.gpu_ts.then_some(wgpu::ComputePassTimestampWrites { query_set: &self.qs, beginning_of_pass_write_index: Some(slot * 2), end_of_pass_write_index: Some(slot * 2 + 1) });
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("ocio"), timestamp_writes: tw });
            let (pipeline, bind) = match &self.pass { Pass::Native { pipeline, bind, .. } => (pipeline, bind), Pass::Lut { pipeline, bind, .. } => (pipeline, bind) };
            pass.set_pipeline(pipeline); pass.set_bind_group(0, bind, &[]);
            pass.dispatch_workgroups(self.size.0.div_ceil(16), self.size.1.div_ceil(16), 1);
        }
        if self.opts.gpu_ts { enc.resolve_query_set(&self.qs, slot * 2..slot * 2 + 2, &self.qbuf, (slot * 256) as u64); }
        self.runs += 1;
        self.encode_ms.push(t.elapsed().as_secs_f32() * 1000.0);
    }

    pub fn snapshot_pipeline(&self, device: &wgpu::Device, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
        crate::make_pipeline(device, &self.blit_shader, &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: None, bind_group_layouts: &[Some(&self.blit_layout)], immediate_size: 0 }), format)
    }

    /// GPU time of the colour pass over the kept runs.
    pub fn report(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> String {
        let n = self.runs.min(QSLOTS);
        if n == 0 { return "no OCIO runs".into() }
        if !self.opts.gpu_ts { let mut e = self.encode_ms.clone(); return format!("OCIO pass ({:?}, {}x{}): {} runs, CPU encode ms p50 {:.3} max {:.3}; GPU time = the xctrace record (--gpu-ts for wgpu timestamp queries, which read 0 half the time on Metal here)", self.opts.route, self.size.0, self.size.1, self.runs, crate::pct(&mut e, 50.0), crate::pct(&mut e, 100.0)); }
        let (ms, _buf) = read_buffer(device, queue, &self.qbuf, (QSLOTS * 256) as u64);
        let mut v: Vec<f32> = (0..n).map(|i| { let o = (i * 256) as usize; let a = u64::from_le_bytes(ms[o..o + 8].try_into().unwrap()); let b = u64::from_le_bytes(ms[o + 8..o + 16].try_into().unwrap()); (b.wrapping_sub(a)) as f32 * self.period / 1e6 }).collect();
        let mut e = self.encode_ms.clone();
        format!("OCIO pass ({:?}, {}x{}): GPU ms p50 {:.3} p95 {:.3} max {:.3} over {} runs; CPU encode ms p50 {:.3} max {:.3}", self.opts.route, self.size.0, self.size.1, crate::pct(&mut v, 50.0), crate::pct(&mut v, 95.0), crate::pct(&mut v, 100.0), n, crate::pct(&mut e, 50.0), crate::pct(&mut e, 100.0))
    }

    /// Read the encoded input + the GPU output back, run the CPU processor on the input, compare (display-encoded [0,1]).
    pub fn verify(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> String {
        let (w, h) = self.size;
        let mut input = read_rgba16f(device, queue, &self.inter, w, h);
        let gpu = read_rgba16f(device, queue, &self.out, w, h);
        let t = std::time::Instant::now();
        self.chain.apply_rgba(&mut input, w, h);
        let cpu_ms = t.elapsed().as_secs_f32() * 1000.0;
        let (mut max_e, mut sum_e, mut sum_sq, mut n_8bit, mut n_10bit) = (0f32, 0f64, 0f64, 0u64, 0u64);
        let npx = (w * h) as usize;
        for i in 0..npx {
            for c in 0..3 {
                let r = input[i * 4 + c].clamp(0.0, 1.0); let g = gpu[i * 4 + c].clamp(0.0, 1.0);
                let e = (r - g).abs();
                max_e = max_e.max(e); sum_e += e as f64; sum_sq += (e as f64) * (e as f64);
                if e > 0.5 / 255.0 { n_8bit += 1 } if e > 0.5 / 1023.0 { n_10bit += 1 }
            }
        }
        let mse = sum_sq / (npx * 3) as f64;
        let psnr = if mse > 0.0 { 10.0 * (1.0 / mse).log10() } else { f64::INFINITY };
        format!("verify ({:?}) vs OCIO CPU on {}x{} ({cpu_ms:.0} ms CPU): max |err| {:.5} ({:.2}/255, {:.2}/1023), mean {:.6}, PSNR {psnr:.1} dB; samples off by > ½ LSB: 8-bit {:.3} %, 10-bit {:.3} %",
            self.opts.route, w, h, max_e, max_e * 255.0, max_e * 1023.0, sum_e / (npx * 3) as f64, 100.0 * n_8bit as f64 / (npx * 3) as f64, 100.0 * n_10bit as f64 / (npx * 3) as f64)
    }
}

