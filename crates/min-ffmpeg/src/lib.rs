//! `min-ffmpeg` — the family's FFmpeg substrate (canon row 188, lifted out of `qcv-player` 2026-11-07 so minNotes, ufb,
//! MinRender and the QCView port decode through ONE crate): FFmpeg 9 behind thin RAII — `Session` (open / read / send /
//! receive / seek), the `FrameIndex` (PTS ↔ frame, keyframes, the background scan), the hardware devices (`HwDevice`:
//! VideoToolbox / D3D12VA) and QCView's per-codec routing, the read-ahead warmer, the thread policy, the RGB range rule —
//! with decoded frames handed on as `min_present::Frame`s inside a `FrameHandle`. Nothing here knows about clocks, slots,
//! scrubbing or audio: those are the player's.
pub mod ffmpeg;
pub mod frame_handle;
pub mod frame_index;
pub mod hw;
pub mod hw_routing;
pub mod read_ahead;
pub mod rgb_range;
pub mod seek_compat;
pub mod source;
pub mod thread_policy;
pub mod yuv_planar;
pub mod decoder_cleanup;
pub use ffmpeg::{OpenOptions, SeqOptions, Session, SessionInfo};
pub use frame_handle::FrameHandle;
pub use frame_index::FrameIndex;
pub use hw::HwDevice;
pub use rgb_range::RangeOverride;
pub use source::Source;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{0}")] Open(String),
    #[error("no video stream")] NoVideo,
    #[error("unsupported pixel format {0}")] PixFmt(String),
}
