//! Web links as files (canon row 585; UFB 1.4.2's `core/src/file_ops.rs`, read from its branch `1.4.x`): "New Web
//! Link" writes a Windows-style `.url` file - Explorer and Finder both open one - and UFB opens `.url` and `.webloc`
//! files itself, by handing the address inside to the system, so both kinds behave alike on both systems.
//!
//! One thing is stricter than 1.4.2 (row 591): an address is handed to the system only when it has a scheme and the
//! scheme is not one of the few that name a file or a program of this machine or run a script ([`REFUSED`]). A link
//! file is something anybody can put on a share. A studio's own schemes and those of the services it uses pass.
//! The same rule holds for a Links cell of the item list.
use std::path::{Path, PathBuf};

/// The file is a web link by its extension.
pub fn is_link(path: &Path) -> bool { path.extension().is_some_and(|e| e.eq_ignore_ascii_case("url") || e.eq_ignore_ascii_case("webloc")) }

/// Schemes that are never handed to the system (row 591): they name a file or a program of this machine, run a
/// script, or are system handlers that have been used to start programs from a link. Everything else that is a
/// scheme is handed on - a studio's own services and the applications it uses have schemes of their own.
const REFUSED: [&str; 14] = ["file", "javascript", "vbscript", "data", "blob", "about", "shell", "search", "search-ms", "ms-msdt", "ms-officecmd", "ms-cxh", "jar", "res"];
/// The scheme of an address, in lower case: letters, digits, `+`, `-`, `.` after a first letter, at least two
/// characters (one letter and a colon is a Windows drive), then `:`. A site's name with a port (`example.com:8080`)
/// is none.
fn scheme(t: &str) -> Option<String> {
    let k = t.find(':')?; let (head, rest) = (&t[..k], &t[k + 1..]);
    if head.len() < 2 || !head.starts_with(|c: char| c.is_ascii_alphabetic()) || !head.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) { return None }
    if !rest.starts_with("//") && head.contains('.') && rest.starts_with(|c: char| c.is_ascii_digit()) { return None }
    Some(head.to_ascii_lowercase())
}
/// An address with a scheme may be handed to the system.
fn handed_on(t: &str) -> bool { !t.is_empty() && !t.chars().any(|c| c.is_control() || c.is_whitespace()) && scheme(t).is_some_and(|s| !REFUSED.contains(&s.as_str())) }
/// What is typed as an address, as the system is given it: one with a scheme as it is (any but the refused ones), a
/// bare site's name with `https://` in front (1.4.2's rule). None: nothing typed, a path, a refused scheme.
pub fn address(text: &str) -> Option<String> {
    let t = text.trim(); if t.is_empty() || t.chars().any(|c| c.is_control() || c.is_whitespace()) { return None }
    if scheme(t).is_some() { return handed_on(t).then(|| t.to_string()) }
    // (no scheme: a site's name has a dot and is no path of either system)
    if !t.contains('.') || t.starts_with(['/', '\\', '.', '~']) || t.contains('\\') || t.as_bytes().get(1) == Some(&b':') { return None }
    Some(format!("https://{t}"))
}
/// A text reads as an address (the clipboard's, for the dialog's first field).
pub fn looks_like(text: &str) -> bool { let t = text.trim(); t.len() < 2048 && address(t).is_some() }

/// The address inside a `.url` or `.webloc` file, as it may be handed on. None: not a link that is read.
pub fn read(path: &Path) -> Option<String> {
    let m = std::fs::metadata(path).ok()?; if m.len() > 1 << 20 { return None }
    let bytes = std::fs::read(path).ok()?;
    let raw = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("url")) { from_ini(&String::from_utf8_lossy(&bytes)) }
        else if bytes.starts_with(b"bplist") { from_binary(path) } else { from_plist(&String::from_utf8_lossy(&bytes)) }?;
    let raw = raw.trim().to_string(); handed_on(&raw).then_some(raw)
}
/// `URL=...` of an `[InternetShortcut]`: the key in any case, CRLF or LF, a byte order mark in front.
fn from_ini(text: &str) -> Option<String> {
    text.trim_start_matches('\u{feff}').lines().find_map(|l| { let (k, v) = l.trim().split_once('=')?; (k.trim().eq_ignore_ascii_case("URL") && !v.trim().is_empty()).then(|| v.trim().to_string()) })
}
/// `<key>URL</key>` and the `<string>` after it, of a property list written as text.
fn from_plist(text: &str) -> Option<String> {
    let rest = &text[text.find("<key>URL</key>")?..];
    let a = rest.find("<string>")? + 8; let b = rest[a..].find("</string>")? + a;
    let raw = rest[a..b].trim(); if raw.is_empty() { return None }
    Some(raw.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&"))
}
/// A property list written in binary (what a drag out of Safari makes): macOS's own tool turns it into text. On
/// Windows such a file is not read.
fn from_binary(path: &Path) -> Option<String> {
    #[cfg(target_os = "macos")] { let out = std::process::Command::new("/usr/bin/plutil").args(["-convert", "xml1", "-o", "-", "--"]).arg(path).output().ok()?; if !out.status.success() { return None } from_plist(&String::from_utf8_lossy(&out.stdout)) }
    #[cfg(not(target_os = "macos"))] { let _ = path; None }
}

/// The file's name for a link: what was typed, with the characters a name cannot carry made `-`; nothing typed: the
/// site (`www.` left out); no site either: `Link`.
pub fn file_name(name: &str, url: &str) -> String {
    let mut base: String = name.trim().chars().map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() { '-' } else { c }).collect();
    base = base.trim().trim_end_matches('.').to_string();
    if base.to_ascii_lowercase().ends_with(".url") { base.truncate(base.len() - 4); base = base.trim().to_string(); }
    if base.is_empty() { let u = url.trim(); let u = match u.find("://") { Some(k) => &u[k + 3..], None => u.split_once(':').map(|p| p.1).unwrap_or(u) }; base = u.split(['/', '?', '#']).next().unwrap_or("").trim_start_matches("www.").chars().map(|c| if matches!(c, ':' | '*' | '"' | '<' | '>' | '|' | '\\') { '-' } else { c }).collect(); }
    if base.is_empty() { base = "Link".into(); }
    format!("{base}.url")
}
/// Makes `<dir>/<name>.url` for `url`. Nothing is written over: a name that is taken gets ` 2`, ` 3`, ...
pub fn create(dir: &Path, name: &str, url: &str) -> Result<PathBuf, String> {
    let url = address(url).ok_or("This is not an address that can be opened: a site's name, or an address with its scheme (https://, mailto:, an application's own).")?;
    if !dir.is_dir() { return Err(format!("Folder does not exist: {}", dir.display())) }
    let file = file_name(name, &url); let stem = &file[..file.len() - 4];
    // (CRLF: what Windows writes, and what every reader takes)
    let body = format!("[InternetShortcut]\r\nURL={url}\r\n");
    for n in 1..10_000 {
        let p = dir.join(if n == 1 { file.clone() } else { format!("{stem} {n}.url") });
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&p) {
            Ok(mut f) => { use std::io::Write; return match f.write_all(body.as_bytes()) { Ok(()) => Ok(p), Err(e) => { drop(f); let _ = std::fs::remove_file(&p); Err(format!("The link could not be written: {e}.")) } } }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("The link could not be made: {e}.")),
        }
    }
    Err("No free name was found for the link.".into())
}

/// The addresses of a Links cell (1.4.2's value: ONE text, its links parted by a comma or a new line), each as it is
/// written and as it may be handed on (None: not an address that is opened).
/// A `ufb:` or `union:` link is kept as it is: the application opens it itself.
pub fn of_cell(text: &str) -> Vec<(String, Option<String>)> { text.split([',', '\n']).map(str::trim).filter(|s| !s.is_empty()).map(|s| (s.to_string(), if own(s) { Some(s.to_string()) } else { address(s) })).collect() }
/// A link of the application's own two schemes.
pub fn own(text: &str) -> bool { text.starts_with("ufb:") || text.starts_with("union:") }

#[cfg(test)]
mod tests {
    use super::*;
    struct Tmp(PathBuf);
    impl Tmp { fn new(n: &str) -> Tmp { let p = std::env::temp_dir().join(format!("ufb-weblink-{n}-{}", std::process::id())); let _ = std::fs::remove_dir_all(&p); std::fs::create_dir_all(&p).unwrap(); Tmp(p) } }
    impl Drop for Tmp { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }

    #[test]
    fn an_address_as_it_is_handed_on() {
        assert_eq!(address(" https://example.com/a?b=1 ").as_deref(), Some("https://example.com/a?b=1"));
        assert_eq!(address("example.com/x").as_deref(), Some("https://example.com/x"));
        assert_eq!(address("example.com:8080/x").as_deref(), Some("https://example.com:8080/x"));
        assert_eq!(address("mailto:a@example.com").as_deref(), Some("mailto:a@example.com"));
        // a studio's own schemes and those of the services it uses are handed on as they are
        for yes in ["slack://channel?team=T1&id=C2", "frameio://project/123", "tel:+15551234", "smb://server/share", "ms-word:ofe|u|https://example.com/a.docx", "studio-tool:open?shot=010"] { assert_eq!(address(yes).as_deref(), Some(yes), "{yes}"); }
        for no in ["", "file:///etc/hosts", "FILE://server/x.exe", "javascript:alert(1)", "data:text/html,x", "ms-msdt:/id", "search-ms:query=x", "word", "two words.com", "c:\\tools\\x.exe", "C:/tools/x.exe", "\\\\server\\share\\x.exe", "/usr/bin/x.sh", "./x.command", "~/x.app"] { assert_eq!(address(no), None, "{no}"); }
        assert!(looks_like("www.example.com")); assert!(!looks_like("a note about nothing"));
    }
    #[test]
    fn a_link_is_named_made_and_read() {
        assert_eq!(file_name("Client / Brief: v2", "https://x"), "Client - Brief- v2.url");
        assert_eq!(file_name("", "https://www.example.com/a/b?c"), "example.com.url"); assert_eq!(file_name("site.URL", "https://x"), "site.url"); assert_eq!(file_name(" . ", ""), "Link.url");
        let t = Tmp::new("make");
        let a = create(&t.0, "", "example.com/brief").unwrap(); assert_eq!(a.file_name().unwrap(), "example.com.url");
        assert_eq!(std::fs::read(&a).unwrap(), b"[InternetShortcut]\r\nURL=https://example.com/brief\r\n"); assert_eq!(read(&a).as_deref(), Some("https://example.com/brief"));
        let b = create(&t.0, "", "https://example.com/other").unwrap(); assert_eq!(b.file_name().unwrap(), "example.com 2.url", "nothing is written over"); assert_eq!(read(&a).as_deref(), Some("https://example.com/brief"));
        assert!(create(&t.0, "x", "file:///etc/hosts").is_err()); assert_eq!(create(&t.0, "", "slack://channel?id=C2").unwrap().file_name().unwrap(), "channel.url"); assert!(create(&t.0.join("none"), "x", "example.com").is_err());
    }
    #[test]
    fn link_files_of_both_systems_are_read_and_nothing_but_the_web_is_handed_on() {
        let t = Tmp::new("read"); let w = |n: &str, body: &str| { let p = t.0.join(n); std::fs::write(&p, body).unwrap(); p };
        assert_eq!(read(&w("a.URL", "\u{feff}[InternetShortcut]\r\nIconIndex=0\r\nurl = https://example.com/a\r\n")).as_deref(), Some("https://example.com/a"));
        assert_eq!(read(&w("b.webloc", "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>URL</key><string>https://example.com/?a=1&amp;b=2</string></dict></plist>")).as_deref(), Some("https://example.com/?a=1&b=2"));
        assert_eq!(read(&w("c.url", "[InternetShortcut]\r\nURL=file:///Applications/Calculator.app\r\n")), None, "a file's address is not opened");
        assert_eq!(read(&w("d.url", "[InternetShortcut]\r\nURL=C:\\Windows\\System32\\calc.exe\r\n")), None); assert_eq!(read(&w("e.url", "nothing")), None); assert_eq!(read(&w("g.url", "[InternetShortcut]\r\nURL=frameio://project/123\r\n")).as_deref(), Some("frameio://project/123")); assert_eq!(read(&w("f.txt", "URL=https://example.com")), None);
        assert!(is_link(Path::new("/x/Site.URL"))); assert!(is_link(Path::new("/x/site.webloc"))); assert!(!is_link(Path::new("/x/site.html")));
    }
    #[test]
    fn the_links_of_a_cell() {
        assert_eq!(of_cell("https://a.example/x, b.example\nnot a link ,"), [("https://a.example/x".to_string(), Some("https://a.example/x".to_string())), ("b.example".to_string(), Some("https://b.example".to_string())), ("not a link".to_string(), None)]);
    }
}
