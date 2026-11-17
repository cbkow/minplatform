//! The sidebar's places (canon row 463): the home folder and its usual folders, and the volumes - on macOS the
//! folders in `/Volumes` with the startup disk first, on Windows the drive letters with their labels. Looking for the
//! volumes touches the file system, so it is done on a worker thread (`refresh`), when the window becomes active.
use crate::lister::{Msg, Port};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub struct Place { pub label: String, pub path: PathBuf, /// a Phosphor slug
    pub icon: &'static str }

pub fn home() -> PathBuf { std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "C:\\" } else { "/" })) }
/// Home and the folders every home has. (Whether each exists is not asked here: a click finds out, off this thread.)
pub fn fixed() -> Vec<Place> {
    let h = home();
    vec![Place { label: "Home".into(), path: h.clone(), icon: "house" }, Place { label: "Desktop".into(), path: h.join("Desktop"), icon: "desktop" },
        Place { label: "Documents".into(), path: h.join("Documents"), icon: "file-text" }, Place { label: "Downloads".into(), path: h.join("Downloads"), icon: "download-simple" }]
}

/// The volumes, and the startup disk's name (the path bar's first segment for a path that is on no other volume).
pub fn volumes() -> (Vec<Place>, String) {
    #[cfg(target_os = "macos")] {
        let (mut out, mut root) = (vec![], None);
        if let Ok(rd) = std::fs::read_dir("/Volumes") { for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned(); if name.starts_with('.') { continue }
            // the startup disk is in /Volumes as a link to "/"
            if e.file_type().is_ok_and(|t| t.is_symlink()) && std::fs::read_link(e.path()).is_ok_and(|t| t == std::path::Path::new("/")) { root = Some(name); continue }
            out.push(Place { label: name, path: e.path(), icon: "hard-drives" });
        } }
        out.sort_by(|a, b| crate::model::natural_key(&a.label).cmp(&crate::model::natural_key(&b.label)));
        let root = root.unwrap_or_else(|| "Startup Disk".to_string());
        out.insert(0, Place { label: root.clone(), path: PathBuf::from("/"), icon: "hard-drive" }); (out, root)
    }
    #[cfg(windows)] {
        use windows::core::HSTRING; use windows::Win32::Storage::FileSystem::{GetLogicalDrives, GetVolumeInformationW};
        let mut out = vec![]; let bits = unsafe { GetLogicalDrives() };
        for i in 0..26u32 { if bits >> i & 1 == 0 { continue } let letter = (b'A' + i as u8) as char; let root = format!("{letter}:\\");
            let mut name = [0u16; 261]; let label = if unsafe { GetVolumeInformationW(&HSTRING::from(root.as_str()), Some(&mut name), None, None, None, None) }.is_ok() { String::from_utf16_lossy(&name[..name.iter().position(|c| *c == 0).unwrap_or(0)]) } else { String::new() };
            out.push(Place { label: if label.is_empty() { format!("Local Disk ({letter}:)") } else { format!("{label} ({letter}:)") }, path: PathBuf::from(root), icon: "hard-drive" }); }
        (out, String::new())
    }
    #[cfg(not(any(target_os = "macos", windows)))] { (vec![Place { label: "File System".into(), path: PathBuf::from("/"), icon: "hard-drive" }], "File System".to_string()) }
}
/// Look for the volumes on a worker thread; the answer comes as `Msg::Places`.
pub fn refresh(port: &Port) { let port = port.clone(); let _ = std::thread::Builder::new().name("ufb-places".into()).spawn(move || { let (volumes, root) = volumes(); port.send(Msg::Places { volumes, root }); }); }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_fixed_places_lie_in_the_home_folder() {
        let f = fixed(); assert_eq!(f.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(), ["Home", "Desktop", "Documents", "Downloads"]);
        assert_eq!(f[0].path, home()); assert!(f[1..].iter().all(|p| p.path.parent() == Some(home().as_path())));
    }
    #[test]
    fn there_is_always_a_volume() { let (v, _root) = volumes(); assert!(!v.is_empty()); #[cfg(unix)] assert!(v.iter().any(|p| p.path == std::path::Path::new("/"))); }
}

/// The favourites the user made (canon row 531): kept in the settings as each folder's identity, so one on a share
/// is the same favourite wherever the share lands. One that leads nowhere on this machine now (its share is not
/// mapped here) is kept, and not shown.
pub fn saved() -> Vec<Place> {
    let text = crate::settings::with(|s| s.string("favorites/list"));
    serde_json::from_str::<serde_json::Value>(&text).ok().and_then(|v| v.as_array().map(|a| a.iter().filter_map(|f| { let path = crate::db::to_native(f["path"].as_str()?)?; let label = f["label"].as_str().filter(|l| !l.is_empty()).map(str::to_string).unwrap_or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string_lossy().into_owned())); Some(Place { label, path, icon: "star" }) }).collect())).unwrap_or_default()
}
fn stored() -> Vec<(String, String)> { let text = crate::settings::with(|s| s.string("favorites/list")); serde_json::from_str::<serde_json::Value>(&text).ok().and_then(|v| v.as_array().map(|a| a.iter().filter_map(|f| Some((f["label"].as_str().unwrap_or("").to_string(), f["path"].as_str()?.to_string()))).collect())).unwrap_or_default() }
fn store(list: &[(String, String)]) { let text = serde_json::Value::Array(list.iter().map(|(l, p)| serde_json::json!({ "label": l, "path": p })).collect()).to_string(); crate::settings::with(|s| s.set_str("favorites/list", &text)); }
/// A folder becomes a favourite (once), under the name given or its own. True: it was added.
pub fn add_favorite(path: &std::path::Path, label: &str) -> bool { let id = crate::db::to_identity(path); let mut l = stored(); if l.iter().any(|f| f.1 == id) { return false } l.push((label.to_string(), id)); store(&l); true }
pub fn remove_favorite(path: &std::path::Path) -> bool { let id = crate::db::to_identity(path); let mut l = stored(); let n = l.len(); l.retain(|f| f.1 != id); if l.len() != n { store(&l); true } else { false } }
pub fn is_favorite(path: &std::path::Path) -> bool { let id = crate::db::to_identity(path); stored().iter().any(|f| f.1 == id) }
