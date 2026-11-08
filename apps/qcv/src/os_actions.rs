//! QCView's `openExternalPath` / `revealInFileManager` / `copyTextToClipboard` (window_manager.cpp), for the Inspector's path
//! actions and Linked Projects (canon row 193).
use std::process::Command;

/// Open with the OS's default app (`.aep` → After Effects, `.prproj` → Premiere) — QCView `QDesktopServices::openUrl`.
pub fn open(path: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")] { Command::new("open").arg(path).spawn().map(|_| ()) }
    #[cfg(windows)] {
        use std::os::windows::process::CommandExt;
        // `explorer "path"` hands the file to its association (ShellExecute's open verb); the path quoted raw like Reveal's
        Command::new("explorer").raw_arg(format!("\"{}\"", min_paths::map::to_native(path, min_paths::Os::Win))).spawn().map(|_| ())
    }
    #[cfg(all(unix, not(target_os = "macos")))] { Command::new("xdg-open").arg(path).spawn().map(|_| ()) }
}

/// Select the file in Finder / Explorer (Linux: open its folder) — QCView `revealInFileManager`, its quoting carried: Explorer
/// parses `/select,"C:\path"` only when the comma form stays outside the quotes, so the command line is built raw.
pub fn reveal(path: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")] { Command::new("open").args(["-R", path]).spawn().map(|_| ()) }
    #[cfg(windows)] {
        use std::os::windows::process::CommandExt;
        Command::new("explorer").raw_arg(format!("/select,\"{}\"", min_paths::map::to_native(path, min_paths::Os::Win))).spawn().map(|_| ())
    }
    #[cfg(all(unix, not(target_os = "macos")))] { let dir = std::path::Path::new(path).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(); Command::new("xdg-open").arg(dir).spawn().map(|_| ()) }
}

/// The path as people paste it on this OS (QCView `toNativeSeparators`).
pub fn native(path: &str) -> String { min_paths::map::to_native(path, min_paths::Os::current()) }

/// Copy text (QCView `copyTextToClipboard`).
pub fn copy(text: &str) -> Result<(), String> { min_paths::clipboard::copy_text(text) }
