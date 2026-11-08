//! The menu bar — QCView's `Main.qml` MenuBar (File / View / Tools / About) carried item for item (canon row 143), built with
//! `muda`: the native NSMenu on macOS (the app menu gains About / Hide / Quit as macOS expects), a Win32 menu bar on Windows.
//!
//! Shortcuts: on macOS the modified ones are the menu's key equivalents (Cmd+O fires the menu, and AppKit swallows the key, so
//! nothing fires twice); on Windows they are shown only (no accelerator table is wired into winit's loop) and the shell's key
//! handler keeps them. Items whose feature has not landed (streams, the host bridges, the colour panel, notes, safety guides,
//! the shortcut sheet) are present and disabled — the menu's shape is final (row 175's rule for Settings).
//! DEVIATION: About = the platform's about panel (muda's predefined item) instead of QCView's own modal.
use muda::{accelerator::{Accelerator, Code, Modifiers}, AboutMetadata, CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    OpenMedia, NewProject, OpenProject, SaveProject, SaveProjectAs, RecentMedia(String), ClearRecentMedia, RecentProject(String), ClearRecentProjects, Quit,
    Minimal, Compact, Pin, AllPanels, ZoomIn, ZoomOut, Fullscreen, DefaultFrame, LeftRail, RightRail, StatusBar,
    Background, Settings, Docs, CopyProjectLink,
}

pub struct AppMenu {
    pub menu: Menu,
    ids: HashMap<MenuId, Cmd>,
    recent_media: Submenu, recent_projects: Submenu,
    compact: CheckMenuItem, pin: CheckMenuItem,
    recents_seen: (Vec<String>, Vec<String>),
}

fn acc(mods: Modifiers, code: Code) -> Option<Accelerator> { Some(Accelerator::new(mods, code)) }
const CMD: Modifiers = if cfg!(target_os = "macos") { Modifiers::META } else { Modifiers::CONTROL };

impl AppMenu {
    pub fn new() -> AppMenu {
        let ids = std::cell::RefCell::new(HashMap::new());
        let item = |text: &str, enabled: bool, a: Option<Accelerator>, cmd: Option<Cmd>| { let i = MenuItem::new(text, enabled, a); if let Some(c) = cmd { ids.borrow_mut().insert(i.id().clone(), c); } i };
        let menu = Menu::new();
        #[cfg(target_os = "macos")] {
            let app = Submenu::new("qcv", true);
            let _ = app.append_items(&[&PredefinedMenuItem::about(Some("About QCView…"), Some(about())), &PredefinedMenuItem::separator(), &PredefinedMenuItem::services(None), &PredefinedMenuItem::separator(), &PredefinedMenuItem::hide(None), &PredefinedMenuItem::hide_others(None), &PredefinedMenuItem::show_all(None), &PredefinedMenuItem::separator()]);
            let _ = app.append(&item("Quit qcv", true, acc(CMD, Code::KeyQ), Some(Cmd::Quit)));   // ours, not the predefined Quit: the settings + window frame save first
            let _ = menu.append(&app);
        }
        // ---- File ----
        let file = Submenu::new("&File", true);
        let recent_media = Submenu::new("Open Recent Media", true);
        let recent_projects = Submenu::new("Open Recent Project", true);
        let _ = file.append_items(&[
            &item("Open Media…", true, acc(CMD, Code::KeyO), Some(Cmd::OpenMedia)),
            &item("Open Stream…", false, None, None),
            &item("Connect to After Effects", false, None, None),
            &item("Connect to Premiere Pro", false, None, None),
            &item("Connect to Blender", false, None, None),
            &recent_media,
            &PredefinedMenuItem::separator(),
            &item("New Project", true, None, Some(Cmd::NewProject)),
            &item("Open Project…", true, acc(CMD | Modifiers::SHIFT, Code::KeyO), Some(Cmd::OpenProject)),
            &recent_projects,
            &item("Save Project", true, acc(CMD, Code::KeyS), Some(Cmd::SaveProject)),
            &item("Save Project As…", true, acc(CMD | Modifiers::SHIFT, Code::KeyS), Some(Cmd::SaveProjectAs)),
            &PredefinedMenuItem::separator(),
            &item("Copy Project Link", true, None, Some(Cmd::CopyProjectLink)),
        ]);
        #[cfg(not(target_os = "macos"))] { let _ = file.append_items(&[&PredefinedMenuItem::separator(), &item("E&xit", true, acc(CMD, Code::KeyQ), Some(Cmd::Quit))]); }
        let _ = menu.append(&file);
        // ---- View ---- (QCView: the panel toggles are deliberately NOT checkable; Compact + Always on Top are)
        let compact = CheckMenuItem::new("Compact Mode", true, false, acc(CMD | Modifiers::SHIFT, Code::KeyC));
        let pin = CheckMenuItem::new("Always on Top", true, false, acc(CMD | Modifiers::ALT, Code::KeyT));
        ids.borrow_mut().insert(compact.id().clone(), Cmd::Compact); ids.borrow_mut().insert(pin.id().clone(), Cmd::Pin);
        let view = Submenu::new("&View", true);
        let _ = view.append_items(&[
            &item("Minimal Mode", true, acc(CMD, Code::Digit0), Some(Cmd::Minimal)),
            &compact, &pin,
            &item("Show All Panels", true, acc(CMD, Code::Digit9), Some(Cmd::AllPanels)),
            &item("Zoom In Timeline", true, acc(CMD, Code::Equal), Some(Cmd::ZoomIn)),
            &item("Zoom Out Timeline", true, acc(CMD, Code::Minus), Some(Cmd::ZoomOut)),
            &item("Fullscreen", true, None, Some(Cmd::Fullscreen)),   // QCView: no menu shortcut (F belongs to the viewport)
            &item("Default Size && Position", true, None, Some(Cmd::DefaultFrame)),
            &PredefinedMenuItem::separator(),
            &item("Left Rail", true, acc(CMD, Code::Digit1), Some(Cmd::LeftRail)),
            &item("Right Rail", true, acc(CMD, Code::Digit2), Some(Cmd::RightRail)),
            &item("Color Panel", false, acc(CMD, Code::Digit3), None),
            &item("Timeline", false, None, None),
            &item("Notes", false, acc(CMD, Code::Digit4), None),
            &item("Status Bar", true, acc(CMD, Code::Digit5), Some(Cmd::StatusBar)),
        ]);
        let _ = menu.append(&view);
        // ---- Tools ----
        let tools = Submenu::new("&Tools", true);
        let _ = tools.append_items(&[
            &item("Background", true, None, Some(Cmd::Background)),
            &item("Safety Guides", false, None, None),
            &item("Keyboard Shortcuts", false, acc(CMD, Code::Slash), None),
            &item("Settings…", true, acc(CMD, Code::Comma), Some(Cmd::Settings)),
        ]);
        let _ = menu.append(&tools);
        // ---- About ---- (Check for Updates waits for an updater: hidden, as QCView hides it off macOS)
        let about_m = Submenu::new("&About", true);
        #[cfg(not(target_os = "macos"))] { let _ = about_m.append_items(&[&PredefinedMenuItem::about(Some("About QCView…"), Some(about())), &PredefinedMenuItem::separator()]); }
        let _ = about_m.append(&item("Docs", true, None, Some(Cmd::Docs)));
        let _ = menu.append(&about_m);
        let mut m = AppMenu { menu, ids: ids.into_inner(), recent_media, recent_projects, compact, pin, recents_seen: (vec![vec!["".into()]].concat(), vec![]) };
        m.set_recents(&[], &[]);
        m
    }
    /// macOS adds Show Tab Bar / Show All Tabs (window tabbing) and Enter Full Screen to any menu titled "View"; QCView's
    /// View menu has neither (its Fullscreen item is the one) — both switched off before the menu is installed.
    #[cfg(target_os = "macos")]
    pub fn quiet_appkit_extras() {
        use objc2_foundation::{NSString, NSUserDefaults};
        let mtm = objc2::MainThreadMarker::new().expect("main thread");
        objc2_app_kit::NSWindow::setAllowsAutomaticWindowTabbing(false, mtm);
        NSUserDefaults::standardUserDefaults().setBool_forKey(false, &NSString::from_str("NSFullScreenMenuItemEverywhere"));
    }
    /// The menu's command for a muda event id.
    pub fn cmd(&self, id: &MenuId) -> Option<Cmd> { self.ids.get(id).cloned() }
    /// QCView's two Open Recent submenus: the file names (both separators split), "(none)" when empty, then Clear Menu.
    pub fn set_recents(&mut self, media: &[String], projects: &[String]) {
        if self.recents_seen.0 == media && self.recents_seen.1 == projects { return }
        self.recents_seen = (media.to_vec(), projects.to_vec());
        for (sub, list, clear, make) in [(&self.recent_media, media, Cmd::ClearRecentMedia, Cmd::RecentMedia as fn(String) -> Cmd), (&self.recent_projects, projects, Cmd::ClearRecentProjects, Cmd::RecentProject as fn(String) -> Cmd)] {
            while sub.remove_at(0).is_some() {}
            if list.is_empty() { let _ = sub.append(&MenuItem::new("(none)", false, None)); continue }
            for p in list {
                let name = p.rsplit(['/', '\\']).next().unwrap_or(p);
                let i = MenuItem::new(name, true, None); self.ids.insert(i.id().clone(), make(p.clone())); let _ = sub.append(&i);
            }
            let _ = sub.append(&PredefinedMenuItem::separator());
            let c = MenuItem::new("Clear Menu", true, None); self.ids.insert(c.id().clone(), clear); let _ = sub.append(&c);
        }
    }
    /// The checkable items follow the state (QCView binds `checked`).
    pub fn set_checks(&self, compact: bool, pin: bool) { self.compact.set_checked(compact); self.pin.set_checked(pin); }
}

fn about() -> AboutMetadata {
    AboutMetadata { name: Some("QCView".into()), version: Some(env!("CARGO_PKG_VERSION").into()), comments: Some("Color-accurate review player.".into()), website: Some("https://qcview.app/".into()), ..Default::default() }
}
