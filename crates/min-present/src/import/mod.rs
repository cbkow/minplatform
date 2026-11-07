//! Zero-copy imports (canon §4.4, row 46): the decoder's own GPU memory becomes wgpu textures through wgpu-hal — one
//! module per backend, the only `as_hal` code in the crate. `metal` carries QCView's `CvPixbufMetalBridge` format table.
use crate::{frame::{Matrix, Transfer}, Error};
use std::{any::Any, ffi::c_void};

#[cfg(target_os = "macos")] pub mod metal;
#[cfg(windows)] pub mod dx12;

/// What the bridge learned about one imported frame (QCView's `PlaneSet`): the plane textures (Y + CbCr, or one packed
/// A,Y,Cb,Cr texture), the code alignment (8-bit, or 16-bit MSB-aligned for 10 / 16-bit layouts), the range from the
/// pixel format, matrix + transfer from the buffer's attachments when present.
pub struct Imported { pub textures: Vec<wgpu::Texture>, pub code_bits: u32, pub packed: bool, pub alpha: bool, pub full_range: Option<bool>, pub matrix: Option<Matrix>, pub transfer: Option<Transfer>, pub(crate) _keep: Box<dyn Any> }

pub(crate) enum Imports {
    #[cfg(target_os = "macos")] Metal(metal::MetalImport),
    #[cfg(windows)] Dx12(dx12::Dx12Import),
    None,
}
impl Imports {
    pub(crate) fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Imports {
        #[cfg(target_os = "macos")] { let _ = queue; if let Some(m) = metal::MetalImport::new(device) { return Imports::Metal(m) } }
        #[cfg(windows)] { if let Some(d) = dx12::Dx12Import::new(device, queue) { return Imports::Dx12(d) } }
        let _ = (device, queue);
        Imports::None
    }
    pub(crate) fn available(&self) -> bool { !matches!(self, Imports::None) }
    pub(crate) fn import(&mut self, device: &wgpu::Device, handle: *mut c_void) -> Result<Imported, Error> {
        match self {
            #[cfg(target_os = "macos")] Imports::Metal(m) => m.import(device, handle),
            #[cfg(windows)] Imports::Dx12(d) => d.import(device, handle),
            Imports::None => { let _ = (device, handle); Err(Error::Import("no zero-copy import on this backend".into())) }
        }
    }
    pub(crate) fn flush(&self) {
        match self {
            #[cfg(target_os = "macos")] Imports::Metal(m) => m.flush(),
            _ => {}
        }
    }
}
