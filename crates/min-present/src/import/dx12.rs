//! Windows: a D3D12VA frame (`Dx12FrameHandle`: the NV12 / P010 `ID3D12Resource` + the decoder's fence) → TWO single-plane
//! wgpu textures through wgpu-hal's DX12 `texture_from_raw(..).with_plane_slice(n)`; the fence is staged as a wait on
//! wgpu's queue before the next submit. The decoder's device context must be built on THIS device (`raw_device`).
use super::Imported;
use crate::{frame::Dx12FrameHandle, Error};
use std::ffi::c_void;
use windows::core::Interface;
use windows::Win32::Graphics::Direct3D12::{ID3D12Device, ID3D12Fence, ID3D12Resource};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_FORMAT_P010};

pub struct Dx12Import { queue: wgpu::Queue, pub waits: u32, pub last_format: &'static str }

impl Dx12Import {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        unsafe { device.as_hal::<wgpu::hal::api::Dx12>()? };
        Some(Dx12Import { queue: queue.clone(), waits: 0, last_format: "" })
    }
    /// The `ID3D12Device` wgpu created, with one reference ADDED for the caller (FFmpeg's device context releases it).
    pub fn raw_device(device: &wgpu::Device) -> Option<*mut c_void> {
        let raw: ID3D12Device = unsafe { device.as_hal::<wgpu::hal::api::Dx12>()?.raw_device().clone() };
        Some(raw.into_raw())
    }
    pub fn import(&mut self, device: &wgpu::Device, handle: *mut c_void) -> Result<Imported, Error> {
        if handle.is_null() { return Err(Error::Import("null Dx12FrameHandle".into())) }
        let f = unsafe { *(handle as *const Dx12FrameHandle) };
        let res: ID3D12Resource = unsafe { ID3D12Resource::from_raw_borrowed(&f.resource).ok_or_else(|| Error::Import("null ID3D12Resource".into()))?.clone() };
        let desc = unsafe { res.GetDesc() };
        let ten_bit = desc.Format == DXGI_FORMAT_P010;
        if desc.Format != DXGI_FORMAT_NV12 && desc.Format != DXGI_FORMAT_P010 { return Err(Error::Import(format!("D3D12VA frame format {:?} is not NV12 / P010", desc.Format))) }
        self.last_format = if ten_bit { "P010" } else { "NV12" };
        let (w, h) = (desc.Width as u32, desc.Height);
        if !f.fence.is_null() {
            let fence: ID3D12Fence = unsafe { ID3D12Fence::from_raw_borrowed(&f.fence).ok_or_else(|| Error::Import("null ID3D12Fence".into()))?.clone() };
            if let Some(q) = unsafe { self.queue.as_hal::<wgpu::hal::api::Dx12>() } { q.add_wait_fence(fence, f.fence_value); self.waits += 1; }
        }
        let mut textures = vec![];
        for plane in 0..2u32 {
            let (fmt, pw, ph) = match (plane, ten_bit) {
                (0, false) => (wgpu::TextureFormat::R8Unorm, w, h), (1, false) => (wgpu::TextureFormat::Rg8Unorm, w / 2, h / 2),
                (0, true) => (wgpu::TextureFormat::R16Unorm, w, h), _ => (wgpu::TextureFormat::Rg16Unorm, w / 2, h / 2),
            };
            let size = wgpu::Extent3d { width: pw, height: ph, depth_or_array_layers: 1 };
            // SAFETY: the resource outlives the texture (the producer's `keep` holds the AVFrame beside it); plane_slice selects the DXGI plane.
            let hal_tex = unsafe { wgpu::hal::dx12::Device::texture_from_raw(res.clone(), fmt, wgpu::TextureDimension::D2, size, 1, 1) }.with_plane_slice(plane);
            let tex = unsafe { device.create_texture_from_hal::<wgpu::hal::api::Dx12>(hal_tex, &wgpu::TextureDescriptor { label: Some("d3d12va plane"), size, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: fmt, usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] }, wgpu::wgt::TextureUses::RESOURCE) };
            textures.push(tex);
        }
        Ok(Imported { textures, ten_bit, full_range: None, _keep: Box::new(res) })
    }
}
