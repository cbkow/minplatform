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
