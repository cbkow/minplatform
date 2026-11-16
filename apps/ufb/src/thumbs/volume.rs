//! Which volume a folder is on, and where on it (canon row 456, point 4: the cache is keyed by VOLUME IDENTITY, so
//! that a share that lands under another folder name - `Mount`, `Mount-2` - or another drive letter keeps its
//! thumbnails). Called in the helpers only: asking the system about a share can stall, and a helper has a bound.
//!
//! The identity: a share is its server and share name (`net://server/share`, lower case - whoever the user, whatever
//! the mount point or letter); a local volume is the system's own identity of it (macOS: the volume's UUID; Windows:
//! its serial number). The place is the folder's path below the volume's root, with `/` between its parts.
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place { pub vol: String, pub dir: String }

/// `//user@Host/Share`, `\\host\share` -> `net://host/share`.
pub fn share_identity(from: &str) -> Option<String> {
    let s = from.replace('\\', "/"); let rest = s.strip_prefix("//")?;
    let (host, share) = rest.split_once('/')?; let host = host.rsplit('@').next().unwrap_or(host); let share = share.trim_end_matches('/');
    if host.is_empty() || share.is_empty() { return None }
    Some(format!("net://{}/{}", host.to_lowercase(), share.to_lowercase()))
}

#[cfg(target_os = "macos")]
pub fn locate(dir: &Path) -> Option<Place> {
    use std::ffi::{CStr, CString}; use std::os::unix::ffi::OsStrExt;
    let c = CString::new(dir.as_os_str().as_bytes()).ok()?;
    let mut sf: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(c.as_ptr(), &mut sf) } != 0 { return None }
    let text = |b: &[libc::c_char]| unsafe { CStr::from_ptr(b.as_ptr()) }.to_string_lossy().into_owned();
    let (from, on, fs) = (text(&sf.f_mntfromname), text(&sf.f_mntonname), text(&sf.f_fstypename));
    let net = share_identity(&from).or_else(|| (fs == "nfs").then(|| format!("net://{}", from.to_lowercase())));
    let below = |p: &Path| p.strip_prefix(&on).ok().map(|r| r.to_string_lossy().into_owned());
    let real = std::fs::canonicalize(dir).ok();
    match net {
        // a share: only a path below its mount point says where on the share the folder is
        Some(vol) => Some(Place { vol, dir: below(dir).or_else(|| real.as_deref().and_then(below))? }),
        // a local volume: below its mount point; the startup disk's folders are not below theirs (the system joins two
        // volumes into one tree), so their whole path is the place
        None => { let vol = uuid(&on).map(|u| format!("vol:{u}")).unwrap_or_else(|| format!("fs:{fs}:{from}"));
            let dir = below(dir).or_else(|| real.as_deref().and_then(below)).unwrap_or_else(|| real.as_deref().unwrap_or(dir).to_string_lossy().into_owned());
            Some(Place { vol, dir }) }
    }
}
/// The volume's UUID, asked of the volume mounted at `mount`.
#[cfg(target_os = "macos")]
fn uuid(mount: &str) -> Option<String> {
    #[repr(C, packed(4))] struct Out { len: u32, uuid: [u8; 16] }
    let c = std::ffi::CString::new(mount).ok()?;
    let mut list: libc::attrlist = unsafe { std::mem::zeroed() }; list.bitmapcount = libc::ATTR_BIT_MAP_COUNT; list.volattr = libc::ATTR_VOL_INFO | libc::ATTR_VOL_UUID;
    let mut out = Out { len: 0, uuid: [0; 16] };
    if unsafe { libc::getattrlist(c.as_ptr(), &mut list as *mut _ as *mut libc::c_void, &mut out as *mut _ as *mut libc::c_void, std::mem::size_of::<Out>(), 0) } != 0 { return None }
    let u = out.uuid; if u.iter().all(|b| *b == 0) { return None }
    Some(u.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(windows)]
pub fn locate(dir: &Path) -> Option<Place> {
    use windows::core::{PCWSTR, PWSTR};
    let s = dir.to_string_lossy().replace('/', "\\");
    let s = match s.strip_prefix("\\\\?\\UNC\\") { Some(r) => format!("\\\\{r}"), None => s.strip_prefix("\\\\?\\").map(str::to_string).unwrap_or(s) };
    if let Some(rest) = s.strip_prefix("\\\\") {
        let mut it = rest.splitn(3, '\\'); let (host, share, below) = (it.next()?, it.next()?, it.next().unwrap_or(""));
        return Some(Place { vol: share_identity(&format!("//{host}/{share}"))?, dir: below.trim_end_matches('\\').replace('\\', "/") })
    }
    let b = s.as_bytes(); if b.len() < 2 || b[1] != b':' || !b[0].is_ascii_alphabetic() { return None }
    let below = s[2..].trim_matches('\\').replace('\\', "/");
    // a letter that is a share: the share is the identity, whatever the letter
    let wide = |t: &str| -> Vec<u16> { t.encode_utf16().chain(std::iter::once(0)).collect() };
    let drive = wide(&s[..2]); let mut remote = [0u16; 1024]; let mut len = remote.len() as u32;
    if unsafe { windows::Win32::NetworkManagement::WNet::WNetGetConnectionW(PCWSTR(drive.as_ptr()), Some(PWSTR(remote.as_mut_ptr())), &mut len) }.0 == 0 {
        let n = remote.iter().position(|c| *c == 0).unwrap_or(remote.len());
        if let Some(vol) = share_identity(&String::from_utf16_lossy(&remote[..n])) { return Some(Place { vol, dir: below }) }
    }
    let root = wide(&format!("{}\\", &s[..2])); let mut serial = 0u32;
    unsafe { windows::Win32::Storage::FileSystem::GetVolumeInformationW(PCWSTR(root.as_ptr()), None, Some(&mut serial), None, None, None).ok()? };
    Some(Place { vol: format!("vol:{serial:08x}"), dir: below })
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn locate(dir: &Path) -> Option<Place> { Some(Place { vol: "fs".into(), dir: std::fs::canonicalize(dir).ok()?.to_string_lossy().into_owned() }) }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_share_is_its_server_and_name_whoever_mounted_it() {
        assert_eq!(share_identity("//artist@Studio-NAS/Jobs").as_deref(), Some("net://studio-nas/jobs"));
        assert_eq!(share_identity("//studio-nas/jobs/").as_deref(), Some("net://studio-nas/jobs"));
        assert_eq!(share_identity("\\\\STUDIO-NAS\\Jobs").as_deref(), Some("net://studio-nas/jobs"));
        assert_eq!(share_identity("//dom;user@host/share").as_deref(), Some("net://host/share"));
        assert_eq!(share_identity("/dev/disk3s1"), None); assert_eq!(share_identity("//host"), None); assert_eq!(share_identity("//host/"), None);
    }
    #[test]
    fn a_local_folder_has_a_volume_and_a_place_on_it() {
        let t = std::env::temp_dir().join(format!("ufb-volume-{}", std::process::id())); std::fs::create_dir_all(t.join("a/b")).unwrap();
        let (a, b) = (locate(&t.join("a")).unwrap(), locate(&t.join("a/b")).unwrap());
        assert_eq!(a.vol, b.vol, "one volume"); assert!(!a.vol.is_empty()); assert_eq!(b.dir, format!("{}/b", a.dir), "the place follows the folders");
        assert_eq!(locate(&t.join("a")), Some(a), "asked again: the same");
        #[cfg(target_os = "macos")] { let root = locate(Path::new("/")).unwrap(); assert!(root.vol.starts_with("vol:") && root.vol.len() == 36, "the startup disk has a UUID: {}", root.vol); assert_eq!(root.dir, ""); }
        // (macOS asks the system about the folder itself; Windows goes by the path's drive)
        #[cfg(not(windows))] assert_eq!(locate(&t.join("not-there")), None);
        let _ = std::fs::remove_dir_all(&t);
    }
}
