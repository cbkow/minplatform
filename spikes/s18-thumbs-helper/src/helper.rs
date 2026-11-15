//! The helper: this executable started with `--helper`. It reads one request, does the work, writes one answer, for
//! ever; it ends when its standard input closes (the application is gone or wants it gone). It has no window, no
//! event loop and no run loop. Everything that reads a file's CONTENTS happens here.
use crate::proto::*;
use std::fs::File;
use std::io::Cursor;
use std::mem::ManuallyDrop;
use std::time::{Duration, Instant};

pub struct Img { pub w: u32, pub h: u32, pub rgba: Vec<u8> }

fn std_files() -> (ManuallyDrop<File>, ManuallyDrop<File>) {
    #[cfg(unix)] unsafe { use std::os::fd::FromRawFd; (ManuallyDrop::new(File::from_raw_fd(0)), ManuallyDrop::new(File::from_raw_fd(1))) }
    #[cfg(windows)] unsafe { use std::os::windows::io::{AsRawHandle, FromRawHandle};
        (ManuallyDrop::new(File::from_raw_handle(std::io::stdin().as_raw_handle())), ManuallyDrop::new(File::from_raw_handle(std::io::stdout().as_raw_handle()))) }
}

/// A crash ends the helper AT ONCE (no crash reporter, no report written): the parent sees the pipe close.
/// `S18_CRASH_REPORT=1` leaves the system's default in place, to measure what the reporter costs.
#[cfg(unix)]
fn crash_handlers() {
    extern "C" fn die(sig: i32) { unsafe { libc::_exit(128 + sig) } }
    if std::env::var_os("S18_CRASH_REPORT").is_some() { return }
    for s in [libc::SIGABRT, libc::SIGSEGV, libc::SIGBUS, libc::SIGILL, libc::SIGFPE, libc::SIGTRAP] { unsafe { libc::signal(s, die as extern "C" fn(i32) as usize); } }
}
#[cfg(windows)]
fn crash_handlers() {
    // UNTESTED. No error box, no Windows Error Reporting dialog: the process just ends.
    use windows::Win32::System::Diagnostics::Debug::*;
    unsafe { SetErrorMode(SEM_FAILCRITICALERRORS | SEM_NOGPFAULTERRORBOX | SEM_NOOPENFILEERRORBOX); }
}

/// If the application is killed while this helper is busy (or hung), nothing closes a pipe the helper is not reading.
/// Unix: the parent's id changes when it dies. (Windows: the parent puts its helpers in a job that dies with it.)
fn watch_parent() {
    #[cfg(unix)] { let parent = unsafe { libc::getppid() };
        std::thread::spawn(move || loop { std::thread::sleep(Duration::from_millis(500)); if unsafe { libc::getppid() } != parent { std::process::exit(0) } }); }
}

pub fn run() -> ! {
    crash_handlers();
    watch_parent();
    #[cfg(windows)] crate::sys::init();
    #[cfg(target_os = "macos")] crate::sys::prohibit();
    let (mut inp, mut out) = std_files();
    // the hello: the parent measures a start from spawn to this frame
    if write_response(&mut *out, &Response::default()).is_err() { std::process::exit(0) }
    loop {
        let Ok(req) = read_request(&mut *inp) else { std::process::exit(0) };
        let resp = handle(&req);
        if write_response(&mut *out, &resp).is_err() { std::process::exit(0) }
    }
}

fn handle(req: &Request) -> Response {
    let t = Instant::now();
    let mut r = Response { id: req.id, ..Default::default() };
    match req.op {
        OP_PING => {}
        OP_DECODE => match decode(&req.path, req.size as u32, req.fmt) {
            Ok((w, h, bytes, enc_us)) => { r.w = w as u16; r.h = h as u16; r.payload = bytes; r.aux = enc_us; r.work_us = (t.elapsed().as_micros() as u32).saturating_sub(enc_us); return r }
            Err(_) => r.status = FAILED,
        },
        OP_THUMB | OP_ICON | OP_TYPE_ICON | OP_QL_ICON => {
            let soft = Duration::from_millis(req.soft_ms.max(1) as u64);
            let res = match req.op {
                OP_THUMB => crate::sys::thumbnail(&req.path, req.size as u32, soft, false),
                OP_QL_ICON => crate::sys::thumbnail(&req.path, req.size as u32, soft, true),
                OP_ICON => crate::sys::icon(&req.path, req.size as u32),
                _ => crate::sys::type_icon(&req.path, req.size as u32),
            };
            match res {
                Ok(img) => { r.w = img.w as u16; r.h = img.h as u16; r.payload = img.rgba }
                Err((status, code)) => { r.status = status; r.aux = code as u32 }
            }
        }
        _ => r.status = FAILED,
    }
    r.work_us = t.elapsed().as_micros() as u32;
    r
}

/// Our own decoder's stand-in: read, decode, scale to fit `size`, and answer RGBA or an encoded JPEG.
/// The two poison names are the spike's damaged files.
pub fn decode(path: &str, size: u32, fmt: u8) -> Result<(u32, u32, Vec<u8>, u32), String> {
    if path.ends_with("abort.poison") { std::process::abort() }
    if path.ends_with("hang.poison") { let mut x = 0u64; loop { x = std::hint::black_box(x.wrapping_add(1)); } }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let img = image::load_from_memory(&bytes).map_err(|e| e.to_string())?;
    drop(bytes);
    let small = img.thumbnail(size, size);
    drop(img);
    if fmt == FMT_JPEG {
        let rgb = small.to_rgb8();
        let t = Instant::now();
        let mut out = Vec::with_capacity(64 << 10);
        image::codecs::jpeg::JpegEncoder::new_with_quality(Cursor::new(&mut out), 85)
            .encode(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8).map_err(|e| e.to_string())?;
        Ok((rgb.width(), rgb.height(), out, t.elapsed().as_micros() as u32))
    } else {
        let rgba = small.to_rgba8();
        Ok((rgba.width(), rgba.height(), rgba.into_raw(), 0))
    }
}
