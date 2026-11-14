//! THE USER GUIDE inside the application (row 359): the manual's own pages - the same Markdown the website is made of -
//! as notes of a small collection kept in the application's data, shown as a folder at the foot of the sidebar. It is
//! no part of the user's collection: nothing of it is saved into, exported with or packaged with their document, and
//! its pages can only be read.
//!
//! The pages travel with the application (`guide/` beside it, or in the bundle's Resources; in the tree,
//! `apps/mn/guide`, filled by `tools/mn-shots/sync-guide.sh`). The collection is built from them on first use and
//! again whenever their stamp changes - through the application's own Markdown import, so a page reads here as an
//! imported note does. A link from one page to another (`[Editing](editing.md)`) becomes `minnotes:guide/editing`.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use mn_doc::{BlockId, Kind};
use mn_store::{Collection, DocKind};

pub const LINK: &str = "minnotes:guide/";
pub const NAME: &str = "User Guide";

pub struct Guide { pub col: Collection, pub folder: BlockId, /// its pages in the manual's order: (the note, the page's file name without `.md`, its title)
    pub pages: Vec<(BlockId, String, String)>, /// the page in the window, if one is
    pub shown: Option<BlockId> }

/// Where the pages are: beside the application, in a bundle's Resources, or in the source tree.
pub fn source() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("MN_GUIDE").map(PathBuf::from).filter(|p| p.is_dir()) { return Some(p) }
    let me = std::env::current_exe().ok()?; let here = me.parent()?;
    for p in [here.join("guide"), here.join("../Resources/guide")] { if p.join("index.md").is_file() { return Some(p) } }
    here.ancestors().map(|a| a.join("apps/mn/guide")).find(|p| p.join("index.md").is_file())
}

/// A page's front matter (`title`, `nav_order`) and its text without it.
fn front(text: &str) -> (String, i64, &str) {
    let Some(rest) = text.strip_prefix("---") else { return (String::new(), 999, text) };
    let Some(end) = rest.find("\n---") else { return (String::new(), 999, text) };
    let (mut title, mut order) = (String::new(), 999);
    for l in rest[..end].lines() { if let Some(v) = l.strip_prefix("title:") { title = v.trim().trim_matches('"').to_string(); } if let Some(v) = l.strip_prefix("nav_order:") { order = v.trim().parse().unwrap_or(999); } }
    (title, order, rest[end + 4..].trim_start_matches(['\r', '\n']))
}

/// The page's text as the importer should read it: pictures by their full path, pages by the guide's own address.
fn prepared(body: &str, src: &Path) -> String {
    let images = src.join("images").to_string_lossy().replace(' ', "%20");
    let mut out = String::with_capacity(body.len() + 256); let mut rest = body;
    while let Some(i) = rest.find("](") {
        out.push_str(&rest[..i + 2]); rest = &rest[i + 2..];
        let end = rest.find(')').unwrap_or(rest.len()); let target = &rest[..end];
        if let Some(pic) = target.strip_prefix("images/") { out.push_str(&images); out.push('/'); out.push_str(pic); }
        else if !target.contains("://") && (target.ends_with(".md") || target.contains(".md#")) { out.push_str(LINK); out.push_str(target.split(".md").next().unwrap_or("")); }
        else { out.push_str(target); }
        rest = &rest[end..];
    }
    out.push_str(rest); out
}

/// The guide's collection for these pages: opened when it was already built from them, built otherwise.
pub fn open(src: &Path, settings: &Path) -> Result<Guide, String> {
    let stamp = std::fs::read_to_string(src.join("STAMP")).unwrap_or_default().trim().chars().filter(|c| c.is_ascii_alphanumeric()).take(40).collect::<String>();
    // (BUILD: raised when the way the collection is made changes, so that one made the old way is made again - 2: the pictures are kept)
    const BUILD: u32 = 2;
    let dir = settings.join("guide").join(if stamp.is_empty() { "dev".to_string() } else { format!("{BUILD}-{stamp}") });
    let file = dir.join(format!("{NAME}.mnd"));
    // (without a stamp - the source tree - it is built afresh each time, so an edited page shows)
    if stamp.is_empty() || !file.is_file() { let _ = std::fs::remove_dir_all(&dir); build(src, &dir, &file).inspect_err(|_| { let _ = std::fs::remove_dir_all(&dir); })?;
        // older builds are of no use any more
        if let Ok(rd) = std::fs::read_dir(settings.join("guide")) { for e in rd.flatten() { if e.path() != dir { let _ = std::fs::remove_dir_all(e.path()); } } } }
    let col = Collection::open_read(&file).map_err(|e| e.to_string())?;
    let folder = col.list(None).map_err(|e| e.to_string())?.into_iter().find(|e| e.kind == DocKind::Folder).ok_or("the guide has no pages")?.id;
    let slugs: HashMap<String, String> = std::fs::read_to_string(dir.join("pages.txt")).unwrap_or_default().lines().filter_map(|l| l.split_once('\t').map(|(a, b)| (a.to_string(), b.to_string()))).collect();
    let pages = col.list(Some(folder)).map_err(|e| e.to_string())?.into_iter().map(|e| (e.id, slugs.get(&e.id.to_string()).cloned().unwrap_or_default(), e.title)).collect();
    Ok(Guide { col, folder, pages, shown: None })
}

fn build(src: &Path, dir: &Path, file: &Path) -> Result<(), String> {
    let work = dir.join("pages"); std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let mut pages: Vec<(i64, String, String, PathBuf)> = vec![];
    for e in std::fs::read_dir(src).map_err(|e| e.to_string())?.flatten() {
        let p = e.path(); if p.extension().is_none_or(|x| x != "md") { continue }
        let text = std::fs::read_to_string(&p).map_err(|e| e.to_string())?; let (title, order, body) = front(&text);
        let slug = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let tmp = work.join(format!("{slug}.md")); std::fs::write(&tmp, prepared(body, src)).map_err(|e| e.to_string())?;
        pages.push((order, slug.clone(), if title.is_empty() { slug } else { title }, tmp));
    }
    if pages.is_empty() { return Err("no pages".into()) }
    pages.sort();
    // the pictures are kept with the guide's collection (the importer's own way), then named by their full path: a
    // record that says "kept with the collection" would be looked for beside the USER's document
    let kept = dir.join(crate::media::SIDE); let places = crate::media::Places::at(Some(dir.to_path_buf())).staged_in(kept.clone());
    let mut col = Collection::open(file).map_err(|e| e.to_string())?;
    let folder = col.create(None, DocKind::Folder, NAME).map_err(|e| e.to_string())?;
    let mut list = String::new();
    for (_, slug, title, tmp) in &pages {
        let done = crate::import::read(tmp, &places)?; let mut doc = crate::import::doc_of(&done.clip);
        let fixes: Vec<(BlockId, Kind)> = doc.walk().iter().filter_map(|b| { let Kind::Media { descriptor } = &b.kind else { return None }; let mut v: serde_json::Value = serde_json::from_str(descriptor).ok()?;
            let name = v.get("src")?.as_str()?.strip_prefix(".minnotes/")?.to_string(); v["src"] = serde_json::Value::String(kept.join(name).to_string_lossy().into_owned()); Some((b.id, Kind::Media { descriptor: v.to_string() })) }).collect();
        for (id, k) in fixes { let _ = doc.set_kind(id, k); }
        // (the page's own first heading is its title: the note is named by the manual's navigation title)
        let id = col.create(Some(folder), DocKind::Note, title).map_err(|e| e.to_string())?; col.save_note(id, &doc).map_err(|e| e.to_string())?;
        list.push_str(&format!("{id}\t{slug}\n"));
    }
    drop(col);
    std::fs::write(dir.join("pages.txt"), list).map_err(|e| e.to_string())?; let _ = std::fs::remove_dir_all(&work);
    Ok(())
}

impl Guide {
    pub fn has(&self, id: BlockId) -> bool { id == self.folder || self.pages.iter().any(|p| p.0 == id) }
    /// The page a guide address names (`minnotes:guide/editing`, with or without a part after `#`).
    pub fn page(&self, link: &str) -> Option<BlockId> { let slug = link.strip_prefix(LINK)?.split('#').next()?; self.pages.iter().find(|p| p.1 == slug).map(|p| p.0) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn a_page_is_read_for_its_title_and_its_links_are_the_guides_own() {
        let (title, order, body) = front("---\ntitle: Web Folder\npermalink: /web-folder/\nnav_order: 12\n---\n\n# Web Folder\ntext");
        assert_eq!((title.as_str(), order, body), ("Web Folder", 12, "# Web Folder\ntext"));
        let p = prepared("See [Editing](editing.md) and [video](media.md#video), ![shot](images/mn3-home.png), [site](https://example.com/a.md).", Path::new("/g s"));
        assert_eq!(p, "See [Editing](minnotes:guide/editing) and [video](minnotes:guide/media), ![shot](/g%20s/images/mn3-home.png), [site](https://example.com/a.md).");
    }
}
