//! The pictures kept on disk (canon row 456, point 4): ONE local database in the system's cache location - nothing is
//! ever written into the user's folders. Per file one MASTER (its longer side at most 512 px) that serves every size
//! asked; it is valid while the file's size and modification time are those it was made for.
//!
//! 1.4.2's design, with the three repairs ruled: keyed by VOLUME IDENTITY and the place on the volume (`volume.rs`),
//! not by the path as given; a running total kept by the database itself (1.4.2 added up the whole table at every
//! insert); what is kept for a file that is seen to be gone is dropped ([`Cache::prune`]). And one more: a picture
//! shown is marked as used at most once a day (1.4.2 wrote at every hit).
//!
//! The database is opened by the HELPERS, each with its own connection (write-ahead log, a busy wait): finding the
//! volume, reading a blob and unpacking it all happen where a stall or a damaged row costs one request. The
//! application never opens it.
//!
//! A master is kept as a JPEG when it is opaque, else as a PNG of its premultiplied pixels (lossless: transparency
//! stays exact). "This file has no picture" is kept too (a row without one), so that a damaged file does not end a
//! helper again at every launch; Refresh forgets those of the folder.
use super::helper::Img;
use super::volume::{self, Place};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The longer side of a kept picture.
pub const MASTER: u32 = 512;
/// What the kept pictures may take on disk; over it, those used longest ago go, down to nine tenths (1.4.2's figure).
pub const CAP: u64 = 512 << 20;
/// The database's file: its schema's number is in the name - another schema is another file.
pub const FILE: &str = "thumbs-1.db";
const JPEG: i64 = 1; const PNG: i64 = 2; const NO_PICTURE: i64 = 0;
const DAY: i64 = 86_400;

pub enum Kept { Hit(Img), NoPicture, Miss }

pub struct Cache { db: Connection, places: HashMap<PathBuf, Option<Place>>, cap: u64 }

fn now() -> i64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0) }

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS thumbs(id INTEGER PRIMARY KEY, vol TEXT NOT NULL, dir TEXT NOT NULL, name TEXT NOT NULL, size INTEGER NOT NULL, mtime INTEGER NOT NULL,
  w INTEGER NOT NULL, h INTEGER NOT NULL, fmt INTEGER NOT NULL, data BLOB, bytes INTEGER NOT NULL, used INTEGER NOT NULL);
CREATE UNIQUE INDEX IF NOT EXISTS thumbs_file ON thumbs(vol, dir, name);
CREATE INDEX IF NOT EXISTS thumbs_used ON thumbs(used);
CREATE TABLE IF NOT EXISTS meta(k TEXT PRIMARY KEY, v INTEGER NOT NULL);
INSERT OR IGNORE INTO meta(k, v) VALUES('total', 0);
CREATE TRIGGER IF NOT EXISTS thumbs_added AFTER INSERT ON thumbs BEGIN UPDATE meta SET v = v + NEW.bytes WHERE k = 'total'; END;
CREATE TRIGGER IF NOT EXISTS thumbs_removed AFTER DELETE ON thumbs BEGIN UPDATE meta SET v = v - OLD.bytes WHERE k = 'total'; END;
";

impl Cache {
    /// Open (or make) the database in `dir`. A file that is no database any more is removed and made anew: it is a
    /// cache. None: no cache - every picture is simply made.
    pub fn open(dir: &Path) -> Option<Cache> {
        std::fs::create_dir_all(dir).ok()?;
        let file = dir.join(FILE);
        let cap = std::env::var("UFB_THUMB_CACHE_CAP").ok().and_then(|v| v.parse().ok()).unwrap_or(CAP);
        let try_open = || -> rusqlite::Result<Connection> {
            let db = Connection::open(&file)?;
            db.busy_timeout(std::time::Duration::from_secs(4))?;
            db.pragma_update(None, "journal_mode", "WAL")?; db.pragma_update(None, "synchronous", "NORMAL")?;
            db.execute_batch(SCHEMA)?;
            Ok(db)
        };
        let db = match try_open() {
            Ok(db) => db,
            Err(rusqlite::Error::SqliteFailure(e, _)) if matches!(e.code, rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase) => {
                for ext in ["", "-wal", "-shm"] { let _ = std::fs::remove_file(dir.join(format!("{FILE}{ext}"))); }
                try_open().ok()?
            }
            // (another helper may be making the database at this very moment)
            Err(_) => { std::thread::sleep(std::time::Duration::from_millis(150)); try_open().ok()? }
        };
        Some(Cache { db, places: HashMap::new(), cap })
    }
    /// The volume and the place of a folder; asked of the system once per folder.
    fn place(&mut self, dir: &Path) -> Option<Place> {
        if let Some(p) = self.places.get(dir) { return p.clone() }
        if self.places.len() > 4096 { self.places.clear(); }
        let p = volume::locate(dir); self.places.insert(dir.to_path_buf(), p.clone()); p
    }
    fn file(&mut self, path: &Path) -> Option<(Place, String)> { let name = path.file_name()?.to_string_lossy().into_owned(); Some((self.place(path.parent()?)?, name)) }

    /// What is kept for the file as it is now.
    pub fn lookup(&mut self, path: &Path, size: u64, mtime: i64) -> Kept {
        let Some((at, name)) = self.file(path) else { return Kept::Miss };
        let row = self.db.query_row("SELECT id, size, mtime, w, h, fmt, data, used FROM thumbs WHERE vol = ?1 AND dir = ?2 AND name = ?3", params![at.vol, at.dir, name],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?, r.get::<_, i64>(4)?, r.get::<_, i64>(5)?, r.get::<_, Option<Vec<u8>>>(6)?, r.get::<_, i64>(7)?))).optional();
        let Ok(Some((id, s, m, w, h, fmt, data, used))) = row else { return Kept::Miss };
        // made for the file as it was: not this one's picture (the row is replaced when the new one is kept)
        if s != size as i64 || m != mtime { return Kept::Miss }
        let t = now(); if used < t - DAY { let _ = self.db.execute("UPDATE thumbs SET used = ?1 WHERE id = ?2", params![t, id]); }
        if fmt == NO_PICTURE { return Kept::NoPicture }
        match data.and_then(|d| unpack(fmt, &d, w as u32, h as u32)) { Some(img) => Kept::Hit(img), None => Kept::Miss }
    }
    fn put(&mut self, path: &Path, size: u64, mtime: i64, w: u32, h: u32, fmt: i64, data: Option<&[u8]>) {
        let Some((at, name)) = self.file(path) else { return }; let cap = self.cap;
        let bytes = data.map(|d| d.len()).unwrap_or(0) as i64;
        let Ok(tx) = self.db.transaction_with_behavior(TransactionBehavior::Immediate) else { return };
        let done = (|| -> rusqlite::Result<()> {
            tx.execute("DELETE FROM thumbs WHERE vol = ?1 AND dir = ?2 AND name = ?3", params![at.vol, at.dir, name])?;
            tx.execute("INSERT INTO thumbs(vol, dir, name, size, mtime, w, h, fmt, data, bytes, used) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)", params![at.vol, at.dir, name, size as i64, mtime, w, h, fmt, data, bytes, now()])?;
            // over the cap: those used longest ago go, a lot at a time, down to nine tenths
            let mut total: i64 = tx.query_row("SELECT v FROM meta WHERE k = 'total'", [], |r| r.get(0))?;
            if total as u64 > cap {
                while total as u64 > cap / 10 * 9 {
                    let n = tx.execute("DELETE FROM thumbs WHERE id IN (SELECT id FROM thumbs ORDER BY used, id LIMIT 64)", [])?;
                    total = tx.query_row("SELECT v FROM meta WHERE k = 'total'", [], |r| r.get(0))?;
                    if n == 0 { break }
                }
            }
            Ok(())
        })();
        let _ = if done.is_ok() { tx.commit() } else { tx.rollback() };
    }
    /// Keep a picture (premultiplied, its longer side at most [`MASTER`]) for the file as it is now.
    pub fn keep(&mut self, path: &Path, size: u64, mtime: i64, img: &Img) {
        if let Some((fmt, data)) = pack(img) { self.put(path, size, mtime, img.w, img.h, fmt, Some(&data)); }
    }
    /// Remember that the file, as it is now, has no picture.
    pub fn mark(&mut self, path: &Path, size: u64, mtime: i64) { self.put(path, size, mtime, 0, 0, NO_PICTURE, None); }
    /// Refresh: forget which files of this folder have no picture.
    pub fn forget(&mut self, dir: &Path) {
        self.places.remove(dir);
        if let Some(at) = self.place(dir) { let _ = self.db.execute("DELETE FROM thumbs WHERE vol = ?1 AND dir = ?2 AND fmt = 0", params![at.vol, at.dir]); }
    }
    /// What is kept for files that are no longer in this folder is dropped. The folder's names are read here - only
    /// when something is kept for it at all. Returns how many went.
    pub fn prune(&mut self, dir: &Path) -> usize {
        let Some(at) = self.place(dir) else { return 0 };
        let kept: Vec<(i64, String)> = { let Ok(mut st) = self.db.prepare("SELECT id, name FROM thumbs WHERE vol = ?1 AND dir = ?2") else { return 0 };
            let Ok(rows) = st.query_map(params![at.vol, at.dir], |r| Ok((r.get(0)?, r.get(1)?))) else { return 0 }; rows.flatten().collect() };
        if kept.is_empty() { return 0 }
        // a folder that cannot be read says nothing about its files
        let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
        let mut here = std::collections::HashSet::new(); for e in rd { match e { Ok(e) => { here.insert(e.file_name().to_string_lossy().into_owned()); } Err(_) => return 0 } }
        let gone: Vec<i64> = kept.into_iter().filter(|(_, name)| !here.contains(name)).map(|(id, _)| id).collect();
        if gone.is_empty() { return 0 }
        let Ok(tx) = self.db.transaction_with_behavior(TransactionBehavior::Immediate) else { return 0 };
        let mut n = 0; for id in &gone { n += tx.execute("DELETE FROM thumbs WHERE id = ?1", params![id]).unwrap_or(0); }
        if tx.commit().is_ok() { n } else { 0 }
    }
    /// (rows, the running total of bytes) - for the tests.
    #[cfg(test)]
    pub fn totals(&self) -> (i64, i64) { (self.db.query_row("SELECT COUNT(*) FROM thumbs", [], |r| r.get(0)).unwrap(), self.db.query_row("SELECT v FROM meta WHERE k = 'total'", [], |r| r.get(0)).unwrap()) }
}

/// A master as it is kept: (format, bytes).
fn pack(img: &Img) -> Option<(i64, Vec<u8>)> {
    if img.w == 0 || img.h == 0 || img.rgba.len() != (img.w * img.h * 4) as usize { return None }
    if img.rgba.chunks_exact(4).all(|p| p[3] == 255) {
        let rgb: Vec<u8> = img.rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
        min_image::write::jpeg8_bytes(&rgb, img.w, img.h, 85).ok().map(|d| (JPEG, d))
    } else { min_image::write::png_rgba8_bytes(&img.rgba, img.w, img.h).ok().map(|d| (PNG, d)) }
}
/// A kept master back as pixels; none when the row is not what it says (it is then made anew).
fn unpack(fmt: i64, data: &[u8], w: u32, h: u32) -> Option<Img> {
    if w == 0 || h == 0 || w > 4096 || h > 4096 { return None }
    let rgba = match fmt {
        JPEG => { let (rgb, jw, jh) = min_image::jpeg::decode_rgb8(data).ok()?; if (jw, jh) != (w, h) { return None } rgb.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect::<Vec<u8>>() }
        PNG => { let mut reader = png::Decoder::new(data).read_info().ok()?; let info = reader.info();
            if (info.width, info.height) != (w, h) || info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight { return None }
            let mut buf = vec![0u8; reader.output_buffer_size()]; let frame = reader.next_frame(&mut buf).ok()?; buf.truncate(frame.buffer_size()); buf }
        _ => return None,
    };
    (rgba.len() == (w * h * 4) as usize).then_some(Img { w, h, rgba })
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Temp(PathBuf);
    impl Temp { fn new(tag: &str) -> Temp { let p = std::env::temp_dir().join(format!("ufb-cache-{tag}-{}", std::process::id())); let _ = std::fs::remove_dir_all(&p); std::fs::create_dir_all(p.join("db")).unwrap(); std::fs::create_dir_all(p.join("files")).unwrap(); Temp(p) }
        fn file(&self, name: &str) -> PathBuf { let p = self.0.join("files").join(name); std::fs::write(&p, b"x").unwrap(); p } }
    impl Drop for Temp { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
    /// w x h, a gradient; `clear`: the right half transparent.
    fn img(w: u32, h: u32, clear: bool) -> Img { let mut rgba = vec![0u8; (w * h * 4) as usize]; for y in 0..h { for x in 0..w { let i = ((y * w + x) * 4) as usize; if clear && x >= w / 2 { continue } rgba[i] = (x * 255 / w) as u8; rgba[i + 1] = (y * 255 / h) as u8; rgba[i + 2] = 90; rgba[i + 3] = 255; } } Img { w, h, rgba } }
    fn hit(k: Kept) -> Img { match k { Kept::Hit(i) => i, Kept::NoPicture => panic!("no picture"), Kept::Miss => panic!("a miss") } }

    #[test]
    fn a_kept_picture_comes_back_while_the_file_is_unchanged() {
        let t = Temp::new("keep"); let mut c = Cache::open(&t.0.join("db")).unwrap(); let (a, b) = (t.file("a.exr"), t.file("b.png"));
        assert!(matches!(c.lookup(&a, 10, 100), Kept::Miss));
        let (opaque, clear) = (img(512, 256, false), img(300, 200, true)); c.keep(&a, 10, 100, &opaque); c.keep(&b, 20, 200, &clear);
        let got = hit(c.lookup(&a, 10, 100)); assert_eq!((got.w, got.h), (512, 256));
        let worst = got.rgba.iter().zip(&opaque.rgba).map(|(x, y)| (*x as i32 - *y as i32).abs()).max().unwrap(); assert!(worst <= 12, "an opaque picture is kept as a JPEG, close to itself: {worst}");
        let got = hit(c.lookup(&b, 20, 200)); assert_eq!(got.rgba, clear.rgba, "transparency is kept exactly");
        // another size or another date: not this file's picture any more
        assert!(matches!(c.lookup(&a, 11, 100), Kept::Miss)); assert!(matches!(c.lookup(&a, 10, 101), Kept::Miss));
        // kept again for the changed file: one row a file, and the running total is the sum of what is there
        c.keep(&a, 11, 100, &img(64, 64, false)); let (rows, total) = c.totals(); assert_eq!(rows, 2);
        let sum: i64 = c.db.query_row("SELECT SUM(bytes) FROM thumbs", [], |r| r.get(0)).unwrap(); assert_eq!(total, sum); assert!(total > 0);
        assert_eq!(hit(c.lookup(&a, 11, 100)).w, 64);
        // a second connection (another helper) sees the same
        let mut d = Cache::open(&t.0.join("db")).unwrap(); assert_eq!(hit(d.lookup(&b, 20, 200)).rgba, clear.rgba);
    }
    #[test]
    fn no_picture_is_remembered_until_the_folder_is_refreshed() {
        let t = Temp::new("none"); let mut c = Cache::open(&t.0.join("db")).unwrap(); let a = t.file("broken.exr");
        c.mark(&a, 10, 100); assert!(matches!(c.lookup(&a, 10, 100), Kept::NoPicture)); assert!(matches!(c.lookup(&a, 12, 100), Kept::Miss), "the file changed: asked again");
        let b = t.file("good.exr"); c.keep(&b, 1, 1, &img(32, 32, false));
        c.forget(&t.0.join("files")); assert!(matches!(c.lookup(&a, 10, 100), Kept::Miss)); assert!(matches!(c.lookup(&b, 1, 1), Kept::Hit(_)), "the pictures stay");
        assert_eq!(c.totals().0, 1);
    }
    #[test]
    fn what_is_kept_for_a_file_that_is_gone_is_dropped() {
        let t = Temp::new("prune"); let mut c = Cache::open(&t.0.join("db")).unwrap(); let (a, b) = (t.file("a.exr"), t.file("b.exr"));
        assert_eq!(c.prune(&t.0.join("files")), 0, "nothing kept: the folder is not even read");
        c.keep(&a, 1, 1, &img(32, 32, false)); c.keep(&b, 1, 1, &img(32, 32, false)); assert_eq!(c.prune(&t.0.join("files")), 0, "both are there");
        std::fs::remove_file(&a).unwrap(); assert_eq!(c.prune(&t.0.join("files")), 1);
        assert!(matches!(c.lookup(&a, 1, 1), Kept::Miss)); assert!(matches!(c.lookup(&b, 1, 1), Kept::Hit(_)));
        let (rows, total) = c.totals(); assert_eq!(rows, 1); let sum: i64 = c.db.query_row("SELECT SUM(bytes) FROM thumbs", [], |r| r.get(0)).unwrap(); assert_eq!(total, sum);
        assert_eq!(c.prune(&t.0.join("gone")), 0, "a folder that cannot be read says nothing");
    }
    #[test]
    fn over_the_cap_the_pictures_used_longest_ago_go() {
        let t = Temp::new("cap"); let mut c = Cache::open(&t.0.join("db")).unwrap();
        let files: Vec<PathBuf> = (0..200).map(|i| t.file(&format!("f{i:03}.exr"))).collect();
        for (i, f) in files.iter().enumerate() { c.keep(f, 1, 1, &img(64, 64, true)); c.db.execute("UPDATE thumbs SET used = ?1 WHERE name = ?2", params![1000 + i as i64, f.file_name().unwrap().to_string_lossy()]).unwrap(); }
        let (rows, total) = c.totals(); assert_eq!(rows, 200); let each = total / 200;
        // a cap of 150 pictures: the next one kept takes the total over it, and the oldest go, 64 at a time, to under nine tenths
        c.cap = (each * 150) as u64; c.keep(&t.file("new.exr"), 1, 1, &img(64, 64, true));
        let (rows, total) = c.totals(); assert!(total as u64 <= c.cap / 10 * 9 && rows == 201 - 128, "{rows} rows, {total} bytes");
        assert!(matches!(c.lookup(&files[0], 1, 1), Kept::Miss), "the oldest went"); assert!(matches!(c.lookup(&files[199], 1, 1), Kept::Hit(_))); assert!(matches!(c.lookup(&t.0.join("files/new.exr"), 1, 1), Kept::Hit(_)));
    }
    #[test]
    fn a_file_that_is_no_database_is_replaced() {
        let t = Temp::new("bad"); std::fs::write(t.0.join("db").join(FILE), vec![0x55u8; 9000]).unwrap();
        let mut c = Cache::open(&t.0.join("db")).expect("made anew"); let a = t.file("a.exr"); c.keep(&a, 1, 1, &img(16, 16, false)); assert!(matches!(c.lookup(&a, 1, 1), Kept::Hit(_)));
        // a row that is not what it says is a miss, not a death
        c.db.execute("UPDATE thumbs SET data = x'00112233'", []).unwrap(); assert!(matches!(c.lookup(&a, 1, 1), Kept::Miss));
    }
}
