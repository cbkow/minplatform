//! S4 — Metal ZERO-COPY import of a ring slot: the mapped slot's pixel region (page-aligned, page-sized by construction) wrapped
//! ONCE per slot with `newBufferWithBytesNoCopy` (shared storage, no deallocator — the mapping owns the memory), then viewed as a
//! linear RGBA16F texture (`newTextureWithDescriptor:offset:bytesPerRow`, rows are 256-byte multiples), then handed to wgpu via hal.
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_metal::{MTLBuffer, MTLDevice, MTLPixelFormat, MTLResourceOptions, MTLTextureDescriptor, MTLTextureType, MTLTextureUsage, MTLStorageMode};

pub struct SlotBuffers { pub device: Retained<ProtocolObject<dyn MTLDevice>>, pub buffers: Vec<Retained<ProtocolObject<dyn MTLBuffer>>> }

impl SlotBuffers {
    pub fn new(device: &wgpu::Device, base: *mut u8, offsets: &[u64], len: u64) -> Option<SlotBuffers> {
        let mtl = { let hal = unsafe { device.as_hal::<wgpu::hal::api::Metal>() }?; hal.raw_device().clone() };
        let mut buffers = vec![];
        for &off in offsets {
            let ptr = std::ptr::NonNull::new(unsafe { base.add(off as usize) } as *mut std::ffi::c_void)?;
            let buf = unsafe { mtl.newBufferWithBytesNoCopy_length_options_deallocator(ptr, len as usize, MTLResourceOptions::StorageModeShared, None) }?;
            buffers.push(buf);
        }
        Some(SlotBuffers { device: mtl, buffers })
    }
    /// A wgpu texture over slot `i`'s bytes — no copy. Keep it alive as long as the frame is shown.
    pub fn texture(&self, device: &wgpu::Device, i: usize, w: u32, h: u32, bytes_per_row: u32) -> Option<wgpu::Texture> {
        let desc = unsafe { MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(MTLPixelFormat::RGBA16Float, w as usize, h as usize, false) };
        desc.setUsage(MTLTextureUsage::ShaderRead);
        desc.setStorageMode(MTLStorageMode::Shared);
        let raw = self.buffers[i].newTextureWithDescriptor_offset_bytesPerRow(&desc, 0, bytes_per_row as usize)?;
        let hal = unsafe { wgpu::hal::metal::Device::texture_from_raw(raw, wgpu::TextureFormat::Rgba16Float, MTLTextureType::Type2D, 1, 1, wgpu::hal::CopyExtent { width: w, height: h, depth: 1 }, None) };
        let wdesc = wgpu::TextureDescriptor { label: Some("ring slot (zero-copy)"), size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::Rgba16Float, usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] };
        Some(unsafe { device.create_texture_from_hal::<wgpu::hal::api::Metal>(hal, &wdesc, wgpu::wgt::TextureUses::RESOURCE) })
    }
}
