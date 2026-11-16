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

struct Win { window: Arc<Box<dyn Window>>, ui: Ui }
struct App {
    first: Option<PathBuf>, favorites: Vec<Place>, proxy: Arc<EventLoopProxy>, windows: HashMap<WindowId, Win>, focused: Option<WindowId>,
    /// (shift, the command key - Ctrl on Windows -, Alt / Option)
    mods: (bool, bool, bool), /// the Ctrl key itself (macOS: Ctrl+Tab steps through the tabs, row 480)
    ctrl: bool, alt_clean: bool, clipboard: Option<arboard::Clipboard>,
    /// the files that were CUT (Windows' habit): pasted, they are moved
    cut: Option<Vec<PathBuf>>,
    #[cfg(target_os = "macos")] menu: Option<crate::menu::AppMenu>,
}
#[cfg(target_os = "macos")]
static MENU_EVENTS: std::sync::Mutex<Vec<muda::MenuEvent>> = std::sync::Mutex::new(Vec::new());

/// The window's size in logical px: `UFB_SIZE=1440x900`, else 1280 x 800.
fn wanted_size() -> (f64, f64) { std::env::var("UFB_SIZE").ok().and_then(|v| v.split_once('x').and_then(|(a, b)| Some((a.trim().parse::<f64>().ok()?, b.trim().parse::<f64>().ok()?)))).filter(|(w, h)| *w >= 320.0 && *h >= 240.0).unwrap_or((1280.0, 800.0)) }

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

impl App {
    /// What the drag-and-drop layer calls: it answers from what the window last drew (`dnd::on_notice`), asks for a
    /// frame, and wakes the loop for what a drop left to do. Nothing of the application is borrowed by it.
    fn sink(ui: &Ui, window: &Arc<Box<dyn Window>>, proxy: &Arc<EventLoopProxy>) -> min_dnd::Sink {
        let (shared, window, proxy) = (ui.dnd.clone(), window.clone(), proxy.clone());
        std::rc::Rc::new(move |n| { let before = shared.borrow().hover; let op = crate::dnd::on_notice(&shared, n); window.request_redraw(); proxy.wake_up();
            // Windows, a drag of OUR OWN: the system's drag runs inside the pointer's event, and the redraws asked for wait
            // until it has returned - so the outline of the place under the pointer is drawn here, at once (row 478)
            if before != shared.borrow().hover { let id = window.id(); let ui = OWN_DRAG.with(|d| d.borrow().iter().find(|(w, _)| *w == id).map(|(_, u)| *u)); if let Some(ui) = ui { unsafe { (*ui).redraw(); } } }
            op })
    }
    fn open(&mut self, el: &dyn ActiveEventLoop, path: PathBuf) {
        if std::env::var_os("UFB_DEMO").is_some() { eprintln!("UFB_DEMO: a window on {}", path.display()); }
        let (w, h) = wanted_size();
        #[allow(unused_mut)] let mut attrs = WindowAttributes::default().with_title("UFB").with_surface_size(winit::dpi::LogicalSize::new(w, h)).with_min_surface_size(winit::dpi::LogicalSize::new(480.0, 320.0));
        // Windows: the caption takes the menu row's colour, so the window is one surface from its top edge down (as minNotes');
        // and it is made unseen, to be shown after its first frame
        #[cfg(windows)] {
            use winit::platform::windows::WindowAttributesWindows; use min_theme::tokens as tk;
            let c8 = |c: min_theme::Color| winit::platform::windows::Color::from_rgb((c.r * 255.0).round() as u8, (c.g * 255.0).round() as u8, (c.b * 255.0).round() as u8);
            attrs = attrs.with_theme(Some(winit::window::Theme::Dark));
            // (winit's own drag and drop is OFF: a window has one drop target, and it is ours - `min-dnd`)
            attrs = attrs.with_platform_attributes(Box::new(WindowAttributesWindows::default().with_drag_and_drop(false).with_title_background_color(Some(c8(tk::MENU_BAR_BACKGROUND_COLOR))).with_title_text_color(c8(tk::MENU_BAR_TITLE_COLOR))));
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
        window.request_redraw(); let id = window.id(); self.focused = Some(id); self.windows.insert(id, Win { window, ui });
    }
    /// What the windows asked for: a new window, to close, to minimise, to quit. The last window closed ends the application.
    fn settle(&mut self, el: &dyn ActiveEventLoop) {
        let mut asks: Vec<(WindowId, Out)> = vec![]; let mut done = false;
        for (id, w) in self.windows.iter_mut() { for o in w.ui.out.drain(..) { asks.push((*id, o)); } done |= w.ui.done; }
        for (id, o) in asks { match o {
            Out::NewWindow(path) => self.open(el, path),
            Out::Close => { self.windows.remove(&id); if self.focused == Some(id) { self.focused = None; } }
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
            Out::FieldPaste => { if self.clipboard.is_none() { self.clipboard = arboard::Clipboard::new().ok(); }
                if let (Some(cb), Some(w)) = (self.clipboard.as_mut(), self.windows.get_mut(&id)) { if let Ok(text) = cb.get_text() { w.ui.paste(&text); } w.window.request_redraw(); } }
            // the system's menu: handed to the run loop (never shown from inside this callback - S19); a row of ours comes back through `proxy_wake_up`
            Out::Menu(ask) => { #[cfg(target_os = "macos")] if let Some(w) = self.windows.get(&id) { use raw_window_handle::{HasWindowHandle, RawWindowHandle};
                    if let Ok(RawWindowHandle::AppKit(h)) = w.window.window_handle().map(|h| h.as_raw()) { let proxy = self.proxy.clone(); crate::ctxmenu::sys::show(h.ns_view.as_ptr(), id, ask, Arc::new(move || proxy.wake_up())); } }
                #[cfg(not(target_os = "macos"))] { let _ = (id, ask); } }
        } }
        if done || self.windows.is_empty() { self.windows.clear(); el.exit(); }
        #[cfg(target_os = "macos")] if let (Some(m), Some(w)) = (self.menu.as_mut(), self.focused.and_then(|id| self.windows.get(&id))) { m.sync(w.ui.menu_state()); m.set_undo(crate::ops::undo_label()); }
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
        if self.windows.is_empty() { el.exit(); }
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
            WindowEvent::Focused(on) => { if on { self.focused = Some(id); w.ui.activated(); } else { w.ui.deactivated(); self.alt_clean = false; } }
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
        #[cfg(target_os = "macos")] { let events: Vec<muda::MenuEvent> = std::mem::take(&mut *MENU_EVENTS.lock().unwrap());
            for ev in events { if let (Some(c), Some(w)) = (Cmd::from_id(&ev.id.0), self.focused.and_then(|id| self.windows.get_mut(&id)).or(None)) { w.ui.command(c); w.window.request_redraw(); } else if Cmd::from_id(&ev.id.0) == Some(Cmd::Quit) { self.windows.clear(); el.exit(); return } } }
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
    #[cfg(target_os = "macos")] { let p = proxy.clone(); muda::MenuEvent::set_event_handler(Some(move |ev: muda::MenuEvent| { MENU_EVENTS.lock().unwrap().push(ev); p.wake_up(); })); }
    let _ = el.run_app(App { first: Some(folder), favorites, proxy, windows: HashMap::new(), focused: None, mods: (false, false, false), ctrl: false, alt_clean: false, clipboard: None, cut: None, #[cfg(target_os = "macos")] menu: None });
}
