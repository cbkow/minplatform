//! The menu bar (canon row 296): ONE definition ([`spec`]) of minNotes' menus, as QCView's `menu.rs` has it.
//! macOS: the native menu through `muda` — a shortcut is its item's key equivalent, so AppKit takes the key and the
//! command comes back as a menu event (nothing fires twice). Windows: the in-window bar (`min_ui::MenuBar`, row 194) is
//! built from the same `spec` ([`InWindowMenu`], row 370), and the window's key handler runs the shortcuts from `spec` too
//! ([`shortcut`]) - there is no system menu to take them.

#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    NewCollection, Open, OpenRecent(String), ClearRecent, Save, SaveAs, CloseCollection, NewNote, NewFolder, Rename, Quit,
    Undo, Redo, Cut, Copy, Paste, PastePlain, PasteMarkdown, SelectAll,
    Bold, Italic, Underline, Strike, Code, Highlight, ClearFormat,
    Sidebar, Toolbar, ToolbarWide, Footer,
    ExportHtml, ExportSite, ExportMarkdown, ExportDocx, ExportPdf, ExportPoster, CopyMarkdown, ExportPaths, ExportVideoNotes, ExportPackage, CollectMedia, Import, CheckForUpdates, Licenses, PathMappings, Reading, UserGuide, ShortcutsPage, GuideToggle, Pin,
}

/// A shortcut: its key, always with Cmd (macOS) / Ctrl.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sc { pub shift: bool, pub alt: bool, pub key: char }
const fn sc(key: char) -> Option<Sc> { Some(Sc { shift: false, alt: false, key }) }
const fn sc_shift(key: char) -> Option<Sc> { Some(Sc { shift: true, alt: false, key }) }
const fn sc_alt(key: char) -> Option<Sc> { Some(Sc { shift: false, alt: true, key }) }

pub enum Node { Item { label: &'static str, sc: Option<Sc>, cmd: Cmd }, /// an item with a tick (an option that stays)
    Check { label: &'static str, cmd: Cmd }, /// a ticked item with a key of its own
    CheckKey { label: &'static str, sc: Option<Sc>, cmd: Cmd }, Recent, Sep }
fn item(label: &'static str, sc: Option<Sc>, cmd: Cmd) -> Node { Node::Item { label, sc, cmd } }

/// The menus, left to right.
pub fn spec() -> Vec<(&'static str, Vec<Node>)> {
    vec![
        ("File", vec![
            item("New Collection…", sc_alt('n'), Cmd::NewCollection), item("New Note", sc('n'), Cmd::NewNote), item("New Folder", sc_shift('n'), Cmd::NewFolder), item("Rename", None, Cmd::Rename), Node::Sep,
            item("Open…", sc('o'), Cmd::Open), Node::Recent, item("Import…", None, Cmd::Import), Node::Sep,
            item("Save", sc('s'), Cmd::Save), item("Save As…", sc_shift('s'), Cmd::SaveAs), item("Collect Media…", None, Cmd::CollectMedia), item("Path Mappings…", None, Cmd::PathMappings), Node::Sep,
            item("Close Collection", sc('w'), Cmd::CloseCollection),
        ]),
        ("Edit", vec![
            item("Undo", sc('z'), Cmd::Undo), item("Redo", sc_shift('z'), Cmd::Redo), Node::Sep,
            item("Cut", sc('x'), Cmd::Cut), item("Copy", sc('c'), Cmd::Copy), item("Paste", sc('v'), Cmd::Paste), item("Paste and Match Style", sc_shift('v'), Cmd::PastePlain), item("Paste as Markdown", None, Cmd::PasteMarkdown), Node::Sep,
            item("Select All", sc('a'), Cmd::SelectAll),
        ]),
        ("Format", vec![
            item("Bold", sc('b'), Cmd::Bold), item("Italic", sc('i'), Cmd::Italic), item("Underline", sc('u'), Cmd::Underline), item("Strikethrough", sc_shift('x'), Cmd::Strike),
            item("Inline Code", sc('e'), Cmd::Code), item("Highlight", sc_shift('h'), Cmd::Highlight), Node::Sep,
            item("Clear Formatting", sc('\\'), Cmd::ClearFormat),
        ]),
        // row 328: a note written out for other applications; the two options stay as they are set
        ("Export", vec![
            item("as HTML…", None, Cmd::ExportHtml), item("as Web Folder…", None, Cmd::ExportSite), item("as Markdown…", None, Cmd::ExportMarkdown), item("as Word Document…", None, Cmd::ExportDocx), item("as PDF…", None, Cmd::ExportPdf), item("Canvas Poster (PNG)…", None, Cmd::ExportPoster), Node::Sep,
            item("Collection as Package…", None, Cmd::ExportPackage), Node::Sep,
            item("Copy as Markdown", sc_shift('c'), Cmd::CopyMarkdown), Node::Sep,
            Node::Check { label: "Include File Paths", cmd: Cmd::ExportPaths }, Node::Check { label: "Include Video Notes", cmd: Cmd::ExportVideoNotes },
        ]),
        ("View", vec![item("Sidebar", sc('1'), Cmd::Sidebar), item("Toolbar", sc('2'), Cmd::Toolbar), item("Footer", sc('3'), Cmd::Footer), Node::Sep, item("Reading Mode", sc('r'), Cmd::Reading), item("Show / Hide the User Guide", None, Cmd::GuideToggle), Node::Sep, Node::CheckKey { label: "Always on Top", sc: sc_alt('t'), cmd: Cmd::Pin }, Node::Sep, item("Toolbar Over the Page / Across the Window", None, Cmd::ToolbarWide)]),
        ("Help", vec![item("User Guide", None, Cmd::UserGuide), item("Keyboard Shortcuts", sc('/'), Cmd::ShortcutsPage)]),
    ]
}

/// The key a command is run by in the window's key handler: (character, with Shift). None: it has a routine of its own.
pub fn key_of(c: &Cmd) -> Option<(char, bool)> {
    Some(match c { Cmd::NewNote => ('n', false), Cmd::NewFolder => ('n', true), Cmd::Save => ('s', false), Cmd::Undo => ('z', false), Cmd::Redo => ('z', true), Cmd::Cut => ('x', false), Cmd::Copy => ('c', false), Cmd::Paste => ('v', false), Cmd::PastePlain => ('v', true), Cmd::SelectAll => ('a', false),
        Cmd::Bold => ('b', false), Cmd::Italic => ('i', false), Cmd::Underline => ('u', false), Cmd::Strike => ('x', true), Cmd::Code => ('e', false), Cmd::Highlight => ('h', true), Cmd::ClearFormat => ('\\', false), Cmd::Sidebar => ('1', false), Cmd::Reading => ('r', false), Cmd::ShortcutsPage => ('/', false), _ => return None })
}

impl Sc {
    /// Windows' spelling: "Ctrl+Shift+N".
    pub fn label(&self) -> String { format!("Ctrl+{}{}{}", if self.shift { "Shift+" } else { "" }, if self.alt { "Alt+" } else { "" }, self.key.to_ascii_uppercase()) }
}

/// The command a key with Ctrl runs where there is no system menu (row 370): the same keys the macOS menu takes.
pub fn shortcut(key: char, shift: bool, alt: bool) -> Option<Cmd> {
    for (_, nodes) in spec() { for n in nodes { if let Node::Item { sc: Some(s), cmd, .. } | Node::CheckKey { sc: Some(s), cmd, .. } = n { if s.key == key && s.shift == shift && s.alt == alt { return Some(cmd) } } } }
    None
}

/// The in-window bar (Windows; row 370): `spec` as a `min_ui::MenuBar`, made again when the recent list or a tick changes.
/// What macOS keeps in its application menu is here too: Exit at the foot of File, Licenses under Help.
pub struct InWindowMenu { pub bar: min_ui::MenuBar, cmds: Vec<Cmd>, recent: Vec<String>, state: Option<(Vec<String>, [bool; 3])> }

impl InWindowMenu {
    /// Windows always; macOS with `MN_MENU_BAR=1` (a look at the bar without a Windows machine).
    pub fn wanted() -> bool { cfg!(not(target_os = "macos")) || std::env::var_os("MN_MENU_BAR").is_some() }
    pub fn new() -> InWindowMenu { InWindowMenu { bar: min_ui::MenuBar::new(vec![]), cmds: vec![], recent: vec![], state: None } }
    pub fn cmd(&self, id: u32) -> Option<Cmd> { self.cmds.get(id as usize).cloned() }
    pub fn set_recent(&mut self, list: Vec<String>) { self.recent = list; }
    /// The ticks as they now are: (file paths in exports, video notes in exports, always on top).
    pub fn sync(&mut self, checks: [bool; 3], fonts: &mut min_ui::Fonts) {
        if self.state.as_ref().is_some_and(|(r, c)| *r == self.recent && *c == checks) { return }
        self.state = Some((self.recent.clone(), checks));
        use min_ui::{BarItem, BarMenu};
        let mut cmds: Vec<Cmd> = vec![];
        let mut id = |c: Cmd| { cmds.push(c); (cmds.len() - 1) as u32 };
        let on = |c: &Cmd| match c { Cmd::ExportPaths => checks[0], Cmd::ExportVideoNotes => checks[1], Cmd::Pin => checks[2], _ => false };
        let key = |sc: Option<Sc>| sc.map(|s| s.label()).unwrap_or_default();
        let mut menus: Vec<BarMenu> = spec().into_iter().map(|(title, nodes)| BarMenu { title: match title { "Format" => "F&ormat".to_string(), "Export" => "E&xport".to_string(), t => format!("&{t}") }, items: nodes.into_iter().map(|n| match n {
            Node::Item { label, sc, cmd } => BarItem::Action { id: id(cmd), label: label.into(), enabled: true, shortcut: key(sc), checked: None },
            Node::Check { label, cmd } => { let c = on(&cmd); BarItem::Action { id: id(cmd), label: label.into(), enabled: true, shortcut: String::new(), checked: Some(c) } }
            Node::CheckKey { label, sc, cmd } => { let c = on(&cmd); BarItem::Action { id: id(cmd), label: label.into(), enabled: true, shortcut: key(sc), checked: Some(c) } }
            Node::Recent => {
                let mut items: Vec<BarItem> = self.recent.iter().map(|path| BarItem::action(id(Cmd::OpenRecent(path.clone())), &path.rsplit(['/', '\\']).next().unwrap_or(path).replace('&', "&&"), true, "")).collect();
                if items.is_empty() { items.push(BarItem::action(u32::MAX, "(none)", false, "")); } else { items.push(BarItem::Separator); items.push(BarItem::action(id(Cmd::ClearRecent), "Clear Menu", true, "")); }
                BarItem::Sub { label: "Open Recent".into(), enabled: true, items }
            }
            Node::Sep => BarItem::Separator,
        }).collect() }).collect();
        if let Some(file) = menus.first_mut() { file.items.push(BarItem::Separator); file.items.push(BarItem::action(id(Cmd::Quit), "Exit", true, "Alt+F4")); }
        if let Some(help) = menus.last_mut() { help.items.push(BarItem::Separator); help.items.push(BarItem::action(id(Cmd::Licenses), "Licenses\u{2026}", true, "")); help.items.push(BarItem::Separator); help.items.push(BarItem::action(u32::MAX, concat!("minNotes ", env!("CARGO_PKG_VERSION")), false, "")); }
        self.cmds = cmds;
        self.bar.set_menus(menus, fonts);
    }
}

#[cfg(target_os = "macos")]
pub use native::{panel_takes, AppMenu};

#[cfg(target_os = "macos")]
mod native {
    use super::{spec, Cmd, Node, Sc};
    use muda::{accelerator::{Accelerator, Code, Modifiers}, AboutMetadata, CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};
    use std::collections::HashMap;

    /// The Edit menu's items by their ids: what a system panel's text field is sent in their place.
    static EDIT: std::sync::OnceLock<HashMap<MenuId, &'static std::ffi::CStr>> = std::sync::OnceLock::new();
    /// A menu command arrived while a system panel (Save, Open) is up. The Edit menu's keys are the panel's then - sent
    /// to whatever field has the keys there, as the system's own Edit menu would - and every other command is dropped:
    /// the window behind a panel does nothing. True: the event is dealt with.
    pub fn panel_takes(id: &MenuId) -> bool {
        use objc2::{class, msg_send, runtime::{AnyObject, Sel}};
        unsafe {
            let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication]; if app.is_null() { return false }
            let modal: *mut AnyObject = msg_send![app, modalWindow]; if modal.is_null() { return false }
            if let Some(name) = EDIT.get().and_then(|m| m.get(id)) { let sel = Sel::register(name); let nil: *mut AnyObject = std::ptr::null_mut(); let _: bool = msg_send![app, sendAction: sel, to: nil, from: nil]; }
            true
        }
    }

    pub struct AppMenu { pub menu: Menu, ids: HashMap<MenuId, Cmd>, recent: Submenu, seen: Option<Vec<String>>, checks: Vec<(Cmd, CheckMenuItem)> }

    fn accel(s: Sc) -> Option<Accelerator> {
        let code = match s.key { 'a' => Code::KeyA, 'b' => Code::KeyB, 'c' => Code::KeyC, 'e' => Code::KeyE, 'h' => Code::KeyH, 'i' => Code::KeyI, 'n' => Code::KeyN, 'o' => Code::KeyO, 'q' => Code::KeyQ, 's' => Code::KeyS, 'u' => Code::KeyU, 'v' => Code::KeyV, 'w' => Code::KeyW, 'x' => Code::KeyX, 'z' => Code::KeyZ, '1' => Code::Digit1, '2' => Code::Digit2, '3' => Code::Digit3, '\\' => Code::Backslash, _ => return None };
        let mut m = Modifiers::META; if s.shift { m |= Modifiers::SHIFT; } if s.alt { m |= Modifiers::ALT; }
        Some(Accelerator::new(m, code))
    }

    impl AppMenu {
        pub fn new() -> AppMenu {
            let mut ids = HashMap::new();
            let menu = Menu::new();
            let app = Submenu::new("minNotes", true);
            // ours, not the predefined Quit: closing asks about changes that are not in the document first
            let quit = MenuItem::new("Quit minNotes", true, accel(Sc { shift: false, alt: false, key: 'q' }));
            ids.insert(quit.id().clone(), Cmd::Quit);
            let about = AboutMetadata { name: Some("minNotes".into()), version: Some(env!("CARGO_PKG_VERSION").into()), ..Default::default() };
            // the updater's check (Sparkle shows its own window) and the third-party notices that ship with the application (row 348)
            let (updates, licenses) = (MenuItem::new("Check for Updates\u{2026}", true, None), MenuItem::new("Licenses\u{2026}", true, None));
            ids.insert(updates.id().clone(), Cmd::CheckForUpdates); ids.insert(licenses.id().clone(), Cmd::Licenses);
            let _ = app.append_items(&[&PredefinedMenuItem::about(Some("About minNotes"), Some(about)), &updates, &licenses, &PredefinedMenuItem::separator(), &PredefinedMenuItem::services(None), &PredefinedMenuItem::separator(), &PredefinedMenuItem::hide(None), &PredefinedMenuItem::hide_others(None), &PredefinedMenuItem::show_all(None), &PredefinedMenuItem::separator(), &quit]);
            let _ = menu.append(&app);
            let mut recent = None; let mut checks = vec![];
            for (title, nodes) in spec() {
                let sub = Submenu::new(title, true);
                for n in nodes { match n {
                    Node::Item { label, sc, cmd } => { let i = MenuItem::new(label, true, sc.and_then(accel)); ids.insert(i.id().clone(), cmd); let _ = sub.append(&i); }
                    Node::Check { label, cmd } => { let i = CheckMenuItem::new(label, true, false, None); ids.insert(i.id().clone(), cmd.clone()); let _ = sub.append(&i); checks.push((cmd, i)); }
                    Node::CheckKey { label, sc, cmd } => { let i = CheckMenuItem::new(label, true, false, sc.and_then(accel)); ids.insert(i.id().clone(), cmd.clone()); let _ = sub.append(&i); checks.push((cmd, i)); }
                    Node::Recent => { let s = Submenu::new("Open Recent", true); let _ = sub.append(&s); recent = Some(s); }
                    Node::Sep => { let _ = sub.append(&PredefinedMenuItem::separator()); }
                } }
                let _ = menu.append(&sub);
            }
            let _ = EDIT.set(ids.iter().filter_map(|(id, c)| Some((id.clone(), match c { Cmd::Cut => c"cut:", Cmd::Copy => c"copy:", Cmd::Paste | Cmd::PastePlain => c"paste:", Cmd::SelectAll => c"selectAll:", Cmd::Undo => c"undo:", Cmd::Redo => c"redo:", _ => return None }))).collect());
            let mut m = AppMenu { menu, ids, recent: recent.expect("the File menu has Open Recent"), seen: None, checks };
            m.set_recent(&[]); m
        }
        /// AppKit adds tab-bar and full-screen items to any menu titled "View"; minNotes has neither yet.
        pub fn quiet_appkit_extras() {
            use objc2_foundation::{NSString, NSUserDefaults};
            let mtm = objc2::MainThreadMarker::new().expect("main thread");
            objc2_app_kit::NSWindow::setAllowsAutomaticWindowTabbing(false, mtm);
            NSUserDefaults::standardUserDefaults().setBool_forKey(false, &NSString::from_str("NSFullScreenMenuItemEverywhere"));
        }
        /// An option's tick (the system flips it on a click: it is set to what the option now is).
        pub fn set_check(&self, cmd: &Cmd, on: bool) { for (c, i) in &self.checks { if c == cmd { i.set_checked(on); } } }
        pub fn cmd(&self, id: &MenuId) -> Option<Cmd> { self.ids.get(id).cloned() }
        /// Open Recent: the collections' file names, "(none)" when empty, then Clear Menu.
        pub fn set_recent(&mut self, list: &[String]) {
            if self.seen.as_deref() == Some(list) { return }
            self.seen = Some(list.to_vec());
            while self.recent.remove_at(0).is_some() {}
            if list.is_empty() { let _ = self.recent.append(&MenuItem::new("(none)", false, None)); return }
            for path in list { let name = path.rsplit(['/', '\\']).next().unwrap_or(path); let i = MenuItem::new(name, true, None); self.ids.insert(i.id().clone(), Cmd::OpenRecent(path.clone())); let _ = self.recent.append(&i); }
            let _ = self.recent.append(&PredefinedMenuItem::separator());
            let c = MenuItem::new("Clear Menu", true, None); self.ids.insert(c.id().clone(), Cmd::ClearRecent); let _ = self.recent.append(&c);
        }
    }
}
