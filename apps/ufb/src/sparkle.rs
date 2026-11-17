//! macOS: the Sparkle updater (canon rows 457, 519 point 8, 536) - QCView's and minNotes' `sparkle.rs` as it is: one
//! `SPUStandardUpdaterController` started with the application (it reads SUFeedURL / SUPublicEDKey / the check interval
//! from Info.plist and schedules the daily check) and Settings > About > Check for Updates. The RELEASE alone has it:
//! it replaces 1.4.x in place and keeps its feed and key, so an installed 1.4.x updates into it and goes on being
//! offered updates. Sparkle.framework is LOADED at run time from the bundle's Frameworks, not linked: a run outside the
//! bundle, and the test package, simply have no updater. A scripted look never starts it.
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
    let Some(fw) = NSBundle::bundleWithPath(&path) else { eprintln!("Sparkle: no framework in this bundle — no updater"); return false };
    if !unsafe { fw.load() } { eprintln!("Sparkle: the framework did not load"); return false }
    let Some(cls) = AnyClass::get(c"SPUStandardUpdaterController") else { eprintln!("Sparkle: no SPUStandardUpdaterController"); return false };
    let none: *const AnyObject = std::ptr::null();
    let ctl: Option<Retained<AnyObject>> = unsafe {
        let alloc: *mut AnyObject = msg_send![cls, alloc];
        let obj: *mut AnyObject = msg_send![alloc, initWithStartingUpdater: Bool::YES, updaterDelegate: none, userDriverDelegate: none];
        Retained::from_raw(obj)   // +1 from init
    };
    let Some(ctl) = ctl else { return false };
    CONTROLLER.with_borrow_mut(|c| *c = Some(ctl));
    eprintln!("Sparkle: updater started (automatic checks {})", automatic());
    true
}
fn updater() -> Option<Retained<AnyObject>> { CONTROLLER.with_borrow(|c| c.as_ref().and_then(|ctl| unsafe { let u: *mut AnyObject = msg_send![&**ctl, updater]; Retained::retain(u) })) }
/// About ▸ Check for Updates… (Sparkle shows its own window).
pub fn check_now() {
    if !start() { eprintln!("Sparkle: Check for Updates — no updater in this build"); return }
    let none: *const AnyObject = std::ptr::null();
    CONTROLLER.with_borrow(|c| if let Some(ctl) = c { let _: () = unsafe { msg_send![&**ctl, checkForUpdates: none] }; });
}
pub fn automatic() -> bool { updater().map(|u| { let b: Bool = unsafe { msg_send![&*u, automaticallyChecksForUpdates] }; b.as_bool() }).unwrap_or(false) }
