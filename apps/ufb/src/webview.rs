//! An HTML file in the lightbox: the SYSTEM's own web view as a child of our window (canon rows 453, 479; carried from
//! spike S15 - "Native webview or nothing"). macOS: WKWebView through `wry`, with the spike's two pieces of our own
//! (the confined file load, our own message handler). Windows: WebView2 through `wry`, the page's folder served through
//! a made-up https host (S15's Windows run, 2026-11-15), the network shut by a request filter; a machine without the
//! runtime shows the file as its source text.
//!
//! The rules (S15; the network by rows 460 and 512): the view is made unfocused; the page is loaded with read access
//! to ITS OWN FOLDER only; what the page EMBEDS from the web (pictures, players, fonts, frames) is loaded; a link to
//! another page is NOT followed in the view - it is told to us (`open:<address>`) and opened in the system's browser,
//! as is a new window the page asks for; a file outside the page's folder is refused; nothing of a page is kept (no
//! cookies, no storage); Escape pressed inside the page is told to us by a script of ours; the keys are given back to
//! our view before the web view is dropped; nothing of ours can be drawn over it - it is a system view above our
//! surface, so the lightbox keeps its own controls outside the view's rectangle.
use kurbo::Rect;
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

pub type Win = std::sync::Arc<Box<dyn winit31::window::Window>>;
/// What the page told: `key:Escape`, `title:...`, `load:finished`, `blocked:<address>`, `open:<address>` (a link to
/// the web that was pressed: for the system's browser).
pub type Msgs = Rc<RefCell<Vec<String>>>;

/// Our script's second half, on both systems: a press on a link that leads to the web (or to a mail address) is not
/// followed in the view - its address is told to the host. `POST` is the system's way of telling; `OWN` the host name
/// under which the page's own folder is served (Windows), which is no link "to the web".
#[cfg(any(target_os = "macos", windows))]
fn link_script(post: &str, own: &str) -> String {
    format!(r#"document.addEventListener('click', e => {{ const a = e.target && e.target.closest && e.target.closest('a[href]'); if (!a) return; let u; try {{ u = new URL(a.href, location.href); }} catch (x) {{ return; }}
      if ((u.protocol === 'http:' || u.protocol === 'https:' || u.protocol === 'mailto:') && u.host !== '{own}') {{ e.preventDefault(); e.stopPropagation(); {post}('open:' + u.href); }} }}, true); window.__ufbLinks = 1;"#)
}
/// An address the system's browser may be handed: the web or a mail address, nothing else (never a file, never a script).
pub fn outward(url: &str) -> bool { let l = url.to_ascii_lowercase(); l.starts_with("http://") || l.starts_with("https://") || l.starts_with("mailto:") }

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use objc2::rc::Retained; use objc2::runtime::{AnyObject, NSObject, ProtocolObject}; use objc2::{define_class, msg_send, DeclaredClass, MainThreadMarker, MainThreadOnly};
    use objc2_foundation::{NSObjectProtocol, NSString, NSURL}; use objc2_web_kit::{WKScriptMessage, WKScriptMessageHandler, WKUserContentController};
    use wry::dpi::{LogicalPosition, LogicalSize}; use wry::WebViewExtMacOS;

    /// Our script, put in every page before the page's own: Escape is told to the host (the page has the keys once it
    /// was clicked). Through a message handler of OUR OWN: the library's channel drops what a file page sends.
    const INIT: &str = r#"window.addEventListener('keydown', e => { if (e.key === 'Escape') { const h = window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.ufb; if (h) h.postMessage('key:Escape'); e.preventDefault(); e.stopPropagation(); } }, true);"#;
    pub struct Ivars { told: Box<dyn Fn(String)> }
    define_class!(
        #[unsafe(super(NSObject))] #[thread_kind = MainThreadOnly] #[name = "UFBWebMessageHandler"] #[ivars = Ivars]
        pub struct Handler;
        unsafe impl NSObjectProtocol for Handler {}
        unsafe impl WKScriptMessageHandler for Handler {
            #[unsafe(method(userContentController:didReceiveScriptMessage:))]
            fn did_receive(&self, _c: &WKUserContentController, msg: &WKScriptMessage) { #[allow(unused_unsafe)] let body = unsafe { msg.body() }; if let Ok(s) = body.downcast::<NSString>() { (self.ivars().told)(s.to_string()); } }
        }
    );

    pub struct Web { view: wry::WebView, pub msgs: Msgs, rect: std::cell::Cell<(Rect, f64)> }
    fn bounds(r: Rect, s: f64) -> wry::Rect { wry::Rect { position: LogicalPosition::new(r.x0 / s, r.y0 / s).into(), size: LogicalSize::new(r.width() / s, r.height() / s).into() } }
    fn file_url(p: &Path) -> String { format!("file://{}", p.to_string_lossy()) }

    pub fn create(window: &Win, rect: Rect, scale: f64, page: &Path, wake: std::sync::Arc<dyn Fn() + Send + Sync>) -> Result<Web, String> {
        let mtm = MainThreadMarker::new().ok_or("not on the main thread")?;
        let folder = page.parent().ok_or("the page has no folder")?.to_path_buf();
        let msgs: Msgs = Default::default();
        let push = { let (m, w) = (msgs.clone(), wake.clone()); move |s: String| { m.borrow_mut().push(s); w(); } };
        let (p1, p2, p3, p4) = (push.clone(), push.clone(), push.clone(), push.clone());
        // only what lies in the page's own folder (or below) is loaded in the view; anything else is refused and told
        let root = file_url(&folder); let root = if root.ends_with('/') { root } else { format!("{root}/") };
        let view = wry::WebViewBuilder::new()
            .with_bounds(bounds(rect, scale))
            .with_focused(false)              // the library's default would activate the application
            .with_incognito(true)             // a preview leaves nothing behind (no cookies, no storage)
            .with_back_forward_navigation_gestures(false)
            .with_initialization_script(INIT)
            .with_initialization_script(link_script("window.webkit.messageHandlers.ufb.postMessage", ""))
            .with_document_title_changed_handler(move |t| p1(format!("title:{t}")))
            .with_on_page_load_handler(move |e, _url| if matches!(e, wry::PageLoadEvent::Finished) { p2("load:finished".into()) })
            // (this is asked for every frame, and tells no frame from the page: the web is let through - it is what a
            // page embeds; a pressed link never comes here, our script has it; a file outside the folder is refused)
            .with_navigation_handler(move |url| { let ours = url.starts_with(&root) || url.starts_with("about:") || url.starts_with("http://") || url.starts_with("https://") || url.starts_with("data:") || url.starts_with("blob:"); if !ours { p3(format!("blocked:{url}")); } ours })
            .with_new_window_req_handler(move |url, _f| { p4(if outward(&url) { format!("open:{url}") } else { format!("blocked:{url}") }); wry::NewWindowResponse::Deny })
            .build_as_child(window).map_err(|e| e.to_string())?;
        let controller = view.manager();
        let h: Retained<Handler> = unsafe { msg_send![super(Handler::alloc(mtm).set_ivars(Ivars { told: Box::new(push.clone()) })), init] };
        #[allow(unused_unsafe)] unsafe { controller.addScriptMessageHandler_name(ProtocolObject::from_ref(&*h), &NSString::from_str("ufb")) };
        // the page, with read access to its own folder alone
        let wk = view.webview(); let wk: Retained<AnyObject> = unsafe { Retained::cast_unchecked(wk) };
        let file = NSURL::fileURLWithPath(&NSString::from_str(&page.to_string_lossy()));
        let dir = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(&folder.to_string_lossy()), true);
        let nav: Option<Retained<AnyObject>> = unsafe { msg_send![&*wk, loadFileURL: &*file, allowingReadAccessToURL: &*dir] };
        if nav.is_none() { return Err("the page could not be loaded".into()) }
        Ok(Web { view, msgs, rect: std::cell::Cell::new((rect, scale)) })
    }
    impl Web {
        /// The view follows the rectangle (physical px) - asked at every redraw, done when it changed.
        pub fn set_rect(&self, r: Rect, s: f64) { if self.rect.get() != (r, s) { self.rect.set((r, s)); let _ = self.view.set_bounds(bounds(r, s)); } }
        /// The keys go back to our own view (before the web view is dropped; when the pointer is on our controls).
        pub fn focus_parent(&self) { let _ = self.view.focus_parent(); }
        pub fn eval(&self, js: &str) { let _ = self.view.evaluate_script(js); }
    }
    impl Drop for Web { fn drop(&mut self) { let _ = self.view.focus_parent(); } }

    /// The window's number, for a picture of it taken by the system (a scripted look: our own capture has no page in it).
    pub fn window_number(window: &Win) -> isize {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        match window.window_handle().map(|h| h.as_raw()) { Ok(RawWindowHandle::AppKit(h)) => { let v: &objc2_app_kit::NSView = unsafe { &*(h.ns_view.as_ptr() as *const objc2_app_kit::NSView) }; v.window().map(|w| w.windowNumber()).unwrap_or(0) } _ => 0 }
    }
}

/// Windows: WebView2 through `wry` (S15's Windows code, carried). The page's folder is served under a made-up https
/// host - the page has a real origin, reads its own folder and names nothing else on the disk. The web is open to
/// what the page embeds (row 512); the page itself never leaves its folder: a navigation of the page to the web is
/// refused and handed to the system's browser. WebView2 keeps a profile folder: it lies in our cache folder (beside
/// the executable, its default, is not writable for an installed program); the view is private, so nothing of a
/// page stays in it.
#[cfg(windows)]
mod imp {
    use super::*;
    use webview2_com::Microsoft::Web::WebView2::Win32::{ICoreWebView2_3, COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_DENY_CORS};
    use windows::core::{Interface, HSTRING, PCWSTR};
    use wry::dpi::{LogicalPosition, LogicalSize}; use wry::WebViewExtWindows;

    const HOST: &str = "ufb.preview";
    /// Escape is told to the host (the page has the keys once it was clicked).
    const INIT: &str = r#"window.addEventListener('keydown', e => { if (e.key === 'Escape') { if (window.ipc) window.ipc.postMessage('key:Escape'); e.preventDefault(); e.stopPropagation(); } }, true);"#;

    pub struct Web { view: wry::WebView, pub msgs: Msgs, rect: std::cell::Cell<(Rect, f64)>, _ctx: wry::WebContext }
    fn bounds(r: Rect, s: f64) -> wry::Rect { wry::Rect { position: LogicalPosition::new(r.x0 / s, r.y0 / s).into(), size: LogicalSize::new(r.width() / s, r.height() / s).into() } }
    /// A file's name as an address's last part: what an address cannot hold as it is, is written in percent form.
    fn encoded(name: &str) -> String { let mut o = String::new(); for b in name.bytes() { if b.is_ascii_alphanumeric() || b"-._~".contains(&b) { o.push(b as char) } else { o.push_str(&format!("%{b:02X}")) } } o }

    pub fn create(window: &Win, rect: Rect, scale: f64, page: &Path, wake: std::sync::Arc<dyn Fn() + Send + Sync>) -> Result<Web, String> {
        // (no runtime on this machine - Windows 10 without Edge's updates: said, and the file is shown as text)
        wry::webview_version().map_err(|e| format!("the system has no web view: {e}"))?;
        let folder = page.parent().ok_or("the page has no folder")?.to_path_buf();
        let name = page.file_name().ok_or("the page has no name")?.to_string_lossy().into_owned();
        let msgs: Msgs = Default::default();
        let push = { let (m, w) = (msgs.clone(), wake.clone()); move |s: String| { m.borrow_mut().push(s); w(); } };
        let (p0, p1, p2, p3, p4) = (push.clone(), push.clone(), push.clone(), push.clone(), push.clone());
        let data = crate::thumbs::cache_dir().unwrap_or_else(|| std::env::temp_dir().join(crate::identity::data_name())).join("webview2");
        let _ = std::fs::create_dir_all(&data);
        let mut ctx = wry::WebContext::new(Some(data));
        let root = format!("https://{HOST}/");
        let view = { let root = root.clone(); wry::WebViewBuilder::new_with_web_context(&mut ctx)
            .with_bounds(bounds(rect, scale))
            .with_focused(false)
            .with_incognito(true)
            .with_initialization_script(INIT)
            .with_initialization_script(link_script("window.ipc.postMessage", HOST))
            .with_ipc_handler(move |req| p0(req.body().clone()))
            .with_document_title_changed_handler(move |t| p1(format!("title:{t}")))
            .with_on_page_load_handler(move |e, _url| if matches!(e, wry::PageLoadEvent::Finished) { p2("load:finished".into()) })
            // (asked for the page itself, not for its frames: the page stays in its folder, the web goes to the browser)
            .with_navigation_handler(move |url| { let ours = url.starts_with(&root) || url.starts_with("about:"); if !ours { p3(if outward(&url) { format!("open:{url}") } else { format!("blocked:{url}") }); } ours })
            .with_new_window_req_handler(move |url, _f| { p4(if outward(&url) { format!("open:{url}") } else { format!("blocked:{url}") }); wry::NewWindowResponse::Deny })
            .build_as_child(window).map_err(|e| e.to_string())? };
        let core = view.webview();
        unsafe {
            let core3: ICoreWebView2_3 = core.cast().map_err(|e| format!("the folder mapping: {e}"))?;
            let (host, dir) = (HSTRING::from(HOST), HSTRING::from(folder.as_os_str()));
            core3.SetVirtualHostNameToFolderMapping(PCWSTR(host.as_ptr()), PCWSTR(dir.as_ptr()), COREWEBVIEW2_HOST_RESOURCE_ACCESS_KIND_DENY_CORS).map_err(|e| format!("the folder mapping: {e}"))?;
        }
        view.load_url(&format!("{root}{}", encoded(&name))).map_err(|e| e.to_string())?;
        Ok(Web { view, msgs, rect: std::cell::Cell::new((rect, scale)), _ctx: ctx })
    }
    impl Web {
        pub fn set_rect(&self, r: Rect, s: f64) { if self.rect.get() != (r, s) { self.rect.set((r, s)); let _ = self.view.set_bounds(bounds(r, s)); } }
        pub fn focus_parent(&self) { let _ = self.view.focus_parent(); }
        pub fn eval(&self, js: &str) { let _ = self.view.evaluate_script(js); }
    }
    impl Drop for Web { fn drop(&mut self) { let _ = self.view.focus_parent(); } }
    pub fn window_number(_: &Win) -> isize { 0 }
}

#[cfg(not(any(target_os = "macos", windows)))]
mod imp {
    use super::*;
    pub struct Web { pub msgs: Msgs }
    pub fn create(_: &Win, _: Rect, _: f64, _: &Path, _: std::sync::Arc<dyn Fn() + Send + Sync>) -> Result<Web, String> { Err("the system's web view is not built in on this system".into()) }
    impl Web { pub fn set_rect(&self, _: Rect, _: f64) {} pub fn focus_parent(&self) {} pub fn eval(&self, _: &str) {} }
    pub fn window_number(_: &Win) -> isize { 0 }
}
pub use imp::{create, window_number, Web};

/// Files the web view shows.
pub fn is_page(ext: &str) -> bool { matches!(ext, "html" | "htm" | "xhtml") }
