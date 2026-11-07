//! The hardware decode device, shared by every decoder of a session: VideoToolbox on macOS (its own context), D3D12VA on
//! Windows built ON the presenter's `ID3D12Device` so decoded resources need no sharing (S1 Windows).
use ffmpeg_sys_next as ff;
use std::{ffi::c_void, ptr, sync::Arc};

struct Inner(*mut ff::AVBufferRef);
unsafe impl Send for Inner {}
unsafe impl Sync for Inner {}
impl Drop for Inner { fn drop(&mut self) { unsafe { ff::av_buffer_unref(&mut self.0) } } }

#[derive(Clone)]
pub struct HwDevice(Arc<Inner>);

#[cfg(target_os = "macos")] pub const HW_PIX_FMT: ff::AVPixelFormat = ff::AVPixelFormat::AV_PIX_FMT_VIDEOTOOLBOX;
#[cfg(windows)] pub const HW_PIX_FMT: ff::AVPixelFormat = ff::AVPixelFormat::AV_PIX_FMT_D3D12;
#[cfg(not(any(target_os = "macos", windows)))] pub const HW_PIX_FMT: ff::AVPixelFormat = ff::AVPixelFormat::AV_PIX_FMT_NONE;

// libavutil/hwcontext_d3d12va.h, field order as in FFmpeg 9 (ffmpeg-sys-next does not bind this header)
#[cfg(windows)] #[repr(C)] pub(crate) struct AVD3D12VASyncContext { pub fence: *mut c_void, pub event: *mut c_void, pub fence_value: u64 }
#[cfg(windows)] #[repr(C)] pub(crate) struct AVD3D12VAFrame { pub texture: *mut c_void, pub sync_ctx: AVD3D12VASyncContext }
#[cfg(windows)] #[repr(C)] struct AVD3D12VADeviceContext { device: *mut c_void, video_device: *mut c_void, lock: *mut c_void, unlock: *mut c_void, lock_ctx: *mut c_void }

impl HwDevice {
    /// macOS: a VideoToolbox device context.
    #[cfg(target_os = "macos")]
    pub fn platform(_presenter_device: Option<*mut c_void>) -> Option<HwDevice> {
        let mut hwref: *mut ff::AVBufferRef = ptr::null_mut();
        let r = unsafe { ff::av_hwdevice_ctx_create(&mut hwref, ff::AVHWDeviceType::AV_HWDEVICE_TYPE_VIDEOTOOLBOX, ptr::null(), ptr::null_mut(), 0) };
        if r < 0 { log::warn!("VideoToolbox device context: {r}"); return None }
        Some(HwDevice(Arc::new(Inner(hwref))))
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
            Some(HwDevice(Arc::new(Inner(hwref))))
        }
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    pub fn platform(_: Option<*mut c_void>) -> Option<HwDevice> { None }
    pub(crate) fn raw(&self) -> *mut ff::AVBufferRef { self.0 .0 }
    pub(crate) fn device_type(&self) -> ff::AVHWDeviceType { unsafe { (*((*self.0 .0).data as *const ff::AVHWDeviceContext)).type_ } }
    pub fn name(&self) -> &'static str { if cfg!(target_os = "macos") { "VideoToolbox" } else if cfg!(windows) { "D3D12VA" } else { "none" } }
}
