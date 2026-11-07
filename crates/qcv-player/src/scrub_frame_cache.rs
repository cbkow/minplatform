//! `ScrubCacheEntry` — ported from `QCView-Player/src/decode/scrub_frame_cache.{h,cpp}`: the decoded-GOP cache's entry.
//!
//! Inter-frame video can only be decoded forward, so the old scrub path re-seeked to the GOP keyframe and re-decoded the
//! prefix on EVERY step. QuickTime / MPV / AVFoundation decode a GOP once and KEEP the frames; this is the "keep" part:
//! a byte-bounded LRU keyed by display frame number, filled as a free by-product of the forward decode.
//!
//! Entries hold the frame in its CHEAP NATIVE form and defer the expensive conversion to display time: a `Planar` entry
//! owns one `AVFrame` ref to the decoded (software / hw-readback) planar frame — a refcount bump, no copy; the plane
//! repack + upload happen only when the frame is shown. A `Native` entry owns one ref to a VideoToolbox frame (CoreVideo
//! pools grow on demand, so Metal is the one hardware backend safe to hold). There is intentionally no native entry for
//! pool-based backends (D3D12VA / Vulkan / VAAPI draw from a FIXED pool; holding a GOP would starve the decoder): those
//! are read back to planes before caching.
use crate::ffmpeg::HwFrame;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Kind { Planar, Native }

pub struct ScrubCacheEntry { kind: Kind, pts: i64, bytes: usize, frame: HwFrame }
impl ScrubCacheEntry {
    /// `owned_ref` must be a fresh `av_frame_clone` of a planar (software / readback) frame; `bytes` its decoded size.
    pub fn planar(owned_ref: HwFrame, pts: i64, bytes: usize) -> Arc<ScrubCacheEntry> { Arc::new(ScrubCacheEntry { kind: Kind::Planar, pts, bytes, frame: owned_ref }) }
    /// `owned_ref` a fresh clone of a VideoToolbox frame (the CVPixelBuffer rides in `data[3]`); size ≈ w × h × 2.
    pub fn native(owned_ref: HwFrame, width: u32, height: u32, pts: i64) -> Arc<ScrubCacheEntry> { Arc::new(ScrubCacheEntry { kind: Kind::Native, pts, bytes: (width as usize) * (height as usize) * 2, frame: owned_ref }) }
    pub fn kind(&self) -> Kind { self.kind }
    pub fn pts(&self) -> i64 { self.pts }
    pub fn bytes(&self) -> usize { self.bytes }
    /// The cached AVFrame (the entry keeps ownership; the caller clones a ref to publish).
    pub fn avframe(&self) -> *mut ffmpeg_sys_next::AVFrame { self.frame.0 }
}
