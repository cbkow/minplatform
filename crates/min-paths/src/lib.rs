//! `min-paths` — the min*/QCView family's path kit (canon §22, rows 119–126, ruled 2026-11-06).
//!
//! One copy of what every app in the family carried separately: `file://` URLs and `text/uri-list` payloads (ufb's codec,
//! corrected), the `{vol, rel}` media refs and cross-OS string translation over ONE mapping table (minNotes' + ufb's rules,
//! boundary check everywhere), the ONE image-sequence detector (QCView's public import rules, byte-compatible), the network /
//! volatile source classifiers (minNotes' rules via `statfs` / Win32), same-volume by device identity and the drop guards
//! (ufb's, with the descendant guard), clipboard file lists (ufb's AppKit / CF_HDROP code), safe file names with Windows
//! reserved names, and the family's `ufb:///` / `minnotes:` / `qcview://` link codecs.
//!
//! Pure where it can be: every function that has an OS rule takes an [`Os`] so the other platform's behaviour is testable on
//! this one; the sequence detector takes the directory listing as a slice. The only IO is `metadata` / `statfs` / Win32
//! volume queries in `volume` and `classify`, and the pasteboard in `clipboard`.
#![forbid(unsafe_op_in_unsafe_fn)]

pub mod classify;
pub mod clipboard;
pub mod links;
pub mod map;
pub mod names;
pub mod sequence;
pub mod url;
pub mod volume;

/// The family's OS tags: `"win"`, `"mac"`, `"lin"` (minNotes `currentOsTag`, ufb `current_os_tag`, MinRender `currentOsTag`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum Os { Win, Mac, Lin }

impl Os {
    pub const fn current() -> Os {
        if cfg!(windows) { Os::Win } else if cfg!(target_os = "macos") { Os::Mac } else { Os::Lin }
    }
    pub const fn tag(self) -> &'static str { match self { Os::Win => "win", Os::Mac => "mac", Os::Lin => "lin" } }
    pub fn from_tag(tag: &str) -> Option<Os> {
        match tag { "win" | "windows" => Some(Os::Win), "mac" | "macos" => Some(Os::Mac), "lin" | "linux" => Some(Os::Lin), _ => None }
    }
    pub const fn is_windows(self) -> bool { matches!(self, Os::Win) }
    /// The separator the OS writes.
    pub const fn sep(self) -> char { if self.is_windows() { '\\' } else { '/' } }
}

/// `true` for `X:` at the start of `s` (ASCII letter + colon).
pub(crate) fn has_drive(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic()
}

/// Lexical clean-up in the spirit of `QDir::cleanPath`: `\` → `/` when `os` is Windows, duplicate separators collapsed,
/// `.` and `..` resolved (never above the root), trailing separator dropped (except the root itself). No IO.
pub fn clean_path(path: &str, os: Os) -> String {
    let s: String = if os.is_windows() { path.replace('\\', "/") } else { path.to_string() };
    let unc = os.is_windows() && s.starts_with("//");
    let absolute = s.starts_with('/');
    let mut out: Vec<&str> = Vec::new();
    for part in s.split('/') {
        match part {
            "" | "." => {}
            ".." => { if !out.is_empty() && *out.last().unwrap() != ".." { out.pop(); } else if !absolute { out.push(".."); } }
            p => out.push(p),
        }
    }
    let body = out.join("/");
    if unc { format!("//{body}") }
    else if absolute { format!("/{body}") }
    else if body.is_empty() { ".".to_string() }
    else { body }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn clean_path_rules() {
        assert_eq!(clean_path("/a/b/../c/./d/", Os::Mac), "/a/c/d");
        assert_eq!(clean_path("C:\\x\\\\y\\..\\z", Os::Win), "C:/x/z");
        assert_eq!(clean_path("\\\\srv\\share\\a\\..\\b", Os::Win), "//srv/share/b");
        assert_eq!(clean_path("/..", Os::Mac), "/");
        assert_eq!(clean_path("a/../..", Os::Mac), "..");
        assert_eq!(clean_path("/Volumes/x\\y", Os::Mac), "/Volumes/x\\y");   // a POSIX backslash is a filename byte
    }
}
