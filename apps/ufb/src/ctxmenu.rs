//! The context menu of files, folders and places (canon row 460, point 2; spike S19). macOS: ONE native menu that
//! holds our own rows and the system's for the same files - Open, Open With, Show in Finder, Share.
//!
//! S19's rules, kept here: the menu is never shown from inside a winit callback - it is handed to the run loop
//! ([`sys::show`] returns at once); a row of OURS comes back later as a message to the loop ([`take`]), the system's
//! rows act by themselves; the callbacks AppKit calls catch what AppKit throws.
//!
//! Rename, Duplicate, Copy, Paste, Move to Trash and New Folder act (`ops.rs`); Share is a row of ours that shows the system's picker. Tags (they are written INTO files) and Get Info (it asks Finder
//! by an Apple event) are not here yet. Windows' menu - our own drawn one with "Show more options" - is not built.
use std::path::PathBuf;

/// Our own rows.
pub const OPEN: u32 = 1; pub const NEW_WINDOW: u32 = 2; pub const CONTENTS: u32 = 3; pub const REFRESH: u32 = 4;
pub const RENAME: u32 = 5; pub const TRASH: u32 = 6; pub const NEW_FOLDER: u32 = 7; pub const DUPLICATE: u32 = 8; pub const COPY: u32 = 9; pub const PASTE: u32 = 10; pub const PREVIEW: u32 = 11; pub const SPLIT: u32 = 12; pub const GROUP: u32 = 13; pub const NEW_TAB: u32 = 14; pub const SUBSCRIBE: u32 = 15; pub const UNSUBSCRIBE: u32 = 16;

#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    /// A row of ours: its number comes back when it is chosen. `id` 0: a row of a later slice, shown disabled.
    Ours { id: u32, label: std::borrow::Cow<'static, str> },
    Sep,
    /// The system's: open with the default application; the applications that open these files; show in Finder; share.
    Open, OpenWith, Reveal, Share,
}
/// A menu asked for: where (pixels of the window's content), the files the system's rows are about, the rows.
#[derive(Clone, Debug, PartialEq)]
pub struct Ask { pub at: (f64, f64), pub paths: Vec<PathBuf>, pub rows: Vec<Row> }

/// Each system's own word (as in the menu bar).
const TRASH_LABEL: &str = if cfg!(windows) { "Delete" } else { "Move to Trash" };
fn ours(id: u32, label: &'static str) -> Row { Row::Ours { id, label: label.into() } }
/// A row of ours whose words are made at the moment (a column's name).
pub fn named(id: u32, label: String) -> Row { Row::Ours { id, label: label.into() } }
#[allow(dead_code)]
fn later(label: &'static str) -> Row { Row::Ours { id: 0, label: label.into() } }

/// The rows for the chosen entries; each: (it is a folder, it is a package). `sequence`: the one entry is an image
/// sequence shown as one.
pub fn rows_for(entries: &[(bool, bool)], sequence: bool) -> Vec<Row> {
    let one = entries.len() == 1; let folders = entries.iter().any(|e| e.0);
    let mut rows = vec![];
    if one && folders { rows.extend([ours(OPEN, "Open"), ours(NEW_TAB, "Open in New Tab"), ours(NEW_WINDOW, "Open in New Window"), Row::Sep]); }
    else if !folders { if one { rows.push(ours(PREVIEW, "Preview")); } rows.push(Row::Open); if one && entries[0].1 { rows.push(ours(CONTENTS, "Show Package Contents")); } rows.extend([Row::OpenWith, Row::Sep]); }
    rows.extend([Row::Reveal, Row::Sep]);
    // a hand's word against the detector's (row 475): a sequence as its files; several files as one sequence
    if sequence { rows.extend([ours(SPLIT, "Show as Separate Files"), Row::Sep]); } else if !one && !folders { rows.extend([ours(GROUP, "Group as Sequence"), Row::Sep]); }
    if one && !sequence { rows.push(ours(RENAME, "Rename")); }
    rows.extend([ours(DUPLICATE, "Duplicate"), ours(COPY, "Copy"), ours(TRASH, TRASH_LABEL), Row::Sep, Row::Share]);
    rows
}
/// The rows for the folder in view itself (a click on the empty part of a pane).
pub fn rows_for_folder() -> Vec<Row> { vec![ours(NEW_TAB, "Open in New Tab"), ours(NEW_WINDOW, "Open in New Window"), Row::Reveal, Row::Sep, ours(NEW_FOLDER, "New Folder"), ours(PASTE, "Paste"), Row::Sep, ours(REFRESH, "Refresh")] }
/// The rows for a job in the sidebar.
pub fn rows_for_job() -> Vec<Row> { vec![ours(OPEN, "Open"), ours(NEW_TAB, "Open in New Tab"), ours(NEW_WINDOW, "Open in New Window"), Row::Sep, Row::Reveal, Row::Sep, ours(UNSUBSCRIBE, "Unsubscribe")] }
/// The row that makes a folder a job in the sidebar, or takes it out again.
pub fn row_subscribe(subscribed: bool) -> Row { if subscribed { ours(UNSUBSCRIBE, "Unsubscribe") } else { ours(SUBSCRIBE, "Subscribe as Job") } }
/// The rows of a list's header: the fields to sort by - also those too far right to be seen in a narrow pane (row 483).
pub const SORT: u32 = 20;
pub const COLUMN: u32 = 30;
/// `show`: which columns are shown now (0 the name ... 3 the kind): each of the others can be hidden or shown.
pub fn rows_for_sort(show: [bool; 4]) -> Vec<Row> {
    vec![ours(SORT, "Sort by Name"), ours(SORT + 1, "Sort by Date Modified"), ours(SORT + 2, "Sort by Size"), ours(SORT + 3, "Sort by Kind"), Row::Sep,
        ours(COLUMN + 1, if show[1] { "Hide Date Modified" } else { "Show Date Modified" }), ours(COLUMN + 2, if show[2] { "Hide Size" } else { "Show Size" }), ours(COLUMN + 3, if show[3] { "Hide Kind" } else { "Show Kind" })]
}
// The item list's metadata (row 489): a column's own rows, the rows that add, show and restore columns, an item's.
pub const META_ADD: u32 = 40; pub const META_EDIT: u32 = 50; pub const META_LEFT: u32 = 51; pub const META_RIGHT: u32 = 52; pub const META_HIDE: u32 = 53; pub const META_TRASH: u32 = 54; pub const META_MODIFIED: u32 = 55; pub const META_TRACK: u32 = 56; pub const META_CLEAR: u32 = 57;
pub const META_SHOW: u32 = 1000; pub const META_RESTORE: u32 = 2000; pub const META_OPTION: u32 = 3000;
/// `column`: the menu was asked for on a column (its own rows come first). `hidden`, `trashed`: the names of the
/// columns that can be shown again, and brought back. `modified`: the date column is shown.
pub fn rows_for_columns(column: bool, hidden: &[String], trashed: &[String], modified: bool) -> Vec<Row> {
    let mut rows = vec![];
    if column { rows.extend([ours(META_EDIT, "Edit Column\u{2026}"), ours(META_LEFT, "Move Left"), ours(META_RIGHT, "Move Right"), ours(META_HIDE, "Hide Column"), ours(META_TRASH, "Move Column to Trash"), Row::Sep]); }
    for (k, t) in ["Text", "Dropdown", "Date", "Priority", "Number", "Checkbox", "Links", "Notes"].iter().enumerate() { rows.push(named(META_ADD + k as u32, format!("Add {t} Column\u{2026}"))); }
    if !hidden.is_empty() { rows.push(Row::Sep); for (k, n) in hidden.iter().enumerate() { rows.push(named(META_SHOW + k as u32, format!("Show \u{201c}{n}\u{201d}"))); } }
    if !trashed.is_empty() { rows.push(Row::Sep); for (k, n) in trashed.iter().enumerate() { rows.push(named(META_RESTORE + k as u32, format!("Restore \u{201c}{n}\u{201d} from the Trash"))); } }
    rows.extend([Row::Sep, ours(META_MODIFIED, if modified { "Hide Modified" } else { "Show Modified" })]);
    rows
}
pub const TRK_OPEN: u32 = 58;
/// The rows for a row of the tracker.
pub fn rows_for_tracked() -> Vec<Row> { vec![ours(TRK_OPEN, "Open in Job"), Row::Reveal, Row::Sep, ours(META_TRACK, "Untrack")] }
/// The rows for an item of a job's list.
pub fn rows_for_item(tracked: bool) -> Vec<Row> { vec![ours(NEW_TAB, "Open in New Tab"), ours(NEW_WINDOW, "Open in New Window"), Row::Sep, Row::Reveal, Row::Sep, ours(META_TRACK, if tracked { "Untrack" } else { "Mark as Tracked" })] }
/// The rows for a place in the sidebar.
pub fn rows_for_place() -> Vec<Row> { vec![ours(OPEN, "Open"), ours(NEW_TAB, "Open in New Tab"), ours(NEW_WINDOW, "Open in New Window"), Row::Sep, Row::Reveal] }

#[cfg(not(any(target_os = "macos", windows)))]
pub fn take() -> Vec<(winit31::window::WindowId, u32)> { vec![] }
#[cfg(windows)]
pub use win::take;

/// Windows (row 460, point 2): the context menu is OUR OWN drawn one (the window draws it from the same rows), and
/// its last row, "Show more options", opens EXPLORER'S menu for the same files - the shell's own, with every
/// installed extension, tracked on our window. From spike S19's Windows half: the shell's menu is not shown from
/// inside a winit callback - a private message is posted to the window and the menu is shown when our subclass
/// procedure receives it; while it is up the shell is handed the window's menu messages (it fills "Send to", "Open
/// with" and "New" when they open, and paints its owner-drawn rows). The shell acts on its own rows; "rename" alone
/// comes back to us (only the host can start a rename in its own list).
#[cfg(windows)]
pub mod win {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use windows::core::{Interface, PCSTR, PCWSTR, PSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_SHIFT};
    use windows::Win32::UI::Shell::Common::ITEMIDLIST;
    use windows::Win32::UI::Shell::{DefSubclassProc, IContextMenu, IContextMenu2, IContextMenu3, ILFindLastID, IShellFolder, SHBindToParent, SHParseDisplayName, SetWindowSubclass, CMF_CANRENAME, CMF_EXTENDEDVERBS, CMF_NORMAL, CMIC_MASK_CONTROL_DOWN, CMIC_MASK_PTINVOKE, CMIC_MASK_SHIFT_DOWN, CMINVOKECOMMANDINFO, CMINVOKECOMMANDINFOEX, GCS_VERBW};
    use windows::Win32::UI::WindowsAndMessaging::*;
    use winit31::window::WindowId;

    const SHELL_FIRST: u32 = 0x1000; const SHELL_LAST: u32 = 0x7FFF;
    const CMIC_MASK_UNICODE: u32 = 0x4000;   // (shobjidl_core.h; not in the windows crate's Shell module)
    const WM_UFB_SHOW: u32 = WM_APP + 0x55; const WM_UFB_CANCEL: u32 = WM_APP + 0x56;
    /// What comes back to the window after the shell's menu: one of our rows' numbers (`super::RENAME`), or
    /// `super::REFRESH` - the shell did something to the files, the panes read again.
    static CHOSEN: Mutex<Vec<(WindowId, u32)>> = Mutex::new(Vec::new());
    pub fn take() -> Vec<(WindowId, u32)> { std::mem::take(&mut *CHOSEN.lock().unwrap()) }

    struct Spec { window: WindowId, paths: Vec<PathBuf>, at: POINT, wake: Arc<dyn Fn() + Send + Sync> }
    thread_local! {
        static PENDING: RefCell<Option<Spec>> = const { RefCell::new(None) };
        static TRACKED: RefCell<(Option<IContextMenu3>, Option<IContextMenu2>)> = const { RefCell::new((None, None)) };
        static SUBCLASSED: RefCell<Vec<isize>> = const { RefCell::new(Vec::new()) };
    }
    unsafe extern "system" fn subclass(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
        match msg {
            WM_UFB_SHOW => { if let Some(spec) = PENDING.with(|p| p.borrow_mut().take()) { run(hwnd, spec); } return LRESULT(0) }
            // (the scripted looks: EndMenu must be called by the thread that tracks the menu, which is this one)
            WM_UFB_CANCEL => { let _ = unsafe { EndMenu() }; return LRESULT(0) }
            WM_INITMENUPOPUP | WM_DRAWITEM | WM_MEASUREITEM | WM_MENUCHAR => {
                // (WM_DRAWITEM / WM_MEASUREITEM with a non-zero wParam come from a control, not from a menu)
                let from_menu = !((msg == WM_DRAWITEM || msg == WM_MEASUREITEM) && wp.0 != 0);
                let (cm3, cm2) = TRACKED.with(|c| c.borrow().clone());
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
    /// A path as the shell takes it: absolute, with backslashes, never the `\\?\` form (S18).
    fn shell_path(p: &std::path::Path) -> String {
        let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf()).to_string_lossy().replace('/', "\\");
        if let Some(r) = abs.strip_prefix("\\\\?\\UNC\\") { return format!("\\\\{r}") }
        abs.strip_prefix("\\\\?\\").map(str::to_string).unwrap_or(abs)
    }
    unsafe fn verb_of(cm: &IContextMenu, off: u32) -> String { let mut buf = [0u16; 128]; if unsafe { cm.GetCommandString(off as usize, GCS_VERBW, None, PSTR(buf.as_mut_ptr() as *mut u8), buf.len() as u32) }.is_ok() { let n = buf.iter().position(|c| *c == 0).unwrap_or(0); String::from_utf16_lossy(&buf[..n]) } else { String::new() } }

    /// The shell's menu for `paths` (all in ONE folder), tracked at `at` (screen pixels). Ok: the verb of the row
    /// that was chosen and invoked ("" for one without a name), or none when nothing was chosen; "rename" is not invoked.
    unsafe fn show(owner: HWND, at: POINT, paths: &[PathBuf]) -> windows::core::Result<Option<String>> { unsafe {
        let mut abs = Pidls(vec![]);
        for p in paths { let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut(); let wide: Vec<u16> = shell_path(p).encode_utf16().chain(Some(0)).collect(); SHParseDisplayName(PCWSTR(wide.as_ptr()), None, &mut pidl, 0, None)?; abs.0.push(pidl); }
        let folder: IShellFolder = SHBindToParent(abs.0[0], None)?;
        let children: Vec<*const ITEMIDLIST> = abs.0.iter().map(|p| ILFindLastID(*p) as *const ITEMIDLIST).collect();
        let cm: IContextMenu = folder.GetUIObjectOf(owner, &children, None)?;
        let menu = CreatePopupMenu()?;
        // Shift held: the shell's extended verbs too
        let shift = GetKeyState(VK_SHIFT.0 as i32) < 0; let control = GetKeyState(VK_CONTROL.0 as i32) < 0;
        cm.QueryContextMenu(menu, 0, SHELL_FIRST, SHELL_LAST, CMF_NORMAL | CMF_CANRENAME | if shift { CMF_EXTENDEDVERBS } else { 0 }).ok()?;
        if std::env::var_os("UFB_DEMO").is_some() { eprintln!("UFB_DEMO: the shell's menu holds {} rows", GetMenuItemCount(Some(menu))); }
        TRACKED.with(|c| *c.borrow_mut() = (cm.cast::<IContextMenu3>().ok(), cm.cast::<IContextMenu2>().ok()));
        let _ = SetForegroundWindow(owner);   // so that a click elsewhere dismisses the menu
        let cmd = TrackPopupMenuEx(menu, (TPM_RETURNCMD | TPM_RIGHTBUTTON).0, at.x, at.y, owner, None).0 as u32;
        let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
        TRACKED.with(|c| *c.borrow_mut() = (None, None));
        let chosen = if cmd < SHELL_FIRST { None } else {
            let off = cmd - SHELL_FIRST; let verb = verb_of(&cm, off);
            if verb != "rename" {
                let ici = CMINVOKECOMMANDINFOEX { cbSize: std::mem::size_of::<CMINVOKECOMMANDINFOEX>() as u32, fMask: CMIC_MASK_UNICODE | CMIC_MASK_PTINVOKE | if shift { CMIC_MASK_SHIFT_DOWN } else { 0 } | if control { CMIC_MASK_CONTROL_DOWN } else { 0 },
                    hwnd: owner, lpVerb: PCSTR(off as usize as *const u8), lpVerbW: PCWSTR(off as usize as *const u16), nShow: SW_SHOWNORMAL.0, ptInvoke: at, ..Default::default() };
                let _ = cm.InvokeCommand(&ici as *const CMINVOKECOMMANDINFOEX as *const CMINVOKECOMMANDINFO);
            }
            Some(verb)
        };
        let _ = DestroyMenu(menu);
        Ok(chosen)
    } }
    fn run(hwnd: HWND, spec: Spec) {
        // (the scripted looks close the menu by themselves: `UFB_MENU_CLOSE=<seconds>`)
        if let Some(secs) = std::env::var("UFB_MENU_CLOSE").ok().and_then(|v| v.parse::<f64>().ok()) { let h = hwnd.0 as isize; std::thread::spawn(move || { std::thread::sleep(std::time::Duration::from_secs_f64(secs)); unsafe { let _ = PostMessageW(Some(HWND(h as *mut _)), WM_UFB_CANCEL, WPARAM(0), LPARAM(0)); } }); }
        match unsafe { show(hwnd, spec.at, &spec.paths) } {
            // the shell's "rename" is ours to do; after anything else of the shell's the panes read again
            Ok(Some(verb)) => { CHOSEN.lock().unwrap().push((spec.window, if verb == "rename" { super::RENAME } else { super::REFRESH })); (spec.wake)(); }
            Ok(None) => {}
            Err(e) => eprintln!("UFB: the shell's menu could not be shown: {e}"),
        }
        // (the window draws nothing while the shell's menu is up, and nothing tells it to begin again: we do)
        CHOSEN.lock().unwrap().push((spec.window, 0)); (spec.wake)();
    }
    /// Explorer's menu for these files (all in one folder), at a point of the window's content (pixels). Returns at
    /// once: the menu is shown when the window's own message loop reaches our message.
    pub fn more(hwnd: *mut std::ffi::c_void, window: WindowId, at: (f64, f64), paths: Vec<PathBuf>, wake: Arc<dyn Fn() + Send + Sync>) {
        if paths.is_empty() { return } let hwnd = HWND(hwnd);
        unsafe {
            let key = hwnd.0 as isize; let fresh = SUBCLASSED.with(|s| { let mut s = s.borrow_mut(); if s.contains(&key) { false } else { s.push(key); true } });
            if fresh && !SetWindowSubclass(hwnd, Some(subclass), 0x5546, 0).as_bool() { eprintln!("UFB: the window could not be given the shell's menu"); return }
            let mut pt = POINT { x: at.0 as i32, y: at.1 as i32 }; let _ = ClientToScreen(hwnd, &mut pt);
            PENDING.with(|p| *p.borrow_mut() = Some(Spec { window, paths, at: pt, wake }));
            let _ = PostMessageW(Some(hwnd), WM_UFB_SHOW, WPARAM(0), LPARAM(0));
        }
    }
}
#[cfg(target_os = "macos")]
pub use sys::take;

#[cfg(target_os = "macos")]
pub mod sys {
    use super::{Ask, Row};
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Sel};
    use objc2::{define_class, msg_send, sel, AllocAnyThread, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{NSImage, NSMenu, NSMenuItem, NSSharingServicePicker, NSView, NSWorkspace, NSWorkspaceOpenConfiguration};
    use objc2_foundation::{ns_string, NSArray, NSFileManager, NSObject, NSObjectProtocol, NSPoint, NSSize, NSString, NSURL};
    use std::cell::RefCell;
    use std::panic::AssertUnwindSafe;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
    use std::sync::{Arc, Mutex};
    use winit31::window::WindowId;

    /// A menu is up (or about to be): another is not started.
    static BUSY: AtomicBool = AtomicBool::new(false);
    /// Rows of ours that were chosen: (the window the menu was for, the row's number).
    static CHOSEN: Mutex<Vec<(WindowId, u32)>> = Mutex::new(Vec::new());
    pub fn take() -> Vec<(WindowId, u32)> { std::mem::take(&mut *CHOSEN.lock().unwrap()) }

    struct Pending { view: Retained<NSView>, window: WindowId, ask: Ask, wake: Arc<dyn Fn() + Send + Sync> }
    #[derive(Default)]
    struct State {
        target: Option<Retained<Target>>, pending: Option<Pending>,
        /// what the menu on screen is for; the share picker owns its row and must outlive the menu
        paths: Vec<PathBuf>, window: Option<WindowId>, wake: Option<Arc<dyn Fn() + Send + Sync>>, picker: Option<Retained<NSSharingServicePicker>>,
        /// Share was chosen: the system's picker is shown once the menu is gone
        share: bool,
    }
    thread_local! { static ST: RefCell<State> = RefCell::new(State::default()); }
    fn st<R>(f: impl FnOnce(&mut State) -> R) -> R { ST.with(|s| f(&mut s.borrow_mut())) }
    /// What AppKit throws inside a callback of ours stops there.
    fn guarded(f: impl FnOnce()) { let _ = objc2::exception::catch(AssertUnwindSafe(f)); }

    define_class!(
        /// The target of every row and of the delayed calls.
        #[unsafe(super(NSObject))] #[thread_kind = MainThreadOnly] #[name = "UFBContextMenuTarget"]
        struct Target;
        unsafe impl NSObjectProtocol for Target {}
        impl Target {
            /// A row of ours: its tag is its number; the window hears of it through the loop.
            #[unsafe(method(ours:))]
            fn ours(&self, sender: &NSMenuItem) {
                let id = sender.tag(); let (window, wake) = st(|s| (s.window, s.wake.clone()));
                if let (Some(w), true) = (window, id > 0) { CHOSEN.lock().unwrap().push((w, id as u32)); if let Some(wake) = wake { wake(); } }
            }
            /// Open with the default application, as a double click in Finder would.
            #[unsafe(method(open:))]
            fn open(&self, _sender: &NSMenuItem) { let paths = st(|s| s.paths.clone()); guarded(|| { let ws = NSWorkspace::sharedWorkspace(); for p in paths.iter().take(50) { ws.openURL(&url_of(p)); } }); }
            /// "Open With <application>": the application's URL rides on the row.
            #[unsafe(method(openWith:))]
            fn open_with(&self, sender: &NSMenuItem) {
                let Some(app) = sender.representedObject().and_then(|o| o.downcast::<NSURL>().ok()) else { return }; let paths = st(|s| s.paths.clone());
                guarded(|| NSWorkspace::sharedWorkspace().openURLs_withApplicationAtURL_configuration_completionHandler(&urls_of(&paths), &app, &NSWorkspaceOpenConfiguration::configuration(), None));
            }
            #[unsafe(method(reveal:))]
            fn reveal(&self, _sender: &NSMenuItem) { let paths = st(|s| s.paths.clone()); guarded(|| NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&urls_of(&paths))); }
            /// Share: the system's picker, shown at the place of the click when the menu has closed (see `popup`).
            #[unsafe(method(share:))]
            fn share(&self, _sender: &NSMenuItem) { st(|s| s.share = true); }
            /// The scripted looks: how many windows the application has (the picker is one).
            #[unsafe(method(probe:))]
            fn probe(&self, _o: Option<&AnyObject>) { if let Some(mtm) = MainThreadMarker::new() { let ws = objc2_app_kit::NSApplication::sharedApplication(mtm).windows(); eprintln!("UFB_DEMO: {} window(s), {} visible", ws.len(), ws.iter().filter(|w| w.isVisible()).count()); } }
            /// The scripted looks: close the menu; choose a row by program.
            #[unsafe(method(cancel:))]
            fn cancel(&self, menu: &NSMenu) { menu.cancelTracking(); }
            #[unsafe(method(pick:))]
            fn pick(&self, menu: &NSMenu) { if let Some(i) = std::env::var("UFB_MENU_PICK").ok().and_then(|v| v.parse::<isize>().ok()) { guarded(|| menu.performActionForItemAtIndex(i)); } }
            /// The popup, started from the run loop itself - not from inside one of winit's callbacks.
            #[unsafe(method(deferred:))]
            fn deferred(&self, _o: Option<&AnyObject>) { if let Some(p) = st(|s| s.pending.take()) { guarded(|| popup(p)); } BUSY.store(false, Relaxed); }
        }
    );

    fn url_of(p: &Path) -> Retained<NSURL> { NSURL::fileURLWithPath(&NSString::from_str(&p.to_string_lossy())) }
    fn urls_of(paths: &[PathBuf]) -> Retained<NSArray<NSURL>> { NSArray::from_retained_slice(&paths.iter().map(|p| url_of(p)).collect::<Vec<_>>()) }
    fn app_path(app: &NSURL) -> String { app.path().map(|p| p.to_string()).unwrap_or_default() }
    fn app_name(app: &NSURL) -> String { app.path().map(|p| { let n = NSFileManager::defaultManager().displayNameAtPath(&p).to_string(); n.strip_suffix(".app").map(str::to_string).unwrap_or(n) }).unwrap_or_default() }
    fn app_version(app: &NSURL) -> String { objc2_foundation::NSBundle::bundleWithURL(app).and_then(|b| b.objectForInfoDictionaryKey(ns_string!("CFBundleShortVersionString"))).and_then(|v| v.downcast::<NSString>().ok()).map(|v| v.to_string()).unwrap_or_default() }
    /// Two copies of one application: told apart by their version, and when that is the same too, by where they are.
    fn app_names(apps: &[Retained<NSURL>]) -> Vec<String> {
        let names: Vec<String> = apps.iter().map(|a| app_name(a)).collect();
        let with_version: Vec<String> = names.iter().enumerate().map(|(i, n)| if names.iter().filter(|m| *m == n).count() > 1 { let v = app_version(&apps[i]); if v.is_empty() { n.clone() } else { format!("{n} ({v})") } } else { n.clone() }).collect();
        with_version.iter().enumerate().map(|(i, n)| if with_version.iter().filter(|m| *m == n).count() > 1 { format!("{n} \u{2014} {}", Path::new(&app_path(&apps[i])).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default()) } else { n.clone() }).collect()
    }
    /// The applications the system offers for one file: the default, and all of them (the default first, the rest by name).
    fn apps_for(path: &Path) -> (Option<Retained<NSURL>>, Vec<Retained<NSURL>>) {
        let ws = NSWorkspace::sharedWorkspace(); let url = url_of(path);
        let def = ws.URLForApplicationToOpenURL(&url); let mut rest: Vec<Retained<NSURL>> = ws.URLsForApplicationsToOpenURL(&url).iter().collect();
        rest.sort_by_key(|a| app_name(a).to_lowercase());
        let mut out: Vec<Retained<NSURL>> = def.iter().cloned().collect();
        for a in rest { if !out.iter().any(|o| app_path(o) == app_path(&a)) { out.push(a); } }
        (def, out)
    }
    /// For several files: the applications that open every one of them, and a default only when they all share it.
    /// One file of each type is asked about (the system goes by the type); too many types: no list.
    fn apps_for_all(paths: &[PathBuf]) -> (Option<Retained<NSURL>>, Vec<Retained<NSURL>>) {
        let mut seen = std::collections::HashSet::new();
        let reps: Vec<&PathBuf> = paths.iter().filter(|p| seen.insert(p.extension().map(|e| e.to_string_lossy().to_lowercase()))).collect();
        if reps.len() > 12 { return (None, vec![]) }
        let mut it = reps.into_iter(); let Some(first) = it.next() else { return (None, vec![]) }; let (mut def, mut apps) = apps_for(first);
        for p in it { let (d, a) = apps_for(p); apps.retain(|x| a.iter().any(|y| app_path(y) == app_path(x))); if d.as_ref().map(|d| app_path(d)) != def.as_ref().map(|d| app_path(d)) { def = None; } }
        if def.is_none() { apps.sort_by_key(|a| app_name(a).to_lowercase()); }
        (def, apps)
    }
    fn row(mtm: MainThreadMarker, title: &str, action: Option<Sel>, tag: isize) -> Retained<NSMenuItem> {
        let it = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &NSString::from_str(title), action, ns_string!("")) };
        if action.is_some() { if let Some(t) = st(|s| s.target.clone()) { unsafe { it.setTarget(Some(&t)) }; } }
        it.setTag(tag); it
    }
    fn icon(path: &str) -> Retained<NSImage> { let i = NSWorkspace::sharedWorkspace().iconForFile(&NSString::from_str(path)); i.setSize(NSSize::new(16.0, 16.0)); i }

    fn build(mtm: MainThreadMarker, ask: &Ask) -> Retained<NSMenu> {
        let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("")); menu.setAutoenablesItems(false);
        let sep = |menu: &NSMenu| { if menu.numberOfItems() > 0 && !menu.itemAtIndex(menu.numberOfItems() - 1).is_some_and(|i| i.isSeparatorItem()) { menu.addItem(&NSMenuItem::separatorItem(mtm)); } };
        for r in &ask.rows {
            match r {
                Row::Ours { id, label } => { let it = row(mtm, label.as_ref(), Some(sel!(ours:)), *id as isize); it.setEnabled(*id != 0); menu.addItem(&it); }
                Row::Sep => sep(&menu),
                Row::Open => menu.addItem(&row(mtm, "Open", Some(sel!(open:)), 0)),
                Row::Reveal => menu.addItem(&row(mtm, "Show in Finder", Some(sel!(reveal:)), 0)),
                Row::OpenWith => {
                    let (mut def, mut apps) = apps_for_all(&ask.paths);
                    // an application is not offered to open itself
                    apps.retain(|a| !ask.paths.iter().any(|p| p.to_string_lossy() == app_path(a).as_str())); if def.as_ref().is_some_and(|d| !apps.iter().any(|a| app_path(a) == app_path(d))) { def = None; }
                    if apps.is_empty() { continue }
                    let ow = row(mtm, "Open With", None, 0); let owm = NSMenu::initWithTitle(NSMenu::alloc(mtm), ns_string!("Open With")); owm.setAutoenablesItems(false);
                    let names = app_names(&apps);
                    for (i, a) in apps.iter().enumerate() {
                        let is_def = i == 0 && def.is_some();
                        let it = row(mtm, &if is_def { format!("{} (default)", names[i]) } else { names[i].clone() }, Some(sel!(openWith:)), 0);
                        unsafe { it.setRepresentedObject(Some(a)) }; it.setImage(Some(&icon(&app_path(a)))); owm.addItem(&it);
                        if is_def && apps.len() > 1 { owm.addItem(&NSMenuItem::separatorItem(mtm)); }
                    }
                    ow.setSubmenu(Some(&owm)); menu.addItem(&ow);
                }
                // a row of ours that shows the SYSTEM'S picker (AppKit's ready-made row did nothing in a menu shown at a
                // point: it has no view to show its picker from - the owner's walk, 2026-11-16)
                Row::Share => { if ask.paths.is_empty() { continue } let it = row(mtm, "Share\u{2026}", Some(sel!(share:)), 0);
                    if let Some(img) = NSImage::imageWithSystemSymbolName_accessibilityDescription(ns_string!("square.and.arrow.up"), None) { it.setImage(Some(&img)); } menu.addItem(&it); }
            }
        }
        // no separator as the last row
        while menu.numberOfItems() > 0 && menu.itemAtIndex(menu.numberOfItems() - 1).is_some_and(|i| i.isSeparatorItem()) { menu.removeItemAtIndex(menu.numberOfItems() - 1); }
        menu
    }
    /// The menu's rows as text (the scripted looks print it).
    fn dump(menu: &NSMenu, depth: usize, out: &mut String) {
        for it in menu.itemArray().iter() {
            let pad = "  ".repeat(depth + 1);
            if it.isSeparatorItem() { out.push_str(&format!("{pad}--------\n")); continue }
            let mut flags = vec![]; if !it.isEnabled() { flags.push("disabled".to_string()); } if it.tag() != 0 { flags.push(format!("ours {}", it.tag())); } if it.image().is_some() { flags.push("icon".into()); }
            let sub = it.submenu(); if let Some(s) = &sub { flags.push(format!("{} rows", s.numberOfItems())); }
            out.push_str(&format!("{pad}{}{}\n", it.title(), if flags.is_empty() { String::new() } else { format!("   [{}]", flags.join(", ")) }));
            if let Some(s) = sub { if depth == 0 && s.numberOfItems() <= 40 { dump(&s, depth + 1, out); } }
        }
    }

    /// Show the menu and block until it is gone (the run loop goes on turning under it).
    fn popup(p: Pending) {
        let Some(mtm) = MainThreadMarker::new() else { return }; let Some(target) = st(|s| s.target.clone()) else { return };
        st(|s| { s.paths = p.ask.paths.clone(); s.window = Some(p.window); s.wake = Some(p.wake.clone()); s.picker = None; s.share = false; });
        let menu = build(mtm, &p.ask); if menu.numberOfItems() == 0 { return }
        if std::env::var_os("UFB_DEMO").is_some() { let mut text = String::new(); dump(&menu, 0, &mut text); eprint!("UFB_DEMO: the menu\n{text}"); }
        // the scripted looks close the menu by themselves (`UFB_MENU_CLOSE=<seconds>`) and may choose a row first (`UFB_MENU_PICK=<index>`)
        let modes = NSArray::from_slice(&[unsafe { objc2_foundation::NSRunLoopCommonModes }]);
        let later = |sel: Sel, after: f64| unsafe { let _: () = msg_send![&*target, performSelector: sel, withObject: &*menu, afterDelay: after, inModes: &*modes]; };
        if let Some(secs) = std::env::var("UFB_MENU_CLOSE").ok().and_then(|v| v.parse::<f64>().ok()) { if std::env::var_os("UFB_MENU_PICK").is_some() { later(sel!(pick:), secs * 0.4); } later(sel!(cancel:), secs); }
        let scale = p.view.window().map(|w| w.backingScaleFactor()).unwrap_or(1.0).max(0.5);
        let (x, y) = (p.ask.at.0 / scale, p.ask.at.1 / scale);
        let pt = NSPoint::new(x, if p.view.isFlipped() { y } else { p.view.bounds().size.height - y });
        menu.popUpMenuPositioningItem_atLocation_inView(None, pt, Some(&p.view));
        unsafe { let _: () = msg_send![objc2::class!(NSObject), cancelPreviousPerformRequestsWithTarget: &*target]; }
        // Share: the system's picker, from where the click was (the picker is kept: it must outlive its showing)
        if st(|s| std::mem::take(&mut s.share)) {
            let objs: Retained<NSArray<AnyObject>> = unsafe { Retained::cast_unchecked(urls_of(&p.ask.paths)) };
            let picker = unsafe { NSSharingServicePicker::initWithItems(NSSharingServicePicker::alloc(), &objs) };
            picker.showRelativeToRect_ofView_preferredEdge(objc2_foundation::NSRect::new(NSPoint::new(pt.x - 1.0, pt.y - 1.0), NSSize::new(2.0, 2.0)), &p.view, objc2_foundation::NSRectEdge::MinY);
            st(|s| s.picker = Some(picker));
            if std::env::var_os("UFB_DEMO").is_some() { unsafe { let _: () = msg_send![&*target, performSelector: sel!(probe:), withObject: None::<&AnyObject>, afterDelay: 0.6f64]; } }
        }
    }

    /// Ask for the menu at a point of the window's view. Returns at once: the menu is shown by the run loop; a row of
    /// ours that is chosen is found by [`take`] after `wake` was called.
    pub fn show(view: *mut std::ffi::c_void, window: WindowId, ask: Ask, wake: Arc<dyn Fn() + Send + Sync>) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        if ask.rows.is_empty() || BUSY.swap(true, Relaxed) { return }
        let Some(view) = (unsafe { Retained::retain(view as *mut NSView) }) else { BUSY.store(false, Relaxed); return };
        let target = st(|s| s.target.get_or_insert_with(|| unsafe { msg_send![super(Target::alloc(mtm).set_ivars(())), init] }).clone());
        st(|s| s.pending = Some(Pending { view, window, ask, wake }));
        unsafe { let _: () = msg_send![&*target, performSelector: sel!(deferred:), withObject: None::<&AnyObject>, afterDelay: 0.0f64]; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn labels(rows: &[Row]) -> Vec<String> { rows.iter().map(|r| match r { Row::Ours { id, label } => format!("{label}{}", if *id == 0 { " (later)" } else { "" }), Row::Sep => "--".into(), Row::Open => "<Open>".into(), Row::OpenWith => "<Open With>".into(), Row::Reveal => "<Show in Finder>".into(), Row::Share => "<Share>".into() }).collect() }
    #[test]
    fn the_rows_follow_what_is_chosen() {
        assert_eq!(labels(&rows_for(&[(true, false)], false)), ["Open", "Open in New Tab", "Open in New Window", "--", "<Show in Finder>", "--", "Rename", "Duplicate", "Copy", TRASH_LABEL, "--", "<Share>"], "one folder");
        assert_eq!(labels(&rows_for(&[(false, false)], false)), ["Preview", "<Open>", "<Open With>", "--", "<Show in Finder>", "--", "Rename", "Duplicate", "Copy", TRASH_LABEL, "--", "<Share>"], "one file");
        assert_eq!(labels(&rows_for(&[(false, true)], false))[..4], ["Preview", "<Open>", "Show Package Contents", "<Open With>"], "an application");
        assert_eq!(labels(&rows_for(&[(false, false), (false, false)], false)), ["<Open>", "<Open With>", "--", "<Show in Finder>", "--", "Group as Sequence", "--", "Duplicate", "Copy", TRASH_LABEL, "--", "<Share>"], "several files: no Rename; they can be made one sequence");
        assert_eq!(labels(&rows_for(&[(false, false)], true)), ["Preview", "<Open>", "<Open With>", "--", "<Show in Finder>", "--", "Show as Separate Files", "--", "Duplicate", "Copy", TRASH_LABEL, "--", "<Share>"], "a sequence: no Rename; it can be shown as its files");
        assert_eq!(labels(&rows_for(&[(true, false), (false, false)], false)), ["<Show in Finder>", "--", "Duplicate", "Copy", TRASH_LABEL, "--", "<Share>"], "a folder among them: nothing opens");
        assert_eq!(labels(&rows_for_folder()), ["Open in New Tab", "Open in New Window", "<Show in Finder>", "--", "New Folder", "Paste", "--", "Refresh"]);
        assert_eq!(labels(&rows_for_place()), ["Open", "Open in New Tab", "Open in New Window", "--", "<Show in Finder>"]);
    }
}
