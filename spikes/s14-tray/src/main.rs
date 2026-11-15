//! S14 - the life of a tray application on winit 0.31. One windowed program; everything it sees goes to a log.
//!   s14 [--minimized] [--log FILE] [--wait-pid N]
//! S14_SCRIPT="hide@1,show@2,terminate@3,..." does things at given seconds, so that a run needs no hand:
//!   hide | show | rendering | idle | terminate (macOS: what Cmd+Q, the Dock's Quit and a log-out do) |
//!   endsession (Windows: what a sign-out sends) | restart | exit
//! "rendering" makes a quit ASK (canon row 389's exit rule): here the answer is always "Exit after this chunk", which
//! the spike plays as half a second of work before it leaves.
#![cfg_attr(windows, windows_subsystem = "windows")]
use std::io::{Read, Write};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder, TrayIconEvent};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowAttributes, WindowId};

const PORT: u16 = 47814;   // the one-instance endpoint of the spike (the product lifts QCView's local socket)
static LOG: OnceLock<Mutex<std::fs::File>> = OnceLock::new();
static T0: OnceLock<Instant> = OnceLock::new();
fn log(s: impl AsRef<str>) { let line = format!("{:7.3} [{}] {}", T0.get().map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0), std::process::id(), s.as_ref()); if let Some(f) = LOG.get() { let mut f = f.lock().unwrap(); let _ = writeln!(f, "{line}"); let _ = f.flush(); } }

/// What reaches the loop from outside it.
enum Msg { Menu(String), TrayClick, Second(bool), QuitAsked(&'static str), SessionEnd, Script(String) }
static TX: OnceLock<(Mutex<Sender<Msg>>, EventLoopProxy)> = OnceLock::new();
fn send(m: Msg) { if let Some((tx, p)) = TX.get() { let _ = tx.lock().unwrap().send(m); p.wake_up(); } }

struct App { rx: Receiver<Msg>, window: Option<Box<dyn Window>>, tray: Option<TrayIcon>, items: Option<(MenuItem, MenuItem, MenuItem, MenuItem)>, minimized: bool, rendering: bool, stopped: bool, leaving: Option<Instant>, left: bool }
impl App {
    fn show(&mut self, why: &str) { if let Some(w) = &self.window { w.set_visible(true); w.set_minimized(false); w.focus_window(); log(format!("window shown ({why}); visible {:?}", w.is_visible())); } }
    fn hide(&mut self, why: &str) { if let Some(w) = &self.window { w.set_visible(false); log(format!("window hidden ({why}); visible {:?}", w.is_visible())); } }
    /// The one exit rule: while rendering, a quit asks; here it is answered "after this chunk".
    fn quit(&mut self, from: &str) { if self.leaving.is_some() { return } if self.rendering { log(format!("quit from {from}: rendering, so the question is put; answered 'exit after this chunk'")); self.leaving = Some(Instant::now() + Duration::from_millis(500)); } else { log(format!("quit from {from}: idle, leaving now")); self.leaving = Some(Instant::now()); } }
    fn status(&self) -> String { if self.rendering { "Rendering a chunk".into() } else if self.stopped { "Stopped".into() } else { "Idle".into() } }
    fn refresh(&self) { if let Some((status, _, stop, _)) = &self.items { status.set_text(self.status()); stop.set_text(if self.stopped { "Resume Node" } else { "Stop Node" }); } if let Some(t) = &self.tray { let _ = t.set_tooltip(Some(format!("minRender - {}", self.status()))); } }
}
impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, el: &dyn ActiveEventLoop) {
        if self.window.is_some() { return }
        let w = el.create_window(WindowAttributes::default().with_title("S14").with_visible(!self.minimized)).expect("window");
        log(format!("window made; started minimized {}; visible {:?}", self.minimized, w.is_visible())); self.window = Some(w);
        // the tray is made here: after the application has finished launching, on the main thread
        let (status, show, stop, exit) = (MenuItem::new("Idle", false, None), MenuItem::new("Show Window", true, None), MenuItem::new("Stop Node", true, None), MenuItem::new("Exit", true, None));
        let menu = Menu::new(); let _ = menu.append_items(&[&status, &PredefinedMenuItem::separator(), &show, &stop, &PredefinedMenuItem::separator(), &exit]);
        let ids = [(show.id().0.clone(), "show"), (stop.id().0.clone(), "stop"), (exit.id().0.clone(), "exit")];
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| { if let Some((_, n)) = ids.iter().find(|(id, _)| *id == e.id.0) { send(Msg::Menu(n.to_string())); } }));
        TrayIconEvent::set_event_handler(Some(|e: TrayIconEvent| { if let TrayIconEvent::Click { button: tray_icon::MouseButton::Left, button_state: tray_icon::MouseButtonState::Up, .. } = e { send(Msg::TrayClick); } }));
        let px: Vec<u8> = (0..32 * 32).flat_map(|i| { let (x, y) = (i % 32, i / 32); let on = (x as i32 - 16).pow(2) + (y as i32 - 16).pow(2) < 150; [0, 0, 0, if on { 255 } else { 0 }] }).collect();
        let icon = tray_icon::Icon::from_rgba(px, 32, 32).expect("icon");
        // macOS: a template image, and a click opens the menu only (ruled); Windows: the left click shows the window, the right one the menu
        match TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip("minRender - Idle").with_icon(icon).with_icon_as_template(true).with_menu_on_left_click(cfg!(target_os = "macos")).build() { Ok(t) => { log("tray icon made"); self.tray = Some(t); } Err(e) => log(format!("TRAY FAILED: {e}")) }
        self.items = Some((status, show, stop, exit));
    }
    fn window_event(&mut self, _el: &dyn ActiveEventLoop, _id: WindowId, e: WindowEvent) {
        match e { WindowEvent::CloseRequested => { if self.tray.is_some() { self.hide("its close button") } else if let Some(w) = &self.window { w.set_minimized(true); log("no tray: closing minimises"); } } WindowEvent::Focused(f) => log(format!("window focused {f}")), _ => {} }
    }
    fn proxy_wake_up(&mut self, _el: &dyn ActiveEventLoop) {
        while let Ok(m) = self.rx.try_recv() { match m {
            Msg::Menu(n) => { log(format!("tray menu: {n}")); match n.as_str() { "show" => self.show("the tray's menu"), "stop" => { self.stopped = !self.stopped; self.refresh(); } _ => self.quit("the tray's Exit") } }
            Msg::TrayClick => { log("tray icon clicked"); if !cfg!(target_os = "macos") { self.show("a click on the tray icon"); } }
            Msg::Second(min) => { log(format!("a second launch arrived (minimized: {min})")); if !min { self.show("a second launch"); } }
            Msg::QuitAsked(from) => self.quit(from),
            Msg::SessionEnd => { log("the session is ending: the render is killed, the leader told, leaving"); self.leaving = Some(Instant::now()); }
            Msg::Script(s) => { log(format!("script: {s}")); match s.as_str() { "hide" => self.hide("the script"), "show" => self.show("the script"), "rendering" => { self.rendering = true; self.refresh(); } "idle" => { self.rendering = false; self.refresh(); }
                "terminate" => os::terminate(), "endsession" => os::end_session(), "restart" => { restart(self.minimized); self.leaving = Some(Instant::now()); } _ => self.quit("the script") } }
        } }
    }
    fn about_to_wait(&mut self, el: &dyn ActiveEventLoop) {
        // (the loop is told to end ONCE: on macOS each `exit` wakes the loop again, and asking at every turn keeps it turning)
        if self.left { return }
        match self.leaving { Some(t) if Instant::now() >= t => { log("shutdown work done; the loop ends"); self.left = true; el.exit(); } Some(t) => el.set_control_flow(ControlFlow::WaitUntil(t)), None => el.set_control_flow(ControlFlow::Wait) }
    }
}
/// This program starts itself again: a copy that waits for this one to be gone, then is the application.
fn restart(minimized: bool) {
    let mut c = std::process::Command::new(std::env::current_exe().unwrap()); c.args(["--wait-pid", &std::process::id().to_string()]); if minimized { c.arg("--minimized"); }
    if let Some(l) = std::env::args().skip_while(|a| a != "--log").nth(1) { c.args(["--log", &l]); }
    c.env_remove("S14_SCRIPT").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)] { use std::os::windows::process::CommandExt; c.creation_flags(0x0000_0008 | 0x0800_0000); }   // DETACHED_PROCESS | CREATE_NO_WINDOW; no job breakaway is asked for
    #[cfg(unix)] { use std::os::unix::process::CommandExt; c.process_group(0); }
    match c.spawn() { Ok(ch) => log(format!("restart: a copy was started (pid {})", ch.id())), Err(e) => log(format!("RESTART FAILED: {e}")) }
}
fn alive(pid: u32) -> bool {
    #[cfg(unix)] { std::process::Command::new("kill").args(["-0", &pid.to_string()]).stderr(std::process::Stdio::null()).status().map(|s| s.success()).unwrap_or(false) }
    #[cfg(windows)] { use windows::Win32::{Foundation::*, System::Threading::*}; unsafe { let Ok(h) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) else { return false }; let r = WaitForSingleObject(h, 0) != WAIT_OBJECT_0; let _ = CloseHandle(h); r } }
}

#[cfg(target_os = "macos")]
mod os {
    use objc2::rc::Retained; use objc2::runtime::ProtocolObject; use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{NSApplication, NSApplicationDelegate, NSApplicationTerminateReply}; use objc2_foundation::{NSObject, NSObjectProtocol};
    define_class!(
        #[unsafe(super(NSObject))] #[thread_kind = MainThreadOnly] #[name = "S14AppDelegate"]
        struct Delegate;
        unsafe impl NSObjectProtocol for Delegate {}
        unsafe impl NSApplicationDelegate for Delegate {
            // Cmd+Q, the Dock's Quit, a log-out, an updater: all come here. The answer is "not yet"; the loop ends itself.
            #[unsafe(method(applicationShouldTerminate:))]
            fn should_terminate(&self, _app: &NSApplication) -> NSApplicationTerminateReply { super::log("applicationShouldTerminate: asked"); super::send(super::Msg::QuitAsked("the system (terminate:)")); NSApplicationTerminateReply::TerminateCancel }
            // a click on the Dock icon while no window shows
            #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
            fn reopen(&self, _app: &NSApplication, visible: bool) -> bool { super::log(format!("Dock reopen (visible windows: {visible})")); if !visible { super::send(super::Msg::Second(false)); } true }
        }
    );
    pub fn install() { let Some(mtm) = MainThreadMarker::new() else { return }; let d: Retained<Delegate> = unsafe { msg_send![super(Delegate::alloc(mtm).set_ivars(())), init] }; NSApplication::sharedApplication(mtm).setDelegate(Some(ProtocolObject::from_ref(&*d))); std::mem::forget(d); super::log("delegate installed"); }
    pub fn terminate() { if let Some(mtm) = MainThreadMarker::new() { NSApplication::sharedApplication(mtm).terminate(None); } }
    pub fn end_session() { super::log("endsession is Windows' message; on macOS a log-out is terminate:"); }
    pub fn startup_kind() -> String { "n/a".into() }
}
#[cfg(windows)]
mod os {
    use windows::core::{w, PCWSTR}; use windows::Win32::Foundation::*; use windows::Win32::UI::WindowsAndMessaging::*; use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    static HIDDEN: std::sync::OnceLock<isize> = std::sync::OnceLock::new();
    // a top-level window of the application's own, never shown: Windows asks every top-level window whether the session may end
    unsafe extern "system" fn proc(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT { match m {
        WM_QUERYENDSESSION => { super::log("WM_QUERYENDSESSION: yes"); LRESULT(1) }
        WM_ENDSESSION => { super::log(format!("WM_ENDSESSION (ending: {})", w.0 != 0)); if w.0 != 0 { super::send(super::Msg::SessionEnd); } LRESULT(0) }
        _ => unsafe { DefWindowProcW(h, m, w, l) } } }
    pub fn install() { unsafe { let inst = GetModuleHandleW(PCWSTR::null()).unwrap_or_default(); let class = w!("S14SessionWindow");
        let wc = WNDCLASSW { lpfnWndProc: Some(proc), hInstance: inst.into(), lpszClassName: class, ..Default::default() }; RegisterClassW(&wc);
        match CreateWindowExW(WINDOW_EX_STYLE(0), class, w!("s14 session"), WS_OVERLAPPED, 0, 0, 0, 0, None, None, Some(inst.into()), None) { Ok(h) => { let _ = HIDDEN.set(h.0 as isize); super::log("the session window is made"); } Err(e) => super::log(format!("SESSION WINDOW FAILED: {e}")) }
        // a Store update closes the application and starts it again with these arguments, when it runs with a package's identity
        let r = windows::Win32::System::Recovery::RegisterApplicationRestart(w!("--minimized"), windows::Win32::System::Recovery::REGISTER_APPLICATION_RESTART_FLAGS(0)); super::log(format!("RegisterApplicationRestart: {}", if r.is_ok() { "ok".into() } else { format!("{r:?}") })); } }
    pub fn terminate() { super::log("terminate: is macOS's; on Windows the window's close and the session's end are the ways out"); }
    pub fn end_session() { if let Some(h) = HIDDEN.get() { unsafe { let h = HWND(*h as *mut _); let q = SendMessageW(h, WM_QUERYENDSESSION, Some(WPARAM(0)), Some(LPARAM(0))); super::log(format!("the query was answered {}", q.0)); SendMessageW(h, WM_ENDSESSION, Some(WPARAM(1)), Some(LPARAM(0))); } } }
    /// Whether this process has a package's identity (a Store install) - there the startup task starts it with no arguments.
    pub fn startup_kind() -> String { let mut n = 0u32; unsafe { let _ = windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName(&mut n, None); } if n == 0 { "unpackaged".into() } else { "packaged".into() } }
}
#[cfg(not(any(target_os = "macos", windows)))]
mod os { pub fn install() {} pub fn terminate() {} pub fn end_session() {} pub fn startup_kind() -> String { "n/a".into() } }

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect(); let val = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1).cloned());
    let _ = T0.set(Instant::now());
    if let Some(f) = val("--log").and_then(|p| std::fs::OpenOptions::new().create(true).append(true).open(p).ok()) { let _ = LOG.set(Mutex::new(f)); }
    let minimized = args.iter().any(|a| a == "--minimized");
    // started by a copy that is leaving: wait until it is gone
    if let Some(pid) = val("--wait-pid").and_then(|p| p.parse::<u32>().ok()) { let t = Instant::now(); while alive(pid) && t.elapsed() < Duration::from_secs(15) { std::thread::sleep(Duration::from_millis(50)); } log(format!("the former instance ({pid}) was gone after {:.2} s", t.elapsed().as_secs_f64())); }
    // one instance: the first listens; a later one says how it was started and leaves
    let listener = match std::net::TcpListener::bind(("127.0.0.1", PORT)) { Ok(l) => l, Err(_) => {
        #[cfg(windows)] unsafe { let _ = windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(u32::MAX); }   // ASFW_ANY: the first instance may take the foreground
        match std::net::TcpStream::connect(("127.0.0.1", PORT)) { Ok(mut s) => { let _ = s.write_all(if minimized { b"minimized" } else { b"show" }); log(format!("second launch (minimized: {minimized}): handed to the first, leaving")); } Err(e) => log(format!("second launch: the first did not answer ({e})")) } return } };
    log(format!("started; args {args:?}; {}", os::startup_kind()));
    let el = EventLoop::new().unwrap(); let (tx, rx) = channel(); let _ = TX.set((Mutex::new(tx), el.create_proxy()));
    os::install();
    std::thread::spawn(move || for s in listener.incoming().flatten() { let mut s = s; let mut b = [0u8; 16]; let n = s.read(&mut b).unwrap_or(0); send(Msg::Second(&b[..n] == b"minimized")); });
    if let Ok(script) = std::env::var("S14_SCRIPT") { std::thread::spawn(move || { let t = Instant::now(); let mut steps: Vec<(f64, String)> = script.split(',').filter_map(|s| s.split_once('@').map(|(a, b)| (b.parse().unwrap_or(0.0), a.to_string()))).collect(); steps.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (at, what) in steps { let d = at - t.elapsed().as_secs_f64(); if d > 0.0 { std::thread::sleep(Duration::from_secs_f64(d)); } send(Msg::Script(what)); } }); }
    let r = el.run_app(App { rx, window: None, tray: None, items: None, minimized, rendering: false, stopped: false, leaving: None, left: false });
    log(format!("the loop returned ({r:?}); the process ends normally"));
}
