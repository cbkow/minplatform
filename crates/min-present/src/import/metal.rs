//! macOS: a VideoToolbox `CVPixelBuffer` → one `MTLTexture` per plane (`CVMetalTextureCache`) → wgpu through wgpu-hal's
//! Metal `texture_from_raw`. No bytes move. The format table, the range rule and the attachment reads are QCView's
//! `CvPixbufMetalBridge` (`src/render/metal/cv_pixbuf_metal_bridge.mm`); the lifetime rule differs: the wrappers live in
//! `Imported` and the presenter's ring of three releases them (QCView released the previous frame's on the next bridge()).
use super::Imported;
use crate::{frame::{Matrix, Transfer}, Error};
use objc2_core_foundation::{CFRetained, CFString, CFType};
use objc2_core_video::{kCVReturnSuccess, CVBuffer, CVMetalTexture, CVMetalTextureCache, CVMetalTextureGetTexture, CVPixelBuffer, CVPixelBufferGetHeight, CVPixelBufferGetHeightOfPlane, CVPixelBufferGetPixelFormatType, CVPixelBufferGetWidth, CVPixelBufferGetWidthOfPlane};
use objc2_metal::{MTLPixelFormat, MTLTextureType};
use std::{ffi::c_void, ptr::{null_mut, NonNull}};

/// QCView's layout families: NVxx = 8-bit biplanar (R8 / RG8), Pxxx = 10-bit biplanar (R16 / RG16, MSB-aligned),
/// SVxx = 16-bit biplanar, AYpCbCr16 = packed 4:4:4 16-bit with alpha ('y416', ProRes 4444 alpha).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout { Nv12_8, Nv16_8, Nv24_8, P010_10, P210_10, P410_10, Sv22_16, Sv44_16, AYpCbCr16, Unknown }

const fn fourcc(s: &[u8; 4]) -> u32 { u32::from_be_bytes(*s) }
pub fn layout_for(fc: u32) -> Layout {
    match fc {
        x if x == fourcc(b"420v") || x == fourcc(b"420f") => Layout::Nv12_8,
        x if x == fourcc(b"422v") || x == fourcc(b"422f") => Layout::Nv16_8,
        x if x == fourcc(b"444v") || x == fourcc(b"444f") => Layout::Nv24_8,
        x if x == fourcc(b"x420") || x == fourcc(b"xf20") => Layout::P010_10,
        x if x == fourcc(b"x422") || x == fourcc(b"xf22") => Layout::P210_10,
        x if x == fourcc(b"x444") || x == fourcc(b"xf44") => Layout::P410_10,
        x if x == fourcc(b"sv22") => Layout::Sv22_16,
        x if x == fourcc(b"sv44") => Layout::Sv44_16,
        x if x == fourcc(b"y416") => Layout::AYpCbCr16,
        _ => Layout::Unknown,
    }
}
fn is_full_range(fc: u32) -> bool { [fourcc(b"420f"), fourcc(b"422f"), fourcc(b"444f"), fourcc(b"xf20"), fourcc(b"xf22"), fourcc(b"xf44")].contains(&fc) }
fn code_bits(l: Layout) -> u32 { match l { Layout::Nv12_8 | Layout::Nv16_8 | Layout::Nv24_8 => 8, _ => 16 } }
fn plane_formats(l: Layout) -> (MTLPixelFormat, wgpu::TextureFormat, MTLPixelFormat, wgpu::TextureFormat) {
    match l {
        Layout::Nv12_8 | Layout::Nv16_8 | Layout::Nv24_8 => (MTLPixelFormat::R8Unorm, wgpu::TextureFormat::R8Unorm, MTLPixelFormat::RG8Unorm, wgpu::TextureFormat::Rg8Unorm),
        _ => (MTLPixelFormat::R16Unorm, wgpu::TextureFormat::R16Unorm, MTLPixelFormat::RG16Unorm, wgpu::TextureFormat::Rg16Unorm),
    }
}
/// The decoder's zero-copy question (QCView `cvPixelBufferIsZeroCopySupportedRaw`): anything else goes `av_hwframe_transfer_data` + planes.
pub fn zero_copy_supported(pixbuf: *mut c_void) -> bool {
    if pixbuf.is_null() { return false }
    layout_for(CVPixelBufferGetPixelFormatType(unsafe { &*(pixbuf as *const CVPixelBuffer) })) != Layout::Unknown
}
pub fn fourcc_name(pixbuf: *mut c_void) -> String {
    if pixbuf.is_null() { return "null".into() }
    CVPixelBufferGetPixelFormatType(unsafe { &*(pixbuf as *const CVPixelBuffer) }).to_be_bytes().iter().map(|b| *b as char).collect()
}
fn attachment_is(pb: &CVBuffer, key: &CFString, value: &CFString) -> bool {
    #[allow(deprecated)]
    let Some(a): Option<CFRetained<CFType>> = (unsafe { objc2_core_video::CVBufferCopyAttachment(pb, key, null_mut()) }) else { return false };
    a.downcast_ref::<CFString>().map(|s| s == value).unwrap_or(false)
}

pub struct MetalImport { cache: CFRetained<CVMetalTextureCache>, pub last_fourcc: u32 }

impl MetalImport {
    pub fn new(device: &wgpu::Device) -> Option<Self> {
        let mtl = { let hal = unsafe { device.as_hal::<wgpu::hal::api::Metal>() }?; hal.raw_device().clone() };
        let mut out: *mut CVMetalTextureCache = null_mut();
        let r = unsafe { CVMetalTextureCache::create(None, None, &mtl, None, NonNull::from(&mut out)) };
        if r != kCVReturnSuccess { log::warn!("CVMetalTextureCacheCreate failed: {r}"); return None; }
        Some(Self { cache: unsafe { CFRetained::from_raw(NonNull::new(out)?) }, last_fourcc: 0 })
    }
    fn wrap(&self, device: &wgpu::Device, pb: &CVPixelBuffer, mtlfmt: MTLPixelFormat, wfmt: wgpu::TextureFormat, w: usize, h: usize, plane: usize, label: &'static str) -> Result<(wgpu::Texture, CFRetained<CVMetalTexture>), Error> {
        let mut cv: *mut CVMetalTexture = null_mut();
        let r = unsafe { CVMetalTextureCache::create_texture_from_image(None, &self.cache, pb, None, mtlfmt, w, h, plane, NonNull::from(&mut cv)) };
        if r != kCVReturnSuccess { return Err(Error::Import(format!("CVMetalTextureCacheCreateTextureFromImage {label}: {r}"))) }
        let cv = unsafe { CFRetained::from_raw(NonNull::new(cv).ok_or_else(|| Error::Import("null CVMetalTexture".into()))?) };
        let raw = CVMetalTextureGetTexture(&cv).ok_or_else(|| Error::Import("CVMetalTextureGetTexture".into()))?;
        // SAFETY: the MTLTexture is kept alive by `cv`, which `Imported` keeps until the presenter drops the ring slot.
        let hal_tex = unsafe { wgpu::hal::metal::Device::texture_from_raw(raw, wfmt, MTLTextureType::Type2D, 1, 1, wgpu::hal::CopyExtent { width: w as u32, height: h as u32, depth: 1 }, None) };
        let tex = unsafe { device.create_texture_from_hal::<wgpu::hal::api::Metal>(hal_tex, &wgpu::TextureDescriptor { label: Some(label), size: wgpu::Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wfmt, usage: wgpu::TextureUsages::TEXTURE_BINDING, view_formats: &[] }, wgpu::wgt::TextureUses::RESOURCE) };
        Ok((tex, cv))
    }
    pub fn import(&mut self, device: &wgpu::Device, pixbuf: *mut c_void) -> Result<Imported, Error> {
        if pixbuf.is_null() { return Err(Error::Import("null CVPixelBuffer".into())) }
        let pb: &CVPixelBuffer = unsafe { &*(pixbuf as *const CVPixelBuffer) };
        let fc = CVPixelBufferGetPixelFormatType(pb); self.last_fourcc = fc;
        let layout = layout_for(fc);
        if layout == Layout::Unknown { return Err(Error::Import(format!("CVPixelBuffer '{}' is not in the zero-copy table", fourcc_name(pixbuf)))) }
        let matrix = attachment_is(pb, unsafe { objc2_core_video::kCVImageBufferYCbCrMatrixKey }, unsafe { objc2_core_video::kCVImageBufferYCbCrMatrix_ITU_R_2020 }).then_some(Matrix::Bt2020);
        let transfer = if attachment_is(pb, unsafe { objc2_core_video::kCVImageBufferTransferFunctionKey }, unsafe { objc2_core_video::kCVImageBufferTransferFunction_SMPTE_ST_2084_PQ }) { Some(Transfer::Pq) }
            else if attachment_is(pb, unsafe { objc2_core_video::kCVImageBufferTransferFunctionKey }, unsafe { objc2_core_video::kCVImageBufferTransferFunction_ITU_R_2100_HLG }) { Some(Transfer::Hlg) } else { None };
        if layout == Layout::AYpCbCr16 {
            let (w, h) = (CVPixelBufferGetWidth(pb), CVPixelBufferGetHeight(pb));
            let (tex, cv) = self.wrap(device, pb, MTLPixelFormat::RGBA16Unorm, wgpu::TextureFormat::Rgba16Unorm, w, h, 0, "vt packed 4444")?;
            return Ok(Imported { textures: vec![tex], code_bits: 16, packed: true, alpha: true, full_range: Some(false), matrix, transfer, _keep: Box::new(vec![cv]) });
        }
        let (ymtl, ywg, uvmtl, uvwg) = plane_formats(layout);
        let (yw, yh, uvw, uvh) = (CVPixelBufferGetWidthOfPlane(pb, 0), CVPixelBufferGetHeightOfPlane(pb, 0), CVPixelBufferGetWidthOfPlane(pb, 1), CVPixelBufferGetHeightOfPlane(pb, 1));
        let (ty, cy) = self.wrap(device, pb, ymtl, ywg, yw, yh, 0, "vt Y")?;
        let (tuv, cuv) = self.wrap(device, pb, uvmtl, uvwg, uvw, uvh, 1, "vt CbCr")?;
        Ok(Imported { textures: vec![ty, tuv], code_bits: code_bits(layout), packed: false, alpha: false, full_range: Some(is_full_range(fc)), matrix, transfer, _keep: Box::new(vec![cy, cuv]) })
    }
    pub fn flush(&self) { self.cache.flush(0) }
}
