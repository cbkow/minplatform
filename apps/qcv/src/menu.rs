//! The menu bar — QCView's `Main.qml` MenuBar (File / View / Tools / About) carried item for item (canon row 143), from ONE
//! definition ([`spec`]) feeding two front ends:
//! - macOS: the native NSMenu through `muda` ([`AppMenu`]; the app menu gains About / Hide / Quit as macOS expects). The
//!   modified shortcuts are the menu's key equivalents (AppKit swallows the key, so nothing fires twice).
//! - Windows: QCView's IN-WINDOW bar ([`InWindowMenu`] over `min_ui::MenuBar`, canon row 194): QCView drew its Windows bar
//!   in Qt Quick and pinned it dark with its own colours; a native Win32 menu follows the OS scheme and system colours
//!   (the user 2026-11-08: "an always dark mode menu for windows with the deliberate coloring from like the old app").
//!   The shortcuts there are shown only; the shell's key handler runs them. `QCV_MENU_BAR=1` shows it on macOS too.
//!
//! Items whose feature has not landed (streams, the host bridges, the colour panel, notes, safety guides, the shortcut
//! sheet) are present and disabled — the menu's shape is final (row 175's rule for Settings).
//! DEVIATION (macOS only): About = the platform's about panel; Windows shows QCView's own About modal.

#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    OpenMedia, NewProject, OpenProject, SaveProject, SaveProjectAs, RecentMedia(String), ClearRecentMedia, RecentProject(String), ClearRecentProjects, Quit,
    Minimal, Compact, Pin, AllPanels, ZoomIn, ZoomOut, Fullscreen, DefaultFrame, LeftRail, RightRail, StatusBar,
    Background, Settings, Docs, CopyProjectLink, About,
}

/// A shortcut's key (always with Cmd / Ctrl).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum K { O, S, Q, C, T, D0, D1, D2, D3, D4, D5, D9, Equal, Minus, Slash, Comma }
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sc { pub shift: bool, pub alt: bool, pub k: K }
const fn sc(k: K) -> Option<Sc> { Some(Sc { shift: false, alt: false, k }) }
const fn sc_shift(k: K) -> Option<Sc> { Some(Sc { shift: true, alt: false, k }) }
const fn sc_alt(k: K) -> Option<Sc> { Some(Sc { shift: false, alt: true, k }) }
impl Sc {
    /// Windows' spelling: "Ctrl+Shift+O".
    pub fn label(&self) -> String {
        let k = match self.k { K::O => "O", K::S => "S", K::Q => "Q", K::C => "C", K::T => "T", K::D0 => "0", K::D1 => "1", K::D2 => "2", K::D3 => "3", K::D4 => "4", K::D5 => "5", K::D9 => "9", K::Equal => "=", K::Minus => "-", K::Slash => "/", K::Comma => "," };
        format!("Ctrl+{}{}{k}", if self.shift { "Shift+" } else { "" }, if self.alt { "Alt+" } else { "" })
    }
}

pub enum Node {
    Item { label: &'static str, enabled: bool, sc: Option<Sc>, cmd: Option<Cmd> },
    /// Checkable (QCView: Compact Mode and Always on Top only; the panel toggles are deliberately not).
    Check { label: &'static str, sc: Option<Sc>, cmd: Cmd },
    /// QCView's Open Recent Media / Project submenus, filled from the store.
    Recent { projects: bool },
    Sep,
}
/// QCView's catalog context for its menus (Main.qml; canon row 195).
pub fn tr(s: &'static str) -> &'static str { min_i18n::trc("Main", s) }
fn item(label: &'static str, sc: Option<Sc>, cmd: Cmd) -> Node { Node::Item { label: tr(label), enabled: true, sc, cmd: Some(cmd) } }
fn off(label: &'static str, sc: Option<Sc>) -> Node { Node::Item { label: tr(label), enabled: false, sc, cmd: None } }

/// The menus, left to right — the one definition both front ends build from.
pub fn spec() -> Vec<(&'static str, Vec<Node>)> {
    let mac = cfg!(target_os = "macos");
    let mut file = vec![
        item("Open Media…", sc(K::O), Cmd::OpenMedia), off("Open Stream…", None),
        off("Connect to After Effects", None), off("Connect to Premiere Pro", None), off("Connect to Blender", None),
        Node::Recent { projects: false }, Node::Sep,
        item("New Project", None, Cmd::NewProject), item("Open Project…", sc_shift(K::O), Cmd::OpenProject), Node::Recent { projects: true },
        item("Save Project", sc(K::S), Cmd::SaveProject), item("Save Project As…", sc_shift(K::S), Cmd::SaveProjectAs),
        Node::Sep, item("Copy Project Link", None, Cmd::CopyProjectLink),
    ];
    if !mac { file.extend([Node::Sep, item("E&xit", sc(K::Q), Cmd::Quit)]); }
    let view = vec![
        item("Minimal Mode", sc(K::D0), Cmd::Minimal),
        Node::Check { label: tr("Compact Mode"), sc: sc_shift(K::C), cmd: Cmd::Compact },
        Node::Check { label: tr("Always on Top"), sc: sc_alt(K::T), cmd: Cmd::Pin },
        item("Show All Panels", sc(K::D9), Cmd::AllPanels),
        item("Zoom In Timeline", sc(K::Equal), Cmd::ZoomIn), item("Zoom Out Timeline", sc(K::Minus), Cmd::ZoomOut),
        item("Fullscreen", None, Cmd::Fullscreen),   // QCView: no menu shortcut (F belongs to the viewport)
        item("Default Size && Position", None, Cmd::DefaultFrame),
        Node::Sep,
        item("Left Rail", sc(K::D1), Cmd::LeftRail), item("Right Rail", sc(K::D2), Cmd::RightRail),
        off("Color Panel", sc(K::D3)), off("Timeline", None), off("Notes", sc(K::D4)),
        item("Status Bar", sc(K::D5), Cmd::StatusBar),
    ];
    let tools = vec![item("Background", None, Cmd::Background), off("Safety Guides", None), off("Keyboard Shortcuts", sc(K::Slash)), item("Settings…", sc(K::Comma), Cmd::Settings)];
    // Check for Updates waits for an updater: hidden, as QCView hides it off macOS; on macOS About lives in the app menu
    let mut about = vec![];
    if !mac { about.extend([item("About QCView…", None, Cmd::About), Node::Sep]); }
    about.push(item("Docs", None, Cmd::Docs));
    vec![(tr("&File"), file), (tr("&View"), view), (tr("&Tools"), tools), (tr("&About"), about)]
}

/// The store's recent lists → (label, path) rows: the file names (both separators split).
fn recent_rows(list: &[String]) -> Vec<(String, String)> { list.iter().map(|p| (p.rsplit(['/', '\\']).next().unwrap_or(p).to_string(), p.clone())).collect() }

// ---------------------------------------------------------------------------------------------------------------- Windows

/// QCView's in-window bar: `spec` → `min_ui::MenuBar`, rebuilt when a Recent list or a checkmark changes.
pub struct InWindowMenu { pub bar: min_ui::MenuBar, cmds: Vec<Cmd>, state: (Vec<String>, Vec<String>, bool, bool) }

impl InWindowMenu {
    /// Windows always; macOS with `QCV_MENU_BAR=1` (a look at the bar without a Windows box).
    pub fn wanted() -> bool { cfg!(not(target_os = "macos")) || std::env::var_os("QCV_MENU_BAR").is_some() }
    pub fn new(fonts: &mut min_ui::Fonts) -> InWindowMenu {
        let mut m = InWindowMenu { bar: min_ui::MenuBar::new(vec![]), cmds: vec![], state: (vec![String::new()], vec![], false, false) };
        m.update(&[], &[], false, false, fonts); m
    }
    pub fn cmd(&self, id: u32) -> Option<Cmd> { self.cmds.get(id as usize).cloned() }
    /// The live parts: the two Recent submenus and the two checkmarks (QCView binds `checked`).
    pub fn update(&mut self, media: &[String], projects: &[String], compact: bool, pin: bool, fonts: &mut min_ui::Fonts) {
        let st = (media.to_vec(), projects.to_vec(), compact, pin);
        if st == self.state { return }
        self.state = st;
        use min_ui::{BarItem, BarMenu};
        let mut cmds: Vec<Cmd> = vec![];
        let mut id = |c: Cmd| { cmds.push(c); (cmds.len() - 1) as u32 };
        let menus = spec().into_iter().map(|(title, nodes)| BarMenu { title: title.into(), items: nodes.into_iter().map(|n| match n {
            Node::Item { label, enabled, sc, cmd } => BarItem::Action { id: cmd.map(&mut id).unwrap_or(u32::MAX), label: label.into(), enabled, shortcut: sc.map(|s| s.label()).unwrap_or_default(), checked: None },
            Node::Check { label, sc, cmd } => { let on = match cmd { Cmd::Compact => compact, Cmd::Pin => pin, _ => false }; BarItem::Action { id: id(cmd), label: label.into(), enabled: true, shortcut: sc.map(|s| s.label()).unwrap_or_default(), checked: Some(on) } }
            Node::Recent { projects: pj } => {
                let list = if pj { projects } else { media };
                let mut items: Vec<BarItem> = recent_rows(list).into_iter().map(|(name, path)| BarItem::Action { id: id(if pj { Cmd::RecentProject(path) } else { Cmd::RecentMedia(path) }), label: name.replace('&', "&&"), enabled: true, shortcut: String::new(), checked: None }).collect();
                if items.is_empty() { items.push(BarItem::action(u32::MAX, tr("(none)"), false, "")); } else { items.push(BarItem::Separator); items.push(BarItem::action(id(if pj { Cmd::ClearRecentProjects } else { Cmd::ClearRecentMedia }), tr("Clear Menu"), true, "")); }
                BarItem::Sub { label: tr(if pj { "Open Recent Project" } else { "Open Recent Media" }).into(), enabled: true, items }
            }
            Node::Sep => BarItem::Separator,
        }).collect() }).collect();
        self.cmds = cmds;
        self.bar.set_menus(menus, fonts);
    }
}

// ---------------------------------------------------------------------------------------------------------------- macOS

#[cfg(target_os = "macos")]
pub use native::AppMenu;

#[cfg(target_os = "macos")]
mod native {
    use super::{recent_rows, spec, tr, Cmd, Node, Sc, K};
    use muda::{accelerator::{Accelerator, Code, Modifiers}, AboutMetadata, CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};
    use std::collections::HashMap;

    pub struct AppMenu {
        pub menu: Menu,
        ids: HashMap<MenuId, Cmd>,
        recent_media: Submenu, recent_projects: Submenu,
        compact: CheckMenuItem, pin: CheckMenuItem,
        recents_seen: (Vec<String>, Vec<String>),
    }

    fn accel(s: Sc) -> Accelerator {
        let code = match s.k { K::O => Code::KeyO, K::S => Code::KeyS, K::Q => Code::KeyQ, K::C => Code::KeyC, K::T => Code::KeyT, K::D0 => Code::Digit0, K::D1 => Code::Digit1, K::D2 => Code::Digit2, K::D3 => Code::Digit3, K::D4 => Code::Digit4, K::D5 => Code::Digit5, K::D9 => Code::Digit9, K::Equal => Code::Equal, K::Minus => Code::Minus, K::Slash => Code::Slash, K::Comma => Code::Comma };
        let mut m = Modifiers::META; if s.shift { m |= Modifiers::SHIFT; } if s.alt { m |= Modifiers::ALT; }
        Accelerator::new(m, code)
    }

    impl AppMenu {
        pub fn new() -> AppMenu {
            let mut ids = HashMap::new();
            let menu = Menu::new();
            let app = Submenu::new("qcv", true);
            let quit = MenuItem::new("Quit qcv", true, Some(accel(Sc { shift: false, alt: false, k: K::Q })));   // ours, not the predefined Quit: the settings + window frame save first
            ids.insert(quit.id().clone(), Cmd::Quit);
            let _ = app.append_items(&[&PredefinedMenuItem::about(Some(tr("About QCView…")), Some(about())), &PredefinedMenuItem::separator(), &PredefinedMenuItem::services(None), &PredefinedMenuItem::separator(), &PredefinedMenuItem::hide(None), &PredefinedMenuItem::hide_others(None), &PredefinedMenuItem::show_all(None), &PredefinedMenuItem::separator(), &quit]);
            let _ = menu.append(&app);
            let (mut recent_media, mut recent_projects, mut compact, mut pin) = (None, None, None, None);
            for (title, nodes) in spec() {
                let sub = Submenu::new(title, true);
                for n in nodes { match n {
                    Node::Item { label, enabled, sc, cmd } => { let i = MenuItem::new(label, enabled, sc.map(accel)); if let Some(c) = cmd { ids.insert(i.id().clone(), c); } let _ = sub.append(&i); }
                    Node::Check { label, sc, cmd } => { let i = CheckMenuItem::new(label, true, false, sc.map(accel)); ids.insert(i.id().clone(), cmd.clone()); let _ = sub.append(&i); if cmd == Cmd::Compact { compact = Some(i) } else { pin = Some(i) } }
                    Node::Recent { projects } => { let s = Submenu::new(tr(if projects { "Open Recent Project" } else { "Open Recent Media" }), true); let _ = sub.append(&s); if projects { recent_projects = Some(s) } else { recent_media = Some(s) } }
                    Node::Sep => { let _ = sub.append(&PredefinedMenuItem::separator()); }
                } }
                let _ = menu.append(&sub);
            }
            let mut m = AppMenu { menu, ids, recent_media: recent_media.unwrap(), recent_projects: recent_projects.unwrap(), compact: compact.unwrap(), pin: pin.unwrap(), recents_seen: (vec![String::new()], vec![]) };
            m.set_recents(&[], &[]);
            m
        }
        /// macOS adds Show Tab Bar / Show All Tabs (window tabbing) and Enter Full Screen to any menu titled "View"; QCView's
        /// View menu has neither (its Fullscreen item is the one) — both switched off before the menu is installed.
        pub fn quiet_appkit_extras() {
            use objc2_foundation::{NSString, NSUserDefaults};
            let mtm = objc2::MainThreadMarker::new().expect("main thread");
            objc2_app_kit::NSWindow::setAllowsAutomaticWindowTabbing(false, mtm);
            NSUserDefaults::standardUserDefaults().setBool_forKey(false, &NSString::from_str("NSFullScreenMenuItemEverywhere"));
        }
        /// The menu's command for a muda event id.
        pub fn cmd(&self, id: &MenuId) -> Option<Cmd> { self.ids.get(id).cloned() }
        /// QCView's two Open Recent submenus: the file names, "(none)" when empty, then Clear Menu.
        pub fn set_recents(&mut self, media: &[String], projects: &[String]) {
            if self.recents_seen.0 == media && self.recents_seen.1 == projects { return }
            self.recents_seen = (media.to_vec(), projects.to_vec());
            for (sub, list, clear, make) in [(&self.recent_media, media, Cmd::ClearRecentMedia, Cmd::RecentMedia as fn(String) -> Cmd), (&self.recent_projects, projects, Cmd::ClearRecentProjects, Cmd::RecentProject as fn(String) -> Cmd)] {
                while sub.remove_at(0).is_some() {}
                if list.is_empty() { let _ = sub.append(&MenuItem::new(tr("(none)"), false, None)); continue }
                for (name, path) in recent_rows(list) { let i = MenuItem::new(&name, true, None); self.ids.insert(i.id().clone(), make(path)); let _ = sub.append(&i); }
                let _ = sub.append(&PredefinedMenuItem::separator());
                let c = MenuItem::new(tr("Clear Menu"), true, None); self.ids.insert(c.id().clone(), clear); let _ = sub.append(&c);
            }
        }
        /// The checkable items follow the state (QCView binds `checked`).
        pub fn set_checks(&self, compact: bool, pin: bool) { self.compact.set_checked(compact); self.pin.set_checked(pin); }
    }

    fn about() -> AboutMetadata {
        AboutMetadata { name: Some("QCView".into()), version: Some(env!("CARGO_PKG_VERSION").into()), comments: Some(tr("Color-accurate review player.").into()), website: Some("https://qcview.app/".into()), ..Default::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn in_window_bar_carries_the_spec_recents_and_checks() {
        let mut f = min_ui::Fonts::new();
        let mut m = InWindowMenu::new(&mut f);
        assert_eq!(m.bar.menus.iter().map(|x| x.title.as_str()).collect::<Vec<_>>(), ["&File", "&View", "&Tools", "&About"]);
        m.update(&["/a/b/clip&1.mov".into()], &[], true, false, &mut f);
        let file = &m.bar.menus[0].items;
        let min_ui::BarItem::Sub { items, .. } = &file[5] else { panic!("Open Recent Media") };
        let min_ui::BarItem::Action { id, label, .. } = &items[0] else { panic!() };
        assert_eq!(label, "clip&&1.mov", "a file name's & is literal");
        assert_eq!(m.cmd(*id), Some(Cmd::RecentMedia("/a/b/clip&1.mov".into())));
        let min_ui::BarItem::Action { checked, shortcut, .. } = &m.bar.menus[1].items[1] else { panic!() };
        assert_eq!((*checked, shortcut.as_str()), (Some(true), "Ctrl+Shift+C"));
        let min_ui::BarItem::Action { shortcut, .. } = &m.bar.menus[1].items[2] else { panic!() };
        assert_eq!(shortcut, "Ctrl+Alt+T");
    }
}
