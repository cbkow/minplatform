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
    /// Opens `original` into a new session folder under `root`. A file that does not exist yet (a new collection)
    /// starts with no work and no base copy.
    pub fn open(root: &Path, original: &Path) -> io::Result<Session> {
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
    /// The copy the database runs on.
    pub fn work(&self) -> PathBuf { self.dir.join("work.mnd") }
    /// The file as it was when opened or last saved; None for a collection never saved.
    pub fn base(&self) -> Option<PathBuf> { let p = self.dir.join("base.mnd"); p.exists().then_some(p) }
    /// True when the original is no longer what we last read or wrote (someone else saved, or it is gone or new).
    pub fn original_changed(&self) -> bool { Stamp::of(&self.original).ok() != self.stamp }
    /// Publishes: `produce` writes the complete new file to the path it is given (a temporary file BESIDE the
    /// original, so the rename stays on one volume); it is flushed to disk and renamed over the original, and becomes
    /// the new base. On any error the original is untouched and the temporary file is removed.
    pub fn save(&mut self, produce: impl FnOnce(&Path) -> io::Result<()>) -> io::Result<()> { let to = self.original.clone(); self.save_as(&to, produce) }
    /// The same at a new place, which becomes this session's original.
    pub fn save_as(&mut self, to: &Path, produce: impl FnOnce(&Path) -> io::Result<()>) -> io::Result<()> {
        let name = to.file_name().ok_or_else(|| io::Error::other("no file name"))?.to_string_lossy().into_owned();
        let tmp = to.with_file_name(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()));
        let result = (|| {
            produce(&tmp)?;
            File::options().write(true).open(&tmp)?.sync_all()?;
            fs::copy(&tmp, self.dir.join("base.new"))?;
            fs::rename(&tmp, to)?;
            fs::rename(self.dir.join("base.new"), self.dir.join("base.mnd"))
        })();
        if result.is_err() { let _ = fs::remove_file(&tmp); let _ = fs::remove_file(self.dir.join("base.new")); return result }
        self.original = to.to_path_buf(); self.stamp = Stamp::of(to).ok();
        write_origin(&self.dir, &self.original, self.stamp)
    }
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
        put(&file, "theirs, longer").unwrap();
        assert!(s.original_changed());
        assert_eq!(fs::read_to_string(s.base().unwrap()).unwrap(), "ours");
        fs::remove_file(&file).unwrap();
        assert!(s.original_changed());                                    // gone counts as changed
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
        let mut s = Session::open(&d.join("staging"), &file).unwrap();
        assert!(s.base().is_none() && !s.original_changed());
        let moved = d.join("share/renamed.mnd");
        s.save_as(&moved, |tmp| put(tmp, "first")).unwrap();
        assert_eq!((s.original(), fs::read_to_string(s.base().unwrap()).unwrap(), file.exists()), (moved.as_path(), "first".into(), false));
        s.close().unwrap(); fs::remove_dir_all(d).unwrap();
    }
}
