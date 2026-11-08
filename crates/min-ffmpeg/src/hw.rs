//! The hardware decode devices, shared by every decoder of a session: VideoToolbox on macOS (its own context), D3D12VA on
//! Windows built ON the presenter's `ID3D12Device` so decoded resources need no sharing (S1 Windows), and — Windows, S6 —
//! FFmpeg's own Vulkan device for the compute-decoded intra codecs (ProRes, FFV1, APV: QCView's per-codec routing, canon row 149).
//! FFmpeg creates the Vulkan device itself (`av_hwdevice_ctx_create`); on the box its default device already enables
//! `VK_KHR_external_memory_win32` + `VK_KHR_external_semaphore_win32`, which the D3D12 bridge needs (measured 2026-11-07).
use ffmpeg_sys_next as ff;
use std::{ffi::c_void, sync::Arc};

struct Inner { buf: *mut ff::AVBufferRef, kind: HwKind }
unsafe impl Send for Inner {}
unsafe impl Sync for Inner {}
impl Drop for Inner { fn drop(&mut self) { unsafe { ff::av_buffer_unref(&mut self.buf) } } }

/// Which hardware device this is — each has its own hwaccel pixel format and its own frame layout on the way out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HwKind { VideoToolbox, D3d12va, Vulkan }
impl HwKind {
    pub fn pix_fmt(self) -> ff::AVPixelFormat { match self { HwKind::VideoToolbox => ff::AVPixelFormat::AV_PIX_FMT_VIDEOTOOLBOX, HwKind::D3d12va => ff::AVPixelFormat::AV_PIX_FMT_D3D12, HwKind::Vulkan => ff::AVPixelFormat::AV_PIX_FMT_VULKAN } }
    pub fn name(self) -> &'static str { match self { HwKind::VideoToolbox => "VideoToolbox", HwKind::D3d12va => "D3D12VA", HwKind::Vulkan => "Vulkan" } }
    pub fn from_device_type(t: ff::AVHWDeviceType) -> Option<HwKind> { match t { ff::AVHWDeviceType::AV_HWDEVICE_TYPE_VIDEOTOOLBOX => Some(HwKind::VideoToolbox), ff::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D12VA => Some(HwKind::D3d12va), ff::AVHWDeviceType::AV_HWDEVICE_TYPE_VULKAN => Some(HwKind::Vulkan), _ => None } }
}
/// True for any hwaccel output format this crate knows how to carry on (native, bridged, or read back).
pub fn is_hw_format(f: ff::AVPixelFormat) -> bool { matches!(f, ff::AVPixelFormat::AV_PIX_FMT_VIDEOTOOLBOX | ff::AVPixelFormat::AV_PIX_FMT_D3D12 | ff::AVPixelFormat::AV_PIX_FMT_VULKAN) }
/// The hwaccel pixel format of the device context a codec context carries (None when it has none we know).
pub unsafe fn device_pix_fmt(hw_device_ctx: *mut ff::AVBufferRef) -> Option<ff::AVPixelFormat> {
    if hw_device_ctx.is_null() { return None }
    let t = unsafe { (*((*hw_device_ctx).data as *const ff::AVHWDeviceContext)).type_ };
    HwKind::from_device_type(t).map(|k| k.pix_fmt())
}

#[derive(Clone)]
pub struct HwDevice(Arc<Inner>);

// libavutil/hwcontext_d3d12va.h, field order as in FFmpeg 9 (ffmpeg-sys-next does not bind this header)
#[cfg(windows)] #[repr(C)] pub(crate) struct AVD3D12VASyncContext { pub fence: *mut c_void, pub event: *mut c_void, pub fence_value: u64 }
#[cfg(windows)] #[repr(C)] pub(crate) struct AVD3D12VAFrame { pub texture: *mut c_void, pub sync_ctx: AVD3D12VASyncContext }
#[cfg(windows)] #[repr(C)] struct AVD3D12VADeviceContext { device: *mut c_void, video_device: *mut c_void, lock: *mut c_void, unlock: *mut c_void, lock_ctx: *mut c_void }

impl HwDevice {
    /// macOS: a VideoToolbox device context.
    #[cfg(target_os = "macos")]
    pub fn platform(_presenter_device: Option<*mut c_void>) -> Option<HwDevice> {
        let mut hwref: *mut ff::AVBufferRef = std::ptr::null_mut();
        let r = unsafe { ff::av_hwdevice_ctx_create(&mut hwref, ff::AVHWDeviceType::AV_HWDEVICE_TYPE_VIDEOTOOLBOX, std::ptr::null(), std::ptr::null_mut(), 0) };
        if r < 0 { log::warn!("VideoToolbox device context: {r}"); return None }
        Some(HwDevice(Arc::new(Inner { buf: hwref, kind: HwKind::VideoToolbox })))
    }
    /// Windows: FFmpeg's D3D12VA device context on the presenter's `ID3D12Device` (a reference the context releases).
    #[cfg(windows)]
    pub fn platform(presenter_device: Option<*mut c_void>) -> Option<HwDevice> {
        let device = presenter_device?;
        unsafe {
            let hwref = ff::av_hwdevice_ctx_alloc(ff::AVHWDeviceType::AV_HWDEVICE_TYPE_D3D12VA);
            if hwref.is_null() { log::warn!("d3d12va: av_hwdevice_ctx_alloc failed"); return None }
            let hwctx = (*((*hwref).data as *mut ff::AVHWDeviceContext)).hwctx as *mut AVD3D12VADeviceContext;
            (*hwctx).device = device;
            let r = ff::av_hwdevice_ctx_init(hwref);
            if r < 0 { log::warn!("d3d12va: av_hwdevice_ctx_init failed ({r})"); return None }
            Some(HwDevice(Arc::new(Inner { buf: hwref, kind: HwKind::D3d12va })))
        }
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    pub fn platform(_: Option<*mut c_void>) -> Option<HwDevice> { None }
    /// Windows: FFmpeg's own Vulkan device (`av_hwdevice_ctx_create`) for the compute-decoded intra codecs. Logs the enabled
    /// device extensions the D3D12 bridge depends on; None when the loader or the device is missing.
    #[cfg(windows)]
    pub fn vulkan() -> Option<HwDevice> {
        unsafe {
            let mut hwref: *mut ff::AVBufferRef = std::ptr::null_mut();
            let r = ff::av_hwdevice_ctx_create(&mut hwref, ff::AVHWDeviceType::AV_HWDEVICE_TYPE_VULKAN, std::ptr::null(), std::ptr::null_mut(), 0);
            if r < 0 { log::warn!("vulkan: av_hwdevice_ctx_create failed ({r}) — ProRes decodes in software"); return None }
            let vk = (*((*hwref).data as *const ff::AVHWDeviceContext)).hwctx as *const ff::AVVulkanDeviceContext;
            let (mut mem_win32, mut sem_win32) = (false, false);
            for i in 0..(*vk).nb_enabled_dev_extensions.max(0) as usize {
                let e = std::ffi::CStr::from_ptr(*(*vk).enabled_dev_extensions.add(i)).to_string_lossy();
                if e == "VK_KHR_external_memory_win32" { mem_win32 = true } else if e == "VK_KHR_external_semaphore_win32" { sem_win32 = true }
            }
            log::info!("vulkan: FFmpeg device up — {} device extensions, {} queue families; external memory win32 {mem_win32}, external semaphore win32 {sem_win32}", (*vk).nb_enabled_dev_extensions, (*vk).nb_qf);
            if !(mem_win32 && sem_win32) { log::warn!("vulkan: the D3D12 bridge needs VK_KHR_external_memory_win32 + VK_KHR_external_semaphore_win32 — decoded frames will be READ BACK"); }
            Some(HwDevice(Arc::new(Inner { buf: hwref, kind: HwKind::Vulkan })))
        }
    }
    #[cfg(not(windows))]
    pub fn vulkan() -> Option<HwDevice> { None }
    pub fn kind(&self) -> HwKind { self.0.kind }
    pub fn pix_fmt(&self) -> ff::AVPixelFormat { self.0.kind.pix_fmt() }
    pub(crate) fn raw(&self) -> *mut ff::AVBufferRef { self.0.buf }
    pub(crate) fn device_type(&self) -> ff::AVHWDeviceType { unsafe { (*((*self.0.buf).data as *const ff::AVHWDeviceContext)).type_ } }
    pub fn name(&self) -> &'static str { self.0.kind.name() }
}
