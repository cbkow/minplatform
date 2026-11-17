//! A job in a tab (canon rows 454, 482; UFB 1.4.2's `JobView` / `FolderTabView` / `ItemListPanel`, read into
//! `docs/phase5/step0-projects-metadata.md`, sections 3 and 5): the job's SECTIONS - its top-level folders - and the
//! ITEMS of the section in front - the folders directly inside it. The browser beside the item list is THE browser
//! (the window's panes); this holds only what is the job's own.
//!
//! 1.4.2's rules, kept: a section is a top-level folder whose name does not start with a dot; an item is a folder
//! directly inside a section whose name starts with neither a dot nor `_t_` nor `000000` (the placeholders 1.4.2
//! copied into every job); a chosen item shows in the browser - its PROJECT folder when it has one (`project`,
//! `scenes`, ...), and in the second pane its RENDERS folder (`renders`, `output`, ...), each falling back to the item.
//!
//! Nothing here is asked of the file system on the window's thread: a job is read on a thread of its own.
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

/// The folders of an item the first pane goes to, and the second (1.4.2's lists; case is not regarded).
const PROJECT_NAMES: [&str; 21] = ["project", "projects", "scenes", "scene", "work", "source", "src", "maya", "houdini", "nuke", "ae", "c4d", "blender", "flame", "fusion", "resolve", "premiere", "aftereffects", "pfx", "matchmove", "roto"];
const PROXY_NAMES: [&str; 2] = ["proxies", "proxy"];
/// How many of a section's items are looked at to say whether it is one of two tiers (1.4.2's number).
const SAMPLED: usize = 10;
const RENDER_NAMES: [&str; 11] = ["renders", "render", "output", "outputs", "comp", "comps", "export", "exports", "deliverables", "plates", "precomp"];

#[derive(Clone, Debug, PartialEq)]
pub struct Item { pub name: String, pub path: PathBuf, /// seconds since 1970; `i64::MIN`: not known
    pub modified: i64, /// where the browser goes for it: the first pane, the second (found when the job is read, off the window's thread)
    pub project: PathBuf, pub renders: PathBuf, /// its proxies folder, if it has one (the second pane can show it in place of the renders)
    pub proxies: Option<PathBuf> }
/// What a read of the job found.
#[derive(Debug, Default)]
pub struct Read { req: u64, sections: Vec<String>, section: Option<String>, items: Vec<Item>, /// every folder of the section in front (a dated item's letter is the first no folder of the day carries)
    names: Vec<String>, two_tier: bool, missing: bool }

/// A name is an item's (1.4.2's rule).
pub fn is_item_name(name: &str) -> bool { !(name.starts_with('.') || name.starts_with("_t_") || name.starts_with("000000")) }
fn dirs(dir: &Path) -> std::io::Result<Vec<(String, PathBuf, i64)>> {
    let mut out = vec![];
    for e in std::fs::read_dir(dir)?.flatten() {
        // (a link to a folder counts, as in the browser)
        let is_dir = e.file_type().map(|t| t.is_dir() || (t.is_symlink() && e.path().is_dir())).unwrap_or(false); if !is_dir { continue }
        let name = e.file_name().to_string_lossy().into_owned(); if name.starts_with('.') { continue }
        let modified = e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64).unwrap_or(i64::MIN);
        out.push((name, e.path(), modified));
    }
    out.sort_by(|a, b| crate::model::natural_key(&a.0).cmp(&crate::model::natural_key(&b.0)));
    Ok(out)
}
/// The job's sections, and the items of `want` (the first section when there is no such one).
pub fn read(job: &Path, want: Option<&str>) -> Read {
    let Ok(tops) = dirs(job) else { return Read { missing: true, ..Default::default() } };
    let sections: Vec<String> = tops.iter().map(|t| t.0.clone()).collect();
    let section = want.and_then(|w| sections.iter().find(|s| s.eq_ignore_ascii_case(w))).or(sections.first()).cloned();
    let all = section.as_ref().and_then(|s| dirs(&job.join(s)).ok()).unwrap_or_default(); let names = all.iter().map(|d| d.0.clone()).collect();
    let items = all.into_iter().filter(|(n, ..)| is_item_name(n)).map(|(name, path, modified)| { let (project, renders) = targets(&path); let proxies = named(&path, &PROXY_NAMES); Item { name, path, modified, project, renders, proxies } }).collect::<Vec<Item>>();
    let two_tier = two_tier(&items.iter().take(SAMPLED).map(|i| (i.project != i.path, i.renders != i.path)).collect::<Vec<_>>());
    Read { req: 0, sections, section, items, names, two_tier, missing: false }
}
fn named(item: &Path, names: &[&str]) -> Option<PathBuf> { std::fs::read_dir(item).ok()?.flatten().find(|e| e.file_type().is_ok_and(|t| t.is_dir()) && names.iter().any(|n| e.file_name().to_string_lossy().eq_ignore_ascii_case(n))).map(|e| e.path()) }
/// A section is one of TWO TIERS - its items shown as a project folder and a renders folder, in two panes - when one
/// of the items looked at has both, or two of them have at least one (1.4.2's rule; each: has a project folder, has a
/// renders folder). Else it is flat: one pane.
pub fn two_tier(items: &[(bool, bool)]) -> bool { items.iter().any(|(p, r)| *p && *r) || items.iter().filter(|(p, r)| *p || *r).count() >= 2 }
/// Where the browser goes for an item: (the first pane, the second) - its project and renders folders, or itself.
pub fn targets(item: &Path) -> (PathBuf, PathBuf) { (named(item, &PROJECT_NAMES).unwrap_or_else(|| item.to_path_buf()), named(item, &RENDER_NAMES).unwrap_or_else(|| item.to_path_buf())) }

/// What a section showed when it was left: the chosen item; each pane's top folder and the folder it was in (the
/// second pane's when it was shown).
pub type Kept = (Option<PathBuf>, (PathBuf, PathBuf), Option<(PathBuf, PathBuf)>);

pub struct Job {
    pub path: PathBuf, pub name: String,
    /// the sections (folder names), the one in front, its items, the chosen one
    pub sections: Vec<String>, pub section: Option<String>, pub items: Vec<Item>, pub chosen: Option<usize>,
    /// nothing was read yet; the job's folder is not there
    pub loading: bool, pub missing: bool, pub dated_names: Vec<String>,
    /// the section in front is one of two tiers; what the user said of the second pane, a section (it wins over the
    /// section's own); the second pane shows the proxies in place of the renders
    pub two_tier: bool, pub pane2: std::collections::HashMap<String, bool>, pub proxies: bool,
    /// what each section showed when it was left, in this tab (1.4.2's session memory): the chosen item, the first
    /// pane's folder, the second's if it was shown. `hold`: such a view was just put back - the browser is not sent
    /// to the item's folders over it when the section's read arrives
    pub memory: std::collections::HashMap<String, Kept>, pub hold: bool,
    /// an item to choose once it is there (it was just made); the section changed: the browser is to follow
    pub choose: Option<PathBuf>, pub moved: bool,
    req: u64, tx: Sender<Read>, rx: Receiver<Read>, wake: Arc<dyn Fn() + Send + Sync>,
}
impl Job {
    pub fn new(path: PathBuf, name: String, wake: Arc<dyn Fn() + Send + Sync>) -> Job {
        let (tx, rx) = channel(); let mut j = Job { path, name, sections: vec![], section: None, items: vec![], chosen: None, loading: true, missing: false, dated_names: vec![], two_tier: false, pane2: Default::default(), proxies: false, memory: Default::default(), hold: false, choose: None, moved: false, req: 0, tx, rx, wake };
        j.reload(); j
    }
    /// Read the job again (the section in front is kept).
    pub fn reload(&mut self) { let want = self.section.clone(); self.load(want); }
    fn load(&mut self, want: Option<String>) {
        self.req += 1; let (req, job, tx, wake) = (self.req, self.path.clone(), self.tx.clone(), self.wake.clone());
        let _ = std::thread::Builder::new().name("ufb-job".into()).spawn(move || { let mut r = read(&job, want.as_deref()); r.req = req; let _ = tx.send(r); wake(); });
    }
    /// Another section comes to the front.
    pub fn show_section(&mut self, name: &str) { if self.section.as_deref() == Some(name) { return } self.section = Some(name.to_string()); self.items.clear(); self.chosen = None; self.moved = true; self.loading = true; self.load(Some(name.to_string())); }
    /// Take in what was read. True when something came.
    pub fn poll(&mut self) -> bool {
        let mut any = false;
        while let Ok(r) = self.rx.try_recv() {
            if r.req != self.req { continue } any = true;
            let was = self.chosen.and_then(|i| self.items.get(i)).map(|i| i.path.clone());
            if self.section != r.section { self.moved = true; }
            self.missing = r.missing; self.sections = r.sections; self.section = r.section; self.items = r.items; self.dated_names = r.names; self.two_tier = r.two_tier; self.loading = false;
            let want = self.choose.take().or(was); self.chosen = want.and_then(|w| self.items.iter().position(|i| i.path == w));
        }
        any
    }
    /// Whether the second pane is shown in the section in front: only a section of two tiers has one (row 485), and
    /// there the user's word for it counts.
    pub fn wants_two(&self) -> bool { self.two_tier && self.section.as_ref().and_then(|s| self.pane2.get(s)).copied().unwrap_or(true) }
    /// The folder of the section in front.
    pub fn section_dir(&self) -> Option<PathBuf> { self.section.as_ref().map(|s| self.path.join(s)) }
    pub fn chosen_item(&self) -> Option<&Item> { self.chosen.and_then(|i| self.items.get(i)) }
}

/// The jobs folders this build was started with (`--jobs <folder>`; the installed application will take them from the
/// bookmarks and saved shares flagged so): a folder directly inside one can be made a job by New Job.
static ROOTS: std::sync::OnceLock<Vec<PathBuf>> = std::sync::OnceLock::new();
pub fn set_roots(roots: Vec<PathBuf>) { let _ = ROOTS.set(roots); }
/// ... and the saved shares marked as jobs folders, where each is now (`shares.rs`).
static SHARE_ROOTS: std::sync::RwLock<Vec<PathBuf>> = std::sync::RwLock::new(Vec::new());
pub fn set_share_roots(roots: Vec<PathBuf>) { *SHARE_ROOTS.write().unwrap() = roots; }
pub fn is_jobs_folder(dir: &Path) -> bool { ROOTS.get().is_some_and(|r| r.iter().any(|j| j == dir)) || SHARE_ROOTS.read().unwrap().iter().any(|j| j == dir) }

/// The project type of every job, for now: the built-in one (a jobs folder's own type comes with its step, row 454).
pub fn project_type() -> &'static ufb_projtype::ProjectType { static T: std::sync::OnceLock<ufb_projtype::ProjectType> = std::sync::OnceLock::new(); T.get_or_init(ufb_projtype::ProjectType::builtin) }

/// The local date as `YYMMDD` (a dated item's prefix).
pub fn today() -> String { let c = crate::format::local(crate::format::now_secs()); format!("{:02}{:02}{:02}", c.year.rem_euclid(100), c.month, c.day) }
/// The folder of the built-in type's skeletons: beside the executable (a packaged build), in the bundle's resources
/// (macOS), or in the source tree this build was made from (a development build).
pub fn skeletons() -> PathBuf {
    let rel = Path::new("types").join("studio").join("skeletons");
    let exe = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
    let mut tries: Vec<PathBuf> = vec![];
    if let Some(d) = &exe { tries.push(d.join(&rel)); tries.push(d.join("..").join("Resources").join(&rel)); }
    tries.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("resources").join(&rel));
    tries.iter().find(|p| p.is_dir()).cloned().unwrap_or_else(|| tries.pop().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp(name: &str) -> PathBuf { let d = std::env::temp_dir().join(format!("ufb-job-{name}-{}", std::process::id())); let _ = std::fs::remove_dir_all(&d); std::fs::create_dir_all(&d).unwrap(); d }
    #[test]
    fn sections_are_the_top_folders_and_items_the_folders_in_one() {
        let job = temp("read");
        for d in ["3d/sh010/project", "3d/sh010/renders", "3d/sh020", "3d/_t_project_name", "3d/assets", "ae/000000_x", "ae/title", ".ufb/backups", "docs"] { std::fs::create_dir_all(job.join(d)).unwrap(); }
        std::fs::write(job.join("notes.txt"), "x").unwrap(); std::fs::write(job.join("3d").join("file.txt"), "x").unwrap();
        let r = read(&job, None);
        assert_eq!(r.sections, ["3d", "ae", "docs"], "no dot folder, no file"); assert_eq!(r.section.as_deref(), Some("3d"));
        assert_eq!(r.items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(), ["assets", "sh010", "sh020"], "the placeholder and the file are no items");
        let r = read(&job, Some("AE")); assert_eq!(r.section.as_deref(), Some("ae")); assert_eq!(r.items.len(), 1);
        let r = read(&job, Some("gone")); assert_eq!(r.section.as_deref(), Some("3d"), "a section that is not there: the first");
        assert!(read(&job.join("nope"), None).missing);
        // an item with a project and a renders folder shows them; one without shows itself
        assert_eq!(targets(&job.join("3d/sh010")), (job.join("3d/sh010/project"), job.join("3d/sh010/renders")));
        assert_eq!(targets(&job.join("3d/sh020")), (job.join("3d/sh020"), job.join("3d/sh020")));
        // two tiers: one item with both folders is enough; or two items with one each; a flat section has neither
        assert!(read(&job, Some("3d")).two_tier); assert!(!read(&job, Some("ae")).two_tier); assert!(!read(&job, Some("docs")).two_tier);
        assert!(two_tier(&[(false, false), (true, true)])); assert!(two_tier(&[(true, false), (false, true)])); assert!(!two_tier(&[(true, false), (false, false)])); assert!(!two_tier(&[]));
        std::fs::create_dir_all(job.join("ae/title/proxies")).unwrap(); let r = read(&job, Some("ae")); assert_eq!(r.items[0].proxies, Some(job.join("ae/title/proxies")));
        let _ = std::fs::remove_dir_all(&job);
    }
    #[test]
    fn the_date_is_six_digits() { let t = today(); assert_eq!(t.len(), 6); assert!(t.bytes().all(|b| b.is_ascii_digit())); }
}
