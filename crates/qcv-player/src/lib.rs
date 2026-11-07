//! `qcv-player` — QCView's player carried by crate (canon §4, row 131: QCView is the reference base). Module by module
//! after `QCView-Player/src/decode`: `video_decoder` (the streaming decoder, the publish slot, seeks, pause, EOF),
//! `frame_handle`, `frame_index` (two tiers), `read_ahead`, `thread_policy`, `hw_routing`, `rgb_range`, `yuv_planar`,
//! `seek_compat`, `stream_extent`, `decoder_cleanup`; `ffmpeg` is the session helper those share. The platform's own
//! pieces: `hw` (the device shared with the presenter), `source` (the image-sequence open through `min-paths`), and
//! `player` (the rational clock over the decoder — the documented divergence, see `video_decoder::Policy`).
//! Step 2 adds the scrub decoder + cache, the Transport, the timecode formatter and audio.
pub mod ffmpeg;
pub mod frame_handle;
pub mod frame_index;
pub mod hw;
pub mod hw_routing;
pub mod player;
pub mod read_ahead;
pub mod rgb_range;
pub mod seek_compat;
pub mod source;
pub mod stream_extent;
pub mod thread_policy;
pub mod video_decoder;
pub mod yuv_planar;
pub mod decoder_cleanup;
pub use ffmpeg::{OpenOptions, SeqOptions, Session, SessionInfo};
pub use frame_handle::FrameHandle;
pub use frame_index::FrameIndex;
pub use hw::HwDevice;
pub use player::{Player, Stats};
pub use rgb_range::RangeOverride;
pub use source::Source;
pub use video_decoder::{Policy, State, VideoDecoder};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{0}")] Open(String),
    #[error("no video stream")] NoVideo,
    #[error("unsupported pixel format {0}")] PixFmt(String),
}

/// This process's CPU time so far (user + kernel), seconds — "cores used" without a profiler.
#[cfg(unix)]
pub fn process_cpu_seconds() -> f64 { unsafe { let mut r: libc::rusage = std::mem::zeroed(); libc::getrusage(libc::RUSAGE_SELF, &mut r); (r.ru_utime.tv_sec as f64 + r.ru_utime.tv_usec as f64 * 1e-6) + (r.ru_stime.tv_sec as f64 + r.ru_stime.tv_usec as f64 * 1e-6) } }
#[cfg(windows)]
pub fn process_cpu_seconds() -> f64 {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    use windows_sys::Win32::Foundation::FILETIME;
    unsafe {
        let z = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut c, mut e, mut k, mut u) = (z, z, z, z);
        if GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u) == 0 { return 0.0 }
        let ft = |f: FILETIME| ((f.dwHighDateTime as u64) << 32 | f.dwLowDateTime as u64) as f64 * 1e-7;
        ft(k) + ft(u)
    }
}
/// Peak resident set, MB.
#[cfg(unix)]
pub fn max_rss_mb() -> f64 { unsafe { let mut r: libc::rusage = std::mem::zeroed(); libc::getrusage(libc::RUSAGE_SELF, &mut r); r.ru_maxrss as f64 / if cfg!(target_os = "macos") { 1048576.0 } else { 1024.0 } } }
#[cfg(windows)]
pub fn max_rss_mb() -> f64 {
    use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    unsafe { let mut c: PROCESS_MEMORY_COUNTERS = std::mem::zeroed(); c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32; if GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) == 0 { return 0.0 } c.PeakWorkingSetSize as f64 / 1048576.0 }
}
