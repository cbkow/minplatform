//! The application as it lives on a machine (canon rows 389, 402; spike S14): a window that hides, an icon in the tray
//! or the menu bar with a menu, one way out that kills a render at once, the end of a session, a start of
//! itself again. The node runs on its own threads (`node.rs`); this is the main thread's loop.
//!
//! S14's rules, kept: the tray is made after the window, on the main thread; the loop is told to end ONCE, and woken
//! after that; on macOS the application's own delegate answers a quit with "not yet" and the loop ends itself.
use crate::node::Node;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tray_icon::menu::{accelerator::{Accelerator, Code, Modifiers}, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{TrayIcon, TrayIconBuilder, TrayIconEvent};
use winit31 as winit;
use winit::application::ApplicationHandler;
use winit::event::{ButtonSource, ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowAttributes, WindowId};

/// What reaches the loop from outside it.
pub enum Msg { Show, Menu(&'static str), TrayClick, Second(bool), Quit }
static TX: OnceLock<(Mutex<Sender<Msg>>, EventLoopProxy)> = OnceLock::new();
pub fn send(m: Msg) { if let Some((tx, p)) = TX.get() { let _ = tx.lock().unwrap().send(m); p.wake_up(); } }
fn log(s: impl AsRef<str>) { crate::share::log("INFO", "farm", s.as_ref()); }

struct Shell { node: Arc<Node>, ended: Arc<std::sync::atomic::AtomicBool>, rx: Receiver<Msg>, window: Option<Arc<Box<dyn Window>>>, ui: Option<crate::ui::Ui>, pointer: (f64, f64), /// (shift, the command key - Control on Windows)
    mods: (bool, bool), tray: Option<TrayIcon>, status: Option<MenuItem>, stop: Option<MenuItem>, minimized: bool, shown: (String, bool), left: bool, _app_menu: Option<Menu> }
impl Shell {
    fn show(&self) { if let Some(w) = &self.window { w.set_visible(true); w.set_minimized(false); w.focus_window(); w.request_redraw(); } }
    /// What the tray says: its status line, its tooltip, the name of Stop / Resume - set when it changes, not at every turn.
    fn refresh(&mut self) {
        let (job, stopped) = self.node.state();
        let text = match (&job, stopped) { (Some(j), _) => format!("Rendering {j}"), (None, true) => "Stopped".to_string(), _ => "Idle".to_string() };
        if (text.clone(), stopped) == self.shown { return } self.shown = (text.clone(), stopped);
        if let Some(s) = &self.status { s.set_text(&text); } if let Some(s) = &self.stop { s.set_text(if stopped { "Resume Node" } else { "Stop Node" }); }
        // (Windows cuts a tooltip at 127 characters; a job's id has no such bound)
        if let Some(t) = &self.tray { let tip: String = format!("minRender - {text}").chars().take(120).collect(); let _ = t.set_tooltip(Some(tip)); }
    }
    /// The one way out (row 404): every exit is the same and none asks. The render is killed with its whole tree, the
    /// leader is told, the node leaves the farm, the application ends. (A question while rendering was ruled in row 389
    /// and taken out again: it put a dialog between the owner and the kill that has always simply worked.)
    fn quit(&self) { log(if self.node.state().0.is_some() { "exit: the render is killed" } else { "exit" }); self.node.stop.store(true, Ordering::Relaxed); }
}
impl ApplicationHandler for Shell {
    fn can_create_surfaces(&mut self, el: &dyn ActiveEventLoop) {
        if self.window.is_some() { return }
        self.window = el.create_window(WindowAttributes::default().with_title("minRender").with_surface_size({ let (w, h) = crate::ui::Layout::load(&self.node.data).size.unwrap_or((1440.0, 900.0)); winit::dpi::LogicalSize::new(w, h) }).with_visible(!self.minimized)).ok().map(Arc::new);
        self.ui = self.window.clone().and_then(|w| crate::ui::Ui::new(w, self.node.clone())); if self.ui.is_none() { crate::share::log("WARN", "farm", "the window's content could not be set up (no renderer): the node runs, the window stays empty"); }
        let (status, show, stop, exit) = (MenuItem::new("Idle", false, None), MenuItem::new("Show Window", true, None), MenuItem::new("Stop Node", true, None), MenuItem::new("Exit", true, None));
        let menu = Menu::new(); let _ = menu.append_items(&[&status, &PredefinedMenuItem::separator(), &show, &stop, &PredefinedMenuItem::separator(), &exit]);
        // macOS: the application's own menu, whose Quit goes the same way out as every other (never `terminate:`)
        let quit = MenuItem::new("Quit minRender", true, Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyQ)));
        #[cfg(target_os = "macos")] { let app = Menu::new(); let sub = Submenu::new("minRender", true); let _ = sub.append_items(&[&PredefinedMenuItem::hide(None), &PredefinedMenuItem::separator(), &quit]); let _ = app.append(&sub); app.init_for_nsapp(); self._app_menu = Some(app); }
        #[cfg(not(target_os = "macos"))] { let _ = Submenu::new("", false); }
        let ids = [(show.id().0.clone(), "show"), (stop.id().0.clone(), "stop"), (exit.id().0.clone(), "exit"), (quit.id().0.clone(), "exit")];
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| { if let Some((_, n)) = ids.iter().find(|(id, _)| *id == e.id.0) { send(Msg::Menu(n)); } }));
        TrayIconEvent::set_event_handler(Some(|e: TrayIconEvent| { if let TrayIconEvent::Click { button: tray_icon::MouseButton::Left, button_state: tray_icon::MouseButtonState::Up, .. } = e { send(Msg::TrayClick); } }));
        // minRender's own icon, as 0.5.x shows it: black on clear for the macOS menu bar (a template: the system tints it), the colour mark on Windows
        let (px, iw, ih) = { let bytes: &[u8] = if cfg!(target_os = "macos") { include_bytes!("../assets/icons/tray-macos.png") } else { include_bytes!("../assets/icons/tray-windows.png") };
            let mut d = png::Decoder::new(std::io::Cursor::new(bytes)); d.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16); let mut r = d.read_info().expect("the bundled icon"); let mut buf = vec![0u8; r.output_buffer_size()]; let info = r.next_frame(&mut buf).expect("the bundled icon"); buf.truncate(info.buffer_size());
            let rgba: Vec<u8> = match info.color_type { png::ColorType::Rgba => buf, png::ColorType::Rgb => buf.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(), png::ColorType::GrayscaleAlpha => buf.chunks(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(), _ => buf.iter().flat_map(|g| [*g, *g, *g, 255]).collect() };
            (rgba, info.width, info.height) };
        match tray_icon::Icon::from_rgba(px, iw, ih).ok().and_then(|icon| TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip("minRender").with_icon(icon).with_icon_as_template(cfg!(target_os = "macos")).with_menu_on_left_click(cfg!(target_os = "macos")).build().ok()) {
            Some(t) => self.tray = Some(t), None => crate::share::log("WARN", "farm", "no tray icon could be made: closing the window will minimise it") }
        self.status = Some(status); self.stop = Some(stop); self.refresh();
    }
    fn window_event(&mut self, _el: &dyn ActiveEventLoop, _id: WindowId, e: WindowEvent) {
        let visible = self.window.as_ref().is_some_and(|w| w.is_visible() != Some(false)); let redraw = |me: &Shell| if let Some(w) = &me.window { w.request_redraw(); };
        match e {
            WindowEvent::CloseRequested => { if let Some(w) = &self.window { if self.tray.is_some() { w.set_visible(false); } else { w.set_minimized(true); } } }
            // (a hidden window draws nothing: the node's work is not the window's)
            WindowEvent::RedrawRequested => if visible { if let Some(ui) = self.ui.as_mut() { ui.redraw(&self.node.view()); if ui.animating() { redraw(self); } } },
            WindowEvent::SurfaceResized(sz) => { if let Some(ui) = self.ui.as_mut() { ui.resize(sz.width, sz.height); } redraw(self); }
            WindowEvent::ScaleFactorChanged { .. } => redraw(self),
            WindowEvent::MouseWheel { delta, .. } => { let s = self.window.as_ref().map(|w| w.scale_factor()).unwrap_or(1.0); let dy = match delta { MouseScrollDelta::LineDelta(_, y) => y as f64 * 60.0 * s, MouseScrollDelta::PixelDelta(p) => p.y, _ => 0.0 }; if let Some(ui) = self.ui.as_mut() { ui.wheel(-dy); } redraw(self); }
            WindowEvent::ModifiersChanged(m) => { let st = m.state(); self.mods = (st.shift_key(), if cfg!(target_os = "macos") { st.meta_key() } else { st.control_key() }); }
            WindowEvent::PointerMoved { position, .. } => { self.pointer = (position.x, position.y); if let Some(ui) = self.ui.as_mut() { ui.pointer(&min_ui::Event::Move { x: position.x, y: position.y }, &self.node.view(), self.mods); } redraw(self); }
            WindowEvent::PointerButton { state, position, button: ButtonSource::Mouse(b @ (MouseButton::Left | MouseButton::Right)), .. } => { let (x, y) = (position.x, position.y); let button = if b == MouseButton::Left { min_ui::Button::Primary } else { min_ui::Button::Secondary };
                let ev = if state == ElementState::Pressed { min_ui::Event::Down { x, y, button } } else { min_ui::Event::Up { x, y, button } };
                if let Some(ui) = self.ui.as_mut() { ui.pointer(&ev, &self.node.view(), self.mods); } redraw(self); }
            WindowEvent::PointerLeft { .. } => { if let Some(ui) = self.ui.as_mut() { ui.pointer(&min_ui::Event::Leave, &self.node.view(), self.mods); } redraw(self); }
            WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() => { use winit::keyboard::{Key, NamedKey}; use min_ui::{Key as K, Named as N};
                let key = match &event.logical_key { Key::Named(NamedKey::ArrowUp) => K::Named(N::Up), Key::Named(NamedKey::ArrowDown) => K::Named(N::Down), Key::Named(NamedKey::ArrowLeft) => K::Named(N::Left), Key::Named(NamedKey::ArrowRight) => K::Named(N::Right), Key::Named(NamedKey::Home) => K::Named(N::Home), Key::Named(NamedKey::End) => K::Named(N::End),
                    Key::Named(NamedKey::Escape) => K::Named(N::Escape), Key::Named(NamedKey::Enter) => K::Named(N::Enter), Key::Named(NamedKey::Tab) => K::Named(N::Tab), Key::Named(NamedKey::Delete) => K::Named(N::Delete), Key::Named(NamedKey::Backspace) => K::Named(N::Backspace),
                    Key::Named(NamedKey::PageUp) => K::Char("PageUp".into()), Key::Named(NamedKey::PageDown) => K::Char("PageDown".into()), Key::Character(c) => K::Char(c.to_string()), _ => K::Char(String::new()) };
                let ev = min_ui::KeyEvent { key, text: event.text.as_ref().map(|t| t.to_string()).filter(|_| !self.mods.1), shift: self.mods.0, cmd: self.mods.1 };
                if let Some(ui) = self.ui.as_mut() { ui.key_event(&ev, &self.node.view(), self.mods); } redraw(self); }
            _ => {}
        }
    }
    fn proxy_wake_up(&mut self, _el: &dyn ActiveEventLoop) {
        while let Ok(m) = self.rx.try_recv() { match m {
            Msg::Show | Msg::Menu("show") => self.show(),
            Msg::Menu("stop") => { let stopped = self.node.state().1; self.node.set_stopped(!stopped); self.refresh(); }
            Msg::Menu(_) => self.quit(),
            Msg::TrayClick => if !cfg!(target_os = "macos") { self.show() },
            Msg::Second(minimized) => if !minimized { self.show() },
            Msg::Quit => self.quit(),
        } }
    }
    fn about_to_wait(&mut self, el: &dyn ActiveEventLoop) {
        if self.left { return }
        // another node, or the window, asked for a new start: a copy of this program takes over when this one is gone
        // (started again with its window as it is now: hidden stays hidden, shown comes back shown)
        if self.node.restart_asked.swap(false, Ordering::Relaxed) { crate::restart(self.window.as_ref().is_none_or(|w| w.is_visible() == Some(false))); self.node.stop.store(true, Ordering::Relaxed); }
        if self.ended.load(Ordering::Relaxed) { self.left = true; el.exit(); if let Some((_, p)) = TX.get() { p.wake_up(); } return }
        // the window follows the farm four times a second while it shows
        self.refresh(); if let Some(w) = &self.window { if w.is_visible() != Some(false) { w.request_redraw(); } } el.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(250)));
    }
}
/// Runs the application until the node has stopped. `ended` is set by the node's thread when its shutdown is done.
pub fn run(node: Arc<Node>, ended: Arc<std::sync::atomic::AtomicBool>, minimized: bool) {
    let el = EventLoop::new().expect("the event loop"); let (tx, rx) = channel(); let _ = TX.set((Mutex::new(tx), el.create_proxy()));
    os::install();
    let _ = el.run_app(Shell { node, ended, rx, window: None, ui: None, pointer: (0.0, 0.0), mods: (false, false), tray: None, status: None, stop: None, minimized, shown: (String::new(), false), left: false, _app_menu: None });
}
/// With a package's identity (a Store install), started by its startup task at sign-in: the application starts hidden.
pub fn started_at_sign_in() -> bool { os::started_at_sign_in() }

#[cfg(target_os = "macos")]
mod os {
    use objc2::rc::Retained; use objc2::runtime::ProtocolObject; use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{NSApplication, NSApplicationDelegate, NSApplicationTerminateReply}; use objc2_foundation::{NSObject, NSObjectProtocol};
    define_class!(
        #[unsafe(super(NSObject))] #[thread_kind = MainThreadOnly] #[name = "MinRenderAppDelegate"]
        struct Delegate;
        unsafe impl NSObjectProtocol for Delegate {}
        unsafe impl NSApplicationDelegate for Delegate {
            // the Dock's Quit, a log-out, a shutdown, an updater: the answer is "not yet", and the loop ends itself after the node's shutdown
            #[unsafe(method(applicationShouldTerminate:))]
            fn should_terminate(&self, _app: &NSApplication) -> NSApplicationTerminateReply { super::send(super::Msg::Quit); NSApplicationTerminateReply::TerminateCancel }
            // a click on the Dock's icon while no window shows
            #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
            fn reopen(&self, _app: &NSApplication, visible: bool) -> bool { if !visible { super::send(super::Msg::Show); } true }
        }
    );
    pub fn install() { let Some(mtm) = MainThreadMarker::new() else { return }; let d: Retained<Delegate> = unsafe { msg_send![super(Delegate::alloc(mtm).set_ivars(())), init] }; NSApplication::sharedApplication(mtm).setDelegate(Some(ProtocolObject::from_ref(&*d))); std::mem::forget(d); }
    pub fn started_at_sign_in() -> bool { false }
}
#[cfg(windows)]
mod os {
    use windows::core::{w, PCWSTR}; use windows::Win32::Foundation::*; use windows::Win32::UI::WindowsAndMessaging::*;
    // a top-level window of the application's own, never shown: Windows asks every top-level window whether the session may end
    unsafe extern "system" fn proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT { match m {
        WM_QUERYENDSESSION => LRESULT(1),
        WM_ENDSESSION => { if w.0 != 0 { super::send(super::Msg::Quit); } LRESULT(0) }
        _ => unsafe { DefWindowProcW(h, m, w, l) } } }
    fn packaged() -> bool { let mut n = 0u32; unsafe { let _ = windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName(&mut n, None); } n != 0 }
    pub fn install() { unsafe { let inst = windows::Win32::System::LibraryLoader::GetModuleHandleW(PCWSTR::null()).unwrap_or_default(); let class = w!("MinRenderSessionWindow");
        let wc = WNDCLASSW { lpfnWndProc: Some(proc), hInstance: inst.into(), lpszClassName: class, ..Default::default() }; RegisterClassW(&wc);
        let _ = CreateWindowExW(WINDOW_EX_STYLE(0), class, w!("minRender session"), WS_OVERLAPPED, 0, 0, 0, 0, None, None, Some(inst.into()), None);
        // a Store update closes the application and starts it again, hidden
        if packaged() { let _ = windows::Win32::System::Recovery::RegisterApplicationRestart(w!("--minimized"), windows::Win32::System::Recovery::REGISTER_APPLICATION_RESTART_FLAGS(0)); } } }
    pub fn started_at_sign_in() -> bool { packaged() && windows::ApplicationModel::AppInstance::GetActivatedEventArgs().and_then(|a| a.Kind()).is_ok_and(|k| k == windows::ApplicationModel::Activation::ActivationKind::StartupTask) }
}
#[cfg(not(any(target_os = "macos", windows)))]
mod os { pub fn install() {} pub fn started_at_sign_in() -> bool { false } }
