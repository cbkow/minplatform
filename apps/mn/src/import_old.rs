//! A minNotes 2 document imported (row 377; the spec is `docs/phase3/step-import-2x.md`): its PAGES become notes of a
//! collection of the new kind, in a folder named after the old file. The old file is never written to and never even
//! read where it lies - a copy is (row 272). What 2.x kept with the document (`.minnotes/` beside it; `media/` in a
//! package) is brought along; what lay on its canvas outside a page is counted and said, not imported.
//!
//! The reading itself is `mn_store::old::read_pages` and `mn_doc::legacy::convert_page`, written for row 283.
use std::path::{Path, PathBuf};
use mn_doc::{Doc, Kind};

pub struct Page { pub title: String, pub doc: Doc, pub repairs: usize }
pub struct Done { /// the old file's name without its extension
    pub name: String, pub pages: Vec<Page>, /// the old canvas's other things, by 2.x's kind
    pub left: Vec<(String, i64)>, /// where the files 2.x kept with the document are, when there are any
    pub media: Option<PathBuf>, /// the throwaway folder all of this was read in
    pub work: PathBuf }

fn ext(p: &Path) -> String { p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default() }
fn work_dir() -> PathBuf { static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0); std::env::temp_dir().join(format!("minnotes-v2-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed))) }

/// Is this package one of minNotes 2's (a zip with `manifest.json` saying `mnpkg` and a `document.mnd`)? The new
/// packages have no manifest.
pub fn old_package(path: &Path) -> bool {
    if ext(path) != crate::gather::PACKAGE { return false }
    let Ok(f) = std::fs::File::open(path) else { return false }; let Ok(mut z) = zip::ZipArchive::new(f) else { return false };
    if z.by_name("document.mnd").is_err() { return false }
    let Ok(mut m) = z.by_name("manifest.json") else { return false }; let mut s = String::new();
    std::io::Read::read_to_string(&mut m, &mut s).is_ok() && serde_json::from_str::<serde_json::Value>(&s).ok().is_some_and(|v| v.get("format").and_then(|f| f.as_str()) == Some("mnpkg"))
}
/// Is this document one of minNotes 2's? Asked only of a file that would not open; a copy is looked at.
pub fn old_document(path: &Path) -> bool {
    if ext(path) != "mnd" || !path.is_file() { return false }
    let dir = work_dir(); let copy = dir.join("probe.mnd");
    let is = std::fs::create_dir_all(&dir).and_then(|_| stage(path, &copy)).is_ok() && mn_store::old::is_v2(&copy);
    let _ = std::fs::remove_dir_all(&dir); is
}
/// The document copied with what SQLite may have left beside it (changes not yet folded into the file).
fn stage(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::copy(from, to)?;
    for tail in ["-wal", "-shm"] { let side = PathBuf::from(format!("{}{tail}", from.display())); if side.is_file() { let _ = std::fs::copy(&side, format!("{}{tail}", to.display())); } }
    Ok(())
}

/// 2.x's own rule for a page's name: its first heading, else its first line of text, else "Untitled page"; 60 characters.
fn title(doc: &Doc) -> String {
    let all = doc.walk(); let line = |b: &&mn_doc::Block| !b.text.trim().is_empty();
    let b = all.iter().find(|b| matches!(b.kind, Kind::Heading { .. }) && line(b)).or_else(|| all.iter().find(|b| b.kind.is_text() && line(b)));
    b.map(|b| b.text.trim().lines().next().unwrap_or("").chars().take(60).collect::<String>()).filter(|t| !t.is_empty()).unwrap_or_else(|| "Untitled page".into())
}

/// Reads a 2.x document or package (off the window's thread).
pub fn read(path: &Path) -> Result<Done, String> {
    let work = work_dir(); std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let r = read_in(path, &work); if r.is_err() { let _ = std::fs::remove_dir_all(&work); } r
}
fn read_in(path: &Path, work: &Path) -> Result<Done, String> {
    let copy = work.join("document.mnd");
    let media = if old_package(path) {
        let mut z = zip::ZipArchive::new(std::fs::File::open(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let dir = work.join("media");
        for i in 0..z.len() { let mut f = z.by_index(i).map_err(|e| e.to_string())?; let Some(name) = f.enclosed_name() else { continue }; if f.is_dir() { continue }
            let to = if name == Path::new("document.mnd") { copy.clone() } else if let Ok(rel) = name.strip_prefix("media") { dir.join(rel) } else { continue };
            if let Some(d) = to.parent() { std::fs::create_dir_all(d).map_err(|e| e.to_string())?; }
            std::io::copy(&mut f, &mut std::fs::File::create(&to).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?; }
        dir.is_dir().then_some(dir)
    } else {
        stage(path, &copy).map_err(|e| e.to_string())?;
        path.parent().map(|d| d.join(crate::media::OLD_SIDE)).filter(|d| d.is_dir())
    };
    let pages = mn_store::old::read_pages(&copy).map_err(|g| match g { mn_store::Gate::Earlier => "This document was made with minNotes 1. Open it in minNotes 2 and save it there first.".to_string(), g => g.to_string() })?;
    let pages = pages.iter().map(|p| { let c = mn_doc::legacy::convert_page(&p.blocks); Page { title: title(&c.doc), repairs: c.notes.len(), doc: c.doc } }).filter(|p| !p.doc.walk().is_empty()).collect();
    Ok(Done { name: path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "minNotes 2 document".into()), pages, left: mn_store::old::leftovers(&copy), media, work: work.to_path_buf() })
}

/// The media records of a note: (the record, the name of its file when it is one kept with the document).
pub fn records(doc: &Doc) -> Vec<(String, Option<String>)> {
    doc.walk().iter().filter_map(|b| { let Kind::Media { descriptor } = &b.kind else { return None };
        let kept = serde_json::from_str::<serde_json::Value>(descriptor).ok().and_then(|v| v.get("src").and_then(|s| s.as_str()).and_then(|s| s.strip_prefix(".minnotes/")).map(str::to_string));
        Some((descriptor.clone(), kept)) }).collect()
}

/// What was left on the old canvas, in words ("3 pictures or files, 1 text note"); empty when nothing was.
pub fn left_words(left: &[(String, i64)]) -> String {
    let mut by: Vec<(&str, &str, i64)> = vec![("media", "picture or file", 0), ("text", "text note", 0), ("ink", "drawing", 0), ("group", "frame", 0), ("connector", "connector", 0), ("", "other item", 0)];
    for (kind, n) in left { let i = by.iter().position(|x| x.0 == kind.as_str()).unwrap_or(by.len() - 1); by[i].2 += n; }
    by.iter().filter(|x| x.2 > 0).map(|(_, word, n)| if *n == 1 { format!("1 {word}") } else if *word == "picture or file" { format!("{n} pictures or files") } else { format!("{n} {word}s") }).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_minnotes_2_document_and_its_package_are_read_as_pages_with_what_was_left_behind() {
        let d = std::env::temp_dir().join(format!("mn-v2-test-{}", std::process::id())); let _ = std::fs::remove_dir_all(&d); std::fs::create_dir_all(d.join(".minnotes")).unwrap();
        let old = d.join("Old Handbook.mnd"); mn_store::old::write_sample(&old).unwrap(); std::fs::write(d.join(".minnotes/kept.png"), b"png").unwrap();
        assert!(old_document(&old) && !old_package(&old));
        let done = read(&old).unwrap();
        assert_eq!(done.name, "Old Handbook"); assert_eq!(done.pages.iter().map(|p| p.title.as_str()).collect::<Vec<_>>(), ["Shot list", "Delivery notes"]);
        assert!(done.pages.iter().all(|p| p.repairs == 0)); assert_eq!(done.media.as_deref(), Some(d.join(".minnotes").as_path()));
        assert_eq!(left_words(&done.left), "2 pictures or files, 1 text note");
        let recs = records(&done.pages[0].doc); assert_eq!(recs.len(), 1); assert_eq!(recs[0].1.as_deref(), Some("kept.png")); assert_eq!(records(&done.pages[1].doc)[0].1, None);
        let p = &done.pages[0].doc; let kinds: Vec<&Kind> = p.children(None).iter().map(|id| &p.get(*id).unwrap().kind).collect();
        assert!(matches!(kinds[..], [Kind::Heading { level: 1 }, Kind::Paragraph, Kind::Task { state: 2, .. }, Kind::Task { state: 0, .. }, Kind::Columns, Kind::Media { .. }]), "{kinds:?}");
        let _ = std::fs::remove_dir_all(&done.work);
        // the same document as 2.x packs it: a manifest, the document, its kept files under media/
        let pkg = d.join("Old Handbook.mnpkg"); { use std::io::Write; let mut z = zip::ZipWriter::new(std::fs::File::create(&pkg).unwrap()); let o = zip::write::SimpleFileOptions::default();
            z.start_file("manifest.json", o).unwrap(); z.write_all(br#"{"format":"mnpkg","formatVersion":3}"#).unwrap();
            z.start_file("document.mnd", o).unwrap(); z.write_all(&std::fs::read(&old).unwrap()).unwrap();
            z.start_file("media/kept.png", o).unwrap(); z.write_all(b"png").unwrap(); z.finish().unwrap(); }
        assert!(old_package(&pkg));
        let done = read(&pkg).unwrap(); assert_eq!(done.pages.len(), 2); assert!(done.media.as_ref().is_some_and(|m| m.join("kept.png").is_file()));
        let _ = std::fs::remove_dir_all(&done.work); let _ = std::fs::remove_dir_all(&d);
    }
}
