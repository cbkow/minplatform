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
/// (each with its PROJECT TYPE's name - empty: the built-in one; row 454: a job's type is that of its jobs folder)
static SHARE_ROOTS: std::sync::RwLock<Vec<(PathBuf, String)>> = std::sync::RwLock::new(Vec::new());
pub fn set_share_roots(roots: Vec<(PathBuf, String)>) { *SHARE_ROOTS.write().unwrap() = roots; }
pub fn is_jobs_folder(dir: &Path) -> bool { ROOTS.get().is_some_and(|r| r.iter().any(|j| j == dir)) || SHARE_ROOTS.read().unwrap().iter().any(|j| j.0 == dir) }

// ---------------------------------------------------------------- a studio's own types (rows 454, 461, 502)
/// A project type as it is used: what it says, and the folder its skeletons are in.
#[derive(Clone)]
pub struct TypeAt { pub ty: std::sync::Arc<ufb_projtype::ProjectType>, pub skeletons: PathBuf }
/// Where types are looked for, in order: the farm's folder (`<farm>/types` - every machine of the studio reads the
/// same ones) and this machine's settings folder (`types`). A type is a FOLDER there: `type.toml` and `skeletons/`.
static TYPE_DIRS: std::sync::RwLock<Vec<PathBuf>> = std::sync::RwLock::new(Vec::new());
pub fn set_type_dirs(dirs: Vec<PathBuf>) { *TYPE_DIRS.write().unwrap() = dirs; }
/// The types found, by their folder's name (the first place that has a name wins): (name, folder).
pub fn known_types() -> Vec<(String, PathBuf)> { known_in(&TYPE_DIRS.read().unwrap()) }
pub fn known_in(dirs: &[PathBuf]) -> Vec<(String, PathBuf)> {
    let mut out: Vec<(String, PathBuf)> = vec![];
    for d in dirs { let Ok(rd) = std::fs::read_dir(d) else { continue }; let mut here: Vec<(String, PathBuf)> = rd.flatten().filter(|e| e.path().join("type.toml").is_file()).map(|e| (e.file_name().to_string_lossy().into_owned(), e.path())).filter(|(n, _)| !n.starts_with('.')).collect(); here.sort();
        for t in here { if !out.iter().any(|o| o.0.eq_ignore_ascii_case(&t.0)) { out.push(t); } } }
    out
}
/// A type by its name: read from its folder (again when its file changed); Err: why it cannot be used.
pub fn load_type(name: &str) -> Result<TypeAt, String> {
    static CACHE: std::sync::Mutex<Vec<(PathBuf, Option<std::time::SystemTime>, TypeAt)>> = std::sync::Mutex::new(Vec::new());
    let Some((_, dir)) = known_types().into_iter().find(|t| t.0.eq_ignore_ascii_case(name)) else { return Err(format!("The project type \"{name}\" is not there.")) };
    let stamp = std::fs::metadata(dir.join("type.toml")).and_then(|m| m.modified()).ok(); let mut c = CACHE.lock().unwrap();
    if let Some(hit) = c.iter().find(|x| x.0 == dir && x.1 == stamp) { return Ok(hit.2.clone()) }
    let at = TypeAt { ty: std::sync::Arc::new(ufb_projtype::ProjectType::load(&dir)?), skeletons: dir.join("skeletons") };
    c.retain(|x| x.0 != dir); c.push((dir, stamp, at.clone())); Ok(at)
}
/// The type of what lies at `path`: that of the jobs folder it is in; the built-in one when the folder names none, or
/// when its type cannot be read (a job is never left without its sections because a file on a share is broken).
pub fn type_for(path: &Path) -> TypeAt {
    let name = SHARE_ROOTS.read().unwrap().iter().filter(|(root, _)| path.starts_with(root)).max_by_key(|(root, _)| root.as_os_str().len()).map(|(_, n)| n.clone()).unwrap_or_default();
    if !name.is_empty() { if let Ok(at) = load_type(&name) { return at } }
    static BUILTIN: std::sync::OnceLock<std::sync::Arc<ufb_projtype::ProjectType>> = std::sync::OnceLock::new();
    TypeAt { ty: BUILTIN.get_or_init(|| std::sync::Arc::new(ufb_projtype::ProjectType::builtin())).clone(), skeletons: skeletons() }
}

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
    #[test]
    fn a_studios_types_are_found_by_their_folders_the_first_place_winning() {
        let root = std::env::temp_dir().join(format!("ufb-types-{}", std::process::id())); let (farm, own) = (root.join("farm"), root.join("own"));
        let make = |dir: &Path, name: &str, label: &str| { std::fs::create_dir_all(dir.join(name)).unwrap(); std::fs::write(dir.join(name).join("type.toml"), format!("name = \"{label}\"\n[[section]]\nfolder = \"shots\"\nkind = \"folder\"\n")).unwrap(); };
        make(&farm, "episodic", "from the farm"); make(&own, "Episodic", "this machine's"); make(&own, "shorts", "shorts"); std::fs::create_dir_all(own.join("not-a-type")).unwrap(); make(&own, ".hidden", "x");
        let found = known_in(&[farm.clone(), own.clone(), root.join("absent")]);
        assert_eq!(found.iter().map(|t| t.0.as_str()).collect::<Vec<_>>(), ["episodic", "shorts"], "a folder without type.toml is no type; a name is taken once, the farm's first");
        assert_eq!(found[0].1, farm.join("episodic")); assert_eq!(ufb_projtype::ProjectType::load(&found[0].1).unwrap().name, "from the farm");
        let _ = std::fs::remove_dir_all(&root);
    }
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
