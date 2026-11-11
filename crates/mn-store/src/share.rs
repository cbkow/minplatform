//! Touching the document where it lives — possibly a network share (canon rows 291–293). Three plain file operations
//! and nothing else: a FETCH (copy the document to this machine), a PUT (a complete new file becomes the document), and
//! a fingerprint. No database ever runs at the document's place; a PUT writes a copy made locally.
//!
//! What UFB's experience taught (`docs/phase3/network-share-lessons-from-ufb.md`), built in here:
//! - identity is CONTENT, never the date: a fingerprint of the bytes says whether the document is the one we know;
//! - a PUT looks again just before it renames — a save that slipped in is a lost race, reported, nothing overwritten;
//! - the temporary file has a name of its own per write; a refused rename-over has a defined fallback with no delete
//!   of the document in it (set the old file aside, rename, roll back on failure);
//! - the result is verified AT ITS FINAL NAME, by content;
//! - a call that outlives its time limit is not over: [`Worker`] keeps one busy latch that only the returning call
//!   clears, and discards the result of a call the user stopped waiting for.
use std::{fs::{self, File}, io::{self, Read}, path::Path, sync::mpsc, time::{Duration, Instant}};

/// A file's content, cheaply: its length and a 64-bit FNV-1a of its bytes. (Not a security hash: it tells one
/// version of a few-megabyte document from another.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fingerprint { pub len: u64, pub hash: u64 }

pub fn fingerprint(path: &Path) -> io::Result<Fingerprint> {
    let mut f = File::open(path)?; let mut buf = vec![0u8; 1 << 16]; let (mut len, mut hash) = (0u64, 0xcbf2_9ce4_8422_2325u64);
    loop { let n = f.read(&mut buf)?; if n == 0 { break } len += n as u64; for b in &buf[..n] { hash ^= *b as u64; hash = hash.wrapping_mul(0x0000_0100_0000_01b3); } }
    Ok(Fingerprint { len, hash })
}
/// The fingerprint of what is at `path`, or None when nothing is there. Any other error is an error (never "absent").
pub fn fingerprint_if_present(path: &Path) -> io::Result<Option<Fingerprint>> { match fingerprint(path) { Ok(f) => Ok(Some(f)), Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None), Err(e) => Err(e) } }

/// Copies the document to `to` (on this machine) and returns the fingerprint of THE COPY — what was actually read.
pub fn fetch(original: &Path, to: &Path) -> io::Result<Fingerprint> { fs::copy(original, to)?; fingerprint(to) }

#[derive(Debug)]
pub enum PutError {
    /// The document is not the one expected: someone saved in between (or it appeared, or went). Nothing was written.
    Raced,
    /// The new file is in place but reads back different from what was sent.
    Unverified,
    Io(io::Error),
}
impl From<io::Error> for PutError { fn from(e: io::Error) -> Self { PutError::Io(e) } }
impl std::fmt::Display for PutError { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { match self { PutError::Raced => f.write_str("the document was saved by someone else in the meantime"), PutError::Unverified => f.write_str("the document does not read back as written"), PutError::Io(e) => e.fmt(f) } } }

/// Makes the local file `from` the document at `to`. `expect` is what the document must be when the rename happens
/// (None: nothing may be there). The document is either the old one or the complete new one at every moment.
pub fn put(from: &Path, to: &Path, expect: Option<Fingerprint>) -> Result<Fingerprint, PutError> {
    let sent = fingerprint(from)?;
    let name = to.file_name().ok_or_else(|| io::Error::other("no file name"))?.to_string_lossy().into_owned();
    let id = uuid::Uuid::new_v4().simple().to_string();
    let tmp = to.with_file_name(format!(".{name}.{id}.tmp"));
    let result = (|| -> Result<(), PutError> {
        fs::copy(from, &tmp)?;
        File::options().write(true).open(&tmp)?.sync_all()?;
        // look again, at the last moment: is the document still the one this file was made from?
        if fingerprint_if_present(to)? != expect { return Err(PutError::Raced) }
        if let Err(first) = fs::rename(&tmp, to) {
            // rename-over refused (a share may refuse while another machine holds the file): the old file steps aside
            // under a name of its own, the new one takes its place, and on failure the old one comes back
            if expect.is_none() { return Err(first.into()) }
            let aside = to.with_file_name(format!(".{name}.{id}.old"));
            fs::rename(to, &aside).map_err(|_| PutError::Io(first))?;
            if let Err(e) = fs::rename(&tmp, to) { let back = fs::rename(&aside, to); return Err(PutError::Io(if back.is_ok() { e } else { io::Error::other(format!("{e}; the previous document is at {}", aside.display())) })) }
            let _ = fs::remove_file(&aside);   // the document is the new file; this is only its predecessor
        }
        Ok(())
    })();
    if let Err(e) = result { let _ = fs::remove_file(&tmp); return Err(e) }
    // verified where it now is, by content
    if fingerprint(to)? != sent { return Err(PutError::Unverified) }
    Ok(sent)
}

/// One call at a time to the document's place, off the caller's thread. A call cannot be ended from outside (a dead
/// mount parks it, perhaps for good), so: `busy` is cleared only when the call RETURNS; a caller that stops waiting
/// calls [`Worker::abandon`], and the late result is dropped when it comes; nothing new starts in between.
pub struct Worker<T> { tx: mpsc::Sender<(u64, T)>, rx: mpsc::Receiver<(u64, T)>, generation: u64, busy: bool, started: Option<Instant> }
impl<T: Send + 'static> Default for Worker<T> { fn default() -> Self { Self::new() } }
impl<T: Send + 'static> Worker<T> {
    pub fn new() -> Self { let (tx, rx) = mpsc::channel(); Worker { tx, rx, generation: 0, busy: false, started: None } }
    /// Starts `job` on a thread of its own; false (and nothing started) while an earlier call has not returned.
    pub fn start(&mut self, job: impl FnOnce() -> T + Send + 'static) -> bool {
        self.drain();
        if self.busy { return false }
        self.busy = true; self.started = Some(Instant::now());
        let (tx, generation) = (self.tx.clone(), self.generation);
        std::thread::spawn(move || { let _ = tx.send((generation, job())); });
        true
    }
    fn drain(&mut self) -> Option<T> { let mut out = None; while let Ok((g, v)) = self.rx.try_recv() { self.busy = false; self.started = None; if g == self.generation { out = Some(v); } } out }
    /// The result, once the call has returned (None while it runs, and for a call that was abandoned).
    pub fn poll(&mut self) -> Option<T> { self.drain() }
    /// Stop waiting for the running call: its result will be dropped. The worker stays busy until it returns.
    pub fn abandon(&mut self) { if self.busy { self.generation += 1; } }
    pub fn busy(&mut self) -> bool { self.drain(); self.busy }
    /// How long the running call has been out.
    pub fn waiting(&self) -> Option<Duration> { self.started.map(|t| t.elapsed()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    fn scratch(name: &str) -> PathBuf { let d = std::env::temp_dir().join(format!("mn-share-{name}-{}", uuid::Uuid::new_v4().simple())); fs::create_dir_all(d.join("share")).unwrap(); fs::create_dir_all(d.join("local")).unwrap(); d }
    fn only(dir: &Path) -> Vec<String> { let mut v: Vec<String> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect(); v.sort(); v }

    #[test]
    fn a_put_replaces_the_document_whole_and_leaves_nothing_beside_it() {
        let d = scratch("put"); let (doc, new) = (d.join("share/notes.mnd"), d.join("local/publish.mnd"));
        fs::write(&new, "version one").unwrap();
        // nothing may be there yet: expect None
        let one = put(&new, &doc, None).unwrap();
        assert_eq!((fs::read_to_string(&doc).unwrap(), only(&d.join("share")), fingerprint(&doc).unwrap()), ("version one".into(), vec!["notes.mnd".to_string()], one));
        fs::write(&new, "version two, longer").unwrap();
        let two = put(&new, &doc, Some(one)).unwrap();
        assert_eq!((fs::read_to_string(&doc).unwrap(), only(&d.join("share"))), ("version two, longer".into(), vec!["notes.mnd".to_string()]));
        assert!(two != one && fs::read_to_string(&new).unwrap() == "version two, longer");          // the local file is copied, not moved
        fs::remove_dir_all(d).unwrap();
    }
    /// UFB's lesson: a save that slips in before the rename is a lost race — reported, and nothing overwritten.
    #[test]
    fn a_put_refuses_when_the_document_is_not_the_one_expected() {
        let d = scratch("race"); let (doc, new) = (d.join("share/notes.mnd"), d.join("local/publish.mnd"));
        fs::write(&doc, "the document we know").unwrap(); let known = fingerprint(&doc).unwrap();
        fs::write(&new, "our new version").unwrap();
        fs::write(&doc, "someone else's save").unwrap();                                             // same second, perhaps the same size: content tells
        assert!(matches!(put(&new, &doc, Some(known)), Err(PutError::Raced)));
        assert_eq!((fs::read_to_string(&doc).unwrap(), only(&d.join("share"))), ("someone else's save".into(), vec!["notes.mnd".to_string()]));
        // a document that appeared where none was expected, and one that went missing where one was
        assert!(matches!(put(&new, &doc, None), Err(PutError::Raced)));
        fs::remove_file(&doc).unwrap();
        assert!(matches!(put(&new, &doc, Some(known)), Err(PutError::Raced)));
        assert!(only(&d.join("share")).is_empty());                                                  // "missing" never causes a write
        // the folder is gone (a dropped mount): an error, the local file untouched
        fs::remove_dir_all(d.join("share")).unwrap();
        assert!(matches!(put(&new, &doc, None), Err(PutError::Io(_))));
        assert_eq!(fs::read_to_string(&new).unwrap(), "our new version");
        fs::remove_dir_all(d).unwrap();
    }
    #[test]
    fn a_fetch_reports_what_it_actually_read_and_absence_is_not_an_error_only_where_asked() {
        let d = scratch("fetch"); let doc = d.join("share/notes.mnd"); fs::write(&doc, "theirs").unwrap();
        let got = fetch(&doc, &d.join("local/theirs.mnd")).unwrap();
        assert_eq!((got, fs::read_to_string(d.join("local/theirs.mnd")).unwrap()), (fingerprint(&doc).unwrap(), "theirs".into()));
        assert_eq!(fingerprint_if_present(&d.join("share/none.mnd")).unwrap(), None);
        assert!(fetch(&d.join("share/none.mnd"), &d.join("local/x")).is_err());
        assert!(fingerprint_if_present(&doc.join("under-a-file")).is_err());                         // cannot be asked ≠ absent
        assert_ne!(fingerprint(&doc).unwrap(), { fs::write(&doc, "theirz").unwrap(); fingerprint(&doc).unwrap() });   // same length, other content
        fs::remove_dir_all(d).unwrap();
    }
    /// UFB's lesson: a time limit does not end a call. One latch, cleared by the returning call; late results dropped.
    #[test]
    fn the_worker_runs_one_call_at_a_time_and_drops_what_was_abandoned() {
        let mut w: Worker<&'static str> = Worker::new();
        let (go, wait) = mpsc::channel::<()>();
        assert!(w.start(move || { let _ = wait.recv(); "slow" }));
        assert!(!w.start(|| "second") && w.busy() && w.poll().is_none() && w.waiting().is_some());   // nothing new while the first is out
        w.abandon();                                                                                 // the user stops waiting…
        assert!(w.busy() && !w.start(|| "third"));                                                   // …but the call is still out there
        go.send(()).unwrap();
        let t = Instant::now(); while w.busy() && t.elapsed() < Duration::from_secs(5) { std::thread::sleep(Duration::from_millis(2)); }
        assert!(!w.busy() && w.poll().is_none());                                                    // it returned; its result was dropped
        assert!(w.start(|| "fresh"));
        let t = Instant::now(); let mut got = None; while got.is_none() && t.elapsed() < Duration::from_secs(5) { got = w.poll(); std::thread::sleep(Duration::from_millis(2)); }
        assert_eq!((got, w.busy(), w.waiting().is_none()), (Some("fresh"), false, true));
    }
}
