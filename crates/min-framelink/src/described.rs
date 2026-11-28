//! How a provider makes itself known. One JSON file a provider in a per-user folder:
//!
//! ```json
//! { "version": 1, "name": "Example Reader", "executable": "/path/to/reader", "args": ["serve"], "extensions": ["xyz"] }
//! ```
//!
//! A provider writes its own file (at its first start, or its installer does). A host reads the folder when it
//! starts and learns which file types it may hand to which program: none of that is written into the host.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Described {
    /// The description's own version: 1.
    pub version: u32,
    /// The provider's name as a person reads it.
    pub name: String,
    pub executable: PathBuf,
    /// What the executable is started with to speak the link on its standard input and output.
    #[serde(default)] pub args: Vec<String>,
    /// File extensions it opens, lower case, without the dot.
    pub extensions: Vec<String>,
}

/// The folder: `MIN_FRAMELINK_DIR` when set, else `minplatform/frame-providers` in the user's application-support
/// folder (macOS `~/Library/Application Support`, Windows `%APPDATA%`, else `$XDG_CONFIG_HOME` or `~/.config`).
pub fn dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("MIN_FRAMELINK_DIR") { if !d.is_empty() { return Some(PathBuf::from(d)) } }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
    let base = if cfg!(target_os = "macos") { home.map(|h| h.join("Library").join("Application Support")) }
        else if cfg!(windows) { std::env::var_os("APPDATA").map(PathBuf::from).or_else(|| home.map(|h| h.join("AppData").join("Roaming"))) }
        else { std::env::var_os("XDG_CONFIG_HOME").filter(|x| !x.is_empty()).map(PathBuf::from).or_else(|| home.map(|h| h.join(".config"))) };
    base.map(|b| b.join("minplatform").join("frame-providers"))
}

/// Every provider described in the folder whose executable is there, by file name. A file that does not parse, is of
/// another version, or names a program that is gone is passed over.
pub fn installed() -> Vec<Described> { dir().map(|d| installed_in(&d)).unwrap_or_default() }
pub fn installed_in(dir: &Path) -> Vec<Described> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
    files.sort();
    files.iter().filter_map(|f| serde_json::from_slice::<Described>(&std::fs::read(f).ok()?).ok()).filter(|d| d.version == 1 && d.executable.is_file())
        .map(|mut d| { d.extensions = d.extensions.iter().map(|e| e.trim_start_matches('.').to_ascii_lowercase()).filter(|e| !e.is_empty()).collect(); d }).collect()
}

/// A provider's own call: write (or renew) its description as `<file_stem>.json`. Nothing is written when the file
/// already says the same.
pub fn describe(file_stem: &str, d: &Described) -> std::io::Result<()> {
    let dir = dir().ok_or_else(|| std::io::Error::other("no folder for provider descriptions"))?;
    let file = dir.join(format!("{file_stem}.json"));
    let text = serde_json::to_vec_pretty(d).map_err(std::io::Error::other)?;
    if std::fs::read(&file).is_ok_and(|old| old == text) { return Ok(()) }
    std::fs::create_dir_all(&dir)?;
    std::fs::write(file, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn only_what_is_there_and_parses() {
        let dir = std::env::temp_dir().join(format!("min-framelink-described-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir); std::fs::create_dir_all(&dir).unwrap();
        let exe = std::env::current_exe().unwrap();
        let good = Described { version: 1, name: "A".into(), executable: exe.clone(), args: vec!["serve".into()], extensions: vec![".XYZ".into(), "abc".into()] };
        std::fs::write(dir.join("a.json"), serde_json::to_vec(&good).unwrap()).unwrap();
        std::fs::write(dir.join("b.json"), serde_json::to_vec(&Described { executable: "/nowhere/at/all".into(), ..good.clone() }).unwrap()).unwrap();
        std::fs::write(dir.join("c.json"), b"not json").unwrap();
        std::fs::write(dir.join("d.json"), serde_json::to_vec(&Described { version: 2, ..good.clone() }).unwrap()).unwrap();
        let found = installed_in(&dir);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].extensions, vec!["xyz", "abc"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
