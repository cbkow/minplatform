//! UFB (the rebuild, Phase 5; canon rows 434-479): a file browser. Pictures are made in helper copies of this
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
mod archive;
mod board;
mod chips;
mod crumbs;
mod db;
mod dnd;
mod docview;
mod ctxmenu;
mod format;
mod history;
mod http;
mod icons;
mod identity;
mod instance;
mod itemlist;
mod jobview;
mod lightbox;
mod lister;
mod logfile;
mod login;
mod menu;
mod mesh;
mod model;
mod ops;
mod pane;
mod places;
mod prefs;
mod settings;
mod shares;
mod shell;
#[cfg(target_os = "macos")] mod sparkle;
mod sysopen;
mod tasks;
mod thumbs;
mod tracker;
mod transcode;
mod ui;
mod webview;

/// What the command line asked for: the folder to open, the test favorites, and the folders this build may CHANGE
/// things in (row 468, point 2): the folder it was started with, the favorites, each `--allow-writes <folder>`.
/// The one thing a launch is asked to open: its first argument that is neither an option nor an option's value.
fn positional(args: &[String]) -> Option<String> {
    let mut i = 0; while i < args.len() { if matches!(args[i].as_str(), "--map" | "--mesh" | "--mesh-node" | "--mesh-only" | "--allow-writes" | "--favorite" | "--jobs") { i += 2; continue } if args[i].starts_with("--") { i += 1; continue } return Some(args[i].clone()) } None
}
pub fn parse(args: &[String], home: &std::path::Path) -> (std::path::PathBuf, Vec<places::Place>, Vec<std::path::PathBuf>, Vec<std::path::PathBuf>) {
    let abs = |p: &str| std::path::absolute(p).unwrap_or_else(|_| std::path::PathBuf::from(p));
    let (mut folder, mut favorites, mut extra, mut jobs, mut i) = (None, vec![], vec![], vec![], 0);
    while i < args.len() {
        if matches!(args[i].as_str(), "--map" | "--mesh" | "--mesh-node" | "--mesh-only") { i += 2; continue }
        if args[i] == "--allow-writes" { if let Some(p) = args.get(i + 1) { extra.push(abs(p)); } i += 2; continue }
        // a jobs folder (row 482): a favorite with the jobs' mark, where New Job makes jobs
        if args[i] == "--jobs" { if let Some(p) = args.get(i + 1) { let path = abs(p); favorites.push(places::Place { label: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.clone()), path: path.clone(), icon: "briefcase" }); jobs.push(path); } i += 2; continue }
        if args[i] == "--favorite" { if let Some(p) = args.get(i + 1) { let path = abs(p); favorites.push(places::Place { label: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.clone()), path, icon: "star" }); } i += 2; continue }
        // (a link handed over by the system: kept as it is here, resolved once the shares are known)
        if !args[i].starts_with("--") && folder.is_none() { folder = Some(if args[i].starts_with("ufb:") || args[i].starts_with("union:") { std::path::PathBuf::from(&args[i]) } else { abs(&args[i]) }); }
        i += 1;
    }
    let mut writes: Vec<std::path::PathBuf> = folder.iter().cloned().chain(favorites.iter().map(|f: &places::Place| f.path.clone())).chain(extra).collect(); writes.dedup();
    (folder.unwrap_or_else(|| home.to_path_buf()), favorites, writes, jobs)
}

fn main() {
    // a helper copy of the executable (thumbnails, canon row 456): the first branch, before anything else is set up
    if std::env::args_os().nth(1).is_some_and(|a| a == thumbs::pool::HELPER_ARG) { thumbs::helper::run() }
    // the release on Windows keeps its data where 1.4.x does - the per-user LOCAL application-data folder, `ufb`
    // (the family's settings service would take the roaming one): named once, here, for everything that asks (row 534)
    #[cfg(windows)] if identity::kind() == identity::Kind::Release && std::env::var_os("UFB_SETTINGS_DIR").is_none() { if let Some(local) = std::env::var_os("LOCALAPPDATA") { unsafe { std::env::set_var("UFB_SETTINGS_DIR", std::path::PathBuf::from(local).join("ufb")); } } }
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") { println!("UFB [folder] [--favorite <path>]... [--jobs <folder>]... [--allow-writes <folder>]... [--map <windows side>=<macOS side>]... [--mesh <farm folder> [--mesh-node <id>] [--mesh-only <folder>]]"); return }
    // one UFB a user (row 519): a second launch hands what it was asked to open to the one that runs, and ends
    if instance::takes_part() { if let Some(dir) = identity::data_dir() {
        let asked = positional(&args).unwrap_or_default(); let asked = if asked.is_empty() || asked.starts_with("ufb:") || asked.starts_with("union:") { asked } else { std::path::absolute(&asked).map(|p| p.to_string_lossy().into_owned()).unwrap_or(asked) };
        if instance::hand_over(&dir, &asked) { return } instance::claim(&dir); } }
    // what the application says of itself goes to its log file (not in a scripted look, whose script reads the console)
    if logfile::wanted() { if let Some(p) = logfile::install(identity::data_name()) { eprintln!("UFB: the log is {}", p.display()); } }
    // (Nothing is taken over from UFB 1.4.x any more - row 549: 2.0.1 to 2.0.3 did that at their first launch, and
    // the versions after them open the data folder as they find it. The code is in the tag `ufb-v2.0.3`.)
    let (folder, favorites, writes, jobs) = parse(&args, &places::home()); jobview::set_roots(jobs);
    // shared paths (row 494): `--map <windows side>=<macOS side>` names a share as both systems know it - what is under it is shared with the other machines
    { let started: Vec<db::Mapping> = args.windows(2).filter(|w| w[0] == "--map").filter_map(|w| w[1].split_once('=')).map(|(win, mac)| db::Mapping::new(win, mac)).collect(); shares::set_started_with(started.clone());
        // the saved shares (row 498) with where each is now, and those the build was started with: known before the database is opened
        let saved = shares::load(); let mounts = shares::mounts(); let states: Vec<shares::State> = saved.iter().map(|s| shares::resolve(s, &mounts, &|p| p.is_dir())).collect();
        let _ = started; db::set_mappings(shares::mappings_now(&saved, &states, &mounts));
        jobview::set_share_roots(saved.iter().zip(states.iter()).filter(|(s, _)| s.jobs).filter_map(|(s, st)| st.root().map(|p| (p.to_path_buf(), s.ptype.clone()))).collect()); }
    // (files promised by another application arrive in a folder of our own first: the engine may take them out of it)
    let mut writes = writes; let incoming = dnd::staging(); if std::fs::create_dir_all(&incoming).is_ok() { writes.push(incoming); }
    ops::init(&writes);
    // the mesh (row 494): `--mesh <farm folder>` starts this machine's node; `--mesh-node <id>` names it (else the
    // machine's name); `--mesh-only <folder>` keeps everything but that folder's rows out of it. A development build
    // runs at a TEST epoch (no installed UFB is of it) unless `UFB_MESH_EPOCH` says otherwise; `UFB_MESH_PORTS=c,d,u`.
    let arg = |name: &str| args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone());
    // (`UFB_NO_SYNC=1`: the mesh's node is not started, whatever the settings say - a release build tried on a COPY of
    // real data must not join the real mesh under the real machine's name)
    if std::env::var_os("UFB_NO_SYNC").is_some() { eprintln!("UFB: sync is held off by UFB_NO_SYNC"); } else
    if let Some(farm) = arg("--mesh") { mesh::start_with(std::path::Path::new(&farm), &arg("--mesh-node").unwrap_or_default(), "", arg("--mesh-only").as_deref().map(std::path::Path::new)); }
    // ... else what the Sync entry of the sidebar saved (row 499)
    else { let (on, farm, node, leader, only) = settings::with(|s| (s.bool("sync/on"), s.string("sync/farm"), s.string("sync/node"), s.string("sync/leader"), s.string("sync/only")));
        if on && !farm.trim().is_empty() { mesh::start_with(std::path::Path::new(farm.trim()), &node, &leader, (!only.trim().is_empty()).then(|| std::path::Path::new(only.trim()))); } }
    // a link as the folder to start in: where it leads on this machine (a file: its folder), else the home folder
    // (a `union:` link handed over at the start is shown in the file manager; the window opens on the home folder)
    if let Some(p) = folder.to_str().filter(|t| db::is_union(t)).and_then(db::from_link) { if p.exists() { sysopen::reveal(&[p]); } }
    let folder = if folder.to_str().is_some_and(db::is_union) { places::home() } else { folder };
    let folder = match folder.to_str().filter(|t| t.starts_with("ufb:") || t.starts_with("union:")) { Some(link) => db::from_link(link).map(|p| if p.is_dir() { p } else { p.parent().map(|d| d.to_path_buf()).unwrap_or(p) }).filter(|p| p.is_dir()).unwrap_or_else(places::home), None => folder };
    shell::run(folder, favorites); instance::release();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_command_line() {
        let home = std::path::Path::new("/home/u"); let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(parse(&[], home), (home.to_path_buf(), vec![], vec![], vec![]), "started with no folder: the home folder is shown, and nothing may be changed");
        let tmp = std::env::temp_dir(); let (jobs, assets) = (tmp.join("jobs"), tmp.join("assets"));
        let (folder, favs, writes, _) = parse(&a(&[jobs.to_str().unwrap(), "--favorite", assets.to_str().unwrap(), "--favorite", jobs.to_str().unwrap(), "--allow-writes", tmp.join("scratch").to_str().unwrap()]), home);
        assert_eq!(writes, [jobs.clone(), assets.clone(), jobs.clone(), tmp.join("scratch")], "what may be changed: the folder, the favorites, what was named");
        assert_eq!(folder, jobs); assert_eq!(favs.iter().map(|f| (f.label.as_str(), f.path.clone())).collect::<Vec<_>>(), [("assets", assets.clone()), ("jobs", jobs.clone())]);
        let (folder, favs, writes, _) = parse(&a(&["--favorite", assets.to_str().unwrap()]), home); assert_eq!(folder, home); assert_eq!(favs.len(), 1); assert_eq!(writes, [assets.clone()], "the home folder shown by default is not among them");
        assert_eq!(parse(&a(&["--favorite"]), home).1.len(), 0, "a favorite without a path is none");
        // a jobs folder: a favorite with the jobs' mark, a folder that may be changed, and named as a jobs folder
        let (_, favs, writes, roots) = parse(&a(&["--jobs", jobs.to_str().unwrap()]), home); assert_eq!((favs[0].icon, favs[0].path.clone()), ("briefcase", jobs.clone())); assert_eq!(writes, [jobs.clone()]); assert_eq!(roots, [jobs.clone()]);
    }
}
