//! Windows: ONE popup menu that holds our own rows and, below them, the shell's own context menu for the paths
//! (IShellFolder::GetUIObjectOf -> IContextMenu::QueryContextMenu into OUR HMENU), tracked on OUR window.
//! WRITTEN ON macOS, CHECKED BY `cargo check --target x86_64-pc-windows-msvc`, NEVER RUN. Part B on the Windows box.
//!
//! Thread and apartment: the window's own thread. winit has called OleInitialize on it (a single-threaded
//! apartment), which is what shell extensions need; TrackPopupMenuEx must be called by the thread that owns the
//! owner window. The menu is NOT shown from inside a winit callback: a private message is posted to the window
//! and the menu is shown when our subclass procedure receives it, so winit's callbacks stay free while the menu's
//! modal loop dispatches messages.
use crate::{fail, Files, Step, BUSY};
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::atomic::Ordering::Relaxed;
use windows::core::{w, Interface, PCSTR, PCWSTR, PSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_SHIFT};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{DefSubclassProc, IContextMenu, IContextMenu2, IContextMenu3, ILFindLastID, IShellFolder, SHBindToParent, SHParseDisplayName, SetWindowSubclass,
    CMF_CANRENAME, CMF_EXTENDEDVERBS, CMF_NORMAL, CMIC_MASK_CONTROL_DOWN, CMIC_MASK_PTINVOKE, CMIC_MASK_SHIFT_DOWN, CMINVOKECOMMANDINFO, CMINVOKECOMMANDINFOEX, GCS_VERBW};
use windows::Win32::UI::WindowsAndMessaging::*;

/// Our rows' ids and the shell's live in two ranges of one menu: TrackPopupMenuEx returns one number.
pub const RENAME: u32 = 1; pub const TRACKED: u32 = 2; pub const OTHER_PANE: u32 = 3; pub const BOOKMARK: u32 = 4;
const OURS_LAST: u32 = 0x0FFF; const SHELL_FIRST: u32 = 0x1000; const SHELL_LAST: u32 = 0x7FFF;
const CMIC_MASK_UNICODE: u32 = 0x4000;   // (shobjidl_core.h; not in the windows crate's Shell module)
const WM_S19_SHOW: u32 = WM_APP + 0x19; const WM_S19_CANCEL: u32 = WM_APP + 0x1A;

#[derive(Debug)]
pub enum Chosen {
    Nothing,
    /// one of ours: the application acts
    Ours(u32),
    /// a shell row: already invoked through IContextMenu::InvokeCommand
    Shell { verb: String, invoked: bool },
    /// a shell row that only the HOST can do ("rename" starts OUR inline rename; the shell cannot edit our list)
    ShellForUs(String),
}
struct Spec { label: String, paths: Vec<PathBuf>, at: POINT, cancel_after_ms: Option<u64>, dump: bool }
thread_local! {
    static PENDING: RefCell<Option<Spec>> = const { RefCell::new(None) };
    /// the shell menu being tracked: owner-drawn rows and lazily filled submenus ("Send to", "Open with", "New")
    /// are painted and filled by the SHELL when it is handed the owner window's menu messages
    static TRACKED_CM: RefCell<(Option<IContextMenu3>, Option<IContextMenu2>)> = const { RefCell::new((None, None)) };
}

unsafe extern "system" fn subclass(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
    match msg {
        WM_S19_SHOW => { if let Some(spec) = PENDING.with(|p| p.borrow_mut().take()) { run(hwnd, spec); } BUSY.store(false, Relaxed); return LRESULT(0) }
        // the script's "dismiss it": EndMenu must be called by the thread that tracks the menu, which is this one
        WM_S19_CANCEL => { let _ = unsafe { EndMenu() }; return LRESULT(0) }
        WM_INITMENUPOPUP | WM_DRAWITEM | WM_MEASUREITEM | WM_MENUCHAR => {
            // (WM_DRAWITEM / WM_MEASUREITEM with a non-zero wParam come from a control, not from a menu)
            let from_menu = !((msg == WM_DRAWITEM || msg == WM_MEASUREITEM) && wp.0 != 0);
            let (cm3, cm2) = TRACKED_CM.with(|c| c.borrow().clone());
            if from_menu { unsafe {
                if let Some(cm3) = cm3 { let mut r = LRESULT(0); if cm3.HandleMenuMsg2(msg, wp, lp, Some(&mut r)).is_ok() { return match msg { WM_MENUCHAR => r, WM_INITMENUPOPUP => LRESULT(0), _ => LRESULT(1) } } }
                else if let Some(cm2) = cm2 { if msg != WM_MENUCHAR && cm2.HandleMenuMsg(msg, wp, lp).is_ok() { return LRESULT(if msg == WM_INITMENUPOPUP { 0 } else { 1 }) } }
            } }
        }
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, msg, wp, lp) }
}

/// Absolute item lists, freed when dropped.
struct Pidls(Vec<*mut ITEMIDLIST>);
impl Drop for Pidls { fn drop(&mut self) { for p in &self.0 { unsafe { CoTaskMemFree(Some(*p as *const _)) } } } }

/// Build the menu, track it at `at` (screen pixels), act. All the paths must be in ONE folder.
unsafe fn show(owner: HWND, at: POINT, paths: &[PathBuf], dump: bool) -> windows::core::Result<Chosen> { unsafe {
    let mut abs = Pidls(vec![]);
    for p in paths { let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut(); let wide: Vec<u16> = { use std::os::windows::ffi::OsStrExt; p.as_os_str().encode_wide().chain(Some(0)).collect() }; SHParseDisplayName(PCWSTR(wide.as_ptr()), None, &mut pidl, 0, None)?; abs.0.push(pidl); }
    let folder: IShellFolder = SHBindToParent(abs.0[0], None)?;
    let children: Vec<*const ITEMIDLIST> = abs.0.iter().map(|p| ILFindLastID(*p) as *const ITEMIDLIST).collect();
    let cm: IContextMenu = folder.GetUIObjectOf(owner, &children, None)?;

    let menu = CreatePopupMenu()?;
    // ours first: one plain, one greyed, one with a submenu
    AppendMenuW(menu, MF_STRING, RENAME as usize, if paths.len() == 1 { w!("Rename") } else { w!("Rename Items...") })?;
    AppendMenuW(menu, MF_STRING | MF_GRAYED, TRACKED as usize, w!("Mark as Tracked"))?;
    let sub = CreatePopupMenu()?; AppendMenuW(sub, MF_STRING, OTHER_PANE as usize, w!("Other Pane"))?; AppendMenuW(sub, MF_STRING, BOOKMARK as usize, w!("Bookmarks..."))?;
    AppendMenuW(menu, MF_POPUP, sub.0 as usize, w!("Copy to"))?;
    AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null())?;
    // then the shell's, appended into the SAME menu with ids in its own range. Shift held: the extended verbs.
    let shift = GetKeyState(VK_SHIFT.0 as i32) < 0; let control = GetKeyState(VK_CONTROL.0 as i32) < 0;
    let first = GetMenuItemCount(Some(menu)).max(0) as u32;
    let t = std::time::Instant::now();
    let hr = cm.QueryContextMenu(menu, first, SHELL_FIRST, SHELL_LAST, CMF_NORMAL | CMF_CANRENAME | if shift { CMF_EXTENDEDVERBS } else { 0 });
    hr.ok()?; let added = (hr.0 as u32) & 0xFFFF;
    // (the shell loads every installed extension here, on our window's thread: the number Part B is after)
    println!("      QueryContextMenu took {:.0} ms for {} path(s)", t.elapsed().as_secs_f64() * 1000.0, paths.len());
    if dump { println!("      the shell added ids up to +{added}; the menu now holds {} rows:", GetMenuItemCount(Some(menu))); dump_menu(menu, &cm); }

    TRACKED_CM.with(|c| *c.borrow_mut() = (cm.cast::<IContextMenu3>().ok(), cm.cast::<IContextMenu2>().ok()));
    let _ = SetForegroundWindow(owner);   // so that a click elsewhere dismisses the menu
    let cmd = TrackPopupMenuEx(menu, (TPM_RETURNCMD | TPM_RIGHTBUTTON).0, at.x, at.y, owner, None).0 as u32;
    let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
    TRACKED_CM.with(|c| *c.borrow_mut() = (None, None));

    let chosen = if cmd == 0 { Chosen::Nothing } else if cmd <= OURS_LAST { Chosen::Ours(cmd) } else {
        let off = cmd - SHELL_FIRST; let verb = verb_of(&cm, off);
        if verb == "rename" { Chosen::ShellForUs(verb) } else {
            let ici = CMINVOKECOMMANDINFOEX { cbSize: std::mem::size_of::<CMINVOKECOMMANDINFOEX>() as u32, fMask: CMIC_MASK_UNICODE | CMIC_MASK_PTINVOKE | if shift { CMIC_MASK_SHIFT_DOWN } else { 0 } | if control { CMIC_MASK_CONTROL_DOWN } else { 0 },
                hwnd: owner, lpVerb: PCSTR(off as usize as *const u8), lpVerbW: PCWSTR(off as usize as *const u16), nShow: SW_SHOWNORMAL.0, ptInvoke: at, ..Default::default() };
            Chosen::Shell { verb, invoked: cm.InvokeCommand(&ici as *const CMINVOKECOMMANDINFOEX as *const CMINVOKECOMMANDINFO).is_ok() }
        }
    };
    let _ = DestroyMenu(menu);   // (its submenus go with it)
    Ok(chosen)
} }
/// The language-independent name of a shell row ("open", "properties", "rename", "delete", "cut", "copy" ...); many extensions give none.
unsafe fn verb_of(cm: &IContextMenu, off: u32) -> String { let mut buf = [0u16; 128]; if unsafe { cm.GetCommandString(off as usize, GCS_VERBW, None, PSTR(buf.as_mut_ptr() as *mut u8), buf.len() as u32) }.is_ok() { let n = buf.iter().position(|c| *c == 0).unwrap_or(0); String::from_utf16_lossy(&buf[..n]) } else { String::new() } }
unsafe fn dump_menu(menu: HMENU, cm: &IContextMenu) { unsafe {
    for i in 0..GetMenuItemCount(Some(menu)).max(0) { let mut buf = [0u16; 128]; let n = GetMenuStringW(menu, i as u32, Some(&mut buf), MF_BYPOSITION); let id = GetMenuItemID(menu, i);
        let kind = if id == u32::MAX { "submenu".to_string() } else if id == 0 { "separator".into() } else if id <= OURS_LAST { format!("OURS id {id}") } else { format!("shell +{} verb \"{}\"", id - SHELL_FIRST, verb_of(cm, id - SHELL_FIRST)) };
        println!("        {:<40} [{kind}]", if n > 0 { String::from_utf16_lossy(&buf[..n as usize]) } else { "(no text: a separator or an owner-drawn row)".into() }); }
} }

fn run(hwnd: HWND, spec: Spec) {
    if let Some(ms) = spec.cancel_after_ms { let h = hwnd.0 as isize; std::thread::spawn(move || { std::thread::sleep(std::time::Duration::from_millis(ms)); unsafe { let _ = PostMessageW(Some(HWND(h as *mut _)), WM_S19_CANCEL, WPARAM(0), LPARAM(0)); } }); }
    let (c, t) = (crate::counters(), std::time::Instant::now());
    let r = unsafe { show(hwnd, spec.at, &spec.paths, spec.dump) };
    let (dt, d) = (t.elapsed().as_secs_f64(), crate::delta(c));
    match r { Ok(ch) => println!("   {:<26} up {dt:4.2} s  chosen {ch:?} | {}", spec.label, crate::show(d)), Err(e) => fail(&format!("{}: {e}", spec.label)) }
    if spec.cancel_after_ms.is_some() && d[0] == 0 { println!("      (no winit redraw was delivered while the menu was tracked)"); }
}
fn post(hwnd: HWND, spec: Spec) { PENDING.with(|p| *p.borrow_mut() = Some(spec)); BUSY.store(true, Relaxed); unsafe { let _ = PostMessageW(Some(hwnd), WM_S19_SHOW, WPARAM(0), LPARAM(0)); } }

pub fn init(view: *mut std::ffi::c_void, _files: &Files) { unsafe { if !SetWindowSubclass(HWND(view), Some(subclass), 0x5319, 0).as_bool() { fail("SetWindowSubclass"); } } }
/// Part B: the owner's right click; `at` is in the window's client pixels.
pub fn hand(view: *mut std::ffi::c_void, at: (f64, f64), paths: Vec<PathBuf>) {
    if BUSY.load(Relaxed) { return } let hwnd = HWND(view); let mut pt = POINT { x: at.0 as i32, y: at.1 as i32 }; unsafe { let _ = ClientToScreen(hwnd, &mut pt); }
    post(hwnd, Spec { label: format!("hand, {} path(s)", paths.len()), paths, at: pt, cancel_after_ms: None, dump: true });
}
pub fn step(s: Step, view: *mut std::ffi::c_void, files: &Files) {
    let hwnd = HWND(view); let mut pt = POINT { x: 80, y: 80 }; unsafe { let _ = ClientToScreen(hwnd, &mut pt); }
    match s {
        Step::Enumerate | Step::Plumbing => println!("\n== {s:?}: a macOS part (on Windows the shell's menu itself carries Open with)"),
        Step::Menus => { println!("\n== the menu for ONE file, shown for 1 s with its rows printed"); post(hwnd, Spec { label: "menus".into(), paths: files.one(), at: pt, cancel_after_ms: Some(1000), dump: true }); }
        Step::PopInHandler(n) | Step::PopDeferred(n) | Step::PopContext(n) => { if n == 1 { println!("\n== popups, posted to the window and dismissed by program (EndMenu) after 1 s"); }
            post(hwnd, Spec { label: format!("#{n} {}", if n % 2 == 1 { "one file" } else { "two files" }), paths: if n % 2 == 1 { files.one() } else { files.many() }, at: pt, cancel_after_ms: Some(1000), dump: false }); }
        _ => {}
    }
}
