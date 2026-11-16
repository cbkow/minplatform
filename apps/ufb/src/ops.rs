//! File operations (canon rows 436, 437, 455 a, 468): ONE engine of ours on both systems, on each system's own
//! per-file copy routine - macOS `copyfile` (a clone where the volume can, everything of the file kept), Windows
//! `CopyFileEx`. Step 3a: New Folder, Rename, Duplicate, Move to Trash, Delete Immediately, and Undo. Step 3b: copy and
//! move into another folder ([`Op::Transfer`]) - a name that is taken is ASKED about (replace, skip, keep both; for all),
//! two folders of one name MERGE (row 468, point 5), a move on one volume is a rename, a move into its own subfolder is
//! refused, progress is told and the whole can be called off (what is done stays done, and can be undone).
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
    /// Duplicate an image sequence AS a sequence: every frame is copied beside itself under the copy's name of the
    /// sequence's BASE, its frame number kept (`shot_010 copy.1001.exr`, not `shot_010.1001 copy.exr`). Each frame:
    /// (the file, the base, what follows the base in its name - separator, number, extension).
    DuplicateSequence { frames: Vec<(PathBuf, String, String)> },
    Trash { paths: Vec<PathBuf> },
    /// For good, at once: asked about before it is ever sent here.
    Delete { paths: Vec<PathBuf> },
    /// Copy (or move) these INTO the folder `dest`.
    Transfer { paths: Vec<PathBuf>, dest: PathBuf, mv: bool },
    Undo,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind { NewFolder, Rename, Duplicate, Trash, Delete, Undo, Copy, Move }

/// What to do about a name that is taken. `Stop`: nothing more of this operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice { Replace, Skip, KeepBoth, Stop }
/// A name that is taken, as it is asked about: the item, the folder it goes into, whether more of them may follow.
#[derive(Clone, Debug, PartialEq)]
pub struct Conflict { pub name: String, pub dest: PathBuf, pub mv: bool }
/// How a running operation stands, for the status bar.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status { pub label: String, pub items: (u64, u64), pub bytes: (u64, u64), pub current: String }
/// What a running operation is given: how to call it off, where to say how it stands, whom to ask.
pub struct Ctl<'a> { pub cancel: &'a AtomicBool, pub status: &'a Mutex<Option<Status>>, pub ask: &'a mut dyn FnMut(&Conflict) -> (Choice, bool) }

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

enum Record { NewFolder(PathBuf), Rename { from: PathBuf, to: PathBuf }, Copies(Vec<(PathBuf, Stamp)>), Trash(Vec<(PathBuf, PathBuf)>),
    /// a copy or a move into a folder: what was copied; what was moved (from, to); what was replaced (where it was, where in the Trash it is)
    Transfer { mv: bool, copied: Vec<(PathBuf, Stamp)>, moved: Vec<(PathBuf, PathBuf)>, replaced: Vec<(PathBuf, PathBuf)> } }
impl Record {
    fn label(&self) -> String {
        let items = |n: usize| if n == 1 { "1 Item".to_string() } else { format!("{n} Items") };
        match self { Record::NewFolder(_) => "New Folder".into(), Record::Rename { .. } => "Rename".into(), Record::Copies(v) => format!("Duplicate of {}", items(v.len())), Record::Trash(v) => format!("Move to Trash of {}", items(v.len())),
            Record::Transfer { mv, copied, moved, .. } => format!("{} of {}", if *mv { "Move" } else { "Copy" }, items(copied.len() + moved.len())) }
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
/// The name of the second of two that are both kept: `plate 2.exr` (Finder), `plate (2).exr` (Explorer).
pub fn keep_both_name(name: &str, dir: bool, n: usize) -> String {
    let (stem, ext) = match name.rfind('.') { Some(i) if !dir && i > 0 && i + 1 < name.len() => (&name[..i], &name[i..]), _ => (name, "") };
    if cfg!(windows) { format!("{stem} ({n}){ext}") } else { format!("{stem} {n}{ext}") }
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
        // (row 468, point 6: the system's routine is trusted; the sizes are compared)
        let whole = |_: ()| if meta.is_file() && std::fs::symlink_metadata(&part).map(|m| m.len()).ok() != Some(meta.len()) { Err(std::io::Error::other("the copy is not the size of the original")) } else { Ok(()) };
        let r = copy_file(src, &part, pr).and_then(whole).and_then(|_| rename_no_replace(&part, dst));
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
        // (two items of one name sent to the Trash at the same moment - by another application, another thread - can
        // collide there: the system refuses one; asked again, it finds it a free name)
        let once = || objc2::rc::autoreleasepool(|_| {
            let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy())); let mut out = None;
            match NSFileManager::defaultManager().trashItemAtURL_resultingItemURL_error(&url, Some(&mut out)) {
                Ok(()) => { let at = out.and_then(|u| u.path()).map(|p| PathBuf::from(p.to_string())); #[cfg(test)] if let Some(at) = &at { tests::TRASHED.with(|t| t.borrow_mut().push(at.clone())); } Ok(at) }
                // NSFeatureUnsupportedError: the volume has no trash
                Err(e) if e.code() == 3328 => Err(None),
                Err(e) => Err(Some(e.localizedDescription().to_string())),
            }
        });
        let mut r = once(); for _ in 0..3 { if !matches!(r, Err(Some(_))) || std::fs::symlink_metadata(path).is_err() { break } std::thread::sleep(std::time::Duration::from_millis(40)); r = once(); }
        r
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

/// How many items and bytes a copy of `p` is (links are not followed).
fn measure(p: &Path, cancel: &AtomicBool, items: &mut u64, bytes: &mut u64) {
    if cancel.load(Ordering::Relaxed) { return }
    let Ok(m) = std::fs::symlink_metadata(p) else { return }; *items += 1;
    if m.is_dir() { if let Ok(rd) = std::fs::read_dir(p) { for e in rd.flatten() { measure(&e.path(), cancel, items, bytes); } } } else { *bytes += m.len(); }
}
/// The volume is another one: a rename cannot do it.
fn other_volume(e: &std::io::Error) -> bool { #[cfg(unix)] { e.raw_os_error() == Some(libc::EXDEV) } #[cfg(windows)] { e.raw_os_error() == Some(17) } #[cfg(not(any(unix, windows)))] { let _ = e; false } }
/// Move one item to a name that is free: a rename on one volume; across volumes a whole copy, and only then the
/// original goes.
fn move_one(from: &Path, to: &Path, pr: &mut Progress) -> std::io::Result<()> {
    match rename_no_replace(from, to) {
        Err(e) if other_volume(&e) => { copy_tree(from, to, pr)?; if std::fs::symlink_metadata(from)?.is_dir() { std::fs::remove_dir_all(from) } else { std::fs::remove_file(from) } }
        r => r,
    }
}

/// A copy or a move while it runs.
struct Run<'a> { mv: bool, cancel: &'a AtomicBool, status: &'a Mutex<Option<Status>>, ask: &'a mut dyn FnMut(&Conflict) -> (Choice, bool), all: Option<Choice>, stopped: bool,
    items: (u64, u64), bytes: (u64, u64), label: String,
    copied: Vec<(PathBuf, Stamp)>, moved: Vec<(PathBuf, PathBuf)>, replaced: Vec<(PathBuf, PathBuf)>, changed: Vec<PathBuf>, made: Vec<PathBuf> }
impl Run<'_> {
    fn note(&mut self, dir: &Path) { if !self.changed.iter().any(|c| c == dir) { self.changed.push(dir.to_path_buf()); } }
    fn say(&self, current: &str, more: u64) { *self.status.lock().unwrap() = Some(Status { label: self.label.clone(), items: self.items, bytes: (self.bytes.0 + more, self.bytes.1), current: current.to_string() }); }
    /// The first name of the kind `name_of(n)` that nothing in `dir` has.
    fn free(dir: &Path, make: impl Fn(usize) -> String, from: usize) -> PathBuf { (from..100_000).map(|n| dir.join(make(n))).find(|p| std::fs::symlink_metadata(p).is_err()).unwrap_or_else(|| dir.join(make(0))) }

    /// One item into `dir`. `top`: it is one of those asked for (not something inside a folder being merged).
    fn item(&mut self, src: &Path, dir: &Path, top: bool) -> std::io::Result<()> {
        if self.cancel.load(Ordering::Relaxed) { return Err(std::io::ErrorKind::Interrupted.into()) }
        let name = name_of(src); let sm = std::fs::symlink_metadata(src)?; let mut target = dir.join(&name); let mut in_place = false;
        let here = src.parent().is_some_and(|p| p == dir);
        // into the folder it is already in: a move is nothing; a copy is a duplicate, under the system's name for one
        if here { if self.mv { return Ok(()) } target = Self::free(dir, |n| copy_name(&name, sm.is_dir(), n), 1); }
        else if let Ok(tm) = std::fs::symlink_metadata(&target) {
            // two folders of one name: they merge - what is inside goes by the same rules (row 468, point 5)
            if sm.is_dir() && tm.is_dir() {
                self.items.0 += 1;
                for e in std::fs::read_dir(src)? { let e = e?; if self.stopped { return Ok(()) } self.item(&e.path(), &target, false)?; }
                if self.mv { let _ = std::fs::remove_dir(src); if let Some(p) = src.parent() { self.note(p); } }   // (it goes only when nothing was left in it)
                self.note(dir); if top { self.made.push(target); }
                return Ok(())
            }
            let (choice, all) = match self.all { Some(c) => (c, true), None => (self.ask)(&Conflict { name: name.clone(), dest: dir.to_path_buf(), mv: self.mv }) };
            if all && choice != Choice::Stop { self.all = Some(choice); }
            match choice {
                Choice::Stop => { self.stopped = true; return Ok(()) }
                Choice::Skip => { let (mut n, mut b) = (0, 0); measure(src, self.cancel, &mut n, &mut b); self.items.0 += n; self.bytes.0 += b; return Ok(()) }
                Choice::KeepBoth => target = Self::free(dir, |n| keep_both_name(&name, sm.is_dir(), n), 2),
                // what is replaced goes to the Trash where there is one (Undo brings it back); where there is none the
                // new one takes its place only when it is whole
                Choice::Replace => match trash(&target) { Ok(Some(at)) => self.replaced.push((target.clone(), at)), Ok(None) => {} Err(None) => in_place = true, Err(Some(why)) => return Err(std::io::Error::other(why)) },
            }
        }
        self.say(&name, 0);
        let (mut n, mut size) = (0, 0); measure(src, self.cancel, &mut n, &mut size);
        if self.mv {
            // a move on one volume is a rename
            let direct = if in_place { std::fs::rename(src, &target) } else { rename_no_replace(src, &target) };
            match direct {
                Ok(()) => { self.moved.push((src.to_path_buf(), target.clone())); self.items.0 += n; self.bytes.0 += size; self.note(dir); if let Some(p) = src.parent() { self.note(p); } if top { self.made.push(target); } return Ok(()) }
                Err(e) if other_volume(&e) => {}
                Err(e) => return Err(e),
            }
        }
        // a copy (or a move to another volume: the copy first, whole; only then the original goes)
        let (status, label, items, base, total) = (self.status, self.label.clone(), self.items, self.bytes.0, self.bytes.1); let cur = name.clone();
        let mut told = std::time::Instant::now() - std::time::Duration::from_secs(1); let mut within = 0u64; let mut last = 0u64;
        let mut bytes = |file_done: u64| { if file_done < last { within += last; } last = file_done;   // (a folder's files come one after another)
            if told.elapsed() >= std::time::Duration::from_millis(60) { told = std::time::Instant::now(); *status.lock().unwrap() = Some(Status { label: label.clone(), items, bytes: (base + within + file_done, total), current: cur.clone() }); } };
        let mut pr = Progress { cancel: self.cancel, bytes: &mut bytes };
        if in_place {
            // no Trash on this volume: a file takes a file's place in one step, once whole; anything else is cleared away first
            let tm = std::fs::symlink_metadata(&target)?;
            if sm.is_file() && tm.is_file() { let part = partial(&target); let _ = std::fs::remove_file(&part);
                let r = copy_file(src, &part, &mut pr).and_then(|_| std::fs::rename(&part, &target)); if r.is_err() { let _ = std::fs::remove_file(&part); } r?; }
            else { if tm.is_dir() { std::fs::remove_dir_all(&target)?; } else { std::fs::remove_file(&target)?; } copy_tree(src, &target, &mut pr)?; }
        } else { copy_tree(src, &target, &mut pr)?; }
        self.items.0 += n; self.bytes.0 += size; self.note(dir);
        if self.mv { if sm.is_dir() { std::fs::remove_dir_all(src)?; } else { std::fs::remove_file(src)?; } self.moved.push((src.to_path_buf(), target.clone())); if let Some(p) = src.parent() { self.note(p); } }
        else if let Some(s) = stamp(&target) { self.copied.push((target.clone(), s)); }
        if top { self.made.push(target); }
        Ok(())
    }
}

// ---------------------------------------------------------------- the operations

/// The engine's own state: the guard, and what can be undone (the newest last).
pub struct Engine { pub guard: Guard, undo: Vec<Record> }
impl Engine {
    pub fn new(guard: Guard) -> Engine { Engine { guard, undo: vec![] } }
    /// What Undo would undo, as the Edit menu names it.
    pub fn undo_label(&self) -> Option<String> { self.undo.last().map(|r| r.label()) }
    fn done(&mut self, r: Record) { self.undo.push(r); if self.undo.len() > 200 { self.undo.remove(0); } }

    /// An operation that asks nobody (a name that is taken stops it) and tells nobody how it stands.
    #[cfg(test)]
    pub fn run(&mut self, op: Op) -> Report { let (cancel, status) = (AtomicBool::new(false), Mutex::new(None)); let mut ask = |_: &Conflict| (Choice::Stop, false); self.run_with(op, &mut Ctl { cancel: &cancel, status: &status, ask: &mut ask }) }
    pub fn run_with(&mut self, op: Op, ctl: &mut Ctl) -> Report {
        let cancel = ctl.cancel; let mut none = |_: u64| {}; let mut pr = Progress { cancel, bytes: &mut none };
        match op {
            Op::Transfer { paths, dest, mv } => self.transfer(paths, dest, mv, ctl),
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
            Op::DuplicateSequence { frames } => {
                let mut rep = Report { kind: Some(Kind::Duplicate), ..Default::default() }; let mut made = vec![];
                let Some(dir) = frames.first().and_then(|f| f.0.parent()).map(Path::to_path_buf) else { return rep };
                if let Some(e) = frames.iter().find_map(|f| self.guard.check(&f.0).err()) { rep.error = Some(e); return rep }
                // ONE copy name for the whole sequence: the first under which no frame's name is taken
                let Some(n) = (1..10_000).find(|n| frames.iter().all(|(_, base, tail)| std::fs::symlink_metadata(dir.join(format!("{}{tail}", copy_name(base, true, *n)))).is_err())) else { rep.error = Some("The sequence could not be duplicated.".into()); return rep };
                for (src, base, tail) in &frames {
                    if cancel.load(Ordering::Relaxed) { break }
                    let dst = dir.join(format!("{}{tail}", copy_name(base, true, n)));
                    match copy_tree(src, &dst, &mut pr) { Ok(()) => { if let Some(s) = stamp(&dst) { made.push((dst.clone(), s)); } rep.made.push(dst); }
                        Err(e) => { rep.error = Some(format!("\u{201c}{}\u{201d} could not be duplicated: {}.", name_of(src), plain(&e))); break } }
                }
                if !made.is_empty() { rep.changed.push(dir); self.done(Record::Copies(made)); }
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

    /// Copy or move `paths` into `dest`.
    fn transfer(&mut self, paths: Vec<PathBuf>, dest: PathBuf, mv: bool, ctl: &mut Ctl) -> Report {
        let mut rep = Report { kind: Some(if mv { Kind::Move } else { Kind::Copy }), ..Default::default() };
        // the folder they go into must be one this build may change; a moved item's own place too (a copy only reads)
        if let Err(e) = self.guard.check(&dest.join("x")) { rep.error = Some(e); return rep }
        if mv { if let Some(e) = paths.iter().find_map(|p| self.guard.check(p).err()) { rep.error = Some(e); return rep } }
        let Ok(real_dest) = std::fs::canonicalize(&dest) else { rep.error = Some("The folder to put them in is not there.".into()); return rep };
        for p in &paths { if std::fs::canonicalize(p).is_ok_and(|r| real_dest.starts_with(&r)) { rep.error = Some(format!("\u{201c}{}\u{201d} cannot be {} into itself.", name_of(p), if mv { "moved" } else { "copied" })); return rep } }
        // how much there is (a move on one volume moves no bytes, but its items count)
        let verb = if mv { "Moving" } else { "Copying" }; let into = name_of(&dest);
        let say = |items: (u64, u64), bytes: (u64, u64), current: &str| *ctl.status.lock().unwrap() = Some(Status { label: format!("{verb} {} to \u{201c}{into}\u{201d}", if paths.len() == 1 { "1 item".to_string() } else { format!("{} items", paths.len()) }), items, bytes, current: current.to_string() });
        say((0, 0), (0, 0), "");
        let (mut n, mut total) = (0u64, 0u64); for p in &paths { measure(p, ctl.cancel, &mut n, &mut total); }
        let mut run = Run { mv, cancel: ctl.cancel, status: ctl.status, ask: ctl.ask, all: None, stopped: false, items: (0, n), bytes: (0, total), label: format!("{verb} {} to \u{201c}{into}\u{201d}", if paths.len() == 1 { "1 item".to_string() } else { format!("{} items", paths.len()) }),
            copied: vec![], moved: vec![], replaced: vec![], changed: vec![], made: vec![] };
        for p in &paths {
            if run.stopped || run.cancel.load(Ordering::Relaxed) { break }
            if let Err(e) = run.item(p, &dest, true) { if e.kind() != std::io::ErrorKind::Interrupted { rep.error = Some(format!("\u{201c}{}\u{201d} could not be {}: {}.", name_of(p), if mv { "moved" } else { "copied" }, plain(&e))); } break }
        }
        *ctl.status.lock().unwrap() = None;
        rep.changed = run.changed; rep.made = run.made;
        if !(run.copied.is_empty() && run.moved.is_empty() && run.replaced.is_empty()) { self.done(Record::Transfer { mv, copied: run.copied, moved: run.moved, replaced: run.replaced }); }
        rep
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
            Record::Transfer { copied, moved, replaced, .. } => { let mut left = 0;
                // the copies to the Trash (one changed since stays); what was moved, back; then what they replaced, back from the Trash
                for (p, was) in copied.into_iter().rev() { if stamp(&p) != Some(was) { if std::fs::symlink_metadata(&p).is_ok() { #[cfg(test)] eprintln!("UNDO-DEBUG stamp {:?} now {:?} was {:?}", p, stamp(&p), was); left += 1; } continue } match trash(&p) { Ok(_) => note(&mut rep, p.parent()), Err(e) => { #[cfg(test)] eprintln!("UNDO-DEBUG trash {:?} {:?}", p, e); let _ = e; left += 1 } } }
                let cancel = AtomicBool::new(false); let mut none = |_: u64| {};
                for (from, to) in moved.into_iter().rev() { if let Some(dir) = from.parent() { let _ = std::fs::create_dir_all(dir); }
                    match move_one(&to, &from, &mut Progress { cancel: &cancel, bytes: &mut none }) { Ok(()) => { note(&mut rep, from.parent()); note(&mut rep, to.parent()); rep.made.push(from); } Err(_) => left += 1 } }
                for (orig, at) in replaced { match rename_no_replace(&at, &orig) { Ok(()) => note(&mut rep, orig.parent()), Err(_) => left += 1 } }
                if left > 0 { rep.error = Some(if left == 1 { "One item was changed since, or is in the way, and was left where it is.".into() } else { format!("{left} items were changed since, or are in the way, and were left where they are.") }); } }
            Record::Trash(moved) => { let mut left = 0;
                for (orig, at) in moved { match rename_no_replace(&at, &orig) { Ok(()) => { note(&mut rep, orig.parent()); rep.made.push(orig); } Err(_) => left += 1 } }
                if left > 0 { rep.error = Some(if left == 1 { "One item could not be put back: its name is taken, or it is no longer in the Trash.".into() } else { format!("{left} items could not be put back: their names are taken, or they are no longer in the Trash.") }); } }
        }
        rep
    }
}

// ---------------------------------------------------------------- the application's one engine

/// A question the engine waits on: asked of the window that started the operation.
#[derive(Debug)]
pub struct Question { pub id: u64, pub origin: u64, pub conflict: Conflict }
struct Shared { tx: Sender<(u64, Op)>, undo: Arc<Mutex<Option<String>>>, guard: Guard, ports: Arc<Mutex<Vec<(u64, crate::lister::Port)>>>,
    /// the running operation: how it stands, how it is called off; what waits behind it (what the status bar says of each)
    status: Arc<Mutex<Option<Status>>>, cancel: Arc<AtomicBool>, waiting: Arc<Mutex<Vec<String>>>,
    /// the answers to the engine's questions; the question that is open (its number, the window asked)
    answers: Sender<(u64, Choice, bool)>, open: Arc<Mutex<Option<(u64, u64)>>> }
static SHARED: OnceLock<Shared> = OnceLock::new();

fn label_of(op: &Op) -> String {
    let items = |n: usize| if n == 1 { "1 item".to_string() } else { format!("{n} items") };
    match op { Op::NewFolder { .. } => "New folder".into(), Op::Rename { .. } => "Rename".into(), Op::Duplicate { paths } => format!("Duplicate {}", items(paths.len())), Op::DuplicateSequence { frames } => format!("Duplicate a sequence of {} frames", frames.len()), Op::Trash { paths } => format!("Move {} to the Trash", items(paths.len())), Op::Delete { paths } => format!("Delete {}", items(paths.len())),
        Op::Transfer { paths, dest, mv } => format!("{} {} to \u{201c}{}\u{201d}", if *mv { "Move" } else { "Copy" }, items(paths.len()), name_of(dest)), Op::Undo => "Undo".into() }
}

/// Start the engine's thread. `roots`: the folders this build may change things in.
pub fn init(roots: &[PathBuf]) {
    let (tx, rx) = channel::<(u64, Op)>(); let (answers, answered) = channel::<(u64, Choice, bool)>();
    let undo = Arc::new(Mutex::new(None)); let ports: Arc<Mutex<Vec<(u64, crate::lister::Port)>>> = Arc::new(Mutex::new(vec![])); let guard = Guard::new(roots);
    let (status, cancel, waiting, open) = (Arc::new(Mutex::new(None)), Arc::new(AtomicBool::new(false)), Arc::new(Mutex::new(Vec::<String>::new())), Arc::new(Mutex::new(None)));
    let (u, p, g, st, ca, wa, op) = (undo.clone(), ports.clone(), guard.clone(), status.clone(), cancel.clone(), waiting.clone(), open.clone());
    let _ = std::thread::Builder::new().name("ufb-ops".into()).spawn(move || {
        let mut engine = Engine::new(g); let mut next = 1u64;
        while let Ok((origin, job)) = rx.recv() {
            { let mut w = wa.lock().unwrap(); if !w.is_empty() { w.remove(0); } } ca.store(false, Ordering::Relaxed);
            // a question goes to the window that asked for the operation, and the engine waits: for its answer, for the
            // operation being called off, or for that window to be gone
            let mut ask = |c: &Conflict| -> (Choice, bool) {
                let id = next; next += 1; *op.lock().unwrap() = Some((id, origin));
                let q = Arc::new(Question { id, origin, conflict: c.clone() }); let mut sent = false;
                for (w, port) in p.lock().unwrap().iter() { if *w == origin { port.send(crate::lister::Msg::Ask(q.clone())); sent = true; } }
                let mut got = (Choice::Stop, false);
                while sent { match answered.recv_timeout(std::time::Duration::from_millis(100)) { Ok((a, choice, all)) if a == id => { got = (choice, all); break } Ok(_) => {} Err(_) => if ca.load(Ordering::Relaxed) { break } } }
                *op.lock().unwrap() = None; got
            };
            let mut rep = engine.run_with(job, &mut Ctl { cancel: &ca, status: &st, ask: &mut ask }); rep.origin = origin; *u.lock().unwrap() = engine.undo_label(); *st.lock().unwrap() = None;
            // every window hears of it (a folder may be shown in several)
            let rep = Arc::new(rep); for (_, port) in p.lock().unwrap().iter() { port.send(crate::lister::Msg::Op(rep.clone())); }
        }
    });
    let _ = SHARED.set(Shared { tx, undo, guard, ports, status, cancel, waiting, answers, open });
}
/// A window's port: it hears of every operation from now on, and is asked the questions of its own.
pub fn subscribe(window: u64, port: crate::lister::Port) { if let Some(s) = SHARED.get() { s.ports.lock().unwrap().push((window, port)); } }
/// A window is gone: a question that waits on it is answered "stop".
pub fn unsubscribe(window: u64) { if let Some(s) = SHARED.get() { s.ports.lock().unwrap().retain(|(w, _)| *w != window); if let Some((id, w)) = *s.open.lock().unwrap() { if w == window { let _ = s.answers.send((id, Choice::Stop, false)); } } } }
pub fn submit(origin: u64, op: Op) { if let Some(s) = SHARED.get() { s.waiting.lock().unwrap().push(label_of(&op)); let _ = s.tx.send((origin, op)); } }
pub fn answer(question: u64, choice: Choice, all: bool) { if let Some(s) = SHARED.get() { let _ = s.answers.send((question, choice, all)); } }
pub fn undo_label() -> Option<String> { SHARED.get().and_then(|s| s.undo.lock().unwrap().clone()) }
/// The operation that runs now, and what waits behind it.
pub fn status() -> Option<Status> { SHARED.get().and_then(|s| s.status.lock().unwrap().clone()) }
pub fn waiting() -> Vec<String> { SHARED.get().map(|s| s.waiting.lock().unwrap().clone()).unwrap_or_default() }
/// Call the running operation off: what it has done stays done (and can be undone).
pub fn cancel() { if let Some(s) = SHARED.get() { s.cancel.store(true, Ordering::Relaxed); } }
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
        assert!(g.allows(&t.0.join("job/a.txt")) && g.allows(&t.0.join("job/new-name")), "a name inside is allowed"); assert!(!g.allows(&t.0.join("job/sub/b")), "an enclosing folder that is not there is not");
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
        let t = Temp::new("ops-dup"); t.file("plate.exr", 5000); t.file("shots/010/comp.nk", 20); t.file("shots/010/renders/beauty.exr", 300);
        // the name the first copy would take is taken
        let first = copy_name("plate.exr", false, 1); t.file(&first, 1);
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
        std::fs::File::options().write(true).open(t.0.join("plate.exr")).unwrap().set_modified(old).unwrap();
        let mut e = engine(&t);
        let r = ok(e.run(Op::Duplicate { paths: vec![t.0.join("plate.exr"), t.0.join("shots")] }));
        assert_eq!(r.made, [t.0.join(copy_name("plate.exr", false, 2)), t.0.join(copy_name("shots", true, 1))], "the first copy's name was taken: the second is given"); assert_eq!(std::fs::metadata(t.0.join(&first)).unwrap().len(), 1, "what was there is as it was");
        let copy = &r.made[0]; let m = std::fs::metadata(copy).unwrap(); assert_eq!(m.len(), 5000); assert_eq!(m.modified().unwrap(), old, "the copy keeps the file's time");
        assert_eq!(std::fs::metadata(r.made[1].join("010/renders/beauty.exr")).unwrap().len(), 300, "a folder is copied with everything in it");
        assert!(!names(&t.0).iter().any(|n| n.contains("ufb-partial")), "no partial file is left"); assert_eq!(e.undo_label().as_deref(), Some("Duplicate of 2 Items"));
        // undo: the copies go to the Trash - but one that was changed since stays, and that is said
        #[cfg(target_os = "macos")] {
            std::fs::write(copy, b"edited since").unwrap();
            let u = e.run(Op::Undo); assert!(u.error.is_some_and(|w| w.contains("left where it is"))); assert!(copy.exists(), "the changed copy stays"); assert!(!r.made[1].exists(), "the untouched one went to the Trash");
            assert!(t.0.join("plate.exr").exists() && t.0.join("shots/010/comp.nk").exists(), "the originals are untouched");
        }
        // a SEQUENCE is duplicated as a sequence: the copy's name goes on its base, the frame numbers stay; one name for all its frames
        for f in 1..=3 { t.file(&format!("seq.{f:04}.exr"), 10 * f); } t.file(&format!("{}.0002.exr", copy_name("seq", true, 1)), 1);
        let frames: Vec<(PathBuf, String, String)> = (1..=3).map(|f| (t.0.join(format!("seq.{f:04}.exr")), "seq".to_string(), format!(".{f:04}.exr"))).collect();
        let r = ok(e.run(Op::DuplicateSequence { frames })); let second = copy_name("seq", true, 2);
        assert_eq!(r.made, (1..=3).map(|f| t.0.join(format!("{second}.{f:04}.exr"))).collect::<Vec<_>>(), "the first copy name was taken by ONE frame: the whole sequence takes the second"); assert_eq!(std::fs::metadata(&r.made[2]).unwrap().len(), 30);
        assert_eq!(min_paths::sequence::group(&names(&t.0), &Default::default()).iter().map(|g| g.display_name()).collect::<Vec<_>>(), [format!("{second}.[0001-0003].exr"), "seq.[0001-0003].exr".to_string()], "and the copy is a sequence too");
        // a copy that is called off leaves nothing behind
        let cancel = AtomicBool::new(true); let mut none = |_: u64| {}; let dst = t.0.join("never.exr");
        assert!(copy_tree(&t.0.join("plate.exr"), &dst, &mut Progress { cancel: &cancel, bytes: &mut none }).is_err()); assert!(!dst.exists() && !partial(&dst).exists());
        let dst = t.0.join("never-folder"); assert!(copy_tree(&t.0.join("shots"), &dst, &mut Progress { cancel: &cancel, bytes: &mut none }).is_err()); assert!(!dst.exists());
        // ... and a copy onto a name that is taken is refused, the file there untouched
        let cancel = AtomicBool::new(false); assert_eq!(copy_tree(&t.0.join("plate.exr"), &t.0.join(&first), &mut Progress { cancel: &cancel, bytes: &mut none }).unwrap_err().kind(), std::io::ErrorKind::AlreadyExists); assert_eq!(std::fs::metadata(t.0.join(&first)).unwrap().len(), 1);
    }
    /// Run a copy or a move with a fixed answer to every question; how often it was asked comes back too.
    fn transfer(e: &mut Engine, paths: Vec<PathBuf>, dest: &Path, mv: bool, answer: (Choice, bool)) -> (Report, usize) {
        let (cancel, status) = (AtomicBool::new(false), Mutex::new(None)); let mut asked = 0; let mut ask = |_: &Conflict| { asked += 1; answer };
        let r = e.run_with(Op::Transfer { paths, dest: dest.to_path_buf(), mv }, &mut Ctl { cancel: &cancel, status: &status, ask: &mut ask }); assert!(status.lock().unwrap().is_none(), "nothing runs any more"); (r, asked)
    }
    fn len(p: &Path) -> u64 { std::fs::metadata(p).unwrap().len() }
    #[test]
    fn a_copy_into_a_folder_asks_about_a_taken_name_and_merges_folders() {
        let _sweep = Sweep;
        let t = Temp::new("ops-copy"); t.file("src/a.exr", 10); t.file("src/b.exr", 20); t.file("src/shots/010/comp.nk", 30); t.file("src/shots/020/comp.nk", 40);
        t.file("dst/a.exr", 1); t.file("dst/shots/010/comp.nk", 2); t.file("dst/shots/010/notes.txt", 3);
        let (src, dst) = (t.0.join("src"), t.0.join("dst")); let all = || vec![src.join("a.exr"), src.join("b.exr"), src.join("shots")]; let mut e = engine(&t);
        // asked, and stopped at the first question: nothing of the rest is done, nothing replaced
        let (r, asked) = transfer(&mut e, all(), &dst, false, (Choice::Stop, false)); assert_eq!((r.error.clone(), asked), (None, 1)); assert_eq!(names(&dst), ["a.exr", "shots"]); assert_eq!(len(&dst.join("a.exr")), 1);
        // skip, one at a time: asked for each of the two taken names; b.exr and what is new in the folders come; the folders MERGED
        let (r, asked) = transfer(&mut e, all(), &dst, false, (Choice::Skip, false)); assert_eq!((r.error.clone(), asked), (None, 2));
        assert_eq!(names(&dst), ["a.exr", "b.exr", "shots"]); assert_eq!(len(&dst.join("a.exr")), 1); assert_eq!(len(&dst.join("shots/010/comp.nk")), 2, "skipped inside the merged folder too");
        assert_eq!(len(&dst.join("shots/020/comp.nk")), 40, "what was not there came"); assert_eq!(len(&dst.join("shots/010/notes.txt")), 3, "what only the destination had stays"); assert!(r.changed.contains(&dst)); assert!(src.join("shots/020/comp.nk").exists(), "a copy leaves its source");
        assert_eq!(e.undo_label().as_deref(), Some("Copy of 2 Items"), "b.exr and the folder 020");
        // keep both, for all: asked once; each gets the second one's name
        let (r, asked) = transfer(&mut e, vec![src.join("a.exr"), src.join("b.exr")], &dst, false, (Choice::KeepBoth, true)); assert_eq!((r.error.clone(), asked), (None, 1));
        let (a2, b2) = (keep_both_name("a.exr", false, 2), keep_both_name("b.exr", false, 2)); assert_eq!(len(&dst.join(&a2)), 10); assert_eq!(len(&dst.join(&b2)), 20); assert_eq!(len(&dst.join("a.exr")), 1); assert_eq!(r.made, [dst.join(&a2), dst.join(&b2)]);
        // undo takes exactly those two back
        ok(e.run(Op::Undo)); assert!(!dst.join(&a2).exists() && !dst.join(&b2).exists() && dst.join("b.exr").exists());
        // replace: the new one is there, whole; and undo brings the old one back (macOS: it went to the Trash)
        let (r, _) = transfer(&mut e, vec![src.join("a.exr")], &dst, false, (Choice::Replace, false)); assert_eq!(r.error, None); assert_eq!(len(&dst.join("a.exr")), 10);
        #[cfg(target_os = "macos")] { ok(e.run(Op::Undo)); assert_eq!(len(&dst.join("a.exr")), 1, "what was replaced is back"); }
        // into the folder it is in: a duplicate, under the system's name for one; into itself: refused
        let (r, asked) = transfer(&mut e, vec![src.join("b.exr")], &src, false, (Choice::Stop, false)); assert_eq!((r.error, asked), (None, 0)); assert_eq!(len(&src.join(copy_name("b.exr", false, 1))), 20);
        let (r, _) = transfer(&mut e, vec![src.join("shots")], &src.join("shots/010"), false, (Choice::Stop, false)); assert!(r.error.is_some_and(|w| w.contains("into itself"))); assert_eq!(names(&src.join("shots/010")), ["comp.nk"]);
        // from outside the belt INTO it is a copy like any other (the source is only read); out of it is refused
        let o = Temp::new("ops-copy-outside"); o.file("plate.mov", 77);
        let (r, _) = transfer(&mut e, vec![o.0.join("plate.mov")], &dst, false, (Choice::Stop, false)); assert_eq!(r.error, None); assert_eq!(len(&dst.join("plate.mov")), 77);
        let (r, _) = transfer(&mut e, vec![dst.join("plate.mov")], &o.0, false, (Choice::Stop, false)); assert_eq!(r.error.as_deref(), Some(GUARDED)); assert_eq!(names(&o.0), ["plate.mov"]);
        let (r, _) = transfer(&mut e, vec![o.0.join("plate.mov")], &dst.join("shots"), true, (Choice::Stop, false)); assert_eq!(r.error.as_deref(), Some(GUARDED), "a move takes the source away: it must be inside the belt too"); assert!(o.0.join("plate.mov").exists());
    }
    #[test]
    fn a_move_is_a_rename_merges_and_is_undone() {
        let _sweep = Sweep;
        let t = Temp::new("ops-move"); t.file("src/a.exr", 10); t.file("src/shots/010/comp.nk", 30); t.file("src/shots/020/comp.nk", 40); t.file("dst/shots/010/comp.nk", 2); t.dir("dst/empty");
        let (src, dst) = (t.0.join("src"), t.0.join("dst")); let mut e = engine(&t);
        let (r, asked) = transfer(&mut e, vec![src.join("a.exr"), src.join("shots")], &dst, true, (Choice::Skip, false)); assert_eq!((r.error.clone(), asked), (None, 1));
        assert!(!src.join("a.exr").exists()); assert_eq!(len(&dst.join("a.exr")), 10); assert_eq!(len(&dst.join("shots/020/comp.nk")), 40); assert!(!src.join("shots/020").exists(), "what moved is gone from its place");
        assert_eq!(len(&src.join("shots/010/comp.nk")), 30, "what was skipped stays where it was - and so its folders stay"); assert_eq!(len(&dst.join("shots/010/comp.nk")), 2);
        assert!(r.changed.contains(&dst) && r.changed.contains(&src)); assert_eq!(e.undo_label().as_deref(), Some("Move of 2 Items"));
        // into the folder it is in: nothing; into its own subfolder: refused
        let (r, _) = transfer(&mut e, vec![dst.join("a.exr")], &dst, true, (Choice::Stop, false)); assert_eq!(r.error, None); assert!(dst.join("a.exr").exists());
        let (r, _) = transfer(&mut e, vec![dst.join("shots")], &dst.join("shots/020"), true, (Choice::Stop, false)); assert!(r.error.is_some_and(|w| w.contains("into itself"))); assert!(dst.join("shots/020/comp.nk").exists());
        // undo: everything that moved is back where it was
        ok(e.run(Op::Undo)); assert_eq!(len(&src.join("a.exr")), 10); assert_eq!(len(&src.join("shots/020/comp.nk")), 40); assert!(!dst.join("a.exr").exists() && !dst.join("shots/020").exists()); assert_eq!(len(&dst.join("shots/010/comp.nk")), 2);
        // a move that replaces: asked; the old one out of the way, the moved one in its place
        let (r, _) = transfer(&mut e, vec![src.join("shots/010/comp.nk")], &dst.join("shots/010"), true, (Choice::Replace, false)); assert_eq!(r.error, None); assert_eq!(len(&dst.join("shots/010/comp.nk")), 30); assert!(!src.join("shots/010/comp.nk").exists());
        #[cfg(target_os = "macos")] { ok(e.run(Op::Undo)); assert_eq!(len(&src.join("shots/010/comp.nk")), 30); assert_eq!(len(&dst.join("shots/010/comp.nk")), 2, "what was replaced is back"); }
    }
    #[test]
    fn a_copy_tells_how_it_stands_and_can_be_called_off() {
        let t = Temp::new("ops-cancel"); for i in 0..40 { t.file(&format!("src/f{i:02}.exr"), 1000); } t.dir("dst"); let mut e = engine(&t);
        let (cancel, status) = (AtomicBool::new(false), Mutex::new(None)); let mut ask = |_: &Conflict| (Choice::Stop, false);
        // called off before it starts: nothing comes, nothing to undo
        cancel.store(true, Ordering::Relaxed);
        let r = e.run_with(Op::Transfer { paths: vec![t.0.join("src")], dest: t.0.join("dst"), mv: false }, &mut Ctl { cancel: &cancel, status: &status, ask: &mut ask });
        assert_eq!(r.error, None, "calling off is not a failure"); assert!(names(&t.0.join("dst")).is_empty()); assert!(e.undo_label().is_none());
        // how it stands: the items and bytes counted before it began
        cancel.store(false, Ordering::Relaxed); let seen = std::sync::Arc::new(Mutex::new(Vec::<Status>::new())); let files: Vec<PathBuf> = (0..40).map(|i| t.0.join(format!("src/f{i:02}.exr"))).collect();
        let mut n = 0; let mut measured = 0; for f in &files { measure(f, &cancel, &mut n, &mut measured); } assert_eq!((n, measured), (40, 40_000));
        let r = e.run_with(Op::Transfer { paths: files, dest: t.0.join("dst"), mv: false }, &mut Ctl { cancel: &cancel, status: &status, ask: &mut ask }); assert_eq!(r.error, None); assert_eq!(names(&t.0.join("dst")).len(), 40); let _ = seen;
        assert_eq!(e.undo_label().as_deref(), Some("Copy of 40 Items"));
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
