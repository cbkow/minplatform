//! A file's bytes asked onto its storage before it is given its name - on storage that may not know the question.
//!
//! `File::sync_all` on macOS is `fcntl(F_FULLFSYNC)` and nothing else. LucidLink's filespace (served to the Mac as a
//! share from this machine) answers it "Operation not supported" (errno 45, measured 2026-11-18), and so every save
//! that synced its temporary file before the rename failed there: minNotes could not save a collection on LucidLink.
//! SQLite and git meet the same answer and fall back to the plain `fsync`; so does this. Storage that knows neither is
//! not a reason to refuse the write: the callers check what they wrote where it ends up.
use std::{fs::File, io};

/// The full sync; where the storage does not know it, the plain one; where it knows neither, nothing (Ok).
/// Any other failure is the caller's to report.
pub fn settle(f: &File) -> io::Result<()> {
    match f.sync_all() {
        Ok(()) => Ok(()),
        Err(e) if unknown(&e) => match plain(f) { Err(e2) if !unknown(&e2) => Err(e2), _ => Ok(()) },
        Err(e) => Err(e),
    }
}
/// "This storage does not do that": ENOTSUP / EOPNOTSUPP, ENOTTY, EINVAL on the Unixes; ERROR_INVALID_FUNCTION and
/// ERROR_NOT_SUPPORTED on Windows.
fn unknown(e: &io::Error) -> bool {
    if e.kind() == io::ErrorKind::Unsupported { return true }
    match e.raw_os_error() { Some(c) => if cfg!(windows) { matches!(c, 1 | 50) } else { matches!(c, 45 | 95 | 102 | 25 | 22) }, None => false }
}
#[cfg(unix)]
fn plain(f: &File) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    unsafe extern "C" { fn fsync(fd: i32) -> i32; }
    if unsafe { fsync(f.as_raw_fd()) } == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}
#[cfg(not(unix))]
fn plain(f: &File) -> io::Result<()> { f.sync_data() }

#[cfg(test)]
mod tests {
    #[test]
    fn a_local_file_settles() {
        let p = std::env::temp_dir().join(format!("min-paths-settle-{}", std::process::id()));
        let f = std::fs::File::create(&p).unwrap(); assert!(super::settle(&f).is_ok()); let _ = std::fs::remove_file(p);
    }
}
