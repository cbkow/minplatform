//! The menus (canon row 450): seven of them - File, Edit, View, Go, Project, Window, Help - native on macOS, drawn in
//! the window on Windows (as minNotes'), and each system's own keys. One table ([`spec`]) feeds both menus and the
//! key map, so that a key and the menu row that shows it cannot drift apart.
//!
//! What is built is enabled and wired; everything else the rulings name (copy, cut, paste, redo, Get Info, the Project
//! items) is THERE and disabled, so that the menus already read as they will.
use crate::model::SortBy;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cmd { Sequences, Preview, Copy, Cut, Paste, PasteMove, CopyToOther, MoveToOther, NewFolder, Rename, Duplicate, Trash, DeleteNow, Undo, NewWindow, NewTab, CloseTab, NextTab, PrevTab, NewJob, Subscribe, Column(u8), CloseWindow, Open, SelectAll, AsList, AsGrid, Hidden, FoldersFirst, SecondPane, Sort(u8), Refresh, Back, Forward, Up, Home, Desktop, Downloads, GoToFolder, Find, Minimize, Quit, Settings, Redo }
#[cfg_attr(not(any(target_os = "macos", test)), allow(dead_code))]   // the ids are the native menu's (macOS)
impl Cmd {
    const ALL: [Cmd; 49] = [Cmd::Settings, Cmd::Redo, Cmd::Sequences, Cmd::Preview, Cmd::Copy, Cmd::Cut, Cmd::Paste, Cmd::PasteMove, Cmd::CopyToOther, Cmd::MoveToOther, Cmd::NewFolder, Cmd::Rename, Cmd::Duplicate, Cmd::Trash, Cmd::DeleteNow, Cmd::Undo, Cmd::NewWindow, Cmd::NewTab, Cmd::CloseTab, Cmd::NextTab, Cmd::PrevTab, Cmd::NewJob, Cmd::Subscribe, Cmd::Column(1), Cmd::Column(2), Cmd::Column(3), Cmd::CloseWindow, Cmd::Open, Cmd::SelectAll, Cmd::AsList, Cmd::AsGrid, Cmd::Hidden, Cmd::FoldersFirst, Cmd::SecondPane, Cmd::Sort(0), Cmd::Sort(1), Cmd::Sort(2), Cmd::Sort(3), Cmd::Refresh, Cmd::Back, Cmd::Forward, Cmd::Up, Cmd::Home, Cmd::Desktop, Cmd::Downloads, Cmd::GoToFolder, Cmd::Find, Cmd::Minimize, Cmd::Quit];
    pub fn id(self) -> String { format!("{self:?}") }
    pub fn from_id(id: &str) -> Option<Cmd> { Cmd::ALL.into_iter().find(|c| c.id() == id) }
}

/// A key: a name ("N", "[", "Up", "Enter", "F5", "Delete", ".") and its modifiers. `cmd` is the command key on macOS,
/// Ctrl on Windows; `alt` is Option on macOS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sc { pub cmd: bool, pub shift: bool, pub alt: bool, pub key: &'static str }
const fn cmd(key: &'static str) -> Option<Sc> { Some(Sc { cmd: true, shift: false, alt: false, key }) }
const fn cmd_shift(key: &'static str) -> Option<Sc> { Some(Sc { cmd: true, shift: true, alt: false, key }) }
const fn cmd_alt(key: &'static str) -> Option<Sc> { Some(Sc { cmd: true, shift: false, alt: true, key }) }
const fn alt(key: &'static str) -> Option<Sc> { Some(Sc { cmd: false, shift: false, alt: true, key }) }
const fn bare(key: &'static str) -> Option<Sc> { Some(Sc { cmd: false, shift: false, alt: false, key }) }
impl Sc {
    /// As a menu row and a tooltip show it: `Ctrl+Shift+N` on Windows; on macOS the command-key glyphs.
    pub fn label(&self, mac: bool) -> String {
        let key = match self.key { "Up" if mac => "\u{2191}", "Down" if mac => "\u{2193}", "Left" if mac => "\u{2190}", "Right" if mac => "\u{2192}", "Delete" if mac => "\u{232b}", "Enter" if mac => "Return", k => k };
        if mac { format!("{}{}{}{key}", if self.alt { "\u{2325}" } else { "" }, if self.shift { "\u{21e7}" } else { "" }, if self.cmd { "\u{2318}" } else { "" }) }
        else { format!("{}{}{}{key}", if self.cmd { "Ctrl+" } else { "" }, if self.alt { "Alt+" } else { "" }, if self.shift { "Shift+" } else { "" }) }
    }
}

pub enum Node {
    /// `cmd`: none - the row is there and disabled (not this slice's).
    Item { label: &'static str, sc: Option<Sc>, cmd: Option<Cmd> },
    /// A row with a tick.
    Check { label: &'static str, sc: Option<Sc>, cmd: Cmd },
    Sub { label: &'static str, items: Vec<Node> },
    Sep,
}
fn item(label: &'static str, sc: Option<Sc>, cmd: Cmd) -> Node { Node::Item { label, sc, cmd: Some(cmd) } }
/// A row of a later slice: shown, with its key, disabled.
fn later(label: &'static str, sc: Option<Sc>) -> Node { Node::Item { label, sc, cmd: None } }
fn check(label: &'static str, sc: Option<Sc>, cmd: Cmd) -> Node { Node::Check { label, sc, cmd } }

/// The seven menus. `mac`: macOS's wording and keys, else Windows'.
pub fn spec(mac: bool) -> Vec<(&'static str, Vec<Node>)> {
    let m = mac;
    vec![
        ("File", vec![
            item("New Window", cmd("N"), Cmd::NewWindow), item("New Tab", cmd("T"), Cmd::NewTab), item("New Folder", cmd_shift("N"), Cmd::NewFolder), Node::Sep,
            item("Open", if m { cmd("O") } else { bare("Enter") }, Cmd::Open), later("Open With", None), Node::Sep,
            item("Rename", if m { bare("Enter") } else { bare("F2") }, Cmd::Rename), item("Duplicate", if m { cmd("D") } else { None }, Cmd::Duplicate), later(if m { "Get Info" } else { "Properties" }, if m { cmd("I") } else { alt("Enter") }),
            item(if m { "Move to Trash" } else { "Delete" }, if m { cmd("Delete") } else { bare("Delete") }, Cmd::Trash),
            // (for good: it always asks first)
            item(if m { "Delete Immediately\u{2026}" } else { "Delete Permanently\u{2026}" }, if m { cmd_alt("Delete") } else { Some(Sc { cmd: false, shift: true, alt: false, key: "Delete" }) }, Cmd::DeleteNow), Node::Sep,
            // (tabs, row 480: the key closes the TAB in front - the window with its last one, as Finder and Explorer do)
            item("Close Tab", cmd("W"), Cmd::CloseTab), item("Close Window", cmd_shift("W"), Cmd::CloseWindow),
        ]),
        ("Edit", {
            // (each system's own habit, row 450: Explorer cuts and pastes; Finder copies, and moves with "Move Item Here")
            let mut v = vec![item("Undo", cmd("Z"), Cmd::Undo), item("Redo", if m { cmd_shift("Z") } else { cmd("Y") }, Cmd::Redo), Node::Sep,
                if m { later("Cut", cmd("X")) } else { item("Cut", cmd("X"), Cmd::Cut) }, item("Copy", cmd("C"), Cmd::Copy), item("Paste", cmd("V"), Cmd::Paste)];
            if m { v.push(item("Move Item Here", cmd_alt("V"), Cmd::PasteMove)); }
            // (row 450: the two that move files between the panes have no key on purpose - not a thing to do by accident)
            v.extend([Node::Sep, item("Select All", cmd("A"), Cmd::SelectAll), Node::Sep, item("Copy to Other Pane", None, Cmd::CopyToOther), item("Move to Other Pane", None, Cmd::MoveToOther)]);
            v
        }),
        ("View", vec![
            check("as List", cmd("1"), Cmd::AsList), check("as Grid", cmd("2"), Cmd::AsGrid), Node::Sep,
            // (the lightbox, row 452: Space is its key - not written here as a shortcut, because Space is also a letter of a name being typed to find it)
            item("Preview", None, Cmd::Preview), Node::Sep,
            check("Show Hidden Files", if m { cmd_shift(".") } else { cmd("H") }, Cmd::Hidden), check("Folders First", None, Cmd::FoldersFirst),
            // (row 475: an image sequence as ONE entry, or every file by itself)
            check("Group Sequences", None, Cmd::Sequences),
            Node::Sub { label: "Sort By", items: vec![check("Name", None, Cmd::Sort(0)), check("Date Modified", None, Cmd::Sort(1)), check("Size", None, Cmd::Sort(2)), check("Kind", None, Cmd::Sort(3))] },
            // (row 488: which columns a list shows - for the kind of panel the active pane is)
            Node::Sub { label: "Columns", items: vec![check("Date Modified", None, Cmd::Column(1)), check("Size", None, Cmd::Column(2)), check("Kind", None, Cmd::Column(3))] }, Node::Sep,
            check("Show Second Pane", None, Cmd::SecondPane), Node::Sep,
            item("Refresh", if m { cmd("R") } else { bare("F5") }, Cmd::Refresh),
        ]),
        ("Go", vec![
            item("Back", if m { cmd("[") } else { alt("Left") }, Cmd::Back), item("Forward", if m { cmd("]") } else { alt("Right") }, Cmd::Forward), item("Enclosing Folder", if m { cmd("Up") } else { alt("Up") }, Cmd::Up), Node::Sep,
            item("Home", if m { cmd_shift("H") } else { None }, Cmd::Home), item("Desktop", if m { cmd_shift("D") } else { None }, Cmd::Desktop), item("Downloads", if m { cmd_alt("L") } else { None }, Cmd::Downloads), Node::Sep,
            item("Go to Folder\u{2026}", if m { cmd_shift("G") } else { cmd("L") }, Cmd::GoToFolder), item("Find in This Folder", cmd("F"), Cmd::Find),
        ]),
        ("Project", vec![item("New Job\u{2026}", None, Cmd::NewJob), item("Subscribe to Chosen Folder", None, Cmd::Subscribe), Node::Sep, later("Track Selected Items", None), later("Open in Tracker", None), Node::Sep, later("Project Types\u{2026}", None)]),
        ("Window", vec![item("Minimize", if m { cmd("M") } else { None }, Cmd::Minimize), Node::Sep,
            // (macOS also takes Ctrl+Tab and Ctrl+Shift+Tab: `extra` cannot say it - the Ctrl key is not the command key there - so the window does)
            item("Show Next Tab", if m { cmd_shift("]") } else { cmd("Tab") }, Cmd::NextTab), item("Show Previous Tab", if m { cmd_shift("[") } else { cmd_shift("Tab") }, Cmd::PrevTab)]),
        ("Help", vec![later("UFB Help", None)]),
    ]
}

/// Keys that have no menu row of their own (or a second key for one that has). `mac` as in [`spec`].
fn extra(mac: bool) -> Vec<(Sc, Cmd)> {
    let k = |sc: Option<Sc>, c: Cmd| (sc.unwrap(), c);
    if mac { vec![k(cmd("Down"), Cmd::Open)] }
    else { vec![k(bare("Backspace"), Cmd::Back), k(bare("F4"), Cmd::GoToFolder), k(cmd("R"), Cmd::Refresh), k(cmd_shift("."), Cmd::Hidden)] }
}
fn walk<'a>(nodes: &'a [Node], f: &mut dyn FnMut(&'a Node)) { for n in nodes { f(n); if let Node::Sub { items, .. } = n { walk(items, f); } } }
/// The command a key gives (only rows that are enabled have one).
pub fn shortcut(mac: bool, key: &str, cmd: bool, shift: bool, alt: bool) -> Option<Cmd> {
    let hit = |s: &Sc| s.key.eq_ignore_ascii_case(key) && s.cmd == cmd && s.shift == shift && s.alt == alt;
    let mut found = None;
    for (_, nodes) in spec(mac) { walk(&nodes, &mut |n| match n { Node::Item { sc: Some(s), cmd: Some(c), .. } | Node::Check { sc: Some(s), cmd: c, .. } if found.is_none() && hit(s) => found = Some(*c), _ => {} }); }
    found.or_else(|| extra(mac).into_iter().find(|(s, _)| hit(s)).map(|(_, c)| c))
}
/// The key of a command, as a tooltip shows it.
pub fn key_label(mac: bool, c: Cmd) -> Option<String> {
    let mut found = None;
    for (_, nodes) in spec(mac) { walk(&nodes, &mut |n| match n { Node::Item { sc: Some(s), cmd: Some(k), .. } | Node::Check { sc: Some(s), cmd: k, .. } if found.is_none() && *k == c => found = Some(s.label(mac)), _ => {} }); }
    found
}
/// "Back" + its key, for a tooltip.
pub fn tip(text: &str, c: Cmd) -> String { match key_label(cfg!(target_os = "macos"), c) { Some(k) => format!("{text}  {k}"), None => text.to_string() } }

/// What the menus show of the window in front: the ticks, and which rows can act.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct State { pub grid: bool, pub hidden: bool, pub folders_first: bool, pub second: bool, pub sort: SortBy, pub can_back: bool, pub can_forward: bool, pub can_up: bool,
    /// how many entries are chosen in the active pane (2: more than one); something can be undone; things may be changed in this folder
    pub chosen: u8, pub undo: bool, pub redo: bool, pub writable: bool,
    /// a text is being typed (copy and paste are the field's); the second pane is shown and things may be put into its folder
    pub typing: bool, pub other: bool, /// image sequences are shown as one entry each
    pub sequences: bool, /// the folder in view is a jobs folder (New Job makes jobs there)
    pub jobs_folder: bool, /// the columns the active pane's list shows (by number: 1 the date, 2 the size, 3 the kind)
    pub columns: [bool; 4] }
impl State {
    fn checked(&self, c: Cmd) -> bool { match c { Cmd::AsList => !self.grid, Cmd::AsGrid => self.grid, Cmd::Hidden => self.hidden, Cmd::FoldersFirst => self.folders_first, Cmd::SecondPane => self.second, Cmd::Sequences => self.sequences, Cmd::Sort(i) => self.sort.column() == i as usize, Cmd::Column(i) => self.columns.get(i as usize).copied().unwrap_or(false), _ => false } }
    fn enabled(&self, c: Cmd) -> bool { match c { Cmd::Back => self.can_back, Cmd::Forward => self.can_forward, Cmd::Up => self.can_up,
        Cmd::NewFolder => self.writable, Cmd::NewJob => self.jobs_folder && self.writable && !self.typing, Cmd::Subscribe => self.chosen == 1 && !self.typing, Cmd::Rename => self.writable && self.chosen >= 1, Cmd::Duplicate | Cmd::Trash | Cmd::DeleteNow => self.writable && self.chosen >= 1, Cmd::Undo => self.undo, Cmd::Redo => self.redo, Cmd::Preview => self.chosen >= 1,
        Cmd::Copy => self.typing || self.chosen >= 1, Cmd::Cut => self.typing || (self.writable && self.chosen >= 1), Cmd::Paste => self.typing || self.writable, Cmd::PasteMove => !self.typing && self.writable,
        Cmd::CopyToOther => self.other && self.chosen >= 1, Cmd::MoveToOther => self.other && self.writable && self.chosen >= 1, _ => true } }
}

/// The menu bar drawn in the window (Windows; `UFB_MENU_BAR=1` shows it on macOS, to look at).
pub struct InWindowMenu { pub bar: min_ui::MenuBar, cmds: Vec<Cmd>, state: Option<State> }
impl InWindowMenu {
    pub fn wanted() -> bool { cfg!(not(target_os = "macos")) || std::env::var_os("UFB_MENU_BAR").is_some() }
    pub fn new() -> InWindowMenu { InWindowMenu { bar: min_ui::MenuBar::new(vec![]), cmds: vec![], state: None } }
    pub fn cmd(&self, id: u32) -> Option<Cmd> { self.cmds.get(id as usize).copied() }
    pub fn sync(&mut self, st: State, fonts: &mut min_ui::Fonts) {
        if self.state == Some(st) { return } self.state = Some(st);
        use min_ui::{BarItem, BarMenu};
        fn build(nodes: Vec<Node>, st: &State, cmds: &mut Vec<Cmd>) -> Vec<BarItem> {
            nodes.into_iter().map(|n| match n {
                Node::Item { label, sc, cmd } => { let key = sc.map(|s| s.label(false)).unwrap_or_default(); match cmd { Some(c) => { cmds.push(c); BarItem::Action { id: (cmds.len() - 1) as u32, label: label.into(), enabled: st.enabled(c), shortcut: key, checked: None } } None => BarItem::Action { id: u32::MAX, label: label.into(), enabled: false, shortcut: key, checked: None } } }
                Node::Check { label, sc, cmd } => { cmds.push(cmd); BarItem::Action { id: (cmds.len() - 1) as u32, label: label.into(), enabled: true, shortcut: sc.map(|s| s.label(false)).unwrap_or_default(), checked: Some(st.checked(cmd)) } }
                Node::Sub { label, items } => BarItem::Sub { label: label.into(), enabled: true, items: build(items, st, cmds) },
                Node::Sep => BarItem::Separator,
            }).collect()
        }
        let mut cmds = vec![];
        let mut menus: Vec<BarMenu> = spec(false).into_iter().map(|(title, nodes)| BarMenu { title: format!("&{title}"), items: build(nodes, &st, &mut cmds) }).collect();
        if let Some(file) = menus.first_mut() { cmds.push(Cmd::Quit); file.items.push(BarItem::Separator); file.items.push(BarItem::action((cmds.len() - 1) as u32, "E&xit", true, "Alt+F4")); }
        if let Some(edit) = menus.get_mut(1) { edit.items.push(BarItem::Separator); cmds.push(Cmd::Settings); edit.items.push(BarItem::action((cmds.len() - 1) as u32, "Settings\u{2026}", true, "Ctrl+,")); }
        if let Some(help) = menus.last_mut() { help.items.push(BarItem::Separator); help.items.push(BarItem::action(u32::MAX, concat!("UFB ", env!("CARGO_PKG_VERSION")), false, "")); }
        self.cmds = cmds; self.bar.set_menus(menus, fonts);
    }
}

#[cfg(target_os = "macos")]
pub use native::AppMenu;
#[cfg(target_os = "macos")]
mod native {
    use super::{spec, Cmd, Node, Sc, State};
    use muda::{accelerator::{Accelerator, Code, Modifiers}, AboutMetadata, CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};

    /// The application's menu bar. Its rows carry the command's name as their id.
    pub struct AppMenu { pub menu: Menu, checks: Vec<(Cmd, CheckMenuItem)>, items: Vec<(Cmd, MenuItem)>, state: Option<State>, undo: Option<String>, redo: Option<String> }

    /// A key as AppKit's key equivalent. Only keys WITH the command key become one: a bare Return or Delete as a menu's
    /// key would be taken from every text field.
    fn accel(s: Sc) -> Option<Accelerator> {
        if !s.cmd { return None }
        let code = match s.key { "A" => Code::KeyA, "C" => Code::KeyC, "D" => Code::KeyD, "F" => Code::KeyF, "G" => Code::KeyG, "H" => Code::KeyH, "I" => Code::KeyI, "L" => Code::KeyL, "M" => Code::KeyM, "N" => Code::KeyN, "O" => Code::KeyO, "Q" => Code::KeyQ, "R" => Code::KeyR, "T" => Code::KeyT,
            "V" => Code::KeyV, "W" => Code::KeyW, "X" => Code::KeyX, "Y" => Code::KeyY, "Z" => Code::KeyZ, "1" => Code::Digit1, "2" => Code::Digit2, "[" => Code::BracketLeft, "]" => Code::BracketRight, "." => Code::Period, "," => Code::Comma,
            "Up" => Code::ArrowUp, "Down" => Code::ArrowDown, "Left" => Code::ArrowLeft, "Right" => Code::ArrowRight, "Delete" => Code::Backspace, _ => return None };
        let mut m = Modifiers::META; if s.shift { m |= Modifiers::SHIFT; } if s.alt { m |= Modifiers::ALT; }
        Some(Accelerator::new(m, code))
    }
    fn fill(sub: &Submenu, nodes: Vec<Node>, checks: &mut Vec<(Cmd, CheckMenuItem)>, items: &mut Vec<(Cmd, MenuItem)>) {
        for n in nodes { match n {
            Node::Item { label, sc, cmd: Some(c) } => { let i = MenuItem::with_id(c.id(), label, true, sc.and_then(accel)); let _ = sub.append(&i); items.push((c, i)); }
            // (a row of a later slice: its key is shown but, being disabled, does nothing)
            Node::Item { label, sc, cmd: None } => { let _ = sub.append(&MenuItem::new(label, false, sc.and_then(accel))); }
            Node::Check { label, sc, cmd } => { let i = CheckMenuItem::with_id(cmd.id(), label, true, false, sc.and_then(accel)); let _ = sub.append(&i); checks.push((cmd, i)); }
            Node::Sub { label, items: inner } => { let s = Submenu::new(label, true); fill(&s, inner, checks, items); let _ = sub.append(&s); }
            Node::Sep => { let _ = sub.append(&PredefinedMenuItem::separator()); }
        } }
    }
    impl AppMenu {
        pub fn new() -> AppMenu {
            let menu = Menu::new(); let (mut checks, mut items) = (vec![], vec![]);
            let app = Submenu::new("UFB", true);
            let about = AboutMetadata { name: Some("UFB".into()), version: Some(env!("CARGO_PKG_VERSION").into()), ..Default::default() };
            let quit = MenuItem::with_id(Cmd::Quit.id(), "Quit UFB", true, accel(Sc { cmd: true, shift: false, alt: false, key: "Q" }));
            let _ = app.append_items(&[&PredefinedMenuItem::about(Some("About UFB"), Some(about)), &PredefinedMenuItem::separator(), &MenuItem::with_id(Cmd::Settings.id(), "Settings\u{2026}", true, accel(Sc { cmd: true, shift: false, alt: false, key: "," })), &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::services(None), &PredefinedMenuItem::separator(), &PredefinedMenuItem::hide(None), &PredefinedMenuItem::hide_others(None), &PredefinedMenuItem::show_all(None), &PredefinedMenuItem::separator(), &quit]);
            let _ = menu.append(&app);
            for (title, nodes) in spec(true) { let sub = Submenu::new(title, true); fill(&sub, nodes, &mut checks, &mut items); let _ = menu.append(&sub); if title == "Window" { sub.set_as_windows_menu_for_nsapp(); } }
            AppMenu { menu, checks, items, state: None, undo: None, redo: None }
        }
        /// Before the application launches: no "Enter Full Screen" and no window tabs added by AppKit to our menus (as minNotes).
        pub fn quiet_appkit_extras() {
            use objc2_foundation::{NSString, NSUserDefaults};
            let Some(mtm) = objc2::MainThreadMarker::new() else { return };
            objc2_app_kit::NSWindow::setAllowsAutomaticWindowTabbing(false, mtm);
            NSUserDefaults::standardUserDefaults().setBool_forKey(false, &NSString::from_str("NSFullScreenMenuItemEverywhere"));
        }
        /// The ticks and the rows that can act, for the window in front.
        /// The Edit menu names the step Undo would undo ("Undo Rename").
        pub fn set_undo(&mut self, label: Option<String>) {
            if self.undo == label { return }
            if let Some((_, i)) = self.items.iter().find(|(c, _)| *c == Cmd::Undo) { i.set_text(match &label { Some(l) => format!("Undo {l}"), None => "Undo".into() }); }
            self.undo = label;
        }
        /// ... and the one Redo would do again ("Redo Rename").
        pub fn set_redo(&mut self, label: Option<String>) {
            if self.redo == label { return }
            if let Some((_, i)) = self.items.iter().find(|(c, _)| *c == Cmd::Redo) { i.set_text(match &label { Some(l) if !l.is_empty() => format!("Redo {l}"), _ => "Redo".into() }); }
            self.redo = label;
        }
        pub fn sync(&mut self, st: State) {
            if self.state == Some(st) { return } self.state = Some(st);
            for (c, i) in &self.checks { i.set_checked(st.checked(*c)); }
            for (c, i) in &self.items { i.set_enabled(st.enabled(*c)); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn each_system_has_its_own_keys() {
        // macOS (row 450)
        assert_eq!(shortcut(true, "[", true, false, false), Some(Cmd::Back)); assert_eq!(shortcut(true, "]", true, false, false), Some(Cmd::Forward)); assert_eq!(shortcut(true, "Up", true, false, false), Some(Cmd::Up));
        assert_eq!(shortcut(true, "Down", true, false, false), Some(Cmd::Open)); assert_eq!(shortcut(true, "o", true, false, false), Some(Cmd::Open)); assert_eq!(shortcut(true, "g", true, true, false), Some(Cmd::GoToFolder));
        assert_eq!(shortcut(true, ".", true, true, false), Some(Cmd::Hidden)); assert_eq!(shortcut(true, "r", true, false, false), Some(Cmd::Refresh)); assert_eq!(shortcut(true, "Enter", false, false, false), Some(Cmd::Rename), "Return is Rename on macOS");
        assert_eq!(shortcut(true, "Delete", true, false, false), Some(Cmd::Trash)); assert_eq!(shortcut(true, "Delete", true, false, true), Some(Cmd::DeleteNow)); assert_eq!(shortcut(true, "d", true, false, false), Some(Cmd::Duplicate)); assert_eq!(shortcut(true, "Delete", false, false, false), None, "a bare Delete does nothing on macOS");
        assert_eq!(shortcut(true, "Left", false, false, true), None); assert_eq!(shortcut(true, "F5", false, false, false), None);
        // Windows
        assert_eq!(shortcut(false, "Left", false, false, true), Some(Cmd::Back)); assert_eq!(shortcut(false, "Right", false, false, true), Some(Cmd::Forward)); assert_eq!(shortcut(false, "Up", false, false, true), Some(Cmd::Up));
        assert_eq!(shortcut(false, "Backspace", false, false, false), Some(Cmd::Back)); assert_eq!(shortcut(false, "F5", false, false, false), Some(Cmd::Refresh)); assert_eq!(shortcut(false, "r", true, false, false), Some(Cmd::Refresh));
        assert_eq!(shortcut(false, "l", true, false, false), Some(Cmd::GoToFolder)); assert_eq!(shortcut(false, "F4", false, false, false), Some(Cmd::GoToFolder)); assert_eq!(shortcut(false, "Enter", false, false, false), Some(Cmd::Open));
        assert_eq!(shortcut(false, "F2", false, false, false), Some(Cmd::Rename)); assert_eq!(shortcut(false, "Delete", false, false, false), Some(Cmd::Trash)); assert_eq!(shortcut(false, "Delete", false, true, false), Some(Cmd::DeleteNow), "Shift+Delete: for good (it asks)");
        assert_eq!(shortcut(false, "h", true, false, false), Some(Cmd::Hidden)); assert_eq!(shortcut(false, ".", true, true, false), Some(Cmd::Hidden)); assert_eq!(shortcut(false, "[", true, false, false), None);
        // both
        for mac in [true, false] {
            assert_eq!(shortcut(mac, "1", true, false, false), Some(Cmd::AsList)); assert_eq!(shortcut(mac, "2", true, false, false), Some(Cmd::AsGrid)); assert_eq!(shortcut(mac, "n", true, false, false), Some(Cmd::NewWindow));
            assert_eq!(shortcut(mac, "w", true, false, false), Some(Cmd::CloseTab)); assert_eq!(shortcut(mac, "w", true, true, false), Some(Cmd::CloseWindow)); assert_eq!(shortcut(mac, "t", true, false, false), Some(Cmd::NewTab)); assert_eq!(shortcut(mac, "f", true, false, false), Some(Cmd::Find)); assert_eq!(shortcut(mac, "a", true, false, false), Some(Cmd::SelectAll));
            assert_eq!(shortcut(mac, "z", true, false, false), Some(Cmd::Undo)); assert_eq!(shortcut(mac, "n", true, true, false), Some(Cmd::NewFolder));
            assert_eq!(shortcut(mac, "c", true, false, false), Some(Cmd::Copy)); assert_eq!(shortcut(mac, "v", true, false, false), Some(Cmd::Paste));
            // Explorer cuts files; Finder does not (it moves with "Move Item Here")
            assert_eq!(shortcut(mac, "x", true, false, false), if mac { None } else { Some(Cmd::Cut) }); assert_eq!(shortcut(mac, "v", true, false, true), if mac { Some(Cmd::PasteMove) } else { None });
        }
    }
    #[test]
    fn seven_menus_and_the_rows_the_rulings_name() {
        for mac in [true, false] {
            let s = spec(mac); assert_eq!(s.iter().map(|m| m.0).collect::<Vec<_>>(), ["File", "Edit", "View", "Go", "Project", "Window", "Help"]);
            let mut rows: Vec<(&str, bool)> = vec![]; for (_, nodes) in &s { walk(nodes, &mut |n| match n { Node::Item { label, cmd, .. } => rows.push((label, cmd.is_some())), Node::Check { label, .. } => rows.push((label, true)), _ => {} }); }
            let has = |label: &str, on: bool| assert!(rows.contains(&(label, on)), "{label} ({})", if on { "enabled" } else { "disabled" });
            for l in ["New Window", "New Tab", "Close Tab", "Show Next Tab", "Show Previous Tab", "Close Window", "Open", "Select All", "as List", "as Grid", "Show Hidden Files", "Folders First", "Group Sequences", "Show Second Pane", "Name", "Date Modified", "Size", "Kind", "Refresh", "Back", "Forward", "Enclosing Folder", "Home", "Desktop", "Downloads", "Go to Folder\u{2026}", "Minimize", "New Folder", "Rename", "Duplicate", "Undo", "Copy", "Paste", "Copy to Other Pane", "Move to Other Pane", if mac { "Move Item Here" } else { "Cut" }, if mac { "Move to Trash" } else { "Delete" }, if mac { "Delete Immediately\u{2026}" } else { "Delete Permanently\u{2026}" }] { has(l, true); }
            for l in [if mac { "Cut" } else { "Properties" }, if mac { "Get Info" } else { "Properties" }] { has(l, false); } has("Redo", true);
            // the two that move files between the panes have NO key (row 450)
            for (_, nodes) in &s { walk(nodes, &mut |n| if let Node::Item { label, sc, .. } = n { if label.ends_with("Other Pane") { assert!(sc.is_none()); } }); }
        }
        for c in Cmd::ALL { assert_eq!(Cmd::from_id(&c.id()), Some(c)); }
    }
    #[test]
    fn a_key_reads_as_its_system_writes_it() {
        assert_eq!(cmd_shift("N").unwrap().label(false), "Ctrl+Shift+N"); assert_eq!(alt("Left").unwrap().label(false), "Alt+Left"); assert_eq!(bare("F5").unwrap().label(false), "F5"); assert_eq!(Sc { cmd: false, shift: true, alt: false, key: "Delete" }.label(false), "Shift+Delete");
        assert_eq!(cmd_shift("G").unwrap().label(true), "\u{21e7}\u{2318}G"); assert_eq!(cmd("Up").unwrap().label(true), "\u{2318}\u{2191}"); assert_eq!(cmd_alt("L").unwrap().label(true), "\u{2325}\u{2318}L");
        assert_eq!(key_label(true, Cmd::Back).as_deref(), Some("\u{2318}[")); assert_eq!(key_label(false, Cmd::Back).as_deref(), Some("Alt+Left")); assert_eq!(key_label(false, Cmd::SecondPane), None);
        let st = State { grid: true, hidden: false, folders_first: true, second: false, sort: SortBy::Size, can_back: false, can_forward: true, can_up: true, chosen: 1, undo: false, redo: false, writable: true, typing: false, other: false, sequences: true, jobs_folder: false, columns: [true; 4] };
        assert!(st.enabled(Cmd::Copy) && st.enabled(Cmd::Paste) && !st.enabled(Cmd::CopyToOther) && State { other: true, ..st }.enabled(Cmd::MoveToOther) && !State { other: true, writable: false, ..st }.enabled(Cmd::MoveToOther));
        assert!(!State { chosen: 0, ..st }.enabled(Cmd::Copy) && State { chosen: 0, typing: true, ..st }.enabled(Cmd::Copy) && !State { writable: false, ..st }.enabled(Cmd::Paste));
        // what changes files follows the choice, the undo list and whether this folder may be changed
        assert!(st.enabled(Cmd::Rename) && st.enabled(Cmd::Trash) && st.enabled(Cmd::NewFolder) && !st.enabled(Cmd::Undo));
        assert!(State { chosen: 2, ..st }.enabled(Cmd::Rename) && !State { chosen: 0, ..st }.enabled(Cmd::Rename) && State { chosen: 2, ..st }.enabled(Cmd::Duplicate) && !State { chosen: 0, ..st }.enabled(Cmd::Trash) && State { undo: true, ..st }.enabled(Cmd::Undo));
        assert!(!State { writable: false, ..st }.enabled(Cmd::NewFolder) && !State { writable: false, ..st }.enabled(Cmd::DeleteNow));
        assert!(st.checked(Cmd::AsGrid) && !st.checked(Cmd::AsList) && st.checked(Cmd::Sort(2)) && !st.checked(Cmd::Sort(0)) && st.checked(Cmd::FoldersFirst)); assert!(!st.enabled(Cmd::Back) && st.enabled(Cmd::Forward) && st.enabled(Cmd::Refresh));
    }
}
