//! Windows analogue of `vt.rs` (S1 step 1b on DX12): FFmpeg D3D12VA frames — `AVFrame.data[0]` = `AVD3D12VAFrame { ID3D12Resource
//! (NV12 / P010), sync fence }` — become TWO single-plane wgpu textures wrapping the resource's planes through wgpu-hal's DX12
//! `Device::texture_from_raw(..).with_plane_slice(n)`: ZERO copies. FFmpeg's hw device context is created ON wgpu's own
//! ID3D12Device (`AVD3D12VADeviceContext.device`), so the decoder's resources need no NT-handle sharing; the decoder's fence is
//! staged as a wait on wgpu's queue before the next submit (`hal::dx12::Queue::add_wait_fence`).
use crate::decode;
use ffmpeg_sys_next as ff;
use std::ffi::c_void;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::{ID3D12Device, ID3D12Fence, ID3D12Resource};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_FORMAT_P010};

// libavutil/hwcontext_d3d12va.h, field order as in FFmpeg 9 (ffmpeg-sys-next does not bind this header)
#[repr(C)] struct AVD3D12VASyncContext { fence: *mut c_void, event: *mut c_void, fence_value: u64 }
#[repr(C)] struct AVD3D12VAFrame { texture: *mut c_void, sync_ctx: AVD3D12VASyncContext }
#[repr(C)] struct AVD3D12VADeviceContext { device: *mut c_void, video_device: *mut c_void, lock: *mut c_void, unlock: *mut c_void, lock_ctx: *mut c_void }

pub struct Imported { pub textures: Vec<wgpu::Texture>, pub full_range: bool, pub ten_bit: bool }

pub struct VtImport { queue: wgpu::Queue, hwdev: *mut ff::AVBufferRef, pub last_fourcc: u32, pub waits: u32 }

impl VtImport {
    /// Build FFmpeg's D3D12VA device context on wgpu's ID3D12Device and register it for every `Decoder::open(.., hw = true)`.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        let raw: ID3D12Device = unsafe { device.as_hal::<wgpu::hal::api::Dx12>()?.raw_device().clone() };
        unsafe {
            let hwref = ff::av_hwdevice_ctx_alloc(ff::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D12VA);
            if hwref.is_null() { println!("d3d12va: av_hwdevice_ctx_alloc failed"); return None }
            let hwctx = (*((*hwref).data as *mut ff::AVHWDeviceContext)).hwctx as *mut AVD3D12VADeviceContext;
            (*hwctx).device = raw.into_raw() as *mut c_void;   // FFmpeg releases it with the context
            let r = ff::av_hwdevice_ctx_init(hwref);
            if r < 0 { println!("d3d12va: av_hwdevice_ctx_init failed ({r})"); return None }
            *decode::HW_DEVICE_REF.lock().unwrap() = hwref as usize;
            println!("d3d12va: FFmpeg hw device context created on wgpu's ID3D12Device");
            Some(VtImport { queue: queue.clone(), hwdev: hwref, last_fourcc: 0, waits: 0 })
        }
    }

    /// Wrap one decoded frame's planes as wgpu textures (plane 0 R8/R16Unorm, plane 1 RG8/RG16Unorm) and stage its fence wait.
    pub fn import(&mut self, device: &wgpu::Device, frame: *mut c_void) -> Imported {
        let f = unsafe { &*(frame as *const AVD3D12VAFrame) };
        let res: ID3D12Resource = unsafe { ID3D12Resource::from_raw_borrowed(&f.texture).expect("ID3D12Resource").clone() };
        let desc = unsafe { res.GetDesc() };
        let ten_bit = desc.Format == DXGI_FORMAT_P010;
        if desc.Format != DXGI_FORMAT_NV12 && desc.Format != DXGI_FORMAT_P010 { panic!("d3d12va frame format {:?} is not NV12/P010", desc.Format); }
        self.last_fourcc = if ten_bit { u32::from_be_bytes(*b"P010") } else { u32::from_be_bytes(*b"NV12") };
        let (w, h) = (desc.Width as u32, desc.Height);
        if !f.sync_ctx.fence.is_null() {
            let fence: ID3D12Fence = unsafe { ID3D12Fence::from_raw_borrowed(&f.sync_ctx.fence).expect("ID3D12Fence").clone() };
            if let Some(q) = unsafe { self.queue.as_hal::<wgpu::hal::api::Dx12>() } { q.add_wait_fence(fence, f.sync_ctx.fence_value); self.waits += 1; }
        }
        let mut textures = vec![];
        for plane in 0..2u32 {
            let (fmt, pw, ph) = match (plane, ten_bit) {
                (0, false) => (wgpu::TextureFormat::R8Unorm, w, h), (1, false) => (wgpu::TextureFormat::Rg8Unorm, w / 2, h / 2),
                (0, true) => (wgpu::TextureFormat::R16Unorm, w, h), _ => (wgpu::TextureFormat::Rg16Unorm, w / 2, h / 2),
            };
            let size = wgpu::Extent3d { width: pw, height: ph, depth_or_array_layers: 1 };
            // SAFETY: the resource outlives the texture (the AVFrame ref is held beside it by the caller); plane_slice selects the DXGI plane.
            let hal_tex = unsafe { wgpu::hal::dx12::Device::texture_from_raw(res.clone(), fmt, wgpu::TextureDimension::D2, size, 1, 1) }.with_plane_slice(plane);
            let tex = unsafe { device.create_texture_from_hal::<wgpu::hal::api::Dx12>(hal_tex, &wgpu::TextureDescriptor { label: Some("d3d12va plane"), size, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: fmt, usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] }, wgpu::wgt::TextureUses::RESOURCE) };
            textures.push(tex);
        }
        Imported { textures, full_range: false, ten_bit }
    }

    pub fn flush(&self) {}
}
impl Drop for VtImport { fn drop(&mut self) { *decode::HW_DEVICE_REF.lock().unwrap() = 0; unsafe { ff::av_buffer_unref(&mut self.hwdev) } } }
