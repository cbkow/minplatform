//! macOS: the Sparkle updater (canon row 348) - QCView's `sparkle.rs` as it is (canon row 248): one
//! `SPUStandardUpdaterController` started with the application (it reads SUFeedURL / SUPublicEDKey / the check interval
//! from Info.plist and schedules the daily check) and minNotes > Check for Updates. minNotes 3 replaces 2.x in place and
//! keeps 2.x's feed and key, so a 2.x installation updates into it and goes on being offered updates.
//! Sparkle.framework is LOADED at run time from the bundle's Frameworks, not linked: a run outside the bundle simply has
//! no updater. A scripted run (`MN_NO_DIALOGS`) never starts it: it would ask the feed and may put up a window.
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2::{msg_send, MainThreadMarker};
use objc2_foundation::{NSBundle, NSString};
use std::cell::RefCell;

thread_local! { static CONTROLLER: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) }; }

/// Start the updater once (main thread). True when it runs.
pub fn start() -> bool {
    if CONTROLLER.with_borrow(|c| c.is_some()) { return true }
    let Some(_mtm) = MainThreadMarker::new() else { return false };
    let Some(dir) = NSBundle::mainBundle().privateFrameworksPath() else { return false };
    let path = NSString::from_str(&format!("{dir}/Sparkle.framework"));
    let Some(fw) = NSBundle::bundleWithPath(&path) else { log::info!("Sparkle: no framework in this bundle — no updater"); return false };
    if !unsafe { fw.load() } { log::warn!("Sparkle: the framework did not load"); return false }
    let Some(cls) = AnyClass::get(c"SPUStandardUpdaterController") else { log::warn!("Sparkle: no SPUStandardUpdaterController"); return false };
    let none: *const AnyObject = std::ptr::null();
    let ctl: Option<Retained<AnyObject>> = unsafe {
        let alloc: *mut AnyObject = msg_send![cls, alloc];
        let obj: *mut AnyObject = msg_send![alloc, initWithStartingUpdater: Bool::YES, updaterDelegate: none, userDriverDelegate: none];
        Retained::from_raw(obj)   // +1 from init
    };
    let Some(ctl) = ctl else { return false };
    CONTROLLER.with_borrow_mut(|c| *c = Some(ctl));
    log::info!("Sparkle: updater started (automatic checks {})", automatic());
    true
}
fn updater() -> Option<Retained<AnyObject>> { CONTROLLER.with_borrow(|c| c.as_ref().and_then(|ctl| unsafe { let u: *mut AnyObject = msg_send![&**ctl, updater]; Retained::retain(u) })) }
/// About ▸ Check for Updates… (Sparkle shows its own window).
pub fn check_now() {
    if !start() { log::info!("Sparkle: Check for Updates — no updater in this build"); return }
    let none: *const AnyObject = std::ptr::null();
    CONTROLLER.with_borrow(|c| if let Some(ctl) = c { let _: () = unsafe { msg_send![&**ctl, checkForUpdates: none] }; });
}
pub fn automatic() -> bool { updater().map(|u| { let b: Bool = unsafe { msg_send![&*u, automaticallyChecksForUpdates] }; b.as_bool() }).unwrap_or(false) }
