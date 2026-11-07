//! `qcv-player` — the family's player (canon §4): FFmpeg 9 through raw bindings + thin RAII (no safe wrapper covers
//! hwaccel / zero-copy), a hardware device shared between the decoders and the presenter, the streaming decoder slot
//! on its own thread, and the rational clock that decides which frame each refresh shows (S2's rules). Step 2 adds the
//! scrub decoder, the second slot, the Transport and audio.
pub mod ffmpeg;
pub mod hw;
pub mod slot;
pub mod player;
pub mod source;
pub use ffmpeg::{Decoded, Decoder, OpenOptions, SeqOptions};
pub use hw::HwDevice;
pub use player::{Player, Stats};
pub use slot::Slot;
pub use source::Source;

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
