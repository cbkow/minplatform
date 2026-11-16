//! UFB (the rebuild, Phase 5; canon rows 434-464). Slice 2: a browser to LOOK AROUND in - it reads folders and
//! changes nothing. Pictures are made in helper copies of this executable (`UFB --thumb-helper`, see `thumbs`).
//!   UFB [folder] [--favorite <path>]...
//! opens the folder given (the home folder without one); each `--favorite` adds a test entry to the sidebar's
//! Favorites. This slice reads no settings file, no database and nothing of an installed 1.4.x (row 463, point 5).
//! Scripted looks (no hand on the machine): `UFB_SHOT=<file.png>` saves the window as drawn and ends,
//! `UFB_DEMO=<steps;...>` does the steps first (see `ui.rs`), `UFB_SIZE=1440x900` sets the window's size,
//! `UFB_MENU_BAR=1` shows the drawn menu bar on macOS too.
#![cfg_attr(windows, windows_subsystem = "windows")]
mod crumbs;
mod format;
mod history;
mod icons;
mod lister;
mod menu;
mod model;
mod pane;
mod places;
mod shell;
mod thumbs;
mod ui;

/// What the command line asked for: the folder to open, the test favorites.
pub fn parse(args: &[String], home: &std::path::Path) -> (std::path::PathBuf, Vec<places::Place>) {
    let abs = |p: &str| std::path::absolute(p).unwrap_or_else(|_| std::path::PathBuf::from(p));
    let (mut folder, mut favorites, mut i) = (None, vec![], 0);
    while i < args.len() {
        if args[i] == "--favorite" { if let Some(p) = args.get(i + 1) { let path = abs(p); favorites.push(places::Place { label: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.clone()), path, icon: "star" }); } i += 2; continue }
        if !args[i].starts_with("--") && folder.is_none() { folder = Some(abs(&args[i])); }
        i += 1;
    }
    (folder.unwrap_or_else(|| home.to_path_buf()), favorites)
}

fn main() {
    // a helper copy of the executable (thumbnails, canon row 456): the first branch, before anything else is set up
    if std::env::args_os().nth(1).is_some_and(|a| a == thumbs::pool::HELPER_ARG) { thumbs::helper::run() }
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") { println!("UFB [folder] [--favorite <path>]..."); return }
    let (folder, favorites) = parse(&args, &places::home());
    shell::run(folder, favorites);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_command_line() {
        let home = std::path::Path::new("/home/u"); let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(parse(&[], home), (home.to_path_buf(), vec![]));
        let tmp = std::env::temp_dir(); let (jobs, assets) = (tmp.join("jobs"), tmp.join("assets"));
        let (folder, favs) = parse(&a(&[jobs.to_str().unwrap(), "--favorite", assets.to_str().unwrap(), "--favorite", jobs.to_str().unwrap()]), home);
        assert_eq!(folder, jobs); assert_eq!(favs.iter().map(|f| (f.label.as_str(), f.path.clone())).collect::<Vec<_>>(), [("assets", assets.clone()), ("jobs", jobs.clone())]);
        let (folder, favs) = parse(&a(&["--favorite", assets.to_str().unwrap()]), home); assert_eq!(folder, home); assert_eq!(favs.len(), 1);
        assert_eq!(parse(&a(&["--favorite"]), home).1.len(), 0, "a favorite without a path is none");
    }
}
