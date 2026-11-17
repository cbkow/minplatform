//! The application's log file (canon row 519, point 16; 1.4.2 had none): what the application says of itself - its
//! notes, a failure's words, a panic with its place - goes to `ufb-log.txt` in the user's log folder instead of to a
//! console nobody sees (a windowed application has none on Windows). The log of the run before is kept beside it as
//! `ufb-log.prev.txt`, so what a crash said survives one relaunch.
//!
//! macOS `~/Library/Logs/<name>`, Windows `%LOCALAPPDATA%\<name>\logs`; `UFB_LOG_DIR` names another folder,
//! `UFB_LOG=stderr` keeps the console. A scripted look (`UFB_DEMO`, `UFB_SHOT`) keeps the console too: its script reads it.
use std::io::Write;
use std::path::PathBuf;

pub fn dir(name: &str) -> PathBuf {
    if let Some(d) = std::env::var_os("UFB_LOG_DIR").filter(|d| !d.is_empty()) { return d.into() }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from).unwrap_or_default();
    if cfg!(target_os = "macos") { home.join("Library/Logs").join(name) } else if cfg!(windows) { std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join(name).join("logs")).unwrap_or_else(|| home.join(name).join("logs")) } else { home.join(".local/state").join(name) }
}
pub fn wanted() -> bool { std::env::var_os("UFB_DEMO").is_none() && std::env::var_os("UFB_SHOT").is_none() && std::env::var("UFB_LOG").as_deref() != Ok("stderr") }

/// From here on what is written to the error stream is written to the file. The file's path; none: it could not be
/// made, and the console (if there is one) keeps it.
pub fn install(name: &str) -> Option<PathBuf> {
    let d = dir(name); std::fs::create_dir_all(&d).ok()?; let (path, prev) = (d.join("ufb-log.txt"), d.join("ufb-log.prev.txt"));
    if path.exists() { let _ = std::fs::remove_file(&prev); let _ = std::fs::rename(&path, &prev); }
    let mut file = std::fs::File::create(&path).ok()?;
    let _ = writeln!(file, "UFB {} on {} - started {} (seconds since 1970), process {}", env!("CARGO_PKG_VERSION"), std::env::consts::OS, crate::format::now_secs(), std::process::id());
    #[cfg(unix)] { use std::os::fd::AsRawFd; if unsafe { libc::dup2(file.as_raw_fd(), 2) } < 0 { return None } }
    #[cfg(windows)] { use std::os::windows::io::AsRawHandle; use windows::Win32::System::Console::{SetStdHandle, STD_ERROR_HANDLE};
        if unsafe { SetStdHandle(STD_ERROR_HANDLE, windows::Win32::Foundation::HANDLE(file.as_raw_handle())) }.is_err() { return None }
        std::mem::forget(file); }   // (the handle is the error stream's now, for the life of the process)
    Some(path)
}
