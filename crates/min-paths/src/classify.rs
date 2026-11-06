//! Source classifiers (row 122): minNotes' `isNetworkSource` / `isVolatileSource` rules (`MediaStore.cpp`), with `statfs`
//! in place of QStorageInfo, plus MinRender's `shareHostForPath` (`net_utils.cpp`). The copy policy that reads them
//! (minNotes, 2026-08-19): shares are the shared source of truth and stay referenced in place; everything else — and
//! anything the probe cannot classify — is copied. Unknown → NOT a share, the safe side.
use crate::{clean_path, Os};
use std::path::{Path, PathBuf};

/// Filesystem type substrings that mean "a network share" (minNotes' list; matched against the lowercased type name).
pub const NETWORK_FS_TYPES: [&str; 7] = ["smb", "afp", "nfs", "cifs", "dav", "sshfs", "ftp"];

/// `QFileInfo::absoluteFilePath` + `QDir::cleanPath`: relative → under the cwd; on Windows a rooted drive-less path
/// (`/x/y`) gets the cwd's drive, a UNC path stays.
fn absolute(path: &str) -> String {
    let os = Os::current();
    let p = Path::new(path);
    let cwd = || std::env::current_dir().map(|c| c.to_string_lossy().into_owned()).unwrap_or_default();
    let abs = if p.is_absolute() { path.to_string() }
        else if os.is_windows() && (path.starts_with('/') || path.starts_with('\\')) && !path.starts_with("//") && !path.starts_with("\\\\") { let c = cwd(); if crate::has_drive(&c) { format!("{}{}", &c[..2], path) } else { path.to_string() } }
        else { let c = cwd(); if c.is_empty() { path.to_string() } else { format!("{c}/{path}") } };
    clean_path(&abs, os)
}

/// The filesystem type name of the mount holding `path` (its nearest existing ancestor), lowercased: `apfs`, `smbfs`,
/// `nfs`, `ntfs`… `None` when nothing on the way up can be stat'ed. macOS: `statfs().f_fstypename`; Linux:
/// `/proc/self/mounts` by longest mount-point prefix; Windows: `None` (drive type is used instead).
pub fn fs_type_name(path: &Path) -> Option<String> {
    let mut p = Some(path);
    while let Some(c) = p {
        if c.exists() { return fs_type_exact(c) }
        p = c.parent();
    }
    None
}
#[cfg(target_os = "macos")]
fn fs_type_exact(p: &Path) -> Option<String> {
    let c = std::ffi::CString::new(p.as_os_str().as_encoded_bytes()).ok()?;
    unsafe {
        let mut sf: libc::statfs = std::mem::zeroed();
        if libc::statfs(c.as_ptr(), &mut sf) != 0 { return None }
        Some(std::ffi::CStr::from_ptr(sf.f_fstypename.as_ptr()).to_string_lossy().to_lowercase())
    }
}
#[cfg(target_os = "linux")]
fn fs_type_exact(p: &Path) -> Option<String> {
    let canon = std::fs::canonicalize(p).ok()?.to_string_lossy().into_owned();
    let mounts = std::fs::read_to_string("/proc/self/mounts").ok()?;
    let mut best: Option<(usize, String)> = None;
    for line in mounts.lines() {
        let mut it = line.split_whitespace(); let (_src, mp, ty) = (it.next()?, it.next()?, it.next()?);
        let mp = mp.replace("\\040", " ");
        if canon == mp || canon.starts_with(&format!("{}/", mp.trim_end_matches('/'))) || mp == "/" {
            if best.as_ref().is_none_or(|(l, _)| mp.len() >= *l) { best = Some((mp.len(), ty.to_lowercase())); }
        }
    }
    best.map(|(_, t)| t)
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn fs_type_exact(_p: &Path) -> Option<String> { None }

/// True when `path` lives on a network share: Windows — a UNC path, or a drive letter whose `GetDriveTypeW` is
/// `DRIVE_REMOTE`; POSIX — the mount's filesystem type contains one of [`NETWORK_FS_TYPES`]. Cannot tell → false.
pub fn is_network_source(path: &str) -> bool {
    let p = absolute(path);
    if cfg!(windows) {
        if p.starts_with("//") || p.starts_with("\\\\") { return true }
        #[cfg(windows)]
        if crate::has_drive(&p) {
            use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
            const DRIVE_REMOTE: u32 = 4;   // winbase.h; windows-sys 0.60 keeps the constant elsewhere
            let root: Vec<u16> = format!("{}\\", &p[..2]).encode_utf16().chain(std::iter::once(0)).collect();
            return unsafe { GetDriveTypeW(root.as_ptr()) } == DRIVE_REMOTE;
        }
        false
    } else {
        fs_type_name(Path::new(&p)).is_some_and(|t| NETWORK_FS_TYPES.iter().any(|n| t.contains(n)))
    }
}

/// The roots minNotes treats as volatile on this machine: the temp dirs, and on macOS `~/Library` (capture apps' media
/// stores, Mail / Messages attachment caches) and `/var/folders`; on Windows `%TEMP%`, `%TMP%`, `%LOCALAPPDATA%`,
/// `%APPDATA%`. Callers add their own scratch dir through `extra_roots`.
pub fn volatile_roots() -> Vec<PathBuf> {
    let os = Os::current();
    let mut roots: Vec<String> = vec![clean_path(&std::env::temp_dir().to_string_lossy(), os), "/tmp".into(), "/var/tmp".into()];
    if cfg!(target_os = "macos") {
        if let Some(h) = std::env::var_os("HOME") { roots.push(format!("{}/Library", h.to_string_lossy().trim_end_matches('/'))); }
        roots.extend(["/private/var/folders", "/var/folders", "/private/tmp", "/private/var/tmp"].map(String::from));
    }
    if cfg!(windows) {
        for v in ["TEMP", "TMP", "LOCALAPPDATA", "APPDATA"] { if let Some(x) = std::env::var_os(v) { let x = x.to_string_lossy(); if !x.is_empty() { roots.push(clean_path(&x, os)); } } }
    }
    roots.into_iter().filter(|r| !r.is_empty()).map(PathBuf::from).collect()
}

/// True when `path` lives somewhere that routinely deletes its own contents (minNotes: an image referenced in place from
/// one of these rots within days — a real export emitted a dead link, 2026-08-13). Both the absolute and the canonical
/// path are tested, because `/tmp` is a symlink to `/private/tmp` on macOS and a capture app hands `file:///tmp/…`.
/// Case-insensitive on Windows.
pub fn is_volatile_source(path: &str, extra_roots: &[PathBuf]) -> bool {
    let os = Os::current();
    let abs = absolute(path);
    let canon = std::fs::canonicalize(&abs).map(|c| clean_path(&c.to_string_lossy(), os)).unwrap_or_default();
    let fold = |s: &str| if os.is_windows() { s.to_lowercase() } else { s.to_string() };
    let mut roots = volatile_roots(); roots.extend(extra_roots.iter().cloned());
    for root in roots {
        let r = fold(&clean_path(&root.to_string_lossy(), os));
        if r.is_empty() { continue }
        for p in [&abs, &canon] {
            if p.is_empty() { continue }
            let p = fold(p);
            if p == r || p.starts_with(&format!("{r}/")) { return true }
        }
    }
    false
}

/// MinRender's `shareHostForPath`: the host a path is served from — UNC `\\host\share` / `//user@host/share` → `host`;
/// Windows mapped drive → via `WNetGetConnectionW`; macOS mount → `statfs().f_mntfromname` (`//user@host/share` for SMB,
/// `host:/export` for NFS). `None` when the path is local or unknown.
pub fn share_host_for_path(path: &str) -> Option<String> {
    if path.is_empty() { return None }
    let b = path.as_bytes();
    if b.len() > 2 && ((b[0] == b'\\' && b[1] == b'\\') || (b[0] == b'/' && b[1] == b'/')) {
        let rest = &path[2..];
        let host = rest.split(['\\', '/']).next().unwrap_or("");
        let host = host.rsplit('@').next().unwrap_or(host);
        return if host.is_empty() { None } else { Some(host.to_string()) };
    }
    #[cfg(windows)]
    if crate::has_drive(path) {
        use windows_sys::Win32::NetworkManagement::WNet::WNetGetConnectionW;
        let local: Vec<u16> = format!("{}:", &path[..1]).encode_utf16().chain(std::iter::once(0)).collect();
        let mut remote = vec![0u16; 1024]; let mut len = remote.len() as u32;
        if unsafe { WNetGetConnectionW(local.as_ptr(), remote.as_mut_ptr(), &mut len) } == 0 {
            let n = remote.iter().position(|&c| c == 0).unwrap_or(remote.len());
            return share_host_for_path(&String::from_utf16_lossy(&remote[..n]));
        }
        return None;
    }
    #[cfg(target_os = "macos")]
    {
        let c = std::ffi::CString::new(path).ok()?;
        unsafe {
            let mut sf: libc::statfs = std::mem::zeroed();
            if libc::statfs(c.as_ptr(), &mut sf) != 0 { return None }
            let from = std::ffi::CStr::from_ptr(sf.f_mntfromname.as_ptr()).to_string_lossy().into_owned();
            if from.starts_with("//") { return share_host_for_path(&from) }
            if let Some(colon) = from.find(":/") && from.find('/') > Some(colon) { return Some(from[..colon].to_string()) }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn unc_hosts() {
        assert_eq!(share_host_for_path("\\\\nas\\share\\x").as_deref(), Some("nas"));
        assert_eq!(share_host_for_path("//user@nas/share").as_deref(), Some("nas"));
        assert_eq!(share_host_for_path("//a@b@host/s").as_deref(), Some("host"));
        assert_eq!(share_host_for_path(""), None);
    }
    #[test] fn temp_is_volatile_and_home_is_not() {
        let t = std::env::temp_dir().join("x.png");
        assert!(is_volatile_source(&t.to_string_lossy(), &[]));
        assert!(is_volatile_source("/tmp/x.png", &[]) || cfg!(windows));
        let scratch = std::env::temp_dir().join("mp-scratch");   // an absolute root on either OS
        let (inside, beside) = (scratch.join("a/b.png"), std::env::temp_dir().join("mp-scratch-not/a.png"));
        assert!(is_volatile_source(&inside.to_string_lossy(), &[scratch.clone()]));
        assert!(is_volatile_source(&beside.to_string_lossy(), &[]) );   // under temp anyway
        let home_root = PathBuf::from(std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).unwrap_or_default()).join("mp-vol-x");
        assert!(is_volatile_source(&home_root.join("f.png").to_string_lossy(), &[home_root.clone()]));
        assert!(!is_volatile_source(&format!("{}-not/f.png", home_root.to_string_lossy()), &[home_root]));   // the boundary
        if let Some(h) = std::env::var_os("HOME") { assert!(!is_volatile_source(&format!("{}/Documents/x.png", h.to_string_lossy()), &[])); }
    }
    #[test] fn local_volume_is_not_a_share() { assert!(!is_network_source(&std::env::temp_dir().to_string_lossy())); #[cfg(target_os = "macos")] assert_eq!(fs_type_name(Path::new("/")).as_deref(), Some("apfs")); }
}
