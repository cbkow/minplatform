//! Same-volume by DEVICE IDENTITY (row 119 — ufb's leading-component heuristic made every POSIX pair "same volume", so a
//! macOS drag between two volumes defaulted to Move), the drop guards (ufb's drop-onto-self plus the descendant guard the
//! survey found missing), and the path another process must be handed (minNotes' / ufb's long-path and MSIX rule).
use std::path::{Path, PathBuf};

/// `X:` or `\\server\share` for a Windows-syntax path (ufb's `drive_or_unc`; pure, so it is testable everywhere). A UNC
/// root is the text up to the fourth separator; a path with fewer is returned whole.
pub fn drive_or_unc(p: &str) -> Option<String> {
    let t = p.trim_start();
    if t.starts_with("\\\\") || t.starts_with("//") {
        let mut n = 0;
        for (i, b) in t.bytes().enumerate() { if b == b'\\' || b == b'/' { n += 1; if n == 4 { return Some(t[..i].to_string()) } } }
        return Some(t.to_string());
    }
    if crate::has_drive(t) { Some(t[..2].to_string()) } else { None }
}

/// The device that holds `path` — the nearest EXISTING ancestor's `st_dev` on POSIX, volume serial number on Windows.
pub fn device_id(path: &Path) -> Option<u64> {
    let mut p = Some(path);
    while let Some(c) = p {
        if let Some(id) = device_id_exact(c) { return Some(id) }
        p = c.parent();
    }
    None
}

#[cfg(unix)]
fn device_id_exact(p: &Path) -> Option<u64> { use std::os::unix::fs::MetadataExt; std::fs::metadata(p).ok().map(|m| m.dev()) }

#[cfg(windows)]
fn device_id_exact(p: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::*;
    let wide: Vec<u16> = p.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    unsafe {
        let h = CreateFileW(wide.as_ptr(), 0, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, std::ptr::null(), OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS, std::ptr::null_mut());
        if h == INVALID_HANDLE_VALUE { return None }
        let mut info: BY_HANDLE_FILE_INFORMATION = std::mem::zeroed();
        let ok = GetFileInformationByHandle(h, &mut info) != 0;
        CloseHandle(h);
        ok.then_some(info.dwVolumeSerialNumber as u64)
    }
}

/// Do `a` and `b` live on the same volume? Device identity when both (or their nearest existing ancestors) can be stat'ed;
/// on Windows a pair that cannot falls back to comparing drive letter / UNC share (case-insensitively); elsewhere → false.
/// Empty paths are never the same volume. This is what the Copy-vs-Move default of an internal drag reads (row 53).
pub fn same_volume(a: &Path, b: &Path) -> bool {
    if a.as_os_str().is_empty() || b.as_os_str().is_empty() { return false }
    match (device_id(a), device_id(b)) {
        (Some(x), Some(y)) => x == y,
        _ => if cfg!(windows) { match (drive_or_unc(&a.to_string_lossy()), drive_or_unc(&b.to_string_lossy())) { (Some(x), Some(y)) => x.eq_ignore_ascii_case(&y), _ => false } } else { false },
    }
}

fn norm_for_eq(p: &Path) -> String { p.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_lowercase() }

/// ufb's drop-onto-self guard: a file dropped back where it came from (`target == src`, by name or after canonicalising
/// the target's parent) — `fs_extra` would open the destination with `File::create` BEFORE reading the source and
/// truncate it to zero bytes.
pub fn is_drop_onto_self(src: &Path, target: &Path) -> bool {
    if src == target { return true }
    let canon_target = std::fs::canonicalize(target.parent().unwrap_or(target)).map(|p| target.file_name().map(|f| p.join(f)).unwrap_or(p));
    matches!((std::fs::canonicalize(src), canon_target), (Ok(s), Ok(t)) if norm_for_eq(&s) == norm_for_eq(&t))
}

/// The descendant guard ufb lacked: dropping a FOLDER into itself or any of its subfolders (`/a` → `/a/b/`) would recurse
/// forever or truncate. True when `target_dir` is `src` or lies under it, by canonical path when both exist and lexically
/// otherwise.
pub fn is_drop_into_itself(src: &Path, target_dir: &Path) -> bool {
    let (s, t) = (std::fs::canonicalize(src).unwrap_or(src.to_path_buf()), std::fs::canonicalize(target_dir).unwrap_or(target_dir.to_path_buf()));
    let (s, t) = (norm_for_eq(&s), norm_for_eq(&t));
    t == s || t.starts_with(&format!("{s}/"))
}

/// Both guards: `Some(reason)` when the drop must be skipped.
pub fn drop_guard(src: &Path, target: &Path) -> Option<&'static str> {
    if is_drop_onto_self(src, target) { return Some("drop onto self") }
    if src.is_dir() && is_drop_into_itself(src, target.parent().unwrap_or(target)) { return Some("folder dropped into itself") }
    None
}

/// The path ANOTHER process must be handed for a file we can see (minNotes `FileLinks.cpp`, ufb `external_path`): inside
/// an MSIX package, files under AppData live in the package's private LocalCache; opening the file and asking Windows
/// for its final path resolves that, and the `\\?\` / `\\?\UNC\` prefixes are stripped back to the forms other apps
/// accept. Unchanged off Windows or when the path cannot be opened.
pub fn external_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::Storage::FileSystem::*;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        unsafe {
            let h = CreateFileW(wide.as_ptr(), 0, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, std::ptr::null(), OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS, std::ptr::null_mut());
            if h == INVALID_HANDLE_VALUE { return path.to_path_buf() }
            let mut buf = vec![0u16; 4096];
            let n = GetFinalPathNameByHandleW(h, buf.as_mut_ptr(), buf.len() as u32, FILE_NAME_NORMALIZED) as usize;
            CloseHandle(h);
            if n == 0 || n >= buf.len() { return path.to_path_buf() }
            let s = std::ffi::OsString::from_wide(&buf[..n]).to_string_lossy().into_owned();
            let s = if let Some(r) = s.strip_prefix(r"\\?\UNC\") { format!(r"\\{r}") } else { s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(s) };
            PathBuf::from(s)
        }
    }
    #[cfg(not(windows))] { path.to_path_buf() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn drive_or_unc_rules() {
        assert_eq!(drive_or_unc("C:\\x\\y").as_deref(), Some("C:"));
        assert_eq!(drive_or_unc("  d:/x").as_deref(), Some("d:"));
        assert_eq!(drive_or_unc("\\\\srv\\share\\a\\b").as_deref(), Some("\\\\srv\\share"));
        assert_eq!(drive_or_unc("//srv/share").as_deref(), Some("//srv/share"));
        assert_eq!(drive_or_unc("/Volumes/x"), None);
        assert_eq!(drive_or_unc("rel\\x"), None);
    }
    #[test] fn same_volume_is_device_identity() {
        let t = std::env::temp_dir();
        let a = t.join(format!("min-paths-vol-a-{}", std::process::id())); std::fs::write(&a, b"").unwrap();
        assert!(same_volume(&a, &t));
        assert!(same_volume(&t.join("does/not/exist/yet"), &t));   // nearest existing ancestor
        assert!(!same_volume(Path::new(""), &t));
        std::fs::remove_file(&a).unwrap();
    }
    #[test] fn drop_guards() {
        let t = std::env::temp_dir().join(format!("min-paths-drop-{}", std::process::id()));
        std::fs::create_dir_all(t.join("a/b")).unwrap(); std::fs::write(t.join("a/f.txt"), b"x").unwrap();
        assert!(is_drop_onto_self(&t.join("a/f.txt"), &t.join("a/f.txt")));
        assert!(is_drop_onto_self(&t.join("a/f.txt"), &t.join("a/b/../f.txt")));
        assert!(!is_drop_onto_self(&t.join("a/f.txt"), &t.join("a/b/f.txt")));
        assert!(is_drop_into_itself(&t.join("a"), &t.join("a")));
        assert!(is_drop_into_itself(&t.join("a"), &t.join("a/b")));
        assert!(!is_drop_into_itself(&t.join("a"), &t.join("ab")));
        assert!(!is_drop_into_itself(&t.join("a"), &t));
        assert_eq!(drop_guard(&t.join("a"), &t.join("a/b/a")), Some("folder dropped into itself"));
        assert_eq!(drop_guard(&t.join("a/f.txt"), &t.join("a/b/f.txt")), None);
        std::fs::remove_dir_all(&t).unwrap();
    }
    #[test] fn external_path_is_identity_off_msix() { let t = std::env::temp_dir(); assert_eq!(external_path(&t), std::fs::canonicalize(&t).map(|_| t.clone()).unwrap_or(t)); }
}
