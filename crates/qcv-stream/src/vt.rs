//! macOS: the native VideoToolbox H.264 session (QCView `src/output/vt_video_encoder.mm`, its property set kept) and the
//! surfaces it encodes — NV12 video-range `CVPixelBuffer`s, IOSurface-backed and Metal-compatible, from a pool the renderer
//! takes from and writes IN PLACE through wgpu (a Metal texture over each IOSurface plane): no CPU copy between the output
//! pass and the encoder. Output: Annex-B access units (4-byte start codes) with SPS / PPS in band on every keyframe, pts in
//! 90 kHz, on VideoToolbox's own thread — the callback must only enqueue.
//!
//! Raw C bindings (CoreFoundation / CoreVideo / CoreMedia / VideoToolbox): a dozen calls, as the C the original was written in.
use crate::{EncodedUnit, EncoderSpec, Profile};
use objc2_metal::{MTLDevice, MTLPixelFormat, MTLStorageMode, MTLTextureDescriptor, MTLTextureType, MTLTextureUsage};
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

type Ref = *const c_void;
#[repr(C)]
#[derive(Clone, Copy)]
struct CMTime { value: i64, timescale: i32, flags: u32, epoch: i64 }
const fn time(value: i64) -> CMTime { CMTime { value, timescale: 90000, flags: 1, epoch: 0 } }
const INVALID: CMTime = CMTime { value: 0, timescale: 0, flags: 0, epoch: 0 };
type OutputCallback = extern "C" fn(refcon: *mut c_void, source: *mut c_void, status: i32, flags: u32, sample: Ref);

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    static kCFBooleanTrue: Ref; static kCFBooleanFalse: Ref;
    static kCFTypeDictionaryKeyCallBacks: c_void; static kCFTypeDictionaryValueCallBacks: c_void; static kCFTypeArrayCallBacks: c_void;
    fn CFRelease(r: Ref);
    fn CFNumberCreate(alloc: Ref, kind: isize, value: *const c_void) -> Ref;
    fn CFDictionaryCreate(alloc: Ref, keys: *const Ref, values: *const Ref, n: isize, kcb: *const c_void, vcb: *const c_void) -> Ref;
    fn CFDictionaryGetValue(d: Ref, key: Ref) -> Ref;
    fn CFArrayCreate(alloc: Ref, values: *const Ref, n: isize, cb: *const c_void) -> Ref;
    fn CFArrayGetCount(a: Ref) -> isize;
    fn CFArrayGetValueAtIndex(a: Ref, i: isize) -> Ref;
    fn CFBooleanGetValue(b: Ref) -> u8;
}
#[link(name = "CoreVideo", kind = "framework")]
unsafe extern "C" {
    static kCVPixelBufferPixelFormatTypeKey: Ref; static kCVPixelBufferWidthKey: Ref; static kCVPixelBufferHeightKey: Ref;
    static kCVPixelBufferIOSurfacePropertiesKey: Ref; static kCVPixelBufferMetalCompatibilityKey: Ref;
    static kCVPixelBufferPoolMinimumBufferCountKey: Ref; static kCVPixelBufferPoolAllocationThresholdKey: Ref;
    static kCVImageBufferColorPrimaries_ITU_R_709_2: Ref; static kCVImageBufferTransferFunction_ITU_R_709_2: Ref; static kCVImageBufferYCbCrMatrix_ITU_R_709_2: Ref;
    fn CVPixelBufferPoolCreate(alloc: Ref, pool_attrs: Ref, buffer_attrs: Ref, out: *mut Ref) -> i32;
    fn CVPixelBufferPoolCreatePixelBufferWithAuxAttributes(alloc: Ref, pool: Ref, aux: Ref, out: *mut Ref) -> i32;
    fn CVPixelBufferGetIOSurface(pb: Ref) -> Ref;
}
#[link(name = "CoreMedia", kind = "framework")]
unsafe extern "C" {
    static kCMSampleAttachmentKey_NotSync: Ref;
    fn CMSampleBufferGetDataBuffer(s: Ref) -> Ref;
    fn CMSampleBufferGetFormatDescription(s: Ref) -> Ref;
    fn CMSampleBufferGetSampleAttachmentsArray(s: Ref, create: u8) -> Ref;
    fn CMSampleBufferGetPresentationTimeStamp(s: Ref) -> CMTime;
    fn CMSampleBufferGetDuration(s: Ref) -> CMTime;
    fn CMBlockBufferGetDataLength(b: Ref) -> usize;
    fn CMBlockBufferCopyDataBytes(b: Ref, offset: usize, len: usize, dst: *mut u8) -> i32;
    fn CMVideoFormatDescriptionGetH264ParameterSetAtIndex(desc: Ref, index: usize, ptr: *mut *const u8, size: *mut usize, count: *mut usize, nal_len: *mut i32) -> i32;
}
#[link(name = "VideoToolbox", kind = "framework")]
unsafe extern "C" {
    static kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder: Ref;
    static kVTCompressionPropertyKey_RealTime: Ref; static kVTCompressionPropertyKey_AllowFrameReordering: Ref;
    static kVTCompressionPropertyKey_PrioritizeEncodingSpeedOverQuality: Ref; static kVTCompressionPropertyKey_ExpectedFrameRate: Ref;
    static kVTCompressionPropertyKey_MaxKeyFrameInterval: Ref; static kVTCompressionPropertyKey_MaxKeyFrameIntervalDuration: Ref;
    static kVTCompressionPropertyKey_AverageBitRate: Ref; static kVTCompressionPropertyKey_DataRateLimits: Ref;
    static kVTCompressionPropertyKey_ProfileLevel: Ref; static kVTCompressionPropertyKey_H264EntropyMode: Ref;
    static kVTCompressionPropertyKey_ColorPrimaries: Ref; static kVTCompressionPropertyKey_TransferFunction: Ref; static kVTCompressionPropertyKey_YCbCrMatrix: Ref;
    static kVTProfileLevel_H264_Baseline_AutoLevel: Ref; static kVTProfileLevel_H264_Main_AutoLevel: Ref;
    static kVTH264EntropyMode_CAVLC: Ref; static kVTH264EntropyMode_CABAC: Ref;
    static kVTEncodeFrameOptionKey_ForceKeyFrame: Ref;
    fn VTCompressionSessionCreate(alloc: Ref, w: i32, h: i32, codec: u32, encoder_spec: Ref, source_attrs: Ref, data_alloc: Ref, cb: OutputCallback, refcon: *mut c_void, out: *mut Ref) -> i32;
    fn VTSessionSetProperty(session: Ref, key: Ref, value: Ref) -> i32;
    fn VTCompressionSessionPrepareToEncodeFrames(session: Ref) -> i32;
    fn VTCompressionSessionEncodeFrame(session: Ref, image: Ref, pts: CMTime, dur: CMTime, props: Ref, source: *mut c_void, flags_out: *mut u32) -> i32;
    fn VTCompressionSessionCompleteFrames(session: Ref, until: CMTime) -> i32;
    fn VTCompressionSessionInvalidate(session: Ref);
}

const NV12_VIDEO_RANGE: i32 = 0x3432_3076;   // '420v' = kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange
const AVC1: u32 = 0x6176_6331;               // 'avc1'
const WOULD_EXCEED_THRESHOLD: i32 = -6689;   // kCVReturnWouldExceedAllocationThreshold
const START: [u8; 4] = [0, 0, 0, 1];

/// An owned CF object.
struct Cf(Ref);
impl Drop for Cf { fn drop(&mut self) { if !self.0.is_null() { unsafe { CFRelease(self.0) } } } }
fn int(v: i32) -> Cf { Cf(unsafe { CFNumberCreate(std::ptr::null(), 3, &v as *const i32 as *const c_void) }) }        // kCFNumberSInt32Type
fn real(v: f64) -> Cf { Cf(unsafe { CFNumberCreate(std::ptr::null(), 13, &v as *const f64 as *const c_void) }) }      // kCFNumberDoubleType
fn boolean(b: bool) -> Ref { unsafe { if b { kCFBooleanTrue } else { kCFBooleanFalse } } }
fn dict(pairs: &[(Ref, Ref)]) -> Cf {
    let (k, v): (Vec<Ref>, Vec<Ref>) = pairs.iter().copied().unzip();
    Cf(unsafe { CFDictionaryCreate(std::ptr::null(), k.as_ptr(), v.as_ptr(), k.len() as isize, &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks) })
}

/// One NV12 surface from the pool: the pixel buffer and wgpu's views of its two planes (render targets). Dropping it gives
/// the buffer back to the pool once VideoToolbox has let go of it too.
pub struct Surface { pb: Cf, pub y: wgpu::TextureView, pub cbcr: wgpu::TextureView }
unsafe impl Send for Surface {}
impl Surface {
    /// The Windows surface needs work around the queue's submit (its planes are copied on the GPU); here the pass wrote the
    /// encoder's own memory.
    pub fn before_submit(&self, _enc: &mut wgpu::CommandEncoder) {}
    pub fn after_submit(&self, _device: &wgpu::Device, _queue: &wgpu::Queue) {}
}

/// The encoder's surfaces (QCView `prepareSurfaces` / `acquireSurface`): at least 4 buffers, never more than 8 alive — a
/// stalled encoder cannot grow the pool; the renderer skips the frame instead ("≤ 3 in flight, skip rather than queue").
pub struct SurfacePool { pool: Cf, w: u32, h: u32, /// the planes' textures by IOSurface (the pool recycles its buffers)
    textures: Mutex<Vec<(usize, wgpu::Texture, wgpu::Texture)>> }
unsafe impl Send for SurfacePool {}
unsafe impl Sync for SurfacePool {}

impl SurfacePool {
    pub fn new(width: u32, height: u32) -> Result<SurfacePool, String> {
        if width == 0 || height == 0 || width % 2 != 0 || height % 2 != 0 { return Err("stream needs a URL and an even raster".into()) }
        let (fmt, w, h, four) = (int(NV12_VIDEO_RANGE), int(width as i32), int(height as i32), int(4));
        let empty = dict(&[]);
        let attrs = unsafe { dict(&[(kCVPixelBufferPixelFormatTypeKey, fmt.0), (kCVPixelBufferWidthKey, w.0), (kCVPixelBufferHeightKey, h.0), (kCVPixelBufferIOSurfacePropertiesKey, empty.0), (kCVPixelBufferMetalCompatibilityKey, boolean(true))]) };
        let pool_attrs = unsafe { dict(&[(kCVPixelBufferPoolMinimumBufferCountKey, four.0)]) };
        let mut pool: Ref = std::ptr::null();
        let rc = unsafe { CVPixelBufferPoolCreate(std::ptr::null(), pool_attrs.0, attrs.0, &mut pool) };
        if rc != 0 || pool.is_null() { return Err(format!("CVPixelBufferPoolCreate failed ({rc})")) }
        Ok(SurfacePool { pool: Cf(pool), w: width, h: height, textures: Mutex::new(vec![]) })
    }
    pub fn size(&self) -> (u32, u32) { (self.w, self.h) }
    /// A surface to render into; None when 8 are alive (the encoder is behind: skip this frame) or on a failure.
    pub fn acquire(&self, device: &wgpu::Device) -> Option<Surface> {
        let eight = int(8);
        let aux = unsafe { dict(&[(kCVPixelBufferPoolAllocationThresholdKey, eight.0)]) };
        let mut pb: Ref = std::ptr::null();
        let rc = unsafe { CVPixelBufferPoolCreatePixelBufferWithAuxAttributes(std::ptr::null(), self.pool.0, aux.0, &mut pb) };
        if rc != 0 || pb.is_null() { if rc != WOULD_EXCEED_THRESHOLD { log::warn!("stream: pixel buffer from the pool failed ({rc})"); } return None }
        let pb = Cf(pb);
        let ios = unsafe { CVPixelBufferGetIOSurface(pb.0) };
        if ios.is_null() { log::warn!("stream: the pixel buffer has no IOSurface"); return None }
        let mut cache = self.textures.lock().unwrap();
        let (y, c) = match cache.iter().find(|t| t.0 == ios as usize) {
            Some(t) => (t.1.clone(), t.2.clone()),
            None => {
                let y = plane(device, ios, 0, self.w, self.h, MTLPixelFormat::R8Unorm, wgpu::TextureFormat::R8Unorm)?;
                let c = plane(device, ios, 1, self.w / 2, self.h / 2, MTLPixelFormat::RG8Unorm, wgpu::TextureFormat::Rg8Unorm)?;
                if cache.len() >= 16 { cache.clear(); }   // the pool replaced its buffers: forget the old surfaces
                cache.push((ios as usize, y.clone(), c.clone()));
                (y, c)
            }
        };
        Some(Surface { pb, y: y.create_view(&Default::default()), cbcr: c.create_view(&Default::default()) })
    }
}

/// A wgpu render target over one plane of an IOSurface.
fn plane(device: &wgpu::Device, ios: Ref, index: usize, w: u32, h: u32, mfmt: MTLPixelFormat, wfmt: wgpu::TextureFormat) -> Option<wgpu::Texture> {
    let mtl = { let hal = unsafe { device.as_hal::<wgpu::hal::api::Metal>() }?; hal.raw_device().clone() };
    let desc = unsafe { MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(mfmt, w as usize, h as usize, false) };
    desc.setUsage(MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead);
    desc.setStorageMode(MTLStorageMode::Shared);
    let raw = mtl.newTextureWithDescriptor_iosurface_plane(&desc, unsafe { &*(ios as *const objc2_io_surface::IOSurfaceRef) }, index)?;
    let hal = unsafe { wgpu::hal::metal::Device::texture_from_raw(raw, wfmt, MTLTextureType::Type2D, 1, 1, wgpu::hal::CopyExtent { width: w, height: h, depth: 1 }, None) };
    let wdesc = wgpu::TextureDescriptor { label: Some("stream surface plane"), size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wfmt, usage: wgpu::TextureUsages::RENDER_ATTACHMENT, view_formats: &[] };
    Some(unsafe { device.create_texture_from_hal::<wgpu::hal::api::Metal>(hal, &wdesc, wgpu::wgt::TextureUses::UNINITIALIZED) })
}

struct Callback { out: Box<dyn Fn(EncodedUnit) + Send + Sync>, encoded: AtomicU64, errors: AtomicU64 }

/// The H.264 session. `encode` from one thread (the paced clock); the units arrive through `out` on VideoToolbox's thread.
pub struct VtSession { session: Ref, cb: Box<Callback> }
unsafe impl Send for VtSession {}

impl VtSession {
    pub const NAME: &'static str = "VideoToolbox H.264";
    pub fn available() -> bool { true }
    pub fn open(spec: &EncoderSpec, _pool: &SurfacePool, out: Box<dyn Fn(EncodedUnit) + Send + Sync>) -> Result<VtSession, String> {
        let cb = Box::new(Callback { out, encoded: AtomicU64::new(0), errors: AtomicU64::new(0) });
        let (fmt, w, h) = (int(NV12_VIDEO_RANGE), int(spec.width as i32), int(spec.height as i32));
        let mut session: Ref = std::ptr::null();
        let rc = unsafe {
            let enc_spec = dict(&[(kVTVideoEncoderSpecification_RequireHardwareAcceleratedVideoEncoder, boolean(true))]);
            let image = dict(&[(kCVPixelBufferPixelFormatTypeKey, fmt.0), (kCVPixelBufferWidthKey, w.0), (kCVPixelBufferHeightKey, h.0)]);
            VTCompressionSessionCreate(std::ptr::null(), spec.width as i32, spec.height as i32, AVC1, enc_spec.0, image.0, std::ptr::null(), on_output, &*cb as *const Callback as *mut c_void, &mut session)
        };
        if rc != 0 || session.is_null() { return Err(format!("no hardware H.264 encoder (VTCompressionSessionCreate {rc})")) }
        let s = VtSession { session, cb };
        let fps = spec.fps_num.max(1) as f64 / spec.fps_den.max(1) as f64;
        let gop = if spec.gop_frames == 0 { 30 } else { spec.gop_frames };
        let bps = spec.bitrate_kbps as i32 * 1000;
        // QCView's property set (vt:150-194); a rejected property only warns
        unsafe {
            let set = |name: &str, key: Ref, value: Ref| { let rc = VTSessionSetProperty(s.session, key, value); if rc != 0 { log::warn!("stream: VideoToolbox rejected {name} ({rc})"); } };
            set("RealTime", kVTCompressionPropertyKey_RealTime, boolean(true));
            set("AllowFrameReordering", kVTCompressionPropertyKey_AllowFrameReordering, boolean(false));   // never B-frames (Louper)
            set("PrioritizeEncodingSpeedOverQuality", kVTCompressionPropertyKey_PrioritizeEncodingSpeedOverQuality, boolean(true));
            set("ExpectedFrameRate", kVTCompressionPropertyKey_ExpectedFrameRate, real(fps).0);
            set("MaxKeyFrameInterval", kVTCompressionPropertyKey_MaxKeyFrameInterval, int(gop as i32).0);
            set("MaxKeyFrameIntervalDuration", kVTCompressionPropertyKey_MaxKeyFrameIntervalDuration, real(1.0).0);
            set("AverageBitRate", kVTCompressionPropertyKey_AverageBitRate, int(bps).0);
            // at most twice the byte rate over any second
            let (bytes, secs) = (int(bps / 8 * 2), real(1.0));
            let limits = Cf(CFArrayCreate(std::ptr::null(), [bytes.0, secs.0].as_ptr(), 2, &kCFTypeArrayCallBacks));
            set("DataRateLimits", kVTCompressionPropertyKey_DataRateLimits, limits.0);
            let (level, entropy) = match spec.profile { Profile::Baseline => (kVTProfileLevel_H264_Baseline_AutoLevel, kVTH264EntropyMode_CAVLC), Profile::Main => (kVTProfileLevel_H264_Main_AutoLevel, kVTH264EntropyMode_CABAC) };
            set("ProfileLevel", kVTCompressionPropertyKey_ProfileLevel, level);
            set("H264EntropyMode", kVTCompressionPropertyKey_H264EntropyMode, entropy);
            set("ColorPrimaries", kVTCompressionPropertyKey_ColorPrimaries, kCVImageBufferColorPrimaries_ITU_R_709_2);
            set("TransferFunction", kVTCompressionPropertyKey_TransferFunction, kCVImageBufferTransferFunction_ITU_R_709_2);
            set("YCbCrMatrix", kVTCompressionPropertyKey_YCbCrMatrix, kCVImageBufferYCbCrMatrix_ITU_R_709_2);
            let rc = VTCompressionSessionPrepareToEncodeFrames(s.session);
            if rc != 0 { log::warn!("stream: VTCompressionSessionPrepareToEncodeFrames {rc}"); }
        }
        log::info!("stream: {} {}x{} {:.3} fps {} kbps GOP {gop} {:?}", Self::NAME, spec.width, spec.height, fps, spec.bitrate_kbps, spec.profile);
        Ok(s)
    }
    /// Encode `surface` at `pts` / `dur` (90 kHz); `key` forces a keyframe (the first frame, the one after a reconnect).
    pub fn encode(&self, surface: &Surface, pts: i64, dur: i64, key: bool) -> bool {
        let props = key.then(|| unsafe { dict(&[(kVTEncodeFrameOptionKey_ForceKeyFrame, boolean(true))]) });
        let rc = unsafe { VTCompressionSessionEncodeFrame(self.session, surface.pb.0, time(pts), time(dur), props.as_ref().map_or(std::ptr::null(), |p| p.0), std::ptr::null_mut(), std::ptr::null_mut()) };
        if rc != 0 { self.cb.errors.fetch_add(1, Ordering::Relaxed); log::warn!("stream: VTCompressionSessionEncodeFrame {rc}"); }
        rc == 0
    }
    pub fn encoded(&self) -> u64 { self.cb.encoded.load(Ordering::Relaxed) }
    pub fn errors(&self) -> u64 { self.cb.errors.load(Ordering::Relaxed) }
}
impl Drop for VtSession {
    /// QCView `close`: finish what is in flight (the callbacks run before this returns), then invalidate.
    fn drop(&mut self) { unsafe { VTCompressionSessionCompleteFrames(self.session, INVALID); VTCompressionSessionInvalidate(self.session); CFRelease(self.session); } }
}

/// VideoToolbox's output: AVCC (length-prefixed NAL units) → Annex-B, the parameter sets prepended on a keyframe.
extern "C" fn on_output(refcon: *mut c_void, _source: *mut c_void, status: i32, _flags: u32, sample: Ref) {
    let cb = unsafe { &*(refcon as *const Callback) };
    if status != 0 || sample.is_null() { cb.errors.fetch_add(1, Ordering::Relaxed); return }
    unsafe {
        // a sync sample unless the attachment says NotSync
        let atts = CMSampleBufferGetSampleAttachmentsArray(sample, 0);
        let mut key = true;
        if !atts.is_null() && CFArrayGetCount(atts) > 0 { let v = CFDictionaryGetValue(CFArrayGetValueAtIndex(atts, 0), kCMSampleAttachmentKey_NotSync); if !v.is_null() && CFBooleanGetValue(v) != 0 { key = false; } }
        let desc = CMSampleBufferGetFormatDescription(sample);
        let (mut count, mut nal_len) = (0usize, 4i32);
        if !desc.is_null() { CMVideoFormatDescriptionGetH264ParameterSetAtIndex(desc, 0, std::ptr::null_mut(), std::ptr::null_mut(), &mut count, &mut nal_len); }
        let nal_len = if (1..=4).contains(&nal_len) { nal_len as usize } else { 4 };
        let block = CMSampleBufferGetDataBuffer(sample);
        if block.is_null() { cb.errors.fetch_add(1, Ordering::Relaxed); return }
        let len = CMBlockBufferGetDataLength(block);
        let mut avcc = vec![0u8; len];
        if CMBlockBufferCopyDataBytes(block, 0, len, avcc.as_mut_ptr()) != 0 { cb.errors.fetch_add(1, Ordering::Relaxed); return }
        let mut data = Vec::with_capacity(len + 64);
        if key { for i in 0..count {
            let (mut p, mut n) = (std::ptr::null::<u8>(), 0usize);
            if CMVideoFormatDescriptionGetH264ParameterSetAtIndex(desc, i, &mut p, &mut n, std::ptr::null_mut(), std::ptr::null_mut()) == 0 && !p.is_null() { data.extend_from_slice(&START); data.extend_from_slice(std::slice::from_raw_parts(p, n)); }
        } }
        let mut o = 0;
        while o + nal_len <= len {
            let n = avcc[o..o + nal_len].iter().fold(0usize, |a, b| (a << 8) | *b as usize); o += nal_len;
            if n == 0 || o + n > len { break }
            data.extend_from_slice(&START); data.extend_from_slice(&avcc[o..o + n]); o += n;
        }
        let to90k = |t: CMTime| if t.flags & 1 != 0 && t.timescale > 0 { (t.value as i128 * 90000 / t.timescale as i128) as i64 } else { 0 };
        let (pts, dur) = (to90k(CMSampleBufferGetPresentationTimeStamp(sample)), to90k(CMSampleBufferGetDuration(sample)));
        cb.encoded.fetch_add(1, Ordering::Relaxed);
        (cb.out)(EncodedUnit { data, pts, dur, keyframe: key, audio: false });
    }
}
