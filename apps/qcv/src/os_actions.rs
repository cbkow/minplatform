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

/// Hand a URL to whatever the OS has registered for its scheme (QCView `QDesktopServices::openUrl`); Err when nothing took it.
/// Not `open` above: Windows' arm there rewrites the argument as a path.
pub fn open_url(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")] { let st = Command::new("open").arg(url).stderr(std::process::Stdio::null()).status()?; if st.success() { Ok(()) } else { Err(std::io::Error::other("no application opens this link")) } }
    #[cfg(windows)] {
        use windows::{core::{w, HSTRING, PCWSTR}, Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL}};
        let u = HSTRING::from(url);
        let r = unsafe { ShellExecuteW(None, w!("open"), PCWSTR(u.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
        if r.0 as isize > 32 { Ok(()) } else { Err(std::io::Error::other("no application opens this link")) }
    }
    #[cfg(all(unix, not(target_os = "macos")))] { Command::new("xdg-open").arg(url).spawn().map(|_| ()) }
}

/// Is minNotes installed? (Canon row 256.) OURS: the OS is asked who opens `minnotes:` — QCView probed two install paths (and
/// one machine-wide registry key), which misses a Store or per-user install on Windows and a copy outside /Applications on
/// macOS. Asked each time the export menu opens, so installing minNotes while QCView runs is seen.
pub fn minnotes_available() -> bool {
    #[cfg(target_os = "macos")] {
        use objc2_foundation::{NSString, NSURL};
        #[allow(unused_unsafe)]
        unsafe {
            let Some(url) = NSURL::URLWithString(&NSString::from_str("minnotes:///")) else { return false };
            objc2_app_kit::NSWorkspace::sharedWorkspace().URLForApplicationToOpenURL(&url).is_some()
        }
    }
    #[cfg(windows)] {
        use windows::{core::w, Win32::UI::Shell::{AssocQueryStringW, ASSOCF_IS_PROTOCOL, ASSOCSTR_COMMAND, ASSOCSTR_FRIENDLYAPPNAME}};
        // a classic handler answers COMMAND; a packaged (Store) one has no command line but a friendly name
        [ASSOCSTR_COMMAND, ASSOCSTR_FRIENDLYAPPNAME].into_iter().any(|what| { let mut n = 0u32; let _ = unsafe { AssocQueryStringW(ASSOCF_IS_PROTOCOL, what, w!("minnotes"), None, None, &mut n) }; n > 1 })
    }
    #[cfg(all(unix, not(target_os = "macos")))] { false }
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

/// QCView `mediaext::isShortcut`: a Windows shortcut (`.lnk`; never one elsewhere).
pub fn is_shortcut(path: &str) -> bool { cfg!(windows) && path.to_ascii_lowercase().ends_with(".lnk") && !path.contains("://") }

/// QCView `mediaext::resolveShortcut`: the shortcut's target, or None when it is unreadable or its target is gone; a
/// non-shortcut comes back as itself.
pub fn resolve_shortcut(path: &str) -> Option<String> {
    if !is_shortcut(path) { return Some(path.to_string()) }
    #[cfg(windows)] {
        use windows::{core::{Interface, PCWSTR}, Win32::{System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, IPersistFile, STGM_READ}, UI::Shell::{IShellLinkW, ShellLink}}};
        let target = unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);   // S_FALSE when the thread already has COM: fine
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).ok()?;
            let pf: IPersistFile = link.cast().ok()?;
            let w: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
            pf.Load(PCWSTR(w.as_ptr()), STGM_READ).ok()?;
            let mut buf = vec![0u16; 32768];
            link.GetPath(&mut buf, std::ptr::null_mut(), 0).ok()?;
            String::from_utf16_lossy(&buf[..buf.iter().position(|c| *c == 0).unwrap_or(buf.len())])
        };
        if target.is_empty() || !std::path::Path::new(&target).exists() { return None }
        return Some(target)
    }
    #[allow(unreachable_code)] None
}

#[cfg(all(test, windows))]
mod tests {
    /// A real shortcut (made by WScript.Shell) resolves to its target; once the target is gone it reads as dead (None).
    #[test]
    fn a_shortcut_resolves_and_a_dead_one_does_not() {
        let d = std::env::temp_dir().join(format!("qcv-lnk-{}", std::process::id())); std::fs::create_dir_all(&d).unwrap();
        let (target, lnk) = (d.join("clip.mov"), d.join("clip.lnk"));
        std::fs::write(&target, b"x").unwrap();
        let ps = format!("$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}'); $s.TargetPath='{}'; $s.Save()", lnk.display(), target.display());
        assert!(std::process::Command::new("powershell").args(["-NoProfile", "-Command", &ps]).status().unwrap().success());
        let l = lnk.to_string_lossy().into_owned();
        assert!(super::is_shortcut(&l));
        assert_eq!(super::resolve_shortcut(&l).map(|p| p.to_lowercase()), Some(target.to_string_lossy().to_lowercase()));
        std::fs::remove_file(&target).unwrap();
        assert_eq!(super::resolve_shortcut(&l), None, "a dead shortcut");
        assert_eq!(super::resolve_shortcut("C:\\\\x\\\\plain.mov").as_deref(), Some("C:\\\\x\\\\plain.mov"));
        let _ = std::fs::remove_dir_all(d);
    }
}
/// The third-party notices shipped with this build (canon row 252), as a path the browser can open: the bundle's
/// `Resources` on macOS; beside the exe on Windows — copied to `%LOCALAPPDATA%\QCView` first, because a packaged app's
/// folder (WindowsApps) is not a place another program is sure to read from. A development run falls back to the file the
/// packaging scripts last generated in this workspace; None when there is none.
pub fn notices_file() -> Option<String> {
    const NAME: &str = "THIRD-PARTY-NOTICES.html";
    let exe = std::env::current_exe().ok()?; let dir = exe.parent()?;
    let shipped = if cfg!(target_os = "macos") { dir.join("../Resources").join(NAME) } else { dir.join(NAME) };
    let found = if shipped.is_file() { shipped } else {
        let ws = exe.ancestors().nth(3)?;
        [ws.join("target/dist/qcview.app/Contents/Resources").join(NAME), ws.join("packaging/notices/generated").join(if cfg!(windows) { "THIRD-PARTY-NOTICES-windows.html" } else { "THIRD-PARTY-NOTICES-macos.html" })].into_iter().find(|p| p.is_file())?
    };
    #[cfg(windows)] if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let out = std::path::PathBuf::from(local).join("QCView");
        if std::fs::create_dir_all(&out).is_ok() && std::fs::copy(&found, out.join(NAME)).is_ok() { return Some(out.join(NAME).to_string_lossy().into_owned()) }
    }
    Some(std::fs::canonicalize(&found).unwrap_or(found).to_string_lossy().into_owned())
}


#[cfg(test)]
#[test]
fn the_minnotes_probe_answers() { println!("minnotes handler registered: {}", minnotes_available()); }
