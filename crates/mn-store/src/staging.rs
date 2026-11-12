//! Staging (row 272). A collection is never worked on where it lives: on open it is copied into a private session
//! folder on this machine — once as the WORK copy the database runs on, once as the BASE (the file as opened, which a
//! merge on save compares against). Save writes a new file beside the original and renames it over, so a share never
//! holds a half-written file. A session folder whose owner is gone is a crash's leftover and is OFFERED BACK.
//!
//! One session folder: `work.mnd`, `base.mnd`, `origin` (where the file lives and what it looked like when read) and
//! `lock` (held locked while the session lives).
use std::{fs::{self, File}, io, path::{Path, PathBuf}, time::UNIX_EPOCH};

/// What the original looked like when it was last read or written by us.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stamp { pub size: u64, pub mtime_ns: u128 }
impl Stamp {
    pub fn of(path: &Path) -> io::Result<Stamp> { let m = fs::metadata(path)?; Ok(Stamp { size: m.len(), mtime_ns: m.modified()?.duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0) }) }
}

/// What a look at the original's place found. Only `Missing` and `Unreachable` say nothing can be published: on a share,
/// "not there" is as often a dropped mount as a deleted file (UFB's lesson: an error or an absence must never be read
/// as "gone", and must never cause a write).
#[derive(Debug)]
pub enum Original { /// what we last read or wrote
    Same, /// someone else's file is there now
    Changed, /// nothing is at the path (deleted, moved — or the share is not mounted)
    Missing, /// the path could not be asked (the share does not answer, access is refused)
    Unreachable(io::Error) }

#[derive(Debug)]
pub struct Session { original: PathBuf, dir: PathBuf, stamp: Option<Stamp>, _lock: File }

/// A session folder nobody holds: the work of a session that did not close.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Leftover { pub dir: PathBuf, pub original: PathBuf, pub work: PathBuf, pub work_modified_ns: u128 }

fn write_origin(dir: &Path, original: &Path, stamp: Option<Stamp>) -> io::Result<()> {
    let (size, mtime) = stamp.map(|s| (s.size as i128, s.mtime_ns as i128)).unwrap_or((-1, -1));
    let tmp = dir.join("origin.tmp");
    fs::write(&tmp, format!("{size}\n{mtime}\n{}", original.display()))?;
    fs::rename(tmp, dir.join("origin"))
}
fn read_origin(dir: &Path) -> Option<(PathBuf, Option<Stamp>)> {
    let s = fs::read_to_string(dir.join("origin")).ok()?; let mut it = s.splitn(3, '\n');
    let (size, mtime, path) = (it.next()?.parse::<i128>().ok()?, it.next()?.parse::<i128>().ok()?, it.next()?);
    Some((PathBuf::from(path), (size >= 0).then_some(Stamp { size: size as u64, mtime_ns: mtime as u128 })))
}

impl Session {
    /// Opens an EXISTING file into a new session folder under `root`. A path with nothing at it is an error, never a
    /// new collection: on a share a missing file is as likely a missing mount, and treating it as new would let the
    /// next Save put an empty collection where the real one belongs. A new collection is made with [`Session::create`].
    pub fn open(root: &Path, original: &Path) -> io::Result<Session> { fs::metadata(original)?; Session::start(root, original) }
    /// Starts a NEW collection at `original`: nothing may be there yet, and its folder must exist.
    pub fn create(root: &Path, original: &Path) -> io::Result<Session> {
        match fs::metadata(original) { Ok(_) => return Err(io::Error::new(io::ErrorKind::AlreadyExists, "a file is already there")), Err(e) if e.kind() == io::ErrorKind::NotFound => {}, Err(e) => return Err(e) }
        let parent = original.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        if !fs::metadata(parent)?.is_dir() { return Err(io::Error::new(io::ErrorKind::NotFound, "the folder is not there")) }
        Session::start(root, original)
    }
    fn start(root: &Path, original: &Path) -> io::Result<Session> {
        let dir = root.join(uuid::Uuid::new_v4().simple().to_string());
        fs::create_dir_all(&dir)?;
        let lock = File::create(dir.join("lock"))?; lock.try_lock().map_err(io::Error::other)?;
        let stamp = if original.exists() {
            // the stamp is taken BEFORE the copy: a write that lands during the copy then shows as an outside change
            let stamp = Stamp::of(original)?;
            fs::copy(original, dir.join("base.mnd"))?;
            fs::copy(dir.join("base.mnd"), dir.join("work.mnd"))?;
            Some(stamp)
        } else { None };
        write_origin(&dir, original, stamp)?;
        Ok(Session { original: original.to_path_buf(), dir, stamp, _lock: lock })
    }
    /// Takes over a crash's leftover: its work copy becomes this session's, as it was left.
    pub fn resume(left: &Leftover) -> io::Result<Session> {
        let lock = File::options().write(true).open(left.dir.join("lock"))?; lock.try_lock().map_err(io::Error::other)?;
        let (original, stamp) = read_origin(&left.dir).ok_or_else(|| io::Error::other("no origin"))?;
        Ok(Session { original, dir: left.dir.clone(), stamp, _lock: lock })
    }
    pub fn original(&self) -> &Path { &self.original }
    /// Where files added to the collection (dropped pictures) wait on this machine until a save puts them beside the
    /// document (rows 293, 310): the document's place is touched only at open and at a save.
    pub fn media_dir(&self) -> PathBuf { self.dir.join("media") }
    /// The copy the database runs on.
    pub fn work(&self) -> PathBuf { self.dir.join("work.mnd") }
    /// The file as it was when opened or last saved; None for a collection never saved.
    pub fn base(&self) -> Option<PathBuf> { let p = self.dir.join("base.mnd"); p.exists().then_some(p) }
    /// Looks at the original's place (one `stat`).
    pub fn probe(&self) -> Original {
        match (Stamp::of(&self.original), self.stamp) {
            (Ok(now), Some(seen)) => if now == seen { Original::Same } else { Original::Changed },
            (Ok(_), None) => Original::Changed,                                                   // a file appeared where our new collection is to go
            (Err(e), seen) if e.kind() == io::ErrorKind::NotFound => if seen.is_none() { Original::Same } else { Original::Missing },   // a collection never published has nothing there
            (Err(e), _) => Original::Unreachable(e),
        }
    }
    /// True when someone else's file is at the original's place.
    pub fn original_changed(&self) -> bool { matches!(self.probe(), Original::Changed) }
    /// The original has changed under us: copies it, as it is now, into the session as `theirs.mnd` and returns the
    /// copy with the stamp it was taken at (which [`Session::save_merged`] checks before it replaces the original).
    /// The fingerprint of the file as opened or last published (None for a collection never published): what the
    /// document has to be for a publish to go ahead.
    pub fn base_print(&self) -> io::Result<Option<crate::share::Fingerprint>> { match self.base() { Some(b) => crate::share::fingerprint(&b).map(Some), None => Ok(None) } }
    /// Someone else's file is at the document's place: copies it, as it is now, into the session as `theirs.mnd`, and
    /// returns the copy with its fingerprint (which [`Session::save_merged`] expects the document to still have).
    pub fn fetch_theirs(&self) -> io::Result<(PathBuf, crate::share::Fingerprint)> {
        let to = self.dir.join("theirs.mnd"); let print = crate::share::fetch(&self.original, &to)?; Ok((to, print))
    }
    /// Publishes a MERGED file: as [`Session::save`], but the document must still be the one `theirs` was copied from —
    /// if it was saved again, nothing is written and the error kind is `Interrupted`: fetch and merge again.
    pub fn save_merged(&mut self, theirs: crate::share::Fingerprint, produce: impl FnOnce(&Path) -> io::Result<()>) -> io::Result<()> {
        let to = self.original.clone();
        self.publish(&to, Some(theirs), produce)?;
        let _ = fs::remove_file(self.dir.join("theirs.mnd")); Ok(())
    }
    /// Publishes: `produce` writes the complete new file to the path it is given — a file IN THE SESSION, on this
    /// machine (no database writes at the document's place); that file then becomes the document by
    /// [`crate::share::put`], which requires the document to still be the one this session opened or last published
    /// (`Interrupted` if it is not: someone else saved — fetch, merge, [`Session::save_merged`]). On any error the
    /// document is untouched.
    pub fn save(&mut self, produce: impl FnOnce(&Path) -> io::Result<()>) -> io::Result<()> { let (to, expect) = (self.original.clone(), self.base_print()?); self.publish(&to, expect, produce) }
    /// The same at a new place (nothing may be there), which becomes this session's original.
    pub fn save_as(&mut self, to: &Path, produce: impl FnOnce(&Path) -> io::Result<()>) -> io::Result<()> { self.publish(to, None, produce) }
    fn publish(&mut self, to: &Path, expect: Option<crate::share::Fingerprint>, produce: impl FnOnce(&Path) -> io::Result<()>) -> io::Result<()> {
        let staged = self.stage_publish(produce)?;
        match crate::share::put(&staged, to, expect) {
            Ok(_) => self.published(to),
            Err(e) => { let _ = fs::remove_file(&staged); Err(match e { crate::share::PutError::Raced => io::Error::new(io::ErrorKind::Interrupted, e.to_string()), crate::share::PutError::Unverified => io::Error::new(io::ErrorKind::InvalidData, e.to_string()), crate::share::PutError::Io(e) => e }) }
        }
    }
    /// The two halves of a publish, for a caller that runs [`crate::share::put`] itself (off its main thread):
    /// `produce` writes the complete new file INTO THE SESSION, which is flushed; its path is returned.
    pub fn stage_publish(&self, produce: impl FnOnce(&Path) -> io::Result<()>) -> io::Result<PathBuf> {
        let staged = self.dir.join("publish.mnd"); let _ = fs::remove_file(&staged);
        let r = produce(&staged).and_then(|()| File::options().write(true).open(&staged)?.sync_all());
        if let Err(e) = r { let _ = fs::remove_file(&staged); return Err(e) }
        Ok(staged)
    }
    /// The staged file has become the document at `to` (a `put` succeeded): it is the new base, and `to` this
    /// session's original.
    pub fn published(&mut self, to: &Path) -> io::Result<()> {
        fs::rename(self.dir.join("publish.mnd"), self.dir.join("base.mnd"))?;
        let _ = fs::remove_file(self.dir.join("theirs.mnd"));
        self.original = to.to_path_buf(); self.stamp = Stamp::of(to).ok();   // the stamp is a hint only: a share may still answer with the old one
        write_origin(&self.dir, &self.original, self.stamp)
    }
    /// Where a fetched copy of someone else's save goes.
    pub fn theirs_path(&self) -> PathBuf { self.dir.join("theirs.mnd") }
    /// A clean close: the session folder goes. (Dropping a session WITHOUT this leaves it as a leftover — which is
    /// what a crash does, and what a caller with unsaved work may choose.)
    pub fn close(self) -> io::Result<()> { let dir = self.dir.clone(); drop(self); fs::remove_dir_all(dir) }
}

/// The sessions under `root` that nobody holds and that have a work copy: offered back at launch, newest first.
/// Unheld folders with nothing to offer are removed.
pub fn leftovers(root: &Path) -> Vec<Leftover> {
    let mut out = vec![];
    let Ok(rd) = fs::read_dir(root) else { return out };
    for e in rd.flatten() {
        let dir = e.path(); if !dir.is_dir() { continue }
        let Ok(lock) = File::options().write(true).open(dir.join("lock")) else { continue };
        if lock.try_lock().is_err() { continue }   // a live session, here or in another instance
        let work = dir.join("work.mnd");
        match (read_origin(&dir), fs::metadata(&work).and_then(|m| m.modified())) {
            (Some((original, _)), Ok(t)) => out.push(Leftover { dir, original, work, work_modified_ns: t.duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0) }),
            _ => { drop(lock); let _ = fs::remove_dir_all(&dir); }
        }
    }
    out.sort_by(|a, b| b.work_modified_ns.cmp(&a.work_modified_ns));
    out
}
/// Declines a leftover: its folder is removed.
pub fn discard(left: &Leftover) -> io::Result<()> { fs::remove_dir_all(&left.dir) }

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn scratch(name: &str) -> PathBuf { let d = std::env::temp_dir().join(format!("mn-store-{name}-{}", uuid::Uuid::new_v4().simple())); fs::create_dir_all(d.join("share")).unwrap(); fs::create_dir_all(d.join("staging")).unwrap(); d }
    fn put(path: &Path, s: &str) -> io::Result<()> { let mut f = File::create(path)?; f.write_all(s.as_bytes()) }

    #[test]
    fn the_original_is_only_ever_replaced_whole() {
        let d = scratch("save"); let file = d.join("share/notes.mnd"); put(&file, "v1").unwrap();
        let mut s = Session::open(&d.join("staging"), &file).unwrap();
        assert_eq!((fs::read_to_string(s.work()).unwrap(), fs::read_to_string(s.base().unwrap()).unwrap()), ("v1".into(), "v1".into()));
        put(&s.work(), "v2 in progress").unwrap();                       // the work copy changes; the original does not
        assert_eq!(fs::read_to_string(&file).unwrap(), "v1");
        assert!(!s.original_changed());
        // a save that fails half way leaves the original and no stray file beside it
        assert!(s.save(|tmp| { put(tmp, "half")?; Err(io::Error::other("disk full")) }).is_err());
        assert_eq!(fs::read_to_string(&file).unwrap(), "v1");
        assert_eq!(fs::read_dir(d.join("share")).unwrap().count(), 1);
        s.save(|tmp| put(tmp, "v2")).unwrap();
        assert_eq!((fs::read_to_string(&file).unwrap(), fs::read_to_string(s.base().unwrap()).unwrap()), ("v2".into(), "v2".into()));
        assert!(!s.original_changed());
        assert_eq!(fs::read_dir(d.join("share")).unwrap().count(), 1);
        s.close().unwrap();
        assert_eq!(fs::read_dir(d.join("staging")).unwrap().count(), 0);
        fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn an_outside_save_is_seen_and_the_base_still_holds_what_we_opened() {
        let d = scratch("outside"); let file = d.join("share/notes.mnd"); put(&file, "ours").unwrap();
        let s = Session::open(&d.join("staging"), &file).unwrap();
        assert!(matches!(s.probe(), Original::Same));
        put(&file, "theirs, longer").unwrap();
        assert!(s.original_changed() && matches!(s.probe(), Original::Changed));
        assert_eq!(fs::read_to_string(s.base().unwrap()).unwrap(), "ours");
        // gone is NOT "changed" and not "new": it is its own answer, on which nothing is published
        fs::remove_file(&file).unwrap();
        assert!(!s.original_changed() && matches!(s.probe(), Original::Missing));
        // a path that cannot be asked at all (here: a file where a folder should be) is unreachable, not missing
        let odd = Session { original: d.join("staging").join("lock-not-a-dir").join("x.mnd"), dir: d.join("staging"), stamp: s.stamp, _lock: File::create(d.join("probe.lock")).unwrap() };
        put(&d.join("staging").join("lock-not-a-dir"), "a file").unwrap();
        assert!(matches!(odd.probe(), Original::Unreachable(_)), "{:?}", odd.probe());
        std::mem::forget(odd);
        s.close().unwrap(); fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn a_crash_leaves_its_work_to_be_offered_back_and_a_live_session_is_not_offered() {
        let d = scratch("crash"); let root = d.join("staging"); let file = d.join("share/notes.mnd"); put(&file, "v1").unwrap();
        let crashed = Session::open(&root, &file).unwrap();
        put(&crashed.work(), "unsaved work").unwrap();
        let live = Session::open(&root, &file).unwrap();
        assert_eq!(leftovers(&root), []);                                 // both are held
        drop(crashed);                                                    // no close(): what a crash does
        let left = leftovers(&root);
        assert_eq!(left.len(), 1);
        assert_eq!((left[0].original.as_path(), fs::read_to_string(&left[0].work).unwrap()), (file.as_path(), "unsaved work".into()));
        let resumed = Session::resume(&left[0]).unwrap();
        assert_eq!(leftovers(&root), []);                                 // taken: no longer offered
        assert_eq!((fs::read_to_string(resumed.work()).unwrap(), fs::read_to_string(resumed.base().unwrap()).unwrap()), ("unsaved work".into(), "v1".into()));
        assert!(!resumed.original_changed());
        drop(resumed);
        let left = leftovers(&root); discard(&left[0]).unwrap();
        assert_eq!(leftovers(&root), []);
        live.close().unwrap(); fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn a_new_collection_has_no_base_until_its_first_save() {
        let d = scratch("new"); let file = d.join("share/new.mnd");
        // opening a path with nothing at it is refused — a new collection is asked for by name
        assert_eq!(Session::open(&d.join("staging"), &file).unwrap_err().kind(), io::ErrorKind::NotFound);
        assert_eq!(Session::create(&d.join("staging"), &d.join("no-such-folder/new.mnd")).unwrap_err().kind(), io::ErrorKind::NotFound);
        let mut s = Session::create(&d.join("staging"), &file).unwrap();
        assert!(s.base().is_none() && !s.original_changed() && matches!(s.probe(), Original::Same));
        put(&d.join("share/other.mnd"), "x").unwrap();
        assert_eq!(Session::create(&d.join("staging"), &d.join("share/other.mnd")).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        let moved = d.join("share/renamed.mnd");
        s.save_as(&moved, |tmp| put(tmp, "first")).unwrap();
        assert_eq!((s.original(), fs::read_to_string(s.base().unwrap()).unwrap(), file.exists()), (moved.as_path(), "first".into(), false));
        s.close().unwrap(); fs::remove_dir_all(d).unwrap();
    }
}
