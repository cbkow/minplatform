//! The application's loop (winit): its windows - each a whole browser of its own (canon row 448) -, the menu bar, the
//! keys, the mouse. The file system is the workers' (`lister`); they wake this loop when something has come.
//!
//! macOS: a native menu bar (muda), whose key equivalents AppKit answers itself. Windows: the menu bar is drawn in the
//! window (min-ui's, as minNotes'), and the keys are taken here. A window is made unseen on Windows and shown after
//! its first frame, so that it never shows white (the family's fix, rows 382 / 428).
use crate::lister::Port;
thread_local! {
    /// The windows' contents while a drag of our own runs on Windows (see `App::sink`); empty at every other time.
    static OWN_DRAG: std::cell::RefCell<Vec<(WindowId, *mut Ui)>> = const { std::cell::RefCell::new(Vec::new()) };
}
use crate::menu::Cmd;
use crate::places::Place;
use crate::ui::{Out, Ui};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::sync::Arc;
use winit31 as winit;
use winit::application::ApplicationHandler;
use winit::event::{ButtonSource, ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};

// (the interface - with its presenter - is dropped before the window it draws into)
struct Win { ui: Ui, window: Arc<Box<dyn Window>> }
struct App {
    first: Option<PathBuf>, favorites: Vec<Place>, proxy: Arc<EventLoopProxy>, windows: HashMap<WindowId, Win>, focused: Option<WindowId>,
    /// (shift, the command key - Ctrl on Windows -, Alt / Option)
    mods: (bool, bool, bool), /// the Ctrl key itself (macOS: Ctrl+Tab steps through the tabs, row 480)
    ctrl: bool, alt_clean: bool, clipboard: Option<arboard::Clipboard>,
    /// the files that were CUT (Windows' habit): pasted, they are moved
    cut: Option<Vec<PathBuf>>,
    #[cfg(target_os = "macos")] menu: Option<crate::menu::AppMenu>,
    /// the tray (row 505): its icon, its status line and what that says now; the folder a window asked for from it opens in
    tray: Option<tray_icon::TrayIcon>, tray_status: Option<muda::MenuItem>, tray_says: String, home: PathBuf,
}
/// The tray's icon was pressed (Windows: a window is shown).
static TRAY_CLICKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static MENU_EVENTS: std::sync::Mutex<Vec<muda::MenuEvent>> = std::sync::Mutex::new(Vec::new());

/// The window's size in logical px: `UFB_SIZE=1440x900`, else 1280 x 800.
/// The size a window opens at, in points: `UFB_SIZE` (a scripted look), else the size the last one was left at (row 523), else 1280 x 800.
fn wanted_size() -> (f64, f64) {
    let read = |v: String| v.split_once('x').and_then(|(a, b)| Some((a.trim().parse::<f64>().ok()?, b.trim().parse::<f64>().ok()?))).filter(|(w, h)| *w >= 320.0 && *h >= 240.0);
    std::env::var("UFB_SIZE").ok().and_then(read).or_else(|| remembers().then(|| read(crate::settings::with(|s| s.string("window/size")))).flatten()).unwrap_or((1280.0, 800.0))
}
/// A scripted look neither reads nor writes the window's memory: its pictures are of the size it asked for.
fn remembers() -> bool { std::env::var_os("UFB_DEMO").is_none() && std::env::var_os("UFB_SHOT").is_none() && std::env::var_os("UFB_SIZE").is_none() }
/// The window's size is kept as it is now (when it loses the keys, when it closes, when the application ends): a window
/// that fills the screen keeps the size it had before, and that it filled it.
fn remember(window: &dyn Window) {
    if !remembers() || window.is_minimized() == Some(true) { return }
    let max = window.is_maximized(); let (sz, k) = (window.surface_size(), window.scale_factor().max(0.1));
    crate::settings::with(|s| { s.set_bool("window/maximized", max); if !max && window.fullscreen().is_none() && sz.width > 0 { s.set_str("window/size", &format!("{}x{}", (sz.width as f64 / k).round(), (sz.height as f64 / k).round())); } });
}

/// A key as the key map names it: the letter or digit of the physical key (found on any layout), the punctuation the
/// map uses, the named keys.
fn key_name(event: &winit::event::KeyEvent) -> String {
    match &event.logical_key {
        Key::Named(n) => return match n { NamedKey::ArrowUp => "Up", NamedKey::ArrowDown => "Down", NamedKey::ArrowLeft => "Left", NamedKey::ArrowRight => "Right", NamedKey::Enter => "Enter", NamedKey::Backspace => "Backspace", NamedKey::Delete => "Delete", NamedKey::Tab => "Tab", NamedKey::Escape => "Escape",
            NamedKey::F2 => "F2", NamedKey::F4 => "F4", NamedKey::F5 => "F5", NamedKey::Home => "Home", NamedKey::End => "End", NamedKey::PageUp => "PageUp", NamedKey::PageDown => "PageDown", _ => "" }.to_string(),
        _ => {}
    }
    if let PhysicalKey::Code(c) = event.physical_key {
        match c { KeyCode::BracketLeft => return "[".into(), KeyCode::BracketRight => return "]".into(), KeyCode::Period => return ".".into(), KeyCode::Comma => return ",".into(), KeyCode::Space => return "Space".into(), _ => {} }
        let n = format!("{c:?}"); if let Some(l) = n.strip_prefix("Key").or(n.strip_prefix("Digit")).filter(|l| l.len() == 1) { return l.to_string() }
    }
    match &event.logical_key { Key::Character(c) => c.to_uppercase(), _ => String::new() }
}

#[cfg(target_os = "macos")] static DOCK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
/// The application is in the Dock (an ordinary one), or a menu-bar item alone (an accessory). Coming back it is
/// brought forward: a window opened from the tray's menu is to be seen.
#[cfg(target_os = "macos")]
fn dock(on: bool) {
    use objc2::runtime::AnyObject; use objc2::{class, msg_send};
    unsafe { let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication]; if app.is_null() { return }
        let _: bool = msg_send![app, setActivationPolicy: if on { 0isize } else { 1isize }];   // NSApplicationActivationPolicyRegular / Accessory
        if on { let _: () = msg_send![app, activateIgnoringOtherApps: true]; } }
    if std::env::var_os("UFB_CLOSE_AFTER").is_some() { eprintln!("UFB: in the Dock: {on}"); }
}
static CLOSE_NOW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
impl App {
    /// What the drag-and-drop layer calls: it answers from what the window last drew (`dnd::on_notice`), asks for a
    /// frame, and wakes the loop for what a drop left to do. Nothing of the application is borrowed by it.
    fn sink(ui: &Ui, window: &Arc<Box<dyn Window>>, proxy: &Arc<EventLoopProxy>) -> min_dnd::Sink {
        // (the window is held WEAKLY: the layer keeps this sink for as long as it likes, and a window the application
        // has dropped must close - closing the last window with the tray there left it standing, row 515)
        let (shared, window, proxy) = (ui.dnd.clone(), Arc::downgrade(window), proxy.clone());
        std::rc::Rc::new(move |n| { let before = shared.borrow().hover; let op = crate::dnd::on_notice(&shared, n); let Some(window) = window.upgrade() else { return op }; window.request_redraw(); proxy.wake_up();
            // Windows, a drag of OUR OWN: the system's drag runs inside the pointer's event, and the redraws asked for wait
            // until it has returned - so the outline of the place under the pointer is drawn here, at once (row 478)
            if before != shared.borrow().hover { let id = window.id(); let ui = OWN_DRAG.with(|d| d.borrow().iter().find(|(w, _)| *w == id).map(|(_, u)| *u)); if let Some(ui) = ui { unsafe { (*ui).redraw(); } } }
            op })
    }
    fn open(&mut self, el: &dyn ActiveEventLoop, path: PathBuf) {
        if std::env::var_os("UFB_DEMO").is_some() { eprintln!("UFB_DEMO: a window on {}", path.display()); }
        let (w, h) = wanted_size();
        #[allow(unused_mut)] let mut attrs = WindowAttributes::default().with_title("UFB").with_surface_size(winit::dpi::LogicalSize::new(w, h)).with_maximized(remembers() && crate::settings::with(|s| s.bool("window/maximized"))).with_min_surface_size(winit::dpi::LogicalSize::new(480.0, 320.0));
        // Windows: the caption takes the menu row's colour, so the window is one surface from its top edge down (as minNotes');
        // and it is made unseen, to be shown after its first frame
        #[cfg(windows)] {
            use winit::platform::windows::WindowAttributesWindows; use min_theme::tokens as tk;
            let c8 = |c: min_theme::Color| winit::platform::windows::Color::from_rgb((c.r * 255.0).round() as u8, (c.g * 255.0).round() as u8, (c.b * 255.0).round() as u8);
            attrs = attrs.with_theme(Some(winit::window::Theme::Dark));
            // (winit's own drag and drop is OFF: a window has one drop target, and it is ours - `min-dnd`)
            // Windows' two icons, as the family's (rows 248, 372): the title bar's small one from the picture, the taskbar's and
            // Alt+Tab's the exe's own resource (build.rs, id 1) - 1.4.2's icon both
            let small = (|| { let mut d = png::Decoder::new(std::io::Cursor::new(&include_bytes!("../assets/icons/ufb-windows-64.png")[..])); d.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA | png::Transformations::STRIP_16);
                let mut rd = d.read_info().ok()?; let mut buf = vec![0u8; rd.output_buffer_size()]; let info = rd.next_frame(&mut buf).ok()?; buf.truncate(info.buffer_size());
                if info.color_type != png::ColorType::Rgba { return None } winit::icon::RgbaIcon::new(buf, info.width, info.height).ok().map(winit::icon::Icon::from) })();
            attrs = attrs.with_window_icon(small);
            let big = winit::platform::windows::WinIcon::from_resource(1, None).ok().map(winit::icon::Icon::from);
            attrs = attrs.with_platform_attributes(Box::new(WindowAttributesWindows::default().with_taskbar_icon(big).with_drag_and_drop(false).with_title_background_color(Some(c8(tk::MENU_BAR_BACKGROUND_COLOR))).with_title_text_color(c8(tk::MENU_BAR_TITLE_COLOR))));
            attrs = attrs.with_visible(false);
        }
        let Ok(window) = el.create_window(attrs) else { eprintln!("UFB: no window could be made"); return }; let window: Arc<Box<dyn Window>> = Arc::new(window);
        // one channel a window: its workers send into it and wake the loop
        let (tx, rx) = channel(); let proxy = self.proxy.clone(); let port = Port { tx, wake: Arc::new(move || proxy.wake_up()) };
        let Some(mut ui) = Ui::new(window.clone(), port, rx, path, self.favorites.clone()) else { eprintln!("UFB: the window's content could not be set up (no renderer)"); return };
        // the window takes drops of files; files promised by another application are received in a folder of our own first
        static WINDOWS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        if let Err(why) = min_dnd::attach(&**window, WINDOWS.fetch_add(1, std::sync::atomic::Ordering::Relaxed), &crate::dnd::staging(), Self::sink(&ui, &window, &self.proxy)) { eprintln!("UFB: this window takes no drops: {why}"); }
        if cfg!(windows) { ui.redraw(); window.set_visible(true); }
        // `UFB_CLOSE_AFTER=<seconds>`: the windows are closed as their close button closes them, after that long (a check of
        // "the last window closed leaves the application in the tray" that needs no hand)
        if let Some(secs) = std::env::var("UFB_CLOSE_AFTER").ok().and_then(|v| v.parse::<u64>().ok()) { let proxy = self.proxy.clone(); let _ = std::thread::spawn(move || { std::thread::sleep(std::time::Duration::from_secs(secs)); CLOSE_NOW.store(true, std::sync::atomic::Ordering::Relaxed); proxy.wake_up(); }); }
        window.request_redraw(); let id = window.id(); self.focused = Some(id); self.windows.insert(id, Win { window, ui });
    }
    /// What the windows asked for: a new window, to close, to minimise, to quit. The last window closed ends the application.
    /// The tray (row 505; rows 445, 457: one process, in the tray): made once, after the first window, on the main
    /// thread (S14's rule). Its menu: what sync is doing, Open a Window, Quit UFB. With it the application stays when its
    /// last window is closed - sync goes on. Not in a scripted run (those end with their window).
    fn make_tray(&mut self) {
        if self.tray.is_some() || std::env::var_os("UFB_SHOT").is_some() || std::env::var_os("UFB_DEMO").is_some() || std::env::var_os("UFB_NO_TRAY").is_some() { return }
        use muda::{Menu, MenuItem, PredefinedMenuItem};
        let status = MenuItem::with_id("tray-status", "Sync: off", false, None); let menu = Menu::new();
        let _ = menu.append_items(&[&status, &PredefinedMenuItem::separator(), &MenuItem::with_id("tray-open", "Open a Window", true, None), &PredefinedMenuItem::separator(), &MenuItem::with_id("tray-quit", "Quit UFB", true, None)]);
        // UFB's own mark, as 1.4.x shows it: a template for the macOS menu bar (the system tints it), the colour mark on Windows
        let bytes: &[u8] = if cfg!(target_os = "macos") { include_bytes!("../assets/icons/tray-macos.png") } else { include_bytes!("../assets/icons/tray-windows.png") };
        let icon = (|| { let mut d = png::Decoder::new(std::io::Cursor::new(bytes)); d.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16); let mut r = d.read_info().ok()?; let mut buf = vec![0u8; r.output_buffer_size()]; let info = r.next_frame(&mut buf).ok()?; buf.truncate(info.buffer_size());
            let rgba: Vec<u8> = match info.color_type { png::ColorType::Rgba => buf, png::ColorType::Rgb => buf.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(), png::ColorType::GrayscaleAlpha => buf.chunks(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(), _ => buf.iter().flat_map(|g| [*g, *g, *g, 255]).collect() };
            // (the pictures are 512 px: 64 is plenty for a tray on any screen)
            let small = image::imageops::resize(&image::RgbaImage::from_raw(info.width, info.height, rgba)?, 64, 64, image::imageops::FilterType::Triangle);
            tray_icon::Icon::from_rgba(small.into_raw(), 64, 64).ok() })();
        tray_icon::TrayIconEvent::set_event_handler(Some({ let p = self.proxy.clone(); move |e: tray_icon::TrayIconEvent| { if let tray_icon::TrayIconEvent::Click { button: tray_icon::MouseButton::Left, button_state: tray_icon::MouseButtonState::Up, .. } = e { TRAY_CLICKED.store(true, std::sync::atomic::Ordering::Relaxed); p.wake_up(); } } }));
        self.tray = icon.and_then(|icon| { let b = tray_icon::TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip("UFB").with_menu_on_left_click(cfg!(target_os = "macos")); #[cfg(target_os = "macos")] let b = b.with_icon_templated(icon); #[cfg(not(target_os = "macos"))] let b = b.with_icon(icon); b.build().ok() });
        if self.tray.is_none() { eprintln!("UFB: no tray icon could be made: the application ends with its last window"); } self.tray_status = Some(status);
    }
    /// The tray's status line follows sync (set when it changes).
    fn tray_refresh(&mut self) { if self.tray.is_none() { return } let says = format!("Sync: {}", crate::ui::Ui::sync_words()); if says != self.tray_says { if let Some(s) = &self.tray_status { s.set_text(&says); } if let Some(t) = &self.tray { let _ = t.set_tooltip(Some(format!("UFB - {says}"))); } self.tray_says = says; } }
    /// A window from the tray: the one in front is brought forward, or a new one is opened.
    fn tray_open(&mut self, el: &dyn ActiveEventLoop) { match self.focused.and_then(|id| self.windows.get(&id)).or_else(|| self.windows.values().next()) { Some(w) => { w.window.set_minimized(false); w.window.focus_window(); } None => { let home = self.home.clone(); self.open(el, home); } } }
    fn settle(&mut self, el: &dyn ActiveEventLoop) {
        let mut asks: Vec<(WindowId, Out)> = vec![]; let mut done = false;
        if CLOSE_NOW.swap(false, std::sync::atomic::Ordering::Relaxed) { for w in self.windows.values_mut() { w.ui.out.push(Out::Close); } }
        for (id, w) in self.windows.iter_mut() { for o in w.ui.out.drain(..) { asks.push((*id, o)); } done |= w.ui.done; }
        for (id, o) in asks { match o {
            Out::NewWindow(path) => self.open(el, path),
            // (the drag-and-drop layer lets go of the window first: what it keeps must not outlive it)
            Out::Close => { if let Some(w) = self.windows.remove(&id) { remember(&**w.window); min_dnd::detach(&**w.window);
                    // (a check without a hand, `UFB_CLOSE_AFTER`: who else still holds the window - anyone, and it stays on the screen)
                    if std::env::var_os("UFB_CLOSE_AFTER").is_some() { let Win { window, ui } = w; drop(ui); eprintln!("UFB: a window was closed; others still holding it: {}; windows left: {}; tray: {}", Arc::strong_count(&window) - 1, self.windows.len(), self.tray.is_some()); } }
                if self.focused == Some(id) { self.focused = None; } }
            Out::Minimize => if let Some(w) = self.windows.get(&id) { w.window.set_minimized(true); },
            Out::Quit => done = true,
            Out::OpenDefault(path) => crate::sysopen::open(&path),
            Out::OpenWith(path, app) => crate::sysopen::open_with(&path, &app),
            Out::OpenAs(path) => crate::sysopen::open_as(&path), Out::Reveal(paths) => crate::sysopen::reveal(&paths),
            // Windows: Explorer's own menu (it is shown when the window's message loop reaches our message - never from inside this callback)
            Out::ShellMenu(at, paths) => { #[cfg(windows)] if let Some(w) = self.windows.get(&id) { use raw_window_handle::{HasWindowHandle, RawWindowHandle};
                    if let Ok(RawWindowHandle::Win32(h)) = w.window.window_handle().map(|h| h.as_raw()) { let proxy = self.proxy.clone(); crate::ctxmenu::win::more(h.hwnd.get() as *mut std::ffi::c_void, id, at, paths, Arc::new(move || proxy.wake_up())); } }
                #[cfg(not(windows))] { let _ = (at, paths); } }
            // the system's drag of files (we are still inside the pointer's own event, as the layer needs). What may
            // not be taken away by this build (the belt) is offered as a copy or a link only.
            Out::Drag(paths) => if let Some(w) = self.windows.get(&id) {
                let may_take = paths.iter().all(|p| p.parent().is_some_and(crate::ops::allowed_in));
                let ops = min_dnd::COPY | min_dnd::LINK | if may_take { min_dnd::MOVE } else { 0 };
                let (window, pic, sink) = (w.window.clone(), crate::dnd::picture(paths.len(), w.window.scale_factor()), Self::sink(&w.ui, &w.window, &self.proxy));
                // (Windows: the drag returns when it is over; meanwhile the windows are drawn from the drag's own notices.
                // Nothing else touches them while it runs: this thread is inside the system's drag.)
                if cfg!(windows) { let all: Vec<(WindowId, *mut Ui)> = self.windows.iter_mut().map(|(i, w)| (*i, &mut w.ui as *mut Ui)).collect(); OWN_DRAG.with(|d| *d.borrow_mut() = all); }
                let r = min_dnd::start_drag(&**window, &paths, pic, ops, sink); OWN_DRAG.with(|d| d.borrow_mut().clear());
                if let Err(why) = r { eprintln!("UFB: the drag did not start: {why}"); } },
            // the clipboard: files onto it and from it; the text of the field in hand
            Out::CopyFiles { paths, cut } => { if self.clipboard.is_none() { self.clipboard = arboard::Clipboard::new().ok(); }
                if let Some(cb) = self.clipboard.as_mut() { if cb.set().file_list(&paths).is_ok() { self.cut = if cut { Some(paths) } else { None }; crate::pane::set_cut(self.cut.as_deref().unwrap_or(&[])); for w in self.windows.values() { w.window.request_redraw(); } } } }
            Out::PasteFiles { mv } => { if self.clipboard.is_none() { self.clipboard = arboard::Clipboard::new().ok(); }
                let list = self.clipboard.as_mut().and_then(|cb| cb.get().file_list().ok()).unwrap_or_default();
                // files that were CUT here (and are still what the clipboard holds) are moved by a paste, once
                let was_cut = self.cut.as_ref().is_some_and(|c| *c == list); if was_cut { self.cut = None; crate::pane::set_cut(&[]); }
                if let Some(w) = self.windows.get_mut(&id) { w.ui.paste_files(list, mv || was_cut); w.window.request_redraw(); } }
            Out::FieldCopy { cut } => { if self.clipboard.is_none() { self.clipboard = arboard::Clipboard::new().ok(); }
                if let (Some(cb), Some(w)) = (self.clipboard.as_mut(), self.windows.get_mut(&id)) { if let Some(text) = w.ui.copy(cut) { let _ = cb.set_text(text); } w.window.request_redraw(); } }
            Out::CopyText(text) => { if self.clipboard.is_none() { self.clipboard = arboard::Clipboard::new().ok(); } if let Some(cb) = self.clipboard.as_mut() { let _ = cb.set_text(text); } }
            Out::FieldPaste => { if self.clipboard.is_none() { self.clipboard = arboard::Clipboard::new().ok(); }
                if let (Some(cb), Some(w)) = (self.clipboard.as_mut(), self.windows.get_mut(&id)) { if let Ok(text) = cb.get_text() { w.ui.paste(&text); } w.window.request_redraw(); } }
            // the system's menu: handed to the run loop (never shown from inside this callback - S19); a row of ours comes back through `proxy_wake_up`
            Out::Menu(ask) => { #[cfg(target_os = "macos")] if let Some(w) = self.windows.get(&id) { use raw_window_handle::{HasWindowHandle, RawWindowHandle};
                    if let Ok(RawWindowHandle::AppKit(h)) = w.window.window_handle().map(|h| h.as_raw()) { let proxy = self.proxy.clone(); crate::ctxmenu::sys::show(h.ns_view.as_ptr(), id, ask, Arc::new(move || proxy.wake_up())); } }
                #[cfg(not(target_os = "macos"))] { let _ = (id, ask); } }
        } }
        // (with the tray the application stays when its last window is closed: sync goes on - rows 445, 457)
        if done || (self.windows.is_empty() && self.tray.is_none()) { if let Some(w) = self.focused.and_then(|id| self.windows.get(&id)).or_else(|| self.windows.values().next()) { remember(&**w.window); } self.windows.clear(); el.exit(); }
        self.tray_refresh();
        // macOS (row 525): in the Dock while a window is open; with only the tray left, out of it (and out of the
        // application switcher) - 1.4.2 hid its Dock icon when it started hidden; a window opened again brings it back
        #[cfg(target_os = "macos")] if self.tray.is_some() { let want = !self.windows.is_empty(); if DOCK.swap(want, std::sync::atomic::Ordering::Relaxed) != want { dock(want); } }
        #[cfg(target_os = "macos")] if let (Some(m), Some(w)) = (self.menu.as_mut(), self.focused.and_then(|id| self.windows.get(&id))) { m.sync(w.ui.menu_state()); m.set_undo(crate::ops::undo_label()); m.set_redo(crate::ops::redo_label()); }
    }
    /// The drawn menu bar's keys (Windows' rules, as minNotes'): Alt alone, F10, Alt with a letter; while it is open, the arrows, Return, Escape and letters are its own.
    fn menu_bar_takes(&mut self, id: WindowId, event: &winit::event::KeyEvent) -> bool {
        use min_ui::{BarKey, Named as N};
        let (shift, cmd, alt) = self.mods; let Some(w) = self.windows.get_mut(&id) else { return false }; let Some(mb) = w.ui.menubar.as_mut() else { return false };
        let pressed = event.state.is_pressed(); let active = mb.bar.is_active();
        let plain = match event.physical_key { PhysicalKey::Code(c) => { let n = format!("{c:?}"); n.strip_prefix("Key").or(n.strip_prefix("Digit")).filter(|l| l.len() == 1).and_then(|l| l.chars().next()).map(|ch| ch.to_ascii_lowercase()) } _ => None };
        let k = if matches!(event.logical_key, Key::Named(NamedKey::Alt)) {
            if pressed { if !event.repeat { self.alt_clean = !cmd && !shift; } None } else if std::mem::take(&mut self.alt_clean) { Some(BarKey::AltTap) } else { None }
        } else if !pressed { None } else {
            self.alt_clean = false;
            if matches!(event.logical_key, Key::Named(NamedKey::F10)) && !cmd && !shift { Some(BarKey::F10) }
            else if alt && !cmd && !shift && plain.is_some_and(|c| c.is_ascii_alphabetic()) { plain.map(BarKey::Alt) }
            else if !active { None }
            else { Some(match &event.logical_key {
                Key::Named(NamedKey::Escape) => BarKey::Named(N::Escape), Key::Named(NamedKey::Enter) => BarKey::Named(N::Enter), Key::Named(NamedKey::ArrowUp) => BarKey::Named(N::Up), Key::Named(NamedKey::ArrowDown) => BarKey::Named(N::Down),
                Key::Named(NamedKey::ArrowLeft) => BarKey::Named(N::Left), Key::Named(NamedKey::ArrowRight) => BarKey::Named(N::Right), Key::Character(c) if c.as_str() == " " => BarKey::Space,
                Key::Character(c) if !cmd => c.chars().next().map(|ch| BarKey::Char(ch.to_ascii_lowercase())).unwrap_or(BarKey::Other), _ => BarKey::Other }) }
        };
        let Some(k) = k else { return false };
        let (chosen, _, consumed) = w.ui.with_menu_key(&k);
        if let Some(c) = chosen { w.ui.command(c); }
        w.window.request_redraw(); consumed || chosen.is_some()
    }
}

impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, el: &dyn ActiveEventLoop) {
        if !self.windows.is_empty() { return }
        #[cfg(target_os = "macos")] if self.menu.is_none() && !crate::menu::InWindowMenu::wanted() { let m = crate::menu::AppMenu::new(); m.menu.init_for_nsapp(); self.menu = Some(m); }
        if let Some(path) = self.first.take() { self.open(el, path); }
        if self.windows.is_empty() { el.exit(); } else { self.make_tray();
            // the release's updater (macOS; row 536): started once the first window is there - not in a scripted look
            #[cfg(target_os = "macos")] if crate::identity::kind() == crate::identity::Kind::Release && std::env::var_os("UFB_DEMO").is_none() && std::env::var_os("UFB_SHOT").is_none() { crate::sparkle::start(); } }
    }
    fn window_event(&mut self, el: &dyn ActiveEventLoop, id: WindowId, e: WindowEvent) {
        if let WindowEvent::KeyboardInput { event, .. } = &e { if self.menu_bar_takes(id, event) { self.settle(el); return } }
        let mods = self.mods; let ctrl = self.ctrl; #[cfg(target_os = "macos")] let native = self.menu.is_some(); #[cfg(not(target_os = "macos"))] let native = false;
        let Some(w) = self.windows.get_mut(&id) else { return };
        let mut redraw = true;
        match e {
            WindowEvent::CloseRequested => w.ui.out.push(Out::Close),
            WindowEvent::RedrawRequested => { w.ui.redraw(); redraw = w.ui.animating(); }
            WindowEvent::SurfaceResized(sz) => w.ui.resize(sz.width, sz.height),
            WindowEvent::ScaleFactorChanged { .. } => {}
            WindowEvent::Focused(on) => { if on { self.focused = Some(id); w.ui.activated(); } else { remember(&**w.window); w.ui.deactivated(); self.alt_clean = false; } }
            // (sideways too: a trackpad's swipe, a tilting wheel - and macOS turns the wheel sideways itself while Shift is held)
            WindowEvent::MouseWheel { delta, .. } => { let s = w.window.scale_factor(); let (dx, dy) = match delta { MouseScrollDelta::LineDelta(x, y) => (x as f64 * 60.0 * s, y as f64 * 60.0 * s), MouseScrollDelta::PixelDelta(p) => (p.x, p.y), _ => (0.0, 0.0) }; if dy != 0.0 { w.ui.wheel(-dy); } if dx != 0.0 { w.ui.wheel_x(-dx); } }
            WindowEvent::ModifiersChanged(m) => { let st = m.state(); self.ctrl = st.control_key(); self.mods = (st.shift_key(), if cfg!(target_os = "macos") { st.meta_key() } else { st.control_key() }, st.alt_key()); w.ui.mods = min_ui::Mods { shift: self.mods.0, cmd: self.mods.1 }; redraw = false; }
            WindowEvent::PointerMoved { position, .. } => w.ui.pointer(&min_ui::Event::Move { x: position.x, y: position.y }),
            WindowEvent::PointerButton { state, position, button: ButtonSource::Mouse(b), .. } => {
                self.alt_clean = false; let (x, y) = (position.x, position.y);
                match b {
                    MouseButton::Left | MouseButton::Right | MouseButton::Middle => { let button = match b { MouseButton::Left => min_ui::Button::Primary, MouseButton::Right => min_ui::Button::Secondary, _ => min_ui::Button::Middle };
                        w.ui.pointer(&if state == ElementState::Pressed { min_ui::Event::Down { x, y, button } } else { min_ui::Event::Up { x, y, button } }); }
                    // the mouse's own back and forward buttons (row 449)
                    MouseButton::Back if state == ElementState::Pressed => w.ui.command(Cmd::Back),
                    MouseButton::Forward if state == ElementState::Pressed => w.ui.command(Cmd::Forward),
                    _ => {}
                }
            }
            WindowEvent::PointerLeft { .. } => w.ui.pointer(&min_ui::Event::Leave),
            // a pinch on a trackpad zooms the lightbox's picture
            WindowEvent::PinchGesture { delta, .. } => w.ui.pinch(delta),
            // a key let go ends a held shuttle of the lightbox's player
            WindowEvent::KeyboardInput { event, .. } if !event.state.is_pressed() => { let name = key_name(&event); w.ui.key_up(&name); }
            WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() => {
                use min_ui::{Key as K, Named as N};
                let (shift, cmd, alt) = mods; let name = key_name(&event);
                // the clipboard's keys belong to the text field in hand (the path bar takes a pasted path, row 449)
                if cfg!(target_os = "macos") && ctrl && !cmd && name == "Tab" { w.ui.command(if shift { Cmd::PrevTab } else { Cmd::NextTab }); }
                else if cmd && !alt && w.ui.typing() && matches!(name.as_str(), "V" | "C" | "X") {
                    if self.clipboard.is_none() { self.clipboard = arboard::Clipboard::new().ok(); }
                    if let Some(cb) = self.clipboard.as_mut() { if name == "V" { if let Ok(text) = cb.get_text() { w.ui.paste(&text); } } else if let Some(text) = w.ui.copy(name == "X") { let _ = cb.set_text(text); } }
                } else {
                    let key = match &event.logical_key { Key::Named(NamedKey::ArrowUp) => K::Named(N::Up), Key::Named(NamedKey::ArrowDown) => K::Named(N::Down), Key::Named(NamedKey::ArrowLeft) => K::Named(N::Left), Key::Named(NamedKey::ArrowRight) => K::Named(N::Right), Key::Named(NamedKey::Home) => K::Named(N::Home), Key::Named(NamedKey::End) => K::Named(N::End),
                        Key::Named(NamedKey::Escape) => K::Named(N::Escape), Key::Named(NamedKey::Enter) => K::Named(N::Enter), Key::Named(NamedKey::Tab) => K::Named(N::Tab), Key::Named(NamedKey::Delete) => K::Named(N::Delete), Key::Named(NamedKey::Backspace) => K::Named(N::Backspace),
                        Key::Named(NamedKey::PageUp) => K::Char("PageUp".into()), Key::Named(NamedKey::PageDown) => K::Char("PageDown".into()), Key::Character(c) => K::Char(c.to_string()), _ => K::Char(String::new()) };
                    let ev = min_ui::KeyEvent { key, text: event.text.as_ref().map(|t| t.to_string()).filter(|_| !cmd && !(alt && cfg!(windows))), shift, cmd };
                    w.ui.key(&ev, &name, alt, native);
                }
            }
            _ => redraw = false,
        }
        if redraw { w.window.request_redraw(); }
        self.settle(el);
    }
    fn proxy_wake_up(&mut self, el: &dyn ActiveEventLoop) {
        // the menu bar's rows go to the window in front
        // the tray: its icon pressed (Windows), a row of its menu
        if TRAY_CLICKED.swap(false, std::sync::atomic::Ordering::Relaxed) { self.tray_open(el); }
        // a second launch handed something over (row 519): a window comes forward (one is opened when none is), and what
        // was asked for is opened in it - a `union:` link is shown in the file manager instead, as at a start
        for asked in crate::instance::take() {
            eprintln!("UFB: a second launch asked for {}", if asked.is_empty() { "a window" } else { asked.as_str() });
            // (the test package's own scheme reads as the application's: `ufbtest:` is `ufb:`)
            let asked = match asked.strip_prefix("ufbtest:") { Some(rest) => format!("ufb:{rest}"), None => asked };
            if crate::db::is_union(&asked) { if let Some(p) = crate::db::from_link(&asked) { if p.exists() { crate::sysopen::reveal(&[p]); } } continue }
            self.tray_open(el);
            if !asked.is_empty() { if let Some(w) = self.focused.and_then(|id| self.windows.get_mut(&id)).or(None) { w.ui.open_text(&asked); w.window.request_redraw(); } }
        }
        { let mut events = MENU_EVENTS.lock().unwrap(); let (tray, rest): (Vec<muda::MenuEvent>, Vec<muda::MenuEvent>) = std::mem::take(&mut *events).into_iter().partition(|e| e.id.0.starts_with("tray-")); *events = rest; drop(events);
            for ev in tray { match ev.id.0.as_str() { "tray-open" => self.tray_open(el), "tray-quit" => {
                // with tasks running it is asked first: in a window when there is one, by the system's own box when none is open
                if crate::tasks::busy() { match self.focused.filter(|id| self.windows.contains_key(id)).or_else(|| self.windows.keys().next().copied()) {
                    Some(id) => { let w = self.windows.get_mut(&id).unwrap(); w.window.set_minimized(false); w.window.focus_window(); w.ui.ask_quit(); w.window.request_redraw(); continue }
                    None => if !quit_anyway() { continue } } }
                self.windows.clear(); el.exit(); return } _ => {} } } }
        #[cfg(not(target_os = "macos"))] MENU_EVENTS.lock().unwrap().clear();
        #[cfg(target_os = "macos")] { let events: Vec<muda::MenuEvent> = std::mem::take(&mut *MENU_EVENTS.lock().unwrap());
            for ev in events { if self.windows.is_empty() && Cmd::from_id(&ev.id.0) == Some(Cmd::NewWindow) { let home = self.home.clone(); self.open(el, home); continue } if let (Some(c), Some(w)) = (Cmd::from_id(&ev.id.0), self.focused.and_then(|id| self.windows.get_mut(&id)).or(None)) { w.ui.command(c); w.window.request_redraw(); } else if Cmd::from_id(&ev.id.0) == Some(Cmd::Quit) { if crate::tasks::busy() && !quit_anyway() { continue } self.windows.clear(); el.exit(); return } } }
        // a row of ours chosen in a context menu goes to the window the menu was for
        for (id, row) in crate::ctxmenu::take() { if let Some(w) = self.windows.get_mut(&id) { w.ui.menu_chosen(row); w.window.request_redraw(); } }
        // what the workers sent: every window takes its own and draws again
        for w in self.windows.values_mut() { if w.ui.poll() { w.window.request_redraw(); } }
        self.settle(el);
    }
}

pub fn run(folder: PathBuf, favorites: Vec<Place>) {
    #[cfg(target_os = "macos")] crate::menu::AppMenu::quiet_appkit_extras();   // before the application launches: AppKit reads these then
    let el = EventLoop::new().expect("the event loop"); let proxy = Arc::new(el.create_proxy());
    { let p = proxy.clone(); crate::instance::serve(Arc::new(move || p.wake_up())); }
    #[cfg(target_os = "macos")] { let p = proxy.clone(); links::install(Arc::new(move || p.wake_up())); }
    { let p = proxy.clone(); muda::MenuEvent::set_event_handler(Some(move |ev: muda::MenuEvent| { MENU_EVENTS.lock().unwrap().push(ev); p.wake_up(); })); }
    let _ = el.run_app(App { tray: None, tray_status: None, tray_says: String::new(), home: crate::places::home(), first: Some(folder), favorites, proxy, windows: HashMap::new(), focused: None, mods: (false, false, false), ctrl: false, alt_clean: false, clipboard: None, cut: None, #[cfg(target_os = "macos")] menu: None });
}

/// No window is open and tasks still run: the system's own box asks whether to quit all the same.
fn quit_anyway() -> bool {
    let n = crate::tasks::counts().0;
    let r = rfd::MessageDialog::new().set_level(rfd::MessageLevel::Warning).set_title("Quit UFB?").set_description(if n == 1 { "A task is still running. What is done stays done; the rest is stopped.".to_string() } else { format!("{n} tasks are still running or waiting. What is done stays done; the rest is stopped.") }).set_buttons(rfd::MessageButtons::OkCancelCustom("Quit".into(), "Cancel".into())).show();
    matches!(r, rfd::MessageDialogResult::Ok) || r == rfd::MessageDialogResult::Custom("Quit".into())
}

/// macOS: a `ufb:` or `union:` link the system hands to the application (canon rows 519 point 5, 535) - opened from a
/// mail, a chat, a browser, another UFB. The system sends it as an Apple event ("get URL") to the application that is
/// registered for the scheme, whether it was running or was started for it; a handler of our own takes it and puts
/// the link where a second launch's request goes (`instance.rs`), so it is opened the same way. (On Windows the
/// system starts the application with the link as its argument: that IS a second launch.)
#[cfg(target_os = "macos")]
mod links {
    use objc2::rc::Retained; use objc2::runtime::{AnyObject, NSObject}; use objc2::{class, define_class, msg_send, sel, MainThreadMarker, MainThreadOnly};
    use std::sync::{Arc, OnceLock};
    static WAKE: OnceLock<Arc<dyn Fn() + Send + Sync>> = OnceLock::new();
    define_class!(
        #[unsafe(super(NSObject))] #[thread_kind = MainThreadOnly] #[name = "UFBLinkHandler"]
        struct Handler;
        impl Handler {
            #[unsafe(method(handleLink:withReply:))]
            fn handle(&self, event: &AnyObject, _reply: &AnyObject) {
                // the event's direct object ('----') is the address, as text
                let text: Option<String> = unsafe { let d: *mut AnyObject = msg_send![event, paramDescriptorForKeyword: 0x2d2d_2d2du32]; if d.is_null() { None } else { let s: *mut objc2_foundation::NSString = msg_send![d, stringValue]; s.as_ref().map(|s| s.to_string()) } };
                if let Some(t) = text.filter(|t| !t.trim().is_empty()) { crate::instance::ask(t.trim().to_string()); if let Some(w) = WAKE.get() { w(); } }
            }
        }
    );
    pub fn install(wake: Arc<dyn Fn() + Send + Sync>) {
        let Some(mtm) = MainThreadMarker::new() else { return }; let _ = WAKE.set(wake);
        let h: Retained<Handler> = unsafe { msg_send![Handler::alloc(mtm), init] };
        unsafe { let mgr: *mut AnyObject = msg_send![class!(NSAppleEventManager), sharedAppleEventManager]; if mgr.is_null() { return }
            // event class and id 'GURL': "get URL"
            let _: () = msg_send![mgr, setEventHandler: &*h, andSelector: sel!(handleLink:withReply:), forEventClass: 0x4755_524cu32, andEventID: 0x4755_524cu32]; }
        std::mem::forget(h);   // (the manager does not keep its handlers alive: this one lives as long as the application)
    }
}
