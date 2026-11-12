//! The menu bar (canon row 296): ONE definition ([`spec`]) of minNotes' menus, as QCView's `menu.rs` has it.
//! macOS: the native menu through `muda` — a shortcut is its item's key equivalent, so AppKit takes the key and the
//! command comes back as a menu event (nothing fires twice). Windows: the in-window bar (`min_ui::MenuBar`, row 194) is
//! to be built from the same `spec` with the Windows build; until then the window's own key handler runs the shortcuts.

#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    NewCollection, Open, OpenRecent(String), ClearRecent, Save, SaveAs, CloseCollection, NewNote, NewFolder, Rename, Quit,
    Undo, Redo, Cut, Copy, Paste, PastePlain, PasteMarkdown, SelectAll,
    Bold, Italic, Underline, Strike, Code, Highlight, ClearFormat,
    Sidebar, Toolbar, ToolbarWide, Footer,
}

/// A shortcut: its key, always with Cmd (macOS) / Ctrl.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sc { pub shift: bool, pub alt: bool, pub key: char }
const fn sc(key: char) -> Option<Sc> { Some(Sc { shift: false, alt: false, key }) }
const fn sc_shift(key: char) -> Option<Sc> { Some(Sc { shift: true, alt: false, key }) }
const fn sc_alt(key: char) -> Option<Sc> { Some(Sc { shift: false, alt: true, key }) }

pub enum Node { Item { label: &'static str, sc: Option<Sc>, cmd: Cmd }, Recent, Sep }
fn item(label: &'static str, sc: Option<Sc>, cmd: Cmd) -> Node { Node::Item { label, sc, cmd } }

/// The menus, left to right.
pub fn spec() -> Vec<(&'static str, Vec<Node>)> {
    vec![
        ("File", vec![
            item("New Collection…", sc_alt('n'), Cmd::NewCollection), item("New Note", sc('n'), Cmd::NewNote), item("New Folder", sc_shift('n'), Cmd::NewFolder), item("Rename", None, Cmd::Rename), Node::Sep,
            item("Open…", sc('o'), Cmd::Open), Node::Recent, Node::Sep,
            item("Save", sc('s'), Cmd::Save), item("Save As…", sc_shift('s'), Cmd::SaveAs), Node::Sep,
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
        ("View", vec![item("Sidebar", sc('1'), Cmd::Sidebar), item("Toolbar", sc('2'), Cmd::Toolbar), item("Footer", sc('3'), Cmd::Footer), Node::Sep, item("Toolbar Over the Page / Across the Window", None, Cmd::ToolbarWide)]),
    ]
}

/// The key a command is run by in the window's key handler: (character, with Shift). None: it has a routine of its own.
pub fn key_of(c: &Cmd) -> Option<(char, bool)> {
    Some(match c { Cmd::NewNote => ('n', false), Cmd::NewFolder => ('n', true), Cmd::Save => ('s', false), Cmd::Undo => ('z', false), Cmd::Redo => ('z', true), Cmd::Cut => ('x', false), Cmd::Copy => ('c', false), Cmd::Paste => ('v', false), Cmd::PastePlain => ('v', true), Cmd::SelectAll => ('a', false),
        Cmd::Bold => ('b', false), Cmd::Italic => ('i', false), Cmd::Underline => ('u', false), Cmd::Strike => ('x', true), Cmd::Code => ('e', false), Cmd::Highlight => ('h', true), Cmd::ClearFormat => ('\\', false), Cmd::Sidebar => ('1', false), _ => return None })
}

#[cfg(target_os = "macos")]
pub use native::AppMenu;

#[cfg(target_os = "macos")]
mod native {
    use super::{spec, Cmd, Node, Sc};
    use muda::{accelerator::{Accelerator, Code, Modifiers}, AboutMetadata, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};
    use std::collections::HashMap;

    pub struct AppMenu { pub menu: Menu, ids: HashMap<MenuId, Cmd>, recent: Submenu, seen: Option<Vec<String>> }

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
            let _ = app.append_items(&[&PredefinedMenuItem::about(Some("About minNotes"), Some(about)), &PredefinedMenuItem::separator(), &PredefinedMenuItem::services(None), &PredefinedMenuItem::separator(), &PredefinedMenuItem::hide(None), &PredefinedMenuItem::hide_others(None), &PredefinedMenuItem::show_all(None), &PredefinedMenuItem::separator(), &quit]);
            let _ = menu.append(&app);
            let mut recent = None;
            for (title, nodes) in spec() {
                let sub = Submenu::new(title, true);
                for n in nodes { match n {
                    Node::Item { label, sc, cmd } => { let i = MenuItem::new(label, true, sc.and_then(accel)); ids.insert(i.id().clone(), cmd); let _ = sub.append(&i); }
                    Node::Recent => { let s = Submenu::new("Open Recent", true); let _ = sub.append(&s); recent = Some(s); }
                    Node::Sep => { let _ = sub.append(&PredefinedMenuItem::separator()); }
                } }
                let _ = menu.append(&sub);
            }
            let mut m = AppMenu { menu, ids, recent: recent.expect("the File menu has Open Recent"), seen: None };
            m.set_recent(&[]); m
        }
        /// AppKit adds tab-bar and full-screen items to any menu titled "View"; minNotes has neither yet.
        pub fn quiet_appkit_extras() {
            use objc2_foundation::{NSString, NSUserDefaults};
            let mtm = objc2::MainThreadMarker::new().expect("main thread");
            objc2_app_kit::NSWindow::setAllowsAutomaticWindowTabbing(false, mtm);
            NSUserDefaults::standardUserDefaults().setBool_forKey(false, &NSString::from_str("NSFullScreenMenuItemEverywhere"));
        }
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
