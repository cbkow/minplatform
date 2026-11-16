//! Reading folders, never on the UI thread (canon row 463; a share can stall for seconds): one worker thread per
//! reading, its entries sent back in chunks - a folder of a hundred thousand entries shows its first rows at once -
//! and everything tagged with the request's number, so that what arrives for a reading nobody waits for any more is
//! dropped by the pane. A reading that is no longer wanted is told so and stops at its next entry; one that hangs
//! inside the system is left to end by itself.
//!
//! This slice only READS: nothing here creates, renames, moves or deletes anything.
use crate::model::RawEntry;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub enum Msg {
    Chunk { req: u64, entries: Vec<RawEntry> },
    /// The reading is over: the folder's own modification time, the room left on its volume.
    Done { req: u64, mtime: Option<i64>, free: Option<u64> },
    Failed { req: u64, why: String },
    /// A folder's modification time alone (asked when the window became active again).
    Probed { req: u64, mtime: Option<i64> },
    /// A typed path was looked at: is it a folder that can be gone to.
    Checked { req: u64, path: PathBuf, dir: bool },
    /// The volumes, and the startup disk's name.
    Places { volumes: Vec<crate::places::Place>, root: String },
    /// An icon came (see `icons.rs`); nothing to carry - the loop only has to draw again.
    Icon,
}

/// Where a worker sends, and how it wakes the window's loop.
#[derive(Clone)]
pub struct Port { pub tx: Sender<Msg>, pub wake: Arc<dyn Fn() + Send + Sync> }
impl Port { pub fn send(&self, m: Msg) { if self.tx.send(m).is_ok() { (self.wake)(); } } }

pub fn next_req() -> u64 { static N: AtomicU64 = AtomicU64::new(1); N.fetch_add(1, Ordering::Relaxed) }

/// Why a folder could not be read, in plain words.
pub fn plain(e: &std::io::Error) -> String {
    match e.kind() {
        std::io::ErrorKind::NotFound => "it does not exist".into(),
        std::io::ErrorKind::PermissionDenied => "permission denied".into(),
        std::io::ErrorKind::NotADirectory => "it is not a folder".into(),
        std::io::ErrorKind::TimedOut => "it did not answer in time".into(),
        _ => { let s = e.to_string(); let s = s.split(" (os error").next().unwrap_or(&s).trim().to_string(); let mut c = s.chars(); match c.next() { Some(f) => f.to_lowercase().collect::<String>() + c.as_str(), None => "unknown error".into() } }
    }
}
fn secs(t: std::io::Result<std::time::SystemTime>) -> Option<i64> { let t = t.ok()?; Some(match t.duration_since(std::time::UNIX_EPOCH) { Ok(d) => d.as_secs() as i64, Err(e) => -(e.duration().as_secs() as i64) }) }
/// A folder's own modification time (it changes when an entry is added, removed or renamed in it).
pub fn dir_mtime(path: &Path) -> Option<i64> { secs(std::fs::metadata(path).and_then(|m| m.modified())) }

#[cfg(target_os = "macos")]
fn hidden(name: &str, md: &std::fs::Metadata) -> bool { use std::os::macos::fs::MetadataExt; name.starts_with('.') || md.st_flags() & 0x8000 != 0 }   // UF_HIDDEN: what Finder hides without a dot
#[cfg(all(unix, not(target_os = "macos")))]
fn hidden(name: &str, _md: &std::fs::Metadata) -> bool { name.starts_with('.') }
#[cfg(windows)]
fn hidden(_name: &str, md: &std::fs::Metadata) -> bool { use std::os::windows::fs::MetadataExt; md.file_attributes() & 0x2 != 0 }   // FILE_ATTRIBUTE_HIDDEN

/// A folder that the system's own file browser shows as one item (macOS: an application, a bundle, a document
/// package). The system is asked - only about folders whose name has an extension, the others are never packages
/// in practice (a bare folder with the bundle bit set stays a folder here).
#[cfg(target_os = "macos")]
fn is_package(name: &str, path: &Path) -> bool {
    if crate::format::extension(name).is_none() { return false }
    let Some(p) = path.to_str() else { return false };
    objc2::rc::autoreleasepool(|_| objc2_app_kit::NSWorkspace::sharedWorkspace().isFilePackageAtPath(&objc2_foundation::NSString::from_str(p)))
}
#[cfg(not(target_os = "macos"))]
fn is_package(_name: &str, _path: &Path) -> bool { false }

fn entry(e: &std::fs::DirEntry) -> Option<RawEntry> {
    let name = e.file_name().to_string_lossy().into_owned();
    let md = e.metadata().ok();   // the link itself, not what it points to
    let is_hidden = md.as_ref().map(|m| hidden(&name, m)).unwrap_or(name.starts_with('.'));
    // a link to a folder is gone into as a folder
    let md = if md.as_ref().is_some_and(|m| m.file_type().is_symlink()) { std::fs::metadata(e.path()).ok().or(md) } else { md };
    let (dir, size, mtime) = match &md { Some(m) => (m.is_dir(), if m.is_dir() { 0 } else { m.len() }, secs(m.modified()).unwrap_or(i64::MIN)), None => (false, 0, i64::MIN) };
    // a folder the system shows as ONE item (an application, a library): an entry like a file - not gone into, not opened in place
    if dir && is_package(&name, &e.path()) { return Some(RawEntry::new(name, 0, mtime, false, is_hidden).package()) }
    Some(RawEntry::new(name, size, mtime, dir, is_hidden))
}

/// Read a folder on the calling thread, handing its entries to `chunk` as they come: the first few soon, then in
/// larger lots. `cancel` set: the reading stops. (The worker's body; the tests call it directly.)
pub fn read(path: &Path, cancel: &AtomicBool, mut chunk: impl FnMut(Vec<RawEntry>)) -> Result<(), String> {
    let rd = std::fs::read_dir(path).map_err(|e| plain(&e))?;
    let (mut buf, mut first, mut last) = (Vec::new(), true, Instant::now());
    for e in rd {
        if cancel.load(Ordering::Relaxed) { return Ok(()) }
        let Ok(e) = e else { continue };
        if let Some(r) = entry(&e) { buf.push(r); }
        let (n, wait) = if first { (256, Duration::from_millis(40)) } else { (8192, Duration::from_millis(150)) };
        if buf.len() >= n || (!buf.is_empty() && last.elapsed() >= wait) { chunk(std::mem::take(&mut buf)); first = false; last = Instant::now(); }
    }
    if !buf.is_empty() { chunk(buf); }
    Ok(())
}

/// The room left on the volume that holds `path`.
pub fn free_space(path: &Path) -> Option<u64> {
    #[cfg(unix)] { use std::os::unix::ffi::OsStrExt; let c = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?; let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 { return None } Some(s.f_bavail as u64 * s.f_frsize as u64) }
    #[cfg(windows)] { use windows::core::HSTRING; let mut free = 0u64; unsafe { windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(&HSTRING::from(path.as_os_str()), Some(&mut free), None, None).ok()?; } Some(free) }
    #[cfg(not(any(unix, windows)))] { let _ = path; None }
}

/// Start reading `path` for request `req`.
pub fn list(port: &Port, req: u64, path: PathBuf, cancel: Arc<AtomicBool>) {
    let port = port.clone();
    let _ = std::thread::Builder::new().name("ufb-list".into()).spawn(move || {
        match read(&path, &cancel, |entries| port.send(Msg::Chunk { req, entries })) {
            Ok(()) => { if !cancel.load(Ordering::Relaxed) { port.send(Msg::Done { req, mtime: dir_mtime(&path), free: free_space(&path) }); } }
            Err(why) => port.send(Msg::Failed { req, why }),
        }
    });
}
/// Ask for a folder's modification time.
pub fn probe(port: &Port, req: u64, path: PathBuf) { let port = port.clone(); let _ = std::thread::Builder::new().name("ufb-probe".into()).spawn(move || port.send(Msg::Probed { req, mtime: dir_mtime(&path) })); }
/// Ask whether a typed path is a folder.
pub fn check(port: &Port, req: u64, path: PathBuf) { let port = port.clone(); let _ = std::thread::Builder::new().name("ufb-check".into()).spawn(move || { let dir = path.is_dir(); port.send(Msg::Checked { req, path, dir }); }); }

#[cfg(test)]
pub mod tests {
    use super::*;
    /// A folder of the test's own under the system's temporary folder, removed when it is dropped.
    pub struct Temp(pub PathBuf);
    impl Temp {
        pub fn new(tag: &str) -> Temp { let p = std::env::temp_dir().join(format!("ufb-test-{tag}-{}-{}", std::process::id(), next_req())); let _ = std::fs::remove_dir_all(&p); std::fs::create_dir_all(&p).unwrap(); Temp(p) }
        pub fn file(&self, rel: &str, bytes: usize) { let p = self.0.join(rel); std::fs::create_dir_all(p.parent().unwrap()).unwrap(); std::fs::write(p, vec![b'x'; bytes]).unwrap(); }
        pub fn dir(&self, rel: &str) { std::fs::create_dir_all(self.0.join(rel)).unwrap(); }
    }
    impl Drop for Temp { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
    pub fn read_all(path: &Path) -> Result<Vec<RawEntry>, String> { let mut all = vec![]; read(path, &AtomicBool::new(false), |c| all.extend(c))?; Ok(all) }

    #[test]
    fn a_folder_is_read_with_sizes_folders_and_hidden_entries() {
        let t = Temp::new("read"); t.file("plate.mov", 5000); t.file("shot_010_comp_v003.exr", 12); t.file(".hidden_notes", 3); t.dir("renders"); t.file("renders/beauty.exr", 1);
        let mut all = read_all(&t.0).unwrap(); all.sort_by(|a, b| a.name.cmp(&b.name));
        let got: Vec<(&str, u64, bool, bool)> = all.iter().map(|e| (e.name.as_str(), e.size, e.dir, e.hidden)).collect();
        #[cfg(unix)] assert_eq!(got, [(".hidden_notes", 3, false, true), ("plate.mov", 5000, false, false), ("renders", 0, true, false), ("shot_010_comp_v003.exr", 12, false, false)]);
        #[cfg(windows)] assert_eq!(got.len(), 4);
        let now = crate::format::now_secs(); assert!(all.iter().all(|e| (e.mtime - now).abs() < 3600), "modification times are of now");
        assert!(dir_mtime(&t.0).is_some()); assert!(free_space(&t.0).is_some_and(|f| f > 0));
    }
    /// macOS: a folder the system shows as one item is one entry here - not a folder, no size of its own.
    #[cfg(target_os = "macos")]
    #[test]
    fn an_application_is_one_entry() {
        let all = read_all(Path::new("/System/Applications")).unwrap();
        let calc = all.iter().find(|e| e.name == "Calculator.app").expect("the system's calculator"); assert!(calc.package && !calc.dir && calc.size == 0);
        let util = all.iter().find(|e| e.name == "Utilities").expect("the utilities folder"); assert!(util.dir && !util.package, "a plain folder stays one");
        // a folder that only LOOKS like a type is asked about, and is a folder
        let t = Temp::new("pkg"); t.dir("renders.v002"); t.dir("shots.exr"); let all = read_all(&t.0).unwrap(); assert!(all.iter().all(|e| e.dir && !e.package));
    }
    #[test]
    fn a_folder_that_cannot_be_read_says_why() {
        let t = Temp::new("missing");
        assert_eq!(read_all(&t.0.join("not-there")).unwrap_err(), "it does not exist");
        t.file("a-file", 1); assert!(read_all(&t.0.join("a-file")).is_err(), "a file is not a folder");
        assert_eq!(plain(&std::io::Error::from(std::io::ErrorKind::PermissionDenied)), "permission denied");
    }
    #[test]
    fn many_entries_come_in_chunks_and_a_cancelled_reading_stops() {
        let t = Temp::new("chunks"); for i in 0..1000 { t.file(&format!("frame_{i:04}.exr"), 0); }
        let mut sizes = vec![]; read(&t.0, &AtomicBool::new(false), |c| sizes.push(c.len())).unwrap();
        assert_eq!(sizes.iter().sum::<usize>(), 1000); assert!(sizes.len() >= 2 && sizes[0] <= 256, "the first rows come before the rest: {sizes:?}");
        let cancel = AtomicBool::new(false); let mut seen = 0; read(&t.0, &cancel, |c| { seen += c.len(); cancel.store(true, Ordering::Relaxed); }).unwrap();
        assert!(seen < 1000, "nothing more after the reading was called off ({seen})");
    }
    #[test]
    fn the_worker_delivers_through_the_port_and_wakes_the_loop() {
        let t = Temp::new("port"); for i in 0..300 { t.file(&format!("f{i}.txt"), 1); }
        let (tx, rx) = std::sync::mpsc::channel(); let woken = Arc::new(AtomicU64::new(0)); let w = woken.clone();
        let port = Port { tx, wake: Arc::new(move || { w.fetch_add(1, Ordering::Relaxed); }) };
        list(&port, 7, t.0.clone(), Arc::new(AtomicBool::new(false)));
        let (mut n, mut done) = (0, false);
        while let Ok(m) = rx.recv_timeout(Duration::from_secs(20)) { match m { Msg::Chunk { req, entries } => { assert_eq!(req, 7); n += entries.len(); } Msg::Done { req, mtime, free } => { assert_eq!(req, 7); assert!(mtime.is_some() && free.is_some()); done = true; break } _ => panic!("unexpected") } }
        assert!(done && n == 300); assert!(woken.load(Ordering::Relaxed) >= 2);
        list(&port, 8, t.0.join("gone"), Arc::new(AtomicBool::new(false)));
        assert!(matches!(rx.recv_timeout(Duration::from_secs(20)), Ok(Msg::Failed { req: 8, .. })));
        check(&port, 9, t.0.clone()); assert!(matches!(rx.recv_timeout(Duration::from_secs(20)), Ok(Msg::Checked { req: 9, dir: true, .. })));
        probe(&port, 10, t.0.clone()); assert!(matches!(rx.recv_timeout(Duration::from_secs(20)), Ok(Msg::Probed { req: 10, mtime: Some(_) })));
    }
}
