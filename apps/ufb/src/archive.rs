//! Archives (canon row 508, points 5-9): Extract Here and Compress to ZIP, by our own code - no tool is bundled.
//! Read: zip (deflate, stored; ZipCrypto and AES with a password), 7z (LZMA, LZMA2, BZip2, Deflate, PPMd; AES with a
//! password), tar - plain or through gzip, bzip2, xz, zstd - and a single file compressed by one of those four.
//! RAR, cab and iso go through the system's own archive tool (spike S20; a locked RAR is not opened by it).
//! Written: ZIP alone (1.4.2).
//!
//! The rules, 1.4.2's where it had them:
//! - **Nothing is overwritten.** An archive is unpacked into a folder of our own beside it (`.<name>.ufb-partial`) and
//!   only what is whole is given its name: a single top entry whose name is free goes straight beside the archive;
//!   anything else lands in a new folder named after the archive (`shots`, `shots 2`, ...). A ZIP is written under a
//!   partial name and renamed when it is whole (`plate.zip` for one item, `Archive.zip` for several, ` 2` when taken).
//! - **A path in an archive is a relative path below where it is unpacked, or it is not unpacked**: an absolute one,
//!   one with `..` or with a colon in a part is left out and counted. A link is made only when it points to a place
//!   inside what is unpacked (never on Windows, where making one needs a right the user seldom has); hard links and
//!   devices are left out.
//! - **Stopped or failed: what was made is taken away** - it lay under the partial name, never among the user's files.
use std::collections::BTreeSet;
use std::io::{BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wrap { None, Gz, Bz2, Xz, Zst }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format { Zip, SevenZ, Tar(Wrap), /// one file, compressed
    Single(Wrap), /// RAR, cab, iso: through the system's own archive tool (spike S20)
    System }

/// The format by the name's end (letters' case apart), and the name without it (`shots.tar.gz` -> `shots`).
pub fn format_of(name: &str) -> Option<(Format, String)> {
    let low = name.to_lowercase();
    const ENDS: [(&str, Format); 19] = [(".rar", Format::System), (".cab", Format::System), (".iso", Format::System), (".tar.gz", Format::Tar(Wrap::Gz)), (".tgz", Format::Tar(Wrap::Gz)), (".tar.bz2", Format::Tar(Wrap::Bz2)), (".tbz2", Format::Tar(Wrap::Bz2)), (".tbz", Format::Tar(Wrap::Bz2)), (".tar.xz", Format::Tar(Wrap::Xz)), (".txz", Format::Tar(Wrap::Xz)),
        (".tar.zst", Format::Tar(Wrap::Zst)), (".tzst", Format::Tar(Wrap::Zst)), (".tar", Format::Tar(Wrap::None)), (".zip", Format::Zip), (".7z", Format::SevenZ), (".gz", Format::Single(Wrap::Gz)), (".bz2", Format::Single(Wrap::Bz2)), (".xz", Format::Single(Wrap::Xz)), (".zst", Format::Single(Wrap::Zst))];
    ENDS.iter().find(|(end, _)| low.ends_with(end) && low.len() > end.len()).map(|(end, f)| (*f, name[..name.len() - end.len()].to_string()))
}
pub fn is_archive(name: &str) -> bool { format_of(name).is_some() }

#[derive(Debug, PartialEq)]
pub enum Fail { /// it needs a password, or the one given is not the one
    Password, Stopped, Other(String) }
impl From<std::io::Error> for Fail { fn from(e: std::io::Error) -> Fail { Fail::Other(format!("{}.", crate::lister::plain(&e))) } }

/// How a job tells how far it is: the share done (none: not known), what it has in hand.
pub type Say<'a> = &'a mut dyn FnMut(Option<f64>, &str);

struct Sink<'a> { root: PathBuf, cancel: &'a AtomicBool, say: Say<'a>, done: u64, total: u64, /// (read so far, of) when the share is told by the archive's own bytes (a tar knows no total)
    pos: Option<(Arc<AtomicU64>, u64)>, tops: BTreeSet<String>, skipped: usize, said: Instant }
impl Sink<'_> {
    /// Where an entry goes: below the root, or nowhere.
    fn place(&mut self, name: &str) -> Option<PathBuf> {
        let parts: Vec<&str> = name.split(['/', '\\']).filter(|p| !p.is_empty() && *p != ".").collect();
        if parts.is_empty() { return None }
        if name.starts_with('/') || name.starts_with('\\') || parts.iter().any(|p| *p == ".." || p.contains(':') || p.chars().any(|c| c.is_control())) { self.skipped += 1; return None }
        self.tops.insert(parts[0].to_string());
        Some(parts.iter().fold(self.root.clone(), |p, part| p.join(part)))
    }
    fn tell(&mut self, name: &str, force: bool) {
        if !force && self.said.elapsed() < Duration::from_millis(120) { return } self.said = Instant::now();
        let frac = match &self.pos { Some((at, of)) if *of > 0 => Some(at.load(Ordering::Relaxed) as f64 / *of as f64), Some(_) => None, None => (self.total > 0).then(|| self.done as f64 / self.total as f64) };
        (self.say)(frac.map(|f| f.clamp(0.0, 1.0)), name);
    }
    fn dir(&mut self, name: &str) -> Result<(), Fail> { if let Some(p) = self.place(name) { std::fs::create_dir_all(p)?; } Ok(()) }
    fn file(&mut self, name: &str, from: &mut dyn Read, mode: Option<u32>, modified: Option<SystemTime>) -> Result<(), Fail> {
        if self.cancel.load(Ordering::Relaxed) { return Err(Fail::Stopped) }
        let Some(p) = self.place(name) else { return Ok(()) };
        if let Some(up) = p.parent() { std::fs::create_dir_all(up)?; }
        let short = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(); self.tell(&short, false);
        // (a name that is there twice in one archive: the first stays)
        let mut out = match std::fs::OpenOptions::new().write(true).create_new(true).open(&p) { Ok(f) => f, Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => { self.skipped += 1; return Ok(()) } Err(e) => return Err(e.into()) };
        let mut buf = vec![0u8; 256 * 1024];
        loop {
            if self.cancel.load(Ordering::Relaxed) { return Err(Fail::Stopped) }
            let n = match from.read(&mut buf) { Ok(0) => break, Ok(n) => n, Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue, Err(e) => return Err(e.into()) };
            out.write_all(&buf[..n])?; self.done += n as u64; self.tell(&short, false);
        }
        if let Some(t) = modified { let _ = out.set_modified(t); }
        drop(out);
        #[cfg(unix)] if let Some(m) = mode { use std::os::unix::fs::PermissionsExt; let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode((m & 0o777) | 0o600)); }
        #[cfg(not(unix))] let _ = mode;
        Ok(())
    }
    /// A link of an archive: made when what it points to lies inside what is unpacked; left out otherwise.
    fn link(&mut self, name: &str, target: &str) {
        let Some(p) = self.place(name) else { return };
        let inside = { let t = Path::new(target); !t.is_absolute() && !target.contains(':') && { let mut at: Vec<Component> = p.parent().unwrap_or(&self.root).strip_prefix(&self.root).map(|r| r.components().collect()).unwrap_or_default(); let mut ok = true;
            for c in t.components() { match c { Component::ParentDir => if at.pop().is_none() { ok = false; break }, Component::Normal(_) => at.push(c), Component::CurDir => {} _ => { ok = false; break } } } ok } };
        if !inside { self.skipped += 1; return }
        #[cfg(unix)] { if let Some(up) = p.parent() { let _ = std::fs::create_dir_all(up); } if std::os::unix::fs::symlink(target, &p).is_err() { self.skipped += 1; } }
        #[cfg(not(unix))] { self.skipped += 1; }
    }
}

struct Counting<R> { inner: R, at: Arc<AtomicU64> }
impl<R: Read> Read for Counting<R> { fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> { let n = self.inner.read(buf)?; self.at.fetch_add(n as u64, Ordering::Relaxed); Ok(n) } }
fn unwrapped<'a>(wrap: Wrap, r: impl Read + 'a) -> Result<Box<dyn Read + 'a>, Fail> {
    Ok(match wrap { Wrap::None => Box::new(r), Wrap::Gz => Box::new(flate2::read::MultiGzDecoder::new(r)), Wrap::Bz2 => Box::new(bzip2::read::MultiBzDecoder::new(r)), Wrap::Xz => Box::new(lzma_rust2::XzReader::new(r, true)),
        Wrap::Zst => Box::new(ruzstd::decoding::StreamingDecoder::new(r).map_err(|e| Fail::Other(format!("It is not a zstd file ({e}).")))?) })
}
fn secs(t: u64) -> Option<SystemTime> { (t > 0).then(|| UNIX_EPOCH + Duration::from_secs(t)) }

/// What an extract made, and how many entries were left out (unsafe paths, links leading outside, names there twice).
#[derive(Debug, PartialEq)]
pub struct Extracted { pub made: PathBuf, pub skipped: usize }

/// Extract Here: beside the archive, by the rules at the top.
pub fn extract(archive: &Path, password: Option<&str>, cancel: &AtomicBool, say: Say) -> Result<Extracted, Fail> {
    let name = archive.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let (format, stem) = format_of(&name).ok_or_else(|| Fail::Other("It is not an archive this application reads.".into()))?;
    let parent = archive.parent().ok_or_else(|| Fail::Other("The archive has no folder.".into()))?.to_path_buf();
    let root = parent.join(format!(".{stem}.ufb-partial"));
    if root.exists() { std::fs::remove_dir_all(&root)?; }
    std::fs::create_dir(&root)?;
    let mut sink = Sink { root: root.clone(), cancel, say, done: 0, total: 0, pos: None, tops: BTreeSet::new(), skipped: 0, said: Instant::now() - Duration::from_secs(1) };
    let r = match format { Format::Zip => unzip(archive, password, &mut sink), Format::SevenZ => un7z(archive, password, &mut sink), Format::Tar(w) => untar(archive, w, &mut sink), Format::Single(w) => unsingle(archive, w, &stem, &mut sink), Format::System => unsystem(archive, &mut sink) };
    if let Err(e) = r { let _ = std::fs::remove_dir_all(&root); return Err(if cancel.load(Ordering::Relaxed) { Fail::Stopped } else { e }) }
    // its place: a single top entry whose name is free goes straight beside the archive; anything else into a new folder
    let made = (|| -> std::io::Result<PathBuf> {
        if sink.tops.len() == 1 { let top = sink.tops.iter().next().unwrap(); let to = parent.join(top);
            if std::fs::symlink_metadata(root.join(top)).is_ok() && std::fs::symlink_metadata(&to).is_err() && crate::ops::rename_no_replace(&root.join(top), &to).is_ok() { let _ = std::fs::remove_dir(&root); return Ok(to) } }
        for n in 1..10_000 { let to = parent.join(if n == 1 { stem.clone() } else { format!("{stem} {n}") });
            if std::fs::symlink_metadata(&to).is_ok() { continue }
            match crate::ops::rename_no_replace(&root, &to) { Ok(()) => return Ok(to), Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue, Err(e) => return Err(e) } }
        Err(std::io::Error::other("no free name"))
    })();
    match made { Ok(made) => Ok(Extracted { made, skipped: sink.skipped }), Err(e) => { let _ = std::fs::remove_dir_all(&root); Err(e.into()) } }
}

fn unzip(archive: &Path, password: Option<&str>, sink: &mut Sink) -> Result<(), Fail> {
    use zip::result::ZipError;
    let bad = |e: ZipError| match e { ZipError::InvalidPassword => Fail::Password, ZipError::UnsupportedArchive(w) if w == ZipError::PASSWORD_REQUIRED => Fail::Password, ZipError::Io(e) => e.into(), e => Fail::Other(format!("The archive cannot be read ({e}).")) };
    let mut z = zip::ZipArchive::new(BufReader::new(std::fs::File::open(archive)?)).map_err(bad)?;
    sink.total = z.decompressed_size().map(|n| n.min(u64::MAX as u128) as u64).unwrap_or(0);
    for i in 0..z.len() {
        let mut f = match password { Some(p) => z.by_index_decrypt(i, p.as_bytes()), None => z.by_index(i) }.map_err(bad)?;
        let (name, locked) = (f.name().map(|n| n.into_owned()).map_err(bad)?, f.encrypted());
        if f.is_dir() { sink.dir(&name)?; continue }
        if f.is_symlink() { let mut t = String::new(); let _ = f.read_to_string(&mut t); sink.link(&name, &t); continue }
        let modified = f.extra_data_fields().find_map(|x| match x { zip::extra_fields::ExtraField::ExtendedTimestamp(t) => t.mod_time().and_then(|s| secs(s as u64)), _ => None });
        let mode = f.unix_mode();
        // (a wrong password for the old kind of lock is seen only when the bytes do not add up)
        match sink.file(&name, &mut f, mode, modified) { Err(Fail::Other(_)) if locked => return Err(Fail::Password), r => r? }
    }
    Ok(())
}

fn untar(archive: &Path, wrap: Wrap, sink: &mut Sink) -> Result<(), Fail> {
    let file = std::fs::File::open(archive)?; let of = file.metadata().map(|m| m.len()).unwrap_or(0); let at = Arc::new(AtomicU64::new(0));
    sink.pos = Some((at.clone(), of));
    let mut tar = tar::Archive::new(unwrapped(wrap, Counting { inner: BufReader::new(file), at })?);
    for entry in tar.entries()? {
        let mut e = entry?; let name = String::from_utf8_lossy(&e.path_bytes()).into_owned(); let kind = e.header().entry_type();
        let (mode, modified) = (e.header().mode().ok(), e.header().mtime().ok().and_then(secs));
        if kind.is_dir() { sink.dir(&name)?; } else if kind.is_file() || kind == tar::EntryType::Continuous { sink.file(&name, &mut e, mode, modified)?; }
        else if kind.is_symlink() { match e.link_name_bytes() { Some(t) => { let t = String::from_utf8_lossy(&t).into_owned(); sink.link(&name, &t) } None => sink.skipped += 1 } }
        else if !matches!(kind, tar::EntryType::XGlobalHeader | tar::EntryType::XHeader | tar::EntryType::GNULongName | tar::EntryType::GNULongLink) { sink.skipped += 1; }
    }
    Ok(())
}

fn unsingle(archive: &Path, wrap: Wrap, stem: &str, sink: &mut Sink) -> Result<(), Fail> {
    let file = std::fs::File::open(archive)?; let of = file.metadata().map(|m| m.len()).unwrap_or(0); let at = Arc::new(AtomicU64::new(0));
    sink.pos = Some((at.clone(), of));
    let mut r = unwrapped(wrap, Counting { inner: BufReader::new(file), at })?;
    sink.file(stem, &mut r, None, None)
}

/// The system's own archive tool (libarchive's `bsdtar`: `/usr/bin/bsdtar` on macOS, `tar.exe` of Windows 10 1803 and
/// later), as a child: it lists the archive, then unpacks it into our folder - it refuses absolute paths and `..` by
/// itself. A stop ends the child. A locked RAR is not opened by it (S20): said as such.
fn system_tool() -> Option<PathBuf> {
    #[cfg(windows)] let p = PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into())).join("System32").join("tar.exe");
    #[cfg(not(windows))] let p = PathBuf::from("/usr/bin/bsdtar");
    p.is_file().then_some(p)
}
fn unsystem(archive: &Path, sink: &mut Sink) -> Result<(), Fail> {
    use std::io::BufRead; use std::process::{Command, Stdio};
    let tool = system_tool().ok_or_else(|| Fail::Other("This system has no tool that reads this kind of archive.".into()))?;
    let quiet = |c: &mut Command| { #[cfg(windows)] { use std::os::windows::process::CommandExt; c.creation_flags(0x0800_0000); } let _ = c; };
    let mut list = Command::new(&tool); list.arg("-tf").arg(archive).stdin(Stdio::null()).stderr(Stdio::null()); quiet(&mut list);
    let count = list.output().map(|o| o.stdout.iter().filter(|b| **b == b'\n').count()).unwrap_or(0);
    let mut x = Command::new(&tool); x.arg("-xvf").arg(archive).arg("-C").arg(&sink.root).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()); quiet(&mut x);
    let mut child = x.spawn()?; let err = child.stderr.take().ok_or_else(|| Fail::Other("The system's tool could not be started.".into()))?;
    // Its lines are the entries as they are unpacked: `x name`, or `x name: complaint`. An entry it "can't create" is
    // a link this user may not make (Windows without the right for it): left out and counted, as our own code leaves
    // links out there. Any other complaint is a failure of the archive (a locked RAR: every entry complains).
    let (mut seen, mut left, mut fatal, mut said) = (0usize, 0usize, false, String::new());
    for line in BufReader::new(err).split(b'\n').flatten() {
        if sink.cancel.load(Ordering::Relaxed) { let _ = child.kill(); let _ = child.wait(); return Err(Fail::Stopped) }
        let line = String::from_utf8_lossy(&line).trim().to_string();
        match line.strip_prefix("x ") {
            Some(rest) => { seen += 1; let name = match rest.split_once(": ") { Some((name, why)) => { if why.starts_with("Can't create") { left += 1; } else { fatal = true; if said.is_empty() { said = line.clone(); } } name } None => rest };
                let short = name.rsplit(['/', '\\']).find(|p| !p.is_empty()).unwrap_or(name).to_string(); if sink.said.elapsed() >= Duration::from_millis(120) { sink.said = Instant::now(); (sink.say)((count > 0).then(|| (seen as f64 / count as f64).min(1.0)), &short); } }
            None => if !line.is_empty() && said.is_empty() && !line.contains("Error exit delayed") { said = line }
        }
    }
    let ok = child.wait()?.success();
    if sink.cancel.load(Ordering::Relaxed) { return Err(Fail::Stopped) }
    if !ok && (fatal || left == 0 || left >= seen) { return Err(Fail::Other(format!("The system's archive tool could not unpack it{} A RAR locked with a password is not opened by it.", if said.is_empty() { ".".to_string() } else { format!(" ({said}).") }))) }
    sink.skipped += left;
    for e in std::fs::read_dir(&sink.root)?.flatten() { sink.tops.insert(e.file_name().to_string_lossy().into_owned()); }
    Ok(())
}

fn un7z(archive: &Path, password: Option<&str>, sink: &mut Sink) -> Result<(), Fail> {
    use sevenz_rust2::{ArchiveReader, Error, Password};
    let bad = |e: Error| match e { Error::PasswordRequired | Error::MaybeBadPassword(_) => Fail::Password, Error::Io(e, _) | Error::FileOpen(e, _) => e.into(), Error::UnsupportedCompressionMethod(m) => Fail::Other(format!("The archive is packed in a way this application does not read ({m}).")), e => Fail::Other(format!("The archive cannot be read ({e}).")) };
    let mut r = ArchiveReader::open(archive, password.map(Password::new).unwrap_or_else(Password::empty)).map_err(bad)?;
    sink.total = r.archive().files.iter().map(|f| f.size()).sum();
    let mut fail: Option<Fail> = None;
    let res = r.for_each_entries(|entry, from| {
        let name = entry.name().to_string();
        let done = if entry.is_directory() { sink.dir(&name) } else { sink.file(&name, from, None, SystemTime::try_from(entry.last_modified_date()).ok().filter(|t| *t > UNIX_EPOCH)) };
        match done { Ok(()) => Ok(true), Err(e) => { fail = Some(e); Err(Error::Other("stopped".into())) } }
    });
    match (fail, res) { (Some(Fail::Other(_)), _) if password.is_some() => Err(Fail::Password), (Some(f), _) => Err(f), (None, Err(e)) => Err(bad(e)), (None, Ok(())) => Ok(()) }
}

/// The ZIP's name for what is compressed, and the folder it is written in: beside the item nearest the root.
pub fn zip_place(paths: &[PathBuf]) -> Option<(PathBuf, String)> {
    let first = paths.iter().min_by_key(|p| p.components().count())?; let parent = first.parent()?.to_path_buf();
    let name = if paths.len() == 1 { format!("{}.zip", first.file_name()?.to_string_lossy()) } else { "Archive.zip".to_string() };
    Some((parent, name))
}

/// Compress to ZIP. Each item goes in under its own name; where two of them share a name, under their paths from the
/// folder they have in common. `.DS_Store` is left out.
pub fn compress(paths: &[PathBuf], cancel: &AtomicBool, say: Say) -> Result<PathBuf, Fail> {
    use zip::write::SimpleFileOptions;
    let (parent, name) = zip_place(paths).ok_or_else(|| Fail::Other("There is nothing to compress.".into()))?;
    let partial = parent.join(format!(".{name}.ufb-partial"));
    // the names inside
    let base = |p: &PathBuf| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let clash = paths.iter().enumerate().any(|(i, p)| paths[..i].iter().any(|q| base(q).eq_ignore_ascii_case(&base(p))));
    let common = { let mut c: Vec<Component> = paths[0].parent().map(|p| p.components().collect()).unwrap_or_default(); for p in paths { let pc: Vec<Component> = p.parent().map(|p| p.components().collect()).unwrap_or_default(); let n = c.iter().zip(&pc).take_while(|(a, b)| a == b).count(); c.truncate(n); } c.iter().collect::<PathBuf>() };
    let inside = |p: &PathBuf| if clash { p.strip_prefix(&common).map(|r| r.to_string_lossy().replace('\\', "/")).unwrap_or_else(|_| base(p)) } else { base(p) };
    let mut total = 0u64; { fn measure(p: &Path, n: &mut u64, cancel: &AtomicBool) { if cancel.load(Ordering::Relaxed) { return } match std::fs::symlink_metadata(p) { Ok(m) if m.is_dir() => if let Ok(rd) = std::fs::read_dir(p) { for e in rd.flatten() { measure(&e.path(), n, cancel); } }, Ok(m) if m.is_file() => *n += m.len(), _ => {} } } for p in paths { measure(p, &mut total, cancel); } }
    struct Run<'a> { zip: zip::ZipWriter<std::io::BufWriter<std::fs::File>>, cancel: &'a AtomicBool, say: Say<'a>, done: u64, total: u64, said: Instant, buf: Vec<u8> }
    impl Run<'_> {
        fn add(&mut self, p: &Path, name: &str) -> Result<(), Fail> {
            if self.cancel.load(Ordering::Relaxed) { return Err(Fail::Stopped) }
            if p.file_name().is_some_and(|n| n == ".DS_Store") { return Ok(()) }
            let m = std::fs::symlink_metadata(p)?; let zerr = |e: zip::result::ZipError| match e { zip::result::ZipError::Io(e) => Fail::from(e), e => Fail::Other(format!("The archive could not be written ({e}).")) };
            let when = m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| crate::format::local(d.as_secs() as i64)).and_then(|c| zip::DateTime::from_date_and_time(c.year.clamp(1980, 2107) as u16, c.month as u8, c.day as u8, c.hour as u8, c.minute as u8, 0).ok());
            let mut o = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated); if let Some(w) = when { o = o.last_modified_time(w); }
            #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; o = o.unix_permissions(m.permissions().mode() & 0o777); }
            if m.file_type().is_symlink() { let t = std::fs::read_link(p)?; self.zip.add_symlink(name, t.to_string_lossy(), o).map_err(zerr)?; }
            else if m.is_dir() { self.zip.add_directory(format!("{name}/"), o).map_err(zerr)?;
                let mut inner: Vec<PathBuf> = std::fs::read_dir(p)?.flatten().map(|e| e.path()).collect(); inner.sort();
                for c in inner { let n = format!("{name}/{}", c.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()); self.add(&c, &n)?; } }
            else if m.is_file() { self.zip.start_file(name, o.large_file(m.len() >= 0xFFFF_0000)).map_err(zerr)?;
                let mut f = std::fs::File::open(p)?; let short = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                loop { if self.cancel.load(Ordering::Relaxed) { return Err(Fail::Stopped) }
                    let n = match f.read(&mut self.buf) { Ok(0) => break, Ok(n) => n, Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue, Err(e) => return Err(e.into()) };
                    self.zip.write_all(&self.buf[..n])?; self.done += n as u64;
                    if self.said.elapsed() >= Duration::from_millis(120) { self.said = Instant::now(); (self.say)((self.total > 0).then(|| (self.done as f64 / self.total as f64).min(1.0)), &short); } } }
            Ok(())
        }
    }
    let _ = std::fs::remove_file(&partial);
    let mut run = Run { zip: zip::ZipWriter::new(std::io::BufWriter::new(std::fs::File::create(&partial)?)), cancel, say, done: 0, total, said: Instant::now() - Duration::from_secs(1), buf: vec![0u8; 256 * 1024] };
    let done = (|| -> Result<PathBuf, Fail> {
        for p in paths { run.add(p, &inside(p))?; }
        run.zip.finish().map_err(|e| Fail::Other(format!("The archive could not be written ({e}).")))?.into_inner().map_err(|e| Fail::from(e.into_error()))?.sync_all()?;
        let (stem, ext) = (name.trim_end_matches(".zip"), ".zip");
        for n in 1..10_000 { let to = parent.join(if n == 1 { name.clone() } else { format!("{stem} {n}{ext}") });
            if std::fs::symlink_metadata(&to).is_ok() { continue }
            match crate::ops::rename_no_replace(&partial, &to) { Ok(()) => return Ok(to), Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue, Err(e) => return Err(e.into()) } }
        Err(Fail::Other("No free name for the archive.".into()))
    })();
    if done.is_err() { let _ = std::fs::remove_file(&partial); }
    done.map_err(|e| if cancel.load(Ordering::Relaxed) { Fail::Stopped } else { e })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lister::tests::Temp;
    fn names(dir: &Path) -> Vec<String> { let mut v: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect(); v.sort(); v }
    fn go() -> AtomicBool { AtomicBool::new(false) }

    #[test]
    fn the_formats_by_their_names() {
        assert_eq!(format_of("Shots.TAR.GZ"), Some((Format::Tar(Wrap::Gz), "Shots".into()))); assert_eq!(format_of("a.b.zip"), Some((Format::Zip, "a.b".into()))); assert_eq!(format_of("x.7z").map(|f| f.0), Some(Format::SevenZ));
        assert_eq!(format_of("log.txt.gz"), Some((Format::Single(Wrap::Gz), "log.txt".into()))); assert_eq!(format_of("x.tzst").map(|f| f.0), Some(Format::Tar(Wrap::Zst)));
        assert!(format_of(".zip").is_none() && format_of("plate.exr").is_none()); assert_eq!(format_of("x.part01.RAR"), Some((Format::System, "x.part01".into())));
    }
    #[test]
    fn a_zip_is_made_and_unpacked_and_nothing_is_overwritten() {
        let t = Temp::new("arc-zip"); t.dir("shots/010"); t.file("shots/010/comp.nk", 3000); t.file("shots/.DS_Store", 10); t.file("a.txt", 5); t.dir("out");
        let said = std::cell::Cell::new(0); let mut say = |_: Option<f64>, _: &str| said.set(said.get() + 1);
        // several items: Archive.zip beside them, then Archive 2.zip
        let z = compress(&[t.0.join("shots"), t.0.join("a.txt")], &go(), &mut say).unwrap(); assert_eq!(z, t.0.join("Archive.zip"));
        assert_eq!(compress(&[t.0.join("shots"), t.0.join("a.txt")], &go(), &mut say).unwrap(), t.0.join("Archive 2.zip"));
        // one item: its own name
        assert_eq!(compress(&[t.0.join("a.txt")], &go(), &mut say).unwrap(), t.0.join("a.txt.zip"));
        assert!(!names(&t.0).iter().any(|n| n.contains("ufb-partial")));
        // two top entries: a new folder named after the archive; again: "Archive 2"
        let x = extract(&z, None, &go(), &mut say).unwrap(); assert_eq!(x, Extracted { made: t.0.join("Archive"), skipped: 0 });
        assert_eq!(names(&x.made), ["a.txt", "shots"]); assert_eq!(names(&x.made.join("shots")), ["010"], "no .DS_Store went in"); assert_eq!(std::fs::metadata(x.made.join("shots/010/comp.nk")).unwrap().len(), 3000);
        assert_eq!(extract(&z, None, &go(), &mut say).unwrap().made, t.0.join("Archive 2"));
        // one top entry whose name is free: straight beside the archive; taken: into a folder
        std::fs::rename(t.0.join("a.txt.zip"), t.0.join("out/a.txt.zip")).unwrap();
        assert_eq!(extract(&t.0.join("out/a.txt.zip"), None, &go(), &mut say).unwrap().made, t.0.join("out/a.txt")); assert!(t.0.join("out/a.txt").is_file());
        let again = extract(&t.0.join("out/a.txt.zip"), None, &go(), &mut say).unwrap().made; assert_eq!(again, t.0.join("out/a.txt 2")); assert!(again.join("a.txt").is_file());
        assert!(!names(&t.0.join("out")).iter().any(|n| n.contains("ufb-partial"))); assert!(said.get() > 0);
        // called off: nothing is left
        let stop = AtomicBool::new(true); assert_eq!(extract(&z, None, &stop, &mut say), Err(Fail::Stopped)); assert_eq!(compress(&[t.0.join("shots")], &stop, &mut say), Err(Fail::Stopped));
        assert!(!names(&t.0).iter().any(|n| n.contains("ufb-partial") || n == "Archive 3" || n == "shots.zip"));
    }
    #[test]
    fn a_path_that_leads_outside_is_left_out() {
        use zip::write::SimpleFileOptions;
        let t = Temp::new("arc-unsafe"); t.dir("in");
        let mut z = zip::ZipWriter::new(std::fs::File::create(t.0.join("in/x.zip")).unwrap());
        for n in ["../evil.txt", "/abs.txt", "ok/fine.txt", "ok/../../evil2.txt", "C:/win.txt"] { z.start_file(n, SimpleFileOptions::default()).unwrap(); z.write_all(b"x").unwrap(); }
        z.finish().unwrap();
        let x = extract(&t.0.join("in/x.zip"), None, &go(), &mut |_, _| {}).unwrap();
        assert!(!t.0.join("evil.txt").exists() && !t.0.join("evil2.txt").exists() && !t.0.join("in/evil.txt").exists());
        // (the zip writer itself may tidy some of these names: whatever it wrote, only what lies below is there)
        fn walk(d: &Path, out: &mut Vec<String>, root: &Path) { for e in std::fs::read_dir(d).unwrap().flatten() { let p = e.path(); if p.is_dir() { walk(&p, out, root) } else { out.push(p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/")); } } }
        let mut all = vec![]; walk(&t.0, &mut all, &t.0); all.sort(); assert!(all.iter().all(|p| p.starts_with("in/")), "{all:?}"); assert!(all.iter().any(|p| p.ends_with("fine.txt")));
        assert!(x.made.starts_with(t.0.join("in")));
    }
    #[test]
    fn a_tar_through_gzip_with_its_links() {
        let t = Temp::new("arc-tar"); t.dir("in");
        { let gz = flate2::write::GzEncoder::new(std::fs::File::create(t.0.join("in/pack.tar.gz")).unwrap(), flate2::Compression::fast()); let mut b = tar::Builder::new(gz);
            let file = |b: &mut tar::Builder<_>, name: &str, data: &[u8]| { let mut h = tar::Header::new_gnu(); h.set_size(data.len() as u64); h.set_mode(0o644); h.set_mtime(1_700_000_000); h.set_cksum(); b.append_data(&mut h, name, data).unwrap(); };
            file(&mut b, "pack/a.txt", b"hello"); file(&mut b, "pack/sub/b.txt", b"there");
            let link = |b: &mut tar::Builder<_>, name: &str, to: &str| { let mut h = tar::Header::new_gnu(); h.set_entry_type(tar::EntryType::Symlink); h.set_size(0); h.set_cksum(); b.append_link(&mut h, name, to).unwrap(); };
            link(&mut b, "pack/sub/in.txt", "../a.txt"); link(&mut b, "pack/out.txt", "../../../etc/hosts");
            b.into_inner().unwrap().finish().unwrap(); }
        let x = extract(&t.0.join("in/pack.tar.gz"), None, &go(), &mut |_, _| {}).unwrap();
        assert_eq!(x.made, t.0.join("in/pack")); assert_eq!(std::fs::read(x.made.join("sub/b.txt")).unwrap(), b"there");
        assert_eq!(std::fs::metadata(x.made.join("a.txt")).unwrap().modified().unwrap(), UNIX_EPOCH + Duration::from_secs(1_700_000_000), "the file keeps its time");
        assert!(std::fs::symlink_metadata(x.made.join("out.txt")).is_err(), "a link that leads outside is not made");
        #[cfg(unix)] { assert_eq!(std::fs::read(x.made.join("sub/in.txt")).unwrap(), b"hello", "a link inside is made"); assert_eq!(x.skipped, 1); }
        #[cfg(not(unix))] assert_eq!(x.skipped, 2);
        // one compressed file
        { let mut gz = flate2::write::GzEncoder::new(std::fs::File::create(t.0.join("in/log.txt.gz")).unwrap(), flate2::Compression::fast()); gz.write_all(b"a log").unwrap(); gz.finish().unwrap(); }
        let x = extract(&t.0.join("in/log.txt.gz"), None, &go(), &mut |_, _| {}).unwrap(); assert_eq!(x.made, t.0.join("in/log.txt")); assert_eq!(std::fs::read(&x.made).unwrap(), b"a log");
    }
    #[test]
    fn a_locked_zip_wants_its_password() {
        use zip::write::SimpleFileOptions;
        let t = Temp::new("arc-lock"); t.dir("in");
        let mut z = zip::ZipWriter::new(std::fs::File::create(t.0.join("in/secret.zip")).unwrap());
        z.start_file("secret/a.txt", SimpleFileOptions::default().with_aes_encryption(zip::AesMode::Aes256, "open sesame")).unwrap(); z.write_all(b"the plan").unwrap(); z.finish().unwrap();
        let a = t.0.join("in/secret.zip");
        assert_eq!(extract(&a, None, &go(), &mut |_, _| {}), Err(Fail::Password)); assert_eq!(extract(&a, Some("wrong"), &go(), &mut |_, _| {}), Err(Fail::Password));
        assert_eq!(names(&t.0.join("in")), ["secret.zip"], "nothing is left of a failed attempt");
        let x = extract(&a, Some("open sesame"), &go(), &mut |_, _| {}).unwrap(); assert_eq!(std::fs::read(x.made.join("a.txt")).unwrap(), b"the plan");
    }
}

// ---------------------------------------------------------------- the lane (row 508, point 3)

/// What the lane does: one at a time, beside the engine's file operations (a long archive does not hold up a copy).
pub enum Job { Extract { archive: PathBuf, password: Option<String> }, Compress { paths: Vec<PathBuf> }, /// a video or an image sequence to MP4 (`transcode.rs`)
    Transcode { source: crate::transcode::Source } }
type Sent = (u64, u64, Arc<AtomicBool>, Job);
static LANE: std::sync::OnceLock<std::sync::mpsc::Sender<Sent>> = std::sync::OnceLock::new();

/// Ask for it: a row of the tasks' list from now on. `origin`: the window that asked.
pub fn submit(origin: u64, job: Job) {
    let quoted = |p: &Path| format!("\u{201c}{}\u{201d}", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    let label = match &job { Job::Extract { archive, .. } => format!("Extract {}", quoted(archive)), Job::Compress { paths } => if paths.len() == 1 { format!("Compress {}", quoted(&paths[0])) } else { format!("Compress {} items", paths.len()) }, Job::Transcode { source } => format!("Transcode \u{201c}{}\u{201d} to MP4", crate::transcode::label(source)) };
    let (id, flag) = crate::tasks::add(&label);
    let tx = LANE.get_or_init(|| { let (tx, rx) = std::sync::mpsc::channel::<Sent>(); let _ = std::thread::Builder::new().name("ufb-lane".into()).spawn(move || { while let Ok((origin, id, flag, job)) = rx.recv() { run(origin, id, &flag, job); crate::ops::wake(); } }); tx });
    let _ = tx.send((origin, id, flag, job)); crate::ops::wake();
}
fn run(origin: u64, id: u64, flag: &AtomicBool, job: Job) {
    use crate::ops::{Kind, Op, Report};
    if flag.load(Ordering::Relaxed) { return }
    crate::tasks::start(id);
    let mut say = |f: Option<f64>, what: &str| { crate::tasks::progress(id, f, what); crate::ops::wake(); };
    let parent_of = |p: &Path| p.parent().map(Path::to_path_buf).into_iter().collect::<Vec<_>>();
    match job {
        Job::Extract { archive, password } => match extract(&archive, password.as_deref(), flag, &mut say) {
            Ok(x) => { crate::tasks::finish(id, Ok(Some(x.made.clone())));
                let notice = (x.skipped > 0).then(|| if x.skipped == 1 { "One entry of the archive was left out: its path leads outside, or it is a link that cannot be made here.".to_string() } else { format!("{} entries of the archive were left out: their paths lead outside, or they are links that cannot be made here.", x.skipped) });
                crate::ops::announce(Report { origin, kind: Some(Kind::Extract), changed: parent_of(&x.made), made: vec![x.made.clone()], notice, ..Default::default() });
                crate::ops::submit(origin, Op::Note { made: x.made, label: "Extract" }); }
            // (the row goes: the question takes its place, and the answer is a task of its own)
            Err(Fail::Password) => { crate::tasks::finish(id, Err("It needs its password.".into())); crate::tasks::dismiss(id); crate::ops::announce(Report { origin, kind: Some(Kind::Extract), password: Some((archive, password.is_some())), ..Default::default() }); }
            Err(Fail::Stopped) => crate::tasks::finish(id, Err("Stopped".into())),
            Err(Fail::Other(why)) => { crate::tasks::finish(id, Err(why.clone())); crate::ops::announce(Report { origin, kind: Some(Kind::Extract), error: Some(format!("\u{201c}{}\u{201d} could not be unpacked: {why}", archive.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())), ..Default::default() }); }
        },
        Job::Compress { paths } => match compress(&paths, flag, &mut say) {
            Ok(made) => { crate::tasks::finish(id, Ok(Some(made.clone()))); crate::ops::announce(Report { origin, kind: Some(Kind::Compress), changed: parent_of(&made), made: vec![made.clone()], ..Default::default() }); crate::ops::submit(origin, Op::Note { made, label: "Compress" }); }
            Err(Fail::Stopped) | Err(Fail::Password) => crate::tasks::finish(id, Err("Stopped".into())),
            Err(Fail::Other(why)) => { crate::tasks::finish(id, Err(why.clone())); crate::ops::announce(Report { origin, kind: Some(Kind::Compress), error: Some(format!("The archive could not be made: {why}")), ..Default::default() }); }
        },
        Job::Transcode { source } => match crate::transcode::transcode(&source, flag, &mut say) {
            // (the MP4 folder may be new: the folder the source lies in changed too)
            Ok(made) => { crate::tasks::finish(id, Ok(Some(made.clone()))); let mut changed = parent_of(&made); if let Some(up) = changed.first().and_then(|d| d.parent()).map(Path::to_path_buf) { changed.push(up); }
                crate::ops::announce(Report { origin, kind: Some(Kind::Transcode), changed, made: vec![made.clone()], ..Default::default() }); crate::ops::submit(origin, Op::Note { made, label: "Transcode" }); }
            Err(why) if why == "Stopped" => crate::tasks::finish(id, Err(why)),
            Err(why) => { crate::tasks::finish(id, Err(why.clone())); crate::ops::announce(Report { origin, kind: Some(Kind::Transcode), error: Some(format!("\u{201c}{}\u{201d} could not be transcoded: {why}", crate::transcode::label(&source))), ..Default::default() }); }
        },
    }
}
