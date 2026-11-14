//! What the system opens for the app (canon row 320): a link's address, a file in its own application, a file shown
//! in its folder. Nothing is opened in a scripted run (`MN_NO_DIALOGS`).
use std::process::Command;

fn run(mut c: Command) { if std::env::var_os("MN_NO_DIALOGS").is_some() { return } if let Err(e) = c.spawn() { eprintln!("open: {e}"); } }
/// A link's address, when it is one the app opens (`mn_edit::links::openable`).
pub fn url(u: &str) -> bool {
    if !mn_edit::links::openable(u) { return false }
    #[cfg(target_os = "macos")] { let mut c = Command::new("open"); c.arg(u); run(c); }
    #[cfg(target_os = "windows")] { let mut c = Command::new("rundll32"); c.arg("url.dll,FileProtocolHandler").arg(u); run(c); }
    #[cfg(all(unix, not(target_os = "macos")))] { let mut c = Command::new("xdg-open"); c.arg(u); run(c); }
    true
}
/// A file or a folder in the application the system has for it.
pub fn file(p: &std::path::Path) {
    #[cfg(target_os = "macos")] { let mut c = Command::new("open"); c.arg(p); run(c); }
    #[cfg(target_os = "windows")] { let mut c = Command::new("explorer"); c.arg(win(p)); run(c); }
    #[cfg(all(unix, not(target_os = "macos")))] { let mut c = Command::new("xdg-open"); c.arg(p); run(c); }
}
/// A path as Explorer takes it: backslashes only (a path put together with `/` sent it to Documents), no `\\?\` prefix.
#[cfg(target_os = "windows")]
fn win(p: &std::path::Path) -> String { let s = p.to_string_lossy().replace('/', "\\"); match s.strip_prefix("\\\\?\\UNC\\") { Some(r) => format!("\\\\{r}"), None => s.strip_prefix("\\\\?\\").map(|r| r.to_string()).unwrap_or(s) } }
/// A file shown in its folder.
pub fn reveal(p: &std::path::Path) {
    #[cfg(target_os = "macos")] { let mut c = Command::new("open"); c.arg("-R").arg(p); run(c); }
    // (Explorer reads its own command line: the path alone is quoted - `/select,"C:\a b\c"` - never the whole argument,
    // which is what an ordinary argument with a space in it becomes and which sends Explorer to Documents)
    #[cfg(target_os = "windows")] { use std::os::windows::process::CommandExt; let mut c = Command::new("explorer"); c.raw_arg(format!("/select,\"{}\"", win(p))); run(c); }
    #[cfg(all(unix, not(target_os = "macos")))] { if let Some(d) = p.parent() { let mut c = Command::new("xdg-open"); c.arg(d); run(c); } }
}
