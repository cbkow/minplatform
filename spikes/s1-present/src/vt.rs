//! macOS zero-copy: a VideoToolbox CVPixelBuffer → MTLTexture per plane (CVMetalTextureCache) → wgpu texture
//! through wgpu-hal's Metal `texture_from_raw`. No bytes move; the IOSurface the decoder wrote is what the
//! shader samples. The CVMetalTexture + AVFrame are held alive by the caller until the GPU is done.
use objc2_core_foundation::CFRetained;
use objc2_core_video::{kCVReturnSuccess, CVMetalTexture, CVMetalTextureCache, CVMetalTextureGetTexture, CVPixelBuffer, CVPixelBufferGetHeightOfPlane, CVPixelBufferGetPixelFormatType, CVPixelBufferGetPlaneCount, CVPixelBufferGetWidthOfPlane};
use objc2_metal::{MTLPixelFormat, MTLTextureType};
use std::ptr::{null_mut, NonNull};

pub struct VtImport { cache: CFRetained<CVMetalTextureCache>, pub last_fourcc: u32 }
pub struct Imported { pub textures: Vec<wgpu::Texture>, _cv: Vec<CFRetained<CVMetalTexture>>, pub full_range: bool, pub ten_bit: bool }

fn fourcc(s: &[u8; 4]) -> u32 { u32::from_be_bytes(*s) }

impl VtImport {
    pub fn new(device: &wgpu::Device) -> Option<Self> {
        let mtl = { let hal = unsafe { device.as_hal::<wgpu::hal::api::Metal>() }?; hal.raw_device().clone() };
        let mut out: *mut CVMetalTextureCache = null_mut();
        let r = unsafe { CVMetalTextureCache::create(None, None, &mtl, None, NonNull::from(&mut out)) };
        if r != kCVReturnSuccess { println!("CVMetalTextureCacheCreate failed: {r}"); return None; }
        Some(Self { cache: unsafe { CFRetained::from_raw(NonNull::new(out)?) }, last_fourcc: 0 })
    }
    pub fn import(&mut self, device: &wgpu::Device, pixbuf: *mut std::ffi::c_void) -> Imported {
        let pb: &CVPixelBuffer = unsafe { &*(pixbuf as *const CVPixelBuffer) };
        let fc = CVPixelBufferGetPixelFormatType(pb); self.last_fourcc = fc;
        let ten_bit = [fourcc(b"x422"), fourcc(b"x420"), fourcc(b"x444"), fourcc(b"xf20"), fourcc(b"xf22"), fourcc(b"xf44")].contains(&fc);
        let full_range = [fourcc(b"420f"), fourcc(b"422f"), fourcc(b"444f"), fourcc(b"xf20"), fourcc(b"xf22"), fourcc(b"xf44")].contains(&fc);
        let planes = CVPixelBufferGetPlaneCount(pb);
        assert!(planes == 2, "expected a biplanar CVPixelBuffer, got {planes} planes (fourcc {:?})", fc.to_be_bytes().map(|b| b as char));
        let mut textures = Vec::new(); let mut cvs = Vec::new();
        for plane in 0..planes {
            let (mtlfmt, wfmt) = match (plane, ten_bit) {
                (0, false) => (MTLPixelFormat::R8Unorm, wgpu::TextureFormat::R8Unorm), (0, true) => (MTLPixelFormat::R16Unorm, wgpu::TextureFormat::R16Unorm),
                (_, false) => (MTLPixelFormat::RG8Unorm, wgpu::TextureFormat::Rg8Unorm), (_, true) => (MTLPixelFormat::RG16Unorm, wgpu::TextureFormat::Rg16Unorm) };
            let (w, h) = (CVPixelBufferGetWidthOfPlane(pb, plane), CVPixelBufferGetHeightOfPlane(pb, plane));
            let mut cv: *mut CVMetalTexture = null_mut();
            let r = unsafe { CVMetalTextureCache::create_texture_from_image(None, &self.cache, pb, None, mtlfmt, w, h, plane, NonNull::from(&mut cv)) };
            assert!(r == kCVReturnSuccess, "CVMetalTextureCacheCreateTextureFromImage plane {plane}: {r}");
            let cv = unsafe { CFRetained::from_raw(NonNull::new(cv).unwrap()) };
            let raw = CVMetalTextureGetTexture(&cv).expect("MTLTexture");
            let hal_tex = unsafe { wgpu::hal::metal::Device::texture_from_raw(raw, wfmt, MTLTextureType::Type2D, 1, 1, wgpu::hal::CopyExtent { width: w as u32, height: h as u32, depth: 1 }, None) };
            let tex = unsafe { device.create_texture_from_hal::<wgpu::hal::api::Metal>(hal_tex, &wgpu::TextureDescriptor { label: Some("vt plane"), size: wgpu::Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wfmt, usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] }, wgpu::wgt::TextureUses::RESOURCE) };
            textures.push(tex); cvs.push(cv);
        }
        Imported { textures, _cv: cvs, full_range, ten_bit }
    }
    pub fn flush(&self) { self.cache.flush(0) }
}
