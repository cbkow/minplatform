//! Windows: the native Media Foundation H.264 session (QCView `src/output/mf_video_encoder.cpp`, its settings kept) and the
//! surfaces it encodes, by the route spike S7 proved (`docs/spikes/S7-winstream.md`, canon row 241).
//!
//! QCView rendered on D3D11, Media Foundation's own device kind: the encoder's NV12 textures were the renderer's targets.
//! The port renders on DX12, and the hardware MFT does not encode from D3D12. So: the NV12 surfaces are D3D12 resources
//! created SHARED on wgpu's device and opened in a D3D11 device on the same adapter — the device the session runs on. The
//! NV12 pass (wgpu) writes an R8 and an RG8 target; one texel of each is copied out so wgpu leaves them in `COPY_SOURCE`; a
//! raw D3D12 list on wgpu's own queue — right behind wgpu's work — copies them into the surface's two planes. No CPU copy.
//! A fence says when the GPU is done; the encoder's thread waits for it, the render side never does.
//!
//! The session: the first hardware H.264 MFT (asynchronous), low latency, CBR, no B-frames, GOP = one second, Baseline or
//! Main, BT.709 tags, 16–235. Output: Annex-B access units with 4-byte start codes, the parameter sets cached and put before
//! a keyframe that lacks them; pts carried through a FIFO (no reordering), in 90 kHz.
use crate::{EncodedUnit, EncoderSpec, Profile};
use std::collections::VecDeque;
use std::mem::ManuallyDrop;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering::*};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows::core::{Interface, GUID, PWSTR};
use windows::Win32::Foundation::{CloseHandle, GENERIC_ALL, HANDLE};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Direct3D12::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIAdapter, IDXGIFactory4};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_MULTITHREADED};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::System::Variant::VARIANT;

type Res<T> = Result<T, String>;
fn werr<T>(what: &str, r: windows::core::Result<T>) -> Res<T> { r.map_err(|e| format!("{what}: {e}")) }

/// Surfaces alive at once (QCView's pool cap): the newest waiting for the clock, the last one kept for a repeat, those the
/// encoder still holds. None free = the encoder is behind: the renderer skips the picture.
const POOL: usize = 8;

struct Slot { nv12: ID3D12Resource, input: ID3D11Texture2D, alloc: ID3D12CommandAllocator, list: ID3D12GraphicsCommandList, busy: Arc<AtomicBool> }
struct Gpu {
    queue: ID3D12CommandQueue, d3d11: ID3D11Device, fence: ID3D12Fence, fence_value: u64, slots: Vec<Slot>,
    /// the NV12 pass's targets and the one-texel parks that leave them in COPY_SOURCE
    ytex: wgpu::Texture, ctex: wgpu::Texture, ypark: wgpu::Texture, cpark: wgpu::Texture, yraw: ID3D12Resource, craw: ID3D12Resource,
}
unsafe impl Send for Gpu {}

unsafe fn barrier(res: &ID3D12Resource, before: D3D12_RESOURCE_STATES, after: D3D12_RESOURCE_STATES) -> D3D12_RESOURCE_BARRIER {
    D3D12_RESOURCE_BARRIER { Type: D3D12_RESOURCE_BARRIER_TYPE_TRANSITION, Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE, Anonymous: D3D12_RESOURCE_BARRIER_0 { Transition: ManuallyDrop::new(D3D12_RESOURCE_TRANSITION_BARRIER { pResource: unsafe { std::mem::transmute_copy(res) }, StateBefore: before, StateAfter: after, Subresource: D3D12_RESOURCE_BARRIER_ALL_SUBRESOURCES }) } }
}
unsafe fn plane(res: &ID3D12Resource, sub: u32) -> D3D12_TEXTURE_COPY_LOCATION {
    D3D12_TEXTURE_COPY_LOCATION { pResource: unsafe { std::mem::transmute_copy(res) }, Type: D3D12_TEXTURE_COPY_TYPE_SUBRESOURCE_INDEX, Anonymous: D3D12_TEXTURE_COPY_LOCATION_0 { SubresourceIndex: sub } }
}

impl Gpu {
    fn new(device: &wgpu::Device, w: u32, h: u32) -> Res<Gpu> {
        unsafe {
            let (d3d12, queue) = { let hal = device.as_hal::<wgpu::hal::api::Dx12>().ok_or("the stream needs the DX12 renderer")?; (hal.raw_device().clone(), hal.raw_queue().clone()) };
            // a D3D11 device on the adapter wgpu chose (a machine may list virtual display adapters too)
            let factory: IDXGIFactory4 = werr("CreateDXGIFactory1", CreateDXGIFactory1())?;
            let adapter: IDXGIAdapter = werr("EnumAdapterByLuid", factory.EnumAdapterByLuid(d3d12.GetAdapterLuid()))?;
            let mut d3d11: Option<ID3D11Device> = None;
            werr("D3D11CreateDevice", D3D11CreateDevice(&adapter, D3D_DRIVER_TYPE_UNKNOWN, Default::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut d3d11), None, None))?;
            let d3d11 = d3d11.ok_or("no D3D11 device")?;
            if let Ok(mt) = d3d11.cast::<ID3D11Multithread>() { let _ = mt.SetMultithreadProtected(true); }
            let d1: ID3D11Device1 = werr("ID3D11Device1", d3d11.cast())?;
            let heap = D3D12_HEAP_PROPERTIES { Type: D3D12_HEAP_TYPE_DEFAULT, ..Default::default() };
            let desc = D3D12_RESOURCE_DESC { Dimension: D3D12_RESOURCE_DIMENSION_TEXTURE2D, Alignment: 0, Width: w as u64, Height: h, DepthOrArraySize: 1, MipLevels: 1, Format: DXGI_FORMAT_NV12, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 }, Layout: D3D12_TEXTURE_LAYOUT_UNKNOWN, Flags: D3D12_RESOURCE_FLAG_ALLOW_RENDER_TARGET | D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS };
            let mut slots = vec![];
            for _ in 0..POOL {
                let mut r: Option<ID3D12Resource> = None;
                werr("the NV12 surface", d3d12.CreateCommittedResource(&heap, D3D12_HEAP_FLAG_SHARED, &desc, D3D12_RESOURCE_STATE_COMMON, None, &mut r))?;
                let nv12 = r.ok_or("no NV12 surface")?;
                let handle: HANDLE = werr("CreateSharedHandle", d3d12.CreateSharedHandle(&nv12, None, GENERIC_ALL.0, None))?;
                let input: windows::core::Result<ID3D11Texture2D> = d1.OpenSharedResource1(handle);
                let _ = CloseHandle(handle);
                let input = werr("OpenSharedResource1", input)?;
                let alloc: ID3D12CommandAllocator = werr("the command allocator", d3d12.CreateCommandAllocator(D3D12_COMMAND_LIST_TYPE_DIRECT))?;
                let list: ID3D12GraphicsCommandList = werr("the command list", d3d12.CreateCommandList(0, D3D12_COMMAND_LIST_TYPE_DIRECT, &alloc, None))?;
                werr("the command list", list.Close())?;
                slots.push(Slot { nv12, input, alloc, list, busy: Arc::new(AtomicBool::new(false)) });
            }
            let fence: ID3D12Fence = werr("the fence", d3d12.CreateFence(0, D3D12_FENCE_FLAG_NONE))?;
            let tex = |label, w, h, format, usage| device.create_texture(&wgpu::TextureDescriptor { label: Some(label), size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format, usage, view_formats: &[] });
            let target = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC;
            let ytex = tex("stream Y", w, h, wgpu::TextureFormat::R8Unorm, target);
            let ctex = tex("stream CbCr", w / 2, h / 2, wgpu::TextureFormat::Rg8Unorm, target);
            let ypark = tex("stream Y park", 4, 4, wgpu::TextureFormat::R8Unorm, wgpu::TextureUsages::COPY_DST);
            let cpark = tex("stream CbCr park", 4, 4, wgpu::TextureFormat::Rg8Unorm, wgpu::TextureUsages::COPY_DST);
            let (yraw, craw) = { let (a, b) = (ytex.as_hal::<wgpu::hal::api::Dx12>().ok_or("the DX12 texture")?, ctex.as_hal::<wgpu::hal::api::Dx12>().ok_or("the DX12 texture")?); (a.raw_resource().clone(), b.raw_resource().clone()) };
            log::info!("stream: {POOL} NV12 surfaces {w}x{h}, shared from the DX12 device into D3D11");
            Ok(Gpu { queue, d3d11, fence, fence_value: 0, slots, ytex, ctex, ypark, cpark, yraw, craw })
        }
    }
}

/// The encoder's surfaces. Created on the first `acquire` — the session may start before the device is there (the original's
/// trap).
pub struct SurfacePool { w: u32, h: u32, gpu: Arc<Mutex<Option<Gpu>>> }
unsafe impl Send for SurfacePool {}
unsafe impl Sync for SurfacePool {}

/// Frees its surface when the last holder lets go (the renderer's hand-over, the clock's repeat, the encoder's input).
pub struct SlotGuard(Arc<AtomicBool>);
impl Drop for SlotGuard { fn drop(&mut self) { self.0.store(false, Release); } }

/// One surface for a picture: the NV12 pass's two targets to render into, then `before_submit` / `after_submit` around the
/// queue's submit move the planes into the encoder's texture.
pub struct Surface {
    pub y: wgpu::TextureView, pub cbcr: wgpu::TextureView,
    ytex: wgpu::Texture, ctex: wgpu::Texture, ypark: wgpu::Texture, cpark: wgpu::Texture,
    slot: usize, guard: Arc<SlotGuard>, input: ID3D11Texture2D, fence: ID3D12Fence, /// the fence value of this surface's plane copy (0 = not issued)
    value: AtomicU64, gpu: Arc<Mutex<Option<Gpu>>>,
}
unsafe impl Send for Surface {}

impl SurfacePool {
    pub fn new(width: u32, height: u32) -> Res<SurfacePool> {
        if width == 0 || height == 0 || width % 2 != 0 || height % 2 != 0 { return Err("stream needs a URL and an even raster".into()) }
        Ok(SurfacePool { w: width, h: height, gpu: Arc::new(Mutex::new(None)) })
    }
    pub fn size(&self) -> (u32, u32) { (self.w, self.h) }
    fn d3d11(&self) -> Option<ID3D11Device> { self.gpu.lock().unwrap().as_ref().map(|g| g.d3d11.clone()) }
    pub fn acquire(&self, device: &wgpu::Device) -> Option<Surface> {
        let mut g = self.gpu.lock().unwrap();
        if g.is_none() { match Gpu::new(device, self.w, self.h) { Ok(n) => *g = Some(n), Err(e) => { log::warn!("stream: surfaces: {e}"); return None } } }
        let gpu = g.as_mut()?;
        let slot = gpu.slots.iter().position(|s| !s.busy.load(Acquire))?;
        let s = &gpu.slots[slot];
        s.busy.store(true, Release);
        Some(Surface { y: gpu.ytex.create_view(&Default::default()), cbcr: gpu.ctex.create_view(&Default::default()), ytex: gpu.ytex.clone(), ctex: gpu.ctex.clone(), ypark: gpu.ypark.clone(), cpark: gpu.cpark.clone(),
            slot, guard: Arc::new(SlotGuard(s.busy.clone())), input: s.input.clone(), fence: gpu.fence.clone(), value: AtomicU64::new(0), gpu: self.gpu.clone() })
    }
}

impl Surface {
    /// Into the encoder that holds the NV12 pass, before it is finished: one texel of each target copied out, so wgpu
    /// leaves them in the state the plane copy reads from.
    pub fn before_submit(&self, enc: &mut wgpu::CommandEncoder) {
        let one = wgpu::Extent3d { width: 4, height: 4, depth_or_array_layers: 1 };
        enc.copy_texture_to_texture(self.ytex.as_image_copy(), self.ypark.as_image_copy(), one);
        enc.copy_texture_to_texture(self.ctex.as_image_copy(), self.cpark.as_image_copy(), one);
    }
    /// Right after the queue's submit: the two targets into the surface's planes, on wgpu's own queue (so behind its work),
    /// and the fence the encoder waits for.
    pub fn after_submit(&self, _device: &wgpu::Device, _queue: &wgpu::Queue) {
        let mut g = self.gpu.lock().unwrap();
        let Some(gpu) = g.as_mut() else { return };
        let r: windows::core::Result<()> = (|| unsafe {
            let s = &gpu.slots[self.slot];
            s.alloc.Reset()?; s.list.Reset(&s.alloc, None)?;
            s.list.ResourceBarrier(&[barrier(&s.nv12, D3D12_RESOURCE_STATE_COMMON, D3D12_RESOURCE_STATE_COPY_DEST)]);
            s.list.CopyTextureRegion(&plane(&s.nv12, 0), 0, 0, 0, &plane(&gpu.yraw, 0), None);
            s.list.CopyTextureRegion(&plane(&s.nv12, 1), 0, 0, 0, &plane(&gpu.craw, 0), None);
            s.list.ResourceBarrier(&[barrier(&s.nv12, D3D12_RESOURCE_STATE_COPY_DEST, D3D12_RESOURCE_STATE_COMMON)]);
            s.list.Close()?;
            gpu.queue.ExecuteCommandLists(&[Some(s.list.cast()?)]);
            gpu.fence_value += 1;
            gpu.queue.Signal(&gpu.fence, gpu.fence_value)?;
            self.value.store(gpu.fence_value, Release);
            Ok(())
        })();
        if let Err(e) = r { log::warn!("stream: the plane copy: {e}"); }
    }
}

unsafe fn hardware_h264() -> windows::core::Result<Vec<(IMFActivate, String)>> {
    unsafe {
        let out = MFT_REGISTER_TYPE_INFO { guidMajorType: MFMediaType_Video, guidSubtype: MFVideoFormat_H264 };
        let (mut list, mut n) = (std::ptr::null_mut::<Option<IMFActivate>>(), 0u32);
        MFTEnumEx(MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER, None, Some(&out), &mut list, &mut n)?;
        let acts: Vec<IMFActivate> = (0..n as usize).filter_map(|i| (*list.add(i)).take()).collect();
        CoTaskMemFree(Some(list as *const _));
        Ok(acts.into_iter().map(|act| {
            let (mut name, mut len) = (PWSTR::null(), 0u32);
            let _ = act.GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &mut name, &mut len);
            let friendly = if name.is_null() { String::new() } else { let s = name.to_string().unwrap_or_default(); CoTaskMemFree(Some(name.0 as *const _)); s };
            (act, friendly)
        }).collect())
    }
}
/// Is there a hardware H.264 encoder on this machine (QCView `available`)? Asked once.
pub fn available() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        if MFStartup(MF_VERSION, MFSTARTUP_FULL).is_err() { return false }
        let n = hardware_h264().map(|v| v.len()).unwrap_or(0);
        let _ = MFShutdown();
        n > 0
    })
}

/// The AVCC-free form the muxer takes: every NAL unit behind a 4-byte start code. Returns (the units, has an SPS, has an IDR).
fn annexb4(data: &[u8]) -> (Vec<u8>, bool, bool) {
    let (mut starts, mut i) = (vec![], 0usize);
    while i + 3 <= data.len() { if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 { starts.push(i + 3); i += 3; } else { i += 1; } }
    let (mut out, mut sps, mut idr) = (Vec::with_capacity(data.len() + 16), false, false);
    for (n, &s) in starts.iter().enumerate() {
        let mut e = starts.get(n + 1).map_or(data.len(), |x| x - 3);
        if n + 1 < starts.len() { while e > s && data[e - 1] == 0 { e -= 1; } }   // the next start code's leading zero
        if s >= e { continue }
        match data[s] & 0x1f { 7 => sps = true, 5 => idr = true, _ => {} }
        out.extend_from_slice(&[0, 0, 0, 1]); out.extend_from_slice(&data[s..e]);
    }
    (out, sps, idr)
}

struct Input { sample: IMFSample, pts: i64, dur: i64, key: bool, guard: Arc<SlotGuard> }
#[derive(Default)]
struct State { need: u32, pending: VecDeque<Input>, /// the frames inside the encoder, oldest first: their timestamps and their surfaces
    flying: VecDeque<(i64, i64, Arc<SlotGuard>)>, params: Vec<u8> }
struct Shared { mft: IMFTransform, codec: ICodecAPI, state: Mutex<State>, out: Box<dyn Fn(EncodedUnit) + Send + Sync>, stop: AtomicBool, encoded: AtomicU64, errors: AtomicU64 }
unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

impl Shared {
    /// Feed the encoder while it asks and something waits (called with the state locked, from either thread).
    fn pump(&self, st: &mut State) {
        while st.need > 0 {
            let Some(i) = st.pending.pop_front() else { break };
            unsafe {
                if i.key { let _ = self.codec.SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &VARIANT::from(1u32)); }
                match self.mft.ProcessInput(0, &i.sample, 0) {
                    Ok(()) => { st.need -= 1; st.flying.push_back((i.pts, i.dur, i.guard)); }
                    Err(e) => { self.errors.fetch_add(1, Relaxed); log::warn!("stream: ProcessInput: {e}"); if e.code() == MF_E_NOTACCEPTING { st.need = 0; } }
                }
            }
        }
    }
    unsafe fn output(&self) {
        unsafe {
            let mut ob = [MFT_OUTPUT_DATA_BUFFER { dwStreamID: 0, pSample: ManuallyDrop::new(None), dwStatus: 0, pEvents: ManuallyDrop::new(None) }];
            let mut status = 0u32;
            match self.mft.ProcessOutput(0, &mut ob, &mut status) {
                Ok(()) => {
                    let sample = ManuallyDrop::take(&mut ob[0].pSample); let _ = ManuallyDrop::take(&mut ob[0].pEvents);
                    let Some(sample) = sample else { return };
                    let clean = sample.GetUINT32(&MFSampleExtension_CleanPoint).unwrap_or(0) != 0;
                    let Ok(buf) = sample.ConvertToContiguousBuffer() else { self.errors.fetch_add(1, Relaxed); return };
                    let (mut p, mut len) = (std::ptr::null_mut::<u8>(), 0u32);
                    if buf.Lock(&mut p, None, Some(&mut len)).is_err() { self.errors.fetch_add(1, Relaxed); return }
                    let (mut data, has_sps, idr) = annexb4(std::slice::from_raw_parts(p, len as usize));
                    let _ = buf.Unlock();
                    let key = clean || idr;
                    let (pts, dur) = {
                        let mut st = self.state.lock().unwrap();
                        if key { if has_sps { st.params = crate::mux::parameter_sets(&data); } else if !st.params.is_empty() { let mut d = st.params.clone(); d.extend_from_slice(&data); data = d; } }
                        match st.flying.pop_front() { Some((pts, dur, _surface)) => (pts, dur), None => (0, 0) }   // the surface is free again here
                    };
                    self.encoded.fetch_add(1, Relaxed);
                    (self.out)(EncodedUnit { data, pts, dur, keyframe: key, audio: false });
                }
                Err(e) if e.code() == MF_E_TRANSFORM_STREAM_CHANGE => { if let Ok(t) = self.mft.GetOutputAvailableType(0, 0) { let _ = self.mft.SetOutputType(0, &t, 0); } }
                Err(e) => { self.errors.fetch_add(1, Relaxed); log::warn!("stream: ProcessOutput: {e}"); }
            }
        }
    }
}

/// The H.264 session. `encode` from one thread (the paced clock); the units arrive through `out` on the session's thread.
pub struct MfSession { sh: Arc<Shared>, thread: Option<std::thread::JoinHandle<()>>, event: HANDLE }
unsafe impl Send for MfSession {}

fn pack(hi: u32, lo: u32) -> u64 { ((hi as u64) << 32) | lo as u64 }

impl MfSession {
    pub fn open(spec: &EncoderSpec, pool: &SurfacePool, out: Box<dyn Fn(EncodedUnit) + Send + Sync>) -> Res<MfSession> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            werr("MFStartup", MFStartup(MF_VERSION, MFSTARTUP_FULL))?;
            let d3d11 = pool.d3d11().ok_or("the stream's surfaces are not ready")?;
            let acts = werr("MFTEnumEx", hardware_h264())?;
            let (act, friendly) = acts.first().ok_or("no hardware encoder on this platform yet")?;
            let mft: IMFTransform = werr("the encoder", act.ActivateObject())?;
            let attrs = werr("the encoder's attributes", mft.GetAttributes())?;
            werr("async unlock", attrs.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1))?;
            let _ = attrs.SetUINT32(&MF_LOW_LATENCY, 1);
            let (mut token, mut mgr) = (0u32, None::<IMFDXGIDeviceManager>);
            werr("the device manager", MFCreateDXGIDeviceManager(&mut token, &mut mgr))?;
            let mgr = mgr.ok_or("no device manager")?;
            werr("the device manager", mgr.ResetDevice(&d3d11, token))?;
            werr("the encoder's device", mft.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, mgr.as_raw() as usize))?;
            let codec: ICodecAPI = werr("ICodecAPI", mft.cast())?;
            // QCView's settings, before the media types; a refused one only warns (this MFT refuses the B-picture count and
            // sends no B-frames regardless)
            let (fps_num, fps_den) = (spec.fps_num.max(1), spec.fps_den.max(1));
            let gop = if spec.gop_frames == 0 { 30 } else { spec.gop_frames };
            let bps = spec.bitrate_kbps * 1000;
            let set = |name: &str, key: &GUID, v: VARIANT| { if let Err(e) = codec.SetValue(key, &v) { log::info!("stream: the encoder refused {name}: {e}"); } };
            set("AVLowLatencyMode", &CODECAPI_AVLowLatencyMode, VARIANT::from(true));
            set("RateControlMode", &CODECAPI_AVEncCommonRateControlMode, VARIANT::from(eAVEncCommonRateControlMode_CBR.0 as u32));
            set("MeanBitRate", &CODECAPI_AVEncCommonMeanBitRate, VARIANT::from(bps));
            set("BPictureCount", &CODECAPI_AVEncMPVDefaultBPictureCount, VARIANT::from(0u32));
            set("GOPSize", &CODECAPI_AVEncMPVGOPSize, VARIANT::from(gop));
            set("QualityVsSpeed", &CODECAPI_AVEncCommonQualityVsSpeed, VARIANT::from(33u32));
            let common = |t: &IMFMediaType| -> windows::core::Result<()> {
                t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
                t.SetUINT64(&MF_MT_FRAME_SIZE, pack(spec.width, spec.height))?; t.SetUINT64(&MF_MT_FRAME_RATE, pack(fps_num, fps_den))?; t.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))?;
                t.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
                t.SetUINT32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32)?; t.SetUINT32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32)?;
                t.SetUINT32(&MF_MT_VIDEO_PRIMARIES, MFVideoPrimaries_BT709.0 as u32)?; t.SetUINT32(&MF_MT_TRANSFER_FUNCTION, MFVideoTransFunc_709.0 as u32)?;
                Ok(())
            };
            let out_type = werr("the output type", MFCreateMediaType())?;
            werr("the output type", (|| { common(&out_type)?; out_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?; out_type.SetUINT32(&MF_MT_AVG_BITRATE, bps)?;
                out_type.SetUINT32(&MF_MT_MPEG2_PROFILE, match spec.profile { Profile::Baseline => eAVEncH264VProfile_Base.0, Profile::Main => eAVEncH264VProfile_Main.0 } as u32)?;
                mft.SetOutputType(0, &out_type, 0) })())?;
            let in_type = werr("the input type", MFCreateMediaType())?;
            werr("the input type", (|| { common(&in_type)?; in_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?; mft.SetInputType(0, &in_type, 0) })())?;
            // the parameter sets the type already carries (a keyframe may come without them)
            let mut params = vec![];
            if let Ok(t) = mft.GetOutputCurrentType(0) { let mut n = 0u32; if t.GetBlobSize(&MF_MT_MPEG_SEQUENCE_HEADER).map(|s| n = s).is_ok() && n > 0 { let mut b = vec![0u8; n as usize]; if t.GetBlob(&MF_MT_MPEG_SEQUENCE_HEADER, &mut b, None).is_ok() { params = crate::mux::parameter_sets(&b); } } }
            werr("begin streaming", mft.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0))?;
            werr("start of stream", mft.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0))?;
            let events: IMFMediaEventGenerator = werr("the encoder's events", mft.cast())?;
            let name = format!("Media Foundation H.264 ({friendly})");
            log::info!("stream: {name} {}x{} {:.3} fps {} kbps GOP {gop} {:?}", spec.width, spec.height, fps_num as f64 / fps_den as f64, spec.bitrate_kbps, spec.profile);
            let sh = Arc::new(Shared { mft, codec, state: Mutex::new(State { params, ..Default::default() }), out, stop: AtomicBool::new(false), encoded: AtomicU64::new(0), errors: AtomicU64::new(0) });
            // the session's thread: the encoder's events — it asks for input, it has output
            struct Events(IMFMediaEventGenerator);
            unsafe impl Send for Events {}
            let (s2, ev) = (sh.clone(), Events(events));
            let thread = std::thread::Builder::new().name("stream encoder".into()).spawn(move || {
                let ev = ev;
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
                loop {
                    match ev.0.GetEvent(MF_EVENT_FLAG_NO_WAIT) {
                        Ok(e) => {
                            let ty = e.GetType().unwrap_or(0);
                            if ty == METransformNeedInput.0 as u32 { let mut st = s2.state.lock().unwrap(); st.need += 1; s2.pump(&mut st); }
                            else if ty == METransformHaveOutput.0 as u32 { s2.output(); }
                        }
                        Err(e) if e.code() == MF_E_NO_EVENTS_AVAILABLE => { if s2.stop.load(Acquire) { break } std::thread::sleep(Duration::from_millis(1)); }
                        Err(e) => { log::warn!("stream: the encoder's events ended: {e}"); break }
                    }
                }
            }).map_err(|e| e.to_string())?;
            let event = werr("the fence event", CreateEventW(None, false, false, None))?;
            Ok(MfSession { sh, thread: Some(thread), event })
        }
    }
    /// Encode `surface` at `pts` / `dur` (90 kHz); `key` forces a keyframe. Waits (bounded) for the GPU to finish the
    /// surface first — this is the clock's thread, never the renderer's.
    pub fn encode(&self, surface: &Surface, pts: i64, dur: i64, key: bool) -> bool {
        unsafe {
            let v = surface.value.load(Acquire);
            if v == 0 { return false }   // its plane copy was never issued
            if surface.fence.GetCompletedValue() < v && surface.fence.SetEventOnCompletion(v, self.event).is_ok() { WaitForSingleObject(self.event, 250); }
            let r: windows::core::Result<IMFSample> = (|| {
                let buffer = MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, &surface.input, 0, false)?;
                if let Ok(b2) = buffer.cast::<IMF2DBuffer>() { if let Ok(n) = b2.GetContiguousLength() { let _ = buffer.SetCurrentLength(n); } }
                let sample = MFCreateSample()?;
                sample.AddBuffer(&buffer)?;
                // 90 kHz → 100 ns, rounded (QCView)
                sample.SetSampleTime((pts * 10_000_000 + 45_000) / 90_000)?; sample.SetSampleDuration(((dur * 10_000_000 + 45_000) / 90_000).max(1))?;
                Ok(sample)
            })();
            let sample = match r { Ok(s) => s, Err(e) => { self.sh.errors.fetch_add(1, Relaxed); log::warn!("stream: the input sample: {e}"); return false } };
            let mut st = self.sh.state.lock().unwrap();
            // QCView's input queue: 4; an overflow drops the oldest and counts an encoder error
            if st.pending.len() >= 4 { st.pending.pop_front(); self.sh.errors.fetch_add(1, Relaxed); }
            st.pending.push_back(Input { sample, pts, dur, key, guard: surface.guard.clone() });
            self.sh.pump(&mut st);
            true
        }
    }
    pub fn encoded(&self) -> u64 { self.sh.encoded.load(Relaxed) }
    pub fn errors(&self) -> u64 { self.sh.errors.load(Relaxed) }
}
impl Drop for MfSession {
    fn drop(&mut self) {
        self.sh.stop.store(true, Release);
        if let Some(t) = self.thread.take() { let _ = t.join(); }
        unsafe {
            let _ = self.sh.mft.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0);
            let _ = self.sh.mft.ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0);
            { let mut st = self.sh.state.lock().unwrap(); st.pending.clear(); st.flying.clear(); }   // every surface back to the pool
            let _ = CloseHandle(self.event);
            let _ = MFShutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn access_units_come_out_with_four_byte_start_codes() {
        // a 3-byte and a 4-byte start code, an SPS, a PPS and an IDR slice
        let au = [0, 0, 1, 0x67, 1, 2, 0, 0, 0, 1, 0x68, 3, 0, 0, 1, 0x65, 9, 9];
        let (out, sps, idr) = annexb4(&au);
        assert_eq!(out, vec![0, 0, 0, 1, 0x67, 1, 2, 0, 0, 0, 1, 0x68, 3, 0, 0, 0, 1, 0x65, 9, 9]);
        assert!(sps && idr);
        let (_, sps, idr) = annexb4(&[0, 0, 0, 1, 0x41, 5]); assert!(!sps && !idr);
    }
}
