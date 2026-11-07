//! `FrameHandle` — ported from `QCView-Player/src/decode/frame_handle.h`: a decoded frame the renderer owns until it
//! drops it. The pixels live in `min_present::Frame` (planes, or a platform handle with its keep-alive: QCView's Metal /
//! D3D11 / Vulkan / CpuYuv kinds); the handle adds QCView's `pts` (stream ticks and microseconds) and, for the
//! platform's clock, the display frame number from `FrameIndex` (a divergence: QCView's renderer never needs it).
pub struct FrameHandle {
    pub frame: min_present::Frame,
    /// stream time-base ticks (best_effort_timestamp, else pts)
    pub pts: i64,
    /// microseconds, for comparing handles from different sources on one axis
    pub pts_us: i64,
    /// display frame number (`FrameIndex::frame_for_pts`)
    pub frame_no: i32,
    pub keyframe: bool,
    /// `avcodec_receive_frame` wall time incl. the plane repack, ms
    pub decode_ms: f32,
}
impl FrameHandle { pub fn is_native(&self) -> bool { self.frame.is_native() } }
