//! macOS: what the Finder and `minnotes:` addresses ask the application to open (canon row 352) - QCView's `mac_open.rs`
//! (row 248) as it is. Documents and addresses arrive as Apple events on macOS, never as arguments; winit registers no
//! application delegate, so ours implements `application:openURLs:` - files as paths, anything else as the address's
//! text - and keeps each batch for the window to take up at its next turn.
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSApplicationDelegate};
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSURL};
use std::sync::Mutex;

static ASKED: Mutex<Vec<String>> = Mutex::new(Vec::new());

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "MinNotesAppDelegate"]
    struct AppDelegate;

    unsafe impl NSObjectProtocol for AppDelegate {}

    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(application:openURLs:))]
        fn open_urls(&self, _app: &NSApplication, urls: &NSArray<NSURL>) {
            let items: Vec<String> = urls.iter().filter_map(|u| {
                if u.isFileURL() { u.path().map(|p| p.to_string()) } else { u.absoluteString().map(|s| s.to_string()) }
            }).collect();
            if let Some(f) = std::env::var_os("MN_OPEN_LOG") { use std::io::Write; if let Ok(mut o) = std::fs::OpenOptions::new().create(true).append(true).open(f) { let _ = writeln!(o, "delegate: {} item(s)", items.len()); } }
            if items.is_empty() { return }
            ASKED.lock().unwrap().extend(items);
            if let Some(w) = crate::WAKE.get() { w.wake_up(); }
        }
    }
);

/// Install the delegate (after `EventLoop::new`, as winit asks). It lives for the process: `NSApplication` holds its
/// delegate weakly.
pub fn install() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let delegate: Retained<AppDelegate> = unsafe { msg_send![super(AppDelegate::alloc(mtm).set_ivars(())), init] };
    NSApplication::sharedApplication(mtm).setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    std::mem::forget(delegate);
    if let Some(f) = std::env::var_os("MN_OPEN_LOG") { use std::io::Write; if let Ok(mut o) = std::fs::OpenOptions::new().create(true).append(true).open(f) { let _ = writeln!(o, "delegate installed"); } }
}
/// What was asked for since the last call.
pub fn take() -> Vec<String> { std::mem::take(&mut *ASKED.lock().unwrap()) }
