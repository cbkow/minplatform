//! The path bar's arithmetic (canon rows 449 / 463): a path as segments to click - the volume or the drive first -,
//! which of them are shown when they do not all fit (the MIDDLE ones give way to one ellipsis; the last stay), and
//! what a typed path means (`~` is home).
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct Segment { pub label: String, pub path: PathBuf }

/// The segments of `path` written as text. `windows`: drive letters, UNC shares and backslashes; else a Unix path,
/// where `/Volumes/<name>` is a volume by its name and anything else starts at the startup disk (`root_label`).
pub fn segments_of(path: &str, windows: bool, root_label: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = vec![];
    if windows {
        let p = path.replace('/', "\\");
        let (first, rest) = if let Some(unc) = p.strip_prefix("\\\\") {
            let mut it = unc.splitn(3, '\\'); let (server, share) = (it.next().unwrap_or(""), it.next().unwrap_or(""));
            if server.is_empty() { return out }
            (if share.is_empty() { format!("\\\\{server}") } else { format!("\\\\{server}\\{share}") }, it.next().unwrap_or("").to_string())
        } else if p.len() >= 2 && p.as_bytes()[1] == b':' { (format!("{}\\", &p[..2]), p[2..].to_string()) } else { return out };
        out.push((first.trim_end_matches('\\').to_string(), first.clone()));
        let mut at = first;
        for part in rest.split('\\').filter(|s| !s.is_empty()) { if !at.ends_with('\\') { at.push('\\'); } at.push_str(part); out.push((part.to_string(), at.clone())); }
    } else {
        if !path.starts_with('/') { return out }
        let parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let (mut at, from) = if parts.len() >= 2 && parts[0] == "Volumes" { out.push((parts[1].to_string(), format!("/Volumes/{}", parts[1]))); (format!("/Volumes/{}", parts[1]), 2) } else { out.push((root_label.to_string(), "/".to_string())); (String::new(), 0) };
        for part in &parts[from..] { at.push('/'); at.push_str(part); out.push((part.to_string(), at.clone())); }
    }
    out
}
pub fn segments(path: &Path, root_label: &str) -> Vec<Segment> { segments_of(&path.to_string_lossy(), cfg!(windows), root_label).into_iter().map(|(label, p)| Segment { label, path: PathBuf::from(p) }).collect() }

/// Which segments are shown in `room`, given each one's width, the separator's and the ellipsis': `Some(i)` a
/// segment, `None` the ellipsis that stands for the middle ones. The first (the volume) and the last stay; more of
/// the end is shown while it fits. When not even first, ellipsis and last fit, the first gives way too.
pub fn elide(widths: &[f64], sep: f64, dots: f64, room: f64) -> Vec<Option<usize>> {
    let n = widths.len(); let all = || (0..n).map(Some).collect::<Vec<_>>();
    if n <= 2 || widths.iter().sum::<f64>() + sep * (n as f64 - 1.0) <= room { return all() }
    let mut used = widths[0] + sep + dots + sep + widths[n - 1];
    if used > room { return vec![None, Some(n - 1)] }
    let mut from = n - 1;
    while from > 1 && used + widths[from - 1] + sep <= room { from -= 1; used += widths[from] + sep; }
    if from == 1 { return all() }
    std::iter::once(Some(0)).chain(std::iter::once(None)).chain((from..n).map(Some)).collect()
}

/// A typed (or pasted) path as a path: surrounding blanks and quotes go, `~` is `home`, and a path that does not
/// start at a root is taken from `current`.
pub fn typed_path(text: &str, home: &Path, current: &Path) -> Option<PathBuf> {
    let t = text.trim().trim_matches(|c| c == '"' || c == '\'').trim(); if t.is_empty() { return None }
    if t == "~" { return Some(home.to_path_buf()) }
    if let Some(rest) = t.strip_prefix("~/").or_else(|| t.strip_prefix("~\\")) { return Some(home.join(rest)) }
    let p = Path::new(t);
    Some(if p.is_absolute() || (cfg!(windows) && (t.starts_with("\\\\") || t.as_bytes().get(1) == Some(&b':'))) { p.to_path_buf() } else { current.join(p) })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn labels(v: &[(String, String)]) -> Vec<&str> { v.iter().map(|s| s.0.as_str()).collect() }
    #[test]
    fn a_unix_path_starts_at_its_volume() {
        let s = segments_of("/Volumes/Raid/jobs/sp_010/renders", false, "Startup");
        assert_eq!(labels(&s), ["Raid", "jobs", "sp_010", "renders"]); assert_eq!(s[0].1, "/Volumes/Raid"); assert_eq!(s[3].1, "/Volumes/Raid/jobs/sp_010/renders");
        let s = segments_of("/Users/someone/Desktop/", false, "Startup"); assert_eq!(labels(&s), ["Startup", "Users", "someone", "Desktop"]); assert_eq!((s[0].1.as_str(), s[1].1.as_str(), s[3].1.as_str()), ("/", "/Users", "/Users/someone/Desktop"));
        assert_eq!(segments_of("/", false, "Startup"), [("Startup".to_string(), "/".to_string())]);
        assert_eq!(labels(&segments_of("/Volumes", false, "Startup")), ["Startup", "Volumes"], "the folder of volumes itself lies on the startup disk");
        assert!(segments_of("relative/path", false, "Startup").is_empty());
    }
    #[test]
    fn a_windows_path_starts_at_its_drive_or_share() {
        let s = segments_of("C:\\jobs\\sp_010", true, ""); assert_eq!(labels(&s), ["C:", "jobs", "sp_010"]); assert_eq!((s[0].1.as_str(), s[1].1.as_str(), s[2].1.as_str()), ("C:\\", "C:\\jobs", "C:\\jobs\\sp_010"));
        assert_eq!(segments_of("D:/", true, ""), [("D:".to_string(), "D:\\".to_string())]); assert_eq!(segments_of("D:", true, ""), [("D:".to_string(), "D:\\".to_string())]);
        let s = segments_of("\\\\server\\share\\jobs\\a", true, ""); assert_eq!(labels(&s), ["\\\\server\\share", "jobs", "a"]); assert_eq!(s[2].1, "\\\\server\\share\\jobs\\a");
        assert_eq!(labels(&segments_of("\\\\server", true, "")), ["\\\\server"]); assert!(segments_of("jobs\\a", true, "").is_empty());
    }
    #[test]
    fn the_middle_gives_way_and_the_end_stays() {
        let w = [60.0, 50.0, 50.0, 50.0, 50.0, 80.0];   // with separators of 10: 390 in all
        assert_eq!(elide(&w, 10.0, 20.0, 400.0), [Some(0), Some(1), Some(2), Some(3), Some(4), Some(5)]);
        // 60 + 10 + 20 + 10 + 80 = 180; each further segment from the end costs 60
        assert_eq!(elide(&w, 10.0, 20.0, 389.0), [Some(0), None, Some(2), Some(3), Some(4), Some(5)]);
        assert_eq!(elide(&w, 10.0, 20.0, 300.0), [Some(0), None, Some(3), Some(4), Some(5)]); assert_eq!(elide(&w, 10.0, 20.0, 299.0), [Some(0), None, Some(4), Some(5)]);
        assert_eq!(elide(&w, 10.0, 20.0, 240.0), [Some(0), None, Some(4), Some(5)]); assert_eq!(elide(&w, 10.0, 20.0, 239.0), [Some(0), None, Some(5)]);
        assert_eq!(elide(&w, 10.0, 20.0, 180.0), [Some(0), None, Some(5)]); assert_eq!(elide(&w, 10.0, 20.0, 179.0), [None, Some(5)], "not even the volume fits: the last stays");
        assert_eq!(elide(&[60.0, 500.0], 10.0, 20.0, 100.0), [Some(0), Some(1)], "two segments are never elided (the label is cut instead)");
        assert_eq!(elide(&[60.0, 50.0, 80.0], 10.0, 20.0, 199.0), [Some(0), None, Some(2)]); assert!(elide(&[], 10.0, 20.0, 100.0).is_empty());
    }
    #[test]
    fn a_typed_path() {
        let (home, cur) = (Path::new("/home/u"), Path::new("/jobs/a"));
        assert_eq!(typed_path("~", home, cur), Some(PathBuf::from("/home/u"))); assert_eq!(typed_path("  ~/Desktop ", home, cur), Some(PathBuf::from("/home/u/Desktop")));
        assert_eq!(typed_path("\"~/with space\"", home, cur), Some(PathBuf::from("/home/u/with space")));
        assert_eq!(typed_path("renders/v002", home, cur), Some(PathBuf::from("/jobs/a/renders/v002")), "a path that starts nowhere is taken from the folder in view");
        assert_eq!(typed_path("   ", home, cur), None); assert_eq!(typed_path("~user", home, cur), Some(PathBuf::from("/jobs/a/~user")));
        #[cfg(unix)] assert_eq!(typed_path("/tmp/x", home, cur), Some(PathBuf::from("/tmp/x")));
    }
}
