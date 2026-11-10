//! macOS: what the Finder and `qcview://` links ask the app to open (canon row 248). QCView got these as Qt's
//! `QFileOpenEvent`s (documents and URLs arrive as Apple events on macOS, never as arguments); winit registers no
//! application delegate, so ours implements `application:openURLs:` — files as paths, anything else as the URL's text —
//! and hands each batch to the shell, which treats it exactly as a second launch's arguments (`open_forwarded`).
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSApplicationDelegate};
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSURL};
use std::sync::{mpsc, Mutex, OnceLock};

static TX: OnceLock<Mutex<mpsc::Sender<Vec<String>>>> = OnceLock::new();

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "QCViewAppDelegate"]
    struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(application:openURLs:))]
        fn open_urls(&self, _app: &NSApplication, urls: &NSArray<NSURL>) {
            let items: Vec<String> = urls.iter().filter_map(|u| {
                if u.isFileURL() { u.path().map(|p| p.to_string()) } else { u.absoluteString().map(|s| s.to_string()) }
            }).collect();
            if items.is_empty() { return }
            if let Some(tx) = TX.get() { let _ = tx.lock().unwrap().send(items); }
        }
    }
);

/// Install the delegate (after `EventLoop::new`, as winit asks) and return the receiver the shell drains each tick.
/// The delegate lives for the process: `NSApplication` holds its delegate weakly.
pub fn install() -> mpsc::Receiver<Vec<String>> {
    let (tx, rx) = mpsc::channel();
    let _ = TX.set(Mutex::new(tx));
    let mtm = MainThreadMarker::new().expect("main thread");
    let delegate: Retained<AppDelegate> = unsafe { msg_send![super(AppDelegate::alloc(mtm).set_ivars(())), init] };
    NSApplication::sharedApplication(mtm).setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    std::mem::forget(delegate);
    rx
}
