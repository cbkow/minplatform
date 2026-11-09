//! S7 — the Windows stream's surface (docs/spikes/S7-winstream.md): can a picture rendered by wgpu's DX12 device reach the
//! hardware H.264 encoder with no CPU copy?
//!
//!   s7-winstream probe                       the hardware H.264 MFTs and whether one takes a D3D12 device manager
//!   s7-winstream d3d12 <out.h264> [secs] [WxH] [fps] [kbps]    route A: the NV12 D3D12 resource straight to the MFT
//!   s7-winstream d3d11 <out.h264> …          route B: the same resource SHARED into a D3D11 device (QCView's own device kind)
//!
//! Both routes: wgpu renders the picture and runs the stream's real NV12 pass (`crates/qcv-stream/src/nv12.rs`) into an R8
//! and an RG8 texture; a raw D3D12 list on wgpu's own queue copies them into the two planes of an NV12 resource (GPU to
//! GPU); Media Foundation wraps that resource as the encoder's input sample. It prints `expect x y Y Cb Cr` for the flat
//! patches (QCView's BT.709 limited maths) and the encoder's per-frame latency.
#[cfg(not(windows))]
fn main() { eprintln!("s7-winstream: Windows only"); }

#[cfg(windows)]
#[path = "../../../crates/qcv-stream/src/nv12.rs"]
mod nv12;

#[cfg(windows)]
fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let r = match a.first().map(String::as_str) {
        Some("probe") | None => win::probe(),
        Some(route @ ("d3d12" | "d3d11")) => {
            let out = a.get(1).cloned().unwrap_or_else(|| "out.h264".into());
            let secs: f64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(10.0);
            let (w, h) = a.get(3).and_then(|s| s.split_once('x')).and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?))).unwrap_or((1920u32, 1080u32));
            let fps: u32 = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(30);
            let kbps: u32 = a.get(5).and_then(|s| s.parse().ok()).unwrap_or(6000);
            win::run(route == "d3d12", &out, secs, w, h, fps, kbps)
        }
        Some(x) => { eprintln!("unknown mode {x}"); return }
    };
    if let Err(e) = r { eprintln!("FAILED: {e}"); std::process::exit(1); }
}

#[cfg(windows)]
mod win {
    use std::mem::ManuallyDrop;
    use std::time::{Duration, Instant};
    use windows::core::{Interface, Result, GUID, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, GENERIC_ALL, HANDLE};
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
    use windows::Win32::Graphics::Direct3D11::*;
    use windows::Win32::Graphics::Direct3D12::*;
    use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter, IDXGIFactory4};
    use windows::Win32::Media::MediaFoundation::*;
    use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_MULTITHREADED};
    use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject, INFINITE};
    use windows::Win32::System::Variant::VARIANT;

    fn wgpu_dx12() -> (wgpu::Device, wgpu::Queue, String) {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends: wgpu::Backends::DX12, ..wgpu::InstanceDescriptor::new_without_display_handle() });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions { power_preference: wgpu::PowerPreference::HighPerformance, ..Default::default() })).expect("a DX12 adapter");
        let name = adapter.get_info().name;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("a device");
        (device, queue, name)
    }
    unsafe fn hardware_h264() -> Result<Vec<(IMFActivate, String)>> {
        unsafe {
            let out = MFT_REGISTER_TYPE_INFO { guidMajorType: MFMediaType_Video, guidSubtype: MFVideoFormat_H264 };
            let (mut list, mut n) = (std::ptr::null_mut::<Option<IMFActivate>>(), 0u32);
            MFTEnumEx(MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER, None, Some(&out), &mut list, &mut n)?;
            let acts: Vec<IMFActivate> = (0..n as usize).filter_map(|i| (*list.add(i)).take()).collect();
            CoTaskMemFree(Some(list as *const _));
            Ok(acts.into_iter().map(|act| {
                let (mut name, mut len) = (PWSTR::null(), 0u32);
                let _ = act.GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &mut name, &mut len);
                let friendly = if name.is_null() { "?".to_string() } else { name.to_string().unwrap_or_default() };
                (act, friendly)
            }).collect())
        }
    }

    pub fn probe() -> Result<()> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            MFStartup(MF_VERSION, MFSTARTUP_FULL)?;
            let (device, _queue, name) = wgpu_dx12();
            println!("adapter: {name}");
            let d3d12 = { let hal = device.as_hal::<wgpu::hal::api::Dx12>().expect("the DX12 hal"); hal.raw_device().clone() };
            let acts = hardware_h264()?;
            println!("hardware H.264 encoder MFTs: {}", acts.len());
            let u32_of = |a: &IMFAttributes, k: &GUID| a.GetUINT32(k).map(|v| v.to_string()).unwrap_or_else(|_| "—".into());
            for (i, (act, friendly)) in acts.iter().enumerate() {
                println!("[{i}] {friendly}");
                let mft: IMFTransform = match act.ActivateObject() { Ok(m) => m, Err(e) => { println!("    activate failed: {e}"); continue } };
                let Ok(attrs) = mft.GetAttributes() else { println!("    no attributes"); continue };
                println!("    async {}  D3D-aware {}  D3D11-aware {}", u32_of(&attrs, &MF_TRANSFORM_ASYNC), u32_of(&attrs, &MF_SA_D3D_AWARE), u32_of(&attrs, &MF_SA_D3D11_AWARE));
                let _ = attrs.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1);
                let (mut token, mut mgr) = (0u32, None::<IMFDXGIDeviceManager>);
                MFCreateDXGIDeviceManager(&mut token, &mut mgr)?;
                let mgr = mgr.unwrap();
                match mgr.ResetDevice(&d3d12, token) { Ok(()) => println!("    device manager took the D3D12 device"), Err(e) => { println!("    device manager REFUSED the D3D12 device: {e}"); continue } }
                match mft.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, mgr.as_raw() as usize) { Ok(()) => println!("    the MFT ACCEPTED the D3D12 device manager"), Err(e) => println!("    the MFT refused the D3D12 device manager: {e}") }
            }
            MFShutdown()?;
        }
        Ok(())
    }

    fn pack(hi: u32, lo: u32) -> u64 { ((hi as u64) << 32) | lo as u64 }
    unsafe fn barrier(res: &ID3D12Resource, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) -> D3D12_RESOURCE_BARRIER {
        D3D12_RESOURCE_BARRIER { Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION, Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE, Anonymous: D3D12_RESOURCE_BARRIER_0 { Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER { pResource: unsafe { std::mem::transmute_copy(res) }, StateBefore: before, StateAfter: after, Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES }) } }
    }
    unsafe fn location(res: &ID3D12Resource, sub: u32) -> D3D12_TEXTURE_COPY_LOCATION {
        D3D12_TEXTURE_COPY_LOCATION { pResource: unsafe { std::mem::transmute_copy(res) }, Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX, Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: sub } }
    }

    pub fn run(route_d3d12: bool, out_path: &str, secs: f64, w: u32, h: u32, fps: u32, kbps: u32) -> Result<()> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            MFStartup(MF_VERSION, MFSTARTUP_FULL)?;
            let (device, queue, adapter_name) = wgpu_dx12();
            println!("route {}: {w}x{h} {fps} fps {kbps} kbps for {secs} s on {adapter_name}", if route_d3d12 { "A (D3D12 resource to the MFT)" } else { "B (shared into D3D11)" });
            let (d3d12, raw_queue) = { let hal = device.as_hal::<wgpu::hal::api::Dx12>().expect("the DX12 hal"); (hal.raw_device().clone(), hal.raw_queue().clone()) };

            // ---- the picture (the macOS harness's: 8 bars over 8 grey steps, encoded sRGB) and the NV12 pass's two targets
            let bars: [[u8; 3]; 8] = [[191, 191, 191], [191, 191, 0], [0, 191, 191], [0, 191, 0], [191, 0, 191], [191, 0, 0], [0, 0, 191], [255, 255, 255]];
            let greys: [u8; 8] = [0, 32, 64, 96, 128, 160, 192, 224];
            let mut px = vec![0u8; (w * h * 4) as usize];
            for y in 0..h { for x in 0..w { let i = ((x * 8) / w) as usize; let c = if y < h * 2 / 3 { bars[i] } else { [greys[i]; 3] }; px[((y * w + x) * 4) as usize..((y * w + x) * 4 + 4) as usize].copy_from_slice(&[c[0], c[1], c[2], 255]); } }
            let tex = |label, w, h, format, usage| device.create_texture(&wgpu::TextureDescriptor { label: Some(label), size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format, usage, view_formats: &[] });
            let src = tex("picture", w, h, wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST);
            queue.write_texture(src.as_image_copy(), &px, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w * 4), rows_per_image: Some(h) }, wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 });
            let ytex = tex("Y", w, h, wgpu::TextureFormat::R8Unorm, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
            let ctex = tex("CbCr", w / 2, h / 2, wgpu::TextureFormat::Rg8Unorm, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
            // one texel copied out of each after the pass: wgpu leaves them in COPY_SOURCE, the state the raw copy needs
            let ypark = tex("Y park", 4, 4, wgpu::TextureFormat::R8Unorm, wgpu::TextureUsages::COPY_DST);
            let cpark = tex("C park", 4, 4, wgpu::TextureFormat::Rg8Unorm, wgpu::TextureUsages::COPY_DST);
            let (sv, yv, cv) = (src.create_view(&Default::default()), ytex.create_view(&Default::default()), ctex.create_view(&Default::default()));
            let pass = crate::nv12::Nv12Pass::new(&device);
            let yuv = |c: [u8; 3]| { let (r, g, b) = (c[0] as f64 / 255.0, c[1] as f64 / 255.0, c[2] as f64 / 255.0); let y = 0.2126 * r + 0.7152 * g + 0.0722 * b; ((y * 219.0 + 16.0).round() as i32, ((b - y) / 1.8556 * 224.0 + 128.0).round() as i32, ((r - y) / 1.5748 * 224.0 + 128.0).round() as i32) };
            for i in 0..8u32 { let x = w * (2 * i + 1) / 16; let (y1, cb, cr) = yuv(bars[i as usize]); println!("expect {x} {} {y1} {cb} {cr}", h / 3); let (y2, cb, cr) = yuv([greys[i as usize]; 3]); println!("expect {x} {} {y2} {cb} {cr}", h * 5 / 6); }
            let (yraw, craw) = { let (a, b) = (ytex.as_hal::<wgpu::hal::api::Dx12>().expect("hal texture"), ctex.as_hal::<wgpu::hal::api::Dx12>().expect("hal texture")); (a.raw_resource().clone(), b.raw_resource().clone()) };

            // ---- the encoder's surfaces: NV12 resources on wgpu's device (shared for route B), a small ring
            let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
            let desc = D3D12_RESOURCE_DESC { Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D, Alignment: 0, Width: w as u64, Height: h, DepthOrArraySize: 1, MipLevels: 1, Format: DXGI_FORMAT_NV12, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 }, Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN,
                Flags: if route_d3d12 { D3D12_RESOURCE_FLAG_NONE } else { D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET | D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS } };
            const RING: usize = 6;
            let mut surfaces: Vec<ID3D12Resource> = vec![];
            for _ in 0..RING { let mut r: Option<ID3D12Resource> = None; d3d12.CreateCommittedResource(&heap, if route_d3d12 { D3D12_HEAP_FLAG_NONE } else { D3D12_HEAP_FLAG_SHARED }, &desc, D3D12_RESOURCE_STATE_COMMON, None, &mut r)?; surfaces.push(r.unwrap()); }
            println!("NV12 surfaces: {RING} created on the D3D12 device");
            let alloc: ID3D12CommandAllocator = d3d12.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT)?;
            let list: ID3D12GraphicsCommandList = d3d12.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &alloc, None)?;
            list.Close()?;
            let fence: ID3D12Fence = d3d12.CreateFence(0, D3D12_FENCE_FLAG_NONE)?;
            let event = CreateEventW(None, false, false, None)?;
            let mut fence_value = 0u64;

            // ---- the device manager: the D3D12 device itself (A), or a D3D11 device on the same adapter (B)
            let (mut token, mut mgr) = (0u32, None::<IMFDXGIDeviceManager>);
            MFCreateDXGIDeviceManager(&mut token, &mut mgr)?;
            let mgr = mgr.unwrap();
            let mut d3d11_textures: Vec<ID3D11Texture2D> = vec![];
            if route_d3d12 { mgr.ResetDevice(&d3d12, token)?; } else {
                let factory: IDXGIFactory4 = CreateDXGIFactory1()?;
                let adapter: IDXGIAdapter = factory.EnumAdapterByLuid(d3d12.GetAdapterLuid())?;
                let mut d3d11: Option<ID3D11Device> = None;
                D3D11CreateDevice(&adapter, D3D_DRIVER_TYPE_UNKNOWN, Default::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut d3d11), None, None)?;
                let d3d11 = d3d11.unwrap();
                let mt: ID3D11Multithread = d3d11.cast()?; let _ = mt.SetMultithreadProtected(true);
                let d1: ID3D11Device1 = d3d11.cast()?;
                for s in &surfaces { let hnd: HANDLE = d3d12.CreateSharedHandle(s, None, GENERIC_ALL.0, None)?; let t: ID3D11Texture2D = d1.OpenSharedResource1(hnd)?; let _ = CloseHandle(hnd); d3d11_textures.push(t); }
                println!("the NV12 surfaces opened in a D3D11 device on the same adapter");
                mgr.ResetDevice(&d3d11, token)?;
            }

            // ---- the encoder (QCView mf_video_encoder.cpp 436-553)
            let acts = hardware_h264()?;
            let (act, friendly) = acts.first().expect("a hardware H.264 MFT");
            println!("encoder: {friendly}");
            let mft: IMFTransform = act.ActivateObject()?;
            let attrs = mft.GetAttributes()?;
            attrs.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1)?; let _ = attrs.SetUINT32(&MF_LOW_LATENCY, 1);
            mft.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, mgr.as_raw() as usize)?;
            let codec: ICodecAPI = mft.cast()?;
            let set = |name: &str, key: &GUID, v: VARIANT| { if let Err(e) = codec.SetValue(key, &v) { println!("    codec api {name} refused: {e}"); } };
            set("AVLowLatencyMode", &CODECAPI_AVLowLatencyMode, VARIANT::from(true));
            set("RateControlMode CBR", &CODECAPI_AVEncCommonRateControlMode, VARIANT::from(eAVEncCommonRateControlMode_CBR.0 as u32));
            set("MeanBitRate", &CODECAPI_AVEncCommonMeanBitRate, VARIANT::from(kbps * 1000));
            set("BPictureCount 0", &CODECAPI_AVEncMPVDefaultBPictureCount, VARIANT::from(0u32));
            set("GOPSize", &CODECAPI_AVEncMPVGOPSize, VARIANT::from(fps));
            set("QualityVsSpeed", &CODECAPI_AVEncCommonQualityVsSpeed, VARIANT::from(33u32));
            let colour = |t: &IMFMediaType| -> Result<()> {
                t.SetUINT64(&MF_MT_FRAME_SIZE, pack(w, h))?; t.SetUINT64(&MF_MT_FRAME_RATE, pack(fps, 1))?; t.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))?;
                t.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
                t.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32)?; t.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32)?;
                t.SetUINT32(&MF_MT_VIDEO_PRIMARIES, MFVideoPrimaries_BT709.0 as u32)?; t.SetUINT32(&MF_MT_TRANSFER_FUNCTION, MFVideoTransFunc_709.0 as u32)?;
                Ok(())
            };
            let out_type = MFCreateMediaType()?;
            out_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?; out_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?; colour(&out_type)?;
            out_type.SetUINT32(&MF_MT_AVG_BITRATE, kbps * 1000)?; out_type.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_Base.0 as u32)?;
            if let Err(e) = mft.SetOutputType(0, &out_type, 0) { println!("SetOutputType failed: {e}"); return Err(e) }
            let in_type = MFCreateMediaType()?;
            in_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?; in_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?; colour(&in_type)?;
            if route_d3d12 && std::env::var_os("S7_NO_RESOURCE_VERSION").is_none() { let _ = in_type.SetUINT32(&MF_MT_D3D_RESOURCE_VERSION, MF_D3D12_RESOURCE.0 as u32); }
            if let Err(e) = mft.SetInputType(0, &in_type, 0) {
                println!("SetInputType failed: {e}");
                // what the MFT itself offers as input
                for i in 0..8 { match mft.GetInputAvailableType(0, i) { Ok(t) => println!("    offers input {i}: subtype {:?} d3d resource version {:?}", t.GetGUID(&MF_MT_SUBTYPE).ok(), t.GetUINT32(&MF_MT_D3D_RESOURCE_VERSION).ok()), Err(_) => break } }
                return Err(e)
            }
            println!("the MFT took the output type (H.264 Baseline) and the NV12 input type");
            mft.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)?; mft.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)?;
            let events: IMFMediaEventGenerator = mft.cast()?;

            // ---- the loop: NeedInput → render, copy the planes, hand the surface over; HaveOutput → the access unit to the file
            let total = (secs * fps as f64).round() as u64;
            let period = Duration::from_secs_f64(1.0 / fps as f64);
            let dur100 = 10_000_000i64 / fps as i64;
            let (mut sent, mut got, mut keys, mut bytes, mut draining) = (0u64, 0u64, 0u64, 0u64, false);
            let mut out_file = std::fs::File::create(out_path).expect("the output file");
            let mut sent_at: std::collections::HashMap<i64, Instant> = Default::default();
            let (mut lat, mut gpu_ms): (Vec<f64>, Vec<f64>) = (vec![], vec![]);
            let t0 = Instant::now();
            let block = vec![255u8; 64 * 64 * 4]; let black = vec![0u8; 64 * 64 * 4];
            let busy = std::env::var_os("S7_BUSY").is_some();
            let mut seed = 0x2545F4914F6CDD1Du64;
            let (nw, nh) = (w / 2, h / 4);
            let mut noise = vec![0u8; (nw * nh * 4) as usize];
            let block_at = |k: u64| ((k * 8) % (w as u64 - 64)) as u32;
            let region = |x: u32| wgpu::TexelCopyTextureInfo { texture: &src, mip_level: 0, origin: wgpu::Origin3d { x, y: 0, z: 0 }, aspect: wgpu::TextureAspect::All };
            let small = wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(64 * 4), rows_per_image: Some(64) };
            let e64 = wgpu::Extent3d { width: 64, height: 64, depth_or_array_layers: 1 };
            loop {
                let ev = events.GetEvent(MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS(0))?;
                let ty = ev.GetType()?;
                if ty == METransformNeedInput.0 as u32 {
                    if sent >= total { if !draining { draining = true; mft.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0)?; mft.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)?; } continue }
                    let k = sent;
                    // pace to real time, as the stream's clock does
                    if let Some(d) = (t0 + period * k as u32).checked_duration_since(Instant::now()) { std::thread::sleep(d); }
                    let g0 = Instant::now();
                    if k > 0 { queue.write_texture(region(block_at(k - 1)), &black, small, e64); }
                    queue.write_texture(region(block_at(k)), &block, small, e64);
                    if busy { for c in noise.chunks_mut(4) { seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17; c.copy_from_slice(&[(seed >> 8) as u8, (seed >> 24) as u8, (seed >> 40) as u8, 255]); }
                        queue.write_texture(wgpu::TexelCopyTextureInfo { texture: &src, mip_level: 0, origin: wgpu::Origin3d { x: w / 4, y: 64, z: 0 }, aspect: wgpu::TextureAspect::All }, &noise, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(nw * 4), rows_per_image: Some(nh) }, wgpu::Extent3d { width: nw, height: nh, depth_or_array_layers: 1 }); }
                    let mut enc = device.create_command_encoder(&Default::default());
                    pass.encode(&device, &mut enc, &sv, &yv, &cv);
                    let one = wgpu::Extent3d { width: 4, height: 4, depth_or_array_layers: 1 };
                    enc.copy_texture_to_texture(ytex.as_image_copy(), ypark.as_image_copy(), one); enc.copy_texture_to_texture(ctex.as_image_copy(), cpark.as_image_copy(), one);
                    queue.submit([enc.finish()]);
                    // the planes into the NV12 surface, on wgpu's own queue, right behind its work
                    let i = (k as usize) % RING; let nv12 = &surfaces[i];
                    alloc.Reset()?; list.Reset(&alloc, None)?;
                    list.ResourceBarrier(&[barrier(nv12, D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_COPY_DEST)]);
                    list.CopyTextureRegion(&location(nv12, 0), 0, 0, 0, &location(&yraw, 0), None);
                    list.CopyTextureRegion(&location(nv12, 1), 0, 0, 0, &location(&craw, 0), None);
                    list.ResourceBarrier(&[barrier(nv12, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COMMON)]);
                    list.Close()?;
                    raw_queue.ExecuteCommandLists(&[Some(list.cast()?)]);
                    fence_value += 1; raw_queue.Signal(&fence, fence_value)?;
                    fence.SetEventOnCompletion(fence_value, event)?; WaitForSingleObject(event, INFINITE);   // the GPU is done: the surface is the encoder's
                    gpu_ms.push(g0.elapsed().as_secs_f64() * 1000.0);
                    let buffer = if route_d3d12 { MFCreateDXGISurfaceBuffer(&ID3D12Resource::IID, nv12, 0, false)? } else { MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, &d3d11_textures[i], 0, false)? };
                    if let Ok(b2) = buffer.cast::<IMF2DBuffer>() { if let Ok(n) = b2.GetContiguousLength() { let _ = buffer.SetCurrentLength(n); } }
                    let sample = MFCreateSample()?;
                    sample.AddBuffer(&buffer)?;
                    let time = k as i64 * dur100;
                    sample.SetSampleTime(time)?; sample.SetSampleDuration(dur100)?;
                    if k == 0 { set("ForceKeyFrame", &CODECAPI_AVEncVideoForceKeyFrame, VARIANT::from(1u32)); }
                    sent_at.insert(time, Instant::now());
                    mft.ProcessInput(0, &sample, 0)?;
                    sent += 1;
                } else if ty == METransformHaveOutput.0 as u32 {
                    let mut ob = [MFT_OUTPUT_DATA_BUFFER { dwStreamID: 0, pSample: ManuallyDrop::new(None), dwStatus: 0, pEvents: ManuallyDrop::new(None) }];
                    let mut status = 0u32;
                    match mft.ProcessOutput(0, &mut ob, &mut status) {
                        Ok(()) => {
                            let sample = ManuallyDrop::take(&mut ob[0].pSample); let _ = ManuallyDrop::take(&mut ob[0].pEvents);
                            let Some(sample) = sample else { continue };
                            if let Ok(t) = sample.GetSampleTime() { if let Some(at) = sent_at.remove(&t) { lat.push(at.elapsed().as_secs_f64() * 1000.0); } }
                            if sample.GetUINT32(&MFSampleExtension_CleanPoint).unwrap_or(0) != 0 { keys += 1; }
                            let buf = sample.ConvertToContiguousBuffer()?;
                            let (mut p, mut len) = (std::ptr::null_mut::<u8>(), 0u32);
                            buf.Lock(&mut p, None, Some(&mut len))?;
                            use std::io::Write;
                            out_file.write_all(std::slice::from_raw_parts(p, len as usize)).expect("write");
                            buf.Unlock()?;
                            bytes += len as u64; got += 1;
                        }
                        Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => { let t = mft.GetOutputAvailableType(0, 0)?; mft.SetOutputType(0, &t, 0)?; }
                        Err(e) => return Err(e),
                    }
                } else if ty == METransformDrainComplete.0 as u32 { break }
            }
            let dt = t0.elapsed().as_secs_f64();
            let stat = |v: &mut Vec<f64>| { v.sort_by(|a, b| a.partial_cmp(b).unwrap()); if v.is_empty() { (0.0, 0.0, 0.0) } else { (v[v.len() / 2], v[(v.len() * 95 / 100).min(v.len() - 1)], v[v.len() - 1]) } };
            let (l50, l95, lmax) = stat(&mut lat); let (g50, g95, gmax) = stat(&mut gpu_ms);
            println!("done: {sent} sent, {got} encoded, {keys} keyframes, {:.0} kbps over {dt:.1} s", bytes as f64 * 8.0 / 1000.0 / dt);
            println!("encoder latency (input to output) ms: p50 {l50:.1} p95 {l95:.1} max {lmax:.1}; render + plane copy + GPU wait ms: p50 {g50:.2} p95 {g95:.2} max {gmax:.2}");
            let _ = CloseHandle(event);
            let _ = mft.ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
            MFShutdown()?;
        }
        Ok(())
    }
}
