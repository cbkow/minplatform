//! "Open": the file is handed to the system, which opens it in its default application - as a double click in
//! Finder or Explorer does (canon row 473, point 2).
use std::path::Path;

pub fn open(path: &Path) {
    #[cfg(target_os = "macos")] {
        use objc2_foundation::{NSString, NSURL};
        let _ = objc2::exception::catch(std::panic::AssertUnwindSafe(|| { objc2_app_kit::NSWorkspace::sharedWorkspace().openURL(&NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))); }));
    }
    #[cfg(windows)] {
        use windows::core::{w, HSTRING}; use windows::Win32::UI::Shell::ShellExecuteW; use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        // (the shell takes an absolute path with backslashes, never the `\\?\` form - S18)
        let s = path.to_string_lossy().replace('/', "\\"); let s = s.strip_prefix("\\\\?\\").unwrap_or(&s).to_string();
        unsafe { ShellExecuteW(None, w!("open"), &HSTRING::from(s), None, None, SW_SHOWNORMAL); }
    }
    #[cfg(not(any(target_os = "macos", windows)))] { let _ = path; }
}

/// The file is handed to ONE application (macOS: its bundle).
pub fn open_with(path: &Path, app: &Path) {
    #[cfg(target_os = "macos")] {
        use objc2_foundation::{NSArray, NSString, NSURL};
        let url = |p: &Path| NSURL::fileURLWithPath(&NSString::from_str(&p.to_string_lossy()));
        let _ = objc2::exception::catch(std::panic::AssertUnwindSafe(|| objc2_app_kit::NSWorkspace::sharedWorkspace().openURLs_withApplicationAtURL_configuration_completionHandler(&NSArray::from_retained_slice(&[url(path)]), &url(app), &objc2_app_kit::NSWorkspaceOpenConfiguration::configuration(), None)));
    }
    #[cfg(not(target_os = "macos"))] { let _ = app; open(path); }
}

/// Show these in the system's own file browser, selected (Finder; Explorer shows the first).
pub fn reveal(paths: &[std::path::PathBuf]) {
    #[cfg(target_os = "macos")] {
        use objc2_foundation::{NSArray, NSString, NSURL};
        let urls: Vec<_> = paths.iter().map(|p| NSURL::fileURLWithPath(&NSString::from_str(&p.to_string_lossy()))).collect();
        let _ = objc2::exception::catch(std::panic::AssertUnwindSafe(|| objc2_app_kit::NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&urls))));
    }
    #[cfg(windows)] { if let Some(p) = paths.first() { let s = p.to_string_lossy().replace('/', "\\"); let s = s.strip_prefix("\\\\?\\").unwrap_or(&s).to_string(); let _ = std::process::Command::new("explorer.exe").arg(format!("/select,{s}")).spawn(); } }
    #[cfg(not(any(target_os = "macos", windows)))] { let _ = paths; }
}
/// The system's "Open with" chooser for a file (Windows).
pub fn open_as(path: &Path) {
    #[cfg(windows)] {
        use windows::core::{w, HSTRING}; use windows::Win32::UI::Shell::ShellExecuteW; use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let s = path.to_string_lossy().replace('/', "\\"); let s = s.strip_prefix("\\\\?\\").unwrap_or(&s).to_string();
        unsafe { ShellExecuteW(None, w!("openas"), &HSTRING::from(s), None, None, SW_SHOWNORMAL); }
    }
    #[cfg(not(windows))] { let _ = path; }
}
