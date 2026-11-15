//! S15 - the system's own web view as a CHILD inside a rectangle of a winit 0.31 window drawn with wgpu / vello
//! (docs/spikes/S15-webview.md). WKWebView on macOS, WebView2 on Windows, both through `wry`; no engine is bundled.
//!
//! `S15_SCRIPT=all target/s15/release/s15` runs the whole pass bar by itself (about 40 s, one window, no input sent) and
//! prints its findings; they are also written to `target/s15-out/result.txt`. With no script the window stays for a walk
//! by hand: the page in the middle, our strip above with Close, our strip below with the path.
//! `S15_OUT` moves the output folder (default `target/s15-out`, ignored by git). The test site is generated there.
//!
//! The Windows code (`cfg(windows)`) was type-checked from macOS (`cargo check --target x86_64-pc-windows-msvc`) and has
//! NEVER been run. `--no-default-features` builds the same program with no web view (the size comparison).
use kurbo::{Affine, Rect};
use min_present::{Output, Presenter};
use min_ui::{fonts::draw_layout, Family, Fonts, TextButton, UiLayer, Variant};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{ButtonSource, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

type Win = Arc<Box<dyn Window>>;
/// What the page and the web view's callbacks told the host, in order.
type Msgs = Rc<RefCell<Vec<(Instant, String)>>>;

// ---------------------------------------------------------------------------------------------------------------------
// (the clip: 2 s of ffmpeg's `testsrc`, 160x90, H.264 baseline in MP4, 9 KB; kept as `.bin` because the repository ignores *.mp4)
// The test site: <out>/site/page/{index.html, beside.png, beside.txt, clip.mp4} and <out>/site/secret/{outside.png, outside.txt}
// ---------------------------------------------------------------------------------------------------------------------
const PAGE: &str = r#"<!doctype html><html><head><meta charset="utf-8"><title>s15 page</title>
<style>html,body{margin:0;height:100%;overflow:hidden;background:#00c060;font:14px sans-serif;color:#012}
#row{position:absolute;left:40px;top:40px;display:flex;gap:16px;align-items:flex-start}
#out{position:absolute;left:40px;top:150px;color:#012}</style></head>
<body><div id="row"><video id="v" width="160" height="90" autoplay muted loop playsinline></video><canvas id="c" width="90" height="90"></canvas>
<img id="beside" width="32" height="32"><img id="outside" width="32" height="32"></div>
<a id="out" href="https://example.com/">a link to an outside address</a>
<script>
const post = m => window.s15post(m);
document.title = 's15-title-' + (6 * 7);
post('script:ran');
for (const [id, src] of [['beside', 'beside.png'], ['outside', '../secret/outside.png']]) {
  const i = document.getElementById(id);
  i.onload = () => post('img:' + id + ':ok:' + i.naturalWidth + 'x' + i.naturalHeight);
  i.onerror = () => post('img:' + id + ':fail');
  i.src = src;
}
for (const [id, src] of [['beside', 'beside.txt'], ['outside', '../secret/outside.txt']]) {
  fetch(src).then(r => r.ok ? r.text() : Promise.reject('status ' + r.status)).then(t => post('fetch:' + id + ':ok:' + t.trim())).catch(e => post('fetch:' + id + ':fail:' + e));
}
const v = document.getElementById('v');
v.addEventListener('error', () => post('video:error:' + (v.error ? v.error.code : '?')));
v.src = 'clip.mp4';
let n = 0; const vt = setInterval(() => { post('video:t=' + v.currentTime.toFixed(2) + ',w=' + v.videoWidth + ',paused=' + v.paused + ',ready=' + v.readyState); if (++n >= 8) clearInterval(vt); }, 400);
const c = document.getElementById('c').getContext('2d'); let frames = 0;
const draw = () => { frames++; c.fillStyle = '#012'; c.fillRect(0, 0, 90, 90); c.fillStyle = '#fff'; c.fillRect((frames * 2) % 90, 35, 20, 20); requestAnimationFrame(draw); };
requestAnimationFrame(draw); setTimeout(() => post('raf:' + frames), 1000);
// what the host calls (script inside the page; nothing is sent to the desktop)
window.s15_nav = () => document.getElementById('out').click();
window.s15_open = () => { const w = window.open('https://example.com/new-window'); post('open:returned:' + (w ? 'a window' : 'null')); };
window.s15_key = () => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
</script></body></html>"#;

/// The host's own script, put in every page before the page's: the keys the lightbox wants are told to the host.
/// `s15post`: on macOS the host's OWN message handler - the library's `window.ipc` drops every message of a file:// page
/// there (it builds an `http::Uri` from the page's address, and `file:///…` has no authority; found in this spike).
const INIT: &str = r#"window.s15post = s => { const h = window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.s15; if (h) h.postMessage(s); else window.ipc.postMessage(s); };
window.addEventListener('keydown', e => { if (e.key === 'Escape') { window.s15post('key:Escape'); e.preventDefault(); e.stopPropagation(); } }, true);"#;

fn write_png(path: &Path, rgb: [u8; 3]) {
    let f = std::fs::File::create(path).expect("png"); let mut e = png::Encoder::new(std::io::BufWriter::new(f), 16, 16); e.set_color(png::ColorType::Rgb); e.set_depth(png::BitDepth::Eight);
    let px: Vec<u8> = (0..16 * 16).flat_map(|_| rgb).collect(); e.write_header().and_then(|mut w| w.write_image_data(&px)).expect("png");
}
/// Returns (the output folder, the page's folder), both absolute.
fn make_site() -> (PathBuf, PathBuf) {
    let out = std::env::var_os("S15_OUT").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("target/s15-out"));
    let (page, secret) = (out.join("site/page"), out.join("site/secret")); std::fs::create_dir_all(&page).expect("out"); std::fs::create_dir_all(&secret).expect("out");
    std::fs::write(page.join("index.html"), PAGE).unwrap(); std::fs::write(page.join("beside.txt"), "beside-text").unwrap(); std::fs::write(page.join("clip.mp4"), include_bytes!("../assets/clip-h264.bin")).unwrap();
    write_png(&page.join("beside.png"), [255, 255, 255]); write_png(&secret.join("outside.png"), [255, 0, 0]); std::fs::write(secret.join("outside.txt"), "outside-text").unwrap();
    let clean = |p: PathBuf| { let p = p.canonicalize().expect("out"); let s = p.to_string_lossy().to_string(); PathBuf::from(s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(s)) };
    (clean(out), clean(page))
}

// ---------------------------------------------------------------------------------------------------------------------
// The web view
// ---------------------------------------------------------------------------------------------------------------------
/// How the page is given to the view: what the verdict's file-access finding compares.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Access {
    /// macOS: `loadFileURL:allowingReadAccessToURL:` with the page's OWN folder. Windows: the folder mapped to a made-up https host.
    Folder,
    /// The library's plain "load this file:// address" (macOS: `loadRequest:`; Windows: a file:// navigation).
    Request,
    /// macOS only: `loadFileURL:allowingReadAccessToURL:` with the folder ABOVE the page's (to show the parameter is the control).
    Parent,
}

#[cfg(feature = "web")]
mod web {
    use super::*;
    use wry::dpi::{LogicalPosition, LogicalSize};
    pub struct Web { view: wry::WebView }
    fn bounds(r: Rect, s: f64) -> wry::Rect { wry::Rect { position: LogicalPosition::new(r.x0 / s, r.y0 / s).into(), size: LogicalSize::new(r.width() / s, r.height() / s).into() } }
    #[cfg(windows)] const HOST: &str = "s15.preview";
    fn file_url(p: &Path) -> String { let s = p.to_string_lossy().replace('\\', "/"); if s.starts_with('/') { format!("file://{s}") } else { format!("file:///{s}") } }

    /// `None` with the reason when the system has no web view (Windows without the WebView2 runtime).
    pub fn available() -> Result<String, String> { wry::webview_version().map_err(|e| e.to_string()) }

    pub fn create(window: &Win, rect: Rect, scale: f64, out: &Path, page: &Path, access: Access, focused: bool, msgs: &Msgs) -> Result<Web, String> {
        let _ = out;
        let push = { let m = msgs.clone(); move |s: String| m.borrow_mut().push((Instant::now(), s)) };
        // Windows: WebView2 keeps a profile folder; its default is beside the executable, which a packaged or installed
        // program cannot write. It must be given one (here: under the output folder).
        #[allow(unused_mut)] let mut ctx = wry::WebContext::new(if cfg!(windows) { Some(out.join("webview2-data")) } else { None });
        let (p1, p2, p3, p4, p5) = (push.clone(), push.clone(), push.clone(), push.clone(), push.clone());
        let mut b = wry::WebViewBuilder::new_with_web_context(&mut ctx)
            .with_bounds(bounds(rect, scale))
            .with_focused(focused)            // the default is TRUE: the view would take the keys as soon as it is made
            .with_initialization_script(INIT)
            .with_ipc_handler(move |req| p1(req.body().clone()))
            .with_document_title_changed_handler(move |t| p2(format!("title:{t}")))
            .with_on_page_load_handler(move |e, _url| p3(format!("load:{}", match e { wry::PageLoadEvent::Started => "started", wry::PageLoadEvent::Finished => "finished" })))
            // THE POLICY: only the page's own place is loaded in the view; anything else is told to the host and refused.
            .with_navigation_handler(move |url| { let ours = url.starts_with("file://") || url.starts_with("about:") || url.starts_with("https://s15.preview/"); if !ours { p4(format!("nav-blocked:{url}")); } ours })
            .with_new_window_req_handler(move |url, _f| { p5(format!("newwin-blocked:{url}")); wry::NewWindowResponse::Deny });
        if access == Access::Request { b = b.with_url(file_url(&page.join("index.html"))); }
        let view = b.build_as_child(window).map_err(|e| e.to_string())?;
        #[cfg(target_os = "macos")] { use wry::WebViewExtMacOS; mac_handler::add(&view.manager(), Box::new(push.clone())); }
        #[cfg(target_os = "macos")] if access != Access::Request {
            use objc2::rc::Retained; use objc2::runtime::AnyObject; use objc2::msg_send; use objc2_foundation::{NSString, NSURL}; use wry::WebViewExtMacOS;
            let wk = view.webview(); let wk: &AnyObject = unsafe { &*(Retained::as_ptr(&wk) as *const AnyObject) };
            let root = if access == Access::Parent { page.parent().unwrap() } else { page };
            #[allow(unused_unsafe)] unsafe {
                let file = NSURL::fileURLWithPath(&NSString::from_str(&page.join("index.html").to_string_lossy()));
                let dir = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(&root.to_string_lossy()), true);
                let nav: Option<Retained<AnyObject>> = msg_send![wk, loadFileURL: &*file, allowingReadAccessToURL: &*dir];
                if nav.is_none() { return Err("loadFileURL:allowingReadAccessToURL: returned nil".into()) }
            }
        }
        // UNTESTED (written on macOS): the page's folder becomes https://s15.preview/, and nothing else on the disk has
        // an address the page can name. DENY_CORS: other origins cannot read the mapped files.
        #[cfg(windows)] if access != Access::Request {
            use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2_3, COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_DENY_CORS};
            use windows::core::{Interface, HSTRING, PCWSTR}; use wry::WebViewExtWindows;
            let core3: ICoreWebView2_3 = view.webview().cast().map_err(|e| format!("ICoreWebView2_3: {e}"))?;
            let (host, folder) = (HSTRING::from(HOST), HSTRING::from(page.as_os_str()));
            unsafe { core3.SetVirtualHostNameToFolderMapping(PCWSTR(host.as_ptr()), PCWSTR(folder.as_ptr()), COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_DENY_CORS) }.map_err(|e| format!("SetVirtualHostNameToFolderMapping: {e}"))?;
            view.load_url(&format!("https://{HOST}/index.html")).map_err(|e| e.to_string())?;
        }
        Ok(Web { view })
    }
    impl Web {
        pub fn set_rect(&self, r: Rect, s: f64) { let _ = self.view.set_bounds(bounds(r, s)); }
        pub fn eval(&self, js: &str) { let _ = self.view.evaluate_script(js); }
        pub fn url(&self) -> String { self.view.url().unwrap_or_default() }
        pub fn focus_parent(&self) { let _ = self.view.focus_parent(); }
        pub fn focus(&self) { let _ = self.view.focus(); }
        /// The web content process of this view (macOS, a private selector: a measurement of the spike, never the product's).
        pub fn content_pid(&self) -> i32 {
            #[cfg(target_os = "macos")] { use objc2::rc::Retained; use objc2::runtime::AnyObject; use wry::WebViewExtMacOS; let wk = self.view.webview(); let wk: &AnyObject = unsafe { &*(Retained::as_ptr(&wk) as *const AnyObject) }; unsafe { objc2::msg_send![wk, _webProcessIdentifier] } }
            #[cfg(not(target_os = "macos"))] { 0 }
        }
    }
}
/// macOS: a script message handler of our own on the view's content controller (see `INIT`).
#[cfg(all(feature = "web", target_os = "macos"))]
mod mac_handler {
    use objc2::rc::Retained; use objc2::runtime::{NSObject, ProtocolObject}; use objc2::{define_class, msg_send, DeclaredClass, MainThreadMarker, MainThreadOnly};
    use objc2_foundation::{NSObjectProtocol, NSString}; use objc2_web_kit::{WKScriptMessage, WKScriptMessageHandler, WKUserContentController};
    pub struct Ivars { told: Box<dyn Fn(String)> }
    define_class!(
        #[unsafe(super(NSObject))] #[thread_kind = MainThreadOnly] #[name = "S15MessageHandler"] #[ivars = Ivars]
        pub struct Handler;
        unsafe impl NSObjectProtocol for Handler {}
        unsafe impl WKScriptMessageHandler for Handler {
            #[unsafe(method(userContentController:didReceiveScriptMessage:))]
            fn did_receive(&self, _c: &WKUserContentController, msg: &WKScriptMessage) { #[allow(unused_unsafe)] let body = unsafe { msg.body() }; if let Ok(s) = body.downcast::<NSString>() { (self.ivars().told)(s.to_string()); } }
        }
    );
    pub fn add(controller: &WKUserContentController, told: Box<dyn Fn(String)>) {
        let mtm = MainThreadMarker::new().unwrap(); let h: Retained<Handler> = unsafe { msg_send![super(Handler::alloc(mtm).set_ivars(Ivars { told })), init] };
        #[allow(unused_unsafe)] unsafe { controller.addScriptMessageHandler_name(ProtocolObject::from_ref(&*h), &NSString::from_str("s15")) };
    }
}
#[cfg(not(feature = "web"))]
mod web {
    use super::*;
    pub struct Web;
    pub fn available() -> Result<String, String> { Err("built without the web view".into()) }
    pub fn create(_: &Win, _: Rect, _: f64, _: &Path, _: &Path, _: Access, _: bool, _: &Msgs) -> Result<Web, String> { Err("built without the web view".into()) }
    impl Web { pub fn set_rect(&self, _: Rect, _: f64) {} pub fn eval(&self, _: &str) {} pub fn url(&self) -> String { String::new() } pub fn focus_parent(&self) {} pub fn focus(&self) {} pub fn content_pid(&self) -> i32 { 0 } }
}

// ---------------------------------------------------------------------------------------------------------------------
// What can be OBSERVED without touching input: who has the keys, who would get a click, how many windows / processes
// ---------------------------------------------------------------------------------------------------------------------
#[cfg(target_os = "macos")]
mod os {
    use super::Win;
    use objc2::MainThreadMarker; use objc2_app_kit::{NSApplication, NSView}; use objc2_foundation::NSPoint; use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    fn view(w: &Win) -> &NSView { match w.window_handle().unwrap().as_raw() { RawWindowHandle::AppKit(h) => unsafe { &*(h.ns_view.as_ptr() as *const NSView) }, _ => unreachable!() } }
    /// The class of the view that has the keys in our window.
    pub fn key_owner(w: &Win) -> String { view(w).window().and_then(|win| win.firstResponder()).map(|r| r.class().name().to_string_lossy().to_string()).unwrap_or_else(|| "none".into()) }
    /// The class of the view a click at (x, y) - logical, from the top left of the content - would go to.
    pub fn hit(w: &Win, x: f64, y: f64) -> String {
        let v = view(w); let p = NSPoint::new(x, if v.isFlipped() { y } else { v.frame().size.height - y });
        let p = v.convertPoint_toView(p, unsafe { v.superview() }.as_deref());
        v.hitTest(p).map(|h| h.class().name().to_string_lossy().to_string()).unwrap_or_else(|| "none".into())
    }
    /// (the application's windows, the views inside our content view)
    pub fn counts(w: &Win) -> (usize, usize) { let mtm = MainThreadMarker::new().unwrap(); (NSApplication::sharedApplication(mtm).windows().count(), view(w).subviews().count()) }
    pub fn windows() -> String { let mtm = MainThreadMarker::new().unwrap(); NSApplication::sharedApplication(mtm).windows().iter().map(|w| format!("{}{}", w.class().name().to_string_lossy(), if w.isVisible() { "" } else { " (not shown)" })).collect::<Vec<_>>().join(", ") }
    pub fn window_id(w: &Win) -> isize { view(w).window().map(|win| win.windowNumber()).unwrap_or(0) }
    unsafe extern "C" { fn responsibility_get_pid_responsible_for_pid(pid: i32) -> i32; }
    /// WebKit's helper processes under the same responsible process as this one (they are launchd's children, not ours;
    /// started from a terminal the responsible process is the terminal, so the script subtracts what was there before).
    pub fn helpers() -> Vec<(i32, String)> {
        let me = unsafe { responsibility_get_pid_responsible_for_pid(std::process::id() as i32) }; let out = std::process::Command::new("/bin/ps").args(["-axo", "pid=,comm="]).output().map(|o| String::from_utf8_lossy(&o.stdout).to_string()).unwrap_or_default();
        out.lines().filter_map(|l| { let l = l.trim(); let (pid, comm) = l.split_once(' ')?; let pid: i32 = pid.parse().ok()?; if !comm.contains("WebKit") { return None }
            (unsafe { responsibility_get_pid_responsible_for_pid(pid) } == me).then(|| (pid, comm.rsplit('/').next().unwrap_or(comm).to_string())) }).collect()
    }
    /// The window as the system composes it - our drawing AND the web view (window capture; not input).
    /// Returns how it was taken. `screencapture -l` needs the Screen Recording permission of whatever started us; without
    /// it the process captures its OWN window through CoreGraphics, which needs no permission.
    pub fn shot(w: &Win, path: &std::path::Path) -> Option<&'static str> {
        if std::process::Command::new("/usr/sbin/screencapture").args(["-x", "-o", "-l", &window_id(w).to_string()]).arg(path).stderr(std::process::Stdio::null()).status().is_ok_and(|s| s.success()) && path.exists() { return Some("screencapture -l") }
        own_window(window_id(w) as u32, path).then_some("CGWindowListCreateImage of our own window")
    }
    #[link(name = "CoreGraphics", kind = "framework")] unsafe extern "C" {
        fn CGWindowListCreateImage(bounds: CGRect, list: u32, window: u32, image: u32) -> *mut c_void;
        fn CGImageGetWidth(i: *mut c_void) -> usize; fn CGImageGetHeight(i: *mut c_void) -> usize; fn CGImageGetBytesPerRow(i: *mut c_void) -> usize; fn CGImageGetBitsPerPixel(i: *mut c_void) -> usize;
        fn CGImageGetDataProvider(i: *mut c_void) -> *mut c_void; fn CGDataProviderCopyData(p: *mut c_void) -> *mut c_void; fn CGImageRelease(i: *mut c_void);
    }
    #[link(name = "CoreFoundation", kind = "framework")] unsafe extern "C" { fn CFDataGetBytePtr(d: *mut c_void) -> *const u8; fn CFDataGetLength(d: *mut c_void) -> isize; fn CFRelease(d: *mut c_void); }
    use std::ffi::c_void;
    #[repr(C)] struct CGRect { x: f64, y: f64, w: f64, h: f64 }
    fn own_window(id: u32, path: &std::path::Path) -> bool { unsafe {
        // (CGRectNull = the window's own bounds; 8 = only this window; 1 | 8 = no shadow, the screen's full resolution)
        let img = CGWindowListCreateImage(CGRect { x: f64::INFINITY, y: f64::INFINITY, w: 0.0, h: 0.0 }, 8, id, 1 | 8); if img.is_null() { return false }
        let (w, h, row, bpp) = (CGImageGetWidth(img), CGImageGetHeight(img), CGImageGetBytesPerRow(img), CGImageGetBitsPerPixel(img)); let data = CGDataProviderCopyData(CGImageGetDataProvider(img));
        let ok = !data.is_null() && bpp == 32 && CFDataGetLength(data) as usize >= row * h && { let px = std::slice::from_raw_parts(CFDataGetBytePtr(data), row * h);
            let rgb: Vec<u8> = (0..h).flat_map(|y| (0..w).flat_map(move |x| { let i = y * row + x * 4; [px[i + 2], px[i + 1], px[i]] })).collect();   // BGRA in memory
            std::fs::File::create(path).ok().is_some_and(|f| { let mut e = png::Encoder::new(std::io::BufWriter::new(f), w as u32, h as u32); e.set_color(png::ColorType::Rgb); e.set_depth(png::BitDepth::Eight); e.write_header().and_then(|mut wr| wr.write_image_data(&rgb)).is_ok() }) };
        if !data.is_null() { CFRelease(data); } CGImageRelease(img); ok } }
}
// UNTESTED (written on macOS).
#[cfg(windows)]
mod os {
    use super::Win;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle}; use windows::Win32::Foundation::{HWND, POINT}; use windows::Win32::UI::WindowsAndMessaging::*;
    fn hwnd(w: &Win) -> HWND { match w.window_handle().unwrap().as_raw() { RawWindowHandle::Win32(h) => HWND(h.hwnd.get() as *mut core::ffi::c_void), _ => unreachable!() } }
    pub fn key_owner(w: &Win) -> String { let f = unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetFocus() }; if f == hwnd(w) { "our window".into() } else if f.0.is_null() { "none (this thread has no focus)".into() } else if unsafe { IsChild(hwnd(w), f) }.as_bool() { "a child window (the web view)".into() } else { "another window".into() } }
    pub fn hit(w: &Win, x: f64, y: f64) -> String { let s = w.scale_factor(); let c = unsafe { ChildWindowFromPointEx(hwnd(w), POINT { x: (x * s) as i32, y: (y * s) as i32 }, CWP_SKIPINVISIBLE) }; if c == hwnd(w) { "our window".into() } else if c.0.is_null() { "none".into() } else { "a child window (the web view)".into() } }
    pub fn windows() -> String { String::new() }
    pub fn counts(w: &Win) -> (usize, usize) { let mut n = 0; let mut c = HWND::default(); loop { c = unsafe { FindWindowExW(Some(hwnd(w)), Some(c), None, None) }.unwrap_or_default(); if c.0.is_null() { break } n += 1; } (1, n) }
    /// msedgewebview2.exe processes that descend from this one.
    pub fn helpers() -> Vec<(i32, String)> {
        use windows::Win32::System::Diagnostics::ToolHelp::*;
        let mut all: Vec<(u32, u32, String)> = Vec::new();
        unsafe { let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return vec![] }; let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
            if Process32FirstW(snap, &mut e).is_ok() { loop { let n = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(e.szExeFile.len()); all.push((e.th32ProcessID, e.th32ParentProcessID, String::from_utf16_lossy(&e.szExeFile[..n]))); if Process32NextW(snap, &mut e).is_err() { break } } }
            let _ = windows::Win32::Foundation::CloseHandle(snap); }
        let mut ours = vec![std::process::id()]; let mut found = Vec::new();
        loop { let before = ours.len(); for (pid, parent, name) in &all { if ours.contains(parent) && !ours.contains(pid) { ours.push(*pid); found.push((*pid as i32, name.clone())); } } if ours.len() == before { break } }
        found
    }
    pub fn shot(_: &Win, _: &std::path::Path) -> Option<&'static str> { None }
}
#[cfg(not(any(target_os = "macos", windows)))]
mod os { use super::Win; pub fn windows() -> String { String::new() } pub fn key_owner(_: &Win) -> String { String::new() } pub fn hit(_: &Win, _: f64, _: f64) -> String { String::new() } pub fn counts(_: &Win) -> (usize, usize) { (0, 0) } pub fn helpers() -> Vec<(i32, String)> { vec![] } pub fn shot(_: &Win, _: &std::path::Path) -> Option<&'static str> { None } }

// ---------------------------------------------------------------------------------------------------------------------
// The picture's check: the page is green, our drawing in the middle region is blue, the strips are neither.
// ---------------------------------------------------------------------------------------------------------------------
struct Shot { w: usize, h: usize, ch: usize, px: Vec<u8>, /// rows above the content (the title bar)
    top: usize }
impl Shot {
    fn load(path: &Path, content: (u32, u32)) -> Option<Shot> {
        let mut d = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).ok()?)); d.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut rd = d.read_info().ok()?; let mut px = vec![0u8; rd.output_buffer_size()]; let info = rd.next_frame(&mut px).ok()?; px.truncate(info.buffer_size());
        let (w, h) = (info.width as usize, info.height as usize); let ch = px.len() / (w * h);
        if w != content.0 as usize || h < content.1 as usize { println!("   (picture {w}x{h} does not match the content {}x{})", content.0, content.1); return None }
        Some(Shot { w, h, ch, px, top: h - content.1 as usize })
    }
    /// 'g' the page, 'b' our middle region, '.' anything else - at a content pixel.
    fn class(&self, x: usize, y: usize) -> char { let y = y + self.top; if x >= self.w || y >= self.h { return '.' } let i = (y * self.w + x) * self.ch; let (r, g, b) = (self.px[i] as i32, self.px[i + 1] as i32, self.px[i + 2] as i32);
        if g > 130 && g > r + 60 && g > b + 40 { 'g' } else if b > 140 && b > r + 60 && b > g + 60 { 'b' } else { '.' } }
    /// The share of `c` in a rectangle (content pixels), sampled every 3 pixels.
    fn share(&self, r: Rect, c: char) -> f64 { let (mut n, mut k) = (0u32, 0u32); let mut y = r.y0.max(0.0); while y < r.y1 { let mut x = r.x0.max(0.0); while x < r.x1 { n += 1; if self.class(x as usize, y as usize) == c { k += 1; } x += 3.0; } y += 3.0; } if n == 0 { 0.0 } else { k as f64 / n as f64 } }
    /// (the least share of the page in the four bands just INSIDE the rectangle's edges, the greatest share of the page in the four bands just OUTSIDE)
    fn edges(&self, r: Rect, limit: Rect) -> (f64, f64) {
        let band = |a: f64, b: f64| [Rect::new(r.x0 + 12.0, r.y0 + a, r.x1 - 12.0, r.y0 + b), Rect::new(r.x0 + 12.0, r.y1 - b, r.x1 - 12.0, r.y1 - a), Rect::new(r.x0 + a, r.y0 + 12.0, r.x0 + b, r.y1 - 12.0), Rect::new(r.x1 - b, r.y0 + 12.0, r.x1 - a, r.y1 - 12.0)];
        let inside = band(4.0, 12.0).iter().map(|b| self.share(*b, 'g')).fold(1.0, f64::min);
        let outside = band(-12.0, -4.0).iter().map(|b| b.intersect(limit)).filter(|b| b.width() > 1.0 && b.height() > 1.0).map(|b| self.share(b, 'g')).fold(0.0, f64::max);
        (inside, outside)
    }
}

// ---------------------------------------------------------------------------------------------------------------------
// The window
// ---------------------------------------------------------------------------------------------------------------------
struct Gfx { presenter: Presenter, layer: UiLayer, fonts: Fonts }
struct App {
    script: bool, out: PathBuf, page: PathBuf, window: Option<Win>, gfx: Option<Gfx>, close: TextButton, frames: u32, born: Instant,
    web: Option<web::Web>, /// where the web view is (physical pixels), when it is not the whole middle region
    web_rect: Option<Rect>, msgs: Msgs,
    step: u32, since: Instant, i: u32, t0: Instant, results: Vec<String>, times: Vec<f64>, pids: Vec<i32>, fails: u32, /// helper processes that were there before our first web view
    base: Vec<i32>,
}
const TOP: f64 = 44.0; const BOTTOM: f64 = 32.0;
impl App {
    fn scale(&self) -> f64 { self.window.as_ref().map(|w| w.scale_factor()).unwrap_or(1.0) }
    fn size(&self) -> (f64, f64) { let s = self.window.as_ref().unwrap().surface_size(); (s.width as f64, s.height as f64) }
    /// The lightbox's region: between the two strips (physical pixels).
    fn middle(&self) -> Rect { let (w, h) = self.size(); let s = self.scale(); Rect::new(0.0, TOP * s, w, h - BOTTOM * s) }
    fn say(&mut self, s: impl Into<String>) { let s = s.into(); println!("{s}"); self.results.push(s); }
    fn check(&mut self, ok: bool, s: impl Into<String>) { if !ok { self.fails += 1; } let s = format!("[{}] {}", if ok { "ok" } else { "FAIL" }, s.into()); self.say(s); }
    fn redraw(&mut self) {
        let (w, h) = self.size(); let s = self.scale(); if w < 1.0 || h < 1.0 { return } let mid = self.middle(); let now = Instant::now();
        let Some(g) = self.gfx.as_mut() else { return };
        let scene = g.layer.scene_mut(); scene.reset();
        let fill = |scene: &mut vello::Scene, r: Rect, c: [u8; 3]| scene.fill(peniko::Fill::NonZero, Affine::IDENTITY, peniko::Color::from_rgb8(c[0], c[1], c[2]), None, &r);
        fill(scene, Rect::new(0.0, 0.0, w, h), [38, 38, 38]);
        fill(scene, mid, [20, 40, 230]);                                  // OUR drawing where the page goes: seen only where no web view is
        self.close.rect = Rect::new(w - 96.0 * s, 10.0 * s, w - 12.0 * s, 34.0 * s); self.close.paint(scene, &mut g.fonts, s, now);
        let white = peniko::Color::from_rgb8(235, 235, 235);
        let l = g.fonts.layout_fit("S15 - a system web view inside our window", Family::Sans, 14.0 * s as f32, 700.0, white, w - 130.0 * s); draw_layout(scene, &l, Affine::translate((12.0 * s, (TOP * s - l.height() as f64) / 2.0)));
        let l = g.fonts.layout_fit("<out>/site/page/index.html", Family::Mono, 12.0 * s as f32, 400.0, white, w - 24.0 * s); draw_layout(scene, &l, Affine::translate((12.0 * s, h - BOTTOM * s + (BOTTOM * s - l.height() as f64) / 2.0)));
        g.layer.render(&g.presenter.engine.device, &g.presenter.engine.queue);
        let window = self.window.clone().unwrap(); g.presenter.draw(|| window.pre_present_notify()); self.frames += 1;
    }
    fn open(&mut self, access: Access, focused: bool) -> bool {
        self.msgs.borrow_mut().clear(); let r = self.web_rect.unwrap_or(self.middle()); let t = Instant::now();
        match web::create(self.window.as_ref().unwrap(), r, self.scale(), &self.out, &self.page, access, focused, &self.msgs) {
            Ok(w) => { self.times.push(t.elapsed().as_secs_f64() * 1000.0); self.web = Some(w); self.t0 = t; true }
            Err(e) => { self.check(false, format!("the web view could not be made: {e}")); false } }
    }
    fn shut(&mut self) { if let Some(w) = self.web.take() { self.pids.push(w.content_pid()); } }
    fn has(&self, what: &str) -> bool { self.msgs.borrow().iter().any(|(_, m)| m.starts_with(what)) }
    fn when(&self, what: &str) -> Option<f64> { self.msgs.borrow().iter().find(|(_, m)| m.starts_with(what)).map(|(t, _)| t.duration_since(self.t0).as_secs_f64() * 1000.0) }
    fn told(&self, skip: &[&str]) -> String { self.msgs.borrow().iter().map(|(_, m)| m.clone()).filter(|m| !skip.iter().any(|s| m.starts_with(s))).collect::<Vec<_>>().join(" | ") }
    fn observe(&mut self, label: &str) { let w = self.window.clone().unwrap(); let (wins, views) = os::counts(&w); let h: Vec<(i32, String)> = os::helpers().into_iter().filter(|(p, _)| !self.base.contains(p)).collect(); let mut names: Vec<String> = h.iter().map(|(_, n)| n.rsplit('.').next().unwrap_or(n).to_string()).collect(); names.sort();
        self.say(format!("   {label}: keys = {}; windows of the application = {wins}; [{}]; child views / windows in ours = {views}; helper processes = {} {names:?}", os::key_owner(&w), os::windows(), h.len())); }
    fn shot(&mut self, name: &str) -> Option<Shot> { let w = self.window.clone().unwrap(); let path = self.out.join(name); let _ = std::fs::remove_file(&path);
        let Some(how) = os::shot(&w, &path) else { self.check(false, format!("no picture: the window capture did not produce {name}")); return None }; self.say(format!("   {name}: taken by {how}"));
        let s = self.window.as_ref().unwrap().surface_size(); Shot::load(&path, (s.width, s.height)) }

    /// One turn of the script. Each step waits for its own condition; nothing here sends input.
    fn turn(&mut self, el: &dyn ActiveEventLoop) {
        let now = Instant::now(); let waited = now.duration_since(self.since).as_secs_f64(); let s = self.scale(); let mid = self.middle();
        let next = |a: &mut App| { a.step += 1; a.since = Instant::now(); a.i = 0; };
        match self.step {
            0 => if self.frames >= 3 && waited > 0.6 {
                let v = web::available(); self.say(format!("the system's web view: {}", match &v { Ok(v) => format!("present, version {v}"), Err(e) => format!("MISSING ({e})") }));
                if v.is_err() { self.say("   -> the lightbox would show its own 'no preview' card with 'Open in browser'; nothing else to test here"); self.step = 99; return }
                self.base = os::helpers().into_iter().map(|(p, _)| p).collect(); self.say(format!("window: content {}x{} px at scale {s}; the middle region {:?}", self.size().0, self.size().1, mid)); self.observe("before any web view"); next(self); }
            // 1-2: the page appears, its script runs, the video plays (pass bar 1, 2, part of 4)
            1 => { if self.open(Access::Folder, false) { let t = self.times[0]; self.say(format!("the first web view: made in {t:.0} ms (the call returns)")); next(self); } else { self.step = 99; } }
            2 => if waited > 4.2 {
                let (ran, fin) = (self.when("script:ran"), self.when("load:finished"));
                self.check(ran.is_some(), format!("the page's script ran and posted to the host {} ms after the view was asked for (load finished at {} ms)", ran.map(|t| format!("{t:.0}")).unwrap_or("-".into()), fin.map(|t| format!("{t:.0}")).unwrap_or("-".into())));
                self.check(self.has("title:s15-title-42"), "the host read back the title the script computed (s15-title-42)");
                let vt: Vec<(f64, bool)> = self.msgs.borrow().iter().filter_map(|(_, m)| { let m = m.strip_prefix("video:t=")?; let t: f64 = m.split(',').next()?.parse().ok()?; Some((t, m.contains("w=160"))) }).collect();
                let moving = vt.windows(2).filter(|p| p[1].0 != p[0].0).count();
                self.check(moving >= 3 && vt.iter().any(|v| v.1), format!("the <video> is live: currentTime changed in {moving} of {} samples, videoWidth 160 seen: {}", vt.len().saturating_sub(1), vt.iter().any(|v| v.1)));
                let raf = self.told(&[]).split(" | ").find_map(|m| m.strip_prefix("raf:").and_then(|n| n.parse::<u32>().ok())).unwrap_or(0); self.check(raf > 20, format!("the <canvas> animation ran {raf} frames in its first second"));
                self.say(format!("   told by the page (Folder access): {}", self.told(&["video:t=", "load:", "title:", "script:", "raf:"])));
                self.check(self.has("img:beside:ok"), "an image BESIDE the page loads"); self.check(self.has("img:outside:fail"), "an image in a SIBLING folder does not load");
                self.check(!self.has("fetch:outside:ok"), "fetch() of a file in a sibling folder does not succeed");
                self.observe("with the web view (made unfocused)");
                let w = self.window.clone().unwrap(); let (lw, lh) = (self.size().0 / s, self.size().1 / s);
                let (a, b, c) = (os::hit(&w, lw - 50.0, 22.0), os::hit(&w, lw / 2.0, lh / 2.0), os::hit(&w, 40.0, lh - 16.0));
                self.say(format!("   a click would go to: on Close (top strip) = {a}; in the page = {b}; in the bottom strip = {c}"));
                self.check(a == c && a != b, "pointer: our strips are ours, the rectangle is the web view's");
                if let Some(p) = self.shot("shot-1-open.png") { let (inside, outside) = p.edges(mid, Rect::new(0.0, 0.0, self.size().0, self.size().1)); let strips = p.share(Rect::new(0.0, 0.0, self.size().0, TOP * s), 'g') + p.share(Rect::new(0.0, mid.y1, self.size().0, self.size().1), 'g');
                    let page = p.share(mid, 'g');   // (the rest of the rectangle is the page's own video, canvas and images)
                    self.check(inside > 0.95 && outside == 0.0 && strips == 0.0 && page > 0.85, format!("picture 1: page pixels fill the rectangle's edges ({:.0}%) and {:.0}% of it, none outside it ({:.1}%), none in our strips", inside * 100.0, page * 100.0, outside * 100.0)); }
                // 3: the policy (pass bar 4) and the key path (pass bar 5) - asked of the page by script
                if let Some(v) = &self.web { v.eval("window.s15_nav()"); } next(self); }
            3 => if waited > 0.7 { if let Some(v) = &self.web { v.eval("window.s15_open()"); v.eval("window.s15_key()"); } next(self); }
            4 => if waited > 0.7 {
                let url = self.web.as_ref().map(|v| v.url()).unwrap_or_default();
                self.check(self.has("nav-blocked:https://example.com/"), "a link to https://example.com/ was told to the host and NOT loaded"); self.check(url.ends_with("/index.html"), format!("the view is still on its page (…{})", url.rsplit('/').next().unwrap_or("")));
                self.check(self.has("newwin-blocked:https://example.com/new-window"), "window.open() of an outside address was told to the host and refused");
                self.check(self.has("key:Escape"), "a keydown of Escape INSIDE the page reaches the host through the host's own script (a DOM event made by script - no key was sent)");
                self.say(format!("   told: {}", self.told(&["video:t=", "load:", "title:", "script:", "raf:", "img:", "fetch:"]))); next(self); }
            // 5: fifty rectangles (pass bar 3)
            5 => if waited > 0.033 { let i = self.i as f64; let (w, h) = (mid.width(), mid.height());
                let r = Rect::new(mid.x0 + (i * 37.0) % (w * 0.4), mid.y0 + (i * 23.0) % (h * 0.4), mid.x1 - (i * 29.0) % (w * 0.3), mid.y1 - (i * 17.0) % (h * 0.3));
                let r = if self.i == 49 { Rect::new(mid.x0 + (w * 0.25).round(), mid.y0 + (h * 0.2).round(), mid.x1 - (w * 0.1).round(), mid.y1 - (h * 0.15).round()) } else { r };
                self.web_rect = Some(r); if let Some(v) = &self.web { v.set_rect(r, s); } self.redraw(); self.since = now; self.i += 1; if self.i == 50 { next(self); } }
            6 => if waited > 0.7 { let r = self.web_rect.unwrap();
                if let Some(p) = self.shot("shot-2-resized.png") { let (inside, outside) = p.edges(r, mid); let left = Rect::new(mid.x0, mid.y0, r.x0 - 4.0, mid.y1);
                    self.check(inside > 0.95 && outside == 0.0 && p.share(left, 'g') == 0.0 && p.share(left, 'b') > 0.99, format!("picture 2: after 50 rectangles the page is exactly in the last one (inside edges {:.0}%, outside {:.1}%); our drawing shows where it was ({:.0}% ours to its left)", inside * 100.0, outside * 100.0, p.share(left, 'b') * 100.0)); }
                self.check(self.has("script:ran") && self.web.is_some(), "50 changes of the rectangle, no crash, the same page still there");
                self.web = None; self.web_rect = None; self.redraw(); next(self); }
            7 => if waited > 0.6 {
                if let Some(p) = self.shot("shot-3-removed.png") { self.check(p.share(mid, 'g') == 0.0 && p.share(mid, 'b') > 0.99, format!("picture 3: the web view removed - no page pixel left, our drawing is back ({:.0}% ours)", p.share(mid, 'b') * 100.0)); }
                self.observe("after removing it"); self.times.clear(); self.pids.clear(); next(self); }
            // 8: twenty lives (pass bar 3)
            8 => { if self.web.is_none() { if self.i == 20 { next(self); return } if !self.open(Access::Folder, false) { self.step = 99; } }
                   else if self.has("script:ran") || waited > 3.0 { if !self.has("script:ran") { self.check(false, format!("life {}: the page did not run within 3 s", self.i + 1)); } self.shut(); self.i += 1; self.since = now; } }
            9 => if waited > 2.0 { let (wins, views) = os::counts(self.window.as_ref().unwrap()); let mut p = self.pids.clone(); p.sort(); p.dedup();
                let t = &self.times; let avg = t.iter().sum::<f64>() / t.len().max(1) as f64; let max = t.iter().cloned().fold(0.0, f64::max);
                self.say(format!("   20 lives: made in {avg:.0} ms on average (longest {max:.0} ms); {} different content processes were used", p.len()));
                self.check(views == 0 && wins <= 2, format!("after 20 lives: {views} child views left in our window, {wins} window(s) in the application")); self.observe("2 s after the 20th"); next(self); }
            // 10-11: what the other ways of loading allow (the file-access finding)
            10 => { if self.web.is_none() { if !self.open(Access::Request, false) { self.step = 99; } } else if waited > 2.5 { self.say(format!("   told by the page (the library's plain file:// load): {}", self.told(&["video:t=", "load:", "title:", "raf:"]))); self.web = None; next(self); } }
            11 => { if cfg!(not(target_os = "macos")) { next(self); return } if self.web.is_none() { if !self.open(Access::Parent, false) { self.step = 99; } } else if waited > 2.5 { self.say(format!("   told by the page (read access given to the folder ABOVE): {}", self.told(&["video:t=", "load:", "title:", "raf:"]))); self.web = None; next(self); } }
            // 12: who has the keys when the view is made the library's default way (focused), and giving them back
            12 => { if self.web.is_none() { if !self.open(Access::Folder, true) { self.step = 99; } } else if waited > 1.0 { let w = self.window.clone().unwrap(); let short = |s: String| s.rsplit("::").next().unwrap_or("").to_string(); let v = self.web.take().unwrap();
                let a = short(os::key_owner(&w)); v.focus(); let b = short(os::key_owner(&w)); v.focus_parent(); let c = short(os::key_owner(&w)); v.focus(); drop(v); let d = short(os::key_owner(&w));
                self.say(format!("   keys: made FOCUSED (the library's default) = {a}; after focus() (what a click in the page does) = {b}; after focus_parent() = {c}; after focus() and then REMOVING the view = {d}"));
                self.check(c == "WinitView" || cfg!(not(target_os = "macos")), "focus_parent() gives the keys back to our view"); next(self); } }
            13 => if waited > 3.0 { self.observe("3 s after the last view"); self.step = 99; }
            100 => {}   // (the loop is told to end ONCE - S14's rule)
            _ => { self.step = 100; let fails = self.fails; self.say(format!("S15 script: {}", if fails == 0 { "every check passed".to_string() } else { format!("{fails} check(s) FAILED") }));
                let _ = std::fs::write(self.out.join("result.txt"), self.results.join("\n") + "\n"); self.web = None; el.exit(); }
        }
    }
}
impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, el: &dyn ActiveEventLoop) {
        if self.window.is_some() { return }
        // (the script's window does not ask to be the active one: the owner may be typing elsewhere)
        let attrs = WindowAttributes::default().with_title("S15 web view").with_surface_size(winit::dpi::LogicalSize::new(900.0, 620.0)).with_active(!self.script);
        let window: Win = Arc::new(el.create_window(attrs).expect("the window"));
        let instance = Presenter::instance(); let surface = instance.create_surface(window.clone()).expect("the surface"); let size = window.surface_size();
        #[cfg(target_os = "macos")] let view = { use raw_window_handle::{HasWindowHandle, RawWindowHandle}; match window.window_handle().unwrap().as_raw() { RawWindowHandle::AppKit(h) => Some(h.ns_view.as_ptr()), _ => None } };
        #[cfg(not(target_os = "macos"))] let view = None;
        let mut presenter = Presenter::new(&instance, surface, (size.width.max(1), size.height.max(1)), Output::Auto, view).expect("the presenter"); presenter.engine.set_ui_scale(window.scale_factor() as f32);
        let layer = UiLayer::new(&presenter.engine.device, size.width.max(1), size.height.max(1)).expect("the layer"); presenter.engine.set_overlay(Some(layer.view()));
        self.gfx = Some(Gfx { presenter, layer, fonts: Fonts::new() }); self.window = Some(window); self.since = Instant::now(); self.born = Instant::now();
        if !self.script { match web::available() { Ok(_) => { self.open(Access::Folder, false); } Err(e) => println!("no system web view: {e}") } }
    }
    fn window_event(&mut self, el: &dyn ActiveEventLoop, _id: WindowId, e: WindowEvent) {
        match e {
            WindowEvent::CloseRequested => { self.web = None; el.exit(); }
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::SurfaceResized(sz) => { if sz.width > 0 && sz.height > 0 { if let Some(g) = self.gfx.as_mut() { g.presenter.resize(sz.width, sz.height); g.layer.resize(&g.presenter.engine.device, sz.width, sz.height); g.presenter.engine.set_overlay(Some(g.layer.view())); } }
                if let Some(v) = &self.web { v.set_rect(self.web_rect.unwrap_or(self.middle()), self.scale()); } self.redraw(); }
            // the walk by hand: our own controls beside the page
            WindowEvent::PointerMoved { position, .. } => { self.close.event(&min_ui::Event::Move { x: position.x, y: position.y }, Instant::now()); if let Some(w) = &self.window { w.request_redraw(); } }
            WindowEvent::PointerButton { state, position, button: ButtonSource::Mouse(MouseButton::Left), .. } => { let (x, y) = (position.x, position.y); let ev = if state.is_pressed() { min_ui::Event::Down { x, y, button: min_ui::Button::Primary } } else { min_ui::Event::Up { x, y, button: min_ui::Button::Primary } };
                if self.close.event(&ev, Instant::now()).0 { if self.web.take().is_some() { println!("host: Close clicked - the web view is removed"); } else { println!("host: Close clicked - the web view is made again"); self.open(Access::Folder, false); } } if let Some(w) = &self.window { w.request_redraw(); } }
            WindowEvent::KeyboardInput { event, .. } if event.state.is_pressed() => println!("host: OUR window got the key {:?}", event.logical_key),
            _ => {}
        }
    }
    fn about_to_wait(&mut self, el: &dyn ActiveEventLoop) {
        if self.script { self.turn(el); if self.frames < 3 { if let Some(w) = &self.window { w.request_redraw(); } } el.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(16))); }
        else { let told: Vec<String> = self.msgs.borrow_mut().drain(..).map(|(_, m)| m).filter(|m| !m.starts_with("video:t=")).collect(); for m in told { println!("page: {m}"); } el.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(100))); }
    }
}

fn main() {
    let script = std::env::var("S15_SCRIPT").is_ok_and(|v| v == "all"); let (out, page) = make_site(); let now = Instant::now();
    let el = EventLoop::new().expect("the event loop");
    let app = App { script, out, page, window: None, gfx: None, close: TextButton::new("Close", Variant::Secondary).size_s(), frames: 0, born: now, web: None, web_rect: None, msgs: Rc::new(RefCell::new(Vec::new())), step: 0, since: now, i: 0, t0: now, results: Vec::new(), times: Vec::new(), pids: Vec::new(), fails: 0, base: Vec::new() };
    let _ = el.run_app(app);
}
