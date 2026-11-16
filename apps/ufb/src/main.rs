//! UFB (the rebuild, Phase 5; canon rows 434-476): a file browser. Pictures are made in helper copies of this
//! executable (`UFB --thumb-helper`, see `thumbs`) and kept in a cache of its own (`UFB_THUMB_CACHE=<folder>` or `off`;
//! else the system's cache location). File operations (`ops`) change things ONLY inside the folders it was started
//! with - the safety belt of row 468, until the owner's word.
//!   UFB [folder] [--favorite <path>]... [--allow-writes <folder>]...
//! opens the folder given (the home folder without one); each `--favorite` adds a test entry to the sidebar's
//! Favorites. It reads no settings file, no database and nothing of an installed 1.4.x (row 463, point 5).
//! Scripted looks (no hand on the machine): `UFB_SHOT=<file.png>` saves the window as drawn and ends,
//! `UFB_DEMO=<steps;...>` does the steps first (see `ui.rs`), `UFB_SIZE=1440x900` sets the window's size,
//! `UFB_MENU_BAR=1` shows the drawn menu bar on macOS too.
#![cfg_attr(windows, windows_subsystem = "windows")]
mod crumbs;
mod dnd;
mod ctxmenu;
mod format;
mod history;
mod icons;
mod lightbox;
mod lister;
mod menu;
mod model;
mod ops;
mod pane;
mod places;
mod shell;
mod sysopen;
mod thumbs;
mod ui;

/// What the command line asked for: the folder to open, the test favorites, and the folders this build may CHANGE
/// things in (row 468, point 2): the folder it was started with, the favorites, each `--allow-writes <folder>`.
pub fn parse(args: &[String], home: &std::path::Path) -> (std::path::PathBuf, Vec<places::Place>, Vec<std::path::PathBuf>) {
    let abs = |p: &str| std::path::absolute(p).unwrap_or_else(|_| std::path::PathBuf::from(p));
    let (mut folder, mut favorites, mut extra, mut i) = (None, vec![], vec![], 0);
    while i < args.len() {
        if args[i] == "--allow-writes" { if let Some(p) = args.get(i + 1) { extra.push(abs(p)); } i += 2; continue }
        if args[i] == "--favorite" { if let Some(p) = args.get(i + 1) { let path = abs(p); favorites.push(places::Place { label: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.clone()), path, icon: "star" }); } i += 2; continue }
        if !args[i].starts_with("--") && folder.is_none() { folder = Some(abs(&args[i])); }
        i += 1;
    }
    let mut writes: Vec<std::path::PathBuf> = folder.iter().cloned().chain(favorites.iter().map(|f: &places::Place| f.path.clone())).chain(extra).collect(); writes.dedup();
    (folder.unwrap_or_else(|| home.to_path_buf()), favorites, writes)
}

fn main() {
    // a helper copy of the executable (thumbnails, canon row 456): the first branch, before anything else is set up
    if std::env::args_os().nth(1).is_some_and(|a| a == thumbs::pool::HELPER_ARG) { thumbs::helper::run() }
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") { println!("UFB [folder] [--favorite <path>]... [--allow-writes <folder>]..."); return }
    let (folder, favorites, writes) = parse(&args, &places::home());
    // (files promised by another application arrive in a folder of our own first: the engine may take them out of it)
    let mut writes = writes; let incoming = dnd::staging(); if std::fs::create_dir_all(&incoming).is_ok() { writes.push(incoming); }
    ops::init(&writes);
    shell::run(folder, favorites);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_command_line() {
        let home = std::path::Path::new("/home/u"); let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(parse(&[], home), (home.to_path_buf(), vec![], vec![]), "started with no folder: the home folder is shown, and nothing may be changed");
        let tmp = std::env::temp_dir(); let (jobs, assets) = (tmp.join("jobs"), tmp.join("assets"));
        let (folder, favs, writes) = parse(&a(&[jobs.to_str().unwrap(), "--favorite", assets.to_str().unwrap(), "--favorite", jobs.to_str().unwrap(), "--allow-writes", tmp.join("scratch").to_str().unwrap()]), home);
        assert_eq!(writes, [jobs.clone(), assets.clone(), jobs.clone(), tmp.join("scratch")], "what may be changed: the folder, the favorites, what was named");
        assert_eq!(folder, jobs); assert_eq!(favs.iter().map(|f| (f.label.as_str(), f.path.clone())).collect::<Vec<_>>(), [("assets", assets.clone()), ("jobs", jobs.clone())]);
        let (folder, favs, writes) = parse(&a(&["--favorite", assets.to_str().unwrap()]), home); assert_eq!(folder, home); assert_eq!(favs.len(), 1); assert_eq!(writes, [assets.clone()], "the home folder shown by default is not among them");
        assert_eq!(parse(&a(&["--favorite"]), home).1.len(), 0, "a favorite without a path is none");
    }
}
