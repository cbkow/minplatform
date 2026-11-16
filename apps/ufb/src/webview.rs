//! An HTML file in the lightbox: the SYSTEM's own web view as a child of our window (canon rows 453, 479; carried from
//! spike S15 - "Native webview or nothing"). macOS: WKWebView through `wry`, with the spike's two pieces of our own
//! (the confined file load, our own message handler). Windows: not yet - S15 never ran there; an HTML file is shown as
//! its source text until it has.
//!
//! The rules (S15): the view is made unfocused; the page is loaded with read access to ITS OWN FOLDER only; nothing is
//! fetched from the network (a content rule list, in place BEFORE the page loads); a navigation out of the folder and
//! a new window are refused; Escape pressed inside the page is told to us by a script of ours; the keys are given back
//! to our view before the web view is dropped; nothing of ours can be drawn over it - it is a system view above our
//! surface, so the lightbox keeps its own controls outside the view's rectangle.
use kurbo::Rect;
use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

pub type Win = std::sync::Arc<Box<dyn winit31::window::Window>>;
/// What the page told: `key:Escape`, `title:...`, `load:finished`, `blocked:<address>`.
pub type Msgs = Rc<RefCell<Vec<String>>>;

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use objc2::rc::Retained; use objc2::runtime::{AnyObject, NSObject, ProtocolObject}; use objc2::{define_class, msg_send, DeclaredClass, MainThreadMarker, MainThreadOnly};
    use objc2_foundation::{NSError, NSObjectProtocol, NSString, NSURL}; use objc2_web_kit::{WKContentRuleList, WKContentRuleListStore, WKScriptMessage, WKScriptMessageHandler, WKUserContentController};
    use wry::dpi::{LogicalPosition, LogicalSize}; use wry::WebViewExtMacOS;

    /// Our script, put in every page before the page's own: Escape is told to the host (the page has the keys once it
    /// was clicked). Through a message handler of OUR OWN: the library's channel drops what a file page sends.
    const INIT: &str = r#"window.addEventListener('keydown', e => { if (e.key === 'Escape') { const h = window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.ufb; if (h) h.postMessage('key:Escape'); e.preventDefault(); e.stopPropagation(); } }, true);"#;
    /// Nothing is fetched from the network: every http(s) and socket address is blocked for the page.
    const RULES: &str = r#"[{"trigger":{"url-filter":"^https?://.*"},"action":{"type":"block"}},{"trigger":{"url-filter":"^wss?://.*"},"action":{"type":"block"}},{"trigger":{"url-filter":"^ftp://.*"},"action":{"type":"block"}}]"#;

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
            .with_document_title_changed_handler(move |t| p1(format!("title:{t}")))
            .with_on_page_load_handler(move |e, _url| if matches!(e, wry::PageLoadEvent::Finished) { p2("load:finished".into()) })
            .with_navigation_handler(move |url| { let ours = url.starts_with(&root) || url.starts_with("about:"); if !ours { p3(format!("blocked:{url}")); } ours })
            .with_new_window_req_handler(move |url, _f| { p4(format!("blocked:{url}")); wry::NewWindowResponse::Deny })
            .build_as_child(window).map_err(|e| e.to_string())?;
        let controller = view.manager();
        let h: Retained<Handler> = unsafe { msg_send![super(Handler::alloc(mtm).set_ivars(Ivars { told: Box::new(push.clone()) })), init] };
        #[allow(unused_unsafe)] unsafe { controller.addScriptMessageHandler_name(ProtocolObject::from_ref(&*h), &NSString::from_str("ufb")) };
        // The page is loaded only once the rule that keeps it off the network is in place (the rule is compiled by the
        // system, which answers on this thread a moment later). Should the rule fail, the page is NOT loaded.
        let wk = view.webview(); let wk: Retained<AnyObject> = unsafe { Retained::cast_unchecked(wk) };
        let file = NSURL::fileURLWithPath(&NSString::from_str(&page.to_string_lossy()));
        let dir = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(&folder.to_string_lossy()), true);
        let fail = push.clone();
        let done = block2::RcBlock::new(move |list: *mut WKContentRuleList, _err: *mut NSError| {
            let Some(list) = (unsafe { list.as_ref() }) else { fail("failed:the network rule could not be made".into()); return };
            #[allow(unused_unsafe)] unsafe { controller.addContentRuleList(list); }
            let nav: Option<Retained<AnyObject>> = unsafe { msg_send![&*wk, loadFileURL: &*file, allowingReadAccessToURL: &*dir] };
            if nav.is_none() { fail("failed:the page could not be loaded".into()); }
        });
        let store = unsafe { WKContentRuleListStore::defaultStore(mtm) }.ok_or("no content rule store")?;
        unsafe { store.compileContentRuleListForIdentifier_encodedContentRuleList_completionHandler(Some(&NSString::from_str("ufb-preview-offline")), Some(&NSString::from_str(RULES)), Some(&done)) };
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

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::*;
    pub struct Web { pub msgs: Msgs }
    pub fn create(_: &Win, _: Rect, _: f64, _: &Path, _: std::sync::Arc<dyn Fn() + Send + Sync>) -> Result<Web, String> { Err("the system's web view is not built in on this system yet".into()) }
    impl Web { pub fn set_rect(&self, _: Rect, _: f64) {} pub fn focus_parent(&self) {} pub fn eval(&self, _: &str) {} }
    pub fn window_number(_: &Win) -> isize { 0 }
}
pub use imp::{create, window_number, Web};

/// Files the web view shows.
pub fn is_page(ext: &str) -> bool { matches!(ext, "html" | "htm" | "xhtml") }
