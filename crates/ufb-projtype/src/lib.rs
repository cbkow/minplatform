//! The project type (canon rows 454, 461): a small file that says what UFB 1.4.2 said in code and in a bundled folder
//! tree - how a job's folder is named, which sections it has, and what "Add" makes in each. No scripting, no
//! conditions. A type is a FOLDER: `type.toml` and `skeletons/<name>/...` for its shot sections. The owner's layout is
//! the built-in type ([`ProjectType::builtin`]), written in the same form; its skeletons ship with the application.
//!
//! What this crate keeps from 1.4.2, rule for rule (`core/src/jobs.rs`, `core/src/file_ops.rs`): the job's folder name;
//! the dated name and its letter; the shot's copy with `template.<ext>` / `_template.<ext>` renamed and `.gitkeep`
//! left out; a section's kind found by its folder name WITHOUT regard to case. What it mends: a copy that fails takes
//! away what it made (1.4.2 left half a folder); a new job no longer receives the placeholder folders (`_t_...`,
//! `000000...`) that 1.4.2 copied into every job and then hid by their names.
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind { Shot, Dated, Folder, None }

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Section {
    /// The section's folder, directly inside the job.
    pub folder: String,
    /// What the section is called in the window; the folder's name when absent.
    #[serde(default)] pub label: Option<String>,
    pub kind: Kind,
    /// `shot` only: the folder under the type's `skeletons/` that Add copies.
    #[serde(default)] pub skeleton: Option<String>,
    /// `shot` only: files named `<stem>.<ext>` for a stem listed here become `<item>_v001.<ext>`.
    #[serde(default = "default_rename")] pub rename: Vec<String>,
    /// Folders made inside the section when the job is made.
    #[serde(default)] pub folders: Vec<String>,
    /// The columns the section starts with. Kept as written; their shape belongs to the metadata side.
    #[serde(default)] pub columns: Vec<toml::Value>,
}
fn default_rename() -> Vec<String> { vec!["template".into(), "_template".into()] }
impl Section { pub fn label(&self) -> &str { self.label.as_deref().unwrap_or(&self.folder) } }

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectType {
    pub name: String,
    /// `{number}` and `{name}` with anything between them.
    #[serde(default = "default_job_name")] pub job_name: String,
    #[serde(default, rename = "section")] pub sections: Vec<Section>,
}
fn default_job_name() -> String { "{number}_{name}".into() }

const BUILTIN: &str = include_str!("../types/studio/type.toml");
/// What a name may not hold on Windows; a type and a job are shared by both systems.
const FORBIDDEN: [char; 9] = ['\\', '/', ':', '*', '?', '"', '<', '>', '|'];

/// A single folder name that is safe on both systems: not empty, not `.` / `..`, none of Windows' characters, no
/// control character, no space or dot at its end (Windows drops them).
pub fn check_name(what: &str, s: &str) -> Result<(), String> {
    if s.is_empty() { return Err(format!("{what}: the name is empty")) }
    if s == "." || s == ".." { return Err(format!("{what}: \"{s}\" is not a name")) }
    if let Some(c) = s.chars().find(|c| FORBIDDEN.contains(c) || c.is_control()) { return Err(format!("{what}: \"{s}\" holds {c:?}, which a file name cannot")) }
    if s.ends_with(' ') || s.ends_with('.') || s.starts_with(' ') { return Err(format!("{what}: \"{s}\" starts or ends with a space or a dot")) }
    Ok(())
}

impl ProjectType {
    pub fn builtin() -> ProjectType { ProjectType::parse(BUILTIN).expect("the built-in type") }

    pub fn parse(text: &str) -> Result<ProjectType, String> {
        let t: ProjectType = toml::from_str(text).map_err(|e| e.to_string())?;
        t.check()?; Ok(t)
    }
    /// A type's folder: `<dir>/type.toml`.
    pub fn load(dir: &Path) -> Result<ProjectType, String> {
        let f = dir.join("type.toml");
        ProjectType::parse(&std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?)
    }

    fn check(&self) -> Result<(), String> {
        if self.name.trim().is_empty() { return Err("the type has no name".into()) }
        if !self.job_name.contains("{name}") && !self.job_name.contains("{number}") { return Err("job_name holds neither {number} nor {name}".into()) }
        let mut seen: Vec<String> = Vec::new();
        for s in &self.sections {
            check_name("section", &s.folder)?;
            let low = s.folder.to_lowercase();
            if seen.contains(&low) { return Err(format!("section \"{}\" is there twice", s.folder)) }
            seen.push(low);
            match (s.kind, &s.skeleton) {
                (Kind::Shot, None) => return Err(format!("section \"{}\" is a shot section and names no skeleton", s.folder)),
                (Kind::Shot, Some(k)) => check_name("skeleton", k)?,
                (_, Some(_)) => return Err(format!("section \"{}\": only a shot section has a skeleton", s.folder)),
                _ => {}
            }
            for f in &s.folders { check_name("folder", f)?; }
            for r in &s.rename { check_name("rename", r)?; }
        }
        Ok(())
    }

    /// The section for a folder of a job, by its name without regard to case (1.4.2's rule); `None` = a plain folder.
    pub fn section(&self, folder: &str) -> Option<&Section> { let low = folder.to_lowercase(); self.sections.iter().find(|s| s.folder.to_lowercase() == low) }
    /// What Add does in a folder of a job: a folder the type does not name is a plain folder.
    pub fn kind(&self, folder: &str) -> Kind { self.section(folder).map(|s| s.kind).unwrap_or(Kind::Folder) }

    /// A job's folder name from what was typed. 1.4.2's rule: Windows' characters are dropped, the ends trimmed, each
    /// white-space character becomes `_`; when one part is empty the other stands alone. Empty = nothing usable typed.
    pub fn job_folder_name(&self, number: &str, name: &str) -> String {
        let clean = |s: &str| s.chars().filter(|c| !FORBIDDEN.contains(c)).collect::<String>().trim().replace(char::is_whitespace, "_");
        let (n, nm) = (clean(number), clean(name));
        if n.is_empty() || nm.is_empty() { return if n.is_empty() { nm } else { n } }
        self.job_name.replace("{number}", &n).replace("{name}", &nm)
    }

    /// Make a job: its folder under `parent`, every section's folder, and the folders a section starts with. The job's
    /// path. Fails when the parent is not a folder, nothing usable was typed, or the job is already there; whatever a
    /// failed attempt made is taken away again.
    pub fn new_job(&self, parent: &Path, number: &str, name: &str) -> Result<PathBuf, String> {
        if !parent.is_dir() { return Err(format!("Folder does not exist: {}", parent.display())) }
        let folder = self.job_folder_name(number, name);
        check_name("job", &folder)?;
        let job = parent.join(&folder);
        if job.exists() { return Err(format!("\"{folder}\" already exists")) }
        let made = (|| -> std::io::Result<()> {
            std::fs::create_dir(&job)?;
            for s in &self.sections {
                std::fs::create_dir(job.join(&s.folder))?;
                for f in &s.folders { std::fs::create_dir(job.join(&s.folder).join(f))?; }
            }
            Ok(())
        })();
        if let Err(e) = made { let _ = std::fs::remove_dir_all(&job); return Err(format!("{}: {e}", job.display())) }
        Ok(job)
    }

    /// Add an item to a section's folder `dir` (`<job>/<section>`), by the section's kind. `skeletons` is the type's
    /// `skeletons` folder; `today` is the local date as `YYMMDD` (the caller's clock - this crate has none). The new
    /// item's path.
    pub fn add_item(&self, dir: &Path, name: &str, skeletons: &Path, today: &str) -> Result<PathBuf, String> {
        if !dir.is_dir() { return Err(format!("Folder does not exist: {}", dir.display())) }
        let section = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let name = name.trim();
        check_name("name", name)?;
        match self.section(&section) {
            Some(s) if s.kind == Kind::None => Err(format!("Nothing is added in \"{}\"", s.label())),
            Some(s) if s.kind == Kind::Shot => {
                let from = skeletons.join(s.skeleton.as_deref().unwrap_or_default());
                if !from.is_dir() { return Err(format!("Template not found: {}", from.display())) }
                let to = dir.join(name);
                if to.exists() { return Err(format!("\"{name}\" already exists")) }
                std::fs::create_dir(&to).map_err(|e| format!("{}: {e}", to.display()))?;
                if let Err(e) = copy_skeleton(&from, &to, name, &s.rename) { let _ = std::fs::remove_dir_all(&to); return Err(e) }
                Ok(to)
            }
            Some(s) if s.kind == Kind::Dated => {
                let to = dir.join(dated_name(&dir_names(dir)?, today, name)?);
                std::fs::create_dir(&to).map_err(|e| format!("{}: {e}", to.display()))?;
                Ok(to)
            }
            _ => {
                let to = dir.join(name);
                if to.exists() { return Err(format!("\"{name}\" already exists")) }
                std::fs::create_dir(&to).map_err(|e| format!("{}: {e}", to.display()))?;
                Ok(to)
            }
        }
    }
}

fn dir_names(dir: &Path) -> Result<Vec<String>, String> {
    Ok(std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false)).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
}

/// `<YYMMDD><letter>_<name>`: the letter is the first of a-z that no folder of that day carries yet, whatever its name
/// (1.4.2's rule: `271015a_foo` there makes the next `271015b_bar`). Case is not regarded. The window shows this very
/// name before the folder is made (1.4.2's dialog always showed `a`).
pub fn dated_name(existing: &[String], today: &str, name: &str) -> Result<String, String> {
    if today.len() != 6 || !today.bytes().all(|b| b.is_ascii_digit()) { return Err(format!("\"{today}\" is not a date as YYMMDD")) }
    let low: Vec<String> = existing.iter().map(|n| n.to_lowercase()).collect();
    for c in 'a'..='z' {
        let prefix = format!("{today}{c}");
        if !low.iter().any(|n| n.starts_with(&prefix)) { return Ok(format!("{prefix}_{name}")) }
    }
    Err(format!("Every letter for {today} is taken (a to z)"))
}

/// `template.blend` / `_template.aep` → `<item>_v001.blend` / `.aep` for the stems the section lists; everything after
/// the stem's dot is kept. Any other name is kept as it is.
pub fn renamed(file: &str, item: &str, stems: &[String]) -> String {
    for stem in stems {
        if let Some(ext) = file.strip_prefix(stem.as_str()).and_then(|r| r.strip_prefix('.')) { if !ext.is_empty() { return format!("{item}_v001.{ext}") } }
    }
    file.to_string()
}

fn copy_skeleton(from: &Path, to: &Path, item: &str, stems: &[String]) -> Result<(), String> {
    for e in std::fs::read_dir(from).map_err(|e| format!("{}: {e}", from.display()))? {
        let e = e.map_err(|e| e.to_string())?;
        let name = e.file_name().to_string_lossy().into_owned();
        if name == ".gitkeep" { continue }
        let src = e.path();
        if e.file_type().map_err(|e| format!("{}: {e}", src.display()))?.is_dir() {
            let dst = to.join(&name);   // a folder keeps its name: only files are renamed
            std::fs::create_dir(&dst).map_err(|e| format!("{}: {e}", dst.display()))?;
            copy_skeleton(&src, &dst, item, stems)?;
        } else {
            let dst = to.join(renamed(&name, item, stems));
            std::fs::copy(&src, &dst).map_err(|e| format!("{} -> {}: {e}", src.display(), dst.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A scratch folder that goes away with the test.
    struct Tmp(PathBuf);
    impl Tmp { fn new(tag: &str) -> Tmp { let p = std::env::temp_dir().join(format!("ufb-projtype-{tag}-{}", std::process::id())); let _ = fs::remove_dir_all(&p); fs::create_dir_all(&p).unwrap(); Tmp(p) } }
    impl Drop for Tmp { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
    fn tree(root: &Path) -> Vec<String> {
        fn walk(d: &Path, root: &Path, out: &mut Vec<String>) { for e in fs::read_dir(d).unwrap().flatten() { let p = e.path(); out.push(p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/")); if p.is_dir() { walk(&p, root, out) } } }
        let mut v = Vec::new(); walk(root, root, &mut v); v.sort(); v
    }

    #[test]
    fn the_builtin_type_is_the_layout_of_1_4_2() {
        let t = ProjectType::builtin();
        let kinds: Vec<(&str, Kind)> = t.sections.iter().map(|s| (s.folder.as_str(), s.kind)).collect();
        assert_eq!(kinds, vec![("3d", Kind::Shot), ("ae", Kind::Shot), ("assets", Kind::Dated), ("audition", Kind::None), ("docs", Kind::Folder),
            ("illustrator", Kind::Shot), ("photoshop", Kind::Shot), ("postings", Kind::Dated), ("premiere", Kind::Shot)]);
        assert_eq!(t.kind("AE"), Kind::Shot);             // by name, without regard to case
        assert_eq!(t.kind("Audition"), Kind::None);
        assert_eq!(t.kind("client_notes"), Kind::Folder); // a folder the type does not name
        assert_eq!(t.section("3D").unwrap().label(), "3d");
    }

    #[test]
    fn a_job_is_named_as_1_4_2_named_it() {
        let t = ProjectType::builtin();
        assert_eq!(t.job_folder_name("000123", "Spring Spot"), "000123_Spring_Spot");
        assert_eq!(t.job_folder_name(" 12 ", "a/b:c"), "12_abc");
        assert_eq!(t.job_folder_name("", "Only Name"), "Only_Name");
        assert_eq!(t.job_folder_name("77", "  "), "77");
        assert_eq!(t.job_folder_name("7", "a  b"), "7_a__b");   // each white-space character, as the core did (the old dialog's preview differed)
        assert_eq!(t.job_folder_name("?", "*"), "");
        let dash = ProjectType::parse("name = \"x\"\njob_name = \"{name}-{number}\"").unwrap();
        assert_eq!(dash.job_folder_name("12", "spot"), "spot-12");
        assert_eq!(dash.job_folder_name("", "spot"), "spot");
    }

    #[test]
    fn a_new_job_has_its_sections_and_no_placeholders() {
        let tmp = Tmp::new("job"); let t = ProjectType::builtin();
        let job = t.new_job(&tmp.0, "000123", "Spring Spot").unwrap();
        assert_eq!(job.file_name().unwrap(), "000123_Spring_Spot");
        assert_eq!(tree(&job), vec!["3d", "3d/assets", "ae", "assets", "audition", "audition/outputs", "audition/projects", "docs", "docs/decks",
            "illustrator", "photoshop", "postings", "premiere"]);
        assert!(t.new_job(&tmp.0, "000123", "Spring Spot").unwrap_err().contains("already exists"));
        assert!(t.new_job(&tmp.0, "", "").is_err());
        assert!(t.new_job(&tmp.0.join("nowhere"), "1", "a").is_err());
        assert_eq!(fs::read_dir(&tmp.0).unwrap().count(), 1);   // the failures left nothing
    }

    #[test]
    fn add_follows_the_sections_kind() {
        let tmp = Tmp::new("add"); let t = ProjectType::builtin();
        let sk = tmp.0.join("skeletons");
        for d in ["3d/cache", "3d/playblasts", "3d/project", "3d/renders", "ae/project", "ae/proxies", "ae/renders"] { fs::create_dir_all(sk.join(d)).unwrap(); }
        for f in ["3d/cache/.gitkeep", "3d/project/template.blend", "3d/project/notes.txt", "ae/project/_template.aep", "ae/renders/.gitkeep"] { fs::write(sk.join(f), b"x").unwrap(); }
        let job = t.new_job(&tmp.0, "1", "job").unwrap();

        // shot: the skeleton, the template file renamed, .gitkeep left out
        let shot = t.add_item(&job.join("3d"), " sh010 ", &sk, "261115").unwrap();
        assert_eq!(tree(&shot), vec!["cache", "playblasts", "project", "project/notes.txt", "project/sh010_v001.blend", "renders"]);
        let comp = t.add_item(&job.join("ae"), "sh010_comp", &sk, "261115").unwrap();
        assert_eq!(tree(&comp), vec!["project", "project/sh010_comp_v001.aep", "proxies", "renders"]);
        assert!(t.add_item(&job.join("3d"), "sh010", &sk, "261115").unwrap_err().contains("already exists"));
        // a skeleton that is not there: said, and nothing is left behind
        assert!(t.add_item(&job.join("premiere"), "cut", &sk, "261115").unwrap_err().contains("Template not found"));
        assert_eq!(fs::read_dir(job.join("premiere")).unwrap().count(), 0);

        // dated: the letter is the day's, whatever the name
        let p = job.join("postings");
        assert_eq!(t.add_item(&p, "first", &sk, "261115").unwrap().file_name().unwrap(), "261115a_first");
        assert_eq!(t.add_item(&p, "second", &sk, "261115").unwrap().file_name().unwrap(), "261115b_second");
        assert_eq!(t.add_item(&p, "next_day", &sk, "261116").unwrap().file_name().unwrap(), "261116a_next_day");

        // folder, a folder the type does not name, and none
        assert!(t.add_item(&job.join("docs"), "scripts", &sk, "261115").unwrap().is_dir());
        fs::create_dir(job.join("extra")).unwrap();
        assert!(t.add_item(&job.join("extra"), "thing", &sk, "261115").unwrap().is_dir());
        assert!(t.add_item(&job.join("audition"), "x", &sk, "261115").is_err());
        // names that are not names
        for bad in ["", "  ", "a/b", "..", "a:b", "dot."] { assert!(t.add_item(&job.join("docs"), bad, &sk, "261115").is_err(), "{bad:?}"); }
    }

    #[test]
    fn dated_names_and_renames() {
        let have = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(dated_name(&have(&["261115a_x", "261115B_y", "261114c_z"]), "261115", "new").unwrap(), "261115c_new");
        let full: Vec<String> = ('a'..='z').map(|c| format!("261115{c}_x")).collect();
        assert!(dated_name(&full, "261115", "new").is_err());
        assert!(dated_name(&[], "2611", "new").is_err());
        let stems = default_rename();
        assert_eq!(renamed("template.blend", "sh010", &stems), "sh010_v001.blend");
        assert_eq!(renamed("_template.prproj", "cut", &stems), "cut_v001.prproj");
        assert_eq!(renamed("template.tar.gz", "a", &stems), "a_v001.tar.gz");
        for same in ["template", "templates.txt", "my_template.aep", "__template.aep", "template."] { assert_eq!(renamed(same, "a", &stems), same); }
    }

    #[test]
    fn a_type_that_is_wrong_is_refused_with_its_reason() {
        let bad = |t: &str| ProjectType::parse(t).unwrap_err();
        assert!(bad("job_name = \"x\"").contains("name"));
        assert!(bad("name = \"x\"\njob_name = \"fixed\"").contains("job_name"));
        assert!(bad("name = \"x\"\n[[section]]\nfolder = \"a\"\nkind = \"shot\"").contains("skeleton"));
        assert!(bad("name = \"x\"\n[[section]]\nfolder = \"a\"\nkind = \"dated\"\nskeleton = \"a\"").contains("only a shot"));
        assert!(bad("name = \"x\"\n[[section]]\nfolder = \"a\"\nkind = \"folder\"\n[[section]]\nfolder = \"A\"\nkind = \"none\"").contains("twice"));
        assert!(bad("name = \"x\"\n[[section]]\nfolder = \"a/b\"\nkind = \"folder\"").contains("cannot"));
        assert!(bad("name = \"x\"\n[[section]]\nfolder = \"a\"\nkind = \"script\"").contains("kind"));
        assert!(bad("name = \"x\"\nrun = \"rm\"").contains("unknown field"));
        // a studio's own, with a label and columns kept as written
        let t = ProjectType::parse("name = \"Other\"\njob_name = \"{name}\"\n[[section]]\nfolder = \"comp\"\nlabel = \"Compositing\"\nkind = \"shot\"\nskeleton = \"nuke\"\nrename = [\"base\"]\ncolumns = [{ name = \"Status\" }]").unwrap();
        assert_eq!(t.section("COMP").unwrap().label(), "Compositing");
        assert_eq!(t.sections[0].columns.len(), 1);
        assert_eq!(renamed("base.nk", "sh1", &t.sections[0].rename), "sh1_v001.nk");
    }
}
