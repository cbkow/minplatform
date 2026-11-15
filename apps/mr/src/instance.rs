//! One instance for each data folder (QCView's `single_instance.rs`, canon row 143, with minRender's rule of row 389:
//! a second launch that was started minimized does not show the first one's window). A local socket: a file in the
//! per-user temporary folder on macOS, a named pipe on Windows; a socket left by a killed instance is taken over.
#[cfg(unix)] use interprocess::local_socket::GenericFilePath;
#[cfg(not(unix))] use interprocess::local_socket::GenericNamespaced;
use interprocess::local_socket::{prelude::*, ListenerOptions, Name, Stream};
use std::io::{Read, Write};

/// A short name for a data folder: its path, hashed.
fn key(data: &std::path::Path) -> String { use sha2::Digest; let p = std::fs::canonicalize(data).unwrap_or_else(|_| data.to_path_buf()); crate::share::hex(&sha2::Sha256::digest(p.to_string_lossy().as_bytes())[..6]) }
#[cfg(unix)] fn file(data: &std::path::Path) -> std::path::PathBuf { std::env::temp_dir().join(format!("minrender-{}.sock", key(data))) }
fn name(data: &std::path::Path) -> std::io::Result<Name<'static>> {
    #[cfg(unix)] { file(data).to_fs_name::<GenericFilePath>().map(|x| x.into_owned()) }
    #[cfg(not(unix))] { format!("minrender-{}", key(data)).to_ns_name::<GenericNamespaced>().map(|x| x.into_owned()) }
}
/// A later launch: true when a running instance took the word (this process then leaves).
pub fn hand_over(data: &std::path::Path, minimized: bool) -> bool {
    let Ok(n) = name(data) else { return false }; let Ok(mut s) = Stream::connect(n) else { return false };
    // (Windows gives the foreground only to a process allowed to take it: the first instance is, by this call)
    #[cfg(windows)] unsafe { let _ = windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(u32::MAX); }
    s.write_all(if minimized { b"minimized" } else { b"show" }).and_then(|_| s.flush()).is_ok()
}
/// The first launch: `said(minimized)` is called for each later launch.
pub fn listen(data: &std::path::Path, said: impl Fn(bool) + Send + 'static) -> bool {
    let Ok(n) = name(data) else { return false };
    let listener = match ListenerOptions::new().name(n.clone()).create_sync() { Ok(l) => l, Err(_) => {
        if Stream::connect(n.clone()).is_ok() { return false }
        #[cfg(unix)] { let _ = std::fs::remove_file(file(data)); }
        match ListenerOptions::new().name(n).create_sync() { Ok(l) => l, Err(_) => return false } } };
    std::thread::spawn(move || for mut c in listener.incoming().flatten() { let mut b = [0u8; 16]; let n = c.read(&mut b).unwrap_or(0); said(&b[..n] == b"minimized"); });
    true
}
