//! Saved shares (canon rows 444, 445, 498; UFB 1.4.2's mounts, read into `docs/phase5/step0-mounts-agent.md`): a share
//! is a NAME and an ADDRESS - or, on a machine where the same storage is simply a folder, that folder. The
//! application asks the system to connect it and then only WATCHES whether it is there ("ask and watch": it does not
//! own the mount's life - no tearing down and remounting on a judgement of its own). Shares are managed in the
//! sidebar, in place; there is no mount-management dialog, no agent, no cache.
//!
//! WHERE a share is on this machine is the SYSTEM'S answer, asked for by the share's server and name in the system's
//! own list of mounts - never guessed from its name: on a Mac it may have landed as `Mount-2` (row 445), on Windows
//! under any letter. A share that is already there - connected by Finder, by Explorer, at login - is adopted, and never
//! disconnected by us.
//!
//! The pair that makes a share ONE identity on every machine (`db::Mapping`: its Windows side and its macOS side)
//! comes from the share itself: `\\server\share` and `/Volumes/<share>`. There is no second list of path mappings.
use crate::db::Mapping;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Share {
    pub id: String, pub name: String,
    /// a network share: its server, and the share with whatever lies under it in the address (`Server/Flame/JOBS`)
    pub server: String, pub share: String,
    /// ... or a folder of this machine that IS the storage here (kept in step by something else; nothing to connect)
    pub local: String,
    /// its folders are jobs; connect it when the application starts; (Windows) the letter it is given
    pub jobs: bool, pub at_launch: bool, pub letter: String,
    /// the two sides of its identity (`\\server\share`, `/Volumes/share`): the same on every machine that has it
    pub win: String, pub mac: String,
    /// this application connected it (it may disconnect it: only then)
    pub ours: bool,
}
/// How a share stands, in words the sidebar says.
#[derive(Clone, Debug, PartialEq)]
pub enum State { /// there, at this place
    Connected(PathBuf), /// there, but under another name than its own (`Mount-2`): it works; the notice says so
    Landed(PathBuf), NotConnected, Connecting, /// the system wants a name and a password
    SignIn, /// the server does not answer, or refused: why
    Unreachable(String) }
impl State {
    pub fn root(&self) -> Option<&Path> { match self { State::Connected(p) | State::Landed(p) => Some(p), _ => None } }
    pub fn words(&self) -> String { match self { State::Connected(_) => String::new(), State::Landed(p) => format!("as {}", p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()), State::NotConnected => "not connected".into(), State::Connecting => "connecting\u{2026}".into(), State::SignIn => "sign-in needed".into(), State::Unreachable(_) => "not reachable".into() } }
}

/// The last part of a share's path: what it is called, and the folder it is mounted as.
fn leaf(share: &str) -> &str { share.trim_matches('/').rsplit('/').next().unwrap_or("") }
/// An address as it is typed - `smb://server/share`, `\\server\share`, `//server/share`, either on either system - or
/// a folder of this machine. Err: neither.
pub fn parse(text: &str) -> Result<Share, String> {
    let t = text.trim(); if t.is_empty() { return Err("Type a share's address (smb://server/share) or a folder.".into()) }
    let low = t.to_lowercase(); let slashed = t.replace('\\', "/");
    let net = if low.starts_with("smb://") || low.starts_with("cifs://") { Some(slashed.splitn(2, "://").nth(1).unwrap_or("").to_string()) } else if slashed.starts_with("//") { Some(slashed[2..].to_string()) } else { None };
    if let Some(rest) = net {
        let rest = rest.split_once('@').map(|(_, r)| r.to_string()).unwrap_or(rest);   // (a name in the address is not kept: the system holds who signs in)
        let (server, share) = rest.split_once('/').map(|(a, b)| (a.trim(), b.trim_matches('/'))).unwrap_or((rest.trim(), ""));
        if server.is_empty() || share.is_empty() { return Err("A share's address names the server and the share: smb://server/share".into()) }
        let share = crate::shares::decode(share); let name = leaf(&share).to_string();
        return Ok(Share { id: new_id(), name: name.clone(), server: server.to_string(), win: format!("\\\\{server}\\{}", share.replace('/', "\\")), mac: format!("/Volumes/{name}"), share, at_launch: true, ..Default::default() })
    }
    let p = Path::new(t); if !p.is_absolute() { return Err("That is neither a share's address nor a folder's path.".into()) }
    // a folder: its name is the share's on the other system too (`C:\Volumes\X` and `/Volumes/X` - 1.4.x's habit)
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| t.to_string());
    // (a folder under `/Volumes` or `C:\\Volumes` keeps its WHOLE path on the other system: `/Volumes/a/b` is `C:\\Volumes\\a\\b`)
    let sl = t.replace('\\', "/"); let low = sl.to_lowercase(); let under = low.strip_prefix("/volumes/").or_else(|| low.strip_prefix("c:/volumes/")).map(|r| sl[sl.len() - r.len()..].trim_matches('/').to_string()).filter(|r| !r.is_empty());
    let (other_win, other_mac) = match &under { Some(rest) => (format!("C:\\Volumes\\{}", rest.replace('/', "\\")), format!("/Volumes/{rest}")), None => (format!("C:\\Volumes\\{name}"), format!("/Volumes/{name}")) };
    let (win, mac) = if cfg!(windows) { (t.to_string(), other_mac) } else { (other_win, t.to_string()) };
    Ok(Share { id: new_id(), name, local: t.to_string(), win, mac, ..Default::default() })
}
fn new_id() -> String { uuid::Uuid::new_v4().simple().to_string()[..12].to_string() }
/// `%20` and its like in an address, decoded.
pub fn decode(s: &str) -> String { let b = s.as_bytes(); let (mut out, mut i) = (Vec::with_capacity(b.len()), 0); while i < b.len() { if b[i] == b'%' && i + 3 <= b.len() { if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) { out.push(v); i += 3; continue } } out.push(b[i]); i += 1; } String::from_utf8_lossy(&out).into_owned() }
fn encode(s: &str) -> String { s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect() }

/// The shares the build was started with (`--map <windows side>=<macOS side>`): mappings with nothing to connect.
static STARTED_WITH: std::sync::RwLock<Vec<Mapping>> = std::sync::RwLock::new(Vec::new());
pub fn set_started_with(m: Vec<Mapping>) { *STARTED_WITH.write().unwrap() = m; }
pub fn started_with() -> Vec<Mapping> { STARTED_WITH.read().unwrap().clone() }

// ---------------------------------------------------------------- kept in the settings
fn to_json(s: &Share) -> Value { json!({ "id": s.id, "name": s.name, "server": s.server, "share": s.share, "local": s.local, "jobs": s.jobs, "atLaunch": s.at_launch, "letter": s.letter, "win": s.win, "mac": s.mac, "ours": s.ours }) }
fn from_json(v: &Value) -> Option<Share> { let t = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string(); let b = |k: &str| v.get(k).and_then(|x| x.as_bool()).unwrap_or(false);
    let s = Share { id: t("id"), name: t("name"), server: t("server"), share: t("share"), local: t("local"), jobs: b("jobs"), at_launch: b("atLaunch"), letter: t("letter"), win: t("win"), mac: t("mac"), ours: b("ours") }; (!s.id.is_empty() && (!s.server.is_empty() || !s.local.is_empty())).then_some(s) }
pub fn load() -> Vec<Share> { let text = crate::settings::with(|s| s.string("shares/list")); serde_json::from_str::<Value>(&text).ok().and_then(|v| v.as_array().map(|a| a.iter().filter_map(from_json).collect())).unwrap_or_default() }
pub fn save(shares: &[Share]) { let text = Value::Array(shares.iter().map(to_json).collect()).to_string(); crate::settings::with(|s| s.set_str("shares/list", &text)); }

// ---------------------------------------------------------------- where a share is: the system's answer
/// A network volume as the system has it: where it comes from (`server`, `share path`, lower case) and where it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Mount { pub server: String, pub share: String, pub on: PathBuf }
/// `//user@server/share%20name` (macOS's mount table) or `\\server\share` as (server, share), lower case, decoded.
pub fn source(from: &str) -> Option<(String, String)> {
    let t = from.replace('\\', "/"); let t = t.strip_prefix("//")?; let t = t.split_once('@').map(|(_, r)| r).unwrap_or(t);
    let (server, share) = t.split_once('/')?; let share = decode(share.trim_matches('/')); (!server.is_empty() && !share.is_empty()).then(|| (server.to_lowercase(), share.to_lowercase()))
}
/// The network volumes mounted now. (No I/O on the shares themselves: the system's list.)
pub fn mounts() -> Vec<Mount> {
    #[cfg(target_os = "macos")] {
        let mut out = vec![]; let mut list: *mut libc::statfs = std::ptr::null_mut(); let n = unsafe { libc::getmntinfo(&mut list, libc::MNT_NOWAIT) };
        for i in 0..n.max(0) as usize { let m = unsafe { &*list.add(i) }; let text = |c: &[libc::c_char]| unsafe { std::ffi::CStr::from_ptr(c.as_ptr()) }.to_string_lossy().into_owned();
            if text(&m.f_fstypename) != "smbfs" { continue } if let Some((server, share)) = source(&text(&m.f_mntfromname)) { out.push(Mount { server, share, on: PathBuf::from(text(&m.f_mntonname)) }); } }
        out
    }
    #[cfg(windows)] {
        use windows::core::{HSTRING, PWSTR}; use windows::Win32::NetworkManagement::WNet::WNetGetConnectionW;
        let mut out = vec![];
        for l in b'A'..=b'Z' { let drive = format!("{}:", l as char); let mut buf = [0u16; 1024]; let mut len = buf.len() as u32;
            if unsafe { WNetGetConnectionW(&HSTRING::from(drive.as_str()), Some(PWSTR(buf.as_mut_ptr())), &mut len) }.is_ok() { let remote = String::from_utf16_lossy(&buf[..buf.iter().position(|c| *c == 0).unwrap_or(0)]); if let Some((server, share)) = source(&remote) { out.push(Mount { server, share, on: PathBuf::from(format!("{drive}\\")) }); } } }
        out
    }
    #[cfg(not(any(target_os = "macos", windows)))] { vec![] }
}
/// How a share stands, from the system's list of mounts: a network share is where the system has its server and
/// share (a mount of something UNDER the share's own path counts for nothing; one of the share itself, or of what it
/// lies under, does); a folder is there when it is a folder. `exists`: whether a path is a folder (injected: tests).
pub fn resolve(s: &Share, mounts: &[Mount], exists: &dyn Fn(&Path) -> bool) -> State {
    if !s.local.is_empty() { let p = PathBuf::from(&s.local); return if exists(&p) { State::Connected(p) } else { State::NotConnected } }
    let (server, share) = (s.server.to_lowercase(), s.share.to_lowercase());
    for m in mounts { if m.server != server { continue }
        // the share itself is mounted; or a share it lies in is (the address named a folder inside a share)
        let inside = if m.share == share { Some(String::new()) } else { share.strip_prefix(&format!("{}/", m.share)).map(|r| r.to_string()) };
        let Some(rel) = inside else { continue }; let root = if rel.is_empty() { m.on.clone() } else { m.on.join(&s.share[s.share.len() - rel.len()..]) };
        // (a Mac: under another name than its own - `Mount-2` - it is there all the same, and says so)
        let landed = cfg!(target_os = "macos") && rel.is_empty() && m.on.file_name().is_some_and(|n| !n.to_string_lossy().eq_ignore_ascii_case(leaf(&s.share)));
        return if landed { State::Landed(root) } else { State::Connected(root) } }
    State::NotConnected
}
/// A STALE PLACEHOLDER (macOS; 1.4.2's finding, row 445): an ordinary, empty folder left under `/Volumes` with the
/// share's own name - not a mount - because of which the system puts the share's next connection at `<name>-2`.
/// None: there is none (or the name is held by a real volume, or the folder holds something: never ours to remove).
pub fn placeholder(s: &Share) -> Option<PathBuf> {
    #[cfg(target_os = "macos")] { use std::os::unix::fs::MetadataExt;
        if s.server.is_empty() || leaf(&s.share).is_empty() { return None } let p = Path::new("/Volumes").join(leaf(&s.share));
        let (m, up) = (std::fs::symlink_metadata(&p).ok()?, std::fs::metadata("/Volumes").ok()?);
        // a folder itself (no link), on the SAME device as `/Volumes` (a mount point is on its own), with nothing in it
        (m.file_type().is_dir() && m.dev() == up.dev() && std::fs::read_dir(&p).ok()?.next().is_none()).then_some(p) }
    #[cfg(not(target_os = "macos"))] { let _ = s; None }
}
/// The placeholder is removed: `rmdir` (which removes nothing but an EMPTY folder) with the system's own question
/// for an administrator's password - asked only here, only on the user's press.
pub fn fix(s: &Share) -> Result<(), String> {
    let Some(p) = placeholder(s) else { return Ok(()) };   // (gone meanwhile)
    #[cfg(target_os = "macos")] { let script = format!("do shell script \"/bin/rmdir \" & quoted form of \"{}\" with administrator privileges", p.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\""));
        let out = std::process::Command::new("/usr/bin/osascript").arg("-e").arg(script).output().map_err(|e| e.to_string())?;
        if out.status.success() { Ok(()) } else { let e = String::from_utf8_lossy(&out.stderr); Err(if e.contains("-128") { "The password was not given.".into() } else { e.trim().to_string() }) } }
    #[cfg(not(target_os = "macos"))] { let _ = p; Ok(()) }
}
/// The shares as path mappings, with where each really is now.
pub fn mappings_of(shares: &[Share], states: &[State]) -> Vec<Mapping> { shares.iter().enumerate().map(|(i, s)| Mapping::new(&s.win, &s.mac).at(states.get(i).and_then(|st| st.root()).map(|p| p.to_string_lossy().into_owned()))).collect() }

// ---------------------------------------------------------------- asking the system to connect
/// The system is asked to connect a share, as Finder or Explorer would (blocking: called on a thread of its own).
/// `ui`: the user asked - the system's own sign-in may appear; else it is silent. One request at a time: the
/// system's mount service can hang for the whole machine under several (1.4.2's hard rule).
/// Gives how it stands, and whether THIS request connected it (one that was there already is adopted, not ours).
pub fn connect(s: &Share, ui: bool) -> (State, bool) {
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(()); let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    if s.server.is_empty() { return (resolve(s, &[], &|p| p.is_dir()), false) }
    // already there (connected meanwhile, by anything): adopted
    let now = resolve(s, &mounts(), &|p| p.is_dir()); if now.root().is_some() { return (now, false) }
    // the server is looked for first: a request for one that is not there never reaches the mount service (1.4.2: 3 s)
    use std::net::ToSocketAddrs; let host = format!("{}:445", s.server);
    let up = host.to_socket_addrs().ok().and_then(|mut a| a.next()).is_some_and(|a| std::net::TcpStream::connect_timeout(&a, std::time::Duration::from_secs(3)).is_ok());
    if !up { return (State::Unreachable(format!("{} does not answer.", s.server)), false) }
    match platform::connect(s, ui) { Ok(()) => { let st = resolve(s, &mounts(), &|p| p.is_dir()); if st.root().is_some() { (st, true) } else { (State::Unreachable("The system connected it, but it is not in its list.".into()), false) } } Err(st) => (st, false) }
}
/// A share THIS application connected is disconnected (never another's: the caller knows which are ours).
pub fn disconnect(root: &Path) -> Result<(), String> { platform::disconnect(root) }

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use objc2::rc::Retained; use objc2::runtime::AnyObject; use objc2::{class, msg_send}; use objc2_foundation::{NSString, NSURL};
    use std::ffi::c_void;
    #[link(name = "NetFS", kind = "framework")]
    unsafe extern "C" { fn NetFSMountURLSync(url: *const c_void, mountpath: *const c_void, user: *const c_void, passwd: *const c_void, open_options: *const c_void, mount_options: *const c_void, mountpoints: *mut *const c_void) -> i32; }
    /// The system's own mount of a URL, where the system puts it (no place of ours: a place of ours would raise the
    /// system's approval question every time - 1.4.2), with no name and no password (the system's keychain has them).
    pub fn connect(s: &Share, ui: bool) -> Result<(), State> {
        let guest = std::env::var_os("UFB_SHARE_GUEST").is_some();   // (a test's share that takes guests)
        let url = format!("smb://{}{}/{}", if guest { "guest@" } else { "" }, s.server, encode(&s.share));
        let Some(nsurl) = NSURL::URLWithString(&NSString::from_str(&url)) else { return Err(State::Unreachable("The address cannot be read.".into())) };
        let code = unsafe {
            let open: Retained<AnyObject> = msg_send![class!(NSMutableDictionary), new];
            let _: () = msg_send![&*open, setObject: &*NSString::from_str(if ui { "AllowUI" } else { "NoUI" }), forKey: &*NSString::from_str("UIOption")];
            if guest { let yes: Retained<AnyObject> = msg_send![class!(NSNumber), numberWithBool: true]; let _: () = msg_send![&*open, setObject: &*yes, forKey: &*NSString::from_str("Guest")]; }
            let mut points: *const c_void = std::ptr::null();
            NetFSMountURLSync(Retained::as_ptr(&nsurl) as *const c_void, std::ptr::null(), std::ptr::null(), std::ptr::null(), Retained::as_ptr(&open) as *const c_void, std::ptr::null(), &mut points)
        };
        match code { 0 | 17 => Ok(()),   // (17: it is there already)
            // the system would have to ask who signs in (and was not allowed to), or the answer was refused
            13 | 80 | 1 | -5045 | -5046 | -6600 | -5023 => Err(State::SignIn),
            89 | -128 => Err(State::NotConnected),   // the sign-in was left
            2 => Err(State::Unreachable(format!("{} has no share of that name.", s.server))),
            c => Err(State::Unreachable(format!("The system could not connect it ({c})."))) }
    }
    pub fn disconnect(root: &Path) -> Result<(), String> {
        let out = std::process::Command::new("/usr/sbin/diskutil").arg("unmount").arg(root).output().map_err(|e| e.to_string())?;
        if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_string()).or_else(|e: String| if e.is_empty() { Err("It is in use.".to_string()) } else { Err(e) }) }
    }
}
#[cfg(not(target_os = "macos"))]
mod platform {
    use super::*;
    /// (Windows: connecting from here is not built yet - a share that Explorer, or Windows at sign-in, has connected is
    /// found and used; a folder is used as it is.)
    pub fn connect(_: &Share, _: bool) -> Result<(), State> { Err(State::Unreachable("Connecting from here is not built on this system yet: connect it in Explorer, and it is found.".into())) }
    pub fn disconnect(_: &Path) -> Result<(), String> { Err("Not built on this system yet.".into()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_address_is_read_in_either_systems_spelling() {
        let a = parse("smb://nas.local/Jobs").unwrap(); assert_eq!((a.server.as_str(), a.share.as_str(), a.name.as_str(), a.win.as_str(), a.mac.as_str(), a.at_launch), ("nas.local", "Jobs", "Jobs", "\\\\nas.local\\Jobs", "/Volumes/Jobs", true));
        let b = parse("\\\\NAS.local\\Jobs\\").unwrap(); assert_eq!((b.server.as_str(), b.share.as_str()), ("NAS.local", "Jobs")); assert_eq!(Mapping::new(&a.win, &a.mac).uuid, Mapping::new(&b.win, &b.mac).uuid, "either spelling: one identity");
        // a name in the address is not kept; what lies under the share is; %20 is a space
        let c = parse("smb://chris@server/Flame%20Server/Flame/JOBS").unwrap(); assert_eq!((c.server.as_str(), c.share.as_str(), c.name.as_str(), c.mac.as_str()), ("server", "Flame Server/Flame/JOBS", "JOBS", "/Volumes/JOBS"));
        assert!(parse("smb://server").is_err()); assert!(parse("  ").is_err()); assert!(parse("relative/path").is_err());
        // a folder of this machine: the same name on the other system
        let f = parse(if cfg!(windows) { "C:\\Volumes\\gfx" } else { "/Volumes/gfx" }).unwrap(); assert!(f.server.is_empty()); assert_eq!((f.name.as_str(), f.win.as_str(), f.mac.as_str()), ("gfx", "C:\\Volumes\\gfx", "/Volumes/gfx"));
        // a folder deeper under the volumes' folder keeps its whole path on the other system: the same pair from either machine
        let d = parse(if cfg!(windows) { "C:\\Volumes\\gfx\\jobs" } else { "/Volumes/gfx/jobs" }).unwrap(); assert_eq!((d.name.as_str(), d.win.as_str(), d.mac.as_str()), ("jobs", "C:\\Volumes\\gfx\\jobs", "/Volumes/gfx/jobs"));
        assert_eq!(from_json(&to_json(&c)), Some(c));
    }
    #[test]
    fn a_share_is_where_the_system_says_it_is() {
        let m = |from: &str, on: &str| { let (server, share) = source(from).unwrap(); Mount { server, share, on: PathBuf::from(on) } }; let yes = |_: &Path| true; let no = |_: &Path| false;
        assert_eq!(source("//chris@NAS.local/Jobs%20Live"), Some(("nas.local".into(), "jobs live".into()))); assert_eq!(source("\\\\nas\\Jobs"), Some(("nas".into(), "jobs".into()))); assert_eq!(source("/dev/disk3s1"), None);
        let jobs = parse("smb://nas.local/Jobs").unwrap();
        // where the system has it - under its own name, or (a Mac) under another: it is there all the same
        assert_eq!(resolve(&jobs, &[m("//u@nas.local/Jobs", "/Volumes/Jobs")], &yes), State::Connected("/Volumes/Jobs".into()));
        let landed = resolve(&jobs, &[m("//u@nas.local/Jobs", "/Volumes/Jobs-2")], &yes); assert_eq!(landed.root(), Some(Path::new("/Volumes/Jobs-2"))); if cfg!(target_os = "macos") { assert_eq!(landed, State::Landed("/Volumes/Jobs-2".into())); assert_eq!(landed.words(), "as Jobs-2"); }
        // another server's share of the same name is not it; nor a mount of a folder INSIDE it
        assert_eq!(resolve(&jobs, &[m("//u@other/Jobs", "/Volumes/Jobs"), m("//u@nas.local/Jobs/2601", "/Volumes/2601")], &yes), State::NotConnected);
        // an address that named a folder inside a share: found under the mount of the share
        let deep = parse("smb://nas.local/Flame/JOBS").unwrap(); assert_eq!(resolve(&deep, &[m("//u@nas.local/Flame", "/Volumes/Flame")], &yes).root(), Some(Path::new("/Volumes/Flame/JOBS")));
        assert_eq!(resolve(&deep, &[m("//u@nas.local/Flame/JOBS", "/Volumes/JOBS")], &yes), State::Connected("/Volumes/JOBS".into()));
        // a folder: there when it is a folder
        let f = parse(if cfg!(windows) { "C:\\Volumes\\gfx" } else { "/Volumes/gfx" }).unwrap(); assert!(resolve(&f, &[], &yes).root().is_some()); assert_eq!(resolve(&f, &[], &no), State::NotConnected);
        // the mappings carry where each really is
        let maps = mappings_of(&[jobs.clone()], &[State::Landed("/Volumes/Jobs-2".into())]); assert_eq!(maps[0].here.as_deref(), Some("/Volumes/Jobs-2")); assert_eq!(maps[0].mac, "/Volumes/Jobs");
        assert_eq!(mappings_of(&[jobs], &[State::NotConnected])[0].here, None);
    }
}
