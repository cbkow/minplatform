//! File operations (canon rows 436, 437, 455 a, 468): ONE engine of ours on both systems, on each system's own
//! per-file copy routine - macOS `copyfile` (a clone where the volume can, everything of the file kept), Windows
//! `CopyFileEx`. Step 3a: New Folder, Rename, Duplicate, Move to Trash, Delete Immediately, and Undo.
//!
//! What holds for every operation:
//! - it runs on the engine's own thread, never on a window's; a window hears of it by a [`Report`];
//! - nothing that exists is ever replaced: a rename or a finished copy takes its name only if the name is free, and
//!   the system itself is asked to refuse otherwise (no "look, then act");
//! - a copy is written under a partial name beside its destination and takes its real name when it is whole: what is
//!   interrupted leaves nothing with a real name;
//! - every operation says which folders it changed (row 438: the panes read those again - there is no watcher);
//! - Undo (row 468, point 4): one list for the application, for the session. Undoing a copy moves the copies to the
//!   Trash - never deletes; an item that changed since is left alone, and said.
//!
//! THE SAFETY BELT (row 468, point 2): until the owner's word, this build changes nothing outside the folders it was
//! started with ([`Guard`]). Everything here goes through it.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, OnceLock};

/// The folders this build may change things in.
#[derive(Clone, Debug, Default)]
pub struct Guard { roots: Vec<PathBuf> }
pub const GUARDED: &str = "This development build only changes files inside the folders it was started with.";
impl Guard {
    pub fn new(roots: &[PathBuf]) -> Guard { Guard { roots: roots.iter().filter_map(|r| std::fs::canonicalize(r).ok()).collect() } }
    /// `path` (it need not exist yet) lies INSIDE one of the folders - the folders themselves are not to be renamed or
    /// removed. Its enclosing folder is resolved, so that a link cannot lead outside.
    pub fn allows(&self, path: &Path) -> bool {
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else { return false };
        let Ok(parent) = std::fs::canonicalize(parent) else { return false }; let full = parent.join(name);
        self.roots.iter().any(|r| full.starts_with(r) && full != *r)
    }
    fn check(&self, path: &Path) -> Result<(), String> { if self.allows(path) { Ok(()) } else { Err(GUARDED.into()) } }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    NewFolder { dir: PathBuf },
    Rename { path: PathBuf, name: String },
    Duplicate { paths: Vec<PathBuf> },
    Trash { paths: Vec<PathBuf> },
    /// For good, at once: asked about before it is ever sent here.
    Delete { paths: Vec<PathBuf> },
    Undo,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind { NewFolder, Rename, Duplicate, Trash, Delete, Undo }

/// What an operation did. `origin`: the window that asked (it chooses what was made).
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub origin: u64, pub kind: Option<Kind>,
    /// the folders whose listing changed; what now exists because of it (to choose)
    pub changed: Vec<PathBuf>, pub made: Vec<PathBuf>,
    /// what went wrong, in plain words (some of it may have been done all the same)
    pub error: Option<String>,
    /// Move to Trash on a volume that has none: these are still there, and the window asks whether to delete them
    pub no_trash: Vec<PathBuf>,
}

/// (size, modification time) of what a copy made: undo leaves an item alone that is not as it was made.
type Stamp = (u64, Option<std::time::SystemTime>);
fn stamp(p: &Path) -> Option<Stamp> { let m = std::fs::symlink_metadata(p).ok()?; Some((if m.is_dir() { 0 } else { m.len() }, m.modified().ok())) }

enum Record { NewFolder(PathBuf), Rename { from: PathBuf, to: PathBuf }, Copies(Vec<(PathBuf, Stamp)>), Trash(Vec<(PathBuf, PathBuf)>) }
impl Record {
    fn label(&self) -> String {
        let items = |n: usize| if n == 1 { "1 Item".to_string() } else { format!("{n} Items") };
        match self { Record::NewFolder(_) => "New Folder".into(), Record::Rename { .. } => "Rename".into(), Record::Copies(v) => format!("Duplicate of {}", items(v.len())), Record::Trash(v) => format!("Move to Trash of {}", items(v.len())) }
    }
}

// ---------------------------------------------------------------- names

/// Each system's own word for a new folder, and for the second one.
pub fn new_folder_name(n: usize) -> String {
    if cfg!(windows) { if n <= 1 { "New folder".into() } else { format!("New folder ({n})") } } else if n <= 1 { "untitled folder".into() } else { format!("untitled folder {n}") }
}
/// Each system's own name for a copy beside its original: `plate copy.exr`, `plate copy 2.exr` (Finder);
/// `plate - Copy.exr`, `plate - Copy (2).exr` (Explorer). A folder has no extension to keep.
pub fn copy_name(name: &str, dir: bool, n: usize) -> String {
    let (stem, ext) = match name.rfind('.') { Some(i) if !dir && i > 0 && i + 1 < name.len() => (&name[..i], &name[i..]), _ => (name, "") };
    if cfg!(windows) { if n <= 1 { format!("{stem} - Copy{ext}") } else { format!("{stem} - Copy ({n}){ext}") } } else if n <= 1 { format!("{stem} copy{ext}") } else { format!("{stem} copy {n}{ext}") }
}
/// Why a name cannot be given, in the system's usual words; none: it can.
pub fn name_fault(name: &str) -> Option<String> {
    if name.is_empty() || name == "." || name == ".." { return Some("A name is needed.".into()) }
    let bad: &[char] = if cfg!(windows) { &['\\', '/', ':', '*', '?', '"', '<', '>', '|'] } else { &['/'] };
    if let Some(c) = name.chars().find(|c| bad.contains(c) || *c == '\0') { return Some(format!("A name cannot contain \u{201c}{c}\u{201d}.")) }
    if cfg!(target_os = "macos") && name.contains(':') { return Some("A name cannot contain \u{201c}:\u{201d}.".into()) }
    if cfg!(windows) && (name.ends_with('.') || name.ends_with(' ')) { return Some("A name cannot end with a dot or a space.".into()) }
    if name.len() > 255 { return Some("The name is too long.".into()) }
    None
}
pub fn taken(name: &str) -> String { format!("The name \u{201c}{name}\u{201d} is already taken. Please choose a different name.") }

fn plain(e: &std::io::Error) -> String { crate::lister::plain(e) }
fn name_of(p: &Path) -> String { p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default() }

// ---------------------------------------------------------------- the system's routines

/// `from` takes the name `to` ONLY if nothing has it: the system refuses otherwise (no look-then-act).
pub fn rename_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "macos")] {
        use std::os::unix::ffi::OsStrExt;
        let (a, b) = (std::ffi::CString::new(from.as_os_str().as_bytes())?, std::ffi::CString::new(to.as_os_str().as_bytes())?);
        if unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), libc::RENAME_EXCL) } == 0 { return Ok(()) }
        let e = std::io::Error::last_os_error();
        // a volume that cannot do it in one step (some shares): look, then rename - the best there is
        if e.raw_os_error() == Some(libc::ENOTSUP) { if std::fs::symlink_metadata(to).is_ok() { return Err(std::io::ErrorKind::AlreadyExists.into()) } return std::fs::rename(from, to) }
        Err(e)
    }
    #[cfg(windows)] {
        use windows::core::HSTRING; use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVE_FILE_FLAGS};
        unsafe { MoveFileExW(&HSTRING::from(from.as_os_str()), &HSTRING::from(to.as_os_str()), MOVE_FILE_FLAGS(0)) }.map_err(|e| std::io::Error::from_raw_os_error(e.code().0 & 0xffff))
    }
    #[cfg(not(any(target_os = "macos", windows)))] { if std::fs::symlink_metadata(to).is_ok() { return Err(std::io::ErrorKind::AlreadyExists.into()) } std::fs::rename(from, to) }
}

/// What a copy is told while it runs; set `cancel` to stop it (the partial file is removed).
pub struct Progress<'a> { pub cancel: &'a AtomicBool, pub bytes: &'a mut dyn FnMut(u64) }

/// One file (or link), by the system's own routine, to a name that must be free. Everything of the file is kept:
/// its times, its permissions, its extended attributes; on a volume that can, the copy is a clone.
fn copy_file(src: &Path, dst: &Path, pr: &mut Progress) -> std::io::Result<()> {
    // (a clone is done in one step and asks nobody: what was called off is not begun)
    if pr.cancel.load(Ordering::Relaxed) { return Err(std::io::ErrorKind::Interrupted.into()) }
    #[cfg(target_os = "macos")] {
        use std::os::unix::ffi::OsStrExt;
        extern "C" fn status(what: libc::c_int, stage: libc::c_int, state: libc::copyfile_state_t, _src: *const libc::c_char, _dst: *const libc::c_char, ctx: *mut libc::c_void) -> libc::c_int {
            let pr = unsafe { &mut *(ctx as *mut Progress) };
            if pr.cancel.load(Ordering::Relaxed) { return libc::COPYFILE_QUIT }
            if what == libc::COPYFILE_COPY_DATA && stage == libc::COPYFILE_PROGRESS { let mut done: libc::off_t = 0; unsafe { libc::copyfile_state_get(state, libc::COPYFILE_STATE_COPIED as u32, &mut done as *mut _ as *mut libc::c_void); } (pr.bytes)(done as u64); }
            libc::COPYFILE_CONTINUE
        }
        let (a, b) = (std::ffi::CString::new(src.as_os_str().as_bytes())?, std::ffi::CString::new(dst.as_os_str().as_bytes())?);
        unsafe {
            let state = libc::copyfile_state_alloc();
            let cb: libc::copyfile_callback_t = Some(status);
            libc::copyfile_state_set(state, libc::COPYFILE_STATE_STATUS_CB as u32, std::mem::transmute::<libc::copyfile_callback_t, *const libc::c_void>(cb));
            libc::copyfile_state_set(state, libc::COPYFILE_STATE_STATUS_CTX as u32, pr as *mut Progress as *const libc::c_void);
            let r = libc::copyfile(a.as_ptr(), b.as_ptr(), state, libc::COPYFILE_METADATA | libc::COPYFILE_DATA | libc::COPYFILE_CLONE | libc::COPYFILE_EXCL | libc::COPYFILE_NOFOLLOW);
            let e = std::io::Error::last_os_error(); libc::copyfile_state_free(state);
            if r == 0 { Ok(()) } else if pr.cancel.load(Ordering::Relaxed) { Err(std::io::ErrorKind::Interrupted.into()) } else { Err(e) }
        }
    }
    #[cfg(windows)] {
        use windows::core::HSTRING; use windows::Win32::Storage::FileSystem::{CopyFileExW, COPYFILE_FLAGS};
        unsafe extern "system" fn routine(_total: i64, done: i64, _st: i64, _sd: i64, _n: u32, _why: windows::Win32::Storage::FileSystem::LPPROGRESS_ROUTINE_CALLBACK_REASON, _s: windows::Win32::Foundation::HANDLE, _d: windows::Win32::Foundation::HANDLE, ctx: *const std::ffi::c_void) -> windows::Win32::Storage::FileSystem::COPYPROGRESSROUTINE_PROGRESS {
            use windows::Win32::Storage::FileSystem::COPYPROGRESSROUTINE_PROGRESS as P;
            let pr = unsafe { &mut *(ctx as *mut Progress) }; if pr.cancel.load(Ordering::Relaxed) { return P(1) }   // PROGRESS_CANCEL
            (pr.bytes)(done.max(0) as u64); P(0)   // PROGRESS_CONTINUE
        }
        // COPY_FILE_FAIL_IF_EXISTS | COPY_FILE_COPY_SYMLINK
        let r = unsafe { CopyFileExW(&HSTRING::from(src.as_os_str()), &HSTRING::from(dst.as_os_str()), Some(routine), Some(pr as *mut Progress as *const std::ffi::c_void), None, COPYFILE_FLAGS(0x1 | 0x800)) };
        r.map_err(|e| if pr.cancel.load(Ordering::Relaxed) { std::io::ErrorKind::Interrupted.into() } else { std::io::Error::from_raw_os_error(e.code().0 & 0xffff) })
    }
    #[cfg(not(any(target_os = "macos", windows)))] { if pr.cancel.load(Ordering::Relaxed) { return Err(std::io::ErrorKind::Interrupted.into()) } if std::fs::symlink_metadata(dst).is_ok() { return Err(std::io::ErrorKind::AlreadyExists.into()) } let n = std::fs::copy(src, dst)?; (pr.bytes)(n); Ok(()) }
}
/// A folder's own times and attributes onto its copy (after its entries are in: writing them changes its time).
fn copy_dir_meta(src: &Path, dst: &Path) {
    #[cfg(target_os = "macos")] { use std::os::unix::ffi::OsStrExt;
        if let (Ok(a), Ok(b)) = (std::ffi::CString::new(src.as_os_str().as_bytes()), std::ffi::CString::new(dst.as_os_str().as_bytes())) { unsafe { libc::copyfile(a.as_ptr(), b.as_ptr(), std::ptr::null_mut(), libc::COPYFILE_METADATA | libc::COPYFILE_STAT); } } }
    #[cfg(not(target_os = "macos"))] { if let (Ok(m), Ok(f)) = (std::fs::metadata(src), std::fs::File::open(dst)) { if let Ok(t) = m.modified() { let _ = f.set_modified(t); } } }
}
/// The name a file has while its copy is not whole.
fn partial(dst: &Path) -> PathBuf { dst.with_file_name(format!(".{}.ufb-partial", name_of(dst))) }

/// Copy a file, a link or a whole folder to `dst`, which must not exist. A file appears under its name only when it
/// is whole. On failure or cancel what this call made is removed again (it made all of it: `dst` was free).
pub fn copy_tree(src: &Path, dst: &Path, pr: &mut Progress) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(src)?;
    if !meta.is_dir() {
        let part = partial(dst); let _ = std::fs::remove_file(&part);
        let r = copy_file(src, &part, pr).and_then(|_| rename_no_replace(&part, dst));
        if r.is_err() { let _ = std::fs::remove_file(&part); }
        return r
    }
    std::fs::create_dir(dst)?;   // fails when the name is taken
    let r = (|| -> std::io::Result<()> {
        for e in std::fs::read_dir(src)? { let e = e?; if pr.cancel.load(Ordering::Relaxed) { return Err(std::io::ErrorKind::Interrupted.into()) } copy_tree(&e.path(), &dst.join(e.file_name()), pr)?; }
        Ok(())
    })();
    match r { Ok(()) => { copy_dir_meta(src, dst); Ok(()) } Err(e) => { let _ = std::fs::remove_dir_all(dst); Err(e) } }
}

/// Move to the Trash (the Recycle Bin). Ok: where the item is now, when the system says (macOS; it is what Undo
/// moves back). `Err(None)`: the volume has no trash - nothing was done.
fn trash(path: &Path) -> Result<Option<PathBuf>, Option<String>> {
    #[cfg(target_os = "macos")] {
        use objc2_foundation::{NSFileManager, NSString, NSURL};
        objc2::rc::autoreleasepool(|_| {
            let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy())); let mut out = None;
            match NSFileManager::defaultManager().trashItemAtURL_resultingItemURL_error(&url, Some(&mut out)) {
                Ok(()) => { let at = out.and_then(|u| u.path()).map(|p| PathBuf::from(p.to_string())); #[cfg(test)] if let Some(at) = &at { tests::TRASHED.with(|t| t.borrow_mut().push(at.clone())); } Ok(at) }
                // NSFeatureUnsupportedError: the volume has no trash
                Err(e) if e.code() == 3328 => Err(None),
                Err(e) => Err(Some(e.localizedDescription().to_string())),
            }
        })
    }
    #[cfg(windows)] {
        use windows::Win32::UI::Shell::{SHFileOperationW, SHFILEOPSTRUCTW};
        // the shell's own delete with "allow undo": the Recycle Bin, no question, no window of the shell's. (FO_DELETE = 3;
        // FOF_SILENT 0x4 | FOF_NOCONFIRMATION 0x10 | FOF_ALLOWUNDO 0x40 | FOF_NOERRORUI 0x400.) The shell takes an
        // absolute path with backslashes, never the `\\?\` form (S18), ended by two zeros.
        let s = path.to_string_lossy().replace('/', "\\"); let s = s.strip_prefix("\\\\?\\").unwrap_or(&s);
        let from: Vec<u16> = s.encode_utf16().chain([0, 0]).collect();
        let mut op = SHFILEOPSTRUCTW { wFunc: 3, pFrom: windows::core::PCWSTR(from.as_ptr()), fFlags: 0x4 | 0x10 | 0x40 | 0x400, ..Default::default() };
        let r = unsafe { SHFileOperationW(&mut op) };
        if r == 0 && !op.fAnyOperationsAborted.as_bool() { Ok(None) } else { Err(Some(format!("it could not be moved to the Recycle Bin (code {r})"))) }
    }
    #[cfg(not(any(target_os = "macos", windows)))] { let _ = path; Err(None) }
}

// ---------------------------------------------------------------- the operations

/// The engine's own state: the guard, and what can be undone (the newest last).
pub struct Engine { pub guard: Guard, undo: Vec<Record> }
impl Engine {
    pub fn new(guard: Guard) -> Engine { Engine { guard, undo: vec![] } }
    /// What Undo would undo, as the Edit menu names it.
    pub fn undo_label(&self) -> Option<String> { self.undo.last().map(|r| r.label()) }
    fn done(&mut self, r: Record) { self.undo.push(r); if self.undo.len() > 200 { self.undo.remove(0); } }

    pub fn run(&mut self, op: Op) -> Report {
        let cancel = AtomicBool::new(false); let mut none = |_: u64| {}; let mut pr = Progress { cancel: &cancel, bytes: &mut none };
        match op {
            Op::NewFolder { dir } => {
                let mut rep = Report { kind: Some(Kind::NewFolder), ..Default::default() };
                if let Err(e) = self.guard.check(&dir.join("x")) { rep.error = Some(e); return rep }
                for n in 1..10_000 {
                    let p = dir.join(new_folder_name(n));
                    match std::fs::create_dir(&p) {
                        Ok(()) => { rep.changed.push(dir.clone()); rep.made.push(p.clone()); self.done(Record::NewFolder(p)); return rep }
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                        Err(e) => { rep.error = Some(format!("The folder could not be made: {}.", plain(&e))); return rep }
                    }
                }
                rep.error = Some("The folder could not be made.".into()); rep
            }
            Op::Rename { path, name } => {
                let mut rep = Report { kind: Some(Kind::Rename), ..Default::default() };
                let old = name_of(&path); if name == old { return rep }
                if let Some(why) = name_fault(&name) { rep.error = Some(why); return rep }
                let to = path.with_file_name(&name);
                if let Err(e) = self.guard.check(&path).and_then(|_| self.guard.check(&to)) { rep.error = Some(e); return rep }
                // only the case of its letters changes: on a volume that does not tell them apart the name IS its own
                let same = old.to_lowercase() == name.to_lowercase();
                let r = if same { std::fs::rename(&path, &to) } else { rename_no_replace(&path, &to) };
                match r {
                    Ok(()) => { rep.changed.extend(path.parent().map(Path::to_path_buf)); rep.made.push(to.clone()); self.done(Record::Rename { from: path, to }); }
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => rep.error = Some(taken(&name)),
                    Err(e) => rep.error = Some(format!("\u{201c}{old}\u{201d} could not be renamed: {}.", plain(&e))),
                }
                rep
            }
            Op::Duplicate { paths } => {
                let mut rep = Report { kind: Some(Kind::Duplicate), ..Default::default() }; let mut made = vec![];
                for src in paths {
                    let (Some(dir), name) = (src.parent().map(Path::to_path_buf), name_of(&src)) else { continue };
                    if let Err(e) = self.guard.check(&src) { rep.error = Some(e); break }
                    let is_dir = std::fs::symlink_metadata(&src).map(|m| m.is_dir()).unwrap_or(false);
                    let mut r = Err(std::io::ErrorKind::AlreadyExists.into()); let mut dst = PathBuf::new();
                    for n in 1..10_000 { dst = dir.join(copy_name(&name, is_dir, n)); r = copy_tree(&src, &dst, &mut pr); if !matches!(&r, Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists) { break } }
                    match r {
                        Ok(()) => { if !rep.changed.contains(&dir) { rep.changed.push(dir); } if let Some(s) = stamp(&dst) { made.push((dst.clone(), s)); } rep.made.push(dst); }
                        Err(e) => { rep.error = Some(format!("\u{201c}{name}\u{201d} could not be duplicated: {}.", plain(&e))); break }
                    }
                }
                if !made.is_empty() { self.done(Record::Copies(made)); }
                rep
            }
            Op::Trash { paths } => {
                let mut rep = Report { kind: Some(Kind::Trash), ..Default::default() }; let mut moved = vec![];
                for p in paths {
                    if let Err(e) = self.guard.check(&p) { rep.error = Some(e); break }
                    match trash(&p) {
                        Ok(at) => { if let Some(dir) = p.parent() { if !rep.changed.iter().any(|c| c == dir) { rep.changed.push(dir.to_path_buf()); } } if let Some(at) = at { moved.push((p, at)); } }
                        Err(None) => rep.no_trash.push(p),
                        Err(Some(why)) => { rep.error = Some(format!("\u{201c}{}\u{201d} could not be moved to the Trash: {why}", name_of(&p))); break }
                    }
                }
                if !moved.is_empty() { self.done(Record::Trash(moved)); }
                rep
            }
            Op::Delete { paths } => {
                let mut rep = Report { kind: Some(Kind::Delete), ..Default::default() };
                for p in paths {
                    if let Err(e) = self.guard.check(&p) { rep.error = Some(e); break }
                    let r = match std::fs::symlink_metadata(&p) { Ok(m) if m.is_dir() => std::fs::remove_dir_all(&p), Ok(_) => std::fs::remove_file(&p), Err(e) => Err(e) };
                    match r { Ok(()) => { if let Some(dir) = p.parent() { if !rep.changed.iter().any(|c| c == dir) { rep.changed.push(dir.to_path_buf()); } } }
                        Err(e) => { rep.error = Some(format!("\u{201c}{}\u{201d} could not be deleted: {}.", name_of(&p), plain(&e))); break } }
                }
                rep
            }
            Op::Undo => self.undo_last(),
        }
    }

    fn undo_last(&mut self) -> Report {
        let mut rep = Report { kind: Some(Kind::Undo), ..Default::default() };
        let Some(rec) = self.undo.pop() else { return rep };
        let note = |rep: &mut Report, dir: Option<&Path>| { if let Some(d) = dir { if !rep.changed.iter().any(|c| c == d) { rep.changed.push(d.to_path_buf()); } } };
        match rec {
            // the folder goes to the Trash, whatever came into it since: nothing is deleted by an undo
            Record::NewFolder(p) => match trash(&p) { Ok(_) => note(&mut rep, p.parent()), Err(why) => rep.error = Some(format!("The new folder could not be taken back{}", why.map(|w| format!(": {w}")).unwrap_or_else(|| ": its volume has no Trash.".into()))) },
            Record::Rename { from, to } => match rename_no_replace(&to, &from) {
                Ok(()) => { note(&mut rep, from.parent()); rep.made.push(from); }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => rep.error = Some(format!("The rename could not be undone: \u{201c}{}\u{201d} is taken again.", name_of(&from))),
                Err(e) => rep.error = Some(format!("The rename could not be undone: {}.", plain(&e))),
            },
            Record::Copies(made) => { let mut left = 0;
                for (p, was) in made {
                    // changed since it was made (or gone): left alone
                    if stamp(&p) != Some(was) { if std::fs::symlink_metadata(&p).is_ok() { left += 1; } continue }
                    match trash(&p) { Ok(_) => note(&mut rep, p.parent()), Err(_) => left += 1 }
                }
                if left > 0 { rep.error = Some(if left == 1 { "One copy was changed since it was made and was left where it is.".into() } else { format!("{left} copies were changed since they were made and were left where they are.") }); } }
            Record::Trash(moved) => { let mut left = 0;
                for (orig, at) in moved { match rename_no_replace(&at, &orig) { Ok(()) => { note(&mut rep, orig.parent()); rep.made.push(orig); } Err(_) => left += 1 } }
                if left > 0 { rep.error = Some(if left == 1 { "One item could not be put back: its name is taken, or it is no longer in the Trash.".into() } else { format!("{left} items could not be put back: their names are taken, or they are no longer in the Trash.") }); } }
        }
        rep
    }
}

// ---------------------------------------------------------------- the application's one engine

struct Shared { tx: Sender<(u64, Op)>, undo: Arc<Mutex<Option<String>>>, guard: Guard, ports: Arc<Mutex<Vec<crate::lister::Port>>> }
static SHARED: OnceLock<Shared> = OnceLock::new();

/// Start the engine's thread. `roots`: the folders this build may change things in.
pub fn init(roots: &[PathBuf]) {
    let (tx, rx) = channel::<(u64, Op)>(); let undo = Arc::new(Mutex::new(None)); let ports: Arc<Mutex<Vec<crate::lister::Port>>> = Arc::new(Mutex::new(vec![])); let guard = Guard::new(roots);
    let (u, p, g) = (undo.clone(), ports.clone(), guard.clone());
    let _ = std::thread::Builder::new().name("ufb-ops".into()).spawn(move || {
        let mut engine = Engine::new(g);
        while let Ok((origin, op)) = rx.recv() {
            let mut rep = engine.run(op); rep.origin = origin; *u.lock().unwrap() = engine.undo_label();
            // every window hears of it (a folder may be shown in several)
            let rep = Arc::new(rep); for port in p.lock().unwrap().iter() { port.send(crate::lister::Msg::Op(rep.clone())); }
        }
    });
    let _ = SHARED.set(Shared { tx, undo, guard, ports });
}
/// A window's port: it hears of every operation from now on.
pub fn subscribe(port: crate::lister::Port) { if let Some(s) = SHARED.get() { s.ports.lock().unwrap().push(port); } }
pub fn submit(origin: u64, op: Op) { if let Some(s) = SHARED.get() { let _ = s.tx.send((origin, op)); } }
pub fn undo_label() -> Option<String> { SHARED.get().and_then(|s| s.undo.lock().unwrap().clone()) }
/// Whether this build may change things in `dir` (the menus say so before anything is tried).
pub fn allowed_in(dir: &Path) -> bool { SHARED.get().is_some_and(|s| s.guard.allows(&dir.join("x"))) }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lister::tests::Temp;
    fn engine(t: &Temp) -> Engine { Engine::new(Guard::new(&[t.0.clone()])) }
    fn names(dir: &Path) -> Vec<String> { let mut v: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect(); v.sort(); v }
    fn ok(r: Report) -> Report { assert_eq!(r.error, None); r }
    // what a test moved to the Trash is taken out of it again when the test ends: the owner's Trash is not a test's place
    thread_local! { pub static TRASHED: std::cell::RefCell<Vec<PathBuf>> = const { std::cell::RefCell::new(Vec::new()) }; }
    struct Sweep; impl Drop for Sweep { fn drop(&mut self) { TRASHED.with(|t| for p in t.borrow_mut().drain(..) { let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) }; }); } }

    #[test]
    fn the_names_each_system_gives() {
        #[cfg(not(windows))] { assert_eq!(new_folder_name(1), "untitled folder"); assert_eq!(new_folder_name(3), "untitled folder 3");
            assert_eq!(copy_name("plate.exr", false, 1), "plate copy.exr"); assert_eq!(copy_name("plate.exr", false, 2), "plate copy 2.exr"); assert_eq!(copy_name("renders.v2", true, 1), "renders.v2 copy"); assert_eq!(copy_name("README", false, 1), "README copy"); assert_eq!(copy_name(".env", false, 1), ".env copy"); }
        #[cfg(windows)] { assert_eq!(new_folder_name(2), "New folder (2)"); assert_eq!(copy_name("plate.exr", false, 2), "plate - Copy (2).exr"); }
        assert!(name_fault("").is_some() && name_fault("a/b").is_some() && name_fault("..").is_some()); assert_eq!(name_fault("shot_010 v2.exr"), None);
    }
    #[test]
    fn the_belt_holds_everything_to_the_folders_given() {
        let t = Temp::new("ops-guard"); t.dir("job"); t.file("job/a.txt", 1); t.file("outside.txt", 1);
        let g = Guard::new(&[t.0.join("job")]);
        assert!(g.allows(&t.0.join("job/a.txt")) && g.allows(&t.0.join("job/new-name")) && g.allows(&t.0.join("job/sub/../b")) == false, "a name inside is allowed; an enclosing folder that is not there is not");
        assert!(!g.allows(&t.0.join("outside.txt")) && !g.allows(&t.0.join("job")), "not beside the folder, and not the folder itself"); assert!(!g.allows(&t.0.join("job/../outside.txt")), "a way round is resolved");
        #[cfg(unix)] { std::os::unix::fs::symlink(&t.0, t.0.join("job/link")).unwrap(); assert!(!g.allows(&t.0.join("job/link/outside.txt")), "a link does not lead outside"); }
        let mut e = Engine::new(g);
        for op in [Op::Rename { path: t.0.join("outside.txt"), name: "x.txt".into() }, Op::Trash { paths: vec![t.0.join("outside.txt")] }, Op::Delete { paths: vec![t.0.join("outside.txt")] }, Op::Duplicate { paths: vec![t.0.join("outside.txt")] }, Op::NewFolder { dir: t.0.clone() }] {
            assert_eq!(e.run(op).error.as_deref(), Some(GUARDED)); }
        assert_eq!(names(&t.0), ["job", "outside.txt"], "nothing outside was touched"); assert!(e.undo_label().is_none());
        assert!(Engine::new(Guard::new(&[])).run(Op::NewFolder { dir: t.0.join("job") }).error.is_some(), "started with no folder: nothing may be changed");
    }
    #[test]
    fn a_new_folder_and_a_rename_and_their_undo() {
        let _sweep = Sweep;
        let t = Temp::new("ops-rename"); t.file("a.exr", 3); t.file("b.exr", 4); let mut e = engine(&t);
        let r = ok(e.run(Op::NewFolder { dir: t.0.clone() })); let first = r.made[0].clone(); assert_eq!(r.changed, [t.0.clone()]); assert!(first.is_dir());
        let second = ok(e.run(Op::NewFolder { dir: t.0.clone() })).made[0].clone(); assert_ne!(first, second); assert_eq!(name_of(&second), new_folder_name(2));
        assert_eq!(e.undo_label().as_deref(), Some("New Folder"));
        // a name that is taken is refused, and nothing changes; a free one is taken
        let r = e.run(Op::Rename { path: t.0.join("a.exr"), name: "b.exr".into() }); assert_eq!(r.error, Some(taken("b.exr"))); assert_eq!(std::fs::metadata(t.0.join("b.exr")).unwrap().len(), 4, "the other file is as it was");
        assert!(e.run(Op::Rename { path: t.0.join("a.exr"), name: "x/y".into() }).error.is_some());
        let r = ok(e.run(Op::Rename { path: t.0.join("a.exr"), name: "plate.exr".into() })); assert_eq!(r.made, [t.0.join("plate.exr")]); assert!(!t.0.join("a.exr").exists()); assert_eq!(e.undo_label().as_deref(), Some("Rename"));
        // only the case of its letters
        ok(e.run(Op::Rename { path: t.0.join("plate.exr"), name: "Plate.EXR".into() })); assert!(names(&t.0).contains(&"Plate.EXR".to_string()));
        // undo, newest first: the case, the rename, the second folder, the first
        ok(e.run(Op::Undo)); assert!(names(&t.0).contains(&"plate.exr".to_string()));
        let r = ok(e.run(Op::Undo)); assert_eq!(r.made, [t.0.join("a.exr")]); assert_eq!(std::fs::metadata(t.0.join("a.exr")).unwrap().len(), 3);
        // an undo whose old name is taken again says so and replaces nothing
        ok(e.run(Op::Rename { path: t.0.join("a.exr"), name: "c.exr".into() })); t.file("a.exr", 9);
        assert!(e.run(Op::Undo).error.is_some_and(|w| w.contains("taken again"))); assert_eq!(std::fs::metadata(t.0.join("a.exr")).unwrap().len(), 9); assert!(t.0.join("c.exr").exists());
        #[cfg(target_os = "macos")] { ok(e.run(Op::Undo)); assert!(!second.exists(), "the folder made last went to the Trash"); ok(e.run(Op::Undo)); assert!(!first.exists()); assert!(e.undo_label().is_none()); assert!(e.run(Op::Undo).changed.is_empty(), "nothing left to undo"); }
    }
    #[test]
    fn a_duplicate_is_whole_keeps_its_times_and_never_replaces() {
        let _sweep = Sweep;
        let t = Temp::new("ops-dup"); t.file("plate.exr", 5000); t.file("shots/010/comp.nk", 20); t.file("shots/010/renders/beauty.exr", 300); t.file("plate copy.exr", 1);
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        std::fs::File::options().write(true).open(t.0.join("plate.exr")).unwrap().set_modified(old).unwrap();
        let mut e = engine(&t);
        let r = ok(e.run(Op::Duplicate { paths: vec![t.0.join("plate.exr"), t.0.join("shots")] }));
        assert_eq!(r.made, [t.0.join(copy_name("plate.exr", false, 2)), t.0.join(copy_name("shots", true, 1))], "the first copy's name was taken: the second is given"); assert_eq!(std::fs::metadata(t.0.join("plate copy.exr")).map(|m| m.len()).unwrap_or(1), 1, "what was there is as it was");
        let copy = &r.made[0]; let m = std::fs::metadata(copy).unwrap(); assert_eq!(m.len(), 5000); assert_eq!(m.modified().unwrap(), old, "the copy keeps the file's time");
        assert_eq!(std::fs::metadata(r.made[1].join("010/renders/beauty.exr")).unwrap().len(), 300, "a folder is copied with everything in it");
        assert!(!names(&t.0).iter().any(|n| n.contains("ufb-partial")), "no partial file is left"); assert_eq!(e.undo_label().as_deref(), Some("Duplicate of 2 Items"));
        // undo: the copies go to the Trash - but one that was changed since stays, and that is said
        #[cfg(target_os = "macos")] {
            std::fs::write(copy, b"edited since").unwrap();
            let u = e.run(Op::Undo); assert!(u.error.is_some_and(|w| w.contains("left where it is"))); assert!(copy.exists(), "the changed copy stays"); assert!(!r.made[1].exists(), "the untouched one went to the Trash");
            assert!(t.0.join("plate.exr").exists() && t.0.join("shots/010/comp.nk").exists(), "the originals are untouched");
        }
        // a copy that is called off leaves nothing behind
        let cancel = AtomicBool::new(true); let mut none = |_: u64| {}; let dst = t.0.join("never.exr");
        assert!(copy_tree(&t.0.join("plate.exr"), &dst, &mut Progress { cancel: &cancel, bytes: &mut none }).is_err()); assert!(!dst.exists() && !partial(&dst).exists());
        let dst = t.0.join("never-folder"); assert!(copy_tree(&t.0.join("shots"), &dst, &mut Progress { cancel: &cancel, bytes: &mut none }).is_err()); assert!(!dst.exists());
        // ... and a copy onto a name that is taken is refused, the file there untouched
        let cancel = AtomicBool::new(false); assert_eq!(copy_tree(&t.0.join("plate.exr"), &t.0.join("plate copy.exr"), &mut Progress { cancel: &cancel, bytes: &mut none }).unwrap_err().kind(), std::io::ErrorKind::AlreadyExists); assert_eq!(std::fs::metadata(t.0.join("plate copy.exr")).unwrap().len(), 1);
    }
    #[cfg(target_os = "macos")]
    #[test]
    fn the_trash_and_back() {
        let _sweep = Sweep;
        let t = Temp::new("ops-trash"); t.file("a.exr", 3); t.file("folder/b.exr", 4); let mut e = engine(&t);
        let r = ok(e.run(Op::Trash { paths: vec![t.0.join("a.exr"), t.0.join("folder")] })); assert_eq!(r.changed, [t.0.clone()]); assert!(names(&t.0).is_empty()); assert_eq!(e.undo_label().as_deref(), Some("Move to Trash of 2 Items"));
        let r = ok(e.run(Op::Undo)); assert_eq!(names(&t.0), ["a.exr", "folder"]); assert_eq!(r.made.len(), 2); assert_eq!(std::fs::metadata(t.0.join("folder/b.exr")).unwrap().len(), 4);
        // put back onto a name that is taken again: refused, said, nothing replaced
        ok(e.run(Op::Trash { paths: vec![t.0.join("a.exr")] })); t.file("a.exr", 9);
        assert!(e.run(Op::Undo).error.is_some_and(|w| w.contains("could not be put back"))); assert_eq!(std::fs::metadata(t.0.join("a.exr")).unwrap().len(), 9);
        // for good
        ok(e.run(Op::Delete { paths: vec![t.0.join("a.exr"), t.0.join("folder")] })); assert!(names(&t.0).is_empty()); assert!(e.undo_label().is_none(), "what is deleted for good is not in the list");
    }
}
